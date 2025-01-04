use std::sync::Arc;
use rust_latency_timer::LatencyTimer;

fn main() {
    let timer = Arc::new(LatencyTimer::default());
    rust_latency_timer::run(&timer);
    loop {
        timer.count1();
    }
}