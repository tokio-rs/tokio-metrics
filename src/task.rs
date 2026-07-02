use futures_util::task::{ArcWake, AtomicWaker};
use pin_project_lite::pin_project;
use std::cell::RefCell;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use tokio_stream::Stream;

#[cfg(feature = "rt")]
use tokio::time::{Duration, Instant};

use crate::derived_metrics::derived_metrics;
#[cfg(not(feature = "rt"))]
use std::time::{Duration, Instant};

#[cfg(feature = "metrics-rs-integration")]
pub(crate) mod metrics_rs_integration;

/// Monitors key metrics of instrumented tasks.
///
/// This struct is preferred for generating a variable number of monitors at runtime.
/// If you can construct a fixed count of `static` monitors instead, see [`TaskMonitorCore`].
///
/// ### Basic Usage
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
/// async fn main() {
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
/// ### What should I instrument?
/// In most cases, you should construct a *distinct* [`TaskMonitor`] for each kind of key task.
///
/// #### Instrumenting a web application
/// For instance, a web service should have a distinct [`TaskMonitor`] for each endpoint. Within
/// each endpoint, it's prudent to additionally instrument major sub-tasks, each with their own
/// distinct [`TaskMonitor`]s. [*Why are my tasks slow?*](#why-are-my-tasks-slow) explores a
/// debugging scenario for a web service that takes this approach to instrumentation. This
/// approach is exemplified in the below example:
/// ```no_run
/// // The unabridged version of this snippet is in the examples directory of this crate.
///
/// #[tokio::main]
/// async fn main() {
///     // construct a TaskMonitor for root endpoint
///     let monitor_root = tokio_metrics::TaskMonitor::new();
///
///     // construct TaskMonitors for create_users endpoint
///     let monitor_create_user = CreateUserMonitors {
///         // monitor for the entire endpoint
///         route: tokio_metrics::TaskMonitor::new(),
///         // monitor for database insertion subtask
///         insert: tokio_metrics::TaskMonitor::new(),
///     };
///
///     // build our application with two instrumented endpoints
///     let app = axum::Router::new()
///         // `GET /` goes to `root`
///         .route("/", axum::routing::get({
///             let monitor = monitor_root.clone();
///             move || monitor.instrument(async { "Hello, World!" })
///         }))
///         // `POST /users` goes to `create_user`
///         .route("/users", axum::routing::post({
///             let monitors = monitor_create_user.clone();
///             let route = monitors.route.clone();
///             move |payload| {
///                 route.instrument(create_user(payload, monitors))
///             }
///         }));
///
///     // print task metrics for each endpoint every 1s
///     let metrics_frequency = std::time::Duration::from_secs(1);
///     tokio::spawn(async move {
///         let root_intervals = monitor_root.intervals();
///         let create_user_route_intervals =
///             monitor_create_user.route.intervals();
///         let create_user_insert_intervals =
///             monitor_create_user.insert.intervals();
///         let create_user_intervals =
///             create_user_route_intervals.zip(create_user_insert_intervals);
///
///         let intervals = root_intervals.zip(create_user_intervals);
///         for (root_route, (create_user_route, create_user_insert)) in intervals {
///             println!("root_route = {:#?}", root_route);
///             println!("create_user_route = {:#?}", create_user_route);
///             println!("create_user_insert = {:#?}", create_user_insert);
///             tokio::time::sleep(metrics_frequency).await;
///         }
///     });
///
///     // run the server
///     let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
///     let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
///     axum::serve(listener, app)
///         .await
///         .unwrap();
/// }
///
/// async fn create_user(
///     axum::Json(payload): axum::Json<CreateUser>,
///     monitors: CreateUserMonitors,
/// ) -> impl axum::response::IntoResponse {
///     let user = User { id: 1337, username: payload.username, };
///     // instrument inserting the user into the db:
///     let _ = monitors.insert.instrument(insert_user(user.clone())).await;
///     (axum::http::StatusCode::CREATED, axum::Json(user))
/// }
///
/// /* definitions of CreateUserMonitors, CreateUser and User omitted for brevity */
///
/// #
/// # #[derive(Clone)]
/// # struct CreateUserMonitors {
/// #     // monitor for the entire endpoint
/// #     route: tokio_metrics::TaskMonitor,
/// #     // monitor for database insertion subtask
/// #     insert: tokio_metrics::TaskMonitor,
/// # }
/// #
/// # #[derive(serde::Deserialize)] struct CreateUser { username: String, }
/// # #[derive(Clone, serde::Serialize)] struct User { id: u64, username: String, }
/// #
/// // insert the user into the database
/// async fn insert_user(_: User) {
///     /* implementation details elided */
///     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
/// }
/// ```
///
/// ### Why are my tasks slow?
/// **Scenario:** You track key, high-level metrics about the customer response time. An alarm warns
/// you that P90 latency for an endpoint exceeds your targets. What is causing the increase?
///
/// #### Identifying the high-level culprits
/// A set of tasks will appear to execute more slowly if:
/// - they are taking longer to poll (i.e., they consume too much CPU time)
/// - they are waiting longer to be polled (e.g., they're waiting longer in tokio's scheduling
///   queues)
/// - they are waiting longer on external events to complete (e.g., asynchronous network requests)
///
/// The culprits, at a high level, may be some combination of these sources of latency. Fortunately,
/// you have instrumented the key tasks of each of your endpoints with distinct [`TaskMonitor`]s.
/// Using the monitors on the endpoint experiencing elevated latency, you begin by answering:
/// - [*Are my tasks taking longer to poll?*](#are-my-tasks-taking-longer-to-poll)
/// - [*Are my tasks spending more time waiting to be polled?*](#are-my-tasks-spending-more-time-waiting-to-be-polled)
/// - [*Are my tasks spending more time waiting on external events to complete?*](#are-my-tasks-spending-more-time-waiting-on-external-events-to-complete)
///
/// ##### Are my tasks taking longer to poll?
/// - **Did [`mean_poll_duration`][TaskMetrics::mean_poll_duration] increase?**
///   This metric reflects the mean poll duration. If it increased, it means that, on average,
///   individual polls tended to take longer. However, this does not necessarily imply increased
///   task latency: An increase in poll durations could be offset by fewer polls.
/// - **Did [`slow_poll_ratio`][TaskMetrics::slow_poll_ratio] increase?**
///   This metric reflects the proportion of polls that were 'slow'. If it increased, it means that
///   a greater proportion of polls performed excessive computation before yielding. This does not
///   necessarily imply increased task latency: An increase in the proportion of slow polls could be
///   offset by fewer or faster polls.
/// - **Did [`mean_slow_poll_duration`][TaskMetrics::mean_slow_poll_duration] increase?**
///   This metric reflects the mean duration of slow polls. If it increased, it means that, on
///   average, slow polls got slower. This does not necessarily imply increased task latency: An
///   increase in average slow poll duration could be offset by fewer or faster polls.
///
/// If so, [*why are my tasks taking longer to poll?*](#why-are-my-tasks-taking-longer-to-poll)
///
/// ##### Are my tasks spending more time waiting to be polled?
/// - **Did [`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay] increase?**
///   This metric reflects the mean delay between the instant a task is first instrumented and the
///   instant it is first polled. If it increases, it means that, on average, tasks spent longer
///   waiting to be initially run.
/// - **Did [`mean_scheduled_duration`][TaskMetrics::mean_scheduled_duration] increase?**
///   This metric reflects the mean duration that tasks spent in the scheduled state. The
///   'scheduled' state of a task is the duration between the instant a task is awoken and the
///   instant it is subsequently polled. If this metric increases, it means that, on average, tasks
///   spent longer in tokio's queues before being polled.
/// - **Did [`long_delay_ratio`][TaskMetrics::long_delay_ratio] increase?**
///   This metric reflects the proportion of scheduling delays which were 'long'. If it increased,
///   it means that a greater proportion of tasks experienced excessive delays before they could
///   execute after being woken. This does not necessarily indicate an increase in latency, as this
///   could be offset by fewer or faster task polls.
/// - **Did [`mean_long_delay_duration`][TaskMetrics::mean_long_delay_duration] increase?**
///   This metric reflects the mean duration of long delays. If it increased, it means that, on
///   average, long delays got even longer. This does not necessarily imply increased task latency:
///   an increase in average long delay duration could be offset by fewer or faster polls or more
///   short schedules.
///
/// If so, [*why are my tasks spending more time waiting to be polled?*](#why-are-my-tasks-spending-more-time-waiting-to-be-polled)
///
/// ##### Are my tasks spending more time waiting on external events to complete?
/// - **Did [`mean_idle_duration`][TaskMetrics::mean_idle_duration] increase?**
///   This metric reflects the mean duration that tasks spent in the idle state. The idle state is
///   the duration spanning the instant a task completes a poll, and the instant that it is next
///   awoken. Tasks inhabit this state when they are waiting for task-external events to complete
///   (e.g., an asynchronous sleep, a network request, file I/O, etc.). If this metric increases,
///   tasks, in aggregate, spent more time waiting for task-external events to complete.
///
/// If so, [*why are my tasks spending more time waiting on external events to complete?*](#why-are-my-tasks-spending-more-time-waiting-on-external-events-to-complete)
///
/// #### Digging deeper
/// Having [established the high-level culprits](#identifying-the-high-level-culprits), you now
/// search for further explanation...
///
/// ##### Why are my tasks taking longer to poll?
/// You observed that [your tasks are taking longer to poll](#are-my-tasks-taking-longer-to-poll).
/// The culprit is likely some combination of:
/// - **Your tasks are accidentally blocking.** Common culprits include:
///     1. Using the Rust standard library's [filesystem](https://doc.rust-lang.org/std/fs/) or
///        [networking](https://doc.rust-lang.org/std/net/) APIs.
///        These APIs are synchronous; use tokio's [filesystem](https://docs.rs/tokio/latest/tokio/fs/)
///        and [networking](https://docs.rs/tokio/latest/tokio/net/) APIs, instead.
///     3. Calling [`block_on`](https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html#method.block_on).
///     4. Invoking `println!` or other synchronous logging routines.
///        Invocations of `println!` involve acquiring an exclusive lock on stdout, followed by a
///        synchronous write to stdout.
/// 2. **Your tasks are computationally expensive.** Common culprits include:
///     1. TLS/cryptographic routines
///     2. doing a lot of processing on bytes
///     3. calling non-Tokio resources
///
/// ##### Why are my tasks spending more time waiting to be polled?
/// You observed that [your tasks are spending more time waiting to be polled](#are-my-tasks-spending-more-time-waiting-to-be-polled)
/// suggesting some combination of:
/// - Your application is inflating the time elapsed between instrumentation and first poll.
/// - Your tasks are being scheduled into tokio's global queue.
/// - Other tasks are spending too long without yielding, thus backing up tokio's queues.
///
/// Start by asking: [*Is time-to-first-poll unusually high?*](#is-time-to-first-poll-unusually-high)
///
/// ##### Why are my tasks spending more time waiting on external events to complete?
/// You observed that [your tasks are spending more time waiting waiting on external events to
/// complete](#are-my-tasks-spending-more-time-waiting-on-external-events-to-complete). But what
/// event? Fortunately, within the task experiencing increased idle times, you monitored several
/// sub-tasks with distinct [`TaskMonitor`]s. For each of these sub-tasks, you [*you try to identify
/// the performance culprits...*](#identifying-the-high-level-culprits)
///
/// #### Digging even deeper
///
/// ##### Is time-to-first-poll unusually high?
/// Contrast these two metrics:
/// - **[`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay]**
///   This metric reflects the mean delay between the instant a task is first instrumented and the
///   instant it is *first* polled.
/// - **[`mean_scheduled_duration`][TaskMetrics::mean_scheduled_duration]**
///   This metric reflects the mean delay between the instant when tasks were awoken and the
///   instant they were subsequently polled.
///
/// If the former metric exceeds the latter (or increased unexpectedly more than the latter), then
/// start by investigating [*if your application is artificially delaying the time-to-first-poll*](#is-my-application-delaying-the-time-to-first-poll).
///
/// Otherwise, investigate [*if other tasks are polling too long without yielding*](#are-other-tasks-polling-too-long-without-yielding).
///
/// ##### Is my application delaying the time-to-first-poll?
/// You observed that [`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay] increased, more
/// than [`mean_scheduled_duration`][TaskMetrics::mean_scheduled_duration]. Your application may be
/// needlessly inflating the time elapsed between instrumentation and first poll. Are you
/// constructing (and instrumenting) tasks separately from awaiting or spawning them?
///
/// For instance, in the below example, the application induces 1 second delay between when `task`
/// is instrumented and when it is awaited:
/// ```rust
/// #[tokio::main]
/// async fn main() {
///     use tokio::time::Duration;
///     let monitor = tokio_metrics::TaskMonitor::new();
///
///     let task = monitor.instrument(async move {});
///
///     let one_sec = Duration::from_secs(1);
///     tokio::time::sleep(one_sec).await;
///
///     let _ = tokio::spawn(task).await;
///
///     assert!(monitor.cumulative().total_first_poll_delay >= one_sec);
/// }
/// ```
///
/// Otherwise, [`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay] might be unusually high
/// because [*your application is spawning key tasks into tokio's global queue...*](#is-my-application-spawning-more-tasks-into-tokio’s-global-queue)
///
/// ##### Is my application spawning more tasks into tokio's global queue?
/// Tasks awoken from threads *not* managed by the tokio runtime are scheduled with a slower,
/// global "injection" queue.
///
/// You may be notifying runtime tasks from off-runtime. For instance, Given the following:
/// ```ignore
/// #[tokio::main]
/// async fn main() {
///     for _ in 0..100 {
///         let (tx, rx) = oneshot::channel();
///         tokio::spawn(async move {
///             tx.send(());
///         })
///
///         rx.await;
///     }
/// }
/// ```
/// One would expect this to run efficiently, however, the main task is run *off* the main runtime
/// and the spawned tasks are *on* runtime, which means the snippet will run much slower than:
/// ```ignore
/// #[tokio::main]
/// async fn main() {
///     tokio::spawn(async {
///         for _ in 0..100 {
///             let (tx, rx) = oneshot::channel();
///             tokio::spawn(async move {
///                 tx.send(());
///             })
///
///             rx.await;
///         }
///     }).await;
/// }
/// ```
/// The slowdown is caused by a higher time between the `rx` task being notified (in `tx.send()`)
/// and the task being polled.
///
/// ##### Are other tasks polling too long without yielding?
/// You suspect that your tasks are slow because they're backed up in tokio's scheduling queues. For
/// *each* of your application's [`TaskMonitor`]s you check to see [*if their associated tasks are
/// taking longer to poll...*](#are-my-tasks-taking-longer-to-poll)
///
/// ### Limitations
/// The [`TaskMetrics`] type uses [`u64`] to represent both event counters and durations (measured
/// in nanoseconds). Consequently, event counters are accurate for ≤ [`u64::MAX`] events, and
/// durations are accurate for ≤ [`u64::MAX`] nanoseconds.
///
/// The counters and durations of [`TaskMetrics`] produced by [`TaskMonitor::cumulative`] increase
/// monotonically with each successive invocation of [`TaskMonitor::cumulative`]. Upon overflow,
/// counters and durations wrap.
///
/// The counters and durations of [`TaskMetrics`] produced by [`TaskMonitor::intervals`] are
/// calculated by computing the difference of metrics in successive invocations of
/// [`TaskMonitor::cumulative`]. If, within a monitoring interval, an event occurs more than
/// [`u64::MAX`] times, or a monitored duration exceeds [`u64::MAX`] nanoseconds, the metrics for
/// that interval will overflow and not be accurate.
///
/// ##### Examples at the limits
/// Consider the [`TaskMetrics::total_first_poll_delay`] metric. This metric accurately reflects
/// delays between instrumentation and first-poll ≤ [`u64::MAX`] nanoseconds:
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
/// [`total_first_poll_delay`][TaskMetrics::total_first_poll_delay] will overflow:
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
/// If *many* tasks are spawned, it will take far less than a [`u64::MAX`]-nanosecond delay to bring
/// this metric to the precipice of overflow:
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
/// let first_poll_count = u16::MAX as u64;
/// let mut tasks = Vec::with_capacity(first_poll_count as usize);
/// for _ in 0..first_poll_count { tasks.push(monitor.instrument(async {})); }
///
/// // this is the maximum duration representable by tokio_metrics
/// let max_duration = u64::MAX;
///
/// // let's advance the clock justenough such that all of the time-to-first-poll
/// // delays summed nearly equals `max_duration_nanos`, less some remainder...
/// let iffy_delay = max_duration / (first_poll_count as u64);
/// let small_remainder = max_duration % first_poll_count;
/// let _ = tokio::time::advance(Duration::from_nanos(iffy_delay)).await;
///
/// // ...then poll all of the instrumented tasks:
/// for task in tasks { task.await; }
///
/// // `total_first_poll_delay` is at the precipice of overflowing!
/// assert_eq!(
///     next_interval().total_first_poll_delay.as_nanos(),
///     (max_duration - small_remainder) as u128
/// );
/// assert_eq!(
///     monitor.cumulative().total_first_poll_delay.as_nanos(),
///     (max_duration - small_remainder) as u128
/// );
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
/// #
///  let first_poll_count = u16::MAX as u64;
///  let batch_size = first_poll_count / 3;
///
///  let max_duration_ns = u64::MAX;
///  let iffy_delay_ns = max_duration_ns / first_poll_count;
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
#[derive(Clone, Debug)]
pub struct TaskMonitor {
    base: Arc<TaskMonitorCore>,
}

