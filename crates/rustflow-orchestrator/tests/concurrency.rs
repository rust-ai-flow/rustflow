/// Concurrency stress tests for the Scheduler.
///
/// These tests verify that the scheduler can handle large numbers of
/// concurrent workflow executions — the project's key performance claim.
///
/// Run with:
///   cargo test -p rustflow-orchestrator --test concurrency --release -- --nocapture
use async_trait::async_trait;
use rustflow_core::{context::Context, step::Step, types::{StepId, Value}};
use rustflow_orchestrator::scheduler::{Scheduler, StepExecutor};
use std::sync::Arc;
use std::time::Instant;

struct NoopExecutor;

#[async_trait]
impl StepExecutor for NoopExecutor {
    async fn execute(&self, step: &Step, _ctx: &Context) -> Result<Value, String> {
        Ok(Value::from(serde_json::json!(step.id.as_str())))
    }
}

fn parallel_steps(n: usize) -> Vec<Step> {
    (0..n)
        .map(|i| Step::new_tool(format!("s{i}"), format!("s{i}"), "noop", serde_json::Value::Null))
        .collect()
}

fn linear_steps(n: usize) -> Vec<Step> {
    (0..n)
        .map(|i| {
            let mut s = Step::new_tool(format!("s{i}"), format!("s{i}"), "noop", serde_json::Value::Null);
            if i > 0 {
                s.depends_on = vec![StepId::new(format!("s{}", i - 1))];
            }
            s
        })
        .collect()
}

/// Spawns `agent_count` schedulers concurrently, each executing a small
/// parallel workflow. Asserts they all complete without error.
async fn run_concurrent_agents(agent_count: usize, steps_per_agent: usize) -> std::time::Duration {
    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let steps = Arc::new(parallel_steps(steps_per_agent));

    let start = Instant::now();

    let handles: Vec<_> = (0..agent_count)
        .map(|_| {
            let sched = Arc::clone(&scheduler);
            let steps = Arc::clone(&steps);
            tokio::spawn(async move { sched.run(&steps, Context::new()).await.unwrap() })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }

    start.elapsed()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_1000_agents_parallel_workflow() {
    let elapsed = run_concurrent_agents(1_000, 10).await;
    println!("1,000 agents (10 parallel steps each): {elapsed:?}");
    // Sanity bound: should complete well within 30 seconds even in debug builds.
    assert!(elapsed.as_secs() < 30, "took too long: {elapsed:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_10000_agents_single_step() {
    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let steps = Arc::new(vec![Step::new_tool("s0", "s0", "noop", serde_json::Value::Null)]);

    let start = Instant::now();
    let handles: Vec<_> = (0..10_000)
        .map(|_| {
            let sched = Arc::clone(&scheduler);
            let steps = Arc::clone(&steps);
            tokio::spawn(async move { sched.run(&steps, Context::new()).await.unwrap() })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }

    let elapsed = start.elapsed();
    println!("10,000 agents (1 step each): {elapsed:?}");
    assert!(elapsed.as_secs() < 60, "took too long: {elapsed:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_500_agents_linear_chain() {
    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let steps = Arc::new(linear_steps(20));

    let start = Instant::now();
    let handles: Vec<_> = (0..500)
        .map(|_| {
            let sched = Arc::clone(&scheduler);
            let steps = Arc::clone(&steps);
            tokio::spawn(async move { sched.run(&steps, Context::new()).await.unwrap() })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }

    let elapsed = start.elapsed();
    println!("500 agents (20-step linear chain each): {elapsed:?}");
    assert!(elapsed.as_secs() < 30, "took too long: {elapsed:?}");
}

/// Verifies outputs are correct under concurrency (no data races).
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_outputs_are_correct() {
    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let steps = Arc::new(parallel_steps(5));

    let handles: Vec<_> = (0..200)
        .map(|_| {
            let sched = Arc::clone(&scheduler);
            let steps = Arc::clone(&steps);
            tokio::spawn(async move {
                let ctx = sched.run(&steps, Context::new()).await.unwrap();
                // Every step output must be present.
                for i in 0..5 {
                    let key = StepId::new(format!("s{i}"));
                    assert!(ctx.get_step_output(&key).is_some(), "missing output for s{i}");
                }
            })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }
}
