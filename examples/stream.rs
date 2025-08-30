use std::time::Duration;

use futures::{stream::FuturesUnordered, StreamExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let metrics_monitor = tokio_metrics::TaskMonitor::new();

    // print task metrics every 500ms
    {
        let metrics_monitor = metrics_monitor.clone();
        tokio::spawn(async move {
            for deltas in metrics_monitor.intervals() {
                // pretty-print the metric deltas
                println!("{deltas:?}");
                // wait 500ms
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })
    };

    // instrument a stream and await it
    let mut stream =
        metrics_monitor.instrument((0..3).map(|_| do_work()).collect::<FuturesUnordered<_>>());
    while stream.next().await.is_some() {}

    println!("{:?}", metrics_monitor.cumulative());

    Ok(())
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
