use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

/*
Benchmark Results (Updated with realistic scenarios):

┌─────────────────────────────────────────────────────────────────────────────┐
│ Scenario 1: CPU-bound (yield_now) - Pure async machinery overhead          │
├─────────────────────────────────────────────────────────────────────────────┤
│ with_check (empty):  ~1.59 ns                                               │
│ no_check (empty):    ~2.07 ns                                               │
│ Difference:          ~0.48 ns (30% slower without check)                    │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│ Scenario 2: Boxed Future - Heap allocation overhead                        │
├─────────────────────────────────────────────────────────────────────────────┤
│ with_check (empty):  ~1.59 ns                                               │
│ no_check (empty):    ~20.0 ns                                               │
│ Difference:          ~18.4 ns (12x slower without check!)                   │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│ Scenario 3: Simulated I/O (1µs sleep) - Real-world perspective             │
├─────────────────────────────────────────────────────────────────────────────┤
│ with_check (1 item): ~1.15 ms  ← Dominated by sleep, not async overhead    │
│ no_check (1 item):   ~1.15 ms  ← Same! I/O latency >> async overhead       │
│ Difference:          ~0 (negligible)                                        │
│                                                                             │
│ with_check (empty):  ~1.60 ns  ← Fast path                                  │
│ no_check (empty):    ~2.10 ns  ← Still has iterator overhead               │
│ Difference:          ~0.5 ns (but who cares when I/O is 1ms?)              │
└─────────────────────────────────────────────────────────────────────────────┘

⚠️ KEY TAKEAWAYS:

1. For CPU-bound micro-loops:
   - ~0.5ns difference exists but is NEGLIGIBLE in most applications
   - Only matters if you're processing BILLIONS of empty checks per second

2. For Boxed/Spawned futures:
   - ~18ns difference is SIGNIFICANT (heap alloc/dealloc)
   - Worth optimizing in hot paths with frequent empty collections

3. For I/O-bound workloads (the common case):
   - Async overhead is COMPLETELY IRRELEVANT
   - 1µs I/O latency = 1000ns >> 0.5ns async overhead
   - Real network latency (1-100ms) makes this even more irrelevant

4. PRACTICAL ADVICE:
   ✅ Add empty check if: using Box::pin, tokio::spawn, or processing millions/sec
   ❌ Don't bother if: I/O-bound, occasional calls, or code clarity matters more

MIR-level Explanation (see src/mir_demo.rs for full MIR output):

┌──────────────────────────────────────────────────────────────────────────────┐
│ with_check::{closure#0} (Poll function for "with_check" Future)             │
├──────────────────────────────────────────────────────────────────────────────┤
│ bb0: switchInt(discriminant) -> [0: bb1, ...]   // Check current state      │
│                                                                              │
│ bb1: _4 = Vec::is_empty(data)                   // Call is_empty()          │
│      goto -> bb2                                                             │
│                                                                              │
│ bb2: switchInt(_4) -> [0: bb4, otherwise: bb3]  // Branch on result         │
│      ┌─────────────┴─────────────┐                                          │
│      ▼                           ▼                                          │
│ bb3: (empty=true)           bb4: (empty=false)                              │
│      _14 = const ()              _6 = async_work() // Create inner Future   │
│      goto -> bb14                goto -> bb5...bb8  // Setup & poll         │
│      ▼                                                                       │
│ bb14: return Poll::Ready    // FAST PATH: Skip all async machinery!         │
└──────────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────────┐
│ no_check::{closure#0} (Poll function for "no_check" Future)                 │
├──────────────────────────────────────────────────────────────────────────────┤
│ bb0: switchInt(discriminant) -> [0: bb1, ...]   // Check current state      │
│                                                                              │
│ bb1: _4 = IntoIterator::into_iter(data)         // ALWAYS create iterator   │
│      goto -> bb2                                                             │
│                                                                              │
│ bb2: store iterator in state machine            // ALWAYS store state       │
│      goto -> bb3                                                             │
│                                                                              │
│ bb3: _5 = Iterator::next(&mut iter)             // ALWAYS call next()       │
│      goto -> bb4                                                             │
│                                                                              │
│ bb4: switchInt(discriminant(_5)) -> [0: bb7, 1: bb6]                        │
│      ┌─────────────┴─────────────┐                                          │
│      ▼                           ▼                                          │
│ bb7: (None - empty)         bb6: (Some - has items)                         │
│      return Poll::Ready          _9 = async_work()                          │
│      ▲                           ... more work ...                          │
│      │                                                                       │
│      └── Even for empty vec, we've already done:                            │
│          1. IntoIterator::into_iter() call                                  │
│          2. Iterator state stored in Future struct                          │
│          3. Iterator::next() call                                           │
│          4. Option discriminant check                                       │
└──────────────────────────────────────────────────────────────────────────────┘

Key Insight:
- with_check: On empty data, executes bb0 -> bb1 -> bb2 -> bb3 -> bb14 (5 blocks)
  Only operations: is_empty() check + return Ready
  
- no_check: On empty data, executes bb0 -> bb1 -> bb2 -> bb3 -> bb4 -> bb7 (6 blocks)
  Operations: into_iter() + store state + next() + discriminant check + return Ready
  
The ~0.43ns difference comes from the iterator creation and next() call overhead,
even when no actual iteration occurs.
*/

/// Simulates an async loop function with a potential suspension point.
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
    // Scenario 2: Boxed Future - Shows heap allocation overhead
    // ============================================================
    let mut group_boxed = c.benchmark_group("2. Boxed Future (heap alloc)");

    async fn async_loop_boxed(data: Vec<i32>) {
        if data.is_empty() { return; }
        tokio::task::yield_now().await;
    }

    // Case A: Early check with Boxed Future
    // Avoids heap allocation when data is empty.
    group_boxed.bench_function("with_check", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&empty_data);
            if !data.is_empty() {
                Box::pin(async_loop_boxed(data.clone())).await;
            }
        })
    });

    // Case B: Direct entry with Boxed Future
    // Forces a heap allocation even if no work is performed.
    group_boxed.bench_function("no_check", |b| {
        b.to_async(&rt).iter(|| async {
            let data = black_box(&empty_data);
            Box::pin(async_loop_boxed(data.clone())).await;
        })
    });

    group_boxed.finish();

    // ============================================================
    // Scenario 3: Simulated I/O (1µs sleep) - Real-world perspective
    // This shows that async overhead is negligible vs actual I/O latency
    // ============================================================
    let mut group_io = c.benchmark_group("3. Simulated IO (1µs sleep)");
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
