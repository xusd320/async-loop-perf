use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

/*
================================================================================
Benchmark: Does an early `is_empty()` check improve async loop performance?
================================================================================

QUESTION:
  When iterating over a potentially empty collection in async Rust, should you
  add an early `if !data.is_empty()` check before entering the async loop?

TESTED SCENARIOS:

┌─────────────────────────────────────────────────────────────────────────────┐
│ Scenario 1: CPU-bound (yield_now) - Measures pure async machinery overhead │
├─────────────────────────────────────────────────────────────────────────────┤
│ with_check (empty):  ~1.62 ns                                               │
│ no_check (empty):    ~2.07 ns                                               │
│ Difference:          ~0.45 ns (28% relative improvement)                    │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│ Scenario 2: Simulated I/O (1µs sleep) - Real-world perspective             │
├─────────────────────────────────────────────────────────────────────────────┤
│ with_check (1 item): ~1.15 ms                                               │
│ no_check (1 item):   ~1.15 ms                                               │
│ Difference:          ~0 (I/O latency dominates, async overhead irrelevant) │
│                                                                             │
│ with_check (empty):  ~1.61 ns                                               │
│ no_check (empty):    ~2.09 ns                                               │
│ Difference:          ~0.48 ns (but meaningless when I/O takes 1ms+)        │
└─────────────────────────────────────────────────────────────────────────────┘

================================================================================
CONCLUSION
================================================================================

The ~0.5ns overhead of NOT checking `is_empty()` comes from:
  1. Creating an iterator via `IntoIterator::into_iter()`
  2. Calling `Iterator::next()` which returns `None` for empty collections
  3. Matching on the `Option` discriminant

With an early `is_empty()` check, we skip all of the above and return immediately.

HOWEVER, this optimization is RARELY meaningful because:

  • 0.5ns = 0.0000005ms
  • Typical network RTT: 1-100ms
  • Typical disk I/O: 0.1-10ms
  • Ratio: 1 : 2,000,000 to 1 : 200,000,000

PRACTICAL ADVICE:

  ❌ DON'T add early empty checks for:
     - I/O-bound async code (network, disk, database)
     - Code where readability matters more than nanoseconds
     - Anything called less than millions of times per second

  ✅ CONSIDER adding early empty checks for:
     - Extremely hot CPU-bound loops (billions of calls/sec)
     - When profiling shows this specific code path as a bottleneck

BOTTOM LINE:
  The performance gain exists but is negligible in real-world applications.
  Prioritize code clarity over this micro-optimization.

================================================================================
MIR-level Explanation (run: rustup run nightly rustc -Z unpretty=mir src/mir_demo.rs)
================================================================================

with_check path (empty data):
  bb0 -> bb1 (is_empty) -> bb2 (branch) -> bb3 (return Ready)
  Operations: len check + branch + return

no_check path (empty data):
  bb0 -> bb1 (into_iter) -> bb2 (store) -> bb3 (next) -> bb4 (match) -> bb7 (return)
  Operations: iterator creation + state storage + next() + Option match + return

The extra ~0.45ns comes from iterator setup and the next() call, even when
the loop body never executes.
*/

/// Simulates an async loop function with a potential suspension point./// Simulates an async loop function with a potential suspension point.
async fn async_loop_with_await(data: Vec<i32>) {
    for item in data {
        // Key point: even with 0 iterations, the generated Future state machine 
        // includes logic for handling .await suspension.
        tokio::task::yield_now().await;
        black_box(item);
    }
}

/// Simulates a more realistic I/O-bound async function (1µs latency).
async fn async_loop_with_sleep(data: Vec<i32>) {
    for item in data {
        tokio::time::sleep(std::time::Duration::from_micros(1)).await;
        black_box(item);
    }
}

fn bench_empty_check(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let empty_data: Vec<i32> = vec![];

    // ============================================================
    // Scenario 1: CPU-bound (yield_now) - Shows async machinery overhead
    // ============================================================
    let mut group = c.benchmark_group("1. CPU-bound (yield_now)");

    // Case A: Early check
    // Intercepting before calling the async function avoids Future creation and executor polling.
    group.bench_function("with_check", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&empty_data);
            if !data.is_empty() {
                async_loop_with_await(data.clone()).await;
            }
        })
    });

    // Case B: Direct entry
    // A Future is always created and must be polled once by the executor to discover termination.
    group.bench_function("no_check", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&empty_data);
            async_loop_with_await(data.clone()).await;
        })
    });

    group.finish();

    // ============================================================
    // Scenario 2: Simulated I/O (1µs sleep) - Real-world perspective
    // This shows that async overhead is negligible vs actual I/O latency
    // ============================================================
    let mut group_io = c.benchmark_group("2. Simulated IO (1µs sleep)");
    group_io.sample_size(10); // Reduce samples due to sleep

    let one_item: Vec<i32> = vec![1]; // Single item to trigger actual work

    group_io.bench_function("with_check (1 item)", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&one_item);
            if !data.is_empty() {
                async_loop_with_sleep(data.clone()).await;
            }
        })
    });

    group_io.bench_function("no_check (1 item)", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&one_item);
            async_loop_with_sleep(data.clone()).await;
        })
    });

    // Empty case - shows the check cost is negligible vs I/O
    group_io.bench_function("with_check (empty)", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&empty_data);
            if !data.is_empty() {
                async_loop_with_sleep(data.clone()).await;
            }
        })
    });

    group_io.bench_function("no_check (empty)", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&empty_data);
            async_loop_with_sleep(data.clone()).await;
        })
    });

    group_io.finish();
}

criterion_group!(benches, bench_empty_check);
criterion_main!(benches);
