//! C API for ComposingSession — mixed Chinese/English composing.
//!
//! These functions manage the composing session that sits on top of
//! the chewing context, handling Shift SmartToggle and mixed-language input.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use chewing::composing_session::ComposingSession;
use chewing::typing_mode::{ModePreferences, ShiftBehavior};

/// Opaque handle for a ComposingSession.
pub struct QBComposingSession {
    inner: ComposingSession,
    prefs: ModePreferences,
}

/// Create a new ComposingSession with default Q注音 preferences.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_new() -> *mut QBComposingSession {
    let session = Box::new(QBComposingSession {
        inner: ComposingSession::new(),
        prefs: ModePreferences::default(),
    });
    Box::into_raw(session)
}

/// Delete a ComposingSession.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_delete(session: *mut QBComposingSession) {
    if !session.is_null() {
        unsafe { drop(Box::from_raw(session)) };
    }
}

/// Check if currently in English mode.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_is_english(session: *const QBComposingSession) -> i32 {
    if session.is_null() { return 0; }
    let s = unsafe { &*session };
    if s.inner.is_english_mode() { 1 } else { 0 }
}

/// Check if there is mixed content (segments recorded).
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_has_mixed_content(session: *const QBComposingSession) -> i32 {
    if session.is_null() { return 0; }
    let s = unsafe { &*session };
    if s.inner.has_mixed_content() { 1 } else { 0 }
}

/// Handle Shift key press/release.
/// `is_down`: 1 = pressed, 0 = released.
/// `chinese_buffer`: current chewing buffer content (UTF-8).
/// Returns 1 if mode changed, 0 if not.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_handle_shift(
    session: *mut QBComposingSession,
    is_down: i32,
    chinese_buffer: *const c_char,
) -> i32 {
    if session.is_null() { return 0; }
    let s = unsafe { &mut *session };
    let buf = if chinese_buffer.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(chinese_buffer) }.to_str().unwrap_or("")
    };
    let changed = s.inner.handle_shift(is_down != 0, &s.prefs, buf);
    if changed { 1 } else { 0 }
}

/// Check if Shift is currently held down.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_is_shift_held(session: *const QBComposingSession) -> i32 {
    if session.is_null() { return 0; }
    let s = unsafe { &*session };
    if s.inner.is_shift_held() { 1 } else { 0 }
}

/// Type an English character into the composing buffer.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_type_english(session: *mut QBComposingSession, ch: u8) {
    if session.is_null() { return; }
    let s = unsafe { &mut *session };
    s.inner.type_english(ch as char);
}

/// Delete the last English character. Returns 1 if deleted, 0 if empty.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_backspace_english(session: *mut QBComposingSession) -> i32 {
    if session.is_null() { return 0; }
    let s = unsafe { &mut *session };
    if s.inner.backspace_english() { 1 } else { 0 }
}

/// Get the English buffer content. Caller must free with chewing_free().
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_english_buffer(session: *const QBComposingSession) -> *mut c_char {
    if session.is_null() { return std::ptr::null_mut(); }
    let s = unsafe { &*session };
    match CString::new(s.inner.english_buffer()) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Build the full display string from segments + current buffers.
/// `chinese_buffer`: current chewing buffer (UTF-8).
/// `bopomofo`: current bopomofo reading (UTF-8).
/// Returns a UTF-8 string. Caller must free with chewing_free().
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_build_display(
    session: *const QBComposingSession,
    chinese_buffer: *const c_char,
    bopomofo: *const c_char,
) -> *mut c_char {
    if session.is_null() { return std::ptr::null_mut(); }
    let s = unsafe { &*session };
    let chinese = if chinese_buffer.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(chinese_buffer) }.to_str().unwrap_or("")
    };
    let bopo = if bopomofo.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(bopomofo) }.to_str().unwrap_or("")
    };
    let display = s.inner.build_display(chinese, bopo);
    match CString::new(display) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Commit all content in correct order. Returns the committed string.
/// `final_chinese`: committed text from chewing_handle_Enter (UTF-8).
/// Caller must free with chewing_free().
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_commit_all(
    session: *mut QBComposingSession,
    final_chinese: *const c_char,
) -> *mut c_char {
    if session.is_null() { return std::ptr::null_mut(); }
    let s = unsafe { &mut *session };
    let chinese = if final_chinese.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(final_chinese) }.to_str().unwrap_or("")
    };
    let result = s.inner.commit_all(chinese);
    match CString::new(result) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Clear all composing state (Esc/reset).
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_clear(session: *mut QBComposingSession) {
    if session.is_null() { return; }
    let s = unsafe { &mut *session };
    s.inner.clear();
}

/// Set Shift behavior. 0=None, 1=SmartToggle, 2=ToggleOnly.
#[unsafe(no_mangle)]
pub extern "C" fn qb_composing_set_shift_behavior(session: *mut QBComposingSession, behavior: i32) {
    if session.is_null() { return; }
    let s = unsafe { &mut *session };
    s.prefs.shift_behavior = match behavior {
        0 => ShiftBehavior::None,
        1 => ShiftBehavior::SmartToggle,
        2 => ShiftBehavior::ToggleOnly,
        _ => ShiftBehavior::SmartToggle,
    };
}
