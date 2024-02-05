#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! Monitor key metrics of tokio tasks and runtimes.
//!
//! ### Monitoring task metrics
//! [Monitor][TaskMonitor] key [metrics][TaskMetrics] of tokio tasks.
//!
//! In the below example, a [`TaskMonitor`] is [constructed][TaskMonitor::new] and used to
//! [instrument][TaskMonitor::instrument] three worker tasks; meanwhile, a fourth task
//! prints [metrics][TaskMetrics] in 500ms [intervals][TaskMonitor::intervals]:
//! ```
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!     // construct a metrics taskmonitor
//!     let metrics_monitor = tokio_metrics::TaskMonitor::new();
//!
//!     // print task metrics every 500ms
//!     {
//!         let metrics_monitor = metrics_monitor.clone();
//!         tokio::spawn(async move {
//!             for interval in metrics_monitor.intervals() {
//!                 // pretty-print the metric interval
//!                 println!("{:?}", interval);
//!                 // wait 500ms
//!                 tokio::time::sleep(Duration::from_millis(500)).await;
//!             }
//!         });
//!     }
//!
//!     // instrument some tasks and await them
//!     // note that the same taskmonitor can be used for multiple tasks
//!     tokio::join![
//!         metrics_monitor.instrument(do_work()),
//!         metrics_monitor.instrument(do_work()),
//!         metrics_monitor.instrument(do_work())
//!     ];
//!
//!     Ok(())
//! }
//!
//! async fn do_work() {
//!     for _ in 0..25 {
//!         tokio::task::yield_now().await;
//!         tokio::time::sleep(Duration::from_millis(100)).await;
//!     }
//! }
//! ```

#![cfg_attr(
    all(tokio_unstable, feature = "rt"),
    doc = r##"
### Monitoring runtime metrics (unstable)
[Monitor][RuntimeMonitor] key [metrics][RuntimeMetrics] of a tokio runtime.
**This functionality requires `tokio_unstable` and the crate feature `rt`.**

In the below example, a [`RuntimeMonitor`] is [constructed][RuntimeMonitor::new] and
three tasks are spawned and awaited; meanwhile, a fourth task prints [metrics][RuntimeMetrics]
in 500ms [intervals][RuntimeMonitor::intervals]:
```
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let handle = tokio::runtime::Handle::current();
    // construct the runtime metrics monitor
    let runtime_monitor = tokio_metrics::RuntimeMonitor::new(&handle);

    // print runtime metrics every 500ms
    {
        tokio::spawn(async move {
            for interval in runtime_monitor.intervals() {
                // pretty-print the metric interval
                println!("{:?}", interval);
                // wait 500ms
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }

    // await some tasks
    tokio::join![
        do_work(),
        do_work(),
        do_work(),
    ];

    Ok(())
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
```"##
)]

macro_rules! cfg_rt {
    ($($item:item)*) => {
        $(
            #[cfg(all(tokio_unstable, feature = "rt"))]
            #[cfg_attr(docsrs, doc(cfg(all(tokio_unstable, feature = "rt"))))]
            $item
        )*
    };
}

cfg_rt! {
    mod runtime;
    pub use runtime::{
        RuntimeIntervals,
        RuntimeMetrics,
        RuntimeMonitor,
    };
}

mod task;
pub use task::{Instrumented, TaskMetrics, TaskMonitor};

#[cfg(feature = "rt")]
pub mod detectors;
