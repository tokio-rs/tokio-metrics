use futures_util::task::{ArcWake, AtomicWaker};
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{Duration, Instant};

/// Monitors key metrics of instrumented tasks.
///
/// ### Usage
/// A [`TaskMonitor`] tracks key [metrics][TaskMetrics] of async tasks that have been
/// [instrumented][`TaskMonitor::instrument`] with the monitor.
///
/// In the below example, a [`TaskMonitor`] is [constructed][TaskMonitor::new] and used to
/// [instrument][TaskMonitor::instrument] three worker tasks; meanwhile, a fourth task
/// prints [metrics][TaskMetrics] in 500ms [intervals][TaskMonitor::intervals].
/// ```
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///     // construct a metrics monitor
///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
///
///     // print task metrics every 500ms
///     {
///         let metrics_monitor = metrics_monitor.clone();
///         tokio::spawn(async move {
///             for interval in metrics_monitor.intervals() {
///                 // pretty-print the metric interval
///                 println!("{:?}", interval);
///                 // wait 500ms
///                 tokio::time::sleep(Duration::from_millis(500)).await;
///             }
///         });
///     }
///
///     // instrument some tasks and await them
///     // note that the same TaskMonitor can be used for multiple tasks
///     tokio::join![
///         metrics_monitor.instrument(do_work()),
///         metrics_monitor.instrument(do_work()),
///         metrics_monitor.instrument(do_work())
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
///
/// ### Best practices
/// In long-running services that spawn many tasks, favor collecting [`TaskMetrics`] in frequently-sampled
/// [intervals][`TaskMonitor::intervals`], instead of directly inspecting [cumulative][`TaskMonitor::cumulative`]
/// metrics. So long as the sampling period is frequent enough that event counters and durations do
/// not have time to overflow, interval-sampled metrics will remain accurate even if the underlying
/// cumulative metrics overflow.
///
/// ### Limitations
/// The [`TaskMetrics`] type uses [`u64`] to represent both event counters and durations (measured in nanoseconds).
/// Consequently, event counters are accurate for ≤ [`u64::MAX`] events, and durations are accurate for ≤ [`u64::MAX`]
/// nanoseconds.
///
/// The counters and durations of [`TaskMetrics`] produced by [`TaskMonitor::cumulative`] increase
/// monotonically with each successive invocation of [`TaskMonitor::cumulative`]. Upon overflow, counters
/// and durations wrap.
///
/// The counters and durations of [`TaskMetrics`] produced by [`TaskMonitor::intervals`] are calculated by
/// computing the difference of metrics in successive invocations of [`TaskMonitor::cumulative`]. If,
/// within a monitoring interval, an event occurs more than [`u64::MAX`] times, or a monitored duration
/// exceeds [`u64::MAX`] nanoseconds, the metrics for that interval will overflow and not be accurate.
///
/// ##### Overflow examples
/// Consier the [`TaskMetrics::total_first_poll_delay`] metric. This metric accurately reflects delays
/// between instrumentation and first-poll ≤ [`u64::MAX`] nanoseconds:
/// ```
/// use tokio::time::Duration;
///
/// #[tokio::main(flavor = "current_thread", start_paused = true)]
/// async fn main() {
///     let monitor = tokio_metrics::TaskMonitor::new();
///     let mut interval = monitor.intervals();
///     let mut next_interval = || interval.next().unwrap();
///
///     // construct and instrument a task, but do not `await` it
///     let task = monitor.instrument(async {});
///
///     // this is the maximum duration representable by tokio_metrics
///     let max_duration = Duration::from_nanos(u64::MAX);
///
///     // let's advance the clock by this amount and poll `task`
///     let _ = tokio::time::advance(max_duration).await;
///     task.await;
///
///     // durations ≤ `max_duration` are accurately reflected in this metric
///     assert_eq!(next_interval().total_first_poll_delay, max_duration);
///     assert_eq!(monitor.cumulative().total_first_poll_delay, max_duration);
/// }
/// ```
/// If the total delay between instrumentation and first poll exceeds [`u64::MAX`] nanoseconds,
/// `total_first_poll_delay` will overflow:
/// ```
/// # use tokio::time::Duration;
/// #
/// # #[tokio::main(flavor = "current_thread", start_paused = true)]
/// # async fn main() {
/// #    let monitor = tokio_metrics::TaskMonitor::new();
/// #
///  // construct and instrument a task, but do not `await` it
///  let task_a = monitor.instrument(async {});
///  let task_b = monitor.instrument(async {});
///
///  // this is the maximum duration representable by tokio_metrics
///  let max_duration = Duration::from_nanos(u64::MAX);
///
///  // let's advance the clock by 1.5x this amount and await `task`
///  let _ = tokio::time::advance(3 * (max_duration / 2)).await;
///  task_a.await;
///  task_b.await;
///
///  // the `total_first_poll_delay` has overflowed
///  assert!(monitor.cumulative().total_first_poll_delay < max_duration);
/// # }
/// ```
/// If *many* tasks are spawned, it will take far less than a [`u64::MAX`]-nanosecond delay bring this metric to the
/// precipice of overflow:
/// ```
/// # use tokio::time::Duration;
/// #
/// # #[tokio::main(flavor = "current_thread", start_paused = true)]
/// # async fn main() {
/// #     let monitor = tokio_metrics::TaskMonitor::new();
/// #     let mut interval = monitor.intervals();
/// #     let mut next_interval = || interval.next().unwrap();
/// #
/// // construct and instrument u16::MAX tasks, but do not `await` them
/// let total_first_poll_count = u16::MAX as u64;
/// let mut tasks = Vec::with_capacity(total_first_poll_count as usize);
/// for _ in 0..total_first_poll_count { tasks.push(monitor.instrument(async {})); }
///
/// // this is the maximum duration representable by tokio_metrics
/// let max_duration = u64::MAX;
///
/// // let's advance the clock justenough such that all of the time-to-first-poll
/// // delays summed nearly equals `max_duration_nanos`, less some remainder...
/// let iffy_delay = max_duration / (total_first_poll_count as u64);
/// let small_remainder = max_duration % total_first_poll_count;
/// let _ = tokio::time::advance(Duration::from_nanos(iffy_delay)).await;
///
/// // ...then poll all of the instrumented tasks:
/// for task in tasks { task.await; }
///
/// // `total_first_poll_delay` is at the precipice of overflowing!
/// assert_eq!(next_interval().total_first_poll_delay.as_nanos(), (max_duration - small_remainder) as u128);
/// assert_eq!(monitor.cumulative().total_first_poll_delay.as_nanos(), (max_duration - small_remainder) as u128);
/// # }
/// ```
/// Frequent, interval-sampled metrics will retain their accuracy, even if the cumulative
/// metrics counter overflows at most once in the midst of an interval:
/// ```
/// # use tokio::time::Duration;
/// # use tokio_metrics::TaskMonitor;
/// #
/// # #[tokio::main(flavor = "current_thread", start_paused = true)]
/// # async fn main() {
/// #     let monitor = TaskMonitor::new();
/// #     let mut interval = monitor.intervals();
/// #     let mut next_interval = || interval.next().unwrap();
///
///  let total_first_poll_count = u16::MAX as u64;
///  let batch_size = total_first_poll_count / 3;
///
///  let max_duration_ns = u64::MAX;
///  let iffy_delay_ns = max_duration_ns / total_first_poll_count;
///
///  // Instrument `batch_size` number of tasks, wait for `delay` nanoseconds,
///  // then await the instrumented tasks.
///  async fn run_batch(monitor: &TaskMonitor, batch_size: usize, delay: u64) {
///      let mut tasks = Vec::with_capacity(batch_size);
///      for _ in 0..batch_size { tasks.push(monitor.instrument(async {})); }
///      let _ = tokio::time::advance(Duration::from_nanos(delay)).await;
///      for task in tasks { task.await; }
///  }
///
///  // this is how much `total_time_to_first_poll_ns` will
///  // increase with each batch we run
///  let batch_delay = iffy_delay_ns * batch_size;
///
///  // run batches 1, 2, and 3
///  for i in 1..=3 {
///      run_batch(&monitor, batch_size as usize, iffy_delay_ns).await;
///      assert_eq!(1 * batch_delay as u128, next_interval().total_first_poll_delay.as_nanos());
///      assert_eq!(i * batch_delay as u128, monitor.cumulative().total_first_poll_delay.as_nanos());
///  }
///
///  /* now, the `total_time_to_first_poll_ns` counter is at the precipice of overflow */
///  assert_eq!(monitor.cumulative().total_first_poll_delay.as_nanos(), max_duration_ns as u128);
///
///  // run batch 4
///  run_batch(&monitor, batch_size as usize, iffy_delay_ns).await;
///  // the interval counter remains accurate
///  assert_eq!(1 * batch_delay as u128, next_interval().total_first_poll_delay.as_nanos());
///  // but the cumulative counter has overflowed
///  assert_eq!(batch_delay as u128 - 1, monitor.cumulative().total_first_poll_delay.as_nanos());
/// # }
/// ```
/// If a cumulative metric overflows *more than once* in the midst of an interval,
/// its interval-sampled counterpart will also overflow.
#[derive(Clone)]
pub struct TaskMonitor {
    metrics: Arc<RawMetrics>,
}

