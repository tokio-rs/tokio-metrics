use std::{fmt, time::Duration};

use super::{RuntimeIntervals, RuntimeMetrics, RuntimeMonitor};

/// A reporter builder
pub struct RuntimeMetricsReporterBuilder {
    interval: Duration,
    metrics_transformer: Box<dyn FnMut(&'static str) -> metrics::Key>,
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
            metrics_transformer: Box::new(|metric| metrics::Key::from_static_name(metric)),
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
    pub fn build(self, monitor: RuntimeMonitor) -> RuntimeMetricsReporter {
        RuntimeMetricsReporter {
            interval: self.interval,
            intervals: monitor.intervals(),
        }
    }

    /// Run the reporter3
    pub async fn run(self, monitor: RuntimeMonitor) {
        self.build(monitor).run().await;
    }
}

/// A reporter
pub struct RuntimeMetricsReporter {
    interval: Duration,
    intervals: RuntimeIntervals,
}

macro_rules! kind_to_type {
    (Counter) => (metrics::Counter)
}
macro_rules! describe_metric_ref {
    ($transform_fn:ident, $doc:expr, $name:ident: Counter<$unit:ident>) => (
        metrics::describe_counter!($transform_fn(stringify!($name)).name().to_owned(), metrics::Unit::$unit, $doc)
    )
}

macro_rules! metric_refs {
    (
        [$struct_name:ident] {
        $(
            #[doc = $doc:tt]
            $name:ident: $kind:tt <$unit:ident>
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
                /*
                Self {
                    $(
                        $name: panic!(),
                    ),*
                } */
               panic!()
            }

            fn describe(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) {
                $(
                    describe_metric_ref!(transform_fn, $doc, $name: $kind<$unit>);
                ),*
            }

        }
    }
}

metric_refs! {
    [RuntimeMetricRefs] {

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