use std::future::Future;

pub async fn async_work() {
    // Simulates an async operation
}

// Case A: with_check
pub async fn with_check(data: &Vec<i32>) {
    if !data.is_empty() {
        async_work().await;
    }
}

// Case B: no_check
pub async fn no_check(data: &Vec<i32>) {
    for _ in data {
        async_work().await;
    }
}

fn main() {}
