//! Panic-safe wrappers for COM methods.
//!
//! Every `extern "system"` COM callback MUST NOT let a Rust panic cross the
//! FFI boundary — doing so is UB and takes the entire host process down.
//! Each COM method body should be wrapped with one of the macros here so
//! panics are caught, logged to `%TEMP%\qbopomofo_crash.log`, and converted
//! to a COM error return value.

use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// Log a caught panic with context so we can diagnose later.
pub fn log_panic(where_: &str, info: &dyn std::fmt::Display) {
    let path: PathBuf = std::env::temp_dir().join("qbopomofo_crash.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] PANIC in {}: {}", timestamp(), where_, info);
    }
    // Also route through qb_dbg! so live debug logs see it too.
    crate::qb_dbg!("PANIC in {}: {}", where_, info);
}

fn timestamp() -> String {
    use std::time::SystemTime;
    let e = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", e.as_secs(), e.subsec_millis())
}

/// Run a closure, catching any panic. Panics are logged and replaced with
/// the fallback value.
pub fn guard<R, F: FnOnce() -> R>(where_: &'static str, fallback: R, f: F) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let msg = panic_message(&*payload);
            log_panic(where_, &msg);
            fallback
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Wrap a COM method body that returns `windows::core::Result<T>`.
///
/// On panic, writes the crash log and returns `Err(E_FAIL)`.
#[macro_export]
macro_rules! com_method {
    ($where:expr, $body:block) => {{
        $crate::panic_guard::guard(
            $where,
            Err(windows::core::Error::from(windows::Win32::Foundation::E_FAIL)),
            || $body,
        )
    }};
}

/// Wrap a COM method body that returns `windows::core::Result<BOOL>`, where
/// we want BOOL(0) (= not handled / pass-through) on panic.
#[macro_export]
macro_rules! com_method_bool {
    ($where:expr, $body:block) => {{
        $crate::panic_guard::guard(
            $where,
            Ok(windows::core::BOOL(0)),
            || $body,
        )
    }};
}

/// Wrap an `extern "system"` function returning `HRESULT`.
#[macro_export]
macro_rules! com_method_hresult {
    ($where:expr, $body:block) => {{
        $crate::panic_guard::guard(
            $where,
            windows::Win32::Foundation::E_FAIL,
            || $body,
        )
    }};
}

/// Wrap an `extern "system"` function returning `BOOL` (e.g. DllMain).
#[macro_export]
macro_rules! com_method_win_bool {
    ($where:expr, $body:block) => {{
        $crate::panic_guard::guard(
            $where,
            windows::core::BOOL(0),
            || $body,
        )
    }};
}

/// Wrap a body that returns `()`. Used for COM methods returning `Result<()>`.
#[macro_export]
macro_rules! com_method_unit {
    ($where:expr, $body:block) => {{
        $crate::panic_guard::guard(
            $where,
            Ok(()),
            || $body,
        )
    }};
}
