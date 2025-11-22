use std::{fmt, time::Duration};

use tokio::runtime::Handle;

use super::{RuntimeIntervals, RuntimeMetrics, RuntimeMonitor};
use crate::metrics_rs::{metric_refs, DEFAULT_METRIC_SAMPLING_INTERVAL, MyMetricOp};

/// A builder for the [`RuntimeMetricsReporter`] that wraps the RuntimeMonitor, periodically
/// reporting RuntimeMetrics to any configured [metrics-rs] recorder.
///
/// ### Published Metrics
///
/// The published metrics are the fields of [RuntimeMetrics], but with the
/// `tokio_` prefix added, for example, `tokio_workers_count`. If desired, you
/// can use the [`with_metrics_transformer`] function to customize the metric names.
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
///     tokio::task::spawn(
///         tokio_metrics::RuntimeMetricsReporterBuilder::default()
///             // the default metric sampling interval is 30 seconds, which is
///             // too long for quick tests, so have it be 1 second.
///             .with_interval(std::time::Duration::from_secs(1))
///             .describe_and_run(),
///     );
///     // Run some code
///     tokio::task::spawn(async move {
///         for _ in 0..1000 {
///             tokio::time::sleep(Duration::from_millis(10)).await;
///         }
///     })
///     .await
///     .unwrap();
/// }
/// ```
///
/// [`describe_and_run`]: RuntimeMetricsReporterBuilder::describe_and_run
/// [`with_metrics_transformer`]: RuntimeMetricsReporterBuilder::with_metrics_transformer
/// [metrics-rs]: metrics
/// [metrics_exporter_prometheus]: https://docs.rs/metrics_exporter_prometheus
pub struct RuntimeMetricsReporterBuilder {
    interval: Duration,
    metrics_transformer: Box<dyn FnMut(&'static str) -> metrics::Key + Send>,
}

impl fmt::Debug for RuntimeMetricsReporterBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeMetricsReporterBuilder")
            .field("interval", &self.interval)
            // skip metrics_transformer field
            .finish()
    }
}

impl Default for RuntimeMetricsReporterBuilder {
    fn default() -> Self {
        RuntimeMetricsReporterBuilder {
            interval: DEFAULT_METRIC_SAMPLING_INTERVAL,
            metrics_transformer: Box::new(metrics::Key::from_static_name),
        }
    }
}

impl RuntimeMetricsReporterBuilder {
    /// Set the metric sampling interval, default: 30 seconds.
    ///
    /// Note that this is the interval on which metrics are *sampled* from
    /// the Tokio runtime and then set on the [metrics-rs] reporter. Uploading the
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