impl Deref for TaskMonitor {
    type Target = TaskMonitorCore;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl AsRef<TaskMonitorCore> for TaskMonitor {
    fn as_ref(&self) -> &TaskMonitorCore {
        &self.base
    }
}

/// A non-`Clone`, non-allocated, static-friendly version of [`TaskMonitor`].
/// See full docs on the [`TaskMonitor`] struct.
///
/// You should use [`TaskMonitorCore`] if you have a known count of monitors
/// that you want to initialize as compile-time `static` structs.
///
/// You can also use [`TaskMonitorCore`] if you are already passing around an `Arc`-wrapped
/// struct that you want to store your monitor in. This way, you can avoid double-`Arc`'ing it.
///
/// For other most other non-static usage, [`TaskMonitor`] will be more ergonomic.
///
/// ##### Examples
///
/// Static usage:
/// ```
/// use tokio_metrics::TaskMonitorCore;
///
/// static MONITOR: TaskMonitorCore = TaskMonitorCore::new();
///
/// #[tokio::main]
/// async fn main() {
///     assert_eq!(MONITOR.cumulative().first_poll_count, 0);
///
///     MONITOR.instrument(async {}).await;
///     assert_eq!(MONITOR.cumulative().first_poll_count, 1);
/// }
/// ```
///
/// Usage with wrapper struct and [`TaskMonitorCore::instrument_with`]:
/// ```
/// use std::sync::Arc;
/// use tokio_metrics::TaskMonitorCore;
///
/// #[derive(Clone)]
/// struct SharedState(Arc<SharedStateInner>);
/// struct SharedStateInner {
///     monitor: TaskMonitorCore,
///     other_state: SomeOtherSharedState,
/// }
/// /// Imagine: a type that wasn't `Clone` that you want to pass around
/// /// in a similar way as the monitor
/// struct SomeOtherSharedState;
///
/// impl AsRef<TaskMonitorCore> for SharedState {
///     fn as_ref(&self) -> &TaskMonitorCore {
///         &self.0.monitor
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let state = SharedState(Arc::new(SharedStateInner {
///         monitor: TaskMonitorCore::new(),
///         other_state: SomeOtherSharedState,
///     }));
///
///     assert_eq!(state.0.monitor.cumulative().first_poll_count, 0);
///
///     TaskMonitorCore::instrument_with(async {}, state.clone()).await;
///     assert_eq!(state.0.monitor.cumulative().first_poll_count, 1);
/// }
/// ```
#[derive(Debug)]
pub struct TaskMonitorCore {
    metrics: RawMetrics,
    /// Whether instrumented tasks should publish their scheduling delay as a
    /// task-local for [`FutureMonitor`] to sample. Off by default so the common
    /// case pays nothing; enabled via
    /// [`TaskMonitorBuilder::publish_scheduling_delay`]. Kept out of
    /// [`RawMetrics`] so that struct's (heavily contended) layout is unchanged.
    record_scheduling_log: bool,
}

/// Provides an interface for constructing a [`TaskMonitor`] with specialized configuration
/// parameters.
#[derive(Clone, Debug, Default)]
pub struct TaskMonitorBuilder(TaskMonitorCoreBuilder);

impl TaskMonitorBuilder {
    /// Creates a new [`TaskMonitorBuilder`].
    pub fn new() -> Self {
        Self(TaskMonitorCoreBuilder::new())
    }

    /// Specifies the threshold at which polls are considered 'slow'.
    pub fn with_slow_poll_threshold(&mut self, threshold: Duration) -> &mut Self {
        self.0.slow_poll_threshold = Some(threshold);
        self
    }

    /// Specifies the threshold at which schedules are considered 'long'.
    pub fn with_long_delay_threshold(&mut self, threshold: Duration) -> &mut Self {
        self.0.long_delay_threshold = Some(threshold);
        self
    }

    /// Records each instrumented task's scheduling delay so that a
    /// [`FutureMonitor`] running within the task can attribute scheduling delay
    /// to a single future via [`TaskScheduling`].
    ///
    /// Off by default: tasks instrumented by a monitor without this enabled pay
    /// no extra per-poll cost. Enable it on the monitor wrapping the larger task
    /// when you want [`FutureMonitor`]'s scheduling metrics to be populated.
    pub fn publish_scheduling_delay(&mut self) -> &mut Self {
        self.0.record_scheduling_log = true;
        self
    }

    /// Consume the builder, producing a [`TaskMonitor`].
    pub fn build(self) -> TaskMonitor {
        TaskMonitor {
            base: Arc::new(self.0.build()),
        }
    }
}

/// Provides an interface for constructing a [`TaskMonitorCore`] with specialized configuration
/// parameters.
///
/// ```
/// use std::time::Duration;
/// use tokio_metrics::TaskMonitorCoreBuilder;
///
/// static MONITOR: tokio_metrics::TaskMonitorCore = TaskMonitorCoreBuilder::new()
///     .with_slow_poll_threshold(Duration::from_micros(100))
///     .build();
/// ```
#[derive(Clone, Debug, Default)]
pub struct TaskMonitorCoreBuilder {
    slow_poll_threshold: Option<Duration>,
    long_delay_threshold: Option<Duration>,
    record_scheduling_log: bool,
}

impl TaskMonitorCoreBuilder {
    /// Creates a new [`TaskMonitorCoreBuilder`].
    pub const fn new() -> Self {
        Self {
            slow_poll_threshold: None,
            long_delay_threshold: None,
            record_scheduling_log: false,
        }
    }

    /// Specifies the threshold at which polls are considered 'slow'.
    pub const fn with_slow_poll_threshold(self, threshold: Duration) -> Self {
        Self {
            slow_poll_threshold: Some(threshold),
            ..self
        }
    }

    /// Specifies the threshold at which schedules are considered 'long'.
    pub const fn with_long_delay_threshold(self, threshold: Duration) -> Self {
        Self {
            long_delay_threshold: Some(threshold),
            ..self
        }
    }

    /// Records each instrumented task's scheduling delay so that a
    /// [`FutureMonitor`] running within the task can attribute scheduling delay
    /// to a single future via [`TaskScheduling`].
    ///
    /// Off by default; see
    /// [`TaskMonitorBuilder::publish_scheduling_delay`] for details.
    pub const fn publish_scheduling_delay(self) -> Self {
        Self {
            record_scheduling_log: true,
            ..self
        }
    }

    /// Consume the builder, producing a [`TaskMonitorCore`].
    pub const fn build(self) -> TaskMonitorCore {
        let slow = match self.slow_poll_threshold {
            Some(v) => v,
            None => TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD,
        };
        let long = match self.long_delay_threshold {
            Some(v) => v,
            None => TaskMonitor::DEFAULT_LONG_DELAY_THRESHOLD,
        };
        TaskMonitorCore::create(slow, long, self.record_scheduling_log)
    }
}

pin_project! {
    /// An async task that has been instrumented with [`TaskMonitor::instrument`].
    #[derive(Debug)]
    pub struct Instrumented<T, M: AsRef<TaskMonitorCore> = TaskMonitor> {
        // The task being instrumented
        #[pin]
        task: T,

        // True when the task is polled for the first time
        did_poll_once: bool,

        // The instant, tracked as nanoseconds since `instrumented_at`, at which the future finished
        // its last poll.
        idled_at: u64,

        // State shared between the task and its instrumented waker.
        state: Arc<State<M>>,
    }

    impl<T, M: AsRef<TaskMonitorCore>> PinnedDrop for Instrumented<T, M> {
        fn drop(this: Pin<&mut Self>) {
            this.state.monitor.as_ref().metrics.dropped_count.fetch_add(1, SeqCst);
        }
    }
}

/// Key metrics of [instrumented][`TaskMonitor::instrument`] tasks.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskMetrics {
    /// The number of tasks instrumented.
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // 0 tasks have been instrumented
    ///     assert_eq!(next_interval().instrumented_count, 0);
    ///
    ///     monitor.instrument(async {});
    ///
    ///     // 1 task has been instrumented
    ///     assert_eq!(next_interval().instrumented_count, 1);
    ///
    ///     monitor.instrument(async {});
    ///     monitor.instrument(async {});
    ///
    ///     // 2 tasks have been instrumented
    ///     assert_eq!(next_interval().instrumented_count, 2);
    ///
    ///     // since the last interval was produced, 0 tasks have been instrumented
    ///     assert_eq!(next_interval().instrumented_count, 0);
    /// }
    /// ```
    pub instrumented_count: u64,

    /// The number of tasks dropped.
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // 0 tasks have been dropped
    ///     assert_eq!(next_interval().dropped_count, 0);
    ///
    ///     let _task = monitor.instrument(async {});
    ///
    ///     // 0 tasks have been dropped
    ///     assert_eq!(next_interval().dropped_count, 0);
    ///
    ///     monitor.instrument(async {}).await;
    ///     drop(monitor.instrument(async {}));
    ///
    ///     // 2 tasks have been dropped
    ///     assert_eq!(next_interval().dropped_count, 2);
    ///
    ///     // since the last interval was produced, 0 tasks have been dropped
    ///     assert_eq!(next_interval().dropped_count, 0);
    /// }
    /// ```
    pub dropped_count: u64,

    /// The number of tasks polled for the first time.
    ///
    /// ##### Derived metrics
    /// - **[`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay]**
    ///   The mean duration elapsed between the instant tasks are instrumented, and the instant they
    ///   are first polled.
    ///
    /// ##### Examples
    /// In the below example, no tasks are instrumented or polled in the first sampling interval;
    /// one task is instrumented (but not polled) in the second sampling interval; that task is
    /// awaited to completion (and, thus, polled at least once) in the third sampling interval; no
    /// additional tasks are polled for the first time within the fourth sampling interval:
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have been constructed, instrumented, and polled at least once
    ///     assert_eq!(next_interval().first_poll_count, 0);
    ///
    ///     let task = metrics_monitor.instrument(async {});
    ///
    ///     // `task` has been constructed and instrumented, but has not yet been polled
    ///     assert_eq!(next_interval().first_poll_count, 0);
    ///
    ///     // poll `task` to completion
    ///     task.await;
    ///
    ///     // `task` has been constructed, instrumented, and polled at least once
    ///     assert_eq!(next_interval().first_poll_count, 1);
    ///
    ///     // since the last interval was produced, 0 tasks have been constructed, instrumented and polled
    ///     assert_eq!(next_interval().first_poll_count, 0);
    ///
    /// }
    /// ```
    pub first_poll_count: u64,

    /// The total duration elapsed between the instant tasks are instrumented, and the instant they
    /// are first polled.
    ///
    /// ##### Derived metrics
    /// - **[`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay]**
    ///   The mean duration elapsed between the instant tasks are instrumented, and the instant they
    ///   are first polled.
    ///
    /// ##### Examples
    /// In the below example, 0 tasks have been instrumented or polled within the first sampling
    /// interval, a total of 500ms elapse between the instrumentation and polling of tasks within
    /// the second sampling interval, and a total of 350ms elapse between the instrumentation and
    /// polling of tasks within the third sampling interval:
    /// ```
    /// use tokio::time::Duration;
    ///
    /// #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // no tasks have yet been created, instrumented, or polled
    ///     assert_eq!(monitor.cumulative().total_first_poll_delay, Duration::ZERO);
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
    ///     instrument_pause_await(&monitor, task_a_pause_time).await;
    ///
    ///     assert_eq!(next_interval().total_first_poll_delay, task_a_pause_time);
    ///     assert_eq!(monitor.cumulative().total_first_poll_delay, task_a_pause_time);
    ///
    ///     // construct and await a task that pauses for 250ms between instrumentation and first poll
    ///     let task_b_pause_time = Duration::from_millis(250);
    ///     instrument_pause_await(&monitor, task_b_pause_time).await;
    ///
    ///     // construct and await a task that pauses for 100ms between instrumentation and first poll
    ///     let task_c_pause_time = Duration::from_millis(100);
    ///     instrument_pause_await(&monitor, task_c_pause_time).await;
    ///
    ///     assert_eq!(
    ///         next_interval().total_first_poll_delay,
    ///         task_b_pause_time + task_c_pause_time
    ///     );
    ///     assert_eq!(
    ///         monitor.cumulative().total_first_poll_delay,
    ///         task_a_pause_time + task_b_pause_time + task_c_pause_time
    ///     );
    /// }
    /// ```
    ///
    /// ##### When is this metric recorded?
    /// The delay between instrumentation and first poll is not recorded until the first poll
    /// actually occurs:
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
    /// ##### What if first-poll-delay is very large?
    /// The first-poll-delay of *individual* tasks saturates at `u64::MAX` nanoseconds. However, if
    /// the *total* first-poll-delay *across* monitored tasks exceeds `u64::MAX` nanoseconds, this
    /// metric will wrap around:
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

    /// The total number of times that tasks idled, waiting to be awoken.
    ///
    /// An idle is recorded as occurring if a non-zero duration elapses between the instant a
    /// task completes a poll, and the instant that it is next awoken.
    ///
    /// ##### Derived metrics
    /// - **[`mean_idle_duration`][TaskMetrics::mean_idle_duration]**
    ///   The mean duration of idles.
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = move || interval.next().unwrap();
    ///     let one_sec = std::time::Duration::from_secs(1);
    ///
    ///     monitor.instrument(async {}).await;
    ///
    ///     assert_eq!(next_interval().total_idled_count, 0);
    ///     assert_eq!(monitor.cumulative().total_idled_count, 0);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(one_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_idled_count, 1);
    ///     assert_eq!(monitor.cumulative().total_idled_count, 1);
    ///
    ///     monitor.instrument(async {
    ///         tokio::time::sleep(one_sec).await;
    ///         tokio::time::sleep(one_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_idled_count, 2);
    ///     assert_eq!(monitor.cumulative().total_idled_count, 3);
    /// }
    /// ```
    pub total_idled_count: u64,

    /// The total duration that tasks idled.
    ///
    /// An idle is recorded as occurring if a non-zero duration elapses between the instant a
    /// task completes a poll, and the instant that it is next awoken.
    ///
    /// ##### Derived metrics
    /// - **[`mean_idle_duration`][TaskMetrics::mean_idle_duration]**
    ///   The mean duration of idles.
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = move || interval.next().unwrap();
    ///     let one_sec = std::time::Duration::from_secs(1);
    ///     let two_sec = std::time::Duration::from_secs(2);
    ///
    ///     assert_eq!(next_interval().total_idle_duration.as_nanos(), 0);
    ///     assert_eq!(monitor.cumulative().total_idle_duration.as_nanos(), 0);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(one_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_idle_duration, one_sec);
    ///     assert_eq!(monitor.cumulative().total_idle_duration, one_sec);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(two_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_idle_duration, two_sec);
    ///     assert_eq!(monitor.cumulative().total_idle_duration, one_sec + two_sec);
    /// }
    /// ```
    pub total_idle_duration: Duration,

    /// The maximum idle duration that a task took.
    ///
    /// An idle is recorded as occurring if a non-zero duration elapses between the instant a
    /// task completes a poll, and the instant that it is next awoken.
    ///
    /// ##### Examples
    /// ```
    /// #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = move || interval.next().unwrap();
    ///     let one_sec = std::time::Duration::from_secs(1);
    ///     let two_sec = std::time::Duration::from_secs(2);
    ///
    ///     assert_eq!(next_interval().max_idle_duration.as_nanos(), 0);
    ///     assert_eq!(monitor.cumulative().max_idle_duration.as_nanos(), 0);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(one_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().max_idle_duration, one_sec);
    ///     assert_eq!(monitor.cumulative().max_idle_duration, one_sec);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(two_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().max_idle_duration, two_sec);
    ///     assert_eq!(monitor.cumulative().max_idle_duration, two_sec);
    ///
    ///     monitor.instrument(async move {
    ///         tokio::time::sleep(one_sec).await;
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().max_idle_duration, one_sec);
    ///     assert_eq!(monitor.cumulative().max_idle_duration, two_sec);
    /// }
    /// ```
    pub max_idle_duration: Duration,

    /// The total number of times that tasks were awoken (and then, presumably, scheduled for
    /// execution).
    ///
    /// ##### Definition
    /// This metric is equal to [`total_short_delay_count`][TaskMetrics::total_short_delay_count]
    /// \+ [`total_long_delay_count`][TaskMetrics::total_long_delay_count].
    ///
    /// ##### Derived metrics
    /// - **[`mean_scheduled_duration`][TaskMetrics::mean_scheduled_duration]**
    ///   The mean duration that tasks spent waiting to be executed after awakening.
    ///
    /// ##### Examples
    /// In the below example, a task yields to the scheduler a varying number of times between
    /// sampling intervals; this metric is equal to the number of times the task yielded:
    /// ```
    /// #[tokio::main]
    /// async fn main(){
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
    ///             // scheduled once) since the last sampling interval
    ///             assert_eq!(next_interval().total_scheduled_count, 1);
    ///
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///
    ///             // [G] `task` has yielded to the scheduler thrice (and thus been
    ///             // scheduled thrice) since the last sampling interval
    ///             assert_eq!(next_interval().total_scheduled_count, 3);
    ///
    ///             tokio::task::yield_now().await; // yield to the scheduler
    ///
    ///             next_interval
    ///         })
    ///     };
    ///
    ///     // [C] `task` has not yet been polled at all
    ///     assert_eq!(metrics_monitor.cumulative().first_poll_count, 0);
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
    /// }
    /// ```
    #[doc(alias = "total_delay_count")]
    pub total_scheduled_count: u64,

    /// The total duration that tasks spent waiting to be polled after awakening.
    ///
    /// ##### Definition
    /// This metric is equal to [`total_short_delay_duration`][TaskMetrics::total_short_delay_duration]
    /// \+ [`total_long_delay_duration`][TaskMetrics::total_long_delay_duration].
    ///
    /// ##### Derived metrics
    /// - **[`mean_scheduled_duration`][TaskMetrics::mean_scheduled_duration]**
    ///   The mean duration that tasks spent waiting to be executed after awakening.
    ///
    /// ##### Examples
    /// ```
    /// use tokio::time::Duration;
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = metrics_monitor.intervals();
    ///     let mut next_interval = || interval.next().unwrap();
    ///
    ///     // construct and instrument and spawn a task that yields endlessly
    ///     tokio::spawn(metrics_monitor.instrument(async {
    ///         loop { tokio::task::yield_now().await }
    ///     }));
    ///
    ///     tokio::task::yield_now().await;
    ///
    ///     // block the executor for 1 second
    ///     std::thread::sleep(Duration::from_millis(1000));
    ///
    ///     tokio::task::yield_now().await;
    ///
    ///     // `endless_task` will have spent approximately one second waiting
    ///     let total_scheduled_duration = next_interval().total_scheduled_duration;
    ///     assert!(total_scheduled_duration >= Duration::from_millis(1000));
    ///     assert!(total_scheduled_duration <= Duration::from_millis(1100));
    /// }
    /// ```
    #[doc(alias = "total_delay_duration")]
    pub total_scheduled_duration: Duration,

    /// The total number of times that tasks were polled.
    ///
    /// ##### Definition
    /// This metric is equal to [`total_fast_poll_count`][TaskMetrics::total_fast_poll_count]
    /// \+ [`total_slow_poll_count`][TaskMetrics::total_slow_poll_count].
    ///
    /// ##### Derived metrics
    /// - **[`mean_poll_duration`][TaskMetrics::mean_poll_duration]**
    ///   The mean duration of polls.
    ///
    /// ##### Examples
    /// In the below example, a task with multiple yield points is await'ed to completion; this
    /// metric reflects the number of `await`s within each sampling interval:
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // [A] no tasks have been created, instrumented, and polled more than once
    ///     assert_eq!(metrics_monitor.cumulative().first_poll_count, 0);
    ///
    ///     // [B] a `task` is created and instrumented
    ///     let task = {
    ///         let monitor = metrics_monitor.clone();
    ///         metrics_monitor.instrument(async move {
    ///             let mut interval = monitor.intervals();
    ///             let mut next_interval = move || interval.next().unwrap();
    ///
    ///             // [E] task is in the midst of its first poll
    ///             assert_eq!(next_interval().total_poll_count, 0);
    ///
    ///             tokio::task::yield_now().await; // poll 1
    ///
    ///             // [F] task has been polled 1 time
    ///             assert_eq!(next_interval().total_poll_count, 1);
    ///
    ///             tokio::task::yield_now().await; // poll 2
    ///             tokio::task::yield_now().await; // poll 3
    ///             tokio::task::yield_now().await; // poll 4
    ///
    ///             // [G] task has been polled 3 times
    ///             assert_eq!(next_interval().total_poll_count, 3);
    ///
    ///             tokio::task::yield_now().await; // poll 5
    ///
    ///             next_interval                   // poll 6
    ///         })
    ///     };
    ///
    ///     // [C] `task` has not yet been polled at all
    ///     assert_eq!(metrics_monitor.cumulative().total_poll_count, 0);
    ///
    ///     // [D] poll `task` to completion
    ///     let mut next_interval = task.await;
    ///
    ///     // [H] `task` has been polled 2 times since the last sample
    ///     assert_eq!(next_interval().total_poll_count, 2);
    ///
    ///     // [I] `task` has been polled 0 times since the last sample
    ///     assert_eq!(next_interval().total_poll_count, 0);
    ///
    ///     // [J] `task` has been polled 6 times
    ///     assert_eq!(metrics_monitor.cumulative().total_poll_count, 6);
    /// }
    /// ```
    pub total_poll_count: u64,

    /// The total duration elapsed during polls.
    ///
    /// ##### Definition
    /// This metric is equal to [`total_fast_poll_duration`][TaskMetrics::total_fast_poll_duration]
    /// \+ [`total_slow_poll_duration`][TaskMetrics::total_slow_poll_duration].
    ///
    /// ##### Derived metrics
    /// - **[`mean_poll_duration`][TaskMetrics::mean_poll_duration]**
    ///   The mean duration of polls.
    ///
    /// #### Examples
    /// ```
    /// use tokio::time::Duration;
    ///
    /// #[tokio::main(flavor = "current_thread", start_paused = true)]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///     let mut interval = monitor.intervals();
    ///     let mut next_interval = move || interval.next().unwrap();
    ///
    ///     assert_eq!(next_interval().total_poll_duration, Duration::ZERO);
    ///
    ///     monitor.instrument(async {
    ///         tokio::time::advance(Duration::from_secs(1)).await; // poll 1 (1s)
    ///         tokio::time::advance(Duration::from_secs(1)).await; // poll 2 (1s)
    ///         ()                                                  // poll 3 (0s)
    ///     }).await;
    ///
    ///     assert_eq!(next_interval().total_poll_duration, Duration::from_secs(2));
    /// }
    /// ```
    pub total_poll_duration: Duration,

    /// The total number of times that polling tasks completed swiftly.
    ///
    /// Here, 'swiftly' is defined as completing in strictly less time than
    /// [`slow_poll_threshold`][TaskMonitor::slow_poll_threshold].
    ///
    /// ##### Derived metrics
    /// - **[`mean_fast_poll_duration`][TaskMetrics::mean_fast_poll_duration]**
    ///   The mean duration of fast polls.
    ///
    /// ##### Examples
    /// In the below example, 0 polls occur within the first sampling interval, 3 fast polls occur
    /// within the second sampling interval, and 2 fast polls occur within the third sampling
    /// interval:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
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

    /// The total duration of fast polls.
    ///
    /// Here, 'fast' is defined as completing in strictly less time than
    /// [`slow_poll_threshold`][TaskMonitor::slow_poll_threshold].
    ///
    /// ##### Derived metrics
    /// - **[`mean_fast_poll_duration`][TaskMetrics::mean_fast_poll_duration]**
    ///   The mean duration of fast polls.
    ///
    /// ##### Examples
    /// In the below example, no tasks are polled in the first sampling interval; three fast polls
    /// consume a total of 3μs time in the second sampling interval; and two fast polls consume a
    /// total of 2μs time in the third sampling interval:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
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

    /// The total number of times that polling tasks completed slowly.
    ///
    /// Here, 'slowly' is defined as completing in at least as much time as
    /// [`slow_poll_threshold`][TaskMonitor::slow_poll_threshold].
    ///
    /// ##### Derived metrics
    /// - **[`mean_slow_poll_duration`][`TaskMetrics::mean_slow_poll_duration`]**
    ///   The mean duration of slow polls.
    ///
    /// ##### Examples
    /// In the below example, 0 polls occur within the first sampling interval, 3 slow polls occur
    /// within the second sampling interval, and 2 slow polls occur within the third sampling
    /// interval:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
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

    /// The total duration of slow polls.
    ///
    /// Here, 'slowly' is defined as completing in at least as much time as
    /// [`slow_poll_threshold`][TaskMonitor::slow_poll_threshold].
    ///
    /// ##### Derived metrics
    /// - **[`mean_slow_poll_duration`][`TaskMetrics::mean_slow_poll_duration`]**
    ///   The mean duration of slow polls.
    ///
    /// ##### Examples
    /// In the below example, no tasks are polled in the first sampling interval; three slow polls
    /// consume a total of
    /// 30 × [`DEFAULT_SLOW_POLL_THRESHOLD`][TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD]
    /// time in the second sampling interval; and two slow polls consume a total of
    /// 20 × [`DEFAULT_SLOW_POLL_THRESHOLD`][TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD] time in the
    /// third sampling interval:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
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

    /// The total count of tasks with short scheduling delays.
    ///
    /// This is defined as tasks taking strictly less than
    /// [`long_delay_threshold`][TaskMonitor::long_delay_threshold] to be executed after being
    /// scheduled.
    ///
    /// ##### Derived metrics
    /// - **[`mean_short_delay_duration`][TaskMetrics::mean_short_delay_duration]**
    ///   The mean duration of short scheduling delays.
    pub total_short_delay_count: u64,

    /// The total duration of tasks with short scheduling delays.
    ///
    /// This is defined as tasks taking strictly less than
    /// [`long_delay_threshold`][TaskMonitor::long_delay_threshold] to be executed after being
    /// scheduled.
    ///
    /// ##### Derived metrics
    /// - **[`mean_short_delay_duration`][TaskMetrics::mean_short_delay_duration]**
    ///   The mean duration of short scheduling delays.
    pub total_short_delay_duration: Duration,

    /// The total count of tasks with long scheduling delays.
    ///
    /// This is defined as tasks taking
    /// [`long_delay_threshold`][TaskMonitor::long_delay_threshold] or longer to be executed
    /// after being scheduled.
    ///
    /// ##### Derived metrics
    /// - **[`mean_long_delay_duration`][TaskMetrics::mean_long_delay_duration]**
    ///   The mean duration of short scheduling delays.
    pub total_long_delay_count: u64,

    /// The total duration of tasks with long scheduling delays.
    ///
    /// This is defined as tasks taking
    /// [`long_delay_threshold`][TaskMonitor::long_delay_threshold] or longer to be executed
    /// after being scheduled.
    ///
    /// ##### Derived metrics
    /// - **[`mean_long_delay_duration`][TaskMetrics::mean_long_delay_duration]**
    ///   The mean duration of short scheduling delays.
    pub total_long_delay_duration: Duration,
}

/// Tracks the metrics, shared across the various types.
#[derive(Debug)]
struct RawMetrics {
    /// A task poll takes longer than this, it is considered a slow poll.
    slow_poll_threshold: Duration,

    /// A scheduling delay of at least this long will be considered a long delay
    long_delay_threshold: Duration,

    /// Total number of instrumented tasks.
    instrumented_count: AtomicU64,

    /// Total number of instrumented tasks polled at least once.
    first_poll_count: AtomicU64,

    /// Total number of times tasks entered the `idle` state.
    total_idled_count: AtomicU64,

    /// Total number of times tasks were scheduled.
    total_scheduled_count: AtomicU64,

    /// Total number of times tasks were polled fast
    total_fast_poll_count: AtomicU64,

    /// Total number of times tasks were polled slow
    total_slow_poll_count: AtomicU64,

    /// Total number of times tasks had long delay,
    total_long_delay_count: AtomicU64,

    /// Total number of times tasks had little delay
    total_short_delay_count: AtomicU64,

    /// Total number of times tasks were dropped
    dropped_count: AtomicU64,

    /// Total amount of time until the first poll
    total_first_poll_delay_ns: AtomicU64,

    /// Total amount of time tasks spent in the `idle` state.
    total_idle_duration_ns: AtomicU64,

    /// The longest time tasks spent in the `idle` state locally.
    /// This will be used to track the local max between interval
    /// metric snapshots.
    local_max_idle_duration_ns: AtomicU64,

    /// The longest time tasks spent in the `idle` state.
    global_max_idle_duration_ns: AtomicU64,

    /// Total amount of time tasks spent in the waking state.
    total_scheduled_duration_ns: AtomicU64,

    /// Total amount of time tasks spent being polled below the slow cut off.
    total_fast_poll_duration_ns: AtomicU64,

    /// Total amount of time tasks spent being polled above the slow cut off.
    total_slow_poll_duration: AtomicU64,

    /// Total amount of time tasks spent being polled below the long delay cut off.
    total_short_delay_duration_ns: AtomicU64,

    /// Total amount of time tasks spent being polled at or above the long delay cut off.
    total_long_delay_duration_ns: AtomicU64,
}

#[derive(Debug)]
struct State<M> {
    /// Where metrics should be recorded
    monitor: M,

    /// Instant at which the task was instrumented. This is used to track the time to first poll.
    instrumented_at: Instant,

    /// The instant, tracked as nanoseconds since `instrumented_at`, at which the future
    /// was last woken.
    woke_at: AtomicU64,

    /// Waker to forward notifications to.
    waker: AtomicWaker,

    /// Cumulative scheduling info for this task, published as a task-local while
    /// the task is being polled so that nested futures (e.g. a per-future
    /// [`FutureMonitor`]) can attribute scheduling delay to themselves. `None`
    /// unless the monitor opted in via
    /// [`TaskMonitorBuilder::publish_scheduling_delay`], so the common case
    /// allocates nothing and pays no per-poll cost.
    log: Option<Arc<SchedulingLog>>,
}

/// Cumulative scheduling-delay counters for a single instrumented task.
///
/// Scheduling delay (the time a task spends in the runtime's queues between
/// being woken and being polled) is only observable by the root future that the
/// runtime actually schedules. [`Instrumented`] records it here and publishes
/// the log as a task-local for the duration of each poll, so that a future
/// running *within* the task can read the delay accrued during its own lifetime
/// via [`TaskScheduling::try_current`].
#[derive(Debug, Default)]
struct SchedulingLog {
    scheduled_count: AtomicU64,
    scheduled_duration_ns: AtomicU64,
    long_delay_count: AtomicU64,
}

thread_local! {
    /// The scheduling log of the innermost [`Instrumented`] task currently being
    /// polled on this thread, if any.
    ///
    /// This is a hand-rolled task-local built on a `thread_local!`: it is only
    /// set for the duration of a single synchronous poll (see
    /// [`SchedulingLogGuard`]) and never held across an `.await`, so the usual
    /// hazard of thread-locals in async code does not apply.
    static CURRENT_SCHEDULING_LOG: RefCell<Option<Arc<SchedulingLog>>> =
        const { RefCell::new(None) };
}

/// Sets the current task's [`SchedulingLog`] for the duration of a poll,
/// restoring the previous value on drop. This mirrors what
/// `tokio::task::LocalKey::sync_scope` does, but without requiring the optional
/// `tokio` dependency, so it works with and without the `rt` feature.
struct SchedulingLogGuard(Option<Arc<SchedulingLog>>);

impl SchedulingLogGuard {
    fn enter(log: Arc<SchedulingLog>) -> Self {
        let prev = CURRENT_SCHEDULING_LOG.with(|cell| cell.borrow_mut().replace(log));
        SchedulingLogGuard(prev)
    }
}

impl Drop for SchedulingLogGuard {
    fn drop(&mut self) {
        let prev = self.0.take();
        CURRENT_SCHEDULING_LOG.with(|cell| *cell.borrow_mut() = prev);
    }
}

/// A snapshot of the cumulative scheduling delay observed by the root
/// instrumented task that the current future is running on.
///
/// Obtain one with [`TaskScheduling::try_current`] from inside a task that has
/// been instrumented with [`TaskMonitor::instrument`]. Scheduling delay can only
/// be measured by the root future the runtime schedules, so a future running
/// within a larger task samples this at two points and reports the difference —
/// this is exactly what [`FutureMonitor`] does to attribute scheduling delay to
/// a single future.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskScheduling {
    /// The number of times the task was scheduled (woken, then waiting to be polled).
    pub scheduled_count: u64,
    /// The total time the task spent waiting to be polled after being woken.
    pub total_scheduled_duration: Duration,
    /// The number of scheduling delays that crossed the monitor's long-delay threshold.
    pub long_delay_count: u64,
}

impl TaskScheduling {
    /// Reads the cumulative scheduling delay of the task currently being polled.
    ///
    /// Returns `None` when called outside the poll of a future instrumented with
    /// [`TaskMonitor::instrument`].
    pub fn try_current() -> Option<TaskScheduling> {
        CURRENT_SCHEDULING_LOG.with(|cell| {
            cell.borrow().as_ref().map(|log| TaskScheduling {
                scheduled_count: log.scheduled_count.load(SeqCst),
                total_scheduled_duration: Duration::from_nanos(
                    log.scheduled_duration_ns.load(SeqCst),
                ),
                long_delay_count: log.long_delay_count.load(SeqCst),
            })
        })
    }

