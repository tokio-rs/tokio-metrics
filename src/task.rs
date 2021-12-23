use futures_util::task::{ArcWake, AtomicWaker};
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

pub struct TaskMetrics {
    metrics: Arc<Metrics>,
}

pin_project! {
    pub struct InstrumentedTask<T> {
        // The task being instrumented
        #[pin]
        task: T,

        // True when the task is polled for the first time
        did_poll_once: bool,

        // State shared between the task and its instrumented waker.
        state: Arc<State>,
    }
}

/// Change in task metrics since the previous iteration
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Number of new instrumented tasks
    pub num_tasks: u64,

    /// Number of times a task was scheduled.
    pub num_scheduled: u64,

    /// Number of times a task was polled fast
    pub num_fast_polls: u64,

    /// Number of times a task was polled slow
    pub num_slow_polls: u64,

    /// Total amount of time all tasks spent until being polled for the first
    /// time.
    pub total_time_to_first_poll: Duration,

    /// Total amount of time all tasks spent in the scheduled state.
    pub total_time_scheduled: Duration,

    /// Total amount of time tasks were polled fast
    pub total_time_fast_poll: Duration,

    /// Total amount of time tasks were polled slow
    pub total_time_slow_poll: Duration,
}

/// Tracks the metrics, shared across the various types.
struct Metrics {
    /// A task poll takes longer than this, it is considered a slow poll.
    slow_poll_cut_off: Duration,

    /// Total number of instrumented tasks
    num_tasks: AtomicU64,

    /// Total number of times a task was scheduled.
    num_scheduled: AtomicU64,

    /// Total number of times a task was polled fast
    num_fast_polls: AtomicU64,

    /// Total number of times a task was polled slow
    num_slow_polls: AtomicU64,

    /// Total amount of time until the first poll
    total_time_to_first_poll: AtomicU64,

    /// Total amount of time a task has spent in the waking state.
    total_time_scheduled: AtomicU64,

    /// Total amount of time a task has spent being polled below the slow cut off.
    total_time_fast_poll: AtomicU64,

    /// Total amount of time a task has spent being polled above the slow cut off.
    total_time_slow_poll: AtomicU64,
}

struct State {
    /// Where metrics should be recorded
    metrics: Arc<Metrics>,

    /// Instant at which the task was instrumented. This is used to track the time to first poll.
    instrumented_at: Instant,

    /// The instant, tracked as duration since `created_at`, at which the future
    /// was last woken. Tracked as nanoseconds.
    woke_at: AtomicU64,

    /// Waker to forward notifications to.
    waker: AtomicWaker,
}

impl TaskMetrics {
    pub fn new() -> TaskMetrics {
        TaskMetrics::with_slow_poll_cut_off(Duration::from_micros(10))
    }

    pub fn with_slow_poll_cut_off(slow_poll_cut_off: Duration) -> TaskMetrics {
        TaskMetrics {
            metrics: Arc::new(Metrics {
                slow_poll_cut_off,
                num_tasks: AtomicU64::new(0),
                num_scheduled: AtomicU64::new(0),
                num_fast_polls: AtomicU64::new(0),
                num_slow_polls: AtomicU64::new(0),
                total_time_to_first_poll: AtomicU64::new(0),
                total_time_scheduled: AtomicU64::new(0),
                total_time_fast_poll: AtomicU64::new(0),
                total_time_slow_poll: AtomicU64::new(0),
            }),
        }
    }

    pub fn instrument<F: Future>(&self, task: F) -> InstrumentedTask<F> {
        InstrumentedTask {
            task,
            did_poll_once: false,
            state: Arc::new(State {
                metrics: self.metrics.clone(),
                instrumented_at: Instant::now(),
                woke_at: AtomicU64::new(0),
                waker: AtomicWaker::new(),
            }),
        }
    }

    /// Total number of spawned tasks
    pub fn num_tasks(&self) -> u64 {
        self.metrics.num_tasks.load(SeqCst)
    }

    /// Total number of times a task was scheduled.
    pub fn num_scheduled(&self) -> u64 {
        self.metrics.num_scheduled.load(SeqCst)
    }

    /// Total number of task polls that were below the "slow task" cut off.
    pub fn num_fast_polls(&self) -> u64 {
        self.metrics.num_fast_polls.load(SeqCst)
    }

    /// Total number of task polls that were above the "slow task" cut off.
    pub fn num_slow_polls(&self) -> u64 {
        self.metrics.num_slow_polls.load(SeqCst)
    }

    pub fn total_time_to_first_poll(&self) -> Duration {
        let nanos = self.metrics.total_time_to_first_poll.load(SeqCst);
        Duration::from_nanos(nanos)
    }

    /// Total duration that instrumented tasks spent scheduled, waiting to be
    /// executed.
    pub fn total_time_scheduled(&self) -> Duration {
        let nanos = self.metrics.total_time_scheduled.load(SeqCst);
        Duration::from_nanos(nanos)
    }

