use tokio::runtime;

use std::time::{Duration, Instant};

pub struct RuntimeMetrics {
    /// Handle to the runtime
    runtime: runtime::Handle,
}

#[non_exhaustive]
#[derive(Default, Debug, Clone, Copy)]
pub struct Sample {
    /// Number of worker threads
    pub num_workers: usize,

    /// Number of parks across all workers
    pub num_parks: u64,

    /// Maximum parks on any given worker
    pub max_parks: u64,

    /// Number of no-op ticks across all workers
    pub num_noops: u64,

    /// Maximum number of no-op ticks on any given worker
    pub max_noops: u64,

    /// Number of times a worker stole work
    pub num_steals: u64,

    /// Maximum number of times any given worker stole work
    pub max_steals: u64,

    /// Number of tasks scheduled from **outside** of the runtime
    pub num_remote_schedules: u64,

    /// Number of tasks scheduled locally across all workers
    pub num_local_schedules: u64,

    /// Maximum number of tasks scheduled locally on any given worker.
    pub max_local_schedules: u64,

    /// Number of times a task was polled across all workers
    pub num_polls: u64,

    /// Maximum number of tasks polled on any given worker
    pub max_polls: u64,

    /// Total amount of time all workers have been busy
    pub total_time_busy: Duration,

    /// Maximum amount of time any given worker has been busy
    pub max_time_busy: Duration,

    /// Total amount of time elapsed since observing runtime metrics.
    pub total_time: Duration,
}

/// Snapshot of per-worker metrics
struct Worker {
    num_parks: u64,
    num_noops: u64,
    num_steals: u64,
    num_local_schedules: u64,
    num_polls: u64,
    total_time_busy: Duration,
}

impl RuntimeMetrics {
    pub fn new(runtime: &runtime::Handle) -> RuntimeMetrics {
        let runtime = runtime.clone();

        RuntimeMetrics {
            runtime,
        }
    }

    pub fn sample(&self) -> impl Iterator<Item = Sample> {
        struct Iter {
            runtime: runtime::Handle,
            started_at: Instant,
            workers: Vec<Worker>,

            // Number of tasks scheduled from *outside* of the runtime
            num_remote_schedules: u64,
        }

        impl Iter {
            fn probe(&mut self) -> Sample {
                let stats = self.runtime.stats();
                let now = Instant::now();

                let num_remote_schedules = stats.remote_schedule_count();

                let mut sample = Sample {
                    num_workers: self.workers.len(),
                    total_time: now - self.started_at,
                    num_remote_schedules: num_remote_schedules - self.num_remote_schedules,
                    .. Default::default()
                };

                self.num_remote_schedules = num_remote_schedules;
                self.started_at = now;
        
                for (rt, worker) in stats.workers().zip(&mut self.workers) {
                    worker.probe(rt, &mut sample);
                }
        
                sample
            }
        }

        impl Iterator for Iter {
            type Item = Sample;

            fn next(&mut self) -> Option<Sample> {
                Some(self.probe())
            }
        }

        let started_at = Instant::now();

        // TODO: fix Tokio's API to not return an iterator.
        let stats = self.runtime.stats();
        let workers = stats.workers().map(Worker::new).collect();

        Iter {
            runtime: self.runtime.clone(),
            started_at,
            workers,
            num_remote_schedules: stats.remote_schedule_count(),
        }
    }
}

impl Worker {
    fn new(rt: &runtime::stats::WorkerStats) -> Worker {
        Worker {
            num_parks: rt.park_count(),
            num_noops: rt.noop_count(),
            num_steals: rt.steal_count(),
            num_local_schedules: rt.local_schedule_count(),
            num_polls: rt.poll_count(),
            total_time_busy: rt.total_busy_duration(),
        }
    }

    fn probe(&mut self, rt: &runtime::stats::WorkerStats, sample: &mut Sample) {
        macro_rules! metric {
            ( $sum:ident, $max:ident, $probe:ident ) => {{
                let val = rt.$probe();
                let delta = val - self.$sum;
                self.$sum = val;

                sample.$sum += delta;

                if delta > sample.$max {
                    sample.$max = delta;
                }
            }};
        }

        metric!(num_parks, max_parks, park_count);
        metric!(num_noops, max_noops, noop_count);
        metric!(num_steals, max_steals, steal_count);
        metric!(num_local_schedules, max_local_schedules, local_schedule_count);
        metric!(num_polls, max_polls, poll_count);
        metric!(total_time_busy, max_time_busy, total_busy_duration);
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