pin_project! {
    /// An async task that has been instrumented with [`TaskMonitor::instrument`].
    pub struct Instrumented<T> {
        // The task being instrumented
        #[pin]
        task: T,

        // True when the task is polled for the first time
        did_poll_once: bool,

        // The instant, tracked as nanoseconds since `instrumented_at`, at which the future finished
        // its last poll.
        idled_at: u64,

        // State shared between the task and its instrumented waker.
        state: Arc<State>,
    }
}

/// Key metrics of [instrumented][`TaskMonitor::instrument`] tasks.
///
/// ### Index of metrics
/// #### Base metrics
/// - [`TaskMetrics::total_first_poll_count`]:
///     number of new tasks instrumented and polled at least once
/// - [`TaskMetrics::total_scheduled_count`]:
///     number of times instrumented tasks were scheduled for execution
/// - [`TaskMetrics::total_fast_poll_count`]:
///     number of times that polling instrumented tasks completed swiftly
/// - [`TaskMetrics::total_slow_poll_count`]:
///     number of times that polling instrumented tasks completed slowly
/// - [`TaskMetrics::total_first_poll_delay`]
///     total amount of time elapsed between task instrumentation and first poll
/// - [`TaskMetrics::total_scheduled_duration`]
///     total amount of time tasks spent waiting to be scheduled
/// - [`TaskMetrics::total_fast_poll_duration`]
///     total amount of time that fast polls took to complete
/// - [`TaskMetrics::total_slow_poll_duration`]
///     total amount of time that slow polls took to complete
///
/// #### Derived metrics
/// These metrics are derived from [`TaskMetrics`]'s base metrics:
/// - [`TaskMetrics::mean_time_to_first_poll`]:
///     mean amount of time elapsed between task instrumentation and first poll
/// - [`TaskMetrics::mean_time_scheduled`]:
///     mean amount of time that monitored tasks spent waiting to be run
/// - [`TaskMetrics::fast_poll_ratio`]:
///     ratio between the number polls categorized as fast and slow
/// - [`TaskMetrics::mean_fast_polls`]:
///     mean time consumed by fast polls of monitored tasks
/// - [`TaskMetrics::mean_slow_polls`]:
///     mean time consumed by slow polls of monitored tasks
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskMetrics {
    /// The number of new tasks instrumented and polled at least once.
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_time_to_first_poll`]:
    ///   the mean time elapsed between the instrumentation of tasks and the time they are first polled.
    ///
    /// ### Interpretation
    /// This metric, itself, is not a critical metric to log; it is used to calculate
    /// the critical metric [`TaskMetrics::mean_time_to_first_poll`].
    ///
    /// ### Example
    /// In the below example, no tasks are instrumented or polled in the first sampling period;
    /// one task is instrumented, but not polled, in the second sampling period; that task is awaited
    /// to completion (and, thus, polled at least once) in the third sampling period; no additional
    /// tasks are polled for the first time within the fourth sampling period:
    /// ```
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, and polled at least once
    ///     assert_eq!(next_interval().total_first_poll_count, 0);
    ///
    ///     let task = metrics_monitor.instrument(async {});
    ///
    ///     // `task` has been constructed and instrumented, but has not yet been polled
    ///     assert_eq!(next_interval().total_first_poll_count, 0);
    ///
    ///     // poll `task` to completion
    ///     task.await;
    ///
    ///     // `task` has been constructed, instrumented, and polled at least once
    ///     assert_eq!(next_interval().total_first_poll_count, 1);
    ///
    ///     // since the last interval was produced, 0 tasks have been constructed, instrumented and polled
    ///     assert_eq!(next_interval().total_first_poll_count, 0);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub total_first_poll_count: u64,

    /// The amount of time elapsed between when tasks were instrumented and when they were first polled.
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_time_to_first_poll`]:
    ///   the mean time elapsed between the instrumentation of tasks and the time they are first polled.
    ///
    /// ### Example
    /// In the below example, 0 tasks have been instrumented or polled within the first sampling period,
    /// a total of 500ms elapse between the instrumentation and polling of tasks within the second
    /// sampling period, and a total of 350ms elapse between the instrumentation and polling of tasks
    /// within the third sampling period:
    /// ```
    /// use core::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have yet been created, instrumented, or polled
    ///     assert_eq!(metrics_monitor.cumulative().total_first_poll_delay, Duration::ZERO);
    ///     assert_eq!(next_interval().total_first_poll_delay, Duration::ZERO);
    ///
    ///     // constructs and instruments a task, pauses a given duration, then awaits the task
    ///     async fn instrument_pause_await(monitor: &tokio_metrics::TaskMonitor, pause: Duration) {
    ///         let task = monitor.instrument(async move {});
    ///         tokio::time::sleep(pause).await;
    ///         task.await;
    ///     }
    ///
    ///     // construct and await a task that pauses for 500ms between instrumentation and first poll
    ///     let task_a_pause_time = Duration::from_millis(500);
    ///     let task_a_total_time = time(instrument_pause_await(&metrics_monitor, task_a_pause_time)).await;
    ///
    ///     // the `total_first_poll_delay` in this period will be somewhere between the
    ///     // pause time of `task_a`, and the total execution time of `task_a`
    ///     let total_first_poll_delay = next_interval().total_first_poll_delay;
    ///     assert!(total_first_poll_delay >= task_a_pause_time);
    ///     assert!(total_first_poll_delay <= task_a_total_time);
    ///
    ///     // construct and await a task that pauses for 250ms between instrumentation and first poll
    ///     let task_b_pause_time = Duration::from_millis(250);
    ///     let task_b_total_time = time(instrument_pause_await(&metrics_monitor, task_b_pause_time)).await;
    ///
    ///     // construct and await a task that pauses for 100ms between instrumentation and first poll
    ///     let task_c_pause_time = Duration::from_millis(100);
    ///     let task_c_total_time = time(instrument_pause_await(&metrics_monitor, task_c_pause_time)).await;
    ///
    ///     // the `total_first_poll_delay` in this period will be somewhere between the
    ///     // combined pause times of `task_a` and `task_b` (350ms), and the combined total execution times
    ///     // of `task_a` and `task_b`
    ///     let total_first_poll_delay = next_interval().total_first_poll_delay;
    ///     assert!(total_first_poll_delay >= task_b_pause_time + task_c_pause_time);
    ///     assert!(total_first_poll_delay <= task_b_total_time + task_c_total_time);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Produces the amount of time it took to await a given task.
    /// async fn time(task: impl Future) -> Duration {
    ///     let start = tokio::time::Instant::now();
    ///     task.await;
    ///     start.elapsed()
    /// }
    /// ```
    ///
    /// ### When is this metric recorded?
    /// The delay between instrumentation and first poll is not recorded until the first poll actually occurs:
    /// ```
    /// # use tokio::time::Duration;
    /// #
    /// # #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// # async fn main() {
    /// #     let monitor = tokio_metrics::TaskMonitor::new();
    /// #     let mut interval = monitor.intervals();
    /// #     let mut next_interval = || interval.next().unwrap();
    /// #
    /// // we construct and instrument a task, but do not `await` it
    /// let task = monitor.instrument(async {});
    ///
    /// // let's sleep for 1s before we poll `task`
    /// let one_sec = Duration::from_secs(1);
    /// let _ = tokio::time::sleep(one_sec).await;
    ///
    /// // although 1s has now elapsed since the instrumentation of `task`,
    /// // this is not reflected in `total_first_poll_delay`...
    /// assert_eq!(next_interval().total_first_poll_delay, Duration::ZERO);
    /// assert_eq!(monitor.cumulative().total_first_poll_delay, Duration::ZERO);
    ///
    /// // ...and won't be until `task` is actually polled
    /// task.await;
    ///
    /// // now, the 1s delay is reflected in `total_first_poll_delay`:
    /// assert_eq!(next_interval().total_first_poll_delay, one_sec);
    /// assert_eq!(monitor.cumulative().total_first_poll_delay, one_sec);
    /// # }
    /// ```
    ///
    /// ### What if time-to-first-poll is very large?
    /// The time-to-first-poll of *individual* tasks saturates at `u64::MAX` nanoseconds. However, if the *total*
    /// time-to-first-poll *across* monitored tasks exceeds `u64::MAX` nanoseconds, this metric will wrap-around:
    /// ```
    /// use tokio::time::Duration;
    ///
    /// #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // construct and instrument a task, but do not `await` it
    ///     let task = monitor.instrument(async {});
    ///
    ///     // this is the maximum duration representable by tokio_metrics
    ///     let max_duration = Duration::from_nanos(u64::MAX);
    ///
    ///     // let's advance the clock by double this amount and await `task`
    ///     let _ = tokio::time::advance(max_duration * 2).await;
    ///     task.await;
    ///
    ///     // the time-to-first-poll of `task` saturates at `max_duration`
    ///     assert_eq!(monitor.cumulative().total_first_poll_delay, max_duration);
    ///
    ///     // ...but note that the metric *will* wrap around if more tasks are involved
    ///     let task = monitor.instrument(async {});
    ///     let _ = tokio::time::advance(Duration::from_nanos(1)).await;
    ///     task.await;
    ///     assert_eq!(monitor.cumulative().total_first_poll_delay, Duration::ZERO);
    /// }
    /// ```
    pub total_first_poll_delay: Duration,

    pub total_idled_count: u64,

    pub total_idle_duration: Duration,

    /// The number of times instrumented tasks were scheduled for execution.
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_time_scheduled`]:
    ///   the mean amount of time that monitored tasks spent waiting to be run.
    ///
    /// ### Example
    /// In the below example, a task yields to the scheduler a varying number of times between sample periods;
    /// the number of times the task yields matches [`TaskMetrics::total_scheduled_count`]:
    /// ```
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // [A] no tasks have been created, instrumented, and polled more than once
    ///     assert_eq!(metrics_monitor.cumulative().total_scheduled_count, 0);
    ///
    ///     // [B] a `task` is created and instrumented
    ///     let task = {
    ///         let monitor = metrics_monitor.clone();
    ///         metrics_monitor.instrument(async move {
    ///             let mut interval = monitor.intervals();
    ///             let mut next_interval = move || interval.next().unwrap();
    ///
    ///             // [E] `task` has not yet yielded to the scheduler, and
    ///             // thus has not yet been scheduled since its first `poll`
    ///             assert_eq!(next_interval().total_scheduled_count, 0);
    ///
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///
    ///             // [F] `task` has yielded to the scheduler once (and thus been
    ///             // scheduled once) since the last sampling period
    ///             assert_eq!(next_interval().total_scheduled_count, 1);
    ///
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///
    ///             // [G] `task` has yielded to the scheduler thrice (and thus been
    ///             // scheduled thrice) since the last sampling period
    ///             assert_eq!(next_interval().total_scheduled_count, 3);
    ///
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///
    ///             next_interval
    ///         })
    ///     };
    ///
    ///     // [C] `task` has not yet been polled at all
    ///     assert_eq!(metrics_monitor.cumulative().total_first_poll_count, 0);
    ///     assert_eq!(metrics_monitor.cumulative().total_scheduled_count, 0);
    ///
    ///     // [D] poll `task` to completion
    ///     let mut next_interval = task.await;
    ///
    ///     // [H] `task` has been polled 1 times since the last sample
    ///     assert_eq!(next_interval().total_scheduled_count, 1);
    ///
    ///     // [I] `task` has been polled 0 times since the last sample
    ///     assert_eq!(next_interval().total_scheduled_count, 0);
    ///
    ///     // [J] `task` has yielded to the scheduler a total of five times
    ///     assert_eq!(metrics_monitor.cumulative().total_scheduled_count, 5);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub total_scheduled_count: u64,

    /// The total amount of time tasks spent waiting to be scheduled.
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_time_scheduled`]:
    ///   the mean amount of time that monitored tasks spent waiting to be run.
    ///
    /// ### Example
    /// In the below example, a task that yields endlessly is raced against a task that blocks the
    /// executor for 1 second; the yielding task spends approximately 1 second waiting to
    /// be scheduled. In the next sampling period, a task that yields endlessly is raced against a
    /// task that blocks the executor for half a second; the yielding task spends approximately half
    /// a second waiting to be scheduled.
    /// ```
    /// use std::time::Duration;
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // construct and instrument and spawn a task that yields endlessly
    ///     let endless_task = metrics_monitor.instrument(async {
    ///         loop { tokio::task::yield_now().await }
    ///     });
    ///
    ///     // construct and spawn a task that blocks the executor for 1 second
    ///     let one_sec_task = tokio::spawn(async {
    ///         std::thread::sleep(Duration::from_millis(1000))
    ///     });
    ///
    ///     // race `endless_task` against `one_sec_task`
    ///     tokio::select! {
    ///         biased;
    ///         _ = endless_task => { unreachable!() }
    ///         _ = one_sec_task => {}
    ///     }
    ///
    ///     // `endless_task` will have spent approximately one second waiting
    ///     let total_scheduled_duration = next_interval().total_scheduled_duration;
    ///     assert!(total_scheduled_duration >= Duration::from_millis(1000));
    ///     assert!(total_scheduled_duration <= Duration::from_millis(1100));
    ///
    ///     // construct and instrument and spawn a task that yields endlessly
    ///     let endless_task = metrics_monitor.instrument(async {
    ///         loop { tokio::task::yield_now().await }
    ///     });
    ///
    ///     // construct and spawn a task that blocks the executor for 1 second
    ///     let half_sec_task = tokio::spawn(async {
    ///         std::thread::sleep(Duration::from_millis(500))
    ///     });
    ///
    ///     // race `endless_task` against `half_sec_task`
    ///     tokio::select! {
    ///         biased;
    ///         _ = endless_task => { unreachable!() }
    ///         _ = half_sec_task => {}
    ///     }
    ///
    ///     // `endless_task` will have spent approximately half a second waiting
    ///     let total_scheduled_duration = next_interval().total_scheduled_duration;
    ///     assert!(total_scheduled_duration >= Duration::from_millis(500));
    ///     assert!(total_scheduled_duration <= Duration::from_millis(600));
    ///
    ///     Ok(())
    /// }
    /// ```
    pub total_scheduled_duration: Duration,

    /// The number of times that polling instrumented tasks completed swiftly.
    ///
    /// Here, 'swiftly' is defined as completing in strictly less time than [`TaskMonitor::slow_poll_threshold`].
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_fast_polls`]:
    ///   the mean time consumed by fast polls of monitored tasks.
    ///
    /// ### Example
    /// In the below example, 0 polls occur within the first sampling period, 3 fast polls occur within the second
    /// sampling period, and 2 fast polls occur within the third sampling period:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     assert_eq!(next_interval().total_fast_poll_count, 0);
    ///
    ///     let fast = Duration::ZERO;
    ///
    ///     // this task completes in three fast polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(fast).await; // fast poll 1
    ///         spin_for(fast).await; // fast poll 2
    ///         spin_for(fast)        // fast poll 3
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_fast_poll_count, 3);
    ///
    ///     // this task completes in two fast polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(fast).await; // fast poll 1
    ///         spin_for(fast)        // fast poll 2
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_fast_poll_count, 2);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub total_fast_poll_count: u64,

    /// The total amount of time that fast polls took to complete.
    ///
    /// Here, 'fast' is defined as completing in strictly less time than [`TaskMonitor::slow_poll_threshold`].
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_fast_polls`]:
    ///   the mean time consumed by fast polls of monitored tasks.
    ///
    /// ### Example
    /// In the below example, no tasks are polled in the first sampling period; three fast polls consume
    /// a total of 3μs time in the second sampling period;
    /// and two fast polls consume a total of 2μs time in the third
    /// sampling period:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     let interval = next_interval();
    ///     assert_eq!(interval.total_fast_poll_duration, Duration::ZERO);
    ///
    ///     let fast = Duration::from_micros(1);
    ///
    ///     // this task completes in three fast polls
    ///     let task_a_time = time(metrics_monitor.instrument(async {
    ///         spin_for(fast).await; // fast poll 1
    ///         spin_for(fast).await; // fast poll 2
    ///         spin_for(fast)        // fast poll 3
    ///     })).await;
    ///
    ///     let interval = next_interval();
    ///     assert!(interval.total_fast_poll_duration >= fast * 3);
    ///     assert!(interval.total_fast_poll_duration <= task_a_time);
    ///
    ///     // this task completes in two fast polls
    ///     let task_b_time = time(metrics_monitor.instrument(async {
    ///         spin_for(fast).await; // fast poll 1
    ///         spin_for(fast)        // fast poll 2
    ///     })).await;
    ///
    ///     let interval = next_interval();
    ///     assert!(interval.total_fast_poll_duration >= fast * 2);
    ///     assert!(interval.total_fast_poll_duration <= task_b_time);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Produces the amount of time it took to await a given async task.
    /// async fn time(task: impl Future) -> Duration {
    ///     let start = tokio::time::Instant::now();
    ///     task.await;
    ///     start.elapsed()
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub total_fast_poll_duration: Duration,

    /// The number of times that polling instrumented tasks completed slowly.
    ///
    /// Here, 'slowly' is defined as completing in at least as much time as [`TaskMonitor::slow_poll_threshold`].
    ///
    /// ### Derived metrics
    /// - [`TaskMetrics::mean_slow_polls`]:
    ///   the mean time consumed by slow polls of monitored tasks.
    ///
    /// ### Example
    /// In the below example, 0 polls occur within the first sampling period, 3 slow polls occur within the second
    /// sampling period, and 2 slow polls occur within the third sampling period:
    ///
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     assert_eq!(next_interval().total_slow_poll_count, 0);
    ///
    ///     let slow = 10 * metrics_monitor.slow_poll_threshold();
    ///
    ///     // this task completes in three slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow).await; // slow poll 2
    ///         spin_for(slow)        // slow poll 3
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_slow_poll_count, 3);
    ///
    ///     // this task completes in two slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow)        // slow poll 2
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_slow_poll_count, 2);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub total_slow_poll_count: u64,

    /// The total amount of time that slow polls took to complete.
    ///
    /// Here, 'slowly' is defined as completing in at least as much time as [`TaskMonitor::slow_poll_threshold`].
    ///
    /// ### See also
    /// - [`TaskMetrics::mean_slow_polls`]
    ///   derived from [`TaskMetrics::total_slow_poll_duration`] ÷ [`TaskMetrics::total_slow_poll_count`]
    ///
    /// ### Example
    /// In the below example, no tasks are polled in the first sampling period; three slow polls consume
    /// a total of 30 × [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] time in the second sampling period;
    /// and two slow polls consume a total of 20 × [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] time in the
    /// third sampling period:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     let interval = next_interval();
    ///     assert_eq!(interval.total_slow_poll_duration, Duration::ZERO);
    ///
    ///     let slow = 10 * metrics_monitor.slow_poll_threshold();
    ///
    ///     // this task completes in three slow polls
    ///     let task_a_time = time(metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow).await; // slow poll 2
    ///         spin_for(slow)        // slow poll 3
    ///     })).await;
    ///
    ///     let interval = next_interval();
    ///     assert!(interval.total_slow_poll_duration >= slow * 3);
    ///     assert!(interval.total_slow_poll_duration <= task_a_time);
    ///
    ///     // this task completes in two slow polls
    ///     let task_b_time = time(metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow)        // slow poll 2
    ///     })).await;
    ///
    ///     let interval = next_interval();
    ///     assert!(interval.total_slow_poll_duration >= slow * 2);
    ///     assert!(interval.total_slow_poll_duration <= task_b_time);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Produces the amount of time it took to await a given async task.
    /// async fn time(task: impl Future) -> Duration {
    ///     let start = tokio::time::Instant::now();
    ///     task.await;
    ///     start.elapsed()
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub total_slow_poll_duration: Duration,
}

