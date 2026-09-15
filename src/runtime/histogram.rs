use std::time::Duration;

/// A histogram of durations: each bucket pairs a [`Duration`] range with the
/// number of observations that fell into it during the sampling interval.
///
/// Bucket ranges come from the runtime configuration and do not change.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct DurationHistogram {
    buckets: Vec<HistogramBucket>,
}

impl DurationHistogram {
    // Only used to populate the histogram, which requires `tokio_unstable`.
    #[cfg_attr(not(tokio_unstable), allow(dead_code))]
    pub(crate) fn new(buckets: Vec<HistogramBucket>) -> Self {
        Self { buckets }
    }

    /// Returns the histogram buckets.
    pub fn buckets(&self) -> &[HistogramBucket] {
        &self.buckets
    }

    // Only used to populate the histogram, which requires `tokio_unstable`.
    #[cfg_attr(not(tokio_unstable), allow(dead_code))]
    pub(crate) fn buckets_mut(&mut self) -> &mut [HistogramBucket] {
        &mut self.buckets
    }

    /// Returns just the bucket counts as a `Vec<u64>`.
    pub fn as_counts(&self) -> Vec<u64> {
        self.buckets.iter().map(|b| b.count).collect()
    }
}

/// A single bucket in a [`DurationHistogram`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct HistogramBucket {
    range_start: Duration,
    range_end: Duration,
    count: u64,
}

impl HistogramBucket {
    // Only used to populate the histogram, which requires `tokio_unstable`.
    #[cfg_attr(not(tokio_unstable), allow(dead_code))]
    pub(crate) fn new(range_start: Duration, range_end: Duration, count: u64) -> Self {
        Self { range_start, range_end, count }
    }

    /// The start of the time range for this bucket (inclusive).
    pub fn range_start(&self) -> Duration {
        self.range_start
    }

    /// The end of the time range for this bucket (exclusive).
    pub fn range_end(&self) -> Duration {
        self.range_end
    }

    /// Returns the number of observations that fell into this bucket during the
    /// interval.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Adds to the count of this bucket.
    // Only used to populate the histogram, which requires `tokio_unstable`.
    #[cfg_attr(not(tokio_unstable), allow(dead_code))]
    pub(crate) fn add_count(&mut self, delta: u64) {
        self.count = self.count.saturating_add(delta);
    }
}

#[cfg(feature = "metrique-integration")]
impl metrique::writer::Value for DurationHistogram {
    // Emitted as a distribution of bucket midpoints in microseconds, so the
    // closed shape is a float rather than the `Opaque` default.
    const SHAPE: metrique::writer::core::FieldShape<'static> =
        metrique::writer::core::FieldShape::Known(metrique::writer::core::KnownShape::F64);
    const UNIT: metrique::writer::Unit = metrique::writer::Unit::Second(
        metrique::writer::unit::NegativeScale::Micro,
    );

    fn write(&self, writer: impl metrique::writer::ValueWriter) {
        use metrique::writer::{MetricFlags, Observation};

        // Use the bucket midpoint as the representative value.
        // Tokio's last bucket has range_end of Duration::from_nanos(u64::MAX),
        // so use range_start for it since the midpoint wouldn't be representative.
        const LAST_BUCKET_END: Duration = Duration::from_nanos(u64::MAX);
        writer.metric(
            self.buckets.iter().filter(|b| b.count > 0).map(|b| {
                let value_us = if b.range_end == LAST_BUCKET_END {
                    b.range_start.as_micros() as f64
                } else {
                    #[allow(clippy::incompatible_msrv)] // metrique-integration requires 1.89+
                    f64::midpoint(
                        b.range_start.as_micros() as f64,
                        b.range_end.as_micros() as f64,
                    )
                };
                Observation::Repeated {
                    total: value_us * b.count as f64,
                    occurrences: b.count,
                }
            }),
            Self::UNIT,
            [],
            MetricFlags::empty(),
        );
    }
}

#[cfg(feature = "metrique-integration")]
impl metrique::CloseValue for DurationHistogram {
    type Closed = Self;

    fn close(self) -> Self {
        self
    }
}

// `poll_time_histogram_last_bucket_uses_range_start` constructs a
// `RuntimeMetrics` and reads its `poll_time_histogram` field, both of which
// require `tokio_unstable`.
#[cfg(all(test, tokio_unstable, feature = "metrique-integration"))]
mod tests {
    use super::*;
    use crate::runtime::RuntimeMetrics;
    use metrique::CloseValue;
    use metrique::test_util::test_metric;

    #[test]
    fn poll_time_histogram_close_value() {
        let hist = DurationHistogram::new(vec![
            HistogramBucket::new(Duration::from_micros(0), Duration::from_micros(100), 5),
            HistogramBucket::new(Duration::from_micros(100), Duration::from_micros(200), 0),
            HistogramBucket::new(Duration::from_micros(200), Duration::from_micros(500), 3),
        ]);

        let closed = hist.close();
        let buckets = closed.buckets();
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].count(), 5);
        assert_eq!(buckets[0].range_start(), Duration::from_micros(0));
        assert_eq!(buckets[0].range_end(), Duration::from_micros(100));
        assert_eq!(buckets[1].count(), 0);
        assert_eq!(buckets[2].count(), 3);
        assert_eq!(buckets[2].range_start(), Duration::from_micros(200));
        assert_eq!(buckets[2].range_end(), Duration::from_micros(500));
    }

    #[test]
    fn poll_time_histogram_declares_shape_and_unit() {
        use metrique::writer::Value;
        use metrique::writer::core::{FieldShape, KnownShape};

        assert_eq!(
            <DurationHistogram as Value>::SHAPE,
            FieldShape::Known(KnownShape::F64)
        );

        let metrics = RuntimeMetrics {
            poll_time_histogram: DurationHistogram::new(vec![HistogramBucket::new(
                Duration::from_micros(0),
                Duration::from_micros(100),
                1,
            )]),
            ..Default::default()
        };

        // The unit reaching the writer must be the one the impl declares.
        let entry = test_metric(metrics);
        assert_eq!(
            entry.metrics["poll_time_histogram"].unit,
            <DurationHistogram as Value>::UNIT
        );
    }

    #[test]
    fn poll_time_histogram_last_bucket_uses_range_start() {
        let last_bucket_start = Duration::from_millis(500);
        let metrics = RuntimeMetrics {
            poll_time_histogram: DurationHistogram::new(vec![
                HistogramBucket::new(Duration::from_micros(0), Duration::from_micros(100), 0),
                HistogramBucket::new(last_bucket_start, Duration::from_nanos(u64::MAX), 2),
            ]),
            ..Default::default()
        };

        let entry = test_metric(metrics);
        let hist = &entry.metrics["poll_time_histogram"];
        assert_eq!(hist.distribution.len(), 1);

        match hist.distribution[0] {
            metrique::writer::Observation::Repeated { total, occurrences } => {
                assert_eq!(occurrences, 2);
                let expected = last_bucket_start.as_micros() as f64 * 2.0;
                assert!((total - expected).abs() < 0.01);
            }
            other => panic!("expected Repeated, got {other:?}"),
        }
    }
}
