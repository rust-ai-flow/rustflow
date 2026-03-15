use async_trait::async_trait;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use rustflow_core::{context::Context, step::Step, types::{StepId, Value}};
use rustflow_orchestrator::scheduler::{Scheduler, StepExecutor};
use std::sync::Arc;
use tokio::runtime::Runtime;

// ── no-op executor ────────────────────────────────────────────────────────────

struct NoopExecutor;

#[async_trait]
impl StepExecutor for NoopExecutor {
    async fn execute(&self, step: &Step, _ctx: &Context) -> Result<Value, String> {
        Ok(Value::from(serde_json::json!(step.id.as_str())))
    }
}

// ── topology builders ─────────────────────────────────────────────────────────

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

fn parallel_steps(n: usize) -> Vec<Step> {
    (0..n)
        .map(|i| Step::new_tool(format!("s{i}"), format!("s{i}"), "noop", serde_json::Value::Null))
        .collect()
}

// ── benchmarks ────────────────────────────────────────────────────────────────

fn bench_scheduler_parallel(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let mut group = c.benchmark_group("scheduler/parallel_steps");

    for n in [10, 100, 500, 1_000] {
        let steps = parallel_steps(n);
        group.bench_with_input(BenchmarkId::new("steps", n), &steps, |b, steps| {
            b.to_async(&rt).iter(|| {
                let sched = Arc::clone(&scheduler);
                let steps = steps.clone();
                async move {
                    black_box(sched.run(&steps, Context::new()).await.unwrap())
                }
            });
        });
    }
    group.finish();
}

fn bench_scheduler_linear(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let mut group = c.benchmark_group("scheduler/linear_chain");

    for n in [10, 50, 100, 200] {
        let steps = linear_steps(n);
        group.bench_with_input(BenchmarkId::new("steps", n), &steps, |b, steps| {
            b.to_async(&rt).iter(|| {
                let sched = Arc::clone(&scheduler);
                let steps = steps.clone();
                async move {
                    black_box(sched.run(&steps, Context::new()).await.unwrap())
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_scheduler_parallel, bench_scheduler_linear);
criterion_main!(benches);
