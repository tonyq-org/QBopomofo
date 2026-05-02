//! QBopomofo Windows Text Service (TSF)
//!
//! This crate implements a Windows Text Services Framework (TSF) input method.
//! When compiled as a DLL on Windows, it registers as a COM server and provides
//! Chinese Bopomofo input via the chewing engine.
//!
//! ## Architecture
//!
//! - `controller.rs` — Platform-agnostic input logic (engine + session + UI state)
//! - `text_service.rs` — TSF COM wrapper + `TsfSink` bridging `Controller` ↔ TSF
//! - `key_event.rs` — Windows VK → chewing KeyboardEvent mapping + ToUnicode
//! - `candidate_window.rs` — GDI candidate popup
//! - `edit_session.rs` — TSF edit session helpers
//! - `panic_guard.rs` — `com_method_*!` macros for FFI-safe panic boundaries
//! - `com.rs` — COM DLL exports, class factory, and TSF registration
//!
//! The engine is linked directly as a Rust crate (zero FFI overhead).

#[macro_use]
pub mod debug_log;
pub mod candidate_window;
pub mod com;
pub mod controller;
pub mod edit_session;
pub mod key_event;
pub mod panic_guard;
pub mod preferences;
pub mod text_service;
