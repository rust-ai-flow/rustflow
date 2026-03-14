use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use rustflow_core::context::Context;
use rustflow_core::step::{Step, StepState};
use rustflow_core::types::{StepId, Value};

use crate::dag::DagParser;
use crate::error::{OrchestratorError, Result};

/// Trait that the scheduler calls to actually run a single step.
///
/// Users wire in their LLM / tool executors by implementing this trait.
#[async_trait]
pub trait StepExecutor: Send + Sync + 'static {
    async fn execute(&self, step: &Step, ctx: &Context) -> std::result::Result<Value, String>;
}

/// Events emitted by the scheduler during execution.
#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    /// A step has started executing.
    StepStarted {
        step_id: String,
        step_name: String,
    },
    /// A step completed successfully.
    StepSucceeded {
        step_id: String,
        step_name: String,
        elapsed: Duration,
    },
    /// A step failed (may be retried).
    StepFailed {
        step_id: String,
        step_name: String,
        error: String,
        will_retry: bool,
        attempt: u32,
        elapsed: Duration,
    },
    /// A step is being retried.
    StepRetrying {
        step_id: String,
        step_name: String,
        attempt: u32,
    },
}

/// Tracks mutable per-step runtime state.
#[derive(Debug, Clone, Default)]
struct StepStatus {
    state: StepState,
    attempts: u32,
}

/// Executes an agent's steps in DAG order, honouring dependency constraints.
///
/// Steps whose dependencies are all satisfied are launched concurrently via
/// `tokio`. The scheduler polls for newly ready steps each time any step
/// finishes.
pub struct Scheduler {
    executor: Arc<dyn StepExecutor>,
}

impl Scheduler {
    pub fn new(executor: Arc<dyn StepExecutor>) -> Self {
        Self { executor }
    }

    /// Run all steps, returning the final `Context` on success.
    pub async fn run(&self, steps: &[Step], ctx: Context) -> Result<Context> {
        self.run_with_events(steps, ctx, |_| {}).await
    }

