//! Platform-agnostic controller for QBopomofo.
//!
//! The controller owns the chewing engine state, composing session, user
//! preferences, and candidate/space-cycle bookkeeping. It does NOT touch TSF
//! or any UI — instead it emits events through the [`InputSink`] trait, which
//! platform-specific adapters implement (see `text_service::TsfSink` for the
//! real TSF binding and `bin/dev_harness.rs` for a stdout mock).
//!
//! This separation lets us exercise the full input path without COM / TSF,
//! which is essential for iterating on Windows without Sandbox.

use chewing::composing_session::ComposingSession;
use chewing::dictionary::DEFAULT_DICT_NAMES;
use chewing::editor::{BasicEditor, Editor, EditorKeyBehavior};
use chewing::input::{keycode, KeyboardEvent};
use chewing::typing_mode::{CapsLockBehavior, ModePreferences, ShiftBehavior};

use crate::key_event::vkey_to_keyboard_event;
use crate::preferences::Preferences;

// Virtual key codes — duplicated from text_service so controller has no
// platform-specific dependency on the windows crate.
pub const VK_BACK: u32 = 0x08;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;

/// Events the controller emits — the platform adapter renders them.
///
/// Kept intentionally coarse: preedit / commit / candidates / hide.
/// A `caret_hint` is passed along with preedit updates so the adapter can
/// remember where to anchor the candidate window when it later appears.
pub trait InputSink {
    /// Begin or update a preedit (composition) with the given display text.
    /// The implementation should decide whether to start a fresh composition
    /// or replace the current one. Returns the caret screen position if
    /// known, so the controller can remember where to pop candidates.
    fn update_preedit(&self, text: &str) -> Option<(i32, i32)>;

    /// Commit the given text to the underlying document and end any preedit.
    fn commit_text(&self, text: &str);

    /// End the current preedit without committing anything.
    fn end_preedit(&self);

    /// Show the candidate window.
    fn show_candidates(
        &self,
        cands: &[String],
        selection_keys: &[char],
        highlight: usize,
        page_info: &str,
        caret_pos: Option<(i32, i32)>,
    );

    /// Hide the candidate window.
    fn hide_candidates(&self);
}

pub struct Controller {
    editor: Option<Editor>,
    session: ComposingSession,
    prefs: ModePreferences,
    selection_keys: Vec<char>,

    // Space cycle state
    space_cycle_max: u32,
    space_cycle_remaining: u32,
    space_cycle_targets: Vec<String>,
    space_cycle_step: usize,
    #[allow(dead_code)]
    space_cycle_saved_cursor: usize,

    // Candidate UI bookkeeping (controller tracks highlight; sink renders)
    candidate_highlight: usize,
    last_caret_pos: Option<(i32, i32)>,