    /// The change in each counter relative to an `earlier` snapshot, saturating at zero.
    fn since(self, earlier: TaskScheduling) -> TaskScheduling {
        TaskScheduling {
            scheduled_count: self.scheduled_count.saturating_sub(earlier.scheduled_count),
            total_scheduled_duration: self
                .total_scheduled_duration
                .saturating_sub(earlier.total_scheduled_duration),
            long_delay_count: self
                .long_delay_count
                .saturating_sub(earlier.long_delay_count),
        }
    }
}

impl TaskMonitor {
    /// The default duration at which polls cross the threshold into being categorized as 'slow' is
    /// 50μs.
    #[cfg(not(test))]
    pub const DEFAULT_SLOW_POLL_THRESHOLD: Duration = Duration::from_micros(50);
    #[cfg(test)]
    #[allow(missing_docs)]
    pub const DEFAULT_SLOW_POLL_THRESHOLD: Duration = Duration::from_millis(500);

    /// The default duration at which schedules cross the threshold into being categorized as 'long'
    /// is 50μs.
    #[cfg(not(test))]
    pub const DEFAULT_LONG_DELAY_THRESHOLD: Duration = Duration::from_micros(50);
    #[cfg(test)]
    #[allow(missing_docs)]
    pub const DEFAULT_LONG_DELAY_THRESHOLD: Duration = Duration::from_millis(500);

