pub mod edit;
pub mod env;
pub mod feature;
pub mod path;
pub mod profile;
pub mod syscalls;

pub type Set<T> = std::collections::HashSet<T, ahash::RandomState>;
pub type Map<K, V> = std::collections::HashMap<K, V, ahash::RandomState>;

#[macro_export]
macro_rules! debug_timer {
    ($name:literal, $body:block) => {{
        #[cfg(debug_assertions)]
        {
            let start = std::time::Instant::now();
            let result = $body;
            log::info!("{}: {}us", $name, start.elapsed().as_micros());
            result
        }

        #[cfg(not(debug_assertions))]
        $body
    }};

    ($name:literal, $expr:expr) => {{
        #[cfg(debug_assertions)]
        {
            let start = std::time::Instant::now();
            let result = $expr;
            log::info!("{}: {}us", $name, start.elapsed().as_micros());
            result
        }

        #[cfg(not(debug_assertions))]
        $expr
    }};
}
pub use debug_timer;
