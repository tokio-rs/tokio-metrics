# Tokio Metrics

Provides utilities for collecting metrics from a Tokio application, including
runtime and per-task metrics.

```toml
[dependencies]
tokio-metrics = { version = "0.1.0", default-features = false }
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

## Getting Started With Runtime Metrics

This unstable functionality requires `tokio_unstable`, and the  `rt` crate
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

Missing this configuration file during compilation will cause tokio-console to not work, and alternating
between building with and without this configuration file included will cause full rebuilds of your project.

The `rt` feature of `tokio-metrics` is on by default; simply check that you do
not set `default-features = false` when declaring it as a dependency; e.g.:
```toml
[dependencies]
tokio-metrics = "0.1.0"
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
