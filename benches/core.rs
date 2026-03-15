use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use rustflow_core::{
    agent::Agent,
    context::Context,
    step::Step,
    types::{StepId, Value},
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_steps(n: usize) -> Vec<Step> {
    (0..n)
        .map(|i| Step::new_tool(format!("s{i}"), format!("Step {i}"), "noop", serde_json::Value::Null))
        .collect()
}

// ── Agent creation ────────────────────────────────────────────────────────────

fn bench_agent_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("agent_creation");
    for n in [1, 10, 100, 1_000] {
        let steps = make_steps(n);
        group.bench_with_input(BenchmarkId::new("steps", n), &steps, |b, steps| {
            b.iter(|| {
                black_box(Agent::new("bench-agent", steps.clone()))
            });
        });
    }
    group.finish();
}

// ── Agent serde ───────────────────────────────────────────────────────────────

fn bench_agent_serde(c: &mut Criterion) {
    let mut group = c.benchmark_group("agent_serde");
    for n in [1, 10, 100] {
        let agent = Agent::new("bench-agent", make_steps(n));
        let json = serde_json::to_string(&agent).unwrap();

        group.bench_with_input(BenchmarkId::new("serialize_steps", n), &agent, |b, agent| {
            b.iter(|| black_box(serde_json::to_string(agent).unwrap()));
        });

        group.bench_with_input(BenchmarkId::new("deserialize_steps", n), &json, |b, json| {
            b.iter(|| black_box(serde_json::from_str::<Agent>(json).unwrap()));
        });
    }
    group.finish();
}

// ── Context operations ────────────────────────────────────────────────────────

fn bench_context_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("context");

    // write N step outputs
    for n in [10, 100, 1_000] {
        group.bench_with_input(BenchmarkId::new("set_step_outputs", n), &n, |b, &n| {
            b.iter(|| {
                let mut ctx = Context::new();
                for i in 0..n {
                    let sid = StepId::new(format!("s{i}"));
                    ctx.set_step_output(&sid, Value::from(serde_json::json!(i)));
                }
                black_box(ctx)
            });
        });
    }

    // read from a pre-populated context
    for n in [10, 100, 1_000] {
        let mut ctx = Context::new();
        for i in 0..n {
            ctx.set_step_output(&StepId::new(format!("s{i}")), Value::from(serde_json::json!(i)));
        }
        let last_key = StepId::new(format!("s{}", n - 1));

        group.bench_with_input(BenchmarkId::new("get_step_output", n), &(ctx, last_key), |b, (ctx, key)| {
            b.iter(|| black_box(ctx.get_step_output(key)));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_agent_creation, bench_agent_serde, bench_context_ops);
criterion_main!(benches);
