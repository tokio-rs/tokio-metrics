use std::time::Duration;
use tokio_metrics::{TaskMonitor, TaskMonitorCore};

/// It's usually the right choice to use a static [`tokio_metrics::TaskMonitorCore`].
///
/// If you need to dynamically generate task monitors at runtime,
/// [`tokio_metrics::TaskMonitor`] will be more ergonomic.
///
/// See the [`tokio_metrics::TaskMonitorCore`] documentation for more discussion.
static STATIC_MONITOR: TaskMonitorCore = TaskMonitorCore::new();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // spawn a task that prints out from the static monitor on a loop
    tokio::spawn(async {
        for deltas in TaskMonitorCore::intervals(&STATIC_MONITOR) {
            // pretty print
            println!("{deltas:?}");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    tokio::join![
        STATIC_MONITOR.instrument(do_work()),
        STATIC_MONITOR.instrument(do_work()),
        STATIC_MONITOR.instrument(do_work()),
    ];

    // imagine we wanted to generate a task monitor to keep track of all tasks
    // and child tasks spawned by a given request
    for i in 0..5 {
        // roughly equivalent to Arc::new(TaskMonitorCore::new())
        let metrics_monitor = TaskMonitor::new();

        // instrument some tasks and await them
        tokio::join![
            // roughly equivalent to TaskMonitorCore::instrument_with(do_work(), metrics_monitor.clone())
            metrics_monitor.instrument(do_work()),
            metrics_monitor.instrument(do_work()),
            metrics_monitor.instrument(do_work())
        ];

        let cumulative = metrics_monitor.cumulative();
        println!("{i}: {cumulative:?}");
    }

    Ok(())
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
