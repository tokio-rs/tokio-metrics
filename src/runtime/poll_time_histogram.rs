use std::ops::Range;
use std::sync::Arc;
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
pub struct PollTimeHistogram {
    buckets: Arc<Vec<HistogramBucket>>,
}

impl PollTimeHistogram {
    /// Creates a new histogram from the given buckets.
    pub(crate) fn new(buckets: Vec<HistogramBucket>) -> Self {
        Self {
            buckets: Arc::new(buckets),
        }
    }

    /// Returns the histogram buckets with their ranges and counts.
    pub fn buckets(&self) -> &[HistogramBucket] {
        &self.buckets
    }

    pub(crate) fn buckets_mut(&mut self) -> &mut Vec<HistogramBucket> {
        Arc::make_mut(&mut self.buckets)
    }

    /// Returns just the bucket counts, matching the old `Vec<u64>` representation.
    pub fn as_counts(&self) -> Vec<u64> {
        self.buckets.iter().map(|b| b.count).collect()
    }
}

/// A single bucket in a [`PollTimeHistogram`].
#[derive(Debug, Clone)]
pub struct HistogramBucket {
    /// The time range for this bucket (from the runtime's histogram configuration).
    pub range: Range<Duration>,
    /// The number of task polls that fell into this bucket during the interval.
    pub count: u64,
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
        // bucket is an overflow bucket with range.end == Duration::MAX (from
        // Duration::from_nanos(u64::MAX)); use range.start for that bucket
        // since the midpoint would be nonsensical.
        writer.metric(
            self.buckets.iter().filter(|b| b.count > 0).map(|b| {
                let value_us = if b.range.end == Duration::MAX {
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

#[cfg(feature = "metrique-integration")]
impl metrique::CloseValue for &PollTimeHistogram {
    type Closed = PollTimeHistogram;

    fn close(self) -> PollTimeHistogram {
        self.clone()
    }
}

#[cfg(all(test, feature = "metrique-integration"))]
mod tests {
    use super::*;
    use metrique::CloseValue;

    #[test]
    fn poll_time_histogram_close_value() {
        let hist = PollTimeHistogram::new(vec![
                HistogramBucket {
                    range: Duration::from_micros(0)..Duration::from_micros(100),
                    count: 5,
                },
                HistogramBucket {
                    range: Duration::from_micros(100)..Duration::from_micros(200),
                    count: 0,
                },
                HistogramBucket {
                    range: Duration::from_micros(200)..Duration::from_micros(500),
                    count: 3,
                },
            ],
        );

        let closed = hist.close();
        assert_eq!(closed.buckets().len(), 3);
        assert_eq!(closed.buckets()[0].count, 5);
        assert_eq!(
            closed.buckets()[0].range,
            Duration::from_micros(0)..Duration::from_micros(100)
        );
        assert_eq!(closed.buckets()[1].count, 0);
        assert_eq!(closed.buckets()[2].count, 3);
        assert_eq!(
            closed.buckets()[2].range,
            Duration::from_micros(200)..Duration::from_micros(500)
        );
    }
}
