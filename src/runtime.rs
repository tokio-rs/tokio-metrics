use tokio::runtime;
use std::time::{Duration, Instant};

#[cfg(any(docsrs, all(tokio_unstable, feature = "rt")))]
#[cfg_attr(docsrs, doc(cfg(all(tokio_unstable, feature = "rt"))))]
/// Monitors key metrics of the tokio runtime.
/// 
/// ### Usage
/// ```
/// use std::time::Duration;
/// use tokio_metrics::RuntimeMonitor;
/// 
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///     let handle = tokio::runtime::Handle::current();
/// 
///     // print runtime metrics every 500ms
///     {
///         let runtime_monitor = RuntimeMonitor::new(&handle);
///         tokio::spawn(async move {
///             for interval in runtime_monitor.intervals() {
///                 // pretty-print the metric interval
///                 println!("{:?}", interval);
///                 // wait 500ms
///                 tokio::time::sleep(Duration::from_millis(500)).await;
///             }
///         });
///     }
/// 
///     // await some tasks
///     tokio::join![
///         do_work(),
///         do_work(),
///         do_work(),
///     ];
/// 
///     Ok(())
/// }
/// 
/// async fn do_work() {
///     for _ in 0..25 {
///         tokio::task::yield_now().await;
///         tokio::time::sleep(Duration::from_millis(100)).await;
///     }
/// }
/// ```
#[derive(Debug)]
pub struct RuntimeMonitor {
    /// Handle to the runtime
    runtime: runtime::RuntimeMetrics,
}