/// Tracks the metrics, shared across the various types.
struct RawMetrics {
    /// A task poll takes longer than this, it is considered a slow poll.
    slow_poll_threshold: Duration,

    /// Total number of instrumented tasks
    total_first_poll_count: AtomicU64,

    /// Total number of times tasks entered the `idle` state.
    total_idled_count: AtomicU64,

    /// Total number of times tasks were scheduled.
    total_scheduled_count: AtomicU64,

    /// Total number of times tasks were polled fast
    total_fast_poll_count: AtomicU64,

    /// Total number of times tasks were polled slow
    slow_polls_count: AtomicU64,

    /// Total amount of time until the first poll
    total_first_poll_delay_ns: AtomicU64,

    /// Total amount of time tasks spent in the `idle` state.
    total_idle_duration_ns: AtomicU64,

    /// Total amount of time tasks spent in the waking state.
    total_scheduled_duration_ns: AtomicU64,

    /// Total amount of time tasks spent being polled below the slow cut off.
    total_fast_poll_duration_ns: AtomicU64,

    /// Total amount of time tasks spent being polled above the slow cut off.
    total_slow_poll_duration: AtomicU64,
}

struct State {
    /// Where metrics should be recorded
    metrics: Arc<RawMetrics>,

    /// Instant at which the task was instrumented. This is used to track the time to first poll.
    instrumented_at: Instant,

