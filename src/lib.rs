#![warn(
    missing_debug_implementations,
    missing_docs,
    rust_2018_idioms,
    unreachable_pub
)]
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
```

### Monitoring and publishing runtime metrics (unstable)

If the `metrics-rs-integration` feature is additionally enabled, this crate allows
publishing runtime metrics externally via [metrics-rs](metrics) exporters.

For example, you can use [metrics_exporter_prometheus] to make metrics visible
to [Prometheus]. You can see the [metrics_exporter_prometheus] and [metrics-rs](metrics)
docs for guidance on configuring exporters.

The published metrics are the same as the fields of [RuntimeMetrics], but with
a "tokio_" prefix added, for example `tokio_workers_count`.

[metrics_exporter_prometheus]: https://docs.rs/metrics_exporter_prometheus
[RuntimeMetrics]: crate::RuntimeMetrics
[Prometheus]: https://prometheus.io

This example exports [Prometheus] metrics by listening on a local Unix socket
called `prometheus.sock`, which you can access for debugging by
`curl --unix-socket prometheus.sock localhost`.

```
use std::time::Duration;

#[tokio::main]
async fn main() {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_uds_listener("prometheus.sock")
        .install()
        .unwrap();
    tokio::task::spawn(
        tokio_metrics::RuntimeMetricsReporterBuilder::default()
            // the default metric sampling interval is 30 seconds, which is
            // too long for quick tests, so have it be 1 second.
            .with_interval(std::time::Duration::from_secs(1))
            .describe_and_run(),
    );
    // Run some code
    tokio::task::spawn(async move {
        for _ in 0..1000 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
```
"##
)]

macro_rules! cfg_rt {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "rt")]
            #[cfg_attr(docsrs, doc(cfg(feature = "rt")))]
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

#[cfg(all(tokio_unstable, feature = "rt", feature = "metrics-rs-integration"))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(tokio_unstable, feature = "rt", feature = "metrics-rs-integration")))
)]
pub use runtime::metrics_rs_integration::{RuntimeMetricsReporter, RuntimeMetricsReporterBuilder};

mod task;
pub use task::{Instrumented, TaskMetrics, TaskMonitor};