    /// Run all steps with an event callback for progress reporting.
    ///
    /// Returns the final `Context` and a map of step durations.
    pub async fn run_with_events<F>(
        &self,
        steps: &[Step],
        ctx: Context,
        mut on_event: F,
    ) -> Result<Context>
    where
        F: FnMut(SchedulerEvent),
    {
        // Validate the DAG up front (detects cycles, duplicate IDs, unknown deps).
        DagParser::parse(steps)?;

        let dep_map: HashMap<String, HashSet<String>> = DagParser::build_dependency_map(steps);

        // Clone step data so tasks can own their copies.
        let steps_by_id: HashMap<String, Step> = steps
            .iter()
            .map(|s| (s.id.as_str().to_string(), s.clone()))
            .collect();

        let statuses: Arc<Mutex<HashMap<String, StepStatus>>> = Arc::new(Mutex::new(
            steps
                .iter()
                .map(|s| (s.id.as_str().to_string(), StepStatus::default()))
                .collect(),
        ));
        let ctx_shared: Arc<Mutex<Context>> = Arc::new(Mutex::new(ctx));

        // Track per-step start times for duration reporting.
        let step_starts: Arc<Mutex<HashMap<String, Instant>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let mut join_set: JoinSet<(String, std::result::Result<Value, String>, Duration)> =
            JoinSet::new();
        let mut in_flight: HashSet<String> = HashSet::new();

        loop {
            // ── 1. Launch every step that is newly ready ──────────────────────
            let ready: Vec<String> = {
                let sg = statuses.lock().await;
                let completed: HashSet<&str> = sg
                    .iter()
                    .filter(|(_, s)| s.state == StepState::Success)
                    .map(|(id, _)| id.as_str())
                    .collect();
                let failed_any = sg.values().any(|s| s.state == StepState::Failed);

                if failed_any {
                    let failed_id = sg
                        .iter()
                        .find(|(_, s)| s.state == StepState::Failed)
                        .map(|(id, _)| id.clone())
                        .unwrap_or_default();
                    return Err(OrchestratorError::StepFailed {
                        step_id: failed_id,
                        reason: "step failed after all retries".to_string(),
                    });
                }

                steps_by_id
                    .keys()
                    .filter(|id| {
                        let status = &sg[*id];
                        status.state == StepState::Pending && !in_flight.contains(*id)
                    })
                    .filter(|id| dep_map[*id].iter().all(|d| completed.contains(d.as_str())))
                    .cloned()
                    .collect()
            };

            for step_id in ready {
                // Mark as running.
                statuses.lock().await.get_mut(&step_id).unwrap().state = StepState::Running;

                in_flight.insert(step_id.clone());

                let step_clone = steps_by_id[&step_id].clone();
                let executor = Arc::clone(&self.executor);
                let ctx_clone = ctx_shared.lock().await.clone();
                let sid = step_id.clone();
                info!(step_id = %sid, "launching step");

                on_event(SchedulerEvent::StepStarted {
                    step_id: sid.clone(),
                    step_name: step_clone.name.clone(),
                });

                let start = Instant::now();
                step_starts
                    .lock()
                    .await
                    .insert(sid.clone(), start);

                join_set.spawn(async move {
                    let result = executor.execute(&step_clone, &ctx_clone).await;
                    let elapsed = start.elapsed();
                    (sid, result, elapsed)
                });
            }

            // ── 2. Check termination condition ────────────────────────────────
            let all_done = {
                let sg = statuses.lock().await;
                sg.values()
                    .all(|s| s.state == StepState::Success || s.state == StepState::Failed)
            };
            if all_done && join_set.is_empty() {
                break;
            }

            // ── 3. Wait for the next step to finish ───────────────────────────
            if join_set.is_empty() {
                // Nothing is running yet (e.g., everything is Retrying); yield.
                tokio::task::yield_now().await;
                continue;
            }

            match join_set.join_next().await {
                Some(Ok((step_id, Ok(value), elapsed))) => {
                    info!(step_id = %step_id, "step succeeded");
                    let step_name = steps_by_id[&step_id].name.clone();
                    on_event(SchedulerEvent::StepSucceeded {
                        step_id: step_id.clone(),
                        step_name,
                        elapsed,
                    });
                    let sid = StepId::new(&step_id);
                    ctx_shared.lock().await.set_step_output(&sid, value);
                    statuses.lock().await.get_mut(&step_id).unwrap().state = StepState::Success;
                    in_flight.remove(&step_id);
                }
                Some(Ok((step_id, Err(err_msg), elapsed))) => {
                    warn!(step_id = %step_id, error = %err_msg, "step failed");
                    let step = &steps_by_id[&step_id];
                    let (should_retry, delay, attempt) = {
                        let mut sg = statuses.lock().await;
                        let status = sg.get_mut(&step_id).unwrap();
                        status.attempts += 1;
                        let attempt = status.attempts;
                        let retry = status.attempts <= step.retry_policy.max_retries();
                        let delay = if retry {
                            step.retry_policy.backoff(status.attempts - 1)
                        } else {
                            Duration::ZERO
                        };
                        status.state = if retry {
                            StepState::Retrying
                        } else {
                            StepState::Failed
                        };
                        (retry, delay, attempt)
                    };

                    on_event(SchedulerEvent::StepFailed {
                        step_id: step_id.clone(),
                        step_name: step.name.clone(),
                        error: err_msg,
                        will_retry: should_retry,
                        attempt,
                        elapsed,
                    });

                    in_flight.remove(&step_id);

                    if should_retry {
                        on_event(SchedulerEvent::StepRetrying {
                            step_id: step_id.clone(),
                            step_name: step.name.clone(),
                            attempt: attempt + 1,
                        });

                        if delay > Duration::ZERO {
                            tokio::time::sleep(delay).await;
                        }
                        // Re-queue.
                        statuses.lock().await.get_mut(&step_id).unwrap().state = StepState::Pending;
                    } else {
                        error!(step_id = %step_id, "step exhausted retries");
                    }
                }
                Some(Err(join_err)) => {
                    error!("task panicked: {join_err}");
                    return Err(OrchestratorError::StepFailed {
                        step_id: "unknown".to_string(),
                        reason: join_err.to_string(),
                    });
                }
                None => {
                    // JoinSet is exhausted; loop again to recheck state.
                }
            }
        }

        let ctx = ctx_shared.lock().await.clone();
        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflow_core::retry::RetryPolicy;
    use rustflow_core::step::Step;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A mock executor that always succeeds, echoing the step ID.
    struct SuccessExecutor;

    #[async_trait]
    impl StepExecutor for SuccessExecutor {
        async fn execute(&self, step: &Step, _ctx: &Context) -> std::result::Result<Value, String> {
            Ok(Value::from(
                serde_json::json!(format!("{}-done", step.id.as_str())),
            ))
        }
    }

    /// A mock executor that always fails.
    struct FailExecutor;

    #[async_trait]
    impl StepExecutor for FailExecutor {
        async fn execute(&self, _step: &Step, _ctx: &Context) -> std::result::Result<Value, String> {
            Err("boom".to_string())
        }
    }

    /// A mock executor that fails N times then succeeds.
    struct FailThenSucceedExecutor {
        fail_count: AtomicU32,
        fail_until: u32,
    }

    impl FailThenSucceedExecutor {
        fn new(fail_until: u32) -> Self {
            Self {
                fail_count: AtomicU32::new(0),
                fail_until,
            }
        }
    }

    #[async_trait]
    impl StepExecutor for FailThenSucceedExecutor {
        async fn execute(&self, _step: &Step, _ctx: &Context) -> std::result::Result<Value, String> {
            let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if count < self.fail_until {
                Err("transient error".to_string())
            } else {
                Ok(Value::from(serde_json::json!("recovered")))
            }
        }
    }

    fn tool_step(id: &str, deps: Vec<&str>) -> Step {
        Step::new_tool(id, id, "noop", serde_json::Value::Null)
            .with_depends_on(deps.into_iter().map(StepId::from).collect())
    }

    #[tokio::test]
    async fn test_scheduler_single_step() {
        let scheduler = Scheduler::new(Arc::new(SuccessExecutor));
        let steps = vec![tool_step("a", vec![])];
        let ctx = scheduler.run(&steps, Context::new()).await.unwrap();
        let output = ctx.get_step_output(&StepId::new("a")).unwrap();
        assert_eq!(output.as_str(), Some("a-done"));
    }

    #[tokio::test]
    async fn test_scheduler_linear_chain() {
        let scheduler = Scheduler::new(Arc::new(SuccessExecutor));
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec!["a"]),
            tool_step("c", vec!["b"]),
        ];
        let ctx = scheduler.run(&steps, Context::new()).await.unwrap();
        assert!(ctx.get_step_output(&StepId::new("a")).is_some());
        assert!(ctx.get_step_output(&StepId::new("b")).is_some());
        assert!(ctx.get_step_output(&StepId::new("c")).is_some());
    }

    #[tokio::test]
    async fn test_scheduler_parallel_steps() {
        let scheduler = Scheduler::new(Arc::new(SuccessExecutor));
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec![]),
            tool_step("c", vec!["a", "b"]),
        ];
        let ctx = scheduler.run(&steps, Context::new()).await.unwrap();
        assert!(ctx.get_step_output(&StepId::new("c")).is_some());
    }

    #[tokio::test]
    async fn test_scheduler_step_failure() {
        let scheduler = Scheduler::new(Arc::new(FailExecutor));
        let steps = vec![tool_step("a", vec![])];
        let err = scheduler.run(&steps, Context::new()).await.unwrap_err();
        assert!(matches!(err, OrchestratorError::StepFailed { .. }));
    }

    #[tokio::test]
    async fn test_scheduler_retry_then_succeed() {
        let executor = FailThenSucceedExecutor::new(2); // fail twice, then succeed
        let scheduler = Scheduler::new(Arc::new(executor));
        let steps = vec![
            Step::new_tool("a", "a", "noop", serde_json::Value::Null)
                .with_retry(RetryPolicy::Fixed {
                    max_retries: 3,
                    interval_ms: 0,
                }),
        ];
        let ctx = scheduler.run(&steps, Context::new()).await.unwrap();
        let output = ctx.get_step_output(&StepId::new("a")).unwrap();
        assert_eq!(output.as_str(), Some("recovered"));
    }

    #[tokio::test]
    async fn test_scheduler_retry_exhausted() {
        let executor = FailThenSucceedExecutor::new(10); // always fail within 2 retries
        let scheduler = Scheduler::new(Arc::new(executor));
        let steps = vec![
            Step::new_tool("a", "a", "noop", serde_json::Value::Null)
                .with_retry(RetryPolicy::Fixed {
                    max_retries: 2,
                    interval_ms: 0,
                }),
        ];
        let err = scheduler.run(&steps, Context::new()).await.unwrap_err();
        assert!(matches!(err, OrchestratorError::StepFailed { .. }));
    }

    #[tokio::test]
    async fn test_scheduler_rejects_cycle() {
        let scheduler = Scheduler::new(Arc::new(SuccessExecutor));
        let steps = vec![
            tool_step("a", vec!["b"]),
            tool_step("b", vec!["a"]),
        ];
        let err = scheduler.run(&steps, Context::new()).await.unwrap_err();
        assert!(matches!(err, OrchestratorError::CycleDetected { .. }));
    }

    #[tokio::test]
    async fn test_scheduler_empty_steps() {
        let scheduler = Scheduler::new(Arc::new(SuccessExecutor));
        let steps: Vec<Step> = vec![];
        let ctx = scheduler.run(&steps, Context::new()).await.unwrap();
        assert!(ctx.step_outputs.is_empty());
    }

    #[tokio::test]
    async fn test_scheduler_preserves_initial_context() {
        let scheduler = Scheduler::new(Arc::new(SuccessExecutor));
        let steps = vec![tool_step("a", vec![])];
        let mut initial_ctx = Context::new();
        initial_ctx.set_var("key", Value::from(serde_json::json!("value")));
        let ctx = scheduler.run(&steps, initial_ctx).await.unwrap();
        assert_eq!(ctx.get_var("key").unwrap().as_str(), Some("value"));
    }
}
