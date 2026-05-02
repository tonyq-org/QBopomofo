//! Debug logging for QBopomofo Windows TSF.
//!
//! Enabled by setting the environment variable `QBOPOMOFO_DEBUG=1`.
//! When enabled, writes to `%TEMP%\qbopomofo.log`.
//! When disabled, all logging is a no-op with zero overhead.

use std::io::Write;
use std::sync::OnceLock;

static DEBUG_ENABLED: OnceLock<bool> = OnceLock::new();

fn is_debug() -> bool {
    *DEBUG_ENABLED.get_or_init(|| std::env::var("QBOPOMOFO_DEBUG").is_ok())
}

/// Write a debug log line. No-op if QBOPOMOFO_DEBUG is not set.
pub fn dbg_log(msg: &str) {
    if !is_debug() {
        return;
    }

    let path = std::env::temp_dir().join("qbopomofo.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

/// Convenience macro for formatted debug logging.
#[macro_export]
macro_rules! qb_dbg {
    ($($arg:tt)*) => {
        $crate::debug_log::dbg_log(&format!($($arg)*))
    };
}

fn timestamp() -> String {
    // Simple timestamp using SystemTime
    use std::time::SystemTime;
    let elapsed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
    let millis = elapsed.subsec_millis();
    format!("{}.{:03}", secs, millis)
}
