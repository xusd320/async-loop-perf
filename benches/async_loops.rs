use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

/*
Benchmark Results (Typical):
---------------------------------------------------------
| Scenario (Empty Data)      | with_check | no_check |
|----------------------------|------------|----------|
| Stack Future (With Await)  | ~1.60 ns   | ~2.03 ns |
| Boxed Future (Heap Alloc)  | ~1.61 ns   | ~18.56 ns|
---------------------------------------------------------

Conclusion:
1. Even for stack-allocated futures, direct entry costs ~0.43ns (state machine init + 1st poll).
2. For boxed futures, direct entry costs ~17ns due to unnecessary heap allocation/deallocation.
3. Early `is_empty()` check is a nearly zero-cost optimization that bypasses executor overhead.

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

fn bench_empty_check(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let empty_data: Vec<i32> = vec![];

    let mut group = c.benchmark_group("Async Empty Check (With Await Logic)");

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

    let mut group_boxed = c.benchmark_group("Async Empty Check (Boxed Future)");

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
}

criterion_group!(benches, bench_empty_check);
criterion_main!(benches);