    /// Set a custom "metrics transformer", which is used during `build` to transform the metric
    /// names into metric keys, for example to add dimensions. The string metric names used by this reporter
    /// all start with `tokio_`. The default transformer is just [`metrics::Key::from_static_name`]
    ///
    /// For example, to attach a dimension named "application" with value "my_app", and to replace
    /// `tokio_` with `my_app_`
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
    ///         tokio_metrics::RuntimeMetricsReporterBuilder::default().with_metrics_transformer(|name| {
    ///             let name = name.replacen("tokio_", "my_app_", 1);
    ///             Key::from_parts(name, &[("application", "my_app")])
    ///         })
    ///         .describe_and_run()
    ///     );
    /// }
    /// ```
    pub fn with_metrics_transformer(
        mut self,
        transformer: impl FnMut(&'static str) -> metrics::Key + Send + 'static,
    ) -> Self {
        self.metrics_transformer = Box::new(transformer);
        self
    }

    /// Build the [`RuntimeMetricsReporter`] for the current Tokio runtime. This function will capture
    /// the [`Counter`]s, [`Gauge`]s and [`Histogram`]s from the current [metrics-rs] reporter,
    /// so if you are using [`with_local_recorder`], you should wrap this function and [`describe`] with it.
    ///
    /// For example:
    /// ```
    /// # use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let builder = tokio_metrics::RuntimeMetricsReporterBuilder::default();
    ///     let recorder = Arc::new(metrics_util::debugging::DebuggingRecorder::new());
    ///     let metrics_reporter = metrics::with_local_recorder(&recorder, || builder.describe().build());
    ///
    ///     // no need to wrap `run()`, since the metrics are already captured
    ///     tokio::task::spawn(metrics_reporter.run());
    /// }
    /// ```
    ///
    ///
    /// [`Counter`]: metrics::Counter
    /// [`Gauge`]: metrics::Counter
    /// [`Histogram`]: metrics::Counter
    /// [metrics-rs]: metrics
    /// [`with_local_recorder`]: metrics::with_local_recorder
    /// [`describe`]: Self::describe
    #[must_use = "reporter does nothing unless run"]
    pub fn build(self) -> RuntimeMetricsReporter {
        self.build_with_monitor(RuntimeMonitor::new(&Handle::current()))
    }

    /// Build the [`RuntimeMetricsReporter`] with a specific [`RuntimeMonitor`]. This function will capture
    /// the [`Counter`]s, [`Gauge`]s and [`Histogram`]s from the current [metrics-rs] reporter,
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
    pub fn build_with_monitor(mut self, monitor: RuntimeMonitor) -> RuntimeMetricsReporter {
        RuntimeMetricsReporter {
            interval: self.interval,
            intervals: monitor.intervals(),
            emitter: RuntimeMetricRefs::capture(&mut self.metrics_transformer),
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
        RuntimeMetricRefs::describe(&mut self.metrics_transformer);
        self
    }

    /// Runs the reporter (within the returned future), [describing] the metrics beforehand.
    ///
    /// Describing metrics makes the reporter attach descriptions and units to them,
    /// which makes them easier to use. However, some reporters don't support
    /// describing the same metric name more than once. If you are emitting multiple
    /// metrics via a single reporter, try to call [`describe`] once and [`run`] for each
    /// runtime metrics reporter.
    ///
    /// ### Working with a custom reporter
    ///
    /// If you want to set a local metrics reporter, you shouldn't be calling this method,
    /// but you should instead call `.describe().build()` within [`with_local_recorder`] and then
    /// call `run` (see the docs on [`build`]).
    ///
    /// [describing]: Self::describe
    /// [`describe`]: Self::describe
    /// [`build`]: Self::build.
    /// [`run`]: RuntimeMetricsReporter::run
    /// [`with_local_recorder`]: metrics::with_local_recorder
    pub async fn describe_and_run(self) {
        self.describe().build().run().await;
    }

    /// Runs the reporter (within the returned future), not describing the metrics beforehand.
    ///
    /// ### Working with a custom reporter
    ///
    /// If you want to set a local metrics reporter, you shouldn't be calling this method,
    /// but you should instead call `.describe().build()` within [`with_local_recorder`] and then
    /// call [`run`] (see the docs on [`build`]).
    ///
    /// [`build`]: Self::build
    /// [`run`]: RuntimeMetricsReporter::run
    /// [`with_local_recorder`]: metrics::with_local_recorder
    pub async fn run_without_describing(self) {
        self.build().run().await;
    }
}

/// Collects metrics from a Tokio runtime and uploads them to [metrics_rs](metrics).
pub struct RuntimeMetricsReporter {
    interval: Duration,
    intervals: RuntimeIntervals,
    emitter: RuntimeMetricRefs,
}

metric_refs! {
    [RuntimeMetricRefs] [elapsed] {
        stable {
            /// The number of worker threads used by the runtime
            workers_count: Gauge<Count> [],
            /// The number of times worker threads parked
            max_park_count: Gauge<Count> [],
            /// The minimum number of times any worker thread parked
            min_park_count: Gauge<Count> [],
            /// The number of times worker threads parked
            total_park_count: Gauge<Count> [],
            /// The amount of time worker threads were busy
            total_busy_duration: Counter<Microseconds> [],
            /// The maximum amount of time a worker thread was busy
            max_busy_duration: Counter<Microseconds> [],
            /// The minimum amount of time a worker thread was busy
            min_busy_duration: Counter<Microseconds> [],
            /// The number of tasks currently scheduled in the runtime's global queue
            global_queue_depth: Gauge<Count> [],
        }
        unstable {
            /// The average duration of a single invocation of poll on a task
            mean_poll_duration: Gauge<Microseconds> [],
            /// The average duration of a single invocation of poll on a task on the worker with the lowest value
            mean_poll_duration_worker_min: Gauge<Microseconds> [],
            /// The average duration of a single invocation of poll on a task on the worker with the highest value
            mean_poll_duration_worker_max: Gauge<Microseconds> [],
            /// A histogram of task polls since the previous probe grouped by poll times
            poll_time_histogram: Histogram<Microseconds> [],
            /// The number of times worker threads unparked but performed no work before parking again
            total_noop_count: Counter<Count> [],
            /// The maximum number of times any worker thread unparked but performed no work before parking again
            max_noop_count: Counter<Count> [],
            /// The minimum number of times any worker thread unparked but performed no work before parking again
            min_noop_count: Counter<Count> [],
            /// The number of tasks worker threads stole from another worker thread
            total_steal_count: Counter<Count> [],
            /// The maximum number of tasks any worker thread stole from another worker thread.
            max_steal_count: Counter<Count> [],
            /// The minimum number of tasks any worker thread stole from another worker thread
            min_steal_count: Counter<Count> [],
            /// The number of times worker threads stole tasks from another worker thread
            total_steal_operations: Counter<Count> [],
            /// The maximum number of times any worker thread stole tasks from another worker thread
            max_steal_operations: Counter<Count> [],
            /// The minimum number of times any worker thread stole tasks from another worker thread
            min_steal_operations: Counter<Count> [],
            /// The number of tasks scheduled from **outside** of the runtime
            num_remote_schedules: Counter<Count> [],
            /// The number of tasks scheduled from worker threads
            total_local_schedule_count: Counter<Count> [],
            /// The maximum number of tasks scheduled from any one worker thread
            max_local_schedule_count: Counter<Count> [],
            /// The minimum number of tasks scheduled from any one worker thread
            min_local_schedule_count: Counter<Count> [],
            /// The number of times worker threads saturated their local queues
            total_overflow_count: Counter<Count> [],
            /// The maximum number of times any one worker saturated its local queue
            max_overflow_count: Counter<Count> [],
            /// The minimum number of times any one worker saturated its local queue
            min_overflow_count: Counter<Count> [],
            /// The number of tasks that have been polled across all worker threads
            total_polls_count: Counter<Count> [],
            /// The maximum number of tasks that have been polled in any worker thread
            max_polls_count: Counter<Count> [],
            /// The minimum number of tasks that have been polled in any worker thread
            min_polls_count: Counter<Count> [],
            /// The total number of tasks currently scheduled in workers' local queues
            total_local_queue_depth: Gauge<Count> [],
            /// The maximum number of tasks currently scheduled any worker's local queue
            max_local_queue_depth: Gauge<Count> [],
            /// The minimum number of tasks currently scheduled any worker's local queue
            min_local_queue_depth: Gauge<Count> [],
            /// The number of tasks currently waiting to be executed in the runtime's blocking threadpool.
            blocking_queue_depth: Gauge<Count> [],
            /// The current number of alive tasks in the runtime.
            live_tasks_count: Gauge<Count> [],
            /// The number of additional threads spawned by the runtime.
            blocking_threads_count: Gauge<Count> [],
            /// The number of idle threads, which have spawned by the runtime for `spawn_blocking` calls.
            idle_blocking_threads_count: Gauge<Count> [],
            /// Returns the number of times that tasks have been forced to yield back to the scheduler after exhausting their task budgets
            budget_forced_yield_count: Counter<Count> [],
            /// Returns the number of ready events processed by the runtime’s I/O driver
            io_driver_ready_count: Counter<Count> [],
        }
    }
}

impl fmt::Debug for RuntimeMetricsReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeMetricsReporter")
            .field("interval", &self.interval)
            // skip intervals field
            .finish()
    }
}

impl RuntimeMetricsReporter {
    /// Collect and publish metrics once to the configured [metrics_rs](metrics) reporter.
    pub fn run_once(&mut self) {
        let metrics = self
            .intervals
            .next()
            .expect("RuntimeIntervals::next never returns None");
        self.emitter.emit(metrics, &self.intervals.runtime);
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