    /// Constructs a new task monitor.
    ///
    /// Uses [`Self::DEFAULT_SLOW_POLL_THRESHOLD`] as the threshold at which polls will be
    /// considered 'slow'.
    ///
    /// Uses [`Self::DEFAULT_LONG_DELAY_THRESHOLD`] as the threshold at which scheduling will be
    /// considered 'long'.
    pub fn new() -> TaskMonitor {
        TaskMonitor::with_slow_poll_threshold(Self::DEFAULT_SLOW_POLL_THRESHOLD)
    }

    /// Constructs a builder for a task monitor.
    pub fn builder() -> TaskMonitorBuilder {
        TaskMonitorBuilder::new()
    }

    /// Constructs a new task monitor with a given threshold at which polls are considered 'slow'.
    ///
    /// ##### Selecting an appropriate threshold
    /// TODO. What advice can we give here?
    ///
    /// ##### Examples
    /// In the below example, low-threshold and high-threshold monitors are constructed and
    /// instrument identical tasks; the low-threshold monitor reports4 slow polls, and the
    /// high-threshold monitor reports only 2 slow polls:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    /// use tokio_metrics::TaskMonitor;
    ///
    /// #[tokio::main]
    /// async fn main() {
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
        let base =
            TaskMonitorCore::create(slow_poll_cut_off, Self::DEFAULT_LONG_DELAY_THRESHOLD, false);
        TaskMonitor {
            base: Arc::new(base),
        }
    }

    /// Produces the duration greater-than-or-equal-to at which polls are categorized as slow.
    ///
    /// ##### Examples
    /// In the below example, [`TaskMonitor`] is initialized with [`TaskMonitor::new`];
    /// consequently, its slow-poll threshold equals [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`]:
    /// ```
    /// use tokio_metrics::TaskMonitor;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let metrics_monitor = TaskMonitor::new();
    ///
    ///     assert_eq!(
    ///         metrics_monitor.slow_poll_threshold(),
    ///         TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD
    ///     );
    /// }
    /// ```
    pub fn slow_poll_threshold(&self) -> Duration {
        self.base.metrics.slow_poll_threshold
    }

    /// Produces the duration greater-than-or-equal-to at which scheduling delays are categorized
    /// as long.
    pub fn long_delay_threshold(&self) -> Duration {
        self.base.metrics.long_delay_threshold
    }

    /// Produces an instrumented façade around a given async task.
    ///
    /// ##### Examples
    /// Instrument an async task by passing it to [`TaskMonitor::instrument`]:
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // 0 tasks have been instrumented, much less polled
    ///     assert_eq!(metrics_monitor.cumulative().first_poll_count, 0);
    ///
    ///     // instrument a task and poll it to completion
    ///     metrics_monitor.instrument(async {}).await;
    ///
    ///     // 1 task has been instrumented and polled
    ///     assert_eq!(metrics_monitor.cumulative().first_poll_count, 1);
    ///
    ///     // instrument a task and poll it to completion
    ///     metrics_monitor.instrument(async {}).await;
    ///
    ///     // 2 tasks have been instrumented and polled
    ///     assert_eq!(metrics_monitor.cumulative().first_poll_count, 2);
    /// }
    /// ```
    /// An aync task may be tracked by multiple [`TaskMonitor`]s; e.g.:
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let monitor_a = tokio_metrics::TaskMonitor::new();
    ///     let monitor_b = tokio_metrics::TaskMonitor::new();
    ///
    ///     // 0 tasks have been instrumented, much less polled
    ///     assert_eq!(monitor_a.cumulative().first_poll_count, 0);
    ///     assert_eq!(monitor_b.cumulative().first_poll_count, 0);
    ///
    ///     // instrument a task and poll it to completion
    ///     monitor_a.instrument(monitor_b.instrument(async {})).await;
    ///
    ///     // 1 task has been instrumented and polled
    ///     assert_eq!(monitor_a.cumulative().first_poll_count, 1);
    ///     assert_eq!(monitor_b.cumulative().first_poll_count, 1);
    /// }
    /// ```
    /// It is also possible (but probably undesirable) to instrument an async task multiple times
    /// with the same [`TaskMonitor`]; e.g.:
    /// ```
    /// #[tokio::main]
    /// async fn main() {
    ///     let monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // 0 tasks have been instrumented, much less polled
    ///     assert_eq!(monitor.cumulative().first_poll_count, 0);
    ///
    ///     // instrument a task and poll it to completion
    ///     monitor.instrument(monitor.instrument(async {})).await;
    ///
    ///     // 2 tasks have been instrumented and polled, supposedly
    ///     assert_eq!(monitor.cumulative().first_poll_count, 2);
    /// }
    /// ```
    pub fn instrument<F>(&self, task: F) -> Instrumented<F> {
        TaskMonitorCore::instrument_with(task, self.clone())
    }

