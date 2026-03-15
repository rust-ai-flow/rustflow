use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use rustflow_core::{step::Step, types::StepId};
use rustflow_orchestrator::dag::DagParser;

// ── topology builders ─────────────────────────────────────────────────────────

/// s0 → s1 → s2 → … → sN-1
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

/// N independent steps (maximum parallelism)
fn parallel_steps(n: usize) -> Vec<Step> {
    (0..n)
        .map(|i| Step::new_tool(format!("s{i}"), format!("s{i}"), "noop", serde_json::Value::Null))
        .collect()
}

/// N-wide diamond:  root → [b0..bN-1] → sink
fn diamond_steps(n: usize) -> Vec<Step> {
    let mut steps = Vec::with_capacity(n + 2);

    // root
    steps.push(Step::new_tool("root", "root", "noop", serde_json::Value::Null));

    // N parallel branches, each depending on root
    for i in 0..n {
        let mut s = Step::new_tool(format!("b{i}"), format!("b{i}"), "noop", serde_json::Value::Null);
        s.depends_on = vec![StepId::new("root")];
        steps.push(s);
    }

    // sink depends on all branches
    let mut sink = Step::new_tool("sink", "sink", "noop", serde_json::Value::Null);
    sink.depends_on = (0..n).map(|i| StepId::new(format!("b{i}"))).collect();
    steps.push(sink);

    steps
}

// ── benchmarks ────────────────────────────────────────────────────────────────

fn bench_dag_linear(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_parse/linear");
    for n in [10, 100, 1_000, 5_000] {
        let steps = linear_steps(n);
        group.bench_with_input(BenchmarkId::new("steps", n), &steps, |b, steps| {
            b.iter(|| black_box(DagParser::parse(steps).unwrap()));
        });
    }
    group.finish();
}

fn bench_dag_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_parse/parallel");
    for n in [10, 100, 1_000, 5_000] {
        let steps = parallel_steps(n);
        group.bench_with_input(BenchmarkId::new("steps", n), &steps, |b, steps| {
            b.iter(|| black_box(DagParser::parse(steps).unwrap()));
        });
    }
    group.finish();
}

fn bench_dag_diamond(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_parse/diamond");
    for n in [10, 100, 500] {
        let steps = diamond_steps(n);
        group.bench_with_input(BenchmarkId::new("branches", n), &steps, |b, steps| {
            b.iter(|| black_box(DagParser::parse(steps).unwrap()));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_dag_linear, bench_dag_parallel, bench_dag_diamond);
criterion_main!(benches);