    activated: bool,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            editor: None,
            session: ComposingSession::new(),
            prefs: ModePreferences {
                shift_behavior: ShiftBehavior::SmartToggle,
                ..ModePreferences::default()
            },
            selection_keys: "1234567890".chars().collect(),
            space_cycle_max: 0,
            space_cycle_remaining: 0,
            space_cycle_targets: Vec::new(),
            space_cycle_step: 0,
            space_cycle_saved_cursor: 0,
            candidate_highlight: 0,
            last_caret_pos: None,
            activated: false,
        }
    }

    /// Initialize the engine and apply user preferences. Must be called before
    /// `on_key_down`. `dict_path` is where dictionaries live (None = default).
    pub fn activate(&mut self, dict_path: Option<String>) {
        let user_prefs = Preferences::load();
        self.apply_user_prefs(&user_prefs);

        let mut ed = Editor::chewing(dict_path, None, &DEFAULT_DICT_NAMES);
        ed.set_editor_options(|opts| {
            opts.candidates_per_page = user_prefs.candidates_per_page as usize;
            opts.space_is_select_key = true;
            opts.esc_clear_all_buffer = true;
            opts.auto_commit_threshold = 20;
            opts.auto_shift_cursor = true;
        });
        self.editor = Some(ed);
        self.activated = true;
    }

    pub fn deactivate(&mut self) {
        self.editor = None;
        self.session.clear();
        self.reset_space_cycle();
        self.candidate_highlight = 0;
        self.last_caret_pos = None;
        self.activated = false;
    }

    pub fn is_activated(&self) -> bool {
        self.activated
    }

    pub fn selection_keys(&self) -> &[char] {
        &self.selection_keys
    }

    /// Snapshot of whether the engine has content in its buffer
    /// (e.g. committed Chinese chars that haven't been flushed) or the mixed
    /// composing session has data. Used by OnTestKeyDown to decide eat-or-pass.
    pub fn has_content(&self) -> bool {
        let engine_content = self.editor.as_ref().map_or(false, |e| !e.is_empty());
        engine_content || self.session.has_mixed_content()
    }

    pub fn is_selecting(&self) -> bool {
        self.editor.as_ref().map_or(false, |e| e.is_selecting())
    }

    pub fn is_shift_held(&self) -> bool {
        self.session.is_shift_held()
    }

    fn apply_user_prefs(&mut self, user_prefs: &Preferences) {
        self.prefs.shift_behavior = user_prefs.shift_behavior;
        self.prefs.caps_lock_behavior = user_prefs.caps_lock_behavior;
        self.selection_keys = user_prefs.selection_keys.chars().collect();
        self.space_cycle_max = user_prefs.space_cycle_count;
        self.space_cycle_remaining = user_prefs.space_cycle_count;
    }

    // -----------------------------------------------------------------------
    // Display string — pure read
    // -----------------------------------------------------------------------

    fn build_display(&self) -> String {
        let Some(editor) = self.editor.as_ref() else { return String::new(); };

        if self.session.has_mixed_content() {
            self.session
                .build_display(&editor.display(), &editor.syllable_buffer_display())
        } else {
            let display = editor.display();
            let syllable = editor.syllable_buffer_display();
            if syllable.is_empty() {
                display
            } else {
                format!("{}{}", display, syllable)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Key event entry points
    // -----------------------------------------------------------------------

    /// OnTestKeyDown decision — does the controller want to handle this key?
    /// Returns true if the key should be eaten.
    pub fn should_eat_key_down(&self, vkey: u32, ch: char, ctrl: bool) -> bool {
        if !self.activated {
            return false;
        }
        if ctrl || vkey == VK_CONTROL || vkey == VK_MENU {
            return false;
        }
        // Eat Shift for SmartToggle
        if vkey == VK_SHIFT {
            return true;
        }

        let has_content = self.has_content();
        if has_content {
            // Engine may handle this key — eat if we know a mapping
            vkey_to_keyboard_event(vkey, ch, false, ctrl, false).is_some()
        } else {
            // Not composing: only eat keys that start input
            ch.is_ascii_graphic() || vkey == VK_SPACE
        }
    }

    pub fn should_eat_key_up(&self, vkey: u32) -> bool {
        if !self.activated {
            return false;
        }
        vkey == VK_SHIFT && self.session.is_shift_held()
    }

    /// Handle a key-down event. Returns true if the key was handled (eaten).
    pub fn on_key_down(
        &mut self,
        vkey: u32,
        ch: char,
        shift: bool,
        ctrl: bool,
        caps_lock: bool,
        sink: &dyn InputSink,
    ) -> bool {
        if ctrl || vkey == VK_CONTROL || vkey == VK_MENU {
            return false;
        }

        // Shift key-down: notify session
        if vkey == VK_SHIFT {
            let chinese_buf = self
                .editor
                .as_ref()
                .map_or(String::new(), |e| e.display());
            self.session.handle_shift(true, &self.prefs, &chinese_buf);
            return true;
        }

        // CapsLock English mode
        let caps_english =
            caps_lock && self.prefs.caps_lock_behavior == CapsLockBehavior::ToggleChineseEnglish;

        let is_english = self.session.is_english_mode();
        let is_shift_held = self.session.is_shift_held();

        if is_english || caps_english {
            return self.handle_english_key(vkey, ch, shift, sink);
        }
        // SmartToggle gesture: while Shift is held, only ASCII letters go to
        // English; shift+punctuation must fall through to the engine so
        // shift+comma/period produce full-width 「，」「。」.
        if shift && is_shift_held && ch.is_ascii_alphabetic() {
            return self.handle_english_key(vkey, ch, shift, sink);
        }

        // Candidate selection mode — intercept before engine
        if self.is_selecting() {
            if let Some(handled) = self.handle_candidate_key(vkey, ch, sink) {
                return handled;
            }
            // Fall through to engine
        }

        // Space cycle — intercept Space before engine
        if !self.is_selecting() && vkey == VK_SPACE && !shift {
            if let Some(handled) = self.handle_space_cycle(sink) {
                return handled;
            }
        }

        if vkey != VK_SPACE {
            self.reset_space_cycle();
        }

        let Some(evt) = vkey_to_keyboard_event(vkey, ch, shift, ctrl, caps_lock) else {
            return false;
        };

        if shift && !ch.is_ascii_alphabetic() {
            self.session.mark_shift_used();
        }

        let behavior = self
            .editor
            .as_mut()
            .map(|e| e.process_keyevent(evt))
            .unwrap_or(EditorKeyBehavior::Ignore);

        match behavior {
            EditorKeyBehavior::Absorb => {
                self.send_update(sink);
                true
            }
            EditorKeyBehavior::Commit => {
                self.handle_commit_flow(sink);
                true
            }
            EditorKeyBehavior::Bell | EditorKeyBehavior::Ignore => {
                self.handle_bell_ignore(vkey, sink)
            }
        }
    }

    /// Handle a key-up event. Returns true if the key was handled.
    pub fn on_key_up(&mut self, vkey: u32, sink: &dyn InputSink) -> bool {
        if vkey != VK_SHIFT || !self.activated {
            return false;
        }
        let chinese_buf = self
            .editor
            .as_ref()
            .map_or(String::new(), |e| e.display());
        let changed = self.session.handle_shift(false, &self.prefs, &chinese_buf);
        if changed {
            let has_content = self.session.has_mixed_content() || !chinese_buf.is_empty();
            if has_content {
                self.send_update(sink);
            }
        }
        true
    }

    pub fn on_composition_terminated(&mut self, sink: &dyn InputSink) {
        self.session.clear();
        if let Some(ed) = self.editor.as_mut() {
            ed.clear();
        }
        self.reset_space_cycle();
        sink.hide_candidates();
    }

    // -----------------------------------------------------------------------
    // Private handlers — moved verbatim from text_service.rs
    // -----------------------------------------------------------------------

    fn send_update(&mut self, sink: &dyn InputSink) {
        let text = self.build_display();
        let caret = sink.update_preedit(&text);
        if let Some(p) = caret {
            self.last_caret_pos = Some(p);
        }
        self.refresh_candidates(sink);
    }

    /// Re-render candidates based on current editor state.
    fn refresh_candidates(&mut self, sink: &dyn InputSink) {
        let Some(editor) = self.editor.as_ref() else {
            sink.hide_candidates();
            return;
        };

        if !editor.is_selecting() {
            sink.hide_candidates();
            return;
        }

        let cands = editor.paginated_candidates().unwrap_or_default();
        let page_no = editor.current_page_no().unwrap_or(0);
        let total_page = editor.total_page().unwrap_or(1);
        let page_info = if total_page > 1 {
            format!("{}/{}", page_no + 1, total_page)
        } else {
            String::new()
        };

        let limit = self.selection_keys.len();
        let cands: Vec<String> = cands.into_iter().take(limit).collect();

        // New page / new selection → reset highlight
        if self.candidate_highlight >= cands.len() {
            self.candidate_highlight = 0;
        }

        sink.show_candidates(
            &cands,
            &self.selection_keys,
            self.candidate_highlight,
            &page_info,
            self.last_caret_pos,
        );
    }

    fn handle_bell_ignore(&mut self, vkey: u32, sink: &dyn InputSink) -> bool {
        let has_content = self.has_content();

        if vkey == VK_RETURN && has_content {
            let chinese_buf = self
                .editor
                .as_ref()
                .map_or(String::new(), |e| e.display());
            let commit_str = if self.session.has_mixed_content() {
                self.session.commit_all(&chinese_buf)
            } else {
                chinese_buf
            };
            if let Some(ed) = self.editor.as_mut() {
                ed.clear();
            }
            if !commit_str.is_empty() {
                sink.commit_text(&commit_str);
            } else {
                sink.end_preedit();
            }
            sink.hide_candidates();
            return true;
        }

        if vkey == VK_ESCAPE && has_content {
            self.session.clear();
            if let Some(ed) = self.editor.as_mut() {
                ed.clear();
            }
            sink.end_preedit();
            sink.hide_candidates();
            return true;
        }

        if vkey == VK_BACK && !has_content {
            sink.end_preedit();
            return false;
        }

        has_content
    }

    fn handle_commit_flow(&mut self, sink: &dyn InputSink) {
        let commit_text = match self.editor.as_mut() {
            Some(ed) => {
                let text = ed.display_commit().to_string();
                ed.ack();
                text
            }
            None => return,
        };

        if self.session.has_mixed_content() {
            let full = self.session.commit_all(&commit_text);
            if !full.is_empty() {
                sink.commit_text(&full);
            }
            sink.hide_candidates();
            return;
        }

        let has_remaining = self.editor.as_ref().map_or(false, |e| !e.is_empty());

        if !commit_text.is_empty() {
            sink.commit_text(&commit_text);
        }
        if has_remaining {
            self.send_update(sink);
        } else {
            sink.hide_candidates();
        }
    }

    fn handle_english_key(
        &mut self,
        vkey: u32,
        ch: char,
        shift: bool,
        sink: &dyn InputSink,
    ) -> bool {
        // Printable ASCII or space
        if ch.is_ascii_graphic() || vkey == VK_SPACE {
            let actual_ch = if vkey == VK_SPACE { ' ' } else { ch };
            let chinese_buf = self
                .editor
                .as_ref()
                .map_or(String::new(), |e| e.display());
            let direct_commit = self.session.type_english(actual_ch, &chinese_buf);

            if direct_commit {
                sink.commit_text(&actual_ch.to_string());
                return true;
            }
            self.send_update(sink);
            return true;
        }

        if vkey == VK_BACK {
            if self.session.backspace_english() {
                self.send_update(sink);
                return true;
            }
            // Fall through
        }

        if vkey == VK_RETURN {
            let chinese_buf = self
                .editor
                .as_ref()
                .map_or(String::new(), |e| e.display());
            let commit_str = self.session.commit_all(&chinese_buf);
            if let Some(ed) = self.editor.as_mut() {
                ed.clear();
            }
            if !commit_str.is_empty() {
                sink.commit_text(&commit_str);
            }
            sink.hide_candidates();
            return true;
        }

        if vkey == VK_ESCAPE {
            self.session.clear();
            if let Some(ed) = self.editor.as_mut() {
                ed.clear();
            }
            sink.end_preedit();
            sink.hide_candidates();
            return true;
        }

        if shift {
            self.session.mark_shift_used();
        }
        false
    }

    /// Candidate-mode key handling. Returns Some(handled) if intercepted.
    fn handle_candidate_key(
        &mut self,
        vkey: u32,
        ch: char,
        sink: &dyn InputSink,
    ) -> Option<bool> {
        // Up/Down — UI-only highlight navigation
        if vkey == VK_UP {
            if self.candidate_highlight > 0 {
                self.candidate_highlight -= 1;
                self.refresh_candidates(sink);
            }
            return Some(true);
        }
        if vkey == VK_DOWN {
            let count = self
                .editor
                .as_ref()
                .and_then(|e| e.paginated_candidates().ok())
                .map_or(0, |c| c.len());
            let limit = count.min(self.selection_keys.len());
            if limit > 0 && self.candidate_highlight + 1 < limit {
                self.candidate_highlight += 1;
                self.refresh_candidates(sink);
            }
            return Some(true);
        }

        // Left — prev page (engine)
        if vkey == VK_LEFT {
            let evt = vkey_to_keyboard_event(vkey, '\0', false, false, false)?;
            if let Some(ed) = self.editor.as_mut() {
                ed.process_keyevent(evt);
            }
            self.candidate_highlight = 0;
            self.refresh_candidates(sink);
            return Some(true);
        }

        // Right / Space — next page (engine)
        if vkey == VK_RIGHT || vkey == VK_SPACE {
            let evt = vkey_to_keyboard_event(vkey, '\0', false, false, false)?;
            if let Some(ed) = self.editor.as_mut() {
                ed.process_keyevent(evt);
            }
            self.candidate_highlight = 0;
            self.refresh_candidates(sink);
            return Some(true);
        }

        // Enter — pick highlighted
        if vkey == VK_RETURN {
            let idx = self.candidate_highlight;
            if let Some(ed) = self.editor.as_mut() {
                let _ = ed.select(idx);
            }
            let still_selecting = self.is_selecting();
            self.candidate_highlight = 0;
            if still_selecting {
                self.refresh_candidates(sink);
                self.send_update(sink);
            } else {
                sink.hide_candidates();
                let has_commit = self
                    .editor
                    .as_ref()
                    .map_or(false, |e| !e.display_commit().is_empty());
                if has_commit {
                    self.handle_commit_flow(sink);
                } else {
                    self.send_update(sink);
                }
            }
            return Some(true);
        }

        // Esc — cancel without selecting
        if vkey == VK_ESCAPE {
            if let Some(ed) = self.editor.as_mut() {
                let _ = ed.cancel_selecting();
            }
            self.candidate_highlight = 0;
            sink.hide_candidates();
            self.send_update(sink);
            return Some(true);
        }

        // Backspace — cancel + delete one
        if vkey == VK_BACK {
            if let Some(ed) = self.editor.as_mut() {
                let _ = ed.cancel_selecting();
            }
            self.candidate_highlight = 0;
            sink.hide_candidates();
            let evt = vkey_to_keyboard_event(vkey, '\0', false, false, false)?;
            if let Some(ed) = self.editor.as_mut() {
                ed.process_keyevent(evt);
            }
            self.send_update(sink);
            return Some(true);
        }

        // Selection keys — direct selection
        if let Some(idx) = self.selection_keys.iter().position(|&k| k == ch) {
            let page_count = self
                .editor
                .as_ref()
                .and_then(|e| e.paginated_candidates().ok())
                .map_or(0, |c| c.len());
            if idx < page_count {
                if let Some(ed) = self.editor.as_mut() {
                    let _ = ed.select(idx);
                }
                self.candidate_highlight = 0;
                let still_selecting = self.is_selecting();
                if still_selecting {
                    self.refresh_candidates(sink);
                    self.send_update(sink);
                } else {
                    sink.hide_candidates();
                    let has_commit = self
                        .editor
                        .as_ref()
                        .map_or(false, |e| !e.display_commit().is_empty());
                    if has_commit {
                        self.handle_commit_flow(sink);
                    } else {
                        self.send_update(sink);
                    }
                }
                return Some(true);
            }
        }

        None
    }

    /// Handle space-cycle auto-selection. Returns Some(handled) if intercepted.
    fn handle_space_cycle(&mut self, sink: &dyn InputSink) -> Option<bool> {
        if self.space_cycle_remaining == 0 {
            return None;
        }

        let (has_buffer, is_composing_syllable, is_selecting) = {
            let ed = self.editor.as_ref()?;
            (
                !ed.is_empty(),
                !ed.syllable_buffer_display().is_empty(),
                ed.is_selecting(),
            )
        };

        if !has_buffer || is_composing_syllable || is_selecting {
            return None;
        }

        if self.space_cycle_targets.is_empty() {
            // First press — enter selecting mode, compute targets.
            let saved_cursor = self.editor.as_ref().map_or(0, |e| e.cursor());
            self.space_cycle_saved_cursor = saved_cursor;
            let current_buf: Vec<char> = self
                .editor
                .as_ref()
                .map_or(String::new(), |e| e.display())
                .chars()
                .collect();

            let space_evt = KeyboardEvent::builder().code(keycode::KEY_SPACE).build();
            if let Some(ed) = self.editor.as_mut() {
                ed.process_keyevent(space_evt);
            }

            let now_selecting = self.editor.as_ref().map_or(false, |e| e.is_selecting());
            if !now_selecting {
                self.space_cycle_remaining = 0;
                return None;
            }

            let candidates = self
                .editor
                .as_ref()
                .and_then(|e| e.all_candidates().ok())
                .unwrap_or_default();

            let select_pos = if saved_cursor >= current_buf.len() {
                saved_cursor.saturating_sub(1)
            } else {
                saved_cursor
            };

            let mut excluded = std::collections::HashSet::new();
            for cand in &candidates {
                let cand_len = cand.chars().count();
                let end = std::cmp::min(select_pos + cand_len, current_buf.len());
                if select_pos < end {
                    let buf_slice: String = current_buf[select_pos..end].iter().collect();
                    if buf_slice == *cand {
                        excluded.insert(cand.clone());
                    }
                }
            }

            let max = self.space_cycle_max as usize;
            let mut seen = excluded;
            let mut targets: Vec<String> = Vec::new();
            for cand in &candidates {
                if !seen.contains(cand) {
                    targets.push(cand.clone());
                    seen.insert(cand.clone());
                    if targets.len() >= max {
                        break;
                    }
                }
            }

            if targets.is_empty() {
                self.space_cycle_remaining = 0;
                self.send_update(sink);
                return Some(true);
            }

            let target = targets[0].clone();
            if let Some(idx) = candidates.iter().position(|c| *c == target) {
                if let Some(ed) = self.editor.as_mut() {
                    let _ = ed.select(idx);
                }
            }

            self.space_cycle_targets = targets;
            self.space_cycle_step = 1;
            self.space_cycle_remaining -= 1;
            self.send_update(sink);
            return Some(true);
        }

        // Subsequent press — next target
        if self.space_cycle_step >= self.space_cycle_targets.len() {
            self.space_cycle_remaining = 0;
            return None;
        }

        let space_evt = KeyboardEvent::builder().code(keycode::KEY_SPACE).build();
        if let Some(ed) = self.editor.as_mut() {
            ed.process_keyevent(space_evt);
        }

        let now_selecting = self.editor.as_ref().map_or(false, |e| e.is_selecting());
        if !now_selecting {
            self.space_cycle_remaining = 0;
            return None;
        }

        let candidates = self
            .editor
            .as_ref()
            .and_then(|e| e.all_candidates().ok())
            .unwrap_or_default();

        let target = self.space_cycle_targets[self.space_cycle_step].clone();
        if let Some(idx) = candidates.iter().position(|c| *c == target) {
            if let Some(ed) = self.editor.as_mut() {
                let _ = ed.select(idx);
            }
        }

        self.space_cycle_step += 1;
        self.space_cycle_remaining -= 1;
        self.send_update(sink);
        Some(true)
    }

    fn reset_space_cycle(&mut self) {
        self.space_cycle_remaining = self.space_cycle_max;
        self.space_cycle_targets.clear();
        self.space_cycle_step = 0;
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}