#[cfg(any(docsrs, all(tokio_unstable, feature = "rt")))]
#[cfg_attr(docsrs, doc(cfg(all(tokio_unstable, feature = "rt"))))]
/// Key runtime metrics.
#[non_exhaustive]
#[derive(Default, Debug, Clone, Copy)]
pub struct RuntimeMetrics {
    /// The number of worker threads used by the runtime.
    ///
    /// This metric is static for a runtime.
    /// 
    /// This metric is always equal to [`tokio::runtime::RuntimeMetrics::num_workers`].
    /// When using the `current_thread` runtime, the return value is always `1`.
    /// 
    /// The number of workers is set by configuring 
    /// [`worker_threads`][`tokio::runtime::Builder::worker_threads`] with
    /// [`tokio::runtime::Builder`], or by parameterizing [`tokio::main`].
    ///
    /// ##### Examples
    /// In the below example, the number of workers is set by parameterizing [`tokio::main`]:
    /// ```
    /// use tokio::runtime::Handle;
    ///
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 10)]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     assert_eq!(next_interval().workers_count, 10);
    /// }
    /// ```
    ///
    /// [`tokio::main`]: https://docs.rs/tokio/latest/tokio/attr.main.html
    /// 
    /// When using the `current_thread` runtime, the return value is always `1`; e.g.:
    /// ```
    /// use tokio::runtime::Handle;
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     assert_eq!(next_interval().workers_count, 1);
    /// }
    /// ```
    /// 
    /// This metric is always equal to [`tokio::runtime::RuntimeMetrics::num_workers`]; e.g.:
    /// ```
    /// use tokio::runtime::Handle;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let handle = Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    ///
    ///     assert_eq!(next_interval().workers_count, handle.metrics().num_workers());
    /// }
    /// ```
    pub workers_count: usize,

    /// The number of times worker threads parked.
    ///
    /// The worker park count increases by one each time the worker parks the thread waiting for 
    /// new inbound events to process. This usually means the worker has processed all pending work
    /// and is currently idle.
    ///
    /// ##### Definition
    /// This metric is derived from the sum of [`tokio::runtime::RuntimeMetrics::worker_park_count`]
    /// across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::max_park_count`]
    /// - [`RuntimeMetrics::min_park_count`]
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = next_interval(); // end of interval 1
    ///     assert_eq!(interval.total_park_count, 0);
    /// 
    ///     induce_parks().await;
    /// 
    ///     let interval = next_interval(); // end of interval 2
    ///     assert!(interval.total_park_count >= 1); // usually 1 or 2 parks
    /// }
    /// 
    /// async fn induce_parks() {
    ///     let _ = tokio::time::timeout(std::time::Duration::ZERO, async {
    ///         loop { tokio::task::yield_now().await; }
    ///     }).await;
    /// }
    /// ```
    pub total_park_count: u64,

    /// The maximum number of times any worker thread parked.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_park_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_park_count`]
    /// - [`RuntimeMetrics::min_park_count`]
    pub max_park_count: u64,

    /// The minimum number of times any worker thread parked.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_park_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_park_count`]
    /// - [`RuntimeMetrics::max_park_count`]
    pub min_park_count: u64,

    /// The number of times worker threads unparked but performed no work before parking again.
    ///
    /// The worker no-op count increases by one each time the worker unparks the thread but finds
    /// no new work and goes back to sleep. This indicates a false-positive wake up.
    /// 
    /// ##### Definition
    /// This metric is derived from the sum of [`tokio::runtime::RuntimeMetrics::worker_noop_count`] 
    /// across all worker threads.
    ///
    /// ##### Examples
    /// Unfortunately, there isn't a great way to reliably induce no-op parks, as they occur as
    /// false-positive events under concurrency.
    /// 
    /// The below example triggers fewer than two parks in the single-threaded runtime:
    /// ```
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     assert_eq!(next_interval().total_park_count, 0);
    ///     
    ///     async {
    ///         tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    ///     }.await;
    /// 
    ///     assert!(next_interval().total_park_count > 0);
    /// }
    /// ```
    /// 
    /// The below example triggers fewer than two parks in the multi-threaded runtime:
    /// ```
    /// #[tokio::main(flavor = "multi_thread")]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     assert_eq!(next_interval().total_noop_count, 0);
    ///     
    ///     async {
    ///         tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    ///     }.await;
    /// 
    ///     assert!(next_interval().total_noop_count > 0);
    /// }
    /// ```
    pub total_noop_count: u64,

    /// The maximum number of times any worker thread unparked but performed no work before parking
    /// again.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_noop_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_noop_count`]
    /// - [`RuntimeMetrics::min_noop_count`]
    pub max_noop_count: u64,

    /// The minimum number of times any worker thread unparked but performed no work before parking
    /// again.
    /// 
    /// ##### Definition
    /// This metric is derived from the minimum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_noop_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_noop_count`]
    /// - [`RuntimeMetrics::max_noop_count`]
    pub min_noop_count: u64,

    /// The number of tasks worker threads stole from another worker thread.
    ///
    /// The worker steal count starts increases by the amount of stolen tasks each time the worker
    /// has processed its scheduled queue and successfully steals more pending tasks from another
    /// worker.
    /// 
    /// This metric only applies to the **multi-threaded** runtime and will always return `0` when 
    /// using the current thread runtime.
    /// 
    /// ##### Definition
    /// This metric is derived from the sum of [`tokio::runtime::RuntimeMetrics::worker_steal_count`] for 
    /// all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::min_steal_count`]
    /// - [`RuntimeMetrics::max_steal_count`]
    ///
    /// ##### Examples
    /// In the below example, a blocking channel is used to backup one worker thread:
    /// ```
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = next_interval(); // end of first sampling interval
    ///     assert_eq!(interval.total_steal_count, 0);
    ///     assert_eq!(interval.min_steal_count, 0);
    ///     assert_eq!(interval.max_steal_count, 0);
    /// 
    ///     // induce a steal
    ///     async {
    ///         let (tx, rx) = std::sync::mpsc::channel();
    ///         // Move to the runtime.
    ///         tokio::spawn(async move {
    ///             // Spawn the task that sends to the channel
    ///             tokio::spawn(async move {
    ///                 tx.send(()).unwrap();
    ///             });
    ///             // Spawn a task that bumps the previous task out of the "next
    ///             // scheduled" slot.
    ///             tokio::spawn(async {});
    ///             // Blocking receive on the channel.
    ///             rx.recv().unwrap();
    ///             flush_metrics().await;
    ///         }).await.unwrap();
    ///         flush_metrics().await;
    ///     }.await;
    ///     
    ///     let interval = { flush_metrics().await; next_interval() }; // end of interval 2
    ///     assert_eq!(interval.total_steal_count, 1);
    ///     assert_eq!(interval.min_steal_count, 0);
    ///     assert_eq!(interval.max_steal_count, 1);
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end of interval 3
    ///     assert_eq!(interval.total_steal_count, 0);
    ///     assert_eq!(interval.min_steal_count, 0);
    ///     assert_eq!(interval.max_steal_count, 0);
    /// }
    /// 
    /// async fn flush_metrics() {
    ///     let _ = tokio::time::sleep(std::time::Duration::ZERO).await;
    /// }
    /// ```
    pub total_steal_count: u64,

    /// The maximum number of tasks any worker thread stole from another worker thread.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of [`tokio::runtime::RuntimeMetrics::worker_steal_count`] 
    /// across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_steal_count`]
    /// - [`RuntimeMetrics::min_steal_count`]
    pub max_steal_count: u64,

    /// The minimum number of tasks any worker thread stole from another worker thread.
    /// 
    /// ##### Definition
    /// This metric is derived from the minimum of [`tokio::runtime::RuntimeMetrics::worker_steal_count`] 
    /// across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_steal_count`]
    /// - [`RuntimeMetrics::max_steal_count`]
    pub min_steal_count: u64,

    /// The number of tasks scheduled from **outside** of the runtime.
    ///
    /// The remote schedule count increases by one each time a task is woken from **outside** of
    /// the runtime. This usually means that a task is spawned or notified from a non-runtime 
    /// thread and must be queued using the Runtime's injection queue, which tends to be slower.
    ///
    /// ##### Definition
    /// This metric is derived from [`tokio::runtime::RuntimeMetrics::remote_schedule_count`].
    ///
    /// ##### Examples
    /// In the below example, a remote schedule is induced by spawning a system thread, then
    /// spawning a tokio task from that system thread:
    /// ```
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = next_interval(); // end of first sampling interval
    ///     assert_eq!(interval.num_remote_schedules, 0);
    ///
    ///     // spawn a non-runtime thread
    ///     std::thread::spawn(move || {
    ///         // spawn two tasks from this non-runtime thread
    ///         async move {
    ///             handle.spawn(async {}).await;
    ///             handle.spawn(async {}).await;
    ///         }
    ///     }).join().unwrap().await;
    ///
    ///     let interval = next_interval(); // end of second sampling interval
    ///     assert_eq!(interval.num_remote_schedules, 2);
    /// 
    ///     let interval = next_interval(); // end of third sampling interval
    ///     assert_eq!(interval.num_remote_schedules, 0);
    /// }
    /// ```
    pub num_remote_schedules: u64,

    /// The number of tasks scheduled from worker threads.
    ///
    /// The local schedule count increases by one each time a task is woken from **inside** of the
    /// runtime. This usually means that a task is spawned or notified from within a runtime thread
    /// and will be queued on the worker-local queue.
    ///
    /// ##### Definition
    /// This metric is derived from the sum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_local_schedule_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::min_local_schedule_count`]
    /// - [`RuntimeMetrics::max_local_schedule_count`]
    ///
    /// ##### Examples
    /// ###### With `current_thread` runtime
    /// In the below example, two tasks are spawned from the context of a third tokio task:
    /// ```
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end interval 2
    ///     assert_eq!(interval.total_local_schedule_count, 0);
    /// 
    ///     let task = async {
    ///         tokio::spawn(async {}); // local schedule 1
    ///         tokio::spawn(async {}); // local schedule 2
    ///     };
    /// 
    ///     let handle = tokio::spawn(task); // local schedule 3
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end interval 2
    ///     assert_eq!(interval.total_local_schedule_count, 3);
    /// 
    ///     let _ = handle.await;
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end interval 3
    ///     assert_eq!(interval.total_local_schedule_count, 0);
    /// }
    /// 
    /// async fn flush_metrics() {
    ///     tokio::task::yield_now().await;
    /// }
    /// ```
    /// 
    /// ###### With `multi_thread` runtime
    /// In the below example, 100 tasks are spawned:
    /// ```
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = next_interval(); // end of interval 1
    ///     assert_eq!(interval.total_local_schedule_count, 0);
    /// 
    ///     use std::sync::atomic::{AtomicBool, Ordering};
    ///     static SPINLOCK: AtomicBool = AtomicBool::new(true);
    /// 
    ///     // block the other worker thread
    ///     tokio::spawn(async {
    ///         while SPINLOCK.load(Ordering::SeqCst) {}
    ///     });
    /// 
    ///     // FIXME: why does this need to be in a `spawn`?
    ///     let _ = tokio::spawn(async {
    ///         // spawn 100 tasks
    ///         for _ in 0..100 {
    ///             tokio::spawn(async {});
    ///         }
    ///         // this spawns 1 more task
    ///         flush_metrics().await;
    ///     }).await;
    /// 
    ///     // unblock the other worker thread
    ///     SPINLOCK.store(false, Ordering::SeqCst);
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end of interval 2
    ///     assert_eq!(interval.total_local_schedule_count, 100 + 1);
    /// }
    /// 
    /// async fn flush_metrics() {
    ///     let _ = tokio::time::sleep(std::time::Duration::ZERO).await;
    /// }
    /// ```
    pub total_local_schedule_count: u64,

    /// The maximum number of tasks scheduled from any one worker thread.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_local_schedule_count`] for all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_local_schedule_count`]
    /// - [`RuntimeMetrics::min_local_schedule_count`]
    pub max_local_schedule_count: u64,

    /// The minimum number of tasks scheduled from any one worker thread.
    /// 
    /// ##### Definition
    /// This metric is derived from the minimum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_local_schedule_count`] for all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_local_schedule_count`]
    /// - [`RuntimeMetrics::max_local_schedule_count`]
    pub min_local_schedule_count: u64,

    /// The number of times worker threads saturated their local queues.
    ///
    /// The worker steal count increases by one each time the worker attempts to schedule a task
    /// locally, but its local queue is full. When this happens, half of the
    /// local queue is moved to the injection queue.
    /// 
    /// This metric only applies to the **multi-threaded** scheduler.
    /// 
    /// ##### Definition
    /// This metric is derived from the sum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_overflow_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::min_overflow_count`]
    /// - [`RuntimeMetrics::max_overflow_count`]
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = next_interval(); // end of interval 1
    ///     assert_eq!(interval.total_overflow_count, 0);
    /// 
    ///     use std::sync::atomic::{AtomicBool, Ordering};
    ///     static SPINLOCK: AtomicBool = AtomicBool::new(true);
    /// 
    ///     // block the other worker thread
    ///     tokio::spawn(async {
    ///         while SPINLOCK.load(Ordering::SeqCst) {}
    ///     });
    /// 
    ///     // spawn a ton of tasks
    ///     let _ = tokio::spawn(async {
    ///         // we do this in a `tokio::spawn` because it is impossible to
    ///         // overflow the main task
    ///         for _ in 0..300 {
    ///             tokio::spawn(async {});
    ///         }
    ///     }).await;
    /// 
    ///     // unblock the other worker thread
    ///     SPINLOCK.store(false, Ordering::SeqCst);
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end of interval 2
    ///     assert_eq!(interval.total_overflow_count, 1);
    /// }
    /// 
    /// async fn flush_metrics() {
    ///     let _ = tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    /// }
    /// ```
    pub total_overflow_count: u64,

    /// The maximum number of times any one worker saturated its local queue.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_overflow_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_overflow_count`]
    /// - [`RuntimeMetrics::min_overflow_count`]
    pub max_overflow_count: u64,

    /// The minimum number of times any one worker saturated its local queue.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_overflow_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_overflow_count`]
    /// - [`RuntimeMetrics::max_overflow_count`]
    pub min_overflow_count: u64,

    /// The number of tasks that have been polled across all worker threads.
    ///
    /// The worker poll count increases by one each time a worker polls a scheduled task.
    /// 
    /// ##### Definition
    /// This metric is derived from the sum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_poll_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::min_polls_count`]
    /// - [`RuntimeMetrics::max_polls_count`]
    /// 
    /// ##### Examples
    /// In the below example, 42 tasks are spawned and polled:
    /// ```
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end of interval 1
    ///     assert_eq!(interval.total_polls_count, 0);
    ///     assert_eq!(interval.min_polls_count, 0);
    ///     assert_eq!(interval.max_polls_count, 0);
    /// 
    ///     const N: u64 = 42;
    /// 
    ///     for _ in 0..N {
    ///         let _ = tokio::spawn(async {}).await;
    ///     }
    /// 
    ///     let interval = { flush_metrics().await; next_interval() }; // end of interval 2
    ///     assert_eq!(interval.total_polls_count, N);
    ///     assert_eq!(interval.min_polls_count, N);
    ///     assert_eq!(interval.max_polls_count, N);
    /// }
    /// 
    /// async fn flush_metrics() {
    ///     let _ = tokio::task::yield_now().await;
    /// }
    /// ```
    pub total_polls_count: u64,

    /// The maximum number of tasks that have been polled in any worker thread.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_poll_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_polls_count`]
    /// - [`RuntimeMetrics::min_polls_count`]
    pub max_polls_count: u64,

    /// The minimum number of tasks that have been polled in any worker thread.
    /// 
    /// ##### Definition
    /// This metric is derived from the minimum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_poll_count`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_polls_count`]
    /// - [`RuntimeMetrics::max_polls_count`]
    pub min_polls_count: u64,

    /// The amount of time worker threads were busy.
    /// 
    /// The worker busy duration increases whenever the worker is spending time processing work.
    /// Using this value can indicate the total load of workers.
    /// 
    /// ##### Definition
    /// This metric is derived from the sum of
    /// [`tokio::runtime::RuntimeMetrics::worker_total_busy_duration`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::min_busy_duration`]
    /// - [`RuntimeMetrics::max_busy_duration`]
    ///
    /// ##### Examples
    /// In the below example, tasks spend a total of 3s busy:
    /// ```
    /// use tokio::time::Duration;
    /// 
    /// fn main() {
    ///     let start = tokio::time::Instant::now();
    /// 
    ///     let rt = tokio::runtime::Builder::new_current_thread()
    ///         .enable_all()
    ///         .build()
    ///         .unwrap();
    ///     
    ///     let handle = rt.handle();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let delay_1s = Duration::from_secs(1);
    ///     let delay_3s = Duration::from_secs(3);
    /// 
    ///     rt.block_on(async {
    ///         // keep the main task busy for 1s
    ///         spin_for(delay_1s);
    /// 
    ///         // spawn a task and keep it busy for 2s
    ///         let _ = tokio::spawn(async move { 
    ///             spin_for(delay_3s);
    ///         }).await;
    ///     });
    /// 
    ///     // flush metrics
    ///     drop(rt);
    /// 
    ///     let elapsed = start.elapsed();
    /// 
    ///     let interval =  next_interval(); // end of interval 2
    ///     assert!(interval.total_busy_duration >= delay_1s + delay_3s);
    ///     assert!(interval.total_busy_duration <= elapsed);
    /// }
    /// 
    /// fn time<F>(task: F) -> Duration
    /// where
    ///     F: Fn() -> ()
    /// {
    ///     let start = tokio::time::Instant::now();
    ///     task();
    ///     start.elapsed()
    /// }
    /// 
    /// /// Block the current thread for a given `duration`.
    /// fn spin_for(duration: Duration) {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    /// }
    /// ```
    /// 
    /// Busy times may not accumulate as the above example suggests (FIXME: Why?); e.g., if we
    /// remove the three second delay, the time spent busy falls to mere microseconds:
    /// ```should_panic
    /// use tokio::time::Duration;
    /// 
    /// fn main() {
    ///     let rt = tokio::runtime::Builder::new_current_thread()
    ///         .enable_all()
    ///         .build()
    ///         .unwrap();
    ///     
    ///     let handle = rt.handle();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let delay_1s = Duration::from_secs(1);
    /// 
    ///     let elapsed = time(|| rt.block_on(async {
    ///         // keep the main task busy for 1s
    ///         spin_for(delay_1s);
    ///     }));
    /// 
    ///     // flush metrics
    ///     drop(rt);
    /// 
    ///     let interval =  next_interval(); // end of interval 2
    ///     assert!(interval.total_busy_duration >= delay_1s); // FAIL
    ///     assert!(interval.total_busy_duration <= elapsed);
    /// }
    /// 
    /// fn time<F>(task: F) -> Duration
    /// where
    ///     F: Fn() -> ()
    /// {
    ///     let start = tokio::time::Instant::now();
    ///     task();
    ///     start.elapsed()
    /// }
    /// 
    /// /// Block the current thread for a given `duration`.
    /// fn spin_for(duration: Duration) {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    /// }
    /// ```
    pub total_busy_duration: Duration,

    /// The maximum amount of time a worker thread was busy.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of
    /// [`tokio::runtime::RuntimeMetrics::worker_total_busy_duration`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_busy_duration`]
    /// - [`RuntimeMetrics::min_busy_duration`]
    pub max_busy_duration: Duration,

    /// The minimum amount of time a worker thread was busy.
    /// 
    /// ##### Definition
    /// This metric is derived from the minimum of
    /// [`tokio::runtime::RuntimeMetrics::worker_total_busy_duration`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_busy_duration`]
    /// - [`RuntimeMetrics::max_busy_duration`]
    pub min_busy_duration: Duration,

    /// The number of tasks currently scheduled in the runtime's injection queue.
    ///
    /// Tasks that are spawned or notified from a non-runtime thread are scheduled using the 
    /// runtime's injection queue. This metric returns the **current** number of tasks pending in
    /// the injection queue. As such, the returned value may increase or decrease as new tasks are
    /// scheduled and processed.
    /// 
    /// ##### Definition
    /// This metric is derived from [`tokio::runtime::RuntimeMetrics::injection_queue_depth`].
    ///
    /// ##### Example
    /// ```
    /// # let current_thread = tokio::runtime::Builder::new_current_thread()
    /// #     .enable_all()
    /// #     .build()
    /// #     .unwrap();
    /// # 
    /// # let multi_thread = tokio::runtime::Builder::new_multi_thread()
    /// #     .worker_threads(2)
    /// #     .enable_all()
    /// #     .build()
    /// #     .unwrap();
    /// # 
    /// # for runtime in [current_thread, multi_thread] {
    /// let handle = runtime.handle().clone();
    /// let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    /// let mut intervals = monitor.intervals();
    /// let mut next_interval = || intervals.next().unwrap();
    /// 
    /// let interval = next_interval(); // end of interval 1
    /// assert_eq!(interval.num_remote_schedules, 0);
    /// 
    /// // spawn a system thread outside of the runtime
    /// std::thread::spawn(move || {
    ///     // spawn two tasks from this non-runtime thread
    ///     handle.spawn(async {});
    ///     handle.spawn(async {});
    /// }).join().unwrap();
    /// 
    /// // flush metrics
    /// drop(runtime);
    /// 
    /// let interval = next_interval(); // end of interval 2
    /// assert_eq!(interval.num_remote_schedules, 2);
    /// # }
    /// ```
    pub injection_queue_depth: usize,

    /// The total number of tasks currently scheduled in workers' local queues.
    ///
    /// Tasks that are spawned or notified from within a runtime thread are scheduled using that
    /// worker's local queue. This metric returns the **current** number of tasks pending in all
    /// workers' local queues. As such, the returned value may increase or decrease as new tasks
    /// are scheduled and processed.
    ///
    /// ##### Definition
    /// This metric is derived from [`tokio::runtime::RuntimeMetrics::worker_local_queue_depth`].
    ///
    /// ##### See also
    /// - [`RuntimeMetrics::min_local_queue_depth`]
    /// - [`RuntimeMetrics::max_local_queue_depth`]
    /// 
    /// ##### Example
    /// 
    /// ###### With `current_thread` runtime
    /// The below example spawns 100 tasks:
    /// ```
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     const N: usize = 100;
    /// 
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval =  next_interval(); // end of interval 1
    ///     assert_eq!(interval.total_local_queue_depth, 0);
    /// 
    /// 
    ///     for _ in 0..N {
    ///         tokio::spawn(async {});
    ///     }
    ///     let interval =  next_interval(); // end of interval 2
    ///     assert_eq!(interval.total_local_queue_depth, N);
    /// }
    /// ```
    /// 
    /// ###### With `multi_thread runtime
    /// The below example spawns 100 tasks:
    /// ```
    /// #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
    /// async fn main() {
    ///     const N: usize = 100;
    /// 
    ///     let handle = tokio::runtime::Handle::current();
    ///     let monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///     let mut intervals = monitor.intervals();
    ///     let mut next_interval = || intervals.next().unwrap();
    /// 
    ///     let interval =  next_interval(); // end of interval 1
    ///     assert_eq!(interval.total_local_queue_depth, 0);
    /// 
    ///     use std::sync::atomic::{AtomicBool, Ordering};
    ///     static SPINLOCK_A: AtomicBool = AtomicBool::new(true);
    /// 
    ///     // block the other worker thread
    ///     tokio::spawn(async {
    ///         while SPINLOCK_A.load(Ordering::SeqCst) {}
    ///     });
    /// 
    ///     static SPINLOCK_B: AtomicBool = AtomicBool::new(true);
    /// 
    ///     let _ = tokio::spawn(async {
    ///         for _ in 0..N {
    ///             tokio::spawn(async {
    ///                 while SPINLOCK_B.load(Ordering::SeqCst) {}
    ///             });
    ///         }
    ///     }).await;
    /// 
    ///     // unblock the other worker thread
    ///     SPINLOCK_A.store(false, Ordering::SeqCst);
    /// 
    ///     let interval =  next_interval(); // end of interval 2
    ///     assert_eq!(interval.total_local_queue_depth, N - 1);
    /// 
    ///     SPINLOCK_B.store(false, Ordering::SeqCst);
    /// }
    /// ```
    pub total_local_queue_depth: usize,

    /// The maximum number of tasks currently scheduled any worker's local queue.
    /// 
    /// ##### Definition
    /// This metric is derived from the maximum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_local_queue_depth`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_local_queue_depth`]
    /// - [`RuntimeMetrics::min_local_queue_depth`]
    pub max_local_queue_depth: usize,

    /// The minimum number of tasks currently scheduled any worker's local queue.
    /// 
    /// ##### Definition
    /// This metric is derived from the minimum of 
    /// [`tokio::runtime::RuntimeMetrics::worker_local_queue_depth`] across all worker threads.
    /// 
    /// ##### See also
    /// - [`RuntimeMetrics::total_local_queue_depth`]
    /// - [`RuntimeMetrics::max_local_queue_depth`]
    pub min_local_queue_depth: usize,

    /// Total amount of time elapsed since observing runtime metrics.
    pub elapsed: Duration,
}

