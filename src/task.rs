use futures_util::task::{AtomicWaker, ArcWake};
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::{Relaxed, SeqCst}};
use std::task::{Context, Poll};
use std::time::{Instant, Duration};

pin_project! {
    pub struct InstrumentedTask<T> {
        #[pin]
        future: T,
        state: Arc<State>,
    }
}

struct State {
    /// Instant at which the `InstrumentedTask` is created. This instant is used
    /// as the reference point for duration measurements.
    created_at: Instant,

    /// The instant, tracked as duration since `created_at`, at which the future
    /// was last woken. Tracked as microseconds.
    woke_at: AtomicU64,

    /// Total number of times the task was scheduled.
    num_scheduled: AtomicU64,

    /// Total amount of time the task has spent in the waking state.
    total_scheduled: AtomicU64,

    waker: AtomicWaker,
}

impl<T: Future> InstrumentedTask<T> {
    pub fn new(future: T) -> InstrumentedTask<T> {
        let state = Arc::new(State {
            created_at: Instant::now(),
            woke_at: AtomicU64::new(0),
            num_scheduled: AtomicU64::new(0),
            total_scheduled: AtomicU64::new(0),
            waker: AtomicWaker::new(),
        });

        // HAX
        let s = state.clone();
        std::thread::spawn(move || {
            let mut last_num_scheduled = 0;
            let mut last_total_scheduled = 0;
            loop {
                std::thread::sleep(Duration::from_secs(1));

                let num_scheduled = s.num_scheduled.load(Relaxed);
                let total_scheduled = s.total_scheduled.load(Relaxed);

                println!("num_scheduled = {}; total_scheduled = {:?}", num_scheduled - last_num_scheduled, Duration::from_micros(total_scheduled - last_total_scheduled));

                last_num_scheduled = num_scheduled;
                last_total_scheduled = total_scheduled;
            }
        });

        InstrumentedTask {
            future,
            state,
        }
    }
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
        Future::poll(this.future, &mut cx)
    }
}

impl State {
    fn measure_wake(&self) {
        let woke_at: u64 = match self.created_at.elapsed().as_micros().try_into() {
            Ok(woke_at) => woke_at,
            // This is highly unlikely as it would mean the task ran for over
            // 500,000 years. If you ran your service for 500,000 years. If you
            // are reading this 500,000 years in the future, I'm sorry.
            Err(_) => return,
        };

        // We don't actually care about the result
        let _ = self.woke_at.compare_exchange(0, woke_at, SeqCst, SeqCst);
    }

    fn measure_poll(&self) {
        let woke_at = self.woke_at.swap(0, SeqCst);

        if woke_at == 0 {
            // Either this is the first poll or it is a false-positive (polled
            // without scheduled).
            return;
        }

        let scheduled_dur = (self.created_at + Duration::from_micros(woke_at)).elapsed();
        let scheduled_dur: u64 = match scheduled_dur.as_micros().try_into() {
            Ok(scheduled_dur) => scheduled_dur,
            Err(_) => return,
        };
        let total_scheduled = self.total_scheduled.load(Relaxed) + scheduled_dur;
        self.total_scheduled.store(total_scheduled, Relaxed);

        let num_scheduled = self.num_scheduled.load(Relaxed) + 1;
        self.num_scheduled.store(num_scheduled, Relaxed);
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