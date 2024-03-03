//! # Tokio runtime health monitoring detector(s).
//!
//! ## Detecting blocked tokio workers: [`LongRunningTaskDetector`].
//!
//! Blocking IO operations often end up in tokio workers negatively impacting the ability of tokio runtimes to process
//! async requests. The simplest example of this is the use of [`std::thread::sleep`] instead of [`tokio::time::sleep`] which we use
//! in the unit tests to test this utility.
//!
//! The aim of this utility is to detect these situations.
//! [`LongRunningTaskDetector`] is designed to be very low overhead so that it can be safely be run in production.
//! The overhead of this utility is vconfigurable via probing `interval` parameter.
//!
//! ### [`LongRunningTaskDetector`] Example:
//!
//! ```
//! use std::sync::Arc;
//! use tokio_metrics::detectors::LongRunningTaskDetector;
//!
//! let (lrtd, mut builder) = LongRunningTaskDetector::new_multi_threaded(
//!   std::time::Duration::from_millis(10),
//!   std::time::Duration::from_millis(100)
//! );
//! let runtime = builder.worker_threads(2).enable_all().build().unwrap();
//! let runtime_ref = Arc::new(runtime);
//! let lrtd_runtime_ref = runtime_ref.clone();
//! lrtd.start(lrtd_runtime_ref);
//! runtime_ref.block_on(async {
//!   print!("my async code")
//! });
//!
//! ```
//!
//! The above will allow you to get details on what is blocking your tokio worker threads for longer that 100ms.
//! The detail with default action handler will look like:
//!    
//! ```text
//! Detected blocking in worker threads: [
//!  ThreadInfo { id: ThreadId(10), pthread_id: 123145381474304 },
//!  ThreadInfo { id: ThreadId(11), pthread_id: 123145385693184 }
//! ]
//! ```
//!   
//! To get more details(like stack traces) start [`LongRunningTaskDetector`] with [`LongRunningTaskDetector::start_with_custom_action`]
//! and provide a custom handler([`BlockingActionHandler`]) that can dump the thread stack traces. The [`LongRunningTaskDetector`] integration tests
//! include an example implementation that is not signal safe as an example.
//! More detailed blocking can look like:
//!
//! ```text
//! Blocking detected with details: 123145387802624
//! Stack trace for thread tokio-runtime-worker(123145387802624):
//! ...
//!   5: __sigtramp
//!   6: ___semwait_signal
//!   7: <unknown>
//!   8: std::sys::pal::unix::thread::Thread::sleep
//!             at /rustc/.../library/std/src/sys/pal/unix/thread.rs:243:20
//!   9: std::thread::sleep
//!             at /rustc/.../library/std/src/thread/mod.rs:869:5
//!  10: detectors::unix_lrtd_tests::run_blocking_stuff::{{closure}}
//!             at ./tests/detectors.rs:98:9
//!  11: tokio::runtime::task::core::Core<T,S>::poll::{{closure}}
//! ...
//!
//! which will help you easilly identify the blocking operation(s).
//! ```

use rand::thread_rng;
use rand::Rng;
use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Duration;
use std::{env, thread};
use tokio::runtime::{Builder, Runtime};

const PANIC_WORKER_BLOCK_DURATION_DEFAULT: Duration = Duration::from_secs(60);

fn get_panic_worker_block_duration() -> Duration {
    let duration_str = env::var("MY_DURATION_ENV").unwrap_or_else(|_| "60".to_string());
    duration_str
        .parse::<u64>()
        .map(Duration::from_secs)
        .unwrap_or(PANIC_WORKER_BLOCK_DURATION_DEFAULT)
}

