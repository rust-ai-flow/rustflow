/// Heap memory footprint tests for core RustFlow types.
///
/// Uses a custom global allocator to track exact heap bytes — no platform
/// tools required, works on macOS and Linux.
///
/// All tests share a global allocator counter, so they MUST run serially
/// to avoid cross-test interference. Use:
///
///   cargo test -p rustflow-benches --test memory --release -- --nocapture --test-threads=1
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustflow_core::{agent::Agent, context::Context, step::Step, types::StepId};

// ── tracking allocator ────────────────────────────────────────────────────────

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            // subtract old size, add new size
            if new_size > layout.size() {
                ALLOCATED.fetch_add(new_size - layout.size(), Ordering::Relaxed);
            } else {
                ALLOCATED.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

// ── helpers ───────────────────────────────────────────────────────────────────

fn heap_bytes() -> usize {
    ALLOCATED.load(Ordering::Relaxed)
}

fn make_steps(n: usize) -> Vec<Step> {
    (0..n)
        .map(|i| Step::new_tool(format!("s{i}"), format!("step-{i}"), "noop", serde_json::Value::Null))
        .collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Measure heap bytes per Agent, assert < 5 MB (the project's stated target).
#[test]
fn agent_heap_footprint_under_5mb() {
    const N: usize = 1_000;
    const STEPS_PER_AGENT: usize = 10;
    const LIMIT_BYTES: usize = 5 * 1024 * 1024; // 5 MB

    // Warm up: force any lazy allocations (e.g. thread-locals, global hashmaps)
    // to settle before we take the baseline.
    let _ = Agent::new("warmup", make_steps(1));

    let baseline = heap_bytes();

    let agents: Vec<Agent> = (0..N)
        .map(|i| Agent::new(format!("agent-{i}"), make_steps(STEPS_PER_AGENT)))
        .collect();

    let total = heap_bytes() - baseline;
    let per_agent = total / N;

    println!(
        "\n  {} agents × {} steps each",
        N, STEPS_PER_AGENT
    );
    println!("  Total heap delta : {:.2} KB", total as f64 / 1024.0);
    println!("  Per-agent heap   : {:.2} KB  (limit: 5 MB)", per_agent as f64 / 1024.0);

    // Keep agents alive until after measurement.
    drop(agents);

    assert!(
        per_agent < LIMIT_BYTES,
        "per-agent heap {:.1} KB exceeds 5 MB limit",
        per_agent as f64 / 1024.0,
    );
}

/// Verify memory is freed after agents are dropped (no significant leaks).
///
/// Must run in isolation — concurrent allocations from other tests pollute
/// the global counter. Run explicitly with:
///
///   cargo test -p rustflow-benches --test memory --release -- \
///     --nocapture --test-threads=1 --ignored agent_heap_is_freed_after_drop
#[test]
#[ignore = "requires --test-threads=1; see doc comment for run instructions"]
fn agent_heap_is_freed_after_drop() {
    // Warm up
    let _ = Agent::new("warmup", make_steps(1));
    let before = heap_bytes();

    let agents: Vec<Agent> = (0..500)
        .map(|i| Agent::new(format!("a{i}"), make_steps(5)))
        .collect();

    let after_alloc = heap_bytes();
    let allocated = after_alloc.saturating_sub(before);
    assert!(allocated > 0, "agents should have allocated heap");

    drop(agents);

    let after_drop = heap_bytes();
    // How many bytes were NOT freed: (after_alloc - after_drop) is freed,
    // so retained = allocated - freed.
    let freed = after_alloc.saturating_sub(after_drop);
    let retained = allocated.saturating_sub(freed);

    println!(
        "\n  Allocated: {:.1} KB  Freed: {:.1} KB  Retained: {:.1} KB ({:.1}%)",
        allocated as f64 / 1024.0,
        freed as f64 / 1024.0,
        retained as f64 / 1024.0,
        retained as f64 / allocated as f64 * 100.0,
    );

    // Allow up to 10% retained — accounts for Vec/HashMap capacity that the
    // allocator legitimately keeps for reuse.
    let max_retained = allocated / 10;
    assert!(
        retained <= max_retained,
        "retained {:.1} KB is >{:.0}% of allocated {:.1} KB — possible leak",
        retained as f64 / 1024.0,
        10.0,
        allocated as f64 / 1024.0,
    );
}

/// Measure Context heap footprint as step outputs accumulate.
#[test]
fn context_heap_per_step_output() {
    const OUTPUTS: usize = 1_000;

    // Warm up
    let _ = Context::new();

    let baseline = heap_bytes();
    let mut ctx = Context::new();

    for i in 0..OUTPUTS {
        let sid = StepId::new(format!("s{i}"));
        ctx.set_step_output(&sid, rustflow_core::types::Value::from(serde_json::json!(i)));
    }

    let total = heap_bytes() - baseline;
    let per_output = total / OUTPUTS;

    println!(
        "\n  Context with {} step outputs",
        OUTPUTS
    );
    println!("  Total heap : {:.2} KB", total as f64 / 1024.0);
    println!("  Per output : {} B", per_output);

    drop(ctx);
}

/// Measure heap cost of a Scheduler run (orchestration overhead only).
#[test]
fn scheduler_run_heap_overhead() {
    use async_trait::async_trait;
    use rustflow_orchestrator::scheduler::{Scheduler, StepExecutor};
    use rustflow_core::types::Value;
    use std::sync::Arc;

    struct NoopExecutor;

    #[async_trait]
    impl StepExecutor for NoopExecutor {
        async fn execute(&self, step: &Step, _ctx: &Context) -> Result<Value, String> {
            Ok(Value::from(serde_json::json!(step.id.as_str())))
        }
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let scheduler = Arc::new(Scheduler::new(Arc::new(NoopExecutor)));
    let steps = make_steps(10);

    // Warm up
    rt.block_on(async {
        let _ = scheduler.run(&steps, Context::new()).await;
    });

    let baseline = heap_bytes();
    const RUNS: usize = 100;

    rt.block_on(async {
        for _ in 0..RUNS {
            let _ = scheduler.run(&steps, Context::new()).await;
        }
    });

    let total = heap_bytes().saturating_sub(baseline);
    let per_run = total / RUNS;

    println!(
        "\n  {} scheduler runs (10 parallel steps each)",
        RUNS
    );
    println!("  Net heap delta after {} runs: {} B", RUNS, total);
    println!("  Per-run overhead : {} B", per_run);
}
