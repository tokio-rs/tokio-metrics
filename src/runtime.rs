use tokio::runtime;

use std::time::{Duration, Instant};

pub struct RuntimeMetrics {
    /// Handle to the runtime
    runtime: runtime::RuntimeMetrics,
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

    pub min_parks: u64,

    /// Number of no-op ticks across all workers
    pub num_noops: u64,

    /// Maximum number of no-op ticks on any given worker
    pub max_noops: u64,

    pub min_noops: u64,

    /// Number of tasks stolen from other workers
    pub num_steals: u64,

    /// Maximum number of tasks any given worker stole from other workers
    pub max_steals: u64,

    pub min_steals: u64,

    /// Number of tasks scheduled from **outside** of the runtime
    pub num_remote_schedules: u64,

    /// Number of tasks scheduled locally across all workers
    pub num_local_schedules: u64,

    /// Maximum number of tasks scheduled locally on any given worker
    pub max_local_schedules: u64,

    pub min_local_schedules: u64,

    /// Number of tasks moved from a local queue to the global queue
    pub num_overflowed: u64,

    /// Maximum number of tasks any given worker moved from its local queue to
    /// the global queue.
    pub max_overflowed: u64,

    pub min_overflowed: u64,

    /// Number of times a task was polled across all workers
    pub num_polls: u64,

    /// Maximum number of tasks polled on any given worker
    pub max_polls: u64,

    pub min_polls: u64,

    /// Total amount of time all workers have been busy
    pub total_time_busy: Duration,

    /// Maximum amount of time any given worker has been busy
    pub max_time_busy: Duration,

    pub min_time_busy: Duration,

    pub remote_queue_depth: usize,

    pub num_local_scheduled_tasks: usize,

    pub max_local_scheduled_tasks: usize,

    pub min_local_scheduled_tasks: usize,

    /// Total amount of time elapsed since observing runtime metrics.
    pub total_time: Duration,
}

/// Snapshot of per-worker metrics
struct Worker {
    worker: usize,
    num_parks: u64,
    num_noops: u64,
    num_steals: u64,
    num_local_schedules: u64,
    num_overflowed: u64,
    num_polls: u64,
    total_time_busy: Duration,
}

impl RuntimeMetrics {
    pub fn new(runtime: &runtime::Handle) -> RuntimeMetrics {
        let runtime = runtime.metrics();

        RuntimeMetrics {
            runtime,
        }
    }

    pub fn sample(&self) -> impl Iterator<Item = Sample> {
        struct Iter {
            runtime: runtime::RuntimeMetrics,
            started_at: Instant,
            workers: Vec<Worker>,

            // Number of tasks scheduled from *outside* of the runtime
            num_remote_schedules: u64,
        }

        impl Iter {
            fn probe(&mut self) -> Sample {
                let now = Instant::now();

                let num_remote_schedules = self.runtime.remote_schedule_count();

                let mut sample = Sample {
                    num_workers: self.runtime.num_workers(),
                    total_time: now - self.started_at,
                    remote_queue_depth: self.runtime.remote_queue_depth(),
                    num_remote_schedules: num_remote_schedules - self.num_remote_schedules,
                    min_parks: u64::MAX,
                    min_noops: u64::MAX,
                    min_steals: u64::MAX,
                    min_local_schedules: u64::MAX,
                    min_overflowed: u64::MAX,
                    min_polls: u64::MAX,
                    min_time_busy: Duration::from_secs(1000000000),
                    min_local_scheduled_tasks: usize::MAX,
                    .. Default::default()
                };

                self.num_remote_schedules = num_remote_schedules;
                self.started_at = now;

                for worker in &mut self.workers {
                    worker.probe(&self.runtime, &mut sample);
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

        let workers = (0..self.runtime.num_workers())
            .map(|worker| Worker::new(worker, &self.runtime)).collect();

        Iter {
            runtime: self.runtime.clone(),
            started_at,
            workers,
            num_remote_schedules: self.runtime.remote_schedule_count(),
        }
    }
}

impl Worker {
    fn new(worker: usize, rt: &runtime::RuntimeMetrics) -> Worker {
        Worker {
            worker,
            num_parks: rt.worker_park_count(worker),
            num_noops: rt.worker_noop_count(worker),
            num_steals: rt.worker_steal_count(worker),
            num_local_schedules: rt.worker_local_schedule_count(worker),
            num_overflowed: rt.worker_overflow_count(worker),
            num_polls: rt.worker_poll_count(worker),
            total_time_busy: rt.worker_total_busy_duration(worker),
        }
    }

    fn probe(&mut self, rt: &runtime::RuntimeMetrics, sample: &mut Sample) {
        macro_rules! metric {
            ( $sum:ident, $max:ident, $min:ident, $probe:ident ) => {{
                let val = rt.$probe(self.worker);
                let delta = val - self.$sum;
                self.$sum = val;

                sample.$sum += delta;

                if delta > sample.$max {
                    sample.$max = delta;
                }

                if delta < sample.$min {
                    sample.$min = delta;
                }
            }};
        }

        metric!(num_parks, max_parks, min_parks, worker_park_count);
        metric!(num_noops, max_noops, min_noops, worker_noop_count);
        metric!(num_steals, max_steals, min_steals, worker_steal_count);
        metric!(num_local_schedules, max_local_schedules, min_local_schedules, worker_local_schedule_count);
        metric!(num_overflowed, max_overflowed, min_overflowed, worker_overflow_count);
        metric!(num_polls, max_polls, min_polls, worker_poll_count);
        metric!(total_time_busy, max_time_busy, min_time_busy, worker_total_busy_duration);

        // Local scheduled tasks is an absolute value

        let local_scheduled_tasks = rt.worker_local_queue_depth(self.worker);
        sample.num_local_scheduled_tasks += local_scheduled_tasks;

        if local_scheduled_tasks > sample.max_local_scheduled_tasks {
            sample.max_local_scheduled_tasks = local_scheduled_tasks;
        }

        if local_scheduled_tasks < sample.min_local_scheduled_tasks {
            sample.min_local_scheduled_tasks = local_scheduled_tasks;
        }
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