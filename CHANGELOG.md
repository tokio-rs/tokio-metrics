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
