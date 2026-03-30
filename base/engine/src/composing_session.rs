//! ComposingSession — mixed Chinese/English composing with segment tracking.
//!
//! This module manages the composing buffer that sits on top of the chewing
//! Editor. It handles:
//! - Chinese/English mode switching (Shift SmartToggle)
//! - Mixed Chinese/English text in the composing area
//! - Segment-ordered commit (preserving insertion order)
//!
//! Shared between macOS (via C API) and Windows (direct Rust).
//! Zero allocation in the hot path — segments use pre-allocated Vec.

use crate::typing_mode::{ModePreferences, ShiftBehavior};

/// A segment of text in the composing buffer, preserving insertion order.
#[derive(Debug, Clone)]
pub enum Segment {
    /// Chinese text (snapshot of chewing buffer at time of mode switch)
    Chinese(String),
    /// English text typed inline via Shift
    English(String),
}

/// Manages mixed Chinese/English composing state.
///
/// This sits between the platform layer and the chewing Editor,
/// tracking mode switches and text segments to enable mixed-language
/// composing with correct commit ordering.
#[derive(Debug)]
pub struct ComposingSession {
    /// Whether currently in English input mode
    is_english: bool,
    /// Inline English text being typed (current, not yet recorded as segment)
    english_buffer: String,
    /// Recorded segments in insertion order
    segments: Vec<Segment>,
    /// Chinese buffer snapshot when last switched to English
    chinese_snapshot: String,
    /// Shift key state for SmartToggle
    shift_held: bool,
    /// Whether any key was typed while Shift was held
    shift_typed_while_held: bool,
    /// Whether we were in Chinese mode before Shift was pressed
    was_chinese_before_shift: bool,
}

impl ComposingSession {
    pub fn new() -> Self {
        Self {
            is_english: false,
            english_buffer: String::new(),
            segments: Vec::with_capacity(8),
            chinese_snapshot: String::new(),
            shift_held: false,
            shift_typed_while_held: false,
            was_chinese_before_shift: true,
        }
    }

    // MARK: - State queries

    pub fn is_english_mode(&self) -> bool {
        self.is_english
    }

    pub fn english_buffer(&self) -> &str {
        &self.english_buffer
    }

    pub fn has_mixed_content(&self) -> bool {
        !self.segments.is_empty() || !self.english_buffer.is_empty()
    }

    // MARK: - Shift handling