    /// Produces [`TaskMetrics`] for the tasks instrumented by this [`TaskMonitor`], collected since
    /// the construction of [`TaskMonitor`].
    ///
    /// ##### See also
    /// - [`TaskMonitor::intervals`]:
    ///   produces [`TaskMetrics`] for user-defined sampling intervals, instead of cumulatively
    ///
    /// ##### Examples
    /// In the below example, 0 polls occur within the first sampling interval, 3 slow polls occur
    /// within the second sampling interval, and 2 slow polls occur within the third sampling
    /// interval; five slow polls occur across all sampling intervals:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // initialize a stream of sampling intervals
    ///     let mut intervals = metrics_monitor.intervals();
    ///     // each call of `next_interval` will produce metrics for the last sampling interval
    ///     let mut next_interval = || intervals.next().unwrap();
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
    ///     // in the previous sampling interval, there were 3 slow polls
    ///     assert_eq!(next_interval().total_slow_poll_count, 3);
    ///     assert_eq!(metrics_monitor.cumulative().total_slow_poll_count, 3);
    ///
    ///     // this task completes in two slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow)        // slow poll 2
    ///     }).await;
    ///
    ///     // in the previous sampling interval, there were 2 slow polls
    ///     assert_eq!(next_interval().total_slow_poll_count, 2);
    ///
    ///     // across all sampling interval, there were a total of 5 slow polls
    ///     assert_eq!(metrics_monitor.cumulative().total_slow_poll_count, 5);
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
        self.base.metrics.metrics()
    }

    /// Produces an unending iterator of metric sampling intervals.
    ///
    /// Each sampling interval is defined by the time elapsed between advancements of the iterator
    /// produced by [`TaskMonitor::intervals`]. The item type of this iterator is [`TaskMetrics`],
    /// which is a bundle of task metrics that describe *only* events occurring within that sampling
    /// interval.
    ///
    /// ##### Examples
    /// In the below example, 0 polls occur within the first sampling interval, 3 slow polls occur
    /// within the second sampling interval, and 2 slow polls occur within the third sampling
    /// interval; five slow polls occur across all sampling intervals:
    /// ```
    /// use std::future::Future;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
    ///
    ///     // initialize a stream of sampling intervals
    ///     let mut intervals = metrics_monitor.intervals();
    ///     // each call of `next_interval` will produce metrics for the last sampling interval
    ///     let mut next_interval = || intervals.next().unwrap();
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
    ///     // in the previous sampling interval, there were 3 slow polls
    ///     assert_eq!(next_interval().total_slow_poll_count, 3);
    ///
    ///     // this task completes in two slow polls
    ///     let _ = metrics_monitor.instrument(async {
    ///         spin_for(slow).await; // slow poll 1
    ///         spin_for(slow)        // slow poll 2
    ///     }).await;
    ///
    ///     // in the previous sampling interval, there were 2 slow polls
    ///     assert_eq!(next_interval().total_slow_poll_count, 2);
    ///
    ///     // across all sampling intervals, there were a total of 5 slow polls
    ///     assert_eq!(metrics_monitor.cumulative().total_slow_poll_count, 5);
    /// }
    ///
    /// /// Block the current thread for a given `duration`, then (optionally) yield to the scheduler.
    /// fn spin_for(duration: Duration) -> impl Future<Output=()> {
    ///     let start = tokio::time::Instant::now();
    ///     while start.elapsed() <= duration {}
    ///     tokio::task::yield_now()
    /// }
    /// ```
    pub fn intervals(&self) -> TaskIntervals {
        TaskIntervals {
            monitor: self.clone(),
            previous: None,
        }
    }
}

impl TaskMonitorCore {
    /// Returns a const-friendly [`TaskMonitorCoreBuilder`].
    pub const fn builder() -> TaskMonitorCoreBuilder {
        TaskMonitorCoreBuilder::new()
    }

    /// Constructs a new [`TaskMonitorCore`]. Refer to the struct documentation for more discussion
    /// of benefits compared to [`TaskMonitor`].
    ///
    /// Uses [`TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD`] as the threshold at which polls will be
    /// considered 'slow'.
    ///
    /// Uses [`TaskMonitor::DEFAULT_LONG_DELAY_THRESHOLD`] as the threshold at which scheduling will be
    /// considered 'long'.
    pub const fn new() -> TaskMonitorCore {
        TaskMonitorCore::with_slow_poll_threshold(TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD)
    }

    /// Constructs a new task monitor with a given threshold at which polls are considered 'slow'.
    ///
    /// Refer to [`TaskMonitor::with_slow_poll_threshold`] for examples.
    pub const fn with_slow_poll_threshold(slow_poll_cut_off: Duration) -> TaskMonitorCore {
        Self::create(
            slow_poll_cut_off,
            TaskMonitor::DEFAULT_LONG_DELAY_THRESHOLD,
            false,
        )
    }

    /// Produces the duration greater-than-or-equal-to at which polls are categorized as slow.
    ///
    /// Refer to [`TaskMonitor::slow_poll_threshold`] for examples.
    pub fn slow_poll_threshold(&self) -> Duration {
        self.metrics.slow_poll_threshold
    }

    /// Produces the duration greater-than-or-equal-to at which scheduling delays are categorized
    /// as long.
    pub fn long_delay_threshold(&self) -> Duration {
        self.metrics.long_delay_threshold
    }