    /// The instant, tracked as nanoseconds since `instrumented_at`, at which the future
    /// was last woken.
    woke_at: AtomicU64,

    /// Waker to forward notifications to.
    waker: AtomicWaker,
}

impl TaskMonitor {
    /// The default duration at which polls cross the threshold into being categorized as 'slow' is 50μs.
    #[cfg(not(test))]
    pub const DEFAULT_SLOW_POLL_THRESHOLD: Duration = Duration::from_micros(50);
    #[cfg(test)]
    pub const DEFAULT_SLOW_POLL_THRESHOLD: Duration = Duration::from_millis(500);

    /// Constructs a new task monitor.
    ///
    /// Uses [`Self::DEFAULT_SLOW_POLL_THRESHOLD`] as the threshold at which polls will be considered 'slow'.
    pub fn new() -> TaskMonitor {
        TaskMonitor::with_slow_poll_threshold(Self::DEFAULT_SLOW_POLL_THRESHOLD)
    }

    /// Constructs a new task monitor with a given threshold at which polls are considered 'slow'.
    ///
    /// ##### Selecting an appropriate threshold
    /// TODO. What advice can we give here?
    ///
    /// ##### Example
    /// In the below example, low-threshold and high-threshold monitors are constructed and instrument
    /// identical tasks; the low-threshold monitor reports4 slow polls, and the high-threshold monitor
    /// reports only 2 slow polls:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    /// use tokio_metrics::TaskMonitor;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let lo_threshold = Duration::from_micros(10);
    ///     let hi_threshold = Duration::from_millis(10);
    ///
    ///     let lo_monitor = TaskMonitor::with_slow_poll_threshold(lo_threshold);
    ///     let hi_monitor = TaskMonitor::with_slow_poll_threshold(hi_threshold);
    ///
    ///     let make_task = || async {
    ///         spin_for(lo_threshold).await; // faster poll 1
    ///         spin_for(lo_threshold).await; // faster poll 2
    ///         spin_for(hi_threshold).await; // slower poll 3
    ///         spin_for(hi_threshold).await  // slower poll 4
    ///     };
    ///
    ///     lo_monitor.instrument(make_task()).await;
    ///     hi_monitor.instrument(make_task()).await;
    ///
    ///     // the low-threshold monitor reported 4 slow polls:
    ///     assert_eq!(lo_monitor.cumulative().total_slow_poll_count, 4);
    ///     // the high-threshold monitor reported only 2 slow polls:
    ///     assert_eq!(hi_monitor.cumulative().total_slow_poll_count, 2);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn with_slow_poll_threshold(slow_poll_cut_off: Duration) -> TaskMonitor {
        TaskMonitor {
            metrics: Arc::new(RawMetrics {
                slow_poll_threshold: slow_poll_cut_off,
                total_first_poll_count: AtomicU64::new(0),
                total_idled_count: AtomicU64::new(0),
                total_scheduled_count: AtomicU64::new(0),
                total_fast_poll_count: AtomicU64::new(0),
                slow_polls_count: AtomicU64::new(0),
                total_first_poll_delay_ns: AtomicU64::new(0),
                total_scheduled_duration_ns: AtomicU64::new(0),
                total_idle_duration_ns: AtomicU64::new(0),
                total_fast_poll_duration_ns: AtomicU64::new(0),
                total_slow_poll_duration: AtomicU64::new(0),
            }),
        }
    }

    /// Produces the duration greater-than-or-equal-to at which polls are categorized as slow.
    ///
    /// ##### Example
    /// In the below example, [`TaskMonitor`] is initialized with [`TaskMonitor::new`]; consequently, its slow-poll
    /// threshold equals [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`]:
    /// ```
    /// use tokio_metrics::TaskMonitor;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = TaskMonitor::new();
    ///
    ///     assert_eq!(metrics_monitor.slow_poll_threshold(), TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn slow_poll_threshold(&self) -> Duration {
        self.metrics.slow_poll_threshold
    }

    /// Produces an instrumented façade around a given async task.
    ///
    /// ##### Examples
    /// Instrument an async task by passing it to [`TaskMonitor::instrument`]:
    /// ```
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // 0 tasks have been instrumented, much less polled
    ///     assert_eq!(metrics_monitor.cumulative().total_first_poll_count, 0);
    ///
    ///     // instrument a task and poll it to completion
    ///     metrics_monitor.instrument(async {}).await;
    ///
    ///     // 1 task has been instrumented and polled
    ///     assert_eq!(metrics_monitor.cumulative().total_first_poll_count, 1);
    ///
    ///     // instrument a task and poll it to completion
    ///     metrics_monitor.instrument(async {}).await;
    ///
    ///     // 2 tasks have been instrumented and polled
    ///     assert_eq!(metrics_monitor.cumulative().total_first_poll_count, 2);
    ///
    ///     Ok(())
    /// }
    /// ```
    /// An aync task may be tracked by multiple [`TaskMonitor`]s; e.g.:
    /// ```
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let monitor_a = tokio_metrics::TaskMonitor::new();
    ///     let monitor_b = tokio_metrics::TaskMonitor::new();
    ///
    ///     // 0 tasks have been instrumented, much less polled
    ///     assert_eq!(monitor_a.cumulative().total_first_poll_count, 0);
    ///     assert_eq!(monitor_b.cumulative().total_first_poll_count, 0);
    ///
    ///     // instrument a task and poll it to completion
    ///     monitor_a.instrument(monitor_b.instrument(async {})).await;
    ///
    ///     // 1 task has been instrumented and polled
    ///     assert_eq!(monitor_a.cumulative().total_first_poll_count, 1);
    ///     assert_eq!(monitor_b.cumulative().total_first_poll_count, 1);
    ///
    ///     Ok(())
    /// }
    /// ```
    /// It is also possible (but probably undesirable) to instrument an async task multiple times
    /// with the same [`TaskMonitor`]; e.g.:
    /// ```
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // 0 tasks have been instrumented, much less polled
    ///     assert_eq!(monitor.cumulative().total_first_poll_count, 0);
    ///
    ///     // instrument a task and poll it to completion
    ///     monitor.instrument(monitor.instrument(async {})).await;
    ///
    ///     // 2 tasks have been instrumented and polled, supposedly
    ///     assert_eq!(monitor.cumulative().total_first_poll_count, 2);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn instrument<F: Future>(&self, task: F) -> Instrumented<F> {
        Instrumented {
            task,
            did_poll_once: false,
            idled_at: 0,
            state: Arc::new(State {
                metrics: self.metrics.clone(),
                instrumented_at: Instant::now(),
                woke_at: AtomicU64::new(0),
                waker: AtomicWaker::new(),
            }),
        }
    }

    /// Produces [`TaskMetrics`] for the tasks instrumented by this [`TaskMonitor`], collected since the
    /// construction of [`TaskMonitor`].
    ///
    /// ##### See also
    /// - [`TaskMonitor::intervals`]:
    ///     produces [`TaskMetrics`] for user-defined sampling-periods, instead of cumulatively
    ///
    /// ##### Example
    /// In the below example, 0 polls occur within the first sampling period, 3 slow polls occur within the second
    /// sampling period, and 2 slow polls occur within the third sampling period; five slow polls occur across
    /// all sampling periods:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // initialize a stream of sampling periods
    ///     let mut samples = metrics_monitor.intervals();
    ///     // each call of `next_sample` will produce metrics for the last sampling period
    ///     let mut next_sample = || samples.next().unwrap();
    ///
    ///     let slow = 10 * metrics_monitor.slow_poll_threshold();
    ///
    ///     // this task completes in three slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow).await; // slow poll 2
    ///         spin_for(slow)        // slow poll 3
    ///     }).await;
    ///
    ///     // in the previous sampling period, there were 3 slow polls
    ///     assert_eq!(next_sample().total_slow_poll_count, 3);
    ///     assert_eq!(metrics_monitor.cumulative().total_slow_poll_count, 3);
    ///
    ///     // this task completes in two slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow)        // slow poll 2
    ///     }).await;
    ///
    ///     // in the previous sampling period, there were 3 slow polls
    ///     assert_eq!(next_sample().total_slow_poll_count, 2);
    ///
    ///     // across all sampling periods, there were a total of 5 slow polls
    ///     assert_eq!(metrics_monitor.cumulative().total_slow_poll_count, 5);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn cumulative(&self) -> TaskMetrics {
        self.metrics.metrics()
    }

    /// Produces an unending iterator of metric sampling periods.
    ///
    /// Each sampling period is defined by the time elapsed between advancements of the iterator
    /// produced by [`TaskMonitor::intervals`]. The item type of this iterator is [`TaskMetrics`], which is a bundle
    /// of task metrics that describe *only* events occuring within that sampling period.
    ///
    /// ##### Example
    /// In the below example, 0 polls occur within the first sampling period, 3 slow polls occur within the second
    /// sampling period, and 2 slow polls occur within the third sampling period; five slow polls occur across
    /// all sampling periods:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // initialize a stream of sampling periods
    ///     let mut samples = metrics_monitor.intervals();
    ///     // each call of `next_sample` will produce metrics for the last sampling period
    ///     let mut next_sample = || samples.next().unwrap();
    ///
    ///     let slow = 10 * metrics_monitor.slow_poll_threshold();
    ///
    ///     // this task completes in three slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow).await; // slow poll 2
    ///         spin_for(slow)        // slow poll 3
    ///     }).await;
    ///
    ///     // in the previous sampling period, there were 3 slow polls
    ///     assert_eq!(next_sample().total_slow_poll_count, 3);
    ///
    ///     // this task completes in two slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow)        // slow poll 2
    ///     }).await;
    ///
    ///     // in the previous sampling period, there were 3 slow polls
    ///     assert_eq!(next_sample().total_slow_poll_count, 2);
    ///
    ///     // across all sampling periods, there were a total of 5 slow polls
    ///     assert_eq!(metrics_monitor.cumulative().total_slow_poll_count, 5);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn intervals(&self) -> impl Iterator<Item = TaskMetrics> {
        let latest = self.metrics.clone();
        let mut previous = None;

        std::iter::from_fn(move || {
            let latest: TaskMetrics = latest.metrics();

            let next = if let Some(previous) = previous {
                latest - previous
            } else {
                latest
            };

            previous = Some(latest);

            Some(next)
        })
    }
}

