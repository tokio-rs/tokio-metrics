use tokio::runtime;

use std::time::{Duration, Instant};

pub struct RuntimeMetrics {
    runtime: runtime::Handle,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub num_parks: u64,
    pub num_noops: u64,
    pub num_steals: u64,
    pub num_local_schedules: u64,
    pub num_polls: u64,
    pub total_time: Duration,
    pub total_time_busy: Duration,
}

impl RuntimeMetrics {
    pub fn new(runtime: &runtime::Handle) -> RuntimeMetrics {
        RuntimeMetrics { runtime: runtime.clone() }
    }

    // Total number of times a runtime thread parked
    pub fn num_parks(&self) -> u64 {
        self.runtime.stats().workers().map(|w| w.park_count()).sum()
    }

    pub fn num_noops(&self) -> u64 {
        self.runtime.stats().workers().map(|w| w.noop_count()).sum()
    }

    pub fn num_steals(&self) -> u64 {
        self.runtime.stats().workers().map(|w| w.steal_count()).sum()
    }

    pub fn num_local_schedules(&self) -> u64 {
        self.runtime.stats().workers().map(|w| w.local_schedule_count()).sum()
    }

    pub fn num_polls(&self) -> u64 {
        self.runtime.stats().workers().map(|w| w.poll_count()).sum()
    }

    pub fn total_time_busy(&self) -> Duration {
        self.runtime.stats().workers().map(|w| w.total_busy_duration()).sum()
    }

    fn probe(&self, started_at: Instant) -> Sample {
        Sample {
            num_parks: self.num_parks(),
            num_noops: self.num_noops(),
            num_steals: self.num_steals(),
            num_local_schedules: self.num_local_schedules(),
            num_polls: self.num_polls(),
            total_time: started_at.elapsed(),
            total_time_busy: self.total_time_busy(),
        }        
    }

    pub fn sample(&self) -> impl Iterator<Item = Sample> {
        struct Iter(RuntimeMetrics, Sample, Instant);

        impl Iterator for Iter {
            type Item = Sample;

            fn next(&mut self) -> Option<Sample> {
                let latest = self.0.probe(self.2);

                let ret = Sample {
                    num_parks: latest.num_parks - self.1.num_parks,
                    num_noops: latest.num_noops - self.1.num_noops,
                    num_steals: latest.num_steals - self.1.num_steals,
                    num_local_schedules: latest.num_local_schedules - self.1.num_local_schedules,
                    num_polls: latest.num_polls - self.1.num_polls,
                    total_time: latest.total_time - self.1.total_time,
                    total_time_busy: latest.total_time_busy - self.1.total_time_busy,
                };

                self.1 = latest;
                Some(ret)
            }
        }

        let now = Instant::now();

        Iter(RuntimeMetrics {
            runtime: self.runtime.clone()
        }, self.probe(now), now)
    }
}

impl Sample {
    pub fn mean_polls_per_park(&self) -> f64 {
        let num_parks = self.num_parks - self.num_noops;
        if num_parks == 0 {
            0.0
        } else {
            self.num_polls as f64 / num_parks as f64
        }
    }

    pub fn busy_ratio(&self) -> f64 {
        self.total_time_busy.as_nanos() as f64 /
            self.total_time.as_nanos() as f64
    }
}