use criterion::{black_box, criterion_group, criterion_main, Criterion};
use futures::task;
use tokio_metrics::TaskMonitor;
use std::iter;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};
use std::sync::{Arc, Barrier};

pub struct TestFuture;

impl Future for TestFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}


fn bench_poll(c: &mut Criterion) {
    c.bench_function("poll", move |b| {
        b.iter_custom(|iters| {
            let monitor = TaskMonitor::new();
            let num_cpus = num_cpus::get();
            let start = Arc::new(Barrier::new(num_cpus + 1));
            let stop = Arc::new(Barrier::new(num_cpus + 1));

            let mut workers: Vec<_> =
                iter::repeat((monitor, start.clone(), stop.clone()))
                    .take(num_cpus)
                    .map(|(monitor, start, stop)| {
                        thread::spawn(move || {
                            let waker = task::noop_waker();
                            let mut cx = Context::from_waker(&waker);
                            let mut instrumented = Box::pin(monitor.instrument(TestFuture));
                            start.wait();
                            let start_time = Instant::now();
                            for _i in 0..iters {
                                let _ = black_box(instrumented.as_mut().poll(&mut cx));
                            }
                            let stop_time = Instant::now();
                            stop.wait();
                            stop_time - start_time
                        })
                    })
                    .collect();

            start.wait();
            stop.wait();

            let elapsed: Duration = workers.drain(..).map(|w| w.join().unwrap()).sum();

            elapsed / (num_cpus as u32)
        })
    });
}

criterion_group!(benches, bench_poll);
criterion_main!(benches);