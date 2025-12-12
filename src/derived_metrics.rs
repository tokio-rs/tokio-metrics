macro_rules! derived_metrics {
    (
        [$metrics_name:ty] {
            stable {
                $(
                    $(#[$($attributes:tt)*])*
                    $vis:vis fn $name:ident($($args:tt)*) -> $ty:ty $body:block
                )*
            }
            unstable {
                $(
                    $(#[$($unstable_attributes:tt)*])*
                    $unstable_vis:vis fn $unstable_name:ident($($unstable_args:tt)*) -> $unstable_ty:ty $unstable_body:block
                )*
            }
        }
    ) => {
        impl $metrics_name {
            $(
                $(#[$($attributes)*])*
                $vis fn $name($($args)*) -> $ty $body
            )*
            $(
                $(#[$($unstable_attributes)*])*
                #[cfg(tokio_unstable)]
                $unstable_vis fn $unstable_name($($unstable_args)*) -> $unstable_ty $unstable_body
            )*

            #[cfg(all(test, feature = "metrics-rs-integration"))]
            const DERIVED_METRICS: &[&str] = &[$(stringify!($name),)*];
            #[cfg(all(test, tokio_unstable, feature = "metrics-rs-integration"))]
            const UNSTABLE_DERIVED_METRICS: &[&str] = &[$(stringify!($unstable_name),)*];
        }
    };
}

pub(crate) use derived_metrics;