/// Snapshot of per-worker metrics
#[derive(Debug)]
struct Worker {
    worker: usize,
    total_park_count: u64,
    total_noop_count: u64,
    total_steal_count: u64,
    total_local_schedule_count: u64,
    total_overflow_count: u64,
    total_polls_count: u64,
    total_busy_duration: Duration,
}

/// Iterator returned by [`RuntimeMonitor::intervals`].
///
/// See that method's documentation for more details.
#[derive(Debug)]
pub struct RuntimeIntervals {
    runtime: runtime::RuntimeMetrics,
    started_at: Instant,
    workers: Vec<Worker>,

    // Number of tasks scheduled from *outside* of the runtime
    num_remote_schedules: u64,
}

impl RuntimeIntervals {
    fn probe(&mut self) -> RuntimeMetrics {
        let now = Instant::now();

        let num_remote_schedules = self.runtime.remote_schedule_count();

        let mut metrics = RuntimeMetrics {
            workers_count: self.runtime.num_workers(),
            elapsed: now - self.started_at,
            injection_queue_depth: self.runtime.injection_queue_depth(),
            num_remote_schedules: num_remote_schedules - self.num_remote_schedules,
            min_park_count: u64::MAX,
            min_noop_count: u64::MAX,
            min_steal_count: u64::MAX,
            min_local_schedule_count: u64::MAX,
            min_overflow_count: u64::MAX,
            min_polls_count: u64::MAX,
            min_busy_duration: Duration::from_secs(1000000000),
            min_local_queue_depth: usize::MAX,
            ..Default::default()
        };

        self.num_remote_schedules = num_remote_schedules;
        self.started_at = now;

        for worker in &mut self.workers {
            worker.probe(&self.runtime, &mut metrics);
        }

        metrics
    }
}

