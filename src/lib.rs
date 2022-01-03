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
    pub mod runtime;
    pub use runtime::RuntimeMetrics;
}

pub mod task;
pub use task::{InstrumentedTask, TaskMetrics};
