use std::time::Duration;

pub(crate) const DEFAULT_METRIC_SAMPLING_INTERVAL: Duration = Duration::from_secs(30);

macro_rules! kind_to_type {
    (Counter) => {
        metrics::Counter
    };
    (Gauge) => {
        metrics::Gauge
    };
    (PollTimeHistogram) => {
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
    ($transform_fn:ident, $doc:expr, $name:ident: PollTimeHistogram<$unit:ident> []) => {
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
    ($transform_fn:ident, $name:ident: PollTimeHistogram []) => {{
        let (name, labels) = crate::metrics_rs::metric_key!($transform_fn, $name).into_parts();
        metrics::histogram!(name, labels)
    }};
}

macro_rules! metric_refs {
    (
        [$struct_name:ident] [$($ignore:ident),* $(,)?] [$metrics_name:ty] [$emit_arg_type:ty] {
         stable {
            $(
                #[doc = $doc:tt]
                $name:ident: $kind:tt <$unit:ident> $opts:tt
            ),*
            $(,)?
         }
         stable_derived {
             $(
                #[doc = $derived_doc:tt]
                $derived_name:ident: $derived_kind:tt <$derived_unit:ident> $derived_opts:tt
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
         unstable_derived {
             $(
                #[doc = $unstable_derived_doc:tt]
                $unstable_derived_name:ident: $unstable_derived_kind:tt <$unstable_derived_unit:ident> $unstable_derived_opts:tt
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
                $derived_name: crate::metrics_rs::kind_to_type!($derived_kind),
            )*
            $(
                #[cfg(tokio_unstable)]
                $unstable_name: crate::metrics_rs::kind_to_type!($unstable_kind),
            )*
            $(
                #[cfg(tokio_unstable)]
                $unstable_derived_name: crate::metrics_rs::kind_to_type!($unstable_derived_kind),
            )*
        }

        impl $struct_name {
            fn capture(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) -> Self {
                Self {
                    $(
                        $name: crate::metrics_rs::capture_metric_ref!(transform_fn, $name: $kind $opts),
                    )*
                    $(
                        $derived_name: crate::metrics_rs::capture_metric_ref!(transform_fn, $derived_name: $derived_kind $derived_opts),
                    )*
                    $(
                        #[cfg(tokio_unstable)]
                        $unstable_name: crate::metrics_rs::capture_metric_ref!(transform_fn, $unstable_name: $unstable_kind $unstable_opts),
                    )*
                    $(
                        #[cfg(tokio_unstable)]
                        $unstable_derived_name: crate::metrics_rs::capture_metric_ref!(transform_fn, $unstable_derived_name: $unstable_derived_kind $unstable_derived_opts),
                    )*
                }
            }

            fn emit(&self, metrics: $metrics_name, emit_arg: $emit_arg_type) {
                // Emit derived metrics before base metrics because emitting base metrics may move
                // out of `$metrics`.
                $(
                    crate::metrics_rs::MyMetricOp::op((&self.$derived_name, metrics.$derived_name()), emit_arg);
                )*
                $(
                    #[cfg(tokio_unstable)]
                    crate::metrics_rs::MyMetricOp::op((&self.$unstable_derived_name, metrics.$unstable_derived_name()), emit_arg);
                )*
                $(
                    crate::metrics_rs::MyMetricOp::op((&self.$name, metrics.$name), emit_arg);
                )*
                $(
                    #[cfg(tokio_unstable)]
                    crate::metrics_rs::MyMetricOp::op((&self.$unstable_name, metrics.$unstable_name), emit_arg);
                )*
            }

            fn describe(transform_fn: &mut dyn FnMut(&'static str) -> metrics::Key) {
                $(
                    crate::metrics_rs::describe_metric_ref!(transform_fn, $doc, $name: $kind<$unit> $opts);
                )*
                $(
                    crate::metrics_rs::describe_metric_ref!(transform_fn, $derived_doc, $derived_name: $derived_kind<$derived_unit> $derived_opts);
                )*
                $(
                    #[cfg(tokio_unstable)]
                    crate::metrics_rs::describe_metric_ref!(transform_fn, $unstable_doc, $unstable_name: $unstable_kind<$unstable_unit> $unstable_opts);
                )*
                $(
                    #[cfg(tokio_unstable)]
                    crate::metrics_rs::describe_metric_ref!(transform_fn, $unstable_derived_doc, $unstable_derived_name: $unstable_derived_kind<$unstable_derived_unit> $unstable_derived_opts);
                )*
            }
        }

        #[test]
        fn test_no_fields_missing() {
            // test that no fields are missing. We can't use exhaustive matching here
            // since the metrics structs are #[non_exhaustive], so use a debug impl
            let debug = format!("{:#?}", <$metrics_name>::default());
            for line in debug.lines() {
                if line == format!("{} {{", stringify!($metrics_name)) || line == "}" {
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

pub(crate) trait MyMetricOp<T> {
    fn op(self, t: T);
}

impl<T> MyMetricOp<T> for (&metrics::Counter, Duration) {
    fn op(self, _: T) {
        self.0
            .increment(self.1.as_micros().try_into().unwrap_or(u64::MAX));
    }
}

impl<T> MyMetricOp<T> for (&metrics::Counter, u64) {
    fn op(self, _t: T) {
        self.0.increment(self.1);
    }
}

impl<T> MyMetricOp<T> for (&metrics::Gauge, Duration) {
    fn op(self, _t: T) {
        self.0.set(self.1.as_micros() as f64);
    }
}

impl<T> MyMetricOp<T> for (&metrics::Gauge, u64) {
    fn op(self, _: T) {
        self.0.set(self.1 as f64);
    }
}

impl<T> MyMetricOp<T> for (&metrics::Gauge, usize) {
    fn op(self, _t: T) {
        self.0.set(self.1 as f64);
    }
}

impl<T> MyMetricOp<T> for (&metrics::Gauge, f64) {
    fn op(self, _t: T) {
        self.0.set(self.1);
    }
}

#[cfg(tokio_unstable)]
impl MyMetricOp<&tokio::runtime::RuntimeMetrics> for (&metrics::Histogram, Vec<u64>) {
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
