use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

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

        let mut join_set: JoinSet<(String, std::result::Result<Value, String>)> = JoinSet::new();
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

                join_set.spawn(async move {
                    let result = executor.execute(&step_clone, &ctx_clone).await;
                    (sid, result)
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
                Some(Ok((step_id, Ok(value)))) => {
                    info!(step_id = %step_id, "step succeeded");
                    let sid = StepId::new(&step_id);
                    ctx_shared.lock().await.set_step_output(&sid, value);
                    statuses.lock().await.get_mut(&step_id).unwrap().state = StepState::Success;
                    in_flight.remove(&step_id);
                }
                Some(Ok((step_id, Err(err_msg)))) => {
                    warn!(step_id = %step_id, error = %err_msg, "step failed");
                    let step = &steps_by_id[&step_id];
                    let (should_retry, delay) = {
                        let mut sg = statuses.lock().await;
                        let status = sg.get_mut(&step_id).unwrap();
                        status.attempts += 1;
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
                        (retry, delay)
                    };

                    in_flight.remove(&step_id);

                    if should_retry {
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