    /// Produces an instrumented façade around a given async task.
    ///
    /// ##### Examples
    /// ```
    /// use tokio_metrics::TaskMonitorCore;
    ///
    /// static MONITOR: TaskMonitorCore = TaskMonitorCore::new();
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     assert_eq!(MONITOR.cumulative().first_poll_count, 0);
    ///
    ///     MONITOR.instrument(async {}).await;
    ///     assert_eq!(MONITOR.cumulative().first_poll_count, 1);
    /// }
    /// ```
    pub fn instrument<F>(&'static self, task: F) -> Instrumented<F, &'static Self> {
        Self::instrument_with(task, self)
    }

    /// Produces an instrumented façade around a given async task, with an explicit monitor.
    ///
    /// Use this when you have a non-static monitor reference, such as an `Arc<TaskMonitorCore>`.
    ///
    /// ##### Examples
    /// ```
    /// use std::sync::Arc;
    /// use tokio_metrics::TaskMonitorCore;
    ///
    /// #[derive(Clone)]
    /// struct SharedState(Arc<SharedStateInner>);
    /// struct SharedStateInner {
    ///     monitor: TaskMonitorCore,
    ///     other_state: SomeOtherSharedState,
    /// }
    /// /// Imagine: a type that wasn't `Clone` that you want to pass around
    /// /// in a similar way as the monitor
    /// struct SomeOtherSharedState;
    ///
    /// impl AsRef<TaskMonitorCore> for SharedState {
    ///     fn as_ref(&self) -> &TaskMonitorCore {
    ///         &self.0.monitor
    ///     }
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let state = SharedState(Arc::new(SharedStateInner {
    ///         monitor: TaskMonitorCore::new(),
    ///         other_state: SomeOtherSharedState,
    ///     }));
    ///
    ///     assert_eq!(state.0.monitor.cumulative().first_poll_count, 0);
    ///
    ///     TaskMonitorCore::instrument_with(async {}, state.clone()).await;
    ///     assert_eq!(state.0.monitor.cumulative().first_poll_count, 1);
    /// }
    /// ```
    pub fn instrument_with<F, M: AsRef<TaskMonitorCore> + Send + Sync + 'static>(
        task: F,
        monitor: M,
    ) -> Instrumented<F, M> {
        monitor
            .as_ref()
            .metrics
            .instrumented_count
            .fetch_add(1, SeqCst);

        let log = monitor
            .as_ref()
            .record_scheduling_log
            .then(|| Arc::new(SchedulingLog::default()));

        let state: State<M> = State {
            monitor,
            instrumented_at: Instant::now(),
            woke_at: AtomicU64::new(0),
            waker: AtomicWaker::new(),
            log,
        };

        let instrumented: Instrumented<F, M> = Instrumented {
            task,
            did_poll_once: false,
            idled_at: 0,
            state: Arc::new(state),
        };

        instrumented
    }

    /// Produces [`TaskMetrics`] for the tasks instrumented by this [`TaskMonitorCore`], collected since
    /// the construction of [`TaskMonitorCore`].
    ///
    /// ##### See also
    /// - [`TaskMonitorCore::intervals`]:
    ///   produces [`TaskMetrics`] for user-defined sampling intervals, instead of cumulatively
    ///
    /// See [`TaskMonitor::cumulative`] for examples.
    pub fn cumulative(&self) -> TaskMetrics {
        self.metrics.metrics()
    }

    /// Produces an unending iterator of metric sampling intervals.
    ///
    /// Each sampling interval is defined by the time elapsed between advancements of the iterator
    /// produced by [`TaskMonitorCore::intervals`]. The item type of this iterator is [`TaskMetrics`],
    /// which is a bundle of task metrics that describe *only* events occurring within that sampling
    /// interval.
    ///
    /// ##### Examples
    /// The below example demonstrates construction of [`TaskIntervals`] with [`TaskMonitorCore`].
    ///
    /// See [`TaskMonitor::intervals`] for more usage examples.
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// fn main() {
    ///     let metrics_monitor = Arc::new(tokio_metrics::TaskMonitorCore::new());
    ///
    ///     let mut _intervals = tokio_metrics::TaskMonitorCore::intervals(metrics_monitor);
    /// }
    /// ```
    pub fn intervals<Monitor: AsRef<TaskMonitorCore> + Send + Sync + 'static>(
        monitor: Monitor,
    ) -> TaskIntervals<Monitor> {
        let intervals: TaskIntervals<Monitor> = TaskIntervals {
            monitor,
            previous: None,
        };

        intervals
    }
}

impl AsRef<TaskMonitorCore> for TaskMonitorCore {
    fn as_ref(&self) -> &TaskMonitorCore {
        self
    }
}

impl TaskMonitorCore {
    const fn create(
        slow_poll_cut_off: Duration,
        long_delay_cut_off: Duration,
        record_scheduling_log: bool,
    ) -> TaskMonitorCore {
        TaskMonitorCore {
            record_scheduling_log,
            metrics: RawMetrics {
                slow_poll_threshold: slow_poll_cut_off,
                first_poll_count: AtomicU64::new(0),
                total_idled_count: AtomicU64::new(0),
                total_scheduled_count: AtomicU64::new(0),
                total_fast_poll_count: AtomicU64::new(0),
                total_slow_poll_count: AtomicU64::new(0),
                total_long_delay_count: AtomicU64::new(0),
                instrumented_count: AtomicU64::new(0),
                dropped_count: AtomicU64::new(0),
                total_first_poll_delay_ns: AtomicU64::new(0),
                total_scheduled_duration_ns: AtomicU64::new(0),
                local_max_idle_duration_ns: AtomicU64::new(0),
                global_max_idle_duration_ns: AtomicU64::new(0),
                total_idle_duration_ns: AtomicU64::new(0),
                total_fast_poll_duration_ns: AtomicU64::new(0),
                total_slow_poll_duration: AtomicU64::new(0),
                total_short_delay_duration_ns: AtomicU64::new(0),
                long_delay_threshold: long_delay_cut_off,
                total_short_delay_count: AtomicU64::new(0),
                total_long_delay_duration_ns: AtomicU64::new(0),
            },
        }
    }
}

impl RawMetrics {
    fn get_and_reset_local_max_idle_duration(&self) -> Duration {
        Duration::from_nanos(self.local_max_idle_duration_ns.swap(0, SeqCst))
    }

    fn metrics(&self) -> TaskMetrics {
        let total_fast_poll_count = self.total_fast_poll_count.load(SeqCst);
        let total_slow_poll_count = self.total_slow_poll_count.load(SeqCst);

        let total_fast_poll_duration =
            Duration::from_nanos(self.total_fast_poll_duration_ns.load(SeqCst));
        let total_slow_poll_duration =
            Duration::from_nanos(self.total_slow_poll_duration.load(SeqCst));

        let total_poll_count = total_fast_poll_count.saturating_add(total_slow_poll_count);
        let total_poll_duration = total_fast_poll_duration.saturating_add(total_slow_poll_duration);

        TaskMetrics {
            instrumented_count: self.instrumented_count.load(SeqCst),
            dropped_count: self.dropped_count.load(SeqCst),

            total_poll_count,
            total_poll_duration,
            first_poll_count: self.first_poll_count.load(SeqCst),
            total_idled_count: self.total_idled_count.load(SeqCst),
            total_scheduled_count: self.total_scheduled_count.load(SeqCst),
            total_fast_poll_count: self.total_fast_poll_count.load(SeqCst),
            total_slow_poll_count: self.total_slow_poll_count.load(SeqCst),
            total_short_delay_count: self.total_short_delay_count.load(SeqCst),
            total_long_delay_count: self.total_long_delay_count.load(SeqCst),
            total_first_poll_delay: Duration::from_nanos(
                self.total_first_poll_delay_ns.load(SeqCst),
            ),
            max_idle_duration: Duration::from_nanos(self.global_max_idle_duration_ns.load(SeqCst)),
            total_idle_duration: Duration::from_nanos(self.total_idle_duration_ns.load(SeqCst)),
            total_scheduled_duration: Duration::from_nanos(
                self.total_scheduled_duration_ns.load(SeqCst),
            ),
            total_fast_poll_duration: Duration::from_nanos(
                self.total_fast_poll_duration_ns.load(SeqCst),
            ),
            total_slow_poll_duration: Duration::from_nanos(
                self.total_slow_poll_duration.load(SeqCst),
            ),
            total_short_delay_duration: Duration::from_nanos(
                self.total_short_delay_duration_ns.load(SeqCst),
            ),
            total_long_delay_duration: Duration::from_nanos(
                self.total_long_delay_duration_ns.load(SeqCst),
            ),
        }
    }
}

impl Default for TaskMonitor {
    fn default() -> TaskMonitor {
        TaskMonitor::new()
    }
}

impl Default for TaskMonitorCore {
    fn default() -> TaskMonitorCore {
        TaskMonitorCore::new()
    }
}

derived_metrics!(
    [TaskMetrics] {
        stable {
            /// The mean duration elapsed between the instant tasks are instrumented, and the instant they
            /// are first polled.
            ///
            /// ##### Definition
            /// This metric is derived from [`total_first_poll_delay`][TaskMetrics::total_first_poll_delay]
            /// ÷ [`first_poll_count`][TaskMetrics::first_poll_count].
            ///
            /// ##### Interpretation
            /// If this metric increases, it means that, on average, tasks spent longer waiting to be
            /// initially polled.
            ///
            /// ##### See also
            /// - **[`mean_scheduled_duration`][TaskMetrics::mean_scheduled_duration]**
            ///   The mean duration that tasks spent waiting to be executed after awakening.
            ///
            /// ##### Examples
            /// In the below example, no tasks are instrumented or polled within the first sampling
            /// interval; in the second sampling interval, 500ms elapse between the instrumentation of a
            /// task and its first poll; in the third sampling interval, a mean of 750ms elapse between the
            /// instrumentation and first poll of two tasks:
            /// ```
            /// use std::time::Duration;
            ///
            /// #[tokio::main]
            /// async fn main() {
            ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
            ///     let mut interval = metrics_monitor.intervals();
            ///     let mut next_interval = || interval.next().unwrap();
            ///
            ///     // no tasks have yet been created, instrumented, or polled
            ///     assert_eq!(next_interval().mean_first_poll_delay(), Duration::ZERO);
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
            ///     // the `mean_first_poll_delay` will be some duration greater-than-or-equal-to the
            ///     // pause time of 500ms, and less-than-or-equal-to the total runtime of `task_a`
            ///     let mean_first_poll_delay = next_interval().mean_first_poll_delay();
            ///     assert!(mean_first_poll_delay >= task_a_pause_time);
            ///     assert!(mean_first_poll_delay <= task_a_total_time);
            ///
            ///     // construct and await a task that pauses for 500ms between instrumentation and first poll
            ///     let task_b_pause_time = Duration::from_millis(500);
            ///     let task_b_total_time = instrument_pause_await(&metrics_monitor, task_b_pause_time).await;
            ///
            ///     // construct and await a task that pauses for 1000ms between instrumentation and first poll
            ///     let task_c_pause_time = Duration::from_millis(1000);
            ///     let task_c_total_time = instrument_pause_await(&metrics_monitor, task_c_pause_time).await;
            ///
            ///     // the `mean_first_poll_delay` will be some duration greater-than-or-equal-to the
            ///     // average pause time of 500ms, and less-than-or-equal-to the combined total runtime of
            ///     // `task_b` and `task_c`
            ///     let mean_first_poll_delay = next_interval().mean_first_poll_delay();
            ///     assert!(mean_first_poll_delay >= (task_b_pause_time + task_c_pause_time) / 2);
            ///     assert!(mean_first_poll_delay <= (task_b_total_time + task_c_total_time) / 2);
            /// }
            /// ```
            pub fn mean_first_poll_delay(&self) -> Duration {
                mean(self.total_first_poll_delay, self.first_poll_count)
            }

            /// The mean duration of idles.
            ///
            /// ##### Definition
            /// This metric is derived from [`total_idle_duration`][TaskMetrics::total_idle_duration] ÷
            /// [`total_idled_count`][TaskMetrics::total_idled_count].
            ///
            /// ##### Interpretation
            /// The idle state is the duration spanning the instant a task completes a poll, and the instant
            /// that it is next awoken. Tasks inhabit this state when they are waiting for task-external
            /// events to complete (e.g., an asynchronous sleep, a network request, file I/O, etc.). If this
            /// metric increases, it means that tasks, in aggregate, spent more time waiting for
            /// task-external events to complete.
            ///
            /// ##### Examples
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
            ///     assert!(monitor.cumulative().mean_idle_duration() >= one_sec);
            /// }
            /// ```
            pub fn mean_idle_duration(&self) -> Duration {
                mean(self.total_idle_duration, self.total_idled_count)
            }

            /// The mean duration that tasks spent waiting to be executed after awakening.
            ///
            /// ##### Definition
            /// This metric is derived from
            /// [`total_scheduled_duration`][TaskMetrics::total_scheduled_duration] ÷
            /// [`total_scheduled_count`][`TaskMetrics::total_scheduled_count`].
            ///
            /// ##### Interpretation
            /// If this metric increases, it means that, on average, tasks spent longer in the runtime's
            /// queues before being polled.
            ///
            /// ##### See also
            /// - **[`mean_first_poll_delay`][TaskMetrics::mean_first_poll_delay]**
            ///   The mean duration elapsed between the instant tasks are instrumented, and the instant they
            ///   are first polled.
            ///
            /// ##### Examples
            /// ```
            /// use tokio::time::Duration;
            ///
            /// #[tokio::main(flavor = "current_thread")]
            /// async fn main() {
            ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
            ///     let mut interval = metrics_monitor.intervals();
            ///     let mut next_interval = || interval.next().unwrap();
            ///
            ///     // construct and instrument and spawn a task that yields endlessly
            ///     tokio::spawn(metrics_monitor.instrument(async {
            ///         loop { tokio::task::yield_now().await }
            ///     }));
            ///
            ///     tokio::task::yield_now().await;
            ///
            ///     // block the executor for 1 second
            ///     std::thread::sleep(Duration::from_millis(1000));
            ///
            ///     // get the task to run twice
            ///     // the first will have a 1 sec scheduling delay, the second will have almost none
            ///     tokio::task::yield_now().await;
            ///     tokio::task::yield_now().await;
            ///
            ///     // `endless_task` will have spent approximately one second waiting
            ///     let mean_scheduled_duration = next_interval().mean_scheduled_duration();
            ///     assert!(mean_scheduled_duration >= Duration::from_millis(500), "{}", mean_scheduled_duration.as_secs_f64());
            ///     assert!(mean_scheduled_duration <= Duration::from_millis(600), "{}", mean_scheduled_duration.as_secs_f64());
            /// }
            /// ```
            pub fn mean_scheduled_duration(&self) -> Duration {
                mean(self.total_scheduled_duration, self.total_scheduled_count)
            }

            /// The mean duration of polls.
            ///
            /// ##### Definition
            /// This metric is derived from [`total_poll_duration`][TaskMetrics::total_poll_duration] ÷
            /// [`total_poll_count`][TaskMetrics::total_poll_count].
            ///
            /// ##### Interpretation
            /// If this metric increases, it means that, on average, individual polls are tending to take
            /// longer. However, this does not necessarily imply increased task latency: An increase in poll
            /// durations could be offset by fewer polls.
            ///
            /// ##### See also
            /// - **[`slow_poll_ratio`][TaskMetrics::slow_poll_ratio]**
            ///   The ratio between the number polls categorized as slow and fast.
            /// - **[`mean_slow_poll_duration`][TaskMetrics::mean_slow_poll_duration]**
            ///   The mean duration of slow polls.
            ///
            /// ##### Examples
            /// ```
            /// use std::time::Duration;
            ///
            /// #[tokio::main(flavor = "current_thread", start_paused = true)]
            /// async fn main() {
            ///     let monitor = tokio_metrics::TaskMonitor::new();
            ///     let mut interval = monitor.intervals();
            ///     let mut next_interval = move || interval.next().unwrap();
            ///
            ///     assert_eq!(next_interval().mean_poll_duration(), Duration::ZERO);
            ///
            ///     monitor.instrument(async {
            ///         tokio::time::advance(Duration::from_secs(1)).await; // poll 1 (1s)
            ///         tokio::time::advance(Duration::from_secs(1)).await; // poll 2 (1s)
            ///         ()                                                  // poll 3 (0s)
            ///     }).await;
            ///
            ///     assert_eq!(next_interval().mean_poll_duration(), Duration::from_secs(2) / 3);
            /// }
            /// ```
            pub fn mean_poll_duration(&self) -> Duration {
                mean(self.total_poll_duration, self.total_poll_count)
            }

            /// The ratio between the number polls categorized as slow and fast.
            ///
            /// ##### Definition
            /// This metric is derived from [`total_slow_poll_count`][TaskMetrics::total_slow_poll_count] ÷
            /// [`total_poll_count`][TaskMetrics::total_poll_count].
            ///
            /// ##### Interpretation
            /// If this metric increases, it means that a greater proportion of polls took excessively long
            /// before yielding to the scheduler. This does not necessarily imply increased task latency:
            /// An increase in the proportion of slow polls could be offset by fewer or faster polls.
            /// However, as a rule, *should* yield to the scheduler frequently.
            ///
            /// ##### See also
            /// - **[`mean_poll_duration`][TaskMetrics::mean_poll_duration]**
            ///   The mean duration of polls.
            /// - **[`mean_slow_poll_duration`][TaskMetrics::mean_slow_poll_duration]**
            ///   The mean duration of slow polls.
            ///
            /// ##### Examples
            /// Changes in this metric may be observed by varying the ratio of slow and slow fast within
            /// sampling intervals; for instance:
            /// ```
            /// use std::future::Future;
            /// use std::time::Duration;
            ///
            /// #[tokio::main]
            /// async fn main() {
            ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
            ///     let mut interval = metrics_monitor.intervals();
            ///     let mut next_interval = || interval.next().unwrap();
            ///
            ///     // no tasks have been constructed, instrumented, or polled
            ///     let interval = next_interval();
            ///     assert_eq!(interval.total_fast_poll_count, 0);
            ///     assert_eq!(interval.total_slow_poll_count, 0);
            ///     assert!(interval.slow_poll_ratio().is_nan());
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
            ///     assert_eq!(interval.slow_poll_ratio(), ratio(2., 3.));
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
            ///     assert_eq!(interval.slow_poll_ratio(), ratio(3., 2.));
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
            pub fn slow_poll_ratio(&self) -> f64 {
                self.total_slow_poll_count as f64 / self.total_poll_count as f64
            }

            /// The ratio of tasks exceeding [`long_delay_threshold`][TaskMonitor::long_delay_threshold].
            ///
            /// ##### Definition
            /// This metric is derived from [`total_long_delay_count`][TaskMetrics::total_long_delay_count] ÷
            /// [`total_scheduled_count`][TaskMetrics::total_scheduled_count].
            pub fn long_delay_ratio(&self) -> f64 {
                self.total_long_delay_count as f64 / self.total_scheduled_count as f64
            }

            /// The mean duration of fast polls.
            ///
            /// ##### Definition
            /// This metric is derived from
            /// [`total_fast_poll_duration`][TaskMetrics::total_fast_poll_duration] ÷
            /// [`total_fast_poll_count`][TaskMetrics::total_fast_poll_count].
            ///
            /// ##### Examples
            /// In the below example, no tasks are polled in the first sampling interval; three fast polls
            /// consume a mean of
            /// ⅜ × [`DEFAULT_SLOW_POLL_THRESHOLD`][TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD] time in the
            /// second sampling interval; and two fast polls consume a total of
            /// ½ × [`DEFAULT_SLOW_POLL_THRESHOLD`][TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD] time in the
            /// third sampling interval:
            /// ```
            /// use std::future::Future;
            /// use std::time::Duration;
            ///
            /// #[tokio::main]
            /// async fn main() {
            ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
            ///     let mut interval = metrics_monitor.intervals();
            ///     let mut next_interval = || interval.next().unwrap();
            ///
            ///     // no tasks have been constructed, instrumented, or polled
            ///     assert_eq!(next_interval().mean_fast_poll_duration(), Duration::ZERO);
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
            ///     // `mean_fast_poll_duration` ≈ the mean of `fast_1` and `fast_2`
            ///     let mean_fast_poll_duration = next_interval().mean_fast_poll_duration();
            ///     assert!(mean_fast_poll_duration >= (fast_1 + fast_2) / 2);
            ///     assert!(mean_fast_poll_duration <= total_time / 2);
            ///
            ///     // this task completes in three fast polls
            ///     let total_time = time(metrics_monitor.instrument(async {
            ///         spin_for(fast_1).await; // fast poll 1
            ///         spin_for(fast_2).await; // fast poll 2
            ///         spin_for(fast_3)        // fast poll 3
            ///     })).await;
            ///
            ///     // `mean_fast_poll_duration` ≈ the mean of `fast_1`, `fast_2`, `fast_3`
            ///     let mean_fast_poll_duration = next_interval().mean_fast_poll_duration();
            ///     assert!(mean_fast_poll_duration >= (fast_1 + fast_2 + fast_3) / 3);
            ///     assert!(mean_fast_poll_duration <= total_time / 3);
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
            pub fn mean_fast_poll_duration(&self) -> Duration {
                mean(self.total_fast_poll_duration, self.total_fast_poll_count)
            }

            /// The mean duration of slow polls.
            ///
            /// ##### Definition
            /// This metric is derived from
            /// [`total_slow_poll_duration`][TaskMetrics::total_slow_poll_duration] ÷
            /// [`total_slow_poll_count`][TaskMetrics::total_slow_poll_count].
            ///
            /// ##### Interpretation
            /// If this metric increases, it means that a greater proportion of polls took excessively long
            /// before yielding to the scheduler. This does not necessarily imply increased task latency:
            /// An increase in the proportion of slow polls could be offset by fewer or faster polls.
            ///
            /// ##### See also
            /// - **[`mean_poll_duration`][TaskMetrics::mean_poll_duration]**
            ///   The mean duration of polls.
            /// - **[`slow_poll_ratio`][TaskMetrics::slow_poll_ratio]**
            ///   The ratio between the number polls categorized as slow and fast.
            ///
            /// ##### Interpretation
            /// If this metric increases, it means that, on average, slow polls got even slower. This does
            /// necessarily imply increased task latency: An increase in average slow poll duration could be
            /// offset by fewer or faster polls. However, as a rule, *should* yield to the scheduler
            /// frequently.
            ///
            /// ##### Examples
            /// In the below example, no tasks are polled in the first sampling interval; three slow polls
            /// consume a mean of
            /// 1.5 × [`DEFAULT_SLOW_POLL_THRESHOLD`][TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD] time in the
            /// second sampling interval; and two slow polls consume a total of
            /// 2 × [`DEFAULT_SLOW_POLL_THRESHOLD`][TaskMonitor::DEFAULT_SLOW_POLL_THRESHOLD] time in the
            /// third sampling interval:
            /// ```
            /// use std::future::Future;
            /// use std::time::Duration;
            ///
            /// #[tokio::main]
            /// async fn main() {
            ///     let metrics_monitor = tokio_metrics::TaskMonitor::new();
            ///     let mut interval = metrics_monitor.intervals();
            ///     let mut next_interval = || interval.next().unwrap();
            ///
            ///     // no tasks have been constructed, instrumented, or polled
            ///     assert_eq!(next_interval().mean_slow_poll_duration(), Duration::ZERO);
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
            ///     // `mean_slow_poll_duration` ≈ the mean of `slow_1` and `slow_2`
            ///     let mean_slow_poll_duration = next_interval().mean_slow_poll_duration();
            ///     assert!(mean_slow_poll_duration >= (slow_1 + slow_2) / 2);
            ///     assert!(mean_slow_poll_duration <= total_time / 2);
            ///
            ///     // this task completes in three slow polls
            ///     let total_time = time(metrics_monitor.instrument(async {
            ///         spin_for(slow_1).await; // slow poll 1
            ///         spin_for(slow_2).await; // slow poll 2
            ///         spin_for(slow_3)        // slow poll 3
            ///     })).await;
            ///
            ///     // `mean_slow_poll_duration` ≈ the mean of `slow_1`, `slow_2`, `slow_3`
            ///     let mean_slow_poll_duration = next_interval().mean_slow_poll_duration();
            ///     assert!(mean_slow_poll_duration >= (slow_1 + slow_2 + slow_3) / 3);
            ///     assert!(mean_slow_poll_duration <= total_time / 3);
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
            pub fn mean_slow_poll_duration(&self) -> Duration {
                mean(self.total_slow_poll_duration, self.total_slow_poll_count)
            }

            /// The average time taken for a task with a short scheduling delay to be executed after being
            /// scheduled.
            ///
            /// ##### Definition
            /// This metric is derived from
            /// [`total_short_delay_duration`][TaskMetrics::total_short_delay_duration] ÷
            /// [`total_short_delay_count`][TaskMetrics::total_short_delay_count].
            pub fn mean_short_delay_duration(&self) -> Duration {
                mean(
                    self.total_short_delay_duration,
                    self.total_short_delay_count,
                )
            }

            /// The average scheduling delay for a task which takes a long time to start executing after
            /// being scheduled.
            ///
            /// ##### Definition
            /// This metric is derived from
            /// [`total_long_delay_duration`][TaskMetrics::total_long_delay_duration] ÷
            /// [`total_long_delay_count`][TaskMetrics::total_long_delay_count].
            pub fn mean_long_delay_duration(&self) -> Duration {
                mean(self.total_long_delay_duration, self.total_long_delay_count)
            }
        }
        unstable {}
    }
);

impl<T: Future, M: AsRef<TaskMonitorCore> + Send + Sync + 'static> Future for Instrumented<T, M> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        instrument_poll(cx, self, Future::poll)
    }
}

