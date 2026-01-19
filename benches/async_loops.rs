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

MIR-level Explanation:
- In Rust's Mid-level Intermediate Representation (MIR), an `async fn` is desugared into a State Machine (a struct implementing `Future`).
- `no_check` path: The MIR must include instructions to initialize this struct and prepare it for polling. The executor then performs at least one `poll()` call to realize the loop range is empty.
- `with_check` path: The MIR generates a simple branch (`switchInt`). If the condition is false, it jumps over the entire Future creation and awaiting logic. This "short-circuits" the async machinery entirely, avoiding state machine setup and the initial poll.
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
