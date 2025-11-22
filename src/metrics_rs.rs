use std::time::Duration;

pub(crate) const DEFAULT_METRIC_SAMPLING_INTERVAL: Duration = Duration::from_secs(30);

macro_rules! kind_to_type {
    (Counter) => {
        metrics::Counter
    };
    (Gauge) => {
        metrics::Gauge
    };
    (Histogram) => {
        metrics::Histogram
    };
}

macro_rules! metric_key {
    ($transform_fn:ident, $name:ident) => {
        $transform_fn(concat!("tokio_", stringify!($name)))
    };
}

// calling `trim` since /// inserts spaces into docs
macro_rules! describe_metric_ref {
    ($transform_fn:ident, $doc:expr, $name:ident: Counter<$unit:ident> []) => {
        metrics::describe_counter!(
            crate::metrics_rs::metric_key!($transform_fn, $name)
                .name()
                .to_owned(),
            metrics::Unit::$unit,
            $doc.trim()
        )
    };
    ($transform_fn:ident, $doc:expr, $name:ident: Gauge<$unit:ident> []) => {
        metrics::describe_gauge!(
            crate::metrics_rs::metric_key!($transform_fn, $name)
                .name()
                .to_owned(),
            metrics::Unit::$unit,
            $doc.trim()
        )
    };
    ($transform_fn:ident, $doc:expr, $name:ident: Histogram<$unit:ident> []) => {
        metrics::describe_histogram!(
            crate::metrics_rs::metric_key!($transform_fn, $name)
                .name()
                .to_owned(),
            metrics::Unit::$unit,
            $doc.trim()
        )
    };
}

macro_rules! capture_metric_ref {
    ($transform_fn:ident, $name:ident: Counter []) => {{
        let (name, labels) = crate::metrics_rs::metric_key!($transform_fn, $name).into_parts();
        metrics::counter!(name, labels)
    }};
    ($transform_fn:ident, $name:ident: Gauge []) => {{
        let (name, labels) = crate::metrics_rs::metric_key!($transform_fn, $name).into_parts();
        metrics::gauge!(name, labels)
    }};
    ($transform_fn:ident, $name:ident: Histogram []) => {{
        let (name, labels) = crate::metrics_rs::metric_key!($transform_fn, $name).into_parts();
        metrics::histogram!(name, labels)
    }};
}

macro_rules! metric_refs {
    (
        [$struct_name:ident] [$($ignore:ident),* $(,)?] {
         stable {
            $(
                #[doc = $doc:tt]
                $name:ident: $kind:tt <$unit:ident> $opts:tt
            ),*
            $(,)?
         }
         unstable {
            $(
                #[doc = $unstable_doc:tt]
                $unstable_name:ident: $unstable_kind:tt <$unstable_unit:ident> $unstable_opts:tt
            ),*
            $(,)?
         }
        }
  ) => {
        struct $struct_name {
            $(
                $name: crate::metrics_rs::kind_to_type!($kind),
            )*
            $(
                #[cfg(tokio_unstable)]
                $unstable_name: crate::metrics_rs::kind_to_type!($unstable_kind),
            )*
        }

        impl $struct_name {
            fn capture(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) -> Self {
                Self {
                    $(
                        $name: crate::metrics_rs::capture_metric_ref!(transform_fn, $name: $kind $opts),
                    )*
                    $(
                        #[cfg(tokio_unstable)]
                        $unstable_name: crate::metrics_rs::capture_metric_ref!(transform_fn, $unstable_name: $unstable_kind $unstable_opts),
                    )*
                }
            }

            fn emit(&self, metrics: RuntimeMetrics, tokio: &tokio::runtime::RuntimeMetrics) {
                $(
                    MyMetricOp::op((&self.$name, metrics.$name), tokio);
                )*
                $(
                    #[cfg(tokio_unstable)]
                    MyMetricOp::op((&self.$unstable_name, metrics.$unstable_name), tokio);
                )*
            }

            fn describe(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) {
                $(
                    crate::metrics_rs::describe_metric_ref!(transform_fn, $doc, $name: $kind<$unit> $opts);
                )*
                $(
                    #[cfg(tokio_unstable)]
                    crate::metrics_rs::describe_metric_ref!(transform_fn, $unstable_doc, $unstable_name: $unstable_kind<$unstable_unit> $unstable_opts);
                )*
            }
        }

        #[test]
        fn test_no_fields_missing() {
            // test that no fields are missing. We can't use exhaustive matching here
            // since RuntimeMetrics is #[non_exhaustive], so use a debug impl
            let debug = format!("{:#?}", RuntimeMetrics::default());
            for line in debug.lines() {
                if line == "RuntimeMetrics {" || line == "}" {
                    continue
                }
                $(
                    let expected = format!("    {}:", stringify!($ignore));
                    if line.contains(&expected) {
                        continue
                    }
                );*
                $(
                    let expected = format!("    {}:", stringify!($name));
                    eprintln!("{}", expected);
                    if line.contains(&expected) {
                        continue
                    }
                );*
                $(
                    let expected = format!("    {}:", stringify!($unstable_name));
                    eprintln!("{}", expected);
                    if line.contains(&expected) {
                        continue
                    }
                );*
                panic!("missing metric {:?}", line);
            }
        }
    }
}

pub(crate) use capture_metric_ref;
pub(crate) use describe_metric_ref;
pub(crate) use kind_to_type;
pub(crate) use metric_key;
pub(crate) use metric_refs;

pub(crate) trait MyMetricOp {
    fn op(self, tokio: &tokio::runtime::RuntimeMetrics);
}

impl MyMetricOp for (&metrics::Counter, Duration) {
    fn op(self, _tokio: &tokio::runtime::RuntimeMetrics) {
        self.0
            .increment(self.1.as_micros().try_into().unwrap_or(u64::MAX));
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

#[cfg(tokio_unstable)]
impl MyMetricOp for (&metrics::Histogram, Vec<u64>) {
    fn op(self, tokio: &tokio::runtime::RuntimeMetrics) {
        for (i, bucket) in self.1.iter().enumerate() {
            let range = tokio.poll_time_histogram_bucket_range(i);
            if *bucket > 0 {
                // emit using range.start to avoid very large numbers for open bucket
                // FIXME: do we want to do something else here?
                self.0
                    .record_many(range.start.as_micros() as f64, *bucket as usize);
            }
        }
    }
}