    /// Handle Shift key press/release. Returns true if mode changed.
    pub fn handle_shift(&mut self, is_down: bool, prefs: &ModePreferences, chinese_buffer: &str) -> bool {
        match prefs.shift_behavior {
            ShiftBehavior::None => false,
            ShiftBehavior::SmartToggle => {
                if is_down {
                    self.shift_held = true;
                    self.shift_typed_while_held = false;
                    self.was_chinese_before_shift = !self.is_english;
                    false
                } else {
                    let changed;
                    if self.shift_held && !self.shift_typed_while_held {
                        // Short press — toggle
                        let was_english = self.is_english;
                        self.is_english = !self.is_english;
                        self.record_mode_switch(was_english, chinese_buffer);
                        changed = true;
                    } else if self.shift_held && self.shift_typed_while_held && self.was_chinese_before_shift {
                        // Hold released — back to Chinese
                        self.record_mode_switch(true, chinese_buffer);
                        self.is_english = false;
                        changed = true;
                    } else {
                        changed = false;
                    }
                    self.shift_held = false;
                    self.shift_typed_while_held = false;
                    changed
                }
            }
            ShiftBehavior::ToggleOnly => {
                if !is_down {
                    let was_english = self.is_english;
                    self.is_english = !self.is_english;
                    self.record_mode_switch(was_english, chinese_buffer);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Check if Shift is currently held (for SmartToggle temporary English).
    pub fn is_shift_held(&self) -> bool {
        self.shift_held
    }

    // MARK: - English input

    /// Type an English character.
    ///
    /// `chinese_buffer` is the current chewing composing buffer content.
    /// If empty and no segments exist, returns `true` to indicate the caller
    /// should directly commit this character (no Chinese context).
    pub fn type_english(&mut self, ch: char, chinese_buffer: &str) -> bool {
        if self.shift_held {
            self.shift_typed_while_held = true;
            self.is_english = true;
        }

        let has_chinese = !chinese_buffer.is_empty();

        // No Chinese composing — direct commit, skip segment tracking
        if !has_chinese && self.segments.is_empty() {
            return true; // caller should commit directly
        }

        // First English char after Chinese — snapshot Chinese buffer first
        if has_chinese && self.english_buffer.is_empty() {
            // Check if Chinese was already snapshotted
            let already_snapshotted: String = self.segments.iter()
                .filter_map(|s| if let Segment::Chinese(t) = s { Some(t.as_str()) } else { None })
                .collect();
            if chinese_buffer != already_snapshotted {
                self.segments.push(Segment::Chinese(chinese_buffer.to_string()));
            }
        }

        // Add to mixed buffer
        self.english_buffer.push(ch);
        false // handled internally, don't direct commit
    }

    /// Delete the last English character from the current buffer or from
    /// the last English segment. Returns true if something was deleted.
    pub fn backspace_english(&mut self) -> bool {
        // First try the current inline buffer
        if self.english_buffer.pop().is_some() {
            return true;
        }
        // Then try the last segment if it's English
        if let Some(Segment::English(text)) = self.segments.last_mut() {
            text.pop();
            if text.is_empty() {
                self.segments.pop();
            }
            return true;
        }
        false
    }

    // MARK: - Commit

    /// Build the full display string from segments + current buffers.
    ///
    /// `current_chinese` is the current chewing buffer content.
    /// `current_bopomofo` is the current bopomofo reading.
    pub fn build_display(&self, current_chinese: &str, current_bopomofo: &str) -> String {
        let mut display = String::new();

        // Replay recorded segments
        let mut already_snapshotted = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Chinese(text) => {
                    display.push_str(text);
                    already_snapshotted.push_str(text);
                }
                Segment::English(text) => {
                    display.push_str(text);
                }
            }
        }

        // Append new Chinese (buffer minus already-snapshotted)
        if current_chinese.starts_with(&already_snapshotted) {
            let remaining = &current_chinese[already_snapshotted.len()..];
            display.push_str(remaining);
        } else if !current_chinese.is_empty() {
            display.push_str(current_chinese);
        }

        display.push_str(current_bopomofo);
        display.push_str(&self.english_buffer);
        display
    }

    /// Commit all content in correct order. Returns the full committed string.
    ///
    /// `final_chinese` is the committed text from chewing_handle_Enter().
    pub fn commit_all(&mut self, final_chinese: &str) -> String {
        let mut result = String::new();

        // Replay segments
        let mut already_captured = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Chinese(text) => {
                    result.push_str(text);
                    already_captured.push_str(text);
                }
                Segment::English(text) => {
                    result.push_str(text);
                }
            }
        }

        // Remaining Chinese
        if final_chinese.starts_with(&already_captured) {
            let remaining = &final_chinese[already_captured.len()..];
            result.push_str(remaining);
        } else if !final_chinese.is_empty() {
            result.push_str(final_chinese);
        }

        // Remaining English
        result.push_str(&self.english_buffer);

        // Reset
        self.clear();

        result
    }

    /// Clear all state (Esc or reset).
    pub fn clear(&mut self) {
        self.is_english = false;
        self.english_buffer.clear();
        self.segments.clear();
        self.chinese_snapshot.clear();
        self.shift_held = false;
        self.shift_typed_while_held = false;
    }

    // MARK: - Internal

    fn record_mode_switch(&mut self, from_english: bool, chinese_buffer: &str) {
        if from_english {
            // Switching FROM English → Chinese: save English segment
            if !self.english_buffer.is_empty() {
                self.segments.push(Segment::English(self.english_buffer.clone()));
                self.english_buffer.clear();
            }
        } else {
            // Switching FROM Chinese → English: snapshot Chinese buffer
            if !chinese_buffer.is_empty() {
                self.segments.push(Segment::Chinese(chinese_buffer.to_string()));
                self.chinese_snapshot = chinese_buffer.to_string();
            }
        }
    }
}

impl Default for ComposingSession {
    fn default() -> Self {
        Self::new()
    }
}
