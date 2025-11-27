macro_rules! cfg_rt {
    ($($item:item)*) => {
        $(
            #[cfg(all(tokio_unstable, feature = "rt"))]
            #[cfg_attr(docsrs, doc(cfg(all(tokio_unstable, feature = "rt"))))]
            $item
        )*
    };
}

cfg_rt! {
    #[cfg(feature = "metrics-rs-integration")]
    #[test]
    fn main() {
        use metrics::Key;
        use metrics_util::debugging::DebugValue;
        use std::{sync::Arc, time::Duration};
        use tokio::runtime::{HistogramConfiguration, LogHistogram};
        use tokio_metrics::{RuntimeMetricsReporterBuilder,TaskMetricsReporterBuilder,TaskMonitor};

        let worker_threads = 10;

        let config = HistogramConfiguration::log(LogHistogram::default());

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_time()
            .enable_metrics_poll_time_histogram()
            .metrics_poll_time_histogram_configuration(config)
            .worker_threads(worker_threads)
            .build()
            .unwrap();

        rt.block_on(async {
            // test runtime metrics
            let recorder = Arc::new(metrics_util::debugging::DebuggingRecorder::new());
            metrics::set_global_recorder(recorder.clone()).unwrap();
            tokio::task::spawn(RuntimeMetricsReporterBuilder::default().with_interval(Duration::from_millis(100)).describe_and_run());
            let mut done = false;
            for _ in 0..1000 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let snapshot = recorder.snapshotter().snapshot().into_vec();
                if let Some(metric) = snapshot.iter().find(|metrics| {
                    metrics.0.key().name() == "tokio_workers_count"
                }) {
                    done = true;
                    match metric {
                        (_, Some(metrics::Unit::Count), Some(s), DebugValue::Gauge(count))
                            if &s[..] == "The number of worker threads used by the runtime" =>
                        {
                            assert_eq!(count.into_inner() as usize, worker_threads);
                        }
                        _ => panic!("bad {metric:?}"),
                    }
                    break;
                }
            }
            assert!(done, "metric not found");
            tokio::task::spawn(async {
                // spawn a thread with a long poll time, let's see we can find it
                std::thread::sleep(std::time::Duration::from_millis(100));
            }).await.unwrap();
            let mut long_polls_found = 0;
            for _ in 0..15 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let snapshot = recorder.snapshotter().snapshot().into_vec();
                if let Some(metric) = snapshot.iter().find(|metrics| {
                    metrics.0.key().name() == "tokio_poll_time_histogram"
                }) {
                    match metric {
                        (_, Some(metrics::Unit::Microseconds), Some(s), DebugValue::Histogram(hist))
                            if &s[..] == "A histogram of task polls since the previous probe grouped by poll times" =>
                        {
                            for entry in hist {
                                // look for a poll of around 100 milliseconds
                                // the default bucket for 100 milliseconds is between 100 and 100/1.25 = 80
                                if entry.into_inner() >= 80e3 && entry.into_inner() <= 250e3 {
                                    long_polls_found += 1;
                                }
                            }
                        }
                        _ => panic!("bad {metric:?}"),
                    }
                }
                let metric = snapshot.iter().find(|metrics| {
                    metrics.0.key().name() == "tokio_total_polls_count"
                }).unwrap();
                match metric {
                    (_, Some(metrics::Unit::Count), Some(s), DebugValue::Counter(count))
                        if &s[..] == "The number of tasks that have been polled across all worker threads" && *count > 0 =>
                    {
                    }
                    _ => panic!("bad {metric:?}"),
                }
                if long_polls_found > 0 {
                    break
                }
            }
            // check that we found exactly 1 poll in the 100ms region
            assert_eq!(long_polls_found, 1);

            // test task metrics
            let task_monitor = TaskMonitor::new();
            tokio::task::spawn(
                TaskMetricsReporterBuilder::new(|name| {
                    let name = name.replacen("tokio_", "task_", 1);
                    Key::from_parts::<_, &[(&str, &str)]>(name, &[])
                })
                .with_interval(Duration::from_millis(100))
                .describe_and_run(task_monitor.clone())
            );
            task_monitor.instrument(async {}).await;

            let mut done = false;
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let snapshot = recorder.snapshotter().snapshot().into_vec();
                if let Some(metric) = snapshot.iter().find(|metrics| {
                    metrics.0.key().name() == "task_first_poll_count"
                }) {
                    match metric {
                        (_, Some(metrics::Unit::Count), Some(s), DebugValue::Gauge(count))
                            if &s[..] == "The number of tasks polled for the first time." =>
                        {
                            if count.into_inner() == 1.0 {
                                done = true;
                                break;
                            }
                        }
                        _ => panic!("bad {metric:?}"),
                    }
                }
            }
            assert!(done, "metric not found");
        });
    }
}
