use std::ops::Range;
use std::time::Duration;

/// A histogram of task poll durations, pairing each bucket's count with its
/// time range from the runtime configuration.
///
/// This type is returned as part of [`RuntimeMetrics`][super::RuntimeMetrics]
/// when the runtime has poll time histograms enabled via
/// [`enable_metrics_poll_time_histogram`][tokio::runtime::Builder::enable_metrics_poll_time_histogram].
///
/// Each bucket contains the [`Duration`] range configured for that bucket and
/// the count of task polls that fell into that range during the sampling
/// interval.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PollTimeHistogram {
    buckets: Vec<HistogramBucket>,
}

impl PollTimeHistogram {
    /// Creates a new histogram from the given buckets.
    pub(crate) fn new(buckets: Vec<HistogramBucket>) -> Self {
        Self { buckets }
    }

    /// Returns the histogram buckets.
    pub fn buckets(&self) -> &[HistogramBucket] {
        &self.buckets
    }

    /// Returns a mutable reference to the histogram buckets.
    pub(crate) fn buckets_mut(&mut self) -> &mut [HistogramBucket] {
        &mut self.buckets
    }

    /// Returns just the bucket counts as a `Vec<u64>`.
    pub fn as_counts(&self) -> Vec<u64> {
        self.buckets.iter().map(|b| b.count).collect()
    }
}

/// A single bucket in a [`PollTimeHistogram`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HistogramBucket {
    range: Range<Duration>,
    count: u64,
}

impl HistogramBucket {
    /// Creates a new histogram bucket.
    pub(crate) fn new(range: Range<Duration>, count: u64) -> Self {
        Self { range, count }
    }

    /// Returns the time range for this bucket.
    pub fn range(&self) -> &Range<Duration> {
        &self.range
    }

    /// Returns the number of task polls that fell into this bucket during the interval.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Adds to the count of this bucket.
    pub(crate) fn add_count(&mut self, delta: u64) {
        self.count = self.count.saturating_add(delta);
    }
}

impl Default for HistogramBucket {
    fn default() -> Self {
        Self {
            range: Duration::ZERO..Duration::ZERO,
            count: 0,
        }
    }
}

#[cfg(feature = "metrique-integration")]
impl metrique::writer::Value for PollTimeHistogram {
    fn write(&self, writer: impl metrique::writer::ValueWriter) {
        use metrique::writer::unit::NegativeScale;
        use metrique::writer::{MetricFlags, Observation, Unit};

        // Use the bucket midpoint as the representative value. Tokio's last
        // bucket is an overflow bucket whose range.end is
        // Duration::from_nanos(u64::MAX); use range.start for that bucket
        // since the midpoint would be nonsensical.
        const OVERFLOW_END: Duration = Duration::from_nanos(u64::MAX);
        writer.metric(
            self.buckets.iter().filter(|b| b.count > 0).map(|b| {
                let value_us = if b.range.end == OVERFLOW_END {
                    b.range.start.as_micros() as f64
                } else {
                    #[allow(clippy::incompatible_msrv)] // metrique-integration requires 1.89+
                    f64::midpoint(
                        b.range.start.as_micros() as f64,
                        b.range.end.as_micros() as f64,
                    )
                };
                Observation::Repeated {
                    total: value_us * b.count as f64,
                    occurrences: b.count,
                }
            }),
            Unit::Second(NegativeScale::Micro),
            [],
            MetricFlags::empty(),
        );
    }
}

#[cfg(feature = "metrique-integration")]
impl metrique::CloseValue for PollTimeHistogram {
    type Closed = Self;

    fn close(self) -> Self {
        self
    }
}

#[cfg(all(test, feature = "metrique-integration"))]
mod tests {
    use super::*;
    use crate::runtime::RuntimeMetrics;
    use metrique::CloseValue;
    use metrique::test_util::test_metric;

    #[test]
    fn poll_time_histogram_close_value() {
        let hist = PollTimeHistogram::new(vec![
            HistogramBucket::new(Duration::from_micros(0)..Duration::from_micros(100), 5),
            HistogramBucket::new(Duration::from_micros(100)..Duration::from_micros(200), 0),
            HistogramBucket::new(Duration::from_micros(200)..Duration::from_micros(500), 3),
        ]);

        let closed = hist.close();
        let buckets = closed.buckets();
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].count(), 5);
        assert_eq!(
            *buckets[0].range(),
            Duration::from_micros(0)..Duration::from_micros(100)
        );
        assert_eq!(buckets[1].count(), 0);
        assert_eq!(buckets[2].count(), 3);
        assert_eq!(
            *buckets[2].range(),
            Duration::from_micros(200)..Duration::from_micros(500)
        );
    }

    #[test]
    fn poll_time_histogram_overflow_bucket_uses_range_start() {
        let overflow_start = Duration::from_millis(500);
        let metrics = RuntimeMetrics {
            poll_time_histogram: PollTimeHistogram::new(vec![
                HistogramBucket::new(Duration::from_micros(0)..Duration::from_micros(100), 0),
                HistogramBucket::new(overflow_start..Duration::from_nanos(u64::MAX), 2),
            ]),
            ..Default::default()
        };

        let entry = test_metric(metrics);
        let hist = &entry.metrics["poll_time_histogram"];
        assert_eq!(hist.distribution.len(), 1);

        match hist.distribution[0] {
            metrique::writer::Observation::Repeated { total, occurrences } => {
                assert_eq!(occurrences, 2);
                let expected = overflow_start.as_micros() as f64 * 2.0;
                assert!((total - expected).abs() < 0.01);
            }
            other => panic!("expected Repeated, got {other:?}"),
        }
    }
}