impl RawMetrics {
    fn metrics(&self) -> TaskMetrics {
        TaskMetrics {
            total_first_poll_count: self.total_first_poll_count.load(SeqCst),
            total_idled_count: self.total_idled_count.load(SeqCst),
            total_scheduled_count: self.total_scheduled_count.load(SeqCst),
            total_fast_poll_count: self.total_fast_poll_count.load(SeqCst),
            total_slow_poll_count: self.slow_polls_count.load(SeqCst),
            total_first_poll_delay:
                Duration::from_nanos(self.total_first_poll_delay_ns.load(SeqCst)),
            total_idle_duration: 
                Duration::from_nanos(self.total_idle_duration_ns.load(SeqCst)),
            total_scheduled_duration: 
                Duration::from_nanos(self.total_scheduled_duration_ns.load(SeqCst)),
            total_fast_poll_duration:
                Duration::from_nanos(self.total_fast_poll_duration_ns.load(SeqCst)),
            total_slow_poll_duration:
                Duration::from_nanos(self.total_slow_poll_duration.load(SeqCst)),
        }
    }
}

impl std::ops::Sub for TaskMetrics {
    type Output = TaskMetrics;

    fn sub(self, prev: TaskMetrics) -> TaskMetrics {
        TaskMetrics {
            total_first_poll_count: self.total_first_poll_count.wrapping_sub(prev.total_first_poll_count),
            total_idled_count: self.total_idled_count.wrapping_sub(prev.total_idled_count),
            total_scheduled_count: self.total_scheduled_count.wrapping_sub(prev.total_scheduled_count),
            total_fast_poll_count: self.total_fast_poll_count.wrapping_sub(prev.total_fast_poll_count),
            total_slow_poll_count: self.total_slow_poll_count.wrapping_sub(prev.total_slow_poll_count),
            total_first_poll_delay: 
                sub(self.total_first_poll_delay,
                    prev.total_first_poll_delay),
            total_idle_duration:
                sub(self.total_idle_duration,
                    prev.total_idle_duration),
            total_scheduled_duration: 
                sub(self.total_scheduled_duration,
                    prev.total_scheduled_duration),
            total_fast_poll_duration:
                sub(self.total_fast_poll_duration,
                    prev.total_fast_poll_duration),
            total_slow_poll_duration:
                sub(self.total_slow_poll_duration,
                    prev.total_slow_poll_duration),
        }
    }
}

