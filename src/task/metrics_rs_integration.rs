use std::{fmt, time::Duration};

use super::{TaskIntervals, TaskMetrics, TaskMonitor};
use crate::metrics_rs::{metric_refs, DEFAULT_METRIC_SAMPLING_INTERVAL};

/// A builder for the [`TaskMetricsReporter`] that wraps the [`TaskMonitor`], periodically
/// reporting [`TaskMetrics`] to any configured [metrics-rs] recorder.
///
/// ### Published Metrics
///
/// The published metrics are the fields of [`TaskMetrics`], but with the
/// `tokio_` prefix added, for example, `tokio_instrumented_count`. If you have multiple
/// [`TaskMonitor`]s then it is strongly recommended to give each [`TaskMonitor`] a unique metric
/// name or dimension value.
///
/// ### Usage
///
/// To upload metrics via [metrics-rs], you need to set up a reporter, which
/// is actually what exports the metrics outside of the program. You must set
/// up the reporter before you call [`describe_and_run`].
///
/// You can find exporters within the [metrics-rs] docs. One such reporter
/// is the [metrics_exporter_prometheus] reporter, which makes metrics visible
/// through Prometheus.
///
/// You can use it for example to export Prometheus metrics by listening on a local Unix socket
/// called `prometheus.sock`, which you can access for debugging by
/// `curl --unix-socket prometheus.sock localhost`, as follows:
///
/// ```
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     metrics_exporter_prometheus::PrometheusBuilder::new()
///         .with_http_uds_listener("prometheus.sock")
///         .install()
///         .unwrap();
///     let monitor = tokio_metrics::TaskMonitor::new();
///     tokio::task::spawn(
///         tokio_metrics::TaskMetricsReporterBuilder::new(|name| {
///             let name = name.replacen("tokio_", "my_task_", 1);
///             Key::from_parts(name, &[("application", "my_app")])
///         })
///         // the default metric sampling interval is 30 seconds, which is
///         // too long for quick tests, so have it be 1 second.
///         .with_interval(std::time::Duration::from_secs(1))
///         .describe_and_run(monitor.clone()),
///     );
///     // Run some code
///     tokio::task::spawn(monitor.instrument(async move {
///         for _ in 0..1000 {
///             tokio::time::sleep(Duration::from_millis(10)).await;
///         }
///     }))
///     .await
///     .unwrap();
/// }
/// ```
///
/// [`describe_and_run`]: TaskMetricsReporterBuilder::describe_and_run
/// [`with_metrics_transformer`]: TaskMetricsReporterBuilder::with_metrics_transformer
/// [metrics-rs]: metrics
/// [metrics_exporter_prometheus]: https://docs.rs/metrics_exporter_prometheus
pub struct TaskMetricsReporterBuilder {
    interval: Duration,
    metrics_transformer: Box<dyn FnMut(&'static str) -> metrics::Key + Send>,
}

impl fmt::Debug for TaskMetricsReporterBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskMetricsReporterBuilder")
            .field("interval", &self.interval)
            // skip metrics_transformer field
            .finish()
    }
}

