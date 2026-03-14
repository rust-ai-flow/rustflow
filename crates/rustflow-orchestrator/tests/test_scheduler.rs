use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use rustflow_core::context::Context;
use rustflow_core::retry::RetryPolicy;
use rustflow_core::step::Step;
use rustflow_core::types::{StepId, Value};
use rustflow_orchestrator::{OrchestratorError, Scheduler, StepExecutor};

/// A mock executor that always succeeds, echoing the step ID.
struct SuccessExecutor;

#[async_trait]
impl StepExecutor for SuccessExecutor {
    async fn execute(&self, step: &Step, _ctx: &Context) -> Result<Value, String> {
        Ok(Value::from(
            serde_json::json!(format!("{}-done", step.id.as_str())),
        ))
    }
}

/// A mock executor that always fails.
struct FailExecutor;

#[async_trait]
impl StepExecutor for FailExecutor {
    async fn execute(&self, _step: &Step, _ctx: &Context) -> Result<Value, String> {
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
    async fn execute(&self, _step: &Step, _ctx: &Context) -> Result<Value, String> {
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
    let executor = FailThenSucceedExecutor::new(2);
    let scheduler = Scheduler::new(Arc::new(executor));
    let steps = vec![
        Step::new_tool("a", "a", "noop", serde_json::Value::Null).with_retry(RetryPolicy::Fixed {
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
    let executor = FailThenSucceedExecutor::new(10);
    let scheduler = Scheduler::new(Arc::new(executor));
    let steps = vec![
        Step::new_tool("a", "a", "noop", serde_json::Value::Null).with_retry(RetryPolicy::Fixed {
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
    let steps = vec![tool_step("a", vec!["b"]), tool_step("b", vec!["a"])];
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
