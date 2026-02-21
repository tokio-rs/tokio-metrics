use std::time::Duration;
use tokio_metrics::{TaskMonitor, TaskMonitorCore};

/// A static TaskMonitorCore — no Arc or lazy initialization needed.
static STATIC_MONITOR: TaskMonitorCore = TaskMonitorCore::new();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // === Using TaskMonitor (Arc-backed, cloneable) ===
    let metrics_monitor = TaskMonitor::new();

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
        });
    }

    // instrument some tasks and await them
    tokio::join![
        metrics_monitor.instrument(do_work()),
        metrics_monitor.instrument(do_work()),
        metrics_monitor.instrument(do_work())
    ];

    // === Using TaskMonitorCore (static, no allocation) ===
    tokio::spawn(async {
        for deltas in TaskMonitorCore::intervals(&STATIC_MONITOR) {
            println!("{deltas:?}");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    tokio::join![
        TaskMonitorCore::instrument(do_work(), &STATIC_MONITOR),
        TaskMonitorCore::instrument(do_work(), &STATIC_MONITOR),
        TaskMonitorCore::instrument(do_work(), &STATIC_MONITOR),
    ];

    Ok(())
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
