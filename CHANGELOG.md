# 0.4.3 (July 3rd, 2025)

### Added
 - rt: partially stabilize `RuntimeMonitor` and related metrics ([#87])

[#87]: https://github.com/tokio-rs/tokio-metrics/pull/87

# 0.4.2 (April 30th, 2025)

### Fixed
 - docs: specify metrics-rs-integration feature dependency for relevant APIs ([#78])
 - docs: fix links ([#79])

[#78]: https://github.com/tokio-rs/tokio-metrics/pull/78
[#79]: https://github.com/tokio-rs/tokio-metrics/pull/79

# 0.4.1 (April 20th, 2025)

### Added
 - rt: add support for `blocking_queue_depth`, `live_task_count`, `blocking_threads_count`,
   `idle_blocking_threads_count` ([#49], [#74])
 - rt: add integration with metrics.rs ([#68])

[#49]: https://github.com/tokio-rs/tokio-metrics/pull/49
[#68]: https://github.com/tokio-rs/tokio-metrics/pull/68
[#74]: https://github.com/tokio-rs/tokio-metrics/pull/74

# 0.4.0 (November 26th, 2024)

The core Tokio crate has renamed some of the metrics and this breaking release
uses the new names. The minimum required Tokio is bumped to 1.41, and the MSRV
is bumped to 1.70 to match.

- runtime: use new names for poll time histogram ([#66])
- runtime: rename injection queue to global queue ([#66])
- doc: various doc fixes ([#66], [#65])

[#65]: https://github.com/tokio-rs/tokio-metrics/pull/65
[#66]: https://github.com/tokio-rs/tokio-metrics/pull/66

# 0.3.1 (October 12th, 2023)

### Fixed
- task: fix doc error in idle definition ([#54])
- chore: support tokio 1.33 without stats feature ([#55])

[#54]: https://github.com/tokio-rs/tokio-metrics/pull/54
[#55]: https://github.com/tokio-rs/tokio-metrics/pull/55

# 0.3.0 (August 14th, 2023)

### Added
- rt: add support for mean task poll time ([#50])
- rt: add support for task poll count histogram ([#52])

[#50]: https://github.com/tokio-rs/tokio-metrics/pull/50
[#52]: https://github.com/tokio-rs/tokio-metrics/pull/52

# 0.2.2 (April 13th, 2023)
### Added
- task: add TaskMonitorBuilder ([#46])

### Fixed
- task: fix default long delay threshold ([#46])

[#46]: https://github.com/tokio-rs/tokio-metrics/pull/46

# 0.2.1 (April 5th, 2023)

### Added
- task: add short and long delay metrics ([#44])

[#44]: https://github.com/tokio-rs/tokio-metrics/pull/44

# 0.2.0 (March 6th, 2023)

### Added
- Add `Debug` implementations. ([#28])
- rt: add concrete `RuntimeIntervals` iterator type ([#26])
- rt: add budget_forced_yield_count metric ([#39])
- rt: add io_driver_ready_count metric ([#40])
- rt: add steal_operations metric ([#37])
- task: also instrument streams ([#31])

### Documented
- doc: fix count in `TaskMonitor` docstring ([#24])
- doc: the description of steal_count ([#35])

[#24]: https://github.com/tokio-rs/tokio-metrics/pull/24
[#26]: https://github.com/tokio-rs/tokio-metrics/pull/26
[#28]: https://github.com/tokio-rs/tokio-metrics/pull/28
[#31]: https://github.com/tokio-rs/tokio-metrics/pull/31
[#35]: https://github.com/tokio-rs/tokio-metrics/pull/35
[#37]: https://github.com/tokio-rs/tokio-metrics/pull/37
[#39]: https://github.com/tokio-rs/tokio-metrics/pull/39
[#40]: https://github.com/tokio-rs/tokio-metrics/pull/40