impl Iterator for RuntimeIntervals {
    type Item = RuntimeMetrics;

    fn next(&mut self) -> Option<RuntimeMetrics> {
        Some(self.probe())
    }
}

impl RuntimeMonitor {
    pub fn new(runtime: &runtime::Handle) -> RuntimeMonitor {
        let runtime = runtime.metrics();

        RuntimeMonitor { runtime }
    }

    /// Produces an unending iterator of [`RuntimeMetrics`].
    ///
    /// Each sampling interval is defined by the time elapsed between advancements of the iterator
    /// produced by [`RuntimeMonitor::intervals`]. The item type of this iterator is [`RuntimeMetrics`],
    /// which is a bundle of runtime metrics that describe *only* changes occurring within that sampling
    /// interval.
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let handle = tokio::runtime::Handle::current();
    ///     // construct the runtime metrics monitor
    ///     let runtime_monitor = tokio_metrics::RuntimeMonitor::new(&handle);
    ///
    ///     // print runtime metrics every 500ms
    ///     {
    ///         tokio::spawn(async move {
    ///             for interval in runtime_monitor.intervals() {
    ///                 // pretty-print the metric interval
    ///                 println!("{:?}", interval);
    ///                 // wait 500ms
    ///                 tokio::time::sleep(Duration::from_millis(500)).await;
    ///             }
    ///         });
    ///     }
    ///
    ///     // await some tasks
    ///     tokio::join![
    ///         do_work(),
    ///         do_work(),
    ///         do_work(),
    ///     ];
    ///
    ///     Ok(())
    /// }
    ///
    /// async fn do_work() {
    ///     for _ in 0..25 {
    ///         tokio::task::yield_now().await;
    ///         tokio::time::sleep(Duration::from_millis(100)).await;
    ///     }
    /// }
    /// ```
    pub fn intervals(&self) -> RuntimeIntervals {
        let started_at = Instant::now();

        let workers = (0..self.runtime.num_workers())
            .map(|worker| Worker::new(worker, &self.runtime))
            .collect();

        RuntimeIntervals {
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
            total_park_count: rt.worker_park_count(worker),
            total_noop_count: rt.worker_noop_count(worker),
            total_steal_count: rt.worker_steal_count(worker),
            total_local_schedule_count: rt.worker_local_schedule_count(worker),
            total_overflow_count: rt.worker_overflow_count(worker),
            total_polls_count: rt.worker_poll_count(worker),
            total_busy_duration: rt.worker_total_busy_duration(worker),
        }
    }

