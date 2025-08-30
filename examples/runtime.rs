use std::time::Duration;
use tokio_metrics::RuntimeMonitor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let handle = tokio::runtime::Handle::current();

    // print runtime metrics every 500ms
    {
        let runtime_monitor = RuntimeMonitor::new(&handle);
        tokio::spawn(async move {
            for interval in runtime_monitor.intervals() {
                // pretty-print the metric interval
                println!("{interval:?}");
                // wait 500ms
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }

    // await some tasks
    tokio::join![do_work(), do_work(), do_work(),];

    Ok(())
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
