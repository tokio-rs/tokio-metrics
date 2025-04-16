use std::{fmt, time::Duration};

use tokio::runtime::Handle;

use super::{RuntimeIntervals, RuntimeMetrics, RuntimeMonitor};

/// A reporter builder
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
            interval: Duration::from_secs(30),
            metrics_transformer: Box::new(metrics::Key::from_static_name),
        }
    }
}

impl RuntimeMetricsReporterBuilder {
    /// Set the interval
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Build the reporter
    pub fn build(self) -> RuntimeMetricsReporter {
        self.build_with_monitor(RuntimeMonitor::new(&Handle::current()))
    }

    /// Build the reporter with a specific [`RuntimeMonitor`]
    pub fn build_with_monitor(mut self, monitor: RuntimeMonitor) -> RuntimeMetricsReporter {
        RuntimeMetricsReporter {
            interval: self.interval,
            intervals: monitor.intervals(),
            emitter: RuntimeMetricRefs::capture(&mut self.metrics_transformer),
        }
    }

    /// Describe the metrics. You might not want to run this more than once
    pub fn describe(mut self) -> Self {
        RuntimeMetricRefs::describe(&mut self.metrics_transformer);
        self
    }

    /// Run the reporter, describing the metrics beforehand
    pub async fn describe_and_run(self) {
        self.describe().build().run().await;
    }

    /// Run the reporter, not describing the metrics beforehand
    pub async fn run_without_describing(self) {
        self.build().run().await;
    }
}

/// A reporter
pub struct RuntimeMetricsReporter {
    interval: Duration,
    intervals: RuntimeIntervals,
    emitter: RuntimeMetricRefs,
}

macro_rules! kind_to_type {
    (Counter) => (metrics::Counter);
    (Gauge) => (metrics::Gauge);
    (Histogram) => (metrics::Histogram);
}

macro_rules! metric_key {
    ($transform_fn:ident, $name:ident) => ($transform_fn(concat!("tokio_", stringify!($name))))
}

// calling `trim` since /// inserts spaces into docs
macro_rules! describe_metric_ref {
    ($transform_fn:ident, $doc:expr, $name:ident: Counter<$unit:ident> []) => (
        metrics::describe_counter!(metric_key!($transform_fn, $name).name().to_owned(), metrics::Unit::$unit, $doc.trim())
    );
    ($transform_fn:ident, $doc:expr, $name:ident: Gauge<$unit:ident> []) => (
        metrics::describe_gauge!(metric_key!($transform_fn, $name).name().to_owned(), metrics::Unit::$unit, $doc.trim())
    );
    ($transform_fn:ident, $doc:expr, $name:ident: Histogram<$unit:ident> []) => (
        metrics::describe_histogram!(metric_key!($transform_fn, $name).name().to_owned(), metrics::Unit::$unit, $doc.trim())
    );
}

macro_rules! capture_metric_ref {
    ($transform_fn:ident, $name:ident: Counter []) => (
        {
            let (name, labels) = metric_key!($transform_fn, $name).into_parts();
            metrics::counter!(name, labels)
        }
    );
    ($transform_fn:ident, $name:ident: Gauge []) => (
        {
            let (name, labels) = metric_key!($transform_fn, $name).into_parts();
            metrics::gauge!(name, labels)
        }
    );
    ($transform_fn:ident, $name:ident: Histogram []) => (
        {
            let (name, labels) = metric_key!($transform_fn, $name).into_parts();
            metrics::histogram!(name, labels)
        }
    );
}

macro_rules! metric_refs {
    (
        [$struct_name:ident] [$($ignore:ident),* $(,)?] {
        $(
            #[doc = $doc:tt]
            $name:ident: $kind:tt <$unit:ident> $opts:tt
        ),*
        $(,)?
        }
  ) => {
        struct $struct_name {
            $(
                $name: kind_to_type!($kind)
            ),*
        }

        impl $struct_name {
            fn capture(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) -> Self {
                Self {
                    $(
                        $name: capture_metric_ref!(transform_fn, $name: $kind $opts)
                    ),*
                }
            }

            fn emit(&self, metrics: RuntimeMetrics, tokio: &tokio::runtime::RuntimeMetrics) {
                $(
                    MyMetricOp::op((&self.$name, metrics.$name), tokio);
                )*
            }

            fn describe(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) {
                $(
                    describe_metric_ref!(transform_fn, $doc, $name: $kind<$unit> $opts);
                )*
            }
        }
    }
}