impl<T: Stream> Stream for Instrumented<T> {
    type Item = T::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        instrument_poll(cx, self, Stream::poll_next)
    }
}

fn instrument_poll<T, M: AsRef<TaskMonitorCore> + Send + Sync + 'static, Out>(
    cx: &mut Context<'_>,
    instrumented: Pin<&mut Instrumented<T, M>>,
    poll_fn: impl FnOnce(Pin<&mut T>, &mut Context<'_>) -> Poll<Out>,
) -> Poll<Out> {
    let poll_start = Instant::now();
    let this = instrumented.project();
    let idled_at = this.idled_at;
    let state = this.state;
    let instrumented_at = state.instrumented_at;
    let metrics = &state.monitor.as_ref().metrics;
    /* accounting for time-to-first-poll and tasks-count */
    // is this the first time this task has been polled?
    if !*this.did_poll_once {
        // if so, we need to do three things:
        /* 1. note that this task *has* been polled */
        *this.did_poll_once = true;

        /* 2. account for the time-to-first-poll of this task */
        // if the time-to-first-poll of this task exceeds `u64::MAX` ns,
        // round down to `u64::MAX` nanoseconds
        let elapsed = poll_start
            .saturating_duration_since(instrumented_at)
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX);
        // add this duration to `time_to_first_poll_ns_total`
        metrics.total_first_poll_delay_ns.fetch_add(elapsed, SeqCst);

        /* 3. increment the count of tasks that have been polled at least once */
        metrics.first_poll_count.fetch_add(1, SeqCst);
    }
    /* accounting for time-idled and time-scheduled */
    // 1. note (and reset) the instant this task was last awoke
    let woke_at = state.woke_at.swap(0, SeqCst);
    // The state of a future is *idling* in the interim between the instant
    // it completes a `poll`, and the instant it is next awoken.
    if *idled_at < woke_at {
        // increment the counter of how many idles occurred
        metrics.total_idled_count.fetch_add(1, SeqCst);

        // compute the duration of the idle
        let idle_ns = woke_at.saturating_sub(*idled_at);

        // update the max time tasks spent idling, both locally and
        // globally.
        metrics
            .local_max_idle_duration_ns
            .fetch_max(idle_ns, SeqCst);
        metrics
            .global_max_idle_duration_ns
            .fetch_max(idle_ns, SeqCst);
        // adjust the total elapsed time monitored tasks spent idling
        metrics.total_idle_duration_ns.fetch_add(idle_ns, SeqCst);
    }
    // if this task spent any time in the scheduled state after instrumentation,
    // and after first poll, `woke_at` will be greater than 0.
    if woke_at > 0 {
        // increment the counter of how many schedules occurred
        metrics.total_scheduled_count.fetch_add(1, SeqCst);

        // recall that the `woke_at` field is internally represented as
        // nanoseconds-since-instrumentation. here, for accounting purposes,
        // we need to instead represent it as a proper `Instant`.
        let woke_instant = instrumented_at
            .checked_add(Duration::from_nanos(woke_at))
            .unwrap_or(poll_start);

        // the duration this task spent scheduled is time time elapsed between
        // when this task was awoke, and when it was polled.
        let scheduled_ns = poll_start
            .saturating_duration_since(woke_instant)
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX);

        let scheduled = Duration::from_nanos(scheduled_ns);

        let (count_bucket, duration_bucket) = // was the scheduling delay long or short?
            if scheduled >= metrics.long_delay_threshold {
                (&metrics.total_long_delay_count, &metrics.total_long_delay_duration_ns)
            } else {
                (&metrics.total_short_delay_count, &metrics.total_short_delay_duration_ns)
            };
        // update the appropriate bucket
        count_bucket.fetch_add(1, SeqCst);
        duration_bucket.fetch_add(scheduled_ns, SeqCst);

        // add `scheduled_ns` to the Monitor's total
        metrics
            .total_scheduled_duration_ns
            .fetch_add(scheduled_ns, SeqCst);

        // mirror the same scheduling accounting into this task's own log so that
        // futures running within the task can attribute scheduling delay to
        // their own lifetime (see `FutureMonitor`). Only present when the
        // monitor opted in, so the common case skips this entirely.
        if let Some(log) = &state.log {
            log.scheduled_count.fetch_add(1, SeqCst);
            log.scheduled_duration_ns.fetch_add(scheduled_ns, SeqCst);
            if scheduled >= metrics.long_delay_threshold {
                log.long_delay_count.fetch_add(1, SeqCst);
            }
        }
    }
    // Register the waker
    state.waker.register(cx.waker());
    // Get the instrumented waker
    let waker_ref = futures_util::task::waker_ref(state);
    let mut cx = Context::from_waker(&waker_ref);
    // Poll the task, publishing this task's scheduling log as a task-local for
    // the duration of the poll so nested futures can read it.
    let inner_poll_start = Instant::now();
    // Only publish the task-local when the monitor opted in. When it didn't
    // (the common case) this is a single predictable branch and the poll path is
    // byte-for-byte the original — no guard, thread-local, or `Arc` clone.
    let ret = if let Some(log) = &state.log {
        let _sched_guard = SchedulingLogGuard::enter(log.clone());
        poll_fn(this.task, &mut cx)
    } else {
        poll_fn(this.task, &mut cx)
    };
    let inner_poll_end = Instant::now();
    /* idle time starts now */
    *idled_at = inner_poll_end
        .saturating_duration_since(instrumented_at)
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX);
    /* accounting for poll time */
    let inner_poll_duration = inner_poll_end.saturating_duration_since(inner_poll_start);
    let inner_poll_ns: u64 = inner_poll_duration
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX);
    let (count_bucket, duration_bucket) = // was this a slow or fast poll?
            if inner_poll_duration >= metrics.slow_poll_threshold {
                (&metrics.total_slow_poll_count, &metrics.total_slow_poll_duration)
            } else {
                (&metrics.total_fast_poll_count, &metrics.total_fast_poll_duration_ns)
            };
    // update the appropriate bucket
    count_bucket.fetch_add(1, SeqCst);
    duration_bucket.fetch_add(inner_poll_ns, SeqCst);
    ret
}

impl<M> State<M> {
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

impl<M: Send + Sync> ArcWake for State<M> {
    fn wake_by_ref(arc_self: &Arc<State<M>>) {
        arc_self.on_wake();
        arc_self.waker.wake();
    }

    fn wake(self: Arc<State<M>>) {
        self.on_wake();
        self.waker.wake();
    }
}

/// Key metrics of a single instrumented future running within a larger task, as
/// captured by a [`FutureMonitor`].
///
/// Unlike [`TaskMetrics`], which aggregates across every future instrumented by
/// a [`TaskMonitor`], these metrics describe just one future. Idle, poll, and
/// first-poll metrics are measured locally from that future's own polls, so they
/// remain accurate even when the surrounding task interleaves other work.
/// Scheduling metrics are sampled from the root task's [`TaskScheduling`] log
/// over the future's lifetime (see [`FutureMonitor`]).
#[non_exhaustive]
#[cfg_attr(
    feature = "metrique-integration",
    metrique::unit_of_work::metrics(subfield)
)]
#[derive(Debug, Clone, Default)]
pub struct FutureMetrics {
    /// The number of times the future was polled.
    pub poll_count: u64,
    /// The total time spent polling the future.
    pub total_poll_duration: Duration,
    /// The number of polls that exceeded the monitor's slow-poll threshold.
    pub slow_poll_count: u64,
    /// The number of times the future idled, waiting to be awoken.
    pub idle_count: u64,
    /// The total time the future spent idle, waiting on external events.
    pub total_idle_duration: Duration,
    /// The longest single idle the future experienced.
    pub max_idle_duration: Duration,
    /// The delay between the future being instrumented and its first poll.
    pub first_poll_delay: Duration,
    /// The wall-clock time from the future's first poll to its completion.
    pub total_duration: Duration,
    /// The number of times the underlying task was scheduled while the future was alive.
    pub scheduled_count: u64,
    /// The total scheduling delay the underlying task incurred while the future was alive.
    pub total_scheduled_duration: Duration,
    /// The number of those scheduling delays that crossed the long-delay threshold.
    pub long_delay_count: u64,
}

/// Per-future capture state shared between a [`FutureMonitor`] and the
/// [`MonitoredFuture`] future it produces.
#[derive(Debug, Default)]
struct SpanState {
    started: AtomicBool,
    start_scheduled_count: AtomicU64,
    start_scheduled_duration_ns: AtomicU64,
    start_long_delay_count: AtomicU64,
    end_scheduled_count: AtomicU64,
    end_scheduled_duration_ns: AtomicU64,
    end_long_delay_count: AtomicU64,
    first_poll: OnceLock<Instant>,
    total_duration_ns: AtomicU64,
}

fn duration_to_nanos(d: Duration) -> u64 {
    d.as_nanos().try_into().unwrap_or(u64::MAX)
}

impl SpanState {
    /// Records the root scheduling snapshot for a poll. Called at the top of
    /// every poll of the [`MonitoredFuture`], while the root task's
    /// scheduling log is in scope.
    fn record_poll_start(&self, now: TaskScheduling) {
        if !self.started.swap(true, SeqCst) {
            self.start_scheduled_count
                .store(now.scheduled_count, SeqCst);
            self.start_scheduled_duration_ns
                .store(duration_to_nanos(now.total_scheduled_duration), SeqCst);
            self.start_long_delay_count
                .store(now.long_delay_count, SeqCst);
            let _ = self.first_poll.set(Instant::now());
        }
        self.end_scheduled_count.store(now.scheduled_count, SeqCst);
        self.end_scheduled_duration_ns
            .store(duration_to_nanos(now.total_scheduled_duration), SeqCst);
        self.end_long_delay_count
            .store(now.long_delay_count, SeqCst);
    }

    /// Records total elapsed time *after* a poll completes, so the execution
    /// time of the poll just finished — including the final one — is counted.
    fn record_poll_end(&self) {
        if let Some(first_poll) = self.first_poll.get() {
            self.total_duration_ns
                .store(duration_to_nanos(first_poll.elapsed()), SeqCst);
        }
    }

    fn scheduling_delta(&self) -> TaskScheduling {
        let end = TaskScheduling {
            scheduled_count: self.end_scheduled_count.load(SeqCst),
            total_scheduled_duration: Duration::from_nanos(
                self.end_scheduled_duration_ns.load(SeqCst),
            ),
            long_delay_count: self.end_long_delay_count.load(SeqCst),
        };
        let start = TaskScheduling {
            scheduled_count: self.start_scheduled_count.load(SeqCst),
            total_scheduled_duration: Duration::from_nanos(
                self.start_scheduled_duration_ns.load(SeqCst),
            ),
            long_delay_count: self.start_long_delay_count.load(SeqCst),
        };
        end.since(start)
    }

    fn total_duration(&self) -> Duration {
        Duration::from_nanos(self.total_duration_ns.load(SeqCst))
    }
}

fn build_future_metrics(monitor: &TaskMonitor, span: &SpanState) -> FutureMetrics {
    let local = monitor.cumulative();
    let scheduling = span.scheduling_delta();
    FutureMetrics {
        poll_count: local.total_poll_count,
        total_poll_duration: local.total_poll_duration,
        slow_poll_count: local.total_slow_poll_count,
        idle_count: local.total_idled_count,
        total_idle_duration: local.total_idle_duration,
        max_idle_duration: local.max_idle_duration,
        first_poll_delay: local.total_first_poll_delay,
        total_duration: span.total_duration(),
        scheduled_count: scheduling.scheduled_count,
        total_scheduled_duration: scheduling.total_scheduled_duration,
        long_delay_count: scheduling.long_delay_count,
    }
}

