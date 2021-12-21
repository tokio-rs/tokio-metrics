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
        #[pin]
        task: T,
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

    /// Total amount of time all tasks spent in the scheduled state.
    pub total_scheduled_duration: Duration,
}

/// Tracks the metrics, shared across the various types.
struct Metrics {
    /// Instant at which the `InstrumentedTask` is created. This instant is used
    /// as the reference point for duration measurements.
    created_at: Instant,

    /// Total number of instrumented tasks
    num_tasks: AtomicU64,

    /// Total number of times the task was scheduled.
    num_scheduled: AtomicU64,

    /// Total amount of time the task has spent in the waking state.
    total_scheduled_dur: AtomicU64,
}

struct State {
    /// Where metrics should be recorded
    metrics: Arc<Metrics>,

    /// The instant, tracked as duration since `created_at`, at which the future
    /// was last woken. Tracked as nanoseconds.
    woke_at: AtomicU64,

    /// Waker to forward notifications to.
    waker: AtomicWaker,
}

impl TaskMetrics {
    pub fn new() -> TaskMetrics {
        TaskMetrics {
            metrics: Arc::new(Metrics {
                created_at: Instant::now(),
                num_tasks: AtomicU64::new(0),
                num_scheduled: AtomicU64::new(0),
                total_scheduled_dur: AtomicU64::new(0),
            }),
        }
    }

    pub fn instrument<F: Future>(&self, task: F) -> InstrumentedTask<F> {
        InstrumentedTask {
            task,
            state: Arc::new(State {
                metrics: self.metrics.clone(),
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

    /// Total duration that instrumented tasks spent scheduled, waiting to be
    /// executed.
    pub fn total_scheduled_duration(&self) -> Duration {
        let nanos = self.metrics.total_scheduled_dur.load(SeqCst);
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
                    total_scheduled_duration: Duration::from_nanos(
                        self.0.total_scheduled_dur.load(SeqCst),
                    ),
                };

                let ret = if let Some(prev) = self.1 {
                    Sample {
                        num_tasks: latest.num_tasks - prev.num_tasks,
                        num_scheduled: latest.num_scheduled - prev.num_scheduled,
                        total_scheduled_duration: latest.total_scheduled_duration
                            - prev.total_scheduled_duration,
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
    /// Average amount of time tasks spent in the scheduled state this sample.
    pub fn mean_scheduled_duration(&self) -> Duration {
        if self.num_scheduled == 0 {
            Duration::from_micros(0)
        } else {
            self.total_scheduled_duration / self.num_scheduled as _
        }
    }
}

impl<T: Future> InstrumentedTask<T> {
    // fn new(future: T) -> InstrumentedTask<T> {
    //     let state = Arc::new(State {
    //         created_at: Instant::now(),
    //         woke_at: AtomicU64::new(0),
    //         num_scheduled: AtomicU64::new(0),
    //         total_scheduled: AtomicU64::new(0),
    //         waker: AtomicWaker::new(),
    //     });

    //     // HAX
    //     let s = state.clone();
    //     std::thread::spawn(move || {
    //         let mut last_num_scheduled = 0;
    //         let mut last_total_scheduled = 0;
    //         loop {
    //             std::thread::sleep(Duration::from_millis(100));

    //             let num_scheduled = s.num_scheduled.load(Relaxed);
    //             let total_scheduled = s.total_scheduled.load(Relaxed);

    //             let delta_num = num_scheduled - last_num_scheduled;
    //             let delta_dur = Duration::from_nanos(total_scheduled - last_total_scheduled);

    //             if delta_num > 0 {
    //                 let mean = delta_dur / delta_num as _;

    //                 println!("num_scheduled = {}; total_scheduled = {:?}", delta_num, delta_dur);
    //                 println!("mean = {:?}", mean);
    //             }

    //             last_num_scheduled = num_scheduled;
    //             last_total_scheduled = total_scheduled;
    //         }
    //     });

    //     InstrumentedTask {
    //         task: future,
    //         state,
    //     }
    // }
}

impl<T: Future> Future for InstrumentedTask<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        this.state.measure_poll();

        // Register the waker
        this.state.waker.register(cx.waker());

        // Get the instrumented waker
        let waker_ref = futures_util::task::waker_ref(&this.state);
        let mut cx = Context::from_waker(&*waker_ref);

        // Store the waker
        Future::poll(this.task, &mut cx)
    }
}

impl State {
    fn measure_wake(&self) {
        let woke_at: u64 = match self.metrics.created_at.elapsed().as_nanos().try_into() {
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

        let scheduled_dur = (metrics.created_at + Duration::from_nanos(woke_at)).elapsed();
        let scheduled_dur: u64 = match scheduled_dur.as_nanos().try_into() {
            Ok(scheduled_dur) => scheduled_dur,
            Err(_) => return,
        };
        metrics.total_scheduled_dur.fetch_add(scheduled_dur, SeqCst);
        metrics.num_scheduled.fetch_add(1, SeqCst);
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
