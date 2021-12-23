# Tokio Metrics

Provides utilities for collecting metrics from a Tokio application, including
runtime and per-task metrics.

## Getting Started

From within a Tokio application, use `TaskMetrics` to instrument tasks before
spawning them and to observe the metrics. All tasks instrumented with a given
`TaskMetrics` instance will aggregate their metrics together. To split out
metrics for different tasks, use separate `TaskMetrics` instances.

```rust
// Create a `TaskMetrics` instance
let metrics = tokio_metrics::TaskMetrics::new();

// Instrument any number of tasks using the `TaskMetrics` instance
tokio::spawn(metrics.instrument(async {
    // Do work here
}));

// Use the `TaskMetrics` instance to observe reported data.
for sample in metrics.sample() {
    println!("Metrics = {:?}", sample);

    // Wait some time before sampling metrics again.
    tokio::time::sleep(Duration::from_millis(100)).await;
}
```

## Relation to Tokio Console

## License

This project is licensed under the [MIT license].

[MIT license]: LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in tokio-metrics by you, shall be licensed as MIT, without any
additional terms or conditions.