pin_project! {
    /// The future returned by [`FutureMonitor::instrument`].
    ///
    /// Wraps the future, capturing its per-poll metrics locally and
    /// sampling the surrounding task's scheduling delay at each poll. Resolves to
    /// the wrapped future's output paired with the captured [`FutureMetrics`].
    pub struct MonitoredFuture<F> {
        #[pin]
        inner: Instrumented<F, TaskMonitor>,
        monitor: TaskMonitor,
        span: Arc<SpanState>,
    }
}

impl<F> std::fmt::Debug for MonitoredFuture<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitoredFuture")
            .finish_non_exhaustive()
    }
}

impl<F: Future> Future for MonitoredFuture<F> {
    type Output = (F::Output, FutureMetrics);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        // Sample the root task's scheduling log *before* delegating to the inner
        // `Instrumented`, while the root task's log is the one in scope (the
        // inner `Instrumented` shadows it with the future's own log during its
        // poll, which we deliberately ignore).
        this.span
            .record_poll_start(TaskScheduling::try_current().unwrap_or_default());
        let ret = this.inner.poll(cx);
        // Stamp total elapsed *after* the poll so the execution time of the poll
        // just finished — including the final one that returns `Ready` — is
        // captured in `total_duration`.
        this.span.record_poll_end();
        match ret {
            Poll::Ready(output) => {
                let metrics = build_future_metrics(this.monitor, this.span);
                Poll::Ready((output, metrics))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Monitors the metrics of a single future running within a larger,
/// already-[instrumented][TaskMonitor::instrument] task.
///
/// [`TaskMonitor`] aggregates metrics across all the futures it instruments;
/// `FutureMonitor` instead captures the metrics of *one* future so they can be
/// attached to that future's own record. Idle, poll, and first-poll metrics are
/// measured locally from the future. Scheduling delay — which only the
/// root future the runtime schedules can observe — is read from the surrounding
/// task's [`TaskScheduling`] log over the future's lifetime, so for scheduling
/// metrics to be populated the surrounding task must itself be instrumented with
/// [`TaskMonitor::instrument`].
///
/// `FutureMonitor` instruments exactly one future: [`instrument`] consumes the
/// monitor, so it cannot be reused, and the returned future resolves to the
/// wrapped output together with the captured [`FutureMetrics`].
///
/// ##### Examples
/// ```
/// use tokio_metrics::{FutureMonitor, TaskMonitor};
///
/// #[tokio::main]
/// async fn main() {
///     // the larger task is instrumented once; `publish_scheduling_delay`
///     // enables per-future scheduling-delay capture
///     let mut builder = TaskMonitor::builder();
///     builder.publish_scheduling_delay();
///     let task_monitor = builder.build();
///     task_monitor.instrument(async {
///         // each unit of work within the task is measured on its own
///         let (_output, metrics) = FutureMonitor::new()
///             .instrument(async {
///                 tokio::task::yield_now().await;
///             })
///             .await;
///
///         assert!(metrics.poll_count >= 1);
///     }).await;
/// }
/// ```
///
/// [`instrument`]: FutureMonitor::instrument
// Deliberately not `Clone`: cloning would share the underlying span and let two
// instrumented futures be measured as one, which is exactly the reuse `instrument`
// (by consuming `self`) is meant to prevent.
#[derive(Debug, Default)]
pub struct FutureMonitor {
    monitor: TaskMonitor,
    span: Arc<SpanState>,
}

impl FutureMonitor {
    /// Constructs a new `FutureMonitor` for a single future.
    pub fn new() -> FutureMonitor {
        FutureMonitor::default()
    }

    /// Constructs a new `FutureMonitor` whose local poll metrics use a custom
    /// slow-poll threshold (see [`TaskMonitor::with_slow_poll_threshold`]).
    pub fn with_slow_poll_threshold(slow_poll_threshold: Duration) -> FutureMonitor {
        FutureMonitor {
            monitor: TaskMonitor::with_slow_poll_threshold(slow_poll_threshold),
            span: Arc::new(SpanState::default()),
        }
    }

    /// Instruments the future, consuming the monitor.
    ///
    /// Taking `self` by value means a `FutureMonitor` can only instrument one
    /// future. Await the returned [`MonitoredFuture`] to get the future's
    /// output alongside its [`FutureMetrics`].
    pub fn instrument<F>(self, task: F) -> MonitoredFuture<F> {
        MonitoredFuture {
            inner: self.monitor.instrument(task),
            monitor: self.monitor,
            span: self.span,
        }
    }
}

/// Iterator returned by [`TaskMonitor::intervals`].
///
/// See that method's documentation for more details.
#[derive(Debug)]
pub struct TaskIntervals<M: AsRef<TaskMonitorCore> + Send + Sync + 'static = TaskMonitor> {
    monitor: M,
    previous: Option<TaskMetrics>,
}

impl<M: AsRef<TaskMonitorCore> + Send + Sync + 'static> TaskIntervals<M> {
    fn probe(&mut self) -> TaskMetrics {
        let latest = self.monitor.as_ref().metrics.metrics();
        let local_max_idle_duration = self
            .monitor
            .as_ref()
            .metrics
            .get_and_reset_local_max_idle_duration();

        let next = if let Some(previous) = self.previous {
            TaskMetrics {
                instrumented_count: latest
                    .instrumented_count
                    .wrapping_sub(previous.instrumented_count),
                dropped_count: latest.dropped_count.wrapping_sub(previous.dropped_count),
                total_poll_count: latest
                    .total_poll_count
                    .wrapping_sub(previous.total_poll_count),
                total_poll_duration: sub(latest.total_poll_duration, previous.total_poll_duration),
                first_poll_count: latest
                    .first_poll_count
                    .wrapping_sub(previous.first_poll_count),
                total_idled_count: latest
                    .total_idled_count
                    .wrapping_sub(previous.total_idled_count),
                total_scheduled_count: latest
                    .total_scheduled_count
                    .wrapping_sub(previous.total_scheduled_count),
                total_fast_poll_count: latest
                    .total_fast_poll_count
                    .wrapping_sub(previous.total_fast_poll_count),
                total_short_delay_count: latest
                    .total_short_delay_count
                    .wrapping_sub(previous.total_short_delay_count),
                total_slow_poll_count: latest
                    .total_slow_poll_count
                    .wrapping_sub(previous.total_slow_poll_count),
                total_long_delay_count: latest
                    .total_long_delay_count
                    .wrapping_sub(previous.total_long_delay_count),
                total_first_poll_delay: sub(
                    latest.total_first_poll_delay,
                    previous.total_first_poll_delay,
                ),
                max_idle_duration: local_max_idle_duration,
                total_idle_duration: sub(latest.total_idle_duration, previous.total_idle_duration),
                total_scheduled_duration: sub(
                    latest.total_scheduled_duration,
                    previous.total_scheduled_duration,
                ),
                total_fast_poll_duration: sub(
                    latest.total_fast_poll_duration,
                    previous.total_fast_poll_duration,
                ),
                total_short_delay_duration: sub(
                    latest.total_short_delay_duration,
                    previous.total_short_delay_duration,
                ),
                total_slow_poll_duration: sub(
                    latest.total_slow_poll_duration,
                    previous.total_slow_poll_duration,
                ),
                total_long_delay_duration: sub(
                    latest.total_long_delay_duration,
                    previous.total_long_delay_duration,
                ),
            }
        } else {
            latest
        };

        self.previous = Some(latest);

        next
    }
}

impl<M: AsRef<TaskMonitorCore> + Send + Sync + 'static> Iterator for TaskIntervals<M> {
    type Item = TaskMetrics;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.probe())
    }
}

#[inline(always)]
fn to_nanos(d: Duration) -> u64 {
    debug_assert!(d <= Duration::from_nanos(u64::MAX));
    d.as_secs()
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

#[cfg(test)]
mod inference_tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    // Type alias — M defaults to TaskMonitor
    type _BoxedInstrumented = Instrumented<Pin<Box<dyn Future<Output = ()>>>>;

    // Struct field — M defaults to TaskMonitor
    struct _Wrapper {
        _fut: Instrumented<Pin<Box<dyn Future<Output = ()>>>>,
    }

    // Partial type annotation — M defaults to TaskMonitor
    async fn _partial_annotation(monitor: &TaskMonitor) {
        let fut: Instrumented<_> = monitor.instrument(async { 42 });
        fut.await;
    }

    // Common path — fully inferred from instrument()'s return type
    async fn _common_usage(monitor: &TaskMonitor) {
        monitor.instrument(async { 42 }).await;
    }

    // Storing without annotation — both T and M inferred
    async fn _store_without_annotation(monitor: &TaskMonitor) {
        let fut = monitor.instrument(async { 42 });
        fut.await;
    }

    // Function boundary — M defaults to TaskMonitor in the signature
    async fn _function_boundary(fut: Instrumented<impl Future<Output = i32>>) -> i32 {
        fut.await
    }

    // Return position — M defaults to TaskMonitor
    fn _return_position(monitor: &TaskMonitor) -> Instrumented<impl Future<Output = i32> + '_> {
        monitor.instrument(async { 42 })
    }

    // intervals() inference
    fn _intervals_inference(monitor: &TaskMonitor) {
        let mut intervals = monitor.intervals();
        let _: Option<TaskMetrics> = intervals.next();
    }

    #[tokio::test]
    async fn inference_compiles() {
        let monitor = TaskMonitor::new();
        _partial_annotation(&monitor).await;
        _common_usage(&monitor).await;
        _store_without_annotation(&monitor).await;
        _function_boundary(monitor.instrument(async { 42 })).await;
        _return_position(&monitor).await;
        _intervals_inference(&monitor);
    }
}

#[cfg(test)]
mod future_monitor_tests {
    use super::*;

    // Asserts on durations measured by the crate's clock, which is only the
    // runtime's virtual clock under `start_paused` when the `rt` feature selects
    // `tokio::time::Instant`; without `rt` the crate uses the real `std` clock.
    #[cfg(feature = "rt")]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn captures_per_future_idle_active_polls() {
        let task_monitor = TaskMonitor::new();
        task_monitor
            .instrument(async {
                let (_, m) = FutureMonitor::new()
                    .instrument(async {
                        tokio::task::yield_now().await; // an extra poll
                        tokio::time::sleep(Duration::from_secs(1)).await; // idle
                    })
                    .await;

                assert!(m.poll_count >= 2, "poll_count = {}", m.poll_count);
                assert!(m.idle_count >= 1, "idle_count = {}", m.idle_count);
                assert!(
                    m.total_idle_duration >= Duration::from_secs(1),
                    "total_idle_duration = {:?}",
                    m.total_idle_duration
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn scheduling_is_zero_without_instrumented_root() {
        // No surrounding `TaskMonitor::instrument`, so `try_current()` is `None`
        // and no scheduling delay is attributed — but local metrics still work.
        let (_, m) = FutureMonitor::new()
            .instrument(async {
                tokio::task::yield_now().await;
            })
            .await;

        assert_eq!(m.scheduled_count, 0);
        assert_eq!(m.total_scheduled_duration, Duration::ZERO);
        assert!(m.poll_count >= 1, "poll_count = {}", m.poll_count);
    }

    #[tokio::test]
    async fn try_current_tracks_root_scheduling_when_opted_in() {
        assert!(TaskScheduling::try_current().is_none());

        let mut builder = TaskMonitor::builder();
        builder.publish_scheduling_delay();
        let monitor = builder.build();
        monitor
            .instrument(async {
                // The log exists from the first poll, even before any scheduling.
                let before = TaskScheduling::try_current().expect("inside instrumented task");
                tokio::task::yield_now().await;
                let after = TaskScheduling::try_current().expect("inside instrumented task");
                assert!(
                    after.scheduled_count > before.scheduled_count,
                    "scheduled_count should increase after yielding: {} -> {}",
                    before.scheduled_count,
                    after.scheduled_count
                );
            })
            .await;

        assert!(TaskScheduling::try_current().is_none());
    }

    #[tokio::test]
    async fn try_current_is_none_without_opt_in() {
        // A default monitor does not publish the scheduling log, so the common
        // case pays nothing and `FutureMonitor` scheduling stays zero.
        TaskMonitor::new()
            .instrument(async {
                assert!(TaskScheduling::try_current().is_none());
            })
            .await;
    }

    #[tokio::test]
    async fn instrument_yields_output_and_metrics() {
        // `instrument` consumes the monitor and resolves to the future's output
        // paired with its captured metrics — so a monitor can only ever measure
        // a single future.
        let (output, m) = FutureMonitor::new()
            .instrument(async {
                tokio::task::yield_now().await;
                "done"
            })
            .await;

        assert_eq!(output, "done");
        assert!(m.poll_count >= 1, "poll_count = {}", m.poll_count);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn total_duration_includes_final_poll_execution() {
        // The future completes in a single poll that spends real time
        // executing. `total_duration` is stamped after the poll, so it must
        // include that execution time (a regression would record it before the
        // poll and report ~0).
        let (_, m) = FutureMonitor::new()
            .instrument(async {
                let start = std::time::Instant::now();
                while start.elapsed() < Duration::from_millis(20) {}
            })
            .await;

        assert!(m.poll_count == 1, "poll_count = {}", m.poll_count);
        assert!(
            m.total_duration >= Duration::from_millis(15),
            "total_duration = {:?}",
            m.total_duration
        );
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn nested_future_monitors_capture_independently() {
        let mut builder = TaskMonitor::builder();
        builder.publish_scheduling_delay();
        let root = builder.build();

        root.instrument(async {
            // An outer monitored future that does one extra poll of its own, then
            // runs an inner monitored future that sleeps.
            let (inner, outer) = FutureMonitor::new()
                .instrument(async {
                    tokio::task::yield_now().await; // outer-only extra poll
                    let (_, inner) = FutureMonitor::new()
                        .instrument(async {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        })
                        .await;
                    inner
                })
                .await;

            // Each monitor captures only its own future: the inner one sees just
            // the sleep, the outer one additionally sees its yield poll.
            assert_eq!(inner.poll_count, 2, "inner poll_count = {}", inner.poll_count);
            assert_eq!(inner.idle_count, 1);
            assert_eq!(inner.total_idle_duration, Duration::from_secs(1));

            assert!(
                outer.poll_count > inner.poll_count,
                "outer {} should exceed inner {}",
                outer.poll_count,
                inner.poll_count
            );
            // The outer future is idle for the whole time the inner one sleeps.
            assert_eq!(outer.total_idle_duration, Duration::from_secs(1));
        })
        .await;
    }
}