metric_refs! {
    [RuntimeMetricRefs] [elapsed] {
        /// The number of worker threads used by the runtime
        workers_count: Gauge<Count> [],
        /// The number of times worker threads parked
        max_park_count: Gauge<Count> [],
        /// The minimum number of times any worker thread parked
        min_park_count: Gauge<Count> [],
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
        /// The amount of time worker threads were busy
        total_busy_duration: Counter<Microseconds> [],
        /// The maximum amount of time a worker thread was busy
        max_busy_duration: Counter<Microseconds> [],
        /// The minimum amount of time a worker thread was busy
        min_busy_duration: Counter<Microseconds> [],
        /// The number of tasks currently scheduled in the runtime's global queue
        global_queue_depth: Gauge<Count> [],
        /// The total number of tasks currently scheduled in workers' local queues
        total_local_queue_depth: Gauge<Count> [],
        /// The maximum number of tasks currently scheduled any worker's local queue
        max_local_queue_depth: Gauge<Count> [],
        /// The minimum number of tasks currently scheduled any worker's local queue
        min_local_queue_depth: Gauge<Count> [],
        /// Returns the number of times that tasks have been forced to yield back to the scheduler after exhausting their task budgets
        budget_forced_yield_count: Counter<Count> [],
        /// Returns the number of ready events processed by the runtime’s I/O driver
        io_driver_ready_count: Counter<Count> [],
    }
}
trait MyMetricOp {
    fn op(self, tokio: &tokio::runtime::RuntimeMetrics);
}

impl MyMetricOp for (&metrics::Counter, Duration) {
    fn op(self, _tokio: &tokio::runtime::RuntimeMetrics) {
        self.0.increment(self.1.as_micros().try_into().unwrap_or(u64::MAX));
    }
}

impl MyMetricOp for (&metrics::Counter, u64) {
    fn op(self, _tokio: &tokio::runtime::RuntimeMetrics) {
        self.0.increment(self.1);
    }
}

impl MyMetricOp for (&metrics::Gauge, Duration) {
    fn op(self, _tokio: &tokio::runtime::RuntimeMetrics) {
        self.0.set(self.1.as_micros() as f64);
    }
}

impl MyMetricOp for (&metrics::Gauge, u64) {
    fn op(self, _tokio: &tokio::runtime::RuntimeMetrics) {
        self.0.set(self.1 as f64);
    }
}

impl MyMetricOp for (&metrics::Gauge, usize) {
    fn op(self, _tokio: &tokio::runtime::RuntimeMetrics) {
        self.0.set(self.1 as f64);
    }
}

impl MyMetricOp for (&metrics::Histogram, Vec<u64>) {
    fn op(self, tokio: &tokio::runtime::RuntimeMetrics) {
        for (i, bucket) in self.1.iter().enumerate() {
            let range = tokio.poll_time_histogram_bucket_range(i);
            if *bucket > 0 {
                // emit using range.start to avoid very large numbers for open bucket
                // FIXME: do we want to do something else here
                self.0.record_many(range.start.as_micros() as f64, *bucket as usize);
            }
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

impl RuntimeMetricsReporter
{
    /// Collect and publish metrics once
    pub fn run_once(&mut self) {
        let metrics = self.intervals.next().expect("RuntimeIntervals::next never returns None");
        self.emitter.emit(metrics, &self.intervals.runtime);
    }

    /// Collect and run metrics.
    ///
    /// You probably want to run this within its own task (using [`tokio::task::spawn`])
    pub async fn run(mut self) {
        loop {
            self.run_once();
            tokio::time::sleep(self.interval).await;
        }
    }
}