    fn probe(&mut self, rt: &runtime::RuntimeMetrics, metrics: &mut RuntimeMetrics) {
        macro_rules! metric {
            ( $sum:ident, $max:ident, $min:ident, $probe:ident ) => {{
                let val = rt.$probe(self.worker);
                let delta = val - self.$sum;
                self.$sum = val;

                metrics.$sum += delta;

                if delta > metrics.$max {
                    metrics.$max = delta;
                }

                if delta < metrics.$min {
                    metrics.$min = delta;
                }
            }};
        }

        metric!(total_park_count, max_park_count, min_park_count, worker_park_count);
        metric!(total_noop_count, max_noop_count, min_noop_count, worker_noop_count);
        metric!(total_steal_count, max_steal_count, min_steal_count, worker_steal_count);
        metric!(total_local_schedule_count, max_local_schedule_count, min_local_schedule_count, worker_local_schedule_count);
        metric!(total_overflow_count, max_overflow_count, min_overflow_count, worker_overflow_count);
        metric!(total_polls_count, max_polls_count, min_polls_count, worker_poll_count);
        metric!(total_busy_duration, max_busy_duration, min_busy_duration, worker_total_busy_duration);

        // Local scheduled tasks is an absolute value

        let local_scheduled_tasks = rt.worker_local_queue_depth(self.worker);
        metrics.total_local_queue_depth += local_scheduled_tasks;

        if local_scheduled_tasks > metrics.max_local_queue_depth {
            metrics.max_local_queue_depth = local_scheduled_tasks;
        }

        if local_scheduled_tasks < metrics.min_local_queue_depth {
            metrics.min_local_queue_depth = local_scheduled_tasks;
        }
    }
}

impl RuntimeMetrics {
    pub fn mean_polls_per_park(&self) -> f64 {
        let total_park_count = self.total_park_count - self.total_noop_count;
        if total_park_count == 0 {
            0.0
        } else {
            self.total_polls_count as f64 / total_park_count as f64
        }
    }

    pub fn busy_ratio(&self) -> f64 {
        self.total_busy_duration.as_nanos() as f64 /
            self.elapsed.as_nanos() as f64
    }
}