impl TaskMetrics {
    /// The total number of polls.
    ///
    /// ##### Definition
    /// This metric is derived from [`TaskMetrics::total_fast_poll_count`] + [`TaskMetrics::total_slow_poll_count`].
    ///
    /// ##### Example
    /// In the below example, a task with multiple yield points is await'ed to completion; the
    /// [`TaskMetrics::num_polls`] metric reflects the number of `await`s within each sample period.
    /// ```
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // [A] no tasks have been created, instrumented, and polled more than once
    ///     assert_eq!(metrics_monitor.cumulative().total_first_poll_count, 0);
    ///
    ///     // [B] a `task` is created and instrumented
    ///     let task = {
    ///         let monitor = metrics_monitor.clone();
    ///         metrics_monitor.instrument(async move {
    ///             let mut interval = monitor.intervals();
    ///             let mut next_interval = move || interval.next().unwrap();
    ///
    ///             // [E] task is in the midst of its first poll
    ///             assert_eq!(next_interval().num_polls(), 0);
    ///
    ///             tokio::task::yield_now().await; // poll 1
    ///
    ///             // [F] task has been polled 1 time
    ///             assert_eq!(next_interval().num_polls(), 1);
    ///
    ///             tokio::task::yield_now().await; // poll 2
    ///             tokio::task::yield_now().await; // poll 3
    ///             tokio::task::yield_now().await; // poll 4
    ///
    ///             // [G] task has been polled 3 times
    ///             assert_eq!(next_interval().num_polls(), 3);
    ///
    ///             tokio::task::yield_now().await; // poll 5
    ///
    ///             next_interval                      // poll 6
    ///         })
    ///     };
    ///
    ///     // [C] `task` has not yet been polled at all
    ///     assert_eq!(metrics_monitor.cumulative().num_polls(), 0);
    ///
    ///     // [D] poll `task` to completion
    ///     let mut next_interval = task.await;
    ///
    ///     // [H] `task` has been polled 2 times since the last sample
    ///     assert_eq!(next_interval().num_polls(), 2);
    ///
    ///     // [I] `task` has been polled 0 times since the last sample
    ///     assert_eq!(next_interval().num_polls(), 0);
    ///
    ///     // [J] `task` has been polled 6 times
    ///     assert_eq!(metrics_monitor.cumulative().num_polls(), 6);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn num_polls(&self) -> u64 {
        self.total_fast_poll_count + self.total_slow_poll_count
    }

    /// The mean time elapsed between the instrumentation of tasks and the time they are first polled.
    ///
    /// ##### Definition
    /// This metric is derived from [`TaskMetrics::total_first_poll_delay`] ÷ [`TaskMetrics::total_first_poll_count`].
    ///
    /// ### Interpretation
    /// A high `mean_time_to_first_poll` has two potential culprits:
    /// 1.
    ///
    /// ##### Example
    /// In the below example, no tasks are instrumented or polled within the first sample period; in the second
    /// sampling period, 500ms elapse between the instrumentation of a task and its first poll; in the third
    /// sampling period, a mean of 750ms elapse between the instrumentation and first poll of two tasks:
    /// ```
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have yet been created, instrumented, or polled
    ///     assert_eq!(next_interval().mean_time_to_first_poll(), Duration::ZERO);
    ///
    ///     // constructs and instruments a task, pauses for `pause_time`, awaits the task, then
    ///     // produces the total time it took to do all of the aforementioned
    ///     async fn instrument_pause_await(
    ///         metrics_monitor: &tokio_metrics::TaskMonitor,
    ///         pause_time: Duration
    ///     ) -> Duration
    ///     {
    ///         let before_instrumentation = tokio::time::Instant::now();
    ///         let task = metrics_monitor.instrument(async move {});
    ///         tokio::time::sleep(pause_time).await;
    ///         task.await;
    ///         before_instrumentation.elapsed()
    ///     }
    ///
    ///     // construct and await a task that pauses for 500ms between instrumentation and first poll
    ///     let task_a_pause_time = Duration::from_millis(500);
    ///     let task_a_total_time = instrument_pause_await(&metrics_monitor, task_a_pause_time).await;
    ///
    ///     // the `mean_time_to_first_poll` will be some duration greater-than-or-equal-to the
    ///     // pause time of 500ms, and less-than-or-equal-to the total runtime of `task_a`
    ///     let mean_time_to_first_poll = next_interval().mean_time_to_first_poll();
    ///     assert!(mean_time_to_first_poll >= task_a_pause_time);
    ///     assert!(mean_time_to_first_poll <= task_a_total_time);
    ///
    ///     // construct and await a task that pauses for 500ms between instrumentation and first poll
    ///     let task_b_pause_time = Duration::from_millis(500);
    ///     let task_b_total_time = instrument_pause_await(&metrics_monitor, task_b_pause_time).await;
    ///
    ///     // construct and await a task that pauses for 1000ms between instrumentation and first poll
    ///     let task_c_pause_time = Duration::from_millis(1000);
    ///     let task_c_total_time = instrument_pause_await(&metrics_monitor, task_c_pause_time).await;
    ///
    ///     // the `mean_time_to_first_poll` will be some duration greater-than-or-equal-to the
    ///     // average pause time of 500ms, and less-than-or-equal-to the combined total runtime of
    ///     // `task_b` and `task_c`
    ///     let mean_time_to_first_poll = next_interval().mean_time_to_first_poll();
    ///     assert!(mean_time_to_first_poll >= (task_b_pause_time + task_c_pause_time) / 2);
    ///     assert!(mean_time_to_first_poll <= (task_b_total_time + task_c_total_time) / 2);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn mean_time_to_first_poll(&self) -> Duration {
        if self.total_first_poll_count == 0 {
            Duration::ZERO
        } else {
            self.total_first_poll_delay / self.total_first_poll_count as _
        }
    }

    /// The mean duration that monitored tasks spent in the idle state.
    /// 
    /// ##### Definition
    /// This metric is derived from [`TaskMetrics::total_idle_duration`] ÷ [`TaskMetrics::total_idled_count`].
    /// 
    /// ##### Example
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let one_sec = std::time::Duration::from_secs(1);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(one_sec).await;
    ///     }).await;
    ///
    ///     assert!(monitor.cumulative().mean_time_idle() >= one_sec);
    /// }
    /// ```
    pub fn mean_time_idle(&self) -> Duration {
        mean(self.total_idle_duration, self.total_first_poll_count)
    }

    /// The mean amount of time that monitored tasks spent waiting to be run.
    ///
    /// ##### Definition
    /// This metric is derived from [`TaskMetrics::total_scheduled_duration`] ÷ [`TaskMetrics::total_scheduled_count`].
    ///
    /// ##### Interpretation
    /// A high `mean_time_scheduled` has one culprit: monitored tasks tended to spend a long time in the runtime's task
    /// queues.
    ///
    /// ##### Example
    /// In the below example, a task that yields endlessly is raced against a task that blocks the
    /// executor for 1 second; the yielding task spends approximately 1 second waiting to
    /// be scheduled. In the next sampling period, a task that yields endlessly is raced against a
    /// task that blocks the executor for half a second; the yielding task spends approximately half
    /// a second waiting to be scheduled.
    /// ```
    /// use std::time::Duration;
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // construct and instrument a task that yields endlessly
    ///     let endless_task = metrics_monitor.instrument(async {
    ///         loop { tokio::task::yield_now().await }
    ///     });
    ///
    ///     // construct and spawn a task that blocks the executor for 1 second
    ///     let one_sec_task = tokio::spawn(async {
    ///         std::thread::sleep(Duration::from_secs(1))
    ///     });
    ///
    ///     // ensure that at least 2s elapse between the instrumentation of
    ///     // `endless_task` and its first poll
    ///     std::thread::sleep(Duration::from_secs(2));
    ///
    ///     // race `endless_task` against `one_sec_task`
    ///     tokio::select! {
    ///         biased;
    ///         _ = endless_task => { unreachable!() }
    ///         _ = one_sec_task => {}
    ///     }
    ///
    ///     // `endless_task` will have spent approximately one second waiting:
    ///     let interval = next_interval();
    ///     assert!(interval.mean_time_scheduled() >= Duration::from_secs(1));
    ///     // ...but *not* 2s waiting:
    ///     assert!(interval.mean_time_scheduled() <= Duration::from_secs(2));
    ///     // i.e., time_to_first_poll is not factored into total_scheduled_duration:
    ///     assert!(interval.mean_time_to_first_poll() >= Duration::from_secs(2));
    ///
    ///     // construct and instrument and spawn a task that yields endlessly
    ///     let endless_task = metrics_monitor.instrument(async {
    ///         loop { tokio::task::yield_now().await }
    ///     });
    ///
    ///     // construct (but do not spawn) and a task that blocks the executor for 1 second
    ///     let one_sec_task = async {
    ///         std::thread::sleep(Duration::from_secs(1))
    ///     };
    ///
    ///     // race `endless_task` against `one_sec_task`
    ///     tokio::select! {
    ///         biased;
    ///         _ = endless_task => { unreachable!() }
    ///         _ = one_sec_task => {}
    ///     }
    ///
    ///     // `endless_task` will NOT have spent 1 second waiting to be scheduled
    ///     assert!(next_interval().mean_time_scheduled() < Duration::from_secs(1));
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn mean_time_scheduled(&self) -> Duration {
        mean(self.total_scheduled_duration, self.total_scheduled_count)
    }

    /// The ratio between the number polls categorized as fast and slow.
    ///
    /// This metric is derived from [`TaskMetrics::total_fast_poll_count`] ÷ [`TaskMetrics::num_polls`].
    ///
    /// ##### Example
    /// Changes in this metric may be observed by varying the ratio of fast and slow polls within sampling periods;
    /// for instance:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     let interval = next_interval();
    ///     assert_eq!(interval.total_fast_poll_count, 0);
    ///     assert_eq!(interval.total_slow_poll_count, 0);
    ///     assert!(interval.fast_poll_ratio().is_nan());
    ///
    ///     let fast = Duration::ZERO;
    ///     let slow = 10 * metrics_monitor.slow_poll_threshold();
    ///
    ///     // this task completes in three fast polls
    ///     metrics_monitor.instrument(async {
    ///         spin_for(fast).await;   // fast poll 1
    ///         spin_for(fast).await;   // fast poll 2
    ///         spin_for(fast);         // fast poll 3
    ///     }).await;
    ///
    ///     // this task completes in two slow polls
    ///     metrics_monitor.instrument(async {
    ///         spin_for(slow).await;   // slow poll 1
    ///         spin_for(slow);         // slow poll 2
    ///     }).await;
    ///
    ///     let interval = next_interval();
    ///     assert_eq!(interval.total_fast_poll_count, 3);
    ///     assert_eq!(interval.total_slow_poll_count, 2);
    ///     assert_eq!(interval.fast_poll_ratio(), ratio(3., 2.));
    ///
    ///     // this task completes in three slow polls
    ///     metrics_monitor.instrument(async {
    ///         spin_for(slow).await;   // slow poll 1
    ///         spin_for(slow).await;   // slow poll 2
    ///         spin_for(slow);         // slow poll 3
    ///     }).await;
    ///
    ///     // this task completes in two fast polls
    ///     metrics_monitor.instrument(async {
    ///         spin_for(fast).await; // fast poll 1
    ///         spin_for(fast);       // fast poll 2
    ///     }).await;
    ///
    ///     let interval = next_interval();
    ///     assert_eq!(interval.total_fast_poll_count, 2);
    ///     assert_eq!(interval.total_slow_poll_count, 3);
    ///     assert_eq!(interval.fast_poll_ratio(), ratio(2., 3.));
    ///
    ///     Ok(())
    /// }
    ///
    /// fn ratio(a: f64, b: f64) -> f64 {
    ///     a / (a + b)
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn fast_poll_ratio(&self) -> f64 {
        self.total_fast_poll_count as f64 / (self.total_fast_poll_count + self.total_slow_poll_count) as f64
    }

    /// The mean time consumed by fast polls of monitored tasks.
    ///
    /// ##### Definition
    /// This metric is derived from [`TaskMetrics::total_fast_poll_count`] ÷ [`TaskMetrics::num_polls`].
    ///
    /// ##### Example
    /// In the below example, no tasks are polled in the first sampling period; three fast polls consume
    /// a mean of ⅜ × [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] time in the second sampling period;
    /// and two fast polls consume a total of ½ × [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] time in the third
    /// sampling period:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     assert_eq!(next_interval().mean_fast_polls(), Duration::ZERO);
    ///
    ///     let threshold = metrics_monitor.slow_poll_threshold();
    ///     let fast_1 = 1 * Duration::from_micros(1);
    ///     let fast_2 = 2 * Duration::from_micros(1);
    ///     let fast_3 = 3 * Duration::from_micros(1);
    ///
    ///     // this task completes in two fast polls
    ///     let total_time = time(metrics_monitor.instrument(async {
    ///         spin_for(fast_1).await; // fast poll 1
    ///         spin_for(fast_2)        // fast poll 2
    ///     })).await;
    ///
    ///     // `mean_fast_polls` ≈ the mean of `fast_1` and `fast_2`
    ///     let mean_fast_polls = next_interval().mean_fast_polls();
    ///     assert!(mean_fast_polls >= (fast_1 + fast_2) / 2);
    ///     assert!(mean_fast_polls <= total_time / 2);
    ///
    ///     // this task completes in three fast polls
    ///     let total_time = time(metrics_monitor.instrument(async {
    ///         spin_for(fast_1).await; // fast poll 1
    ///         spin_for(fast_2).await; // fast poll 2
    ///         spin_for(fast_3)        // fast poll 3
    ///     })).await;
    ///
    ///     // `mean_fast_polls` ≈ the mean of `fast_1`, `fast_2`, `fast_3`
    ///     let mean_fast_polls = next_interval().mean_fast_polls();
    ///     assert!(mean_fast_polls >= (fast_1 + fast_2 + fast_3) / 3);
    ///     assert!(mean_fast_polls <= total_time / 3);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Produces the amount of time it took to await a given task.
    /// async fn time(task: impl Future) -> Duration {
    ///     let start = tokio::time::Instant::now();
    ///     task.await;
    ///     start.elapsed()
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn mean_fast_polls(&self) -> Duration {
        mean(self.total_fast_poll_duration, self.total_fast_poll_count)
    }

    /// The mean time consumed by slow polls of monitored tasks.
    ///
    /// This metric is derived from [`TaskMetrics::total_slow_poll_duration`] ÷ [`TaskMetrics::total_slow_poll_count`].
    ///
    /// ##### Example
    /// In the below example, no tasks are polled in the first sampling period; three slow polls consume
    /// a mean of 1.5 × [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] time in the second sampling period;
    /// and two slow polls consume a total of 2 × [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] time in the third
    /// sampling period:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, or polled
    ///     assert_eq!(next_interval().mean_slow_polls(), Duration::ZERO);
    ///
    ///     let threshold = metrics_monitor.slow_poll_threshold();
    ///     let slow_1 = 1 * threshold;
    ///     let slow_2 = 2 * threshold;
    ///     let slow_3 = 3 * threshold;
    ///
    ///     // this task completes in two slow polls
    ///     let total_time = time(metrics_monitor.instrument(async {
    ///         spin_for(slow_1).await; // slow poll 1
    ///         spin_for(slow_2)        // slow poll 2
    ///     })).await;
    ///
    ///     // `mean_slow_polls` ≈ the mean of `slow_1` and `slow_2`
    ///     let mean_slow_polls = next_interval().mean_slow_polls();
    ///     assert!(mean_slow_polls >= (slow_1 + slow_2) / 2);
    ///     assert!(mean_slow_polls <= total_time / 2);
    ///
    ///     // this task completes in three slow polls
    ///     let total_time = time(metrics_monitor.instrument(async {
    ///         spin_for(slow_1).await; // slow poll 1
    ///         spin_for(slow_2).await; // slow poll 2
    ///         spin_for(slow_3)        // slow poll 3
    ///     })).await;
    ///
    ///     // `mean_slow_polls` ≈ the mean of `slow_1`, `slow_2`, `slow_3`
    ///     let mean_slow_polls = next_interval().mean_slow_polls();
    ///     assert!(mean_slow_polls >= (slow_1 + slow_2 + slow_3) / 3);
    ///     assert!(mean_slow_polls <= total_time / 3);
    ///
    ///     Ok(())
    /// }
    ///
    /// /// Produces the amount of time it took to await a given task.
    /// async fn time(task: impl Future) -> Duration {
    ///     let start = tokio::time::Instant::now();
    ///     task.await;
    ///     start.elapsed()
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn mean_slow_polls(&self) -> Duration {
        mean(self.total_slow_poll_duration, self.total_slow_poll_count)
    }
}