    /// An iterator that samples the change in metrics each iteration.
    pub fn sample(&self) -> impl Iterator<Item = Sample> {
        struct Iter(Arc<Metrics>, Option<Sample>);

        impl Iterator for Iter {
            type Item = Sample;

            fn next(&mut self) -> Option<Sample> {
                let latest = Sample {
                    num_tasks: self.0.num_tasks.load(SeqCst),
                    num_scheduled: self.0.num_scheduled.load(SeqCst),
                    num_fast_polls: self.0.num_fast_polls.load(SeqCst),
                    num_slow_polls: self.0.num_slow_polls.load(SeqCst),
                    total_time_to_first_poll: Duration::from_nanos(
                        self.0.total_time_to_first_poll.load(SeqCst),
                    ),
                    total_time_scheduled: Duration::from_nanos(
                        self.0.total_time_scheduled.load(SeqCst),
                    ),
                    total_time_fast_poll: Duration::from_nanos(
                        self.0.total_time_fast_poll.load(SeqCst),
                    ),
                    total_time_slow_poll: Duration::from_nanos(
                        self.0.total_time_slow_poll.load(SeqCst),
                    ),
                };

                let ret = if let Some(prev) = self.1 {
                    Sample {
                        num_tasks: latest.num_tasks - prev.num_tasks,
                        num_scheduled: latest.num_scheduled - prev.num_scheduled,
                        num_fast_polls: latest.num_fast_polls - prev.num_fast_polls,
                        num_slow_polls: latest.num_slow_polls - prev.num_slow_polls,
                        total_time_to_first_poll: latest.total_time_to_first_poll
                            - prev.total_time_to_first_poll,
                        total_time_scheduled: latest.total_time_scheduled
                            - prev.total_time_scheduled,
                        total_time_fast_poll: latest.total_time_fast_poll
                            - prev.total_time_fast_poll,
                        total_time_slow_poll: latest.total_time_slow_poll
                            - prev.total_time_slow_poll,
                    }
                } else {
                    latest
                };

                self.1 = Some(latest);

                Some(ret)
            }
        }

        Iter(self.metrics.clone(), None)
    }
}

impl Sample {
    /// Average amount of time new tasks have spent until polled for the first time.
    pub fn mean_time_to_first_poll(&self) -> Duration {
        if self.num_tasks == 0 {
            Duration::from_micros(0)
        } else {
            self.total_time_to_first_poll / self.num_tasks as _
        }
    }

    /// Average amount of time tasks spent in the scheduled state this sample.
    pub fn mean_time_scheduled(&self) -> Duration {
        if self.num_scheduled == 0 {
            Duration::from_micros(0)
        } else {
            self.total_time_scheduled / self.num_scheduled as _
        }
    }

    pub fn fast_poll_ratio(&self) -> f64 {
        self.num_fast_polls as f64 / (self.num_fast_polls + self.num_slow_polls) as f64
    }

    pub fn mean_fast_polls(&self) -> Duration {
        if self.num_fast_polls == 0 {
            Duration::from_micros(0)
        } else {
            self.total_time_fast_poll / self.num_fast_polls as _
        }
    }

    pub fn mean_slow_polls(&self) -> Duration {
        if self.num_slow_polls == 0 {
            Duration::from_micros(0)
        } else {
            self.total_time_slow_poll / self.num_slow_polls as _
        }
    }
}

impl<T: Future> Future for InstrumentedTask<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if !*this.did_poll_once {
            *this.did_poll_once = true;

            if let Ok(nanos) = this.state.instrumented_at.elapsed().as_nanos().try_into() {
                let nanos: u64 = nanos; // Make inference happy
                this.state
                    .metrics
                    .total_time_to_first_poll
                    .fetch_add(nanos, SeqCst);
                this.state.metrics.num_tasks.fetch_add(1, SeqCst);
            }
        }

        this.state.measure_poll();

        // Register the waker
        this.state.waker.register(cx.waker());

        // Get the instrumented waker
        let waker_ref = futures_util::task::waker_ref(&this.state);
        let mut cx = Context::from_waker(&*waker_ref);

        // Poll the task
        let now = Instant::now();
        let ret = Future::poll(this.task, &mut cx);
        this.state.measure_poll_time(now.elapsed());
        ret
    }
}

impl State {
    fn measure_wake(&self) {
        let woke_at: u64 = match self.instrumented_at.elapsed().as_nanos().try_into() {
            Ok(woke_at) => woke_at,
            // This is highly unlikely as it would mean the task ran for over
            // 500 years. If you ran your service for 500 years. If you are
            // reading this 500 years in the future, I'm sorry.
            Err(_) => return,
        };

        // We don't actually care about the result
        let _ = self.woke_at.compare_exchange(0, woke_at, SeqCst, SeqCst);
    }

    fn measure_poll(&self) {
        let metrics = &self.metrics;
        let woke_at = self.woke_at.swap(0, SeqCst);

        if woke_at == 0 {
            // Either this is the first poll or it is a false-positive (polled
            // without scheduled).
            return;
        }

        let scheduled_dur = (self.instrumented_at + Duration::from_nanos(woke_at)).elapsed();
        let scheduled_nanos: u64 = match scheduled_dur.as_nanos().try_into() {
            Ok(scheduled_nanos) => scheduled_nanos,
            Err(_) => return,
        };

        metrics
            .total_time_scheduled
            .fetch_add(scheduled_nanos, SeqCst);
        metrics.num_scheduled.fetch_add(1, SeqCst);
    }

    fn measure_poll_time(&self, duration: Duration) {
        let metrics = &self.metrics;
        let polled_nanos: u64 = match duration.as_nanos().try_into() {
            Ok(polled_nanos) => polled_nanos,
            Err(_) => return,
        };

        if duration >= self.metrics.slow_poll_cut_off {
            metrics.total_time_slow_poll.fetch_add(polled_nanos, SeqCst);
        } else {
            metrics.total_time_fast_poll.fetch_add(polled_nanos, SeqCst);
        }

        metrics.num_fast_polls.fetch_add(1, SeqCst);
    }
}

impl ArcWake for State {
    fn wake_by_ref(arc_self: &Arc<State>) {
        arc_self.measure_wake();
        arc_self.waker.wake();
    }

    fn wake(self: Arc<State>) {
        self.measure_wake();
        self.waker.wake();
    }
}
