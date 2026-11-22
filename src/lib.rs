/// Logs a `tracing::warn!` the first time this call site runs, then never again in the
/// process. For config problems that would otherwise repeat on every render.
#[macro_export]
macro_rules! warn_once {
    ($($arg:tt)+) => {{
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::warn!($($arg)+);
        }
    }};
}

pub mod account;
pub mod ansi;
pub mod cache;
pub mod config;
pub mod context;
pub mod explain;
pub mod format;
pub mod modules;
pub mod passthrough;
pub mod platform;
pub mod renderer;
pub mod terminal;
pub mod uninstall;
pub mod usage_limits;
pub mod usage_limits_state;