impl<T: Future> Future for Instrumented<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let poll_start = Instant::now();
        let this = self.project();
        let idled_at = this.idled_at;
        let state = this.state;
        let instrumented_at = state.instrumented_at;
        let metrics = &state.metrics;

        /* accounting for time-to-first-poll and tasks-count */
        // is this the first time this task has been polled?
        if !*this.did_poll_once {
            // if so, we need to do three things:
            /* 1. note that this task *has* been polled */
            *this.did_poll_once = true;

            /* 2. account for the time-to-first-poll of this task */
            // if the time-to-first-poll of this task exceeds `u64::MAX` ns,
            // round down to `u64::MAX` nanoseconds
            let elapsed = (poll_start - instrumented_at)
                .as_nanos()
                .try_into()
                .unwrap_or(u64::MAX);
            // add this duration to `time_to_first_poll_ns_total`
            metrics
                .total_first_poll_delay_ns
                .fetch_add(elapsed, SeqCst);

            /* 3. increment the count of tasks that have been polled at least once */
            state.metrics.total_first_poll_count.fetch_add(1, SeqCst);
        }

        /* accounting for time-idled and time-scheduled */
        // 1. note (and reset) the instant this task was last awoke
        let woke_at = state.woke_at.swap(0, SeqCst);

        // The state of a future is *idling* in the interim between the instant
        // it completes a `poll`, and the instant it is next awoken.
        if *idled_at < woke_at {
            // increment the counter of how many idles occured
            metrics.total_idled_count.fetch_add(1, SeqCst);

            // compute the duration of the idle
            let idle_ns = woke_at - *idled_at;

            // adjust the total elasped time monitored tasks spent idling
            metrics.total_idle_duration_ns.fetch_add(idle_ns, SeqCst);
        }

        // if this task spent any time in the scheduled state after instrumentation,
        // and after first poll, `woke_at` will be greater than 0.
        if woke_at > 0 {
            // increment the counter of how many schedules occured
            metrics.total_scheduled_count.fetch_add(1, SeqCst);

            // recall that the `woke_at` field is internally represented as
            // nanoseconds-since-instrumentation. here, for accounting purposes,
            // we need to instead represent it as a proper `Instant`.
            let woke_instant = instrumented_at + Duration::from_nanos(woke_at);

            // the duration this task spent scheduled is time time elapsed between
            // when this task was awoke, and when it was polled.
            let scheduled_ns = (poll_start - woke_instant)
                .as_nanos()
                .try_into()
                .unwrap_or(u64::MAX);

            // add `scheduled_ns` to the Monitor's total
            metrics.total_scheduled_duration_ns.fetch_add(scheduled_ns, SeqCst);
        }

        // Register the waker
        state.waker.register(cx.waker());

        // Get the instrumented waker
        let waker_ref = futures_util::task::waker_ref(&state);
        let mut cx = Context::from_waker(&*waker_ref);

        // Poll the task
        let inner_poll_start = Instant::now();
        let ret = Future::poll(this.task, &mut cx);
        let inner_poll_end = Instant::now();

        /* idle time starts now */
        *idled_at = (inner_poll_end - instrumented_at).as_nanos().try_into().unwrap_or(u64::MAX);

        /* accounting for poll time */
        let inner_poll_duration = inner_poll_end - inner_poll_start;
        let inner_poll_ns: u64 = inner_poll_duration
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX);

        let (count_bucket, duration_bucket) = // was this a slow or fast poll?
            if inner_poll_duration >= metrics.slow_poll_threshold {
                (&metrics.slow_polls_count, &metrics.total_slow_poll_duration)
            } else {
                (&metrics.total_fast_poll_count, &metrics.total_fast_poll_duration_ns)
            };

        // update the appropriate bucket
        count_bucket.fetch_add(1, SeqCst);
        duration_bucket.fetch_add(inner_poll_ns, SeqCst);

        ret
    }
}

impl State {
    fn on_wake(&self) {
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
}

impl ArcWake for State {
    fn wake_by_ref(arc_self: &Arc<State>) {
        arc_self.on_wake();
        arc_self.waker.wake();
    }

    fn wake(self: Arc<State>) {
        self.on_wake();
        self.waker.wake();
    }
}

#[inline(always)]
fn to_nanos(d: Duration) -> u64 {
    debug_assert!(d <= Duration::from_nanos(u64::MAX));
    (d.as_secs() as u64)
        .wrapping_mul(1_000_000_000)
        .wrapping_add(d.subsec_nanos() as u64)
}

#[inline(always)]
fn sub(a: Duration, b: Duration) -> Duration {
    let nanos = to_nanos(a).wrapping_sub(to_nanos(b));
    Duration::from_nanos(nanos)
}

#[inline(always)]
fn mean(d: Duration, count: u64) -> Duration {
    if let Some(quotient) = to_nanos(d).checked_div(count) {
        Duration::from_nanos(quotient)
    } else {
        Duration::ZERO
    }
}
