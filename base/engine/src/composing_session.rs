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

    /// Mark that a key was typed while Shift was held (prevents toggle on release).
    pub fn mark_shift_used(&mut self) {
        if self.shift_held {
            self.shift_typed_while_held = true;
        }
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

        // First English char after Chinese — snapshot only NEW Chinese content
        if has_chinese && self.english_buffer.is_empty() {
            let already: String = self
                .segments
                .iter()
                .filter_map(|s| {
                    if let Segment::Chinese(t) = s {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if chinese_buffer.starts_with(&already) {
                let delta = &chinese_buffer[already.len()..];
                if !delta.is_empty() {
                    self.segments.push(Segment::Chinese(delta.to_string()));
                }
            } else if already.is_empty() {
                self.segments.push(Segment::Chinese(chinese_buffer.to_string()));
            }
        }

        // Add to mixed buffer
        self.english_buffer.push(ch);
        false // handled internally, don't direct commit
    }

    /// Delete the last English character from the current buffer or from
    /// the last English segment. Returns true if something was deleted.
    ///
    /// When the last English segment is fully deleted, also removes
    /// the preceding Chinese snapshot segment (since the chewing engine
    /// buffer is the source of truth for Chinese text).
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
                // Also remove the preceding Chinese snapshot — the chewing
                // engine buffer is the source of truth now
                while let Some(Segment::Chinese(_)) = self.segments.last() {
                    self.segments.pop();
                }
            }
            return true;
        }
        false
    }

    // MARK: - Cursor-aware editing

    /// Compute the "remaining Chinese" portion of the display (chewing buffer minus snapshots).
    fn remaining_chinese<'a>(&self, chinese_buffer: &'a str) -> &'a str {
        let already: String = self
            .segments
            .iter()
            .filter_map(|s| {
                if let Segment::Chinese(t) = s {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        if chinese_buffer.starts_with(&already) {
            &chinese_buffer[already.len()..]
        } else if !chinese_buffer.is_empty() {
            chinese_buffer
        } else {
            ""
        }
    }

    /// Query the region type at a given display cursor position.
    ///
    /// Returns: 0=Segment(Chinese), 1=Segment(English), 2=RemainingChinese,
    ///          3=Bopomofo, 4=EnglishBuffer, -1=at/past end
    pub fn cursor_region(&self, pos: usize, chinese_buffer: &str, bopomofo: &str) -> i32 {
        match self.map_display_position(pos, chinese_buffer, bopomofo) {
            Some((kind, _, _)) => kind as i32,
            None => -1,
        }
    }

    /// Convert a display cursor position to the corresponding chewing engine cursor position.
    ///
    /// For Chinese regions (Segment::Chinese or RemainingChinese), returns the character
    /// offset within the chewing buffer. For non-Chinese regions, returns -1.
    pub fn display_to_chewing_cursor(&self, pos: usize, chinese_buffer: &str, bopomofo: &str) -> i32 {
        let mut offset = 0;
        let mut chinese_chars_before = 0usize;

        for seg in &self.segments {
            let len = match seg {
                Segment::Chinese(t) => {
                    let char_count = t.chars().count();
                    if pos < offset + char_count {
                        // Inside this Chinese segment
                        return (chinese_chars_before + (pos - offset)) as i32;
                    }
                    chinese_chars_before += char_count;
                    char_count
                }
                Segment::English(t) => {
                    let char_count = t.chars().count();
                    if pos < offset + char_count {
                        return -1; // In English region
                    }
                    char_count
                }
            };
            offset += len;
        }

        // Remaining Chinese
        let remaining = self.remaining_chinese(chinese_buffer);
        let rem_len = remaining.chars().count();
        if pos < offset + rem_len {
            return (chinese_chars_before + (pos - offset)) as i32;
        }
        offset += rem_len;
        chinese_chars_before += rem_len;

        // Bopomofo — cursor here means at end of Chinese
        let bopo_len = bopomofo.chars().count();
        if pos < offset + bopo_len {
            return chinese_chars_before as i32;
        }

        // At or past end — return total Chinese chars (cursor at end)
        chinese_chars_before as i32
    }

    /// Map a display character position to a region in the underlying data structure.
    /// Returns (region_type, segment_index_or_MAX, char_offset_within_region).
    ///
    /// region_type: 0=Segment(Chinese), 1=Segment(English), 2=RemainingChinese, 3=Bopomofo, 4=EnglishBuffer
    fn map_display_position(
        &self,
        pos: usize,
        chinese_buffer: &str,
        bopomofo: &str,
    ) -> Option<(u8, usize, usize)> {
        let mut offset = 0;

        for (i, seg) in self.segments.iter().enumerate() {
            let (len, kind) = match seg {
                Segment::Chinese(t) => (t.chars().count(), 0u8),
                Segment::English(t) => (t.chars().count(), 1u8),
            };
            if pos < offset + len {
                return Some((kind, i, pos - offset));
            }
            offset += len;
        }

        // Remaining Chinese
        let remaining = self.remaining_chinese(chinese_buffer);
        let rem_len = remaining.chars().count();
        if pos < offset + rem_len {
            return Some((2, usize::MAX, pos - offset));
        }
        offset += rem_len;

        // Bopomofo
        let bopo_len = bopomofo.chars().count();
        if pos < offset + bopo_len {
            return Some((3, usize::MAX, pos - offset));
        }
        offset += bopo_len;

        // English buffer
        let eng_len = self.english_buffer.chars().count();
        if pos < offset + eng_len {
            return Some((4, usize::MAX, pos - offset));
        }

        None // at or past end
    }

    /// Insert an English character at the given display cursor position.
    /// Returns true if handled.
    pub fn insert_english_at(
        &mut self,
        ch: char,
        cursor: usize,
        chinese_buffer: &str,
        bopomofo: &str,
    ) -> bool {
        match self.map_display_position(cursor, chinese_buffer, bopomofo) {
            Some((1, seg_idx, char_offset)) => {
                // English segment
                if let Segment::English(ref mut text) = self.segments[seg_idx] {
                    let byte_pos = text
                        .char_indices()
                        .nth(char_offset)
                        .map(|(i, _)| i)
                        .unwrap_or(text.len());
                    text.insert(byte_pos, ch);
                }
                true
            }
            Some((0, seg_idx, char_offset)) => {
                let Segment::Chinese(text) = &self.segments[seg_idx] else {
                    return false;
                };
                let (prefix, suffix) = Self::split_at_char(text, char_offset);
                let mut replacement = Vec::with_capacity(3);
                if !prefix.is_empty() {
                    replacement.push(Segment::Chinese(prefix));
                }
                replacement.push(Segment::English(ch.to_string()));
                if !suffix.is_empty() {
                    replacement.push(Segment::Chinese(suffix));
                }
                self.segments.splice(seg_idx..=seg_idx, replacement);
                true
            }
            Some((2, _, char_offset)) => {
                let prefix: String = self
                    .remaining_chinese(chinese_buffer)
                    .chars()
                    .take(char_offset)
                    .collect();
                if !prefix.is_empty() {
                    self.segments.push(Segment::Chinese(prefix));
                }
                self.segments.push(Segment::English(ch.to_string()));
                true
            }
            Some((3, _, _)) => {
                let remaining = self.remaining_chinese(chinese_buffer);
                if !remaining.is_empty() {
                    self.segments.push(Segment::Chinese(remaining.to_string()));
                }
                self.segments.push(Segment::English(ch.to_string()));
                true
            }
            Some((4, _, char_offset)) => {
                // English buffer
                let byte_pos = self
                    .english_buffer
                    .char_indices()
                    .nth(char_offset)
                    .map(|(i, _)| i)
                    .unwrap_or(self.english_buffer.len());
                self.english_buffer.insert(byte_pos, ch);
                true
            }
            None => {
                let remaining = self.remaining_chinese(chinese_buffer);
                if !remaining.is_empty() || !bopomofo.is_empty() {
                    if !remaining.is_empty() {
                        self.segments.push(Segment::Chinese(remaining.to_string()));
                    }
                    self.segments.push(Segment::English(ch.to_string()));
                } else if !self.segments.is_empty() {
                    // Preserve insertion order after existing mixed segments.
                    if let Some(Segment::English(text)) = self.segments.last_mut() {
                        text.push(ch);
                    } else {
                        self.segments.push(Segment::English(ch.to_string()));
                    }
                } else {
                    // At end of an existing English buffer — append in place.
                    self.english_buffer.push(ch);
                }
                true
            }
            _ => false, // Chinese or Bopomofo region
        }
    }

    /// Delete the character before the given display cursor position.
    /// Returns: 0 = nothing to delete, 1 = English char deleted, 2 = Chinese region (delegate to chewing).
    pub fn delete_at(&mut self, cursor: usize, chinese_buffer: &str, bopomofo: &str) -> i32 {
        if cursor == 0 {
            return 0;
        }
        // Look at character BEFORE cursor
        match self.map_display_position(cursor - 1, chinese_buffer, bopomofo) {
            Some((1, seg_idx, char_offset)) => {
                // English segment
                if let Segment::English(ref mut text) = self.segments[seg_idx] {
                    if let Some((bp, _)) = text.char_indices().nth(char_offset) {
                        text.remove(bp);
                    }
                    if text.is_empty() {
                        self.segments.remove(seg_idx);
                        // Remove preceding Chinese snapshot (chewing buffer is source of truth)
                        if seg_idx > 0 {
                            if matches!(self.segments.get(seg_idx - 1), Some(Segment::Chinese(_))) {
                                self.segments.remove(seg_idx - 1);
                            }
                        }
                    }
                }
                1
            }
            Some((4, _, char_offset)) => {
                // English buffer
                if let Some((bp, _)) = self.english_buffer.char_indices().nth(char_offset) {
                    self.english_buffer.remove(bp);
                }
                1
            }
            Some((0, _, _)) | Some((2, _, _)) | Some((3, _, _)) => 2, // Chinese or Bopomofo
            _ => 0,
        }
    }

    // MARK: - Resync

    /// Re-synchronize Chinese segments after the chewing buffer has been modified
    /// (e.g., by candidate selection). Each Chinese segment maps to a consecutive
    /// slice of the chewing buffer; this method re-reads those slices from the
    /// updated buffer so snapshots stay in sync.
    pub fn resync_chinese(&mut self, new_chinese_buffer: &str) {
        let mut chars_consumed = 0;
        for seg in &mut self.segments {
            if let Segment::Chinese(text) = seg {
                let old_char_count = text.chars().count();
                let new_text: String = new_chinese_buffer
                    .chars()
                    .skip(chars_consumed)
                    .take(old_char_count)
                    .collect();
                *text = new_text;
                chars_consumed += old_char_count;
            }
        }
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

        // Reset segments/buffers but preserve mode
        let was_english = self.is_english;
        self.clear();
        self.is_english = was_english;

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
            // Switching FROM Chinese → English: snapshot only NEW Chinese content
            if !chinese_buffer.is_empty() {
                let already: String = self
                    .segments
                    .iter()
                    .filter_map(|s| {
                        if let Segment::Chinese(t) = s {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                if chinese_buffer.starts_with(&already) {
                    let delta = &chinese_buffer[already.len()..];
                    if !delta.is_empty() {
                        self.segments.push(Segment::Chinese(delta.to_string()));
                    }
                } else if already.is_empty() {
                    self.segments.push(Segment::Chinese(chinese_buffer.to_string()));
                }
                self.chinese_snapshot = chinese_buffer.to_string();
            }
        }
    }

    fn split_at_char(text: &str, char_offset: usize) -> (String, String) {
        let byte_pos = text
            .char_indices()
            .nth(char_offset)
            .map(|(idx, _)| idx)
            .unwrap_or(text.len());
        (text[..byte_pos].to_string(), text[byte_pos..].to_string())
    }
}

impl Default for ComposingSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ComposingSession;

    #[test]
    fn insert_english_at_splits_remaining_chinese() {
        let mut session = ComposingSession::new();

        assert!(session.insert_english_at('3', 2, "你好世界", ""));
        assert_eq!(session.build_display("你好世界", ""), "你好3世界");
        assert_eq!(session.display_to_chewing_cursor(3, "你好世界", ""), 2);
        assert_eq!(session.commit_all("你好世界"), "你好3世界");
    }

    #[test]
    fn insert_english_at_splits_snapshotted_chinese_segment() {
        let mut session = ComposingSession::new();

        assert!(session.insert_english_at('3', 2, "你好世界", ""));
        assert!(session.insert_english_at('4', 1, "你好世界", ""));

        assert_eq!(session.build_display("你好世界", ""), "你4好3世界");
        assert_eq!(session.display_to_chewing_cursor(2, "你好世界", ""), 1);
        assert_eq!(session.commit_all("你好世界"), "你4好3世界");
    }

    #[test]
    fn insert_english_at_end_of_chinese_keeps_future_chinese_after_it() {
        let mut session = ComposingSession::new();

        assert!(session.insert_english_at('3', 2, "甲乙", ""));

        assert_eq!(session.build_display("甲乙", ""), "甲乙3");
        assert_eq!(session.display_to_chewing_cursor(3, "甲乙", ""), 2);
        assert_eq!(session.build_display("甲乙丙丁", ""), "甲乙3丙丁");
        assert_eq!(session.commit_all("甲乙丙丁"), "甲乙3丙丁");
    }

    #[test]
    fn insert_english_after_space_keeps_future_chinese_after_digit() {
        let mut session = ComposingSession::new();

        assert!(session.insert_english_at(' ', 2, "甲乙", ""));
        assert!(session.insert_english_at('3', 3, "甲乙", ""));

        assert_eq!(session.build_display("甲乙", ""), "甲乙 3");
        assert_eq!(session.build_display("甲乙丙丁", ""), "甲乙 3丙丁");
        assert_eq!(session.commit_all("甲乙丙丁"), "甲乙 3丙丁");
    }

    #[test]
    fn insert_english_at_before_bopomofo_keeps_cursor_after_inserted_text() {
        let mut session = ComposingSession::new();

        assert!(session.insert_english_at('3', 2, "你好", "ㄅ"));

        assert_eq!(session.build_display("你好", "ㄅ"), "你好3ㄅ");
        assert_eq!(session.display_to_chewing_cursor(3, "你好", "ㄅ"), 2);
    }

    /// Reproduces: when the chewing engine re-segments the buffer (e.g. 是→事變),
    /// the Chinese snapshot becomes stale and causes duplicated output on commit.
    /// Real case: "但我發現他現在是" + "alt" + engine changes to "但我發現他現在事變" + "c"
    /// → commitAll produced "但我發現他現在是alt但我發現他現在事變c但我發現他現在事變"
    /// Expected: "但我發現他現在事變altc" (or properly interleaved)
    #[test]
    fn commit_no_duplicate_after_chinese_resegmentation() {
        let mut session = ComposingSession::new();

        // Step 1: Chinese buffer = "甲乙丙", insert English "x" at end (pos 3)
        assert!(session.insert_english_at('x', 3, "甲乙丙", ""));
        // segments: [Chinese("甲乙丙"), English("x")]
        assert_eq!(session.build_display("甲乙丙", ""), "甲乙丙x");

        // Step 2: User types more zhuyin, engine re-segments: "甲乙丙" → "甲乙丁"
        // (the engine changed 丙→丁 during re-evaluation — simulates 是→事)
        // Then user types more zhuyin and buffer grows to "甲乙丁戊"

        // Step 3: Insert English "y" at end of new display
        // display should be: "甲乙丙x丁戊" (old snapshot + english + remaining)
        // but remaining_chinese("甲乙丁戊") fails starts_with("甲乙丙") → returns full buffer
        // This is where the bug manifests — the full buffer gets re-added as a new segment

        // Simulate: resync should fix the stale snapshot
        session.resync_chinese("甲乙丁戊");
        assert_eq!(session.build_display("甲乙丁戊", ""), "甲乙丁x戊");

        // Now commit should not duplicate
        assert_eq!(session.commit_all("甲乙丁戊"), "甲乙丁x戊");
    }

    /// Same scenario but WITHOUT resync — demonstrates the bug
    #[test]
    fn commit_duplicates_without_resync_after_resegmentation() {
        let mut session = ComposingSession::new();

        // Chinese buffer = "甲乙丙", insert English "x" at end
        assert!(session.insert_english_at('x', 3, "甲乙丙", ""));
        assert_eq!(session.build_display("甲乙丙", ""), "甲乙丙x");

        // Engine re-segments: buffer changes to "甲乙丁戊"
        // WITHOUT calling resync_chinese, the snapshot is stale
        // build_display should still work reasonably, but it doesn't:
        let display = session.build_display("甲乙丁戊", "");
        // BUG: this produces "甲乙丙x甲乙丁戊" because remaining_chinese
        // can't match the stale snapshot prefix and returns the full buffer
        assert_eq!(display, "甲乙丙x甲乙丁戊", "Known bug: stale snapshot causes full buffer duplication in display");

        // And commit also duplicates:
        let committed = session.commit_all("甲乙丁戊");
        // BUG: "甲乙丙x甲乙丁戊" — Chinese text duplicated
        assert_eq!(committed, "甲乙丙x甲乙丁戊", "Known bug: stale snapshot causes full buffer duplication in commit");
    }
}
