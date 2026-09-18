# OpenTelemetry runtime metrics integration

Proposal for [#63](https://github.com/tokio-rs/tokio-metrics/issues/63).

## Why add this?

I'd like to add an optional OpenTelemetry integration to `tokio-metrics`. This would let applications send runtime metrics through their existing OpenTelemetry setup using a `Meter` they provide.

I'm starting with this design-only PR to agree on the approach before implementing it. The names and API below are suggestions.

## Proposed approach

I'd put the integration behind an `opentelemetry-integration` feature and depend on the `opentelemetry` API crate with its metrics feature enabled. The application would own SDK and exporter configuration. The SDK would only be needed here for tests and examples.

My starting point is a reporter that samples `RuntimeMonitor::intervals()`, similar to the existing `metrics-rs` reporter. It would record interval deltas as counter additions and current or interval-derived values as gauges.

I considered observable callbacks too. Reading raw cumulative counters through callbacks is a reasonable alternative, but advancing a shared interval iterator from independently scheduled SDK readers would make the sampling periods depend on those readers. A reporter gives the interval-derived metrics one explicit sampling schedule. I'd appreciate feedback on this tradeoff.

## Example usage

Something like this could work; these types and methods are proposed, not existing APIs:

```rust
let reporter = OpenTelemetryReporterBuilder::new(meter)
    .with_interval(Duration::from_secs(30))
    .with_runtime_name("main")
    .build_with_monitor(monitor);

let task = tokio::spawn(reporter.run());
```

I'd also expose `report_once()` for applications that want to control sampling themselves. Exact names, defaults, and validation can be settled during implementation.

For periodic reporting, I'd use a nonzero interval and skip missed timer ticks rather than run catch-up samples. The interval baseline and first report should be arranged so the first sample covers a meaningful period.

The caller would own the reporting task and stop it explicitly when needed. Dropping its `JoinHandle` would detach it, so cancellation would require aborting the task or a shutdown mechanism.

A runtime name would distinguish runtimes sharing a meter. Callers should use a small, stable set of names. Without distinct attributes, counters from different runtimes could be combined and gauge readings could overwrite each other.

## Initial metrics

I'd start with stable runtime counters and gauges. Here are three representative mappings; the metric names are provisional.

| Source | Proposed name | Instrument | Unit |
| --- | --- | --- | --- |
| `workers_count` | `tokio.runtime.workers` | `Gauge<u64>` | `{worker}` |
| `total_park_count` | `tokio.runtime.parks` | `Counter<u64>` | `{event}` |
| `busy_ratio()` | `tokio.runtime.busy_ratio` | `Gauge<f64>` | `1` |

The counter values yielded by each interval are deltas, so the reporter would call `add()` with those values. The SDK reader/exporter would control the exported temporality. Gauges such as the busy ratio would describe the latest sampled interval, not necessarily the exporter's entire collection period.

I'd leave the full metric list and test cases for the implementation PR once we agree on the approach. Task metrics, per-worker labels, and metrics requiring `tokio_unstable` would be follow-up work.

## Histograms

I'd leave pre-aggregated duration histograms out of the first implementation. `DurationHistogram` provides bucket counts, while the OpenTelemetry Rust histogram API records individual values and has no equivalent of `record_many(value, count)`.

Replaying a representative value for every observation would make reporting cost grow with runtime activity, and it would only approximate the original measurements. Recording one value per bucket would lose the observation counts. This needs a separate design before we can support it properly.

## Rust version and dependencies

The selected OpenTelemetry version may require a newer Rust version than the baseline supported by `tokio-metrics`. I'd check the exact dependency requirements before implementation and discuss whether this feature should have its own MSRV, following the approach used for optional integrations such as `metrique`.

Accepting a `Meter` would let callers choose the instrumentation scope without passing a provider into the reporter. I'd also keep SDK internals and configurable metric prefixes out of the initial API to keep the integration small.

Exporter setup, Collector deployment, eBPF instrumentation, tracing spans, and dashboards are outside this proposal.
