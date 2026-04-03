//! QBopomofo Windows Text Service (TSF)
//!
//! This crate implements a Windows Text Services Framework (TSF) input method.
//! When compiled as a DLL on Windows, it registers as a COM server and provides
//! Chinese Bopomofo input via the chewing engine.
//!
//! ## Architecture
//!
//! - `text_service.rs` — Main TSF text service (ITfTextInputProcessorEx, ITfKeyEventSink)
//! - `key_event.rs` — Windows VK → chewing KeyboardEvent mapping + ToUnicode
//! - `com.rs` — COM DLL exports, class factory, and TSF registration
//!
//! The engine is linked directly as a Rust crate (zero FFI overhead).

#[macro_use]
pub mod debug_log;
pub mod candidate_window;
pub mod com;
pub mod edit_session;
pub mod key_event;
pub mod preferences;
pub mod text_service;
