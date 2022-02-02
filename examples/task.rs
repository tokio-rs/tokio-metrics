use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let metrics_monitor = tokio_metrics::TaskMonitor::new();

    // print task metrics every 500ms
    {
        let metrics_monitor = metrics_monitor.clone();
        tokio::spawn(async move {
            for deltas in metrics_monitor.intervals() {
                // pretty-print the metric deltas
                println!("{:?}", deltas);
                // wait 500ms
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }

    // instrument some tasks and await them
    tokio::join![
        metrics_monitor.instrument(do_work()),
        metrics_monitor.instrument(do_work()),
        metrics_monitor.instrument(do_work())
    ];

    Ok(())
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
