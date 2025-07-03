# Tokio Metrics

[![Crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![MIT licensed][mit-badge]][mit-url]
[![Build Status][actions-badge]][actions-url]
[![Discord chat][discord-badge]][discord-url]

[crates-badge]: https://img.shields.io/crates/v/tokio-metrics.svg
[crates-url]: https://crates.io/crates/tokio-metrics
[docs-badge]: https://docs.rs/tokio-metrics/badge.svg
[docs-url]: https://docs.rs/tokio-metrics
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/tokio-rs/tokio-metrics/blob/master/LICENSE
[actions-badge]: https://github.com/tokio-rs/tokio-metrics/workflows/CI/badge.svg
[actions-url]: https://github.com/tokio-rs/tokio-metrics/actions?query=workflow%3ACI+branch%3Amain
[discord-badge]: https://img.shields.io/discord/500028886025895936.svg?logo=discord&style=flat-square
[discord-url]: https://discord.gg/tokio

Provides utilities for collecting metrics from a Tokio application, including
runtime and per-task metrics.

```toml
[dependencies]
tokio-metrics = { version = "0.4.3", default-features = false }
```

## Getting Started With Task Metrics

Use `TaskMonitor` to instrument tasks before spawning them, and to observe
metrics for those tasks. All tasks instrumented with a given `TaskMonitor`
aggregate their metrics together. To split out metrics for different tasks, use
separate `TaskMetrics` instances.

```rust
// construct a TaskMonitor
let monitor = tokio_metrics::TaskMonitor::new();

// print task metrics every 500ms
{
    let frequency = std::time::Duration::from_millis(500);
    let monitor = monitor.clone();
    tokio::spawn(async move {
        for metrics in monitor.intervals() {
            println!("{:?}", metrics);
            tokio::time::sleep(frequency).await;
        }
    });
}

// instrument some tasks and spawn them
loop {
    tokio::spawn(monitor.instrument(do_work()));
}
```

### Task Metrics
#### Base Metrics
- **[`instrumented_count`]**  
  The number of tasks instrumented.
- **[`dropped_count`]**  
  The number of tasks dropped.
- **[`first_poll_count`]**  
  The number of tasks polled for the first time.
- **[`total_first_poll_delay`]**  
  The total duration elapsed between the instant tasks are instrumented, and the instant they are first polled.
- **[`total_idled_count`]**  
  The total number of times that tasks idled, waiting to be awoken.
- **[`total_idle_duration`]**  
  The total duration that tasks idled.
- **[`total_scheduled_count`]**  
  The total number of times that tasks were awoken (and then, presumably, scheduled for execution).
- **[`total_scheduled_duration`]**  
  The total duration that tasks spent waiting to be polled after awakening.
- **[`total_poll_count`]**  
  The total number of times that tasks were polled.
- **[`total_poll_duration`]**  
  The total duration elapsed during polls.
- **[`total_fast_poll_count`]**  
  The total number of times that polling tasks completed swiftly.
- **[`total_fast_poll_duration`]**  
  The total duration of fast polls.
- **[`total_slow_poll_count`]**  
  The total number of times that polling tasks completed slowly.
- **[`total_slow_poll_duration`]**
  The total duration of slow polls.
- **[`total_short_delay_count`]**
  The total count of short scheduling delays.
- **[`total_short_delay_duration`]**
  The total duration of short scheduling delays.
- **[`total_long_delay_count`]**
  The total count of long scheduling delays.
- **[`total_long_delay_duration`]**
  The total duration of long scheduling delays.

#### Derived Metrics
- **[`mean_first_poll_delay`]**  
  The mean duration elapsed between the instant tasks are instrumented, and the instant they are first polled.
- **[`mean_idle_duration`]**  
  The mean duration of idles.
- **[`mean_scheduled_duration`]**  
  The mean duration that tasks spent waiting to be executed after awakening.
- **[`mean_poll_duration`]**  
  The mean duration of polls.
- **[`slow_poll_ratio`]**  
  The ratio between the number polls categorized as slow and fast.
- **[`mean_fast_poll_duration`]**  
  The mean duration of fast polls.
- **[`mean_slow_poll_duration`]**  
  The mean duration of slow polls.
- **[`long_delay_ratio`]**
- The ratio between the number of long scheduling delays and the number of total schedules.
- **[`mean_short_delay_duration`]**
  The mean duration of short schedules.
- **[`mean_long_delay_duration`]**
  The mean duration of long schedules.

[`instrumented_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.instrumented_count
[`dropped_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.dropped_count
[`first_poll_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.first_poll_count
[`total_first_poll_delay`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_first_poll_delay 
[`total_idled_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_idled_count
[`total_idle_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_idle_duration 
[`total_scheduled_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_scheduled_count 
[`total_scheduled_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_scheduled_duration
[`total_poll_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_poll_count
[`total_poll_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_poll_duration
[`total_fast_poll_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_fast_poll_count 
[`total_fast_poll_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_fast_poll_duration 
[`total_slow_poll_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_slow_poll_count
[`total_slow_poll_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_slow_poll_duration 
[`mean_first_poll_delay`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_first_poll_delay
[`mean_idle_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_idle_duration
[`mean_scheduled_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_scheduled_duration
[`mean_poll_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_poll_duration
[`slow_poll_ratio`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.slow_poll_ratio
[`mean_fast_poll_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_fast_poll_duration
[`mean_slow_poll_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_slow_poll_duration
[`total_short_delay_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_short_delay_count
[`total_short_delay_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_short_delay_duration
[`total_long_delay_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_long_delay_count
[`total_long_delay_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#structfield.total_long_delay_duration
[`long_delay_ratio`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.long_delay_ratio
[`mean_short_delay_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_short_delay_duration
[`mean_long_delay_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.TaskMetrics.html#method.mean_long_delay_duration

## Getting Started With Runtime Metrics

Not all runtime metrics are stable. Using unstable metrics requires `tokio_unstable`, and the  `rt` crate
feature. To enable `tokio_unstable`, the `--cfg` `tokio_unstable` must be passed
to `rustc` when compiling. You can do this by setting the `RUSTFLAGS`
environment variable before compiling your application; e.g.:
```sh
RUSTFLAGS="--cfg tokio_unstable" cargo build
```
Or, by creating the file `.cargo/config.toml` in the root directory of your crate.
If you're using a workspace, put this file in the root directory of your workspace instead.
```toml
[build]
rustflags = ["--cfg", "tokio_unstable"]
rustdocflags = ["--cfg", "tokio_unstable"] 
```
Putting `.cargo/config.toml` files below the workspace or crate root directory may lead to tools like
Rust-Analyzer or VSCode not using your `.cargo/config.toml` since they invoke cargo from
the workspace or crate root and cargo only looks for the `.cargo` directory in the current & parent directories.
Cargo ignores configurations in child directories.
More information about where cargo looks for configuration files can be found
[here](https://doc.rust-lang.org/cargo/reference/config.html).

Missing this configuration file during compilation will cause tokio-metrics to not work, and alternating
between building with and without this configuration file included will cause full rebuilds of your project.

### Stable Runtime Metrics

- **[`workers_count`]**
- **[`total_park_count`]**
- **[`max_park_count`]**
- **[`min_park_count`]**
- **[`total_busy_duration`]**
- **[`max_busy_duration`]**
- **[`min_busy_duration`]**
- **[`global_queue_depth`]**

### Collecting Runtime Metrics directly

The `rt` feature of `tokio-metrics` is on by default; simply check that you do
not set `default-features = false` when declaring it as a dependency; e.g.:
```toml
[dependencies]
tokio-metrics = "0.4.3"
```

From within a Tokio runtime, use `RuntimeMonitor` to monitor key metrics of
that runtime.
```rust
let handle = tokio::runtime::Handle::current();
let runtime_monitor = tokio_metrics::RuntimeMonitor::new(&handle);

// print runtime metrics every 500ms
let frequency = std::time::Duration::from_millis(500);
tokio::spawn(async move {
    for metrics in runtime_monitor.intervals() {
        println!("Metrics = {:?}", metrics);
        tokio::time::sleep(frequency).await;
    }
});

// run some tasks
tokio::spawn(do_work());
tokio::spawn(do_work());
tokio::spawn(do_work());
```

### Collecting Runtime Metrics via metrics.rs

If you also enable the `metrics-rs-integration` feature, you can use [metrics.rs] exporters to export metrics outside of your process. `metrics.rs` supports a variety of exporters, including [Prometheus].

The exported metrics by default will be exported with their name, preceded by `tokio_`. For example, `tokio_workers_count` for the [`workers_count`] metric. This can be customized by using the [`with_metrics_tranformer`] function.

If you want to use [Prometheus], you could have this `Cargo.toml`:

[Prometheus]: https://prometheus.io
[`with_metrics_tranformer`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetricsReporterBuilder.html#method.with_metrics_transformer

```toml
[dependencies]
tokio-metrics = { version = "0.4.3", features = ["metrics-rs-integration"] }
metrics = "0.24"
# You don't actually need to use the Prometheus exporter with uds-listener enabled,
# it's just here as an example.
metrics-exporter-prometheus = { version = "0.16", features = ["uds-listener"] }
```

Then, you can launch a metrics exporter:
```rust
// This makes metrics visible via a local Unix socket with name prometheus.sock
// You probably want to do it differently.
//
// If you use this exporter, you can access the metrics for debugging
// by running `curl --unix-socket prometheus.sock localhost`.
metrics_exporter_prometheus::PrometheusBuilder::new()
    .with_http_uds_listener("prometheus.sock")
    .install()
    .unwrap();

// This line launches the reporter that monitors the Tokio runtime and exports the metrics.
tokio::task::spawn(
    tokio_metrics::RuntimeMetricsReporterBuilder::default().describe_and_run(),
);

// run some tasks
tokio::spawn(do_work());
tokio::spawn(do_work());
tokio::spawn(do_work());
```

Of course, it will work with any other [metrics.rs] exporter.

[metrics.rs]: https://docs.rs/metrics

### Runtime Metrics
#### Base Metrics
- **[`workers_count`]**  
  The number of worker threads used by the runtime.
- **[`total_park_count`]**  
  The number of times worker threads parked.
- **[`max_park_count`]**  
  The maximum number of times any worker thread parked.
- **[`min_park_count`]**  
  The minimum number of times any worker thread parked.
- **[`total_noop_count`]**  
  The number of times worker threads unparked but performed no work before parking again.
- **[`max_noop_count`]**  
  The maximum number of times any worker thread unparked but performed no work before parking again.
- **[`min_noop_count`]**  
  The minimum number of times any worker thread unparked but performed no work before parking again.
- **[`total_steal_count`]**  
  The number of tasks worker threads stole from another worker thread.
- **[`max_steal_count`]**  
  The maximum number of tasks any worker thread stole from another worker thread.
- **[`min_steal_count`]**  
  The minimum number of tasks any worker thread stole from another worker thread.
- **[`total_steal_operations`]**  
  The number of times worker threads stole tasks from another worker thread.
- **[`max_steal_operations`]**  
  The maximum number of times any worker thread stole tasks from another worker thread.
- **[`min_steal_operations`]**  
  The minimum number of times any worker thread stole tasks from another worker thread.
- **[`num_remote_schedules`]**  
  The number of tasks scheduled from outside of the runtime.
- **[`total_local_schedule_count`]**  
  The number of tasks scheduled from worker threads.
- **[`max_local_schedule_count`]**  
  The maximum number of tasks scheduled from any one worker thread.
- **[`min_local_schedule_count`]**  
  The minimum number of tasks scheduled from any one worker thread.
- **[`total_overflow_count`]**  
  The number of times worker threads saturated their local queues.
- **[`max_overflow_count`]**  
  The maximum number of times any one worker saturated its local queue.
- **[`min_overflow_count`]**  
  The minimum number of times any one worker saturated its local queue.
- **[`total_polls_count`]**  
  The number of tasks that have been polled across all worker threads.
- **[`max_polls_count`]**  
  The maximum number of tasks that have been polled in any worker thread.
- **[`min_polls_count`]**  
  The minimum number of tasks that have been polled in any worker thread.
- **[`total_busy_duration`]**  
  The amount of time worker threads were busy.
- **[`max_busy_duration`]**  
  The maximum amount of time a worker thread was busy.
- **[`min_busy_duration`]**  
  The minimum amount of time a worker thread was busy.
- **[`injection_queue_depth`]**  
  The number of tasks currently scheduled in the runtime's injection queue.
- **[`total_local_queue_depth`]**  
  The total number of tasks currently scheduled in workers' local queues.
- **[`max_local_queue_depth`]**  
  The maximum number of tasks currently scheduled any worker's local queue.
- **[`min_local_queue_depth`]**  
  The minimum number of tasks currently scheduled any worker's local queue.
- **[`blocking_queue_depth`]**
  The number of tasks currently waiting to be executed in the blocking threadpool.
- **[`live_tasks_count`]**
  The current number of alive tasks in the runtime.
- **[`blocking_threads_count`]**
  The number of additional threads spawned by the runtime.
- **[`idle_blocking_threads_count`]**
  The number of idle threads, which have spawned by the runtime for `spawn_blocking` calls.
- **[`elapsed`]**  
  Total amount of time elapsed since observing runtime metrics.
- **[`budget_forced_yield_count`]**
  The number of times that a task was forced to yield because it exhausted its budget.
- **[`io_driver_ready_count`]**
  The number of ready events received from the I/O driver.

#### Derived Metrics
- **[`mean_polls_per_park`]**
- **[`busy_ratio`]**

[`workers_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.workers_count
[`total_park_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_park_count
[`max_park_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_park_count
[`min_park_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_park_count
[`total_noop_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_noop_count
[`max_noop_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_noop_count
[`min_noop_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_noop_count
[`total_steal_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_steal_count
[`max_steal_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_steal_count
[`min_steal_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_steal_count
[`total_steal_operations`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_steal_operations
[`max_steal_operations`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_steal_operations
[`min_steal_operations`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_steal_operations
[`num_remote_schedules`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.num_remote_schedules
[`total_local_schedule_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_local_schedule_count
[`max_local_schedule_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_local_schedule_count
[`min_local_schedule_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_local_schedule_count
[`total_overflow_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_overflow_count
[`max_overflow_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_overflow_count
[`min_overflow_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_overflow_count
[`total_polls_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_polls_count
[`max_polls_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_polls_count
[`min_polls_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_polls_count
[`total_busy_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_busy_duration
[`max_busy_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_busy_duration
[`min_busy_duration`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_busy_duration
[`injection_queue_depth`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.injection_queue_depth
[`total_local_queue_depth`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.total_local_queue_depth
[`blocking_queue_depth`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.blocking_queue_depth
[`live_tasks_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.live_tasks_count
[`blocking_threads_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.blocking_threads_count
[`idle_blocking_threads_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.idle_blocking_threads_count
[`max_local_queue_depth`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.max_local_queue_depth
[`min_local_queue_depth`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.min_local_queue_depth
[`elapsed`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.elapsed
[`mean_polls_per_park`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#method.mean_polls_per_park
[`busy_ratio`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#method.busy_ratio
[`budget_forced_yield_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.budget_forced_yield_count
[`io_driver_ready_count`]: https://docs.rs/tokio-metrics/0.4.*/tokio_metrics/struct.RuntimeMetrics.html#structfield.io_driver_ready_count


## Relation to Tokio Console

Currently, Tokio Console is primarily intended for **local** debugging. Tokio
metrics is intended to enable reporting of metrics in production to your
preferred tools. Longer term, it is likely that `tokio-metrics` will merge with
Tokio Console.

## License

This project is licensed under the [MIT license].

[MIT license]: LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in tokio-metrics by you, shall be licensed as MIT, without any
additional terms or conditions.