impl TaskMetricsReporterBuilder {
    /// Creates a new [`TaskMetricsReporterBuilder`] with a custom "metrics transformer". The custom
    /// transformer is used during `build` to transform the metric names into metric keys, for
    /// example to add dimensions. The string metric names used by this reporter all start with
    /// `tokio_`. The default transformer is just [`metrics::Key::from_static_name`]
    ///
    /// For example, to attach a dimension named "application" with value "my_app", and to replace
    /// `tokio_` with `my_task_`
    /// ```
    /// # use metrics::Key;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     metrics_exporter_prometheus::PrometheusBuilder::new()
    ///         .with_http_uds_listener("prometheus.sock")
    ///         .install()
    ///         .unwrap();
    ///     tokio::task::spawn(
    ///         tokio_metrics::RuntimeMetricsReporterBuilder::new(|name| {
    ///             let name = name.replacen("tokio_", "my_task_", 1);
    ///             Key::from_parts(name, &[("application", "my_app")])
    ///         })
    ///         .describe_and_run()
    ///     );
    /// }
    /// ```
    pub fn new(transformer: impl FnMut(&'static str) -> metrics::Key + Send + 'static) -> Self {
        TaskMetricsReporterBuilder {
            interval: DEFAULT_METRIC_SAMPLING_INTERVAL,
            metrics_transformer: Box::new(transformer),
        }
    }

    /// Set the metric sampling interval, default: 30 seconds.
    ///
    /// Note that this is the interval on which metrics are *sampled* from
    /// the Tokio task and then set on the [metrics-rs] reporter. Uploading the
    /// metrics upstream is controlled by the reporter set up in the
    /// application, and is normally controlled by a different period.
    ///
    /// For example, if metrics are exported via Prometheus, that
    /// normally operates at a pull-based fashion, and the actual collection
    /// period is controlled by the Prometheus server, which periodically polls the
    /// application's Prometheus exporter to get the latest value of the metrics.
    ///
    /// [metrics-rs]: metrics
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Build the [`TaskMetricsReporter`] with a specific [`TaskMonitor`]. This function will capture
    /// the [`Counter`]s and [`Gauge`]s from the current [metrics-rs] reporter,
    /// so if you are using [`with_local_recorder`], you should wrap this function and [`describe`]
    /// with it.
    ///
    /// [`Counter`]: metrics::Counter
    /// [`Gauge`]: metrics::Counter
    /// [`Histogram`]: metrics::Counter
    /// [metrics-rs]: metrics
    /// [`with_local_recorder`]: metrics::with_local_recorder
    /// [`describe`]: Self::describe
    #[must_use = "reporter does nothing unless run"]
    pub fn build_with_monitor(mut self, monitor: TaskMonitor) -> TaskMetricsReporter {
        TaskMetricsReporter {
            interval: self.interval,
            intervals: monitor.intervals(),
            emitter: TaskMetricRefs::capture(&mut self.metrics_transformer),
        }
    }

    /// Call [`describe_counter`] etc. to describe the emitted metrics.
    ///
    /// Describing metrics makes the reporter attach descriptions and units to them,
    /// which makes them easier to use. However, some reporters don't support
    /// describing the same metric name more than once, so it is generally a good
    /// idea to only call this function once per metric reporter.
    ///
    /// [`describe_counter`]: metrics::describe_counter
    /// [metrics-rs]: metrics
    pub fn describe(mut self) -> Self {
        TaskMetricRefs::describe(&mut self.metrics_transformer);
        self
    }

    /// Runs the reporter (within the returned future), [describing] the metrics beforehand.
    ///
    /// Describing metrics makes the reporter attach descriptions and units to them,
    /// which makes them easier to use. However, some reporters don't support
    /// describing the same metric name more than once. If you are emitting multiple
    /// metrics via a single reporter, try to call [`describe`] once and [`run`] for each
    /// task metrics reporter.
    ///
    /// ### Working with a custom reporter
    ///
    /// If you want to set a local metrics reporter, you shouldn't be calling this method,
    /// but you should instead call `.describe().build()` within [`with_local_recorder`] and then
    /// call `run` (see the docs on [`build_with_monitor`]).
    ///
    /// [describing]: Self::describe
    /// [`describe`]: Self::describe
    /// [`build_with_monitor`]: Self::build_with_monitor.
    /// [`run`]: TaskMetricsReporter::run
    /// [`with_local_recorder`]: metrics::with_local_recorder
    pub async fn describe_and_run(self, monitor: TaskMonitor) {
        self.describe().build_with_monitor(monitor).run().await;
    }

    /// Runs the reporter (within the returned future), not describing the metrics beforehand.
    ///
    /// ### Working with a custom reporter
    ///
    /// If you want to set a local metrics reporter, you shouldn't be calling this method,
    /// but you should instead call `.describe().build()` within [`with_local_recorder`] and then
    /// call [`run`] (see the docs on [`build_with_monitor`]).
    ///
    /// [`build_with_monitor`]: Self::build_with_monitor
    /// [`run`]: TaskMetricsReporter::run
    /// [`with_local_recorder`]: metrics::with_local_recorder
    pub async fn run_without_describing(self, monitor: TaskMonitor) {
        self.build_with_monitor(monitor).run().await;
    }
}

/// Collects metrics from a Tokio task and uploads them to [metrics_rs](metrics).
pub struct TaskMetricsReporter {
    interval: Duration,
    intervals: TaskIntervals,
    emitter: TaskMetricRefs,
}

metric_refs! {
    [TaskMetricRefs] [elapsed] [TaskMetrics] [()] {
        stable {
            /// The number of tasks instrumented.
            instrumented_count: Gauge<Count> [],
            /// The number of tasks dropped.
            dropped_count: Gauge<Count> [],
            /// The number of tasks polled for the first time.
            first_poll_count: Gauge<Count> [],
            /// The total duration elapsed between the instant tasks are instrumented, and the instant they are first polled.
            total_first_poll_delay: Counter<Microseconds> [],
            /// The total number of times that tasks idled, waiting to be awoken.
            total_idled_count: Gauge<Count> [],
            /// The total duration that tasks idled.
            total_idle_duration: Counter<Microseconds> [],
            /// The maximum idle duration that a task took.
            max_idle_duration: Counter<Microseconds> [],
            /// The total number of times that tasks were awoken (and then, presumably, scheduled for execution).
            total_scheduled_count: Gauge<Count> [],
            /// The total duration that tasks spent waiting to be polled after awakening.
            total_scheduled_duration: Counter<Microseconds> [],
            /// The total number of times that tasks were polled.
            total_poll_count: Gauge<Count> [],
            /// The total duration elapsed during polls.
            total_poll_duration: Counter<Microseconds> [],
            /// The total number of times that polling tasks completed swiftly.
            total_fast_poll_count: Gauge<Count> [],
            /// The total duration of fast polls.
            total_fast_poll_duration: Counter<Microseconds> [],
            /// The total number of times that polling tasks completed slowly.
            total_slow_poll_count: Gauge<Count> [],
            /// The total duration of slow polls.
            total_slow_poll_duration: Counter<Microseconds> [],
            /// The total count of tasks with short scheduling delays.
            total_short_delay_count: Gauge<Count> [],
            /// The total count of tasks with long scheduling delays.
            total_long_delay_count: Gauge<Count> [],
            /// The total duration of tasks with short scheduling delays.
            total_short_delay_duration: Counter<Microseconds> [],
            /// The total number of times that a task had a long scheduling duration.
            total_long_delay_duration: Counter<Microseconds> [],
        }
        unstable {}
    }
}

impl fmt::Debug for TaskMetricsReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskMetricsReporter")
            .field("interval", &self.interval)
            // skip intervals field
            .finish()
    }
}

impl TaskMetricsReporter {
    /// Collect and publish metrics once to the configured [metrics_rs](metrics) reporter.
    pub fn run_once(&mut self) {
        let metrics = self
            .intervals
            .next()
            .expect("TaskIntervals::next never returns None");
        self.emitter.emit(metrics, ());
    }

    /// Collect and publish metrics periodically to the configured [metrics_rs](metrics) reporter.
    ///
    /// You probably want to run this within its own task (using [`tokio::task::spawn`])
    pub async fn run(mut self) {
        loop {
            self.run_once();
            tokio::time::sleep(self.interval).await;
        }
    }
}
