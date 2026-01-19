use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

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