#[cfg(unix)]
fn get_thread_id() -> libc::pthread_t {
    unsafe { libc::pthread_self() }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct ThreadInfo {
    id: ThreadId,
    #[cfg(unix)]
    pthread_id: libc::pthread_t,
}

/// A structure to hold information about a thread, including its platform-specific identifiers.
impl ThreadInfo {
    fn new() -> Self {
        ThreadInfo {
            id: thread::current().id(),
            #[cfg(unix)]
            pthread_id: get_thread_id(),
        }
    }

    /// Returns the id
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Returns the `pthread_id` of this thread.
    #[cfg(unix)]
    #[cfg_attr(docsrs, doc(cfg(unix)))]
    pub fn pthread_id(&self) -> &libc::pthread_t {
        &self.pthread_id
    }
}

/// A trait for handling actions when blocking is detected.
///
/// This trait provides a method for handling the detection of a blocking action.
pub trait BlockingActionHandler: Send + Sync {
    /// Called when a blocking action is detected and prior to thread signaling.
    ///
    /// # Arguments
    ///
    /// * `workers` - The list of thread IDs of the tokio runtime worker threads.   /// # Returns
    ///
    fn blocking_detected(&self, workers: &[ThreadInfo]);
}

impl<F> BlockingActionHandler for F
where
    F: Fn(&[ThreadInfo]) + Send + Sync,
{
    fn blocking_detected(&self, workers: &[ThreadInfo]) {
        // Implement the behavior for blocking_detected using the provided function.
        // You can call the function here with the given workers.
        self(workers);
    }
}
struct StdErrBlockingActionHandler;

/// BlockingActionHandler implementation that writes blocker details to standard error.
impl BlockingActionHandler for StdErrBlockingActionHandler {
    fn blocking_detected(&self, workers: &[ThreadInfo]) {
        eprintln!("Detected blocking in worker threads: {:?}", workers);
    }
}

#[derive(Debug)]
struct WorkerSet {
    inner: Mutex<HashSet<ThreadInfo>>,
}

impl WorkerSet {
    fn new() -> Self {
        WorkerSet {
            inner: Mutex::new(HashSet::new()),
        }
    }

    fn add(&self, pid: ThreadInfo) {
        let mut set = self.inner.lock().unwrap();
        set.insert(pid);
    }

    #[cfg(feature = "detectors-multi-thread")]
    fn remove(&self, pid: ThreadInfo) {
        let mut set = self.inner.lock().unwrap();
        set.remove(&pid);
    }

    fn get_all(&self) -> Vec<ThreadInfo> {
        let set = self.inner.lock().unwrap();
        set.iter().cloned().collect()
    }
}

/// Worker health monitoring detector to help with detecting blocking in tokio workers.
///
/// Blocking IO operations often end up in tokio workers negatively impacting the ability of tokio runtimes to process
/// async requests. The simplest example of this is the use of [`std::thread::sleep`] instead of [`tokio::time::sleep`] which we use
/// in the unit tests to test this utility.
///
/// The aim of this utility is to detect these situations.
/// [`LongRunningTaskDetector`] is designed to be very low overhead so that it can be safely be run in production.
/// The overhead of this utility is vconfigurable via probing `interval` parameter.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use tokio_metrics::detectors::LongRunningTaskDetector;
///
/// let (lrtd, mut builder) = LongRunningTaskDetector::new_multi_threaded(
///   std::time::Duration::from_millis(10),
///   std::time::Duration::from_millis(100)
/// );
/// let runtime = builder.worker_threads(2).enable_all().build().unwrap();
/// let runtime_ref = Arc::new(runtime);
/// let lrtd_runtime_ref = runtime_ref.clone();
/// lrtd.start(lrtd_runtime_ref);
/// runtime_ref.block_on(async {
///   print!("my async code")
/// });
///
/// ```
///
/// The above will allow you to get details on what is blocking your tokio worker threads for longer that 100ms.
/// The detail with default action handler will look like:
///    
/// ```text
/// Detected blocking in worker threads: [
///  ThreadInfo { id: ThreadId(10), pthread_id: 123145381474304 },
///  ThreadInfo { id: ThreadId(11), pthread_id: 123145385693184 }
/// ]
/// ```
///   
/// To get more details(like stack traces) start [`LongRunningTaskDetector`] with [`LongRunningTaskDetector::start_with_custom_action`]
/// and provide a custom handler([`BlockingActionHandler`]) that can dump the thread stack traces. The [`LongRunningTaskDetector`] integration tests
/// include an example implementation that is not signal safe as an example.
/// More detailed blocking can look like:
///
/// ```text
/// Blocking detected with details: 123145387802624
/// Stack trace for thread tokio-runtime-worker(123145387802624):
/// ...
///   5: __sigtramp
///   6: ___semwait_signal
///   7: <unknown>
///   8: std::sys::pal::unix::thread::Thread::sleep
///             at /rustc/.../library/std/src/sys/pal/unix/thread.rs:243:20
///   9: std::thread::sleep
///             at /rustc/.../library/std/src/thread/mod.rs:869:5
///  10: detectors::unix_lrtd_tests::run_blocking_stuff::{{closure}}
///             at ./tests/detectors.rs:98:9
///  11: tokio::runtime::task::core::Core<T,S>::poll::{{closure}}
/// ...
///
/// which will help you easilly identify the blocking operation(s).
/// ```
#[derive(Debug)]
pub struct LongRunningTaskDetector {
    interval: Duration,
    detection_time: Duration,
    stop_flag: Arc<Mutex<bool>>,
    workers: Arc<WorkerSet>,
}

async fn do_nothing(tx: mpsc::Sender<()>) {
    // signal I am done
    tx.send(()).unwrap();
}

fn probe(
    tokio_runtime: &Arc<Runtime>,
    detection_time: Duration,
    workers: &Arc<WorkerSet>,
    action: &Arc<dyn BlockingActionHandler>,
) {
    let (tx, rx) = mpsc::channel();
    let _nothing_handle = tokio_runtime.spawn(do_nothing(tx));
    let is_probe_success = match rx.recv_timeout(detection_time) {
        Ok(_result) => true,
        Err(_) => false,
    };
    if !is_probe_success {
        let targets = workers.get_all();
        action.blocking_detected(&targets);
        rx.recv_timeout(get_panic_worker_block_duration()).unwrap();
    }
}

impl LongRunningTaskDetector {
    /// Creates [`LongRunningTaskDetector`] and a current threaded [`tokio::runtime::Builder`].
    ///
    /// The `interval` argument determines the time interval between tokio runtime worker probing.
    /// This interval is randomized.
    ///
    /// The `detection_time` argument determines maximum time allowed for a probe to succeed.
    /// A probe running for longer is considered a tokio worker health issue. (something is blocking the worker threads)
    ///
    /// # Example
    ///
    /// ```
    /// use tokio_metrics::detectors::LongRunningTaskDetector;
    /// use std::time::Duration;
    ///
    /// let (detector, builder) = LongRunningTaskDetector::new_current_threaded(Duration::from_secs(1), Duration::from_secs(5));
    /// ```
    pub fn new_current_threaded(interval: Duration, detection_time: Duration) -> (Self, Builder) {
        let workers = Arc::new(WorkerSet::new());
        workers.add(ThreadInfo::new());
        let runtime_builder = Builder::new_current_thread();
        (
            LongRunningTaskDetector {
                interval,
                detection_time,
                stop_flag: Arc::new(Mutex::new(true)),
                workers,
            },
            runtime_builder,
        )
    }

    /// Creates [`LongRunningTaskDetector`] and a multi threaded [`tokio::runtime::Builder`].
    ///
    /// The `interval` argument determines the time interval between tokio runtime worker probing.
    /// This `interval`` is randomized.
    ///
    /// The `detection_time` argument determines maximum time allowed for a probe to succeed.
    /// A probe running for longer is considered a tokio worker health issue. (something is blocking the worker threads)
    ///
    /// # Example
    ///
    /// ```
    /// use tokio_metrics::detectors::LongRunningTaskDetector;
    /// use std::time::Duration;
    ///
    /// let (detector, builder) = LongRunningTaskDetector::new_multi_threaded(Duration::from_secs(1), Duration::from_secs(5));
    /// ```
    #[cfg(feature = "detectors-multi-thread")]
    #[cfg_attr(docsrs, doc(cfg(feature = "detectors-multi-thread")))]
    pub fn new_multi_threaded(interval: Duration, detection_time: Duration) -> (Self, Builder) {
        let workers = Arc::new(WorkerSet::new());
        let mut runtime_builder = Builder::new_multi_thread();
        let workers_clone = Arc::clone(&workers);
        let workers_clone2 = Arc::clone(&workers);
        runtime_builder
            .on_thread_start(move || {
                workers_clone.add(ThreadInfo::new());
            })
            .on_thread_stop(move || {
                workers_clone2.remove(ThreadInfo::new());
            });
        (
            LongRunningTaskDetector {
                interval,
                detection_time,
                stop_flag: Arc::new(Mutex::new(true)),
                workers,
            },
            runtime_builder,
        )
    }

    /// Starts the monitoring thread with default action handlers (that write details to std err).   
    pub fn start(&self, runtime: Arc<Runtime>) {
        self.start_with_custom_action(runtime, Arc::new(StdErrBlockingActionHandler))
    }

    /// Starts the monitoring process with custom action handlers that
    /// allow you to customize what happens when blocking is detected.
    pub fn start_with_custom_action(
        &self,
        runtime: Arc<Runtime>,
        action: Arc<dyn BlockingActionHandler>,
    ) {
        *self.stop_flag.lock().unwrap() = false;
        let stop_flag = Arc::clone(&self.stop_flag);
        let detection_time = self.detection_time;
        let interval = self.interval;
        let workers = Arc::clone(&self.workers);
        thread::spawn(move || {
            let mut rng = thread_rng();
            while !*stop_flag.lock().unwrap() {
                probe(&runtime, detection_time, &workers, &action);
                thread::sleep(Duration::from_micros(
                    rng.gen_range(10..=interval.as_micros().try_into().unwrap()),
                ));
            }
        });
    }

    /// Stops the monitoring thread. Does nothing if monitoring thread is already stopped.
    pub fn stop(&self) {
        let mut sf = self.stop_flag.lock().unwrap();
        if !(*sf) {
            *sf = true;
        }
    }
}

impl Drop for LongRunningTaskDetector {
    fn drop(&mut self) {
        self.stop();
    }
}
