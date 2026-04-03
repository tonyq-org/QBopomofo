//! TSF Text Service implementation.
//!
//! Core text service struct that wraps the chewing engine and implements
//! COM interfaces for Windows TSF integration. Includes mixed Chinese/English
//! input via ComposingSession and Shift SmartToggle support.

use std::cell::RefCell;

use windows::core::{implement, Interface, IUnknownImpl, Ref, BOOL, GUID};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext,
    ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr,
    ITfTextInputProcessor, ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfTextInputProcessor_Impl, ITfThreadMgr,
};

use chewing::composing_session::ComposingSession;
use chewing::dictionary::DEFAULT_DICT_NAMES;
use chewing::editor::{BasicEditor, Editor, EditorKeyBehavior};
use chewing::typing_mode::{CapsLockBehavior, ModePreferences, ShiftBehavior};

use crate::candidate_window::CandidateWindow;
use crate::edit_session::{self, EditOp, EditResult};
use crate::key_event::{translate_char, vkey_to_keyboard_event};
use crate::preferences::Preferences;

// Windows virtual key codes
const VK_BACK: u32 = 0x08;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;

// ---------------------------------------------------------------------------
// Inner mutable state — editor and session are separate RefCells to avoid
// borrow conflicts (they frequently need independent mutable access).
// ---------------------------------------------------------------------------

struct TsfState {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    composition: Option<ITfComposition>,
    activated: bool,
    prefs: ModePreferences,
    // Space cycle state
    space_cycle_max: u32,
    space_cycle_remaining: u32,
    space_cycle_targets: Vec<String>,
    space_cycle_step: usize,
    space_cycle_saved_cursor: usize,
}

// ---------------------------------------------------------------------------
// COM text service
// ---------------------------------------------------------------------------

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfKeyEventSink,
    ITfCompositionSink,
)]
pub struct QBopomofoTextService {
    editor: RefCell<Option<Editor>>,
    session: RefCell<ComposingSession>,
    state: RefCell<TsfState>,
    candidate_window: RefCell<Option<CandidateWindow>>,
}

impl QBopomofoTextService {
    pub fn new() -> Self {
        Self {
            editor: RefCell::new(None),
            session: RefCell::new(ComposingSession::new()),
            state: RefCell::new(TsfState {
                thread_mgr: None,
                client_id: 0,
                composition: None,
                activated: false,
                prefs: ModePreferences {
                    shift_behavior: ShiftBehavior::SmartToggle,
                    ..ModePreferences::default()
                },
                space_cycle_max: 0,
                space_cycle_remaining: 0,
                space_cycle_targets: Vec::new(),
                space_cycle_step: 0,
                space_cycle_saved_cursor: 0,
            }),
            candidate_window: RefCell::new(None),
        }
    }
}

// ---------------------------------------------------------------------------
// ITfTextInputProcessor / ITfTextInputProcessorEx
// ---------------------------------------------------------------------------

impl ITfTextInputProcessor_Impl for QBopomofoTextService_Impl {
    fn Activate(&self, _ptim: Ref<ITfThreadMgr>, _tid: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn Deactivate(&self) -> windows::core::Result<()> {
        let mut st = self.state.borrow_mut();

        if let Some(ref thread_mgr) = st.thread_mgr {
            if let Ok(km) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                let _ = unsafe { km.UnadviseKeyEventSink(st.client_id) };
            }
        }

        st.composition = None;
        st.thread_mgr = None;
        st.activated = false;
        *self.editor.borrow_mut() = None;
        self.session.borrow_mut().clear();
        *self.candidate_window.borrow_mut() = None;
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for QBopomofoTextService_Impl {
    fn ActivateEx(
        &self,
        ptim: Ref<ITfThreadMgr>,
        tid: u32,
        _dwflags: u32,
    ) -> windows::core::Result<()> {
        qb_dbg!("ActivateEx: start, tid={}", tid);
        let thread_mgr: ITfThreadMgr = match ptim.clone() {
            Some(t) => t,
            None => {
                qb_dbg!("ActivateEx: ptim is null!");
                return Err(windows::core::Error::from(windows::Win32::Foundation::E_POINTER));
            }
        };

        // Load user preferences from registry
        let user_prefs = Preferences::load();

        qb_dbg!("ActivateEx: tid={}, prefs={:?}", tid, user_prefs);

        let dict_path = crate::com::dll_dir();
        qb_dbg!("ActivateEx: dict_path={:?}", dict_path);
        let mut ed = Editor::chewing(dict_path, None, &DEFAULT_DICT_NAMES);
        ed.set_editor_options(|opts| {
            opts.candidates_per_page = user_prefs.candidates_per_page as usize;
            opts.space_is_select_key = true;
            opts.esc_clear_all_buffer = true;
            opts.auto_commit_threshold = 20;
            opts.auto_shift_cursor = true;
        });
        *self.editor.borrow_mut() = Some(ed);

        // Create candidate window
        let mut cw = CandidateWindow::new();
        let sel_keys: Vec<char> = user_prefs.selection_keys.chars().collect();
        cw.set_selection_keys(&sel_keys);
        *self.candidate_window.borrow_mut() = Some(cw);

        let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;
        let self_sink: ITfKeyEventSink = self.to_interface();
        unsafe { keystroke_mgr.AdviseKeyEventSink(tid, &self_sink, true)? };

        let mut st = self.state.borrow_mut();
        st.thread_mgr = Some(thread_mgr);
        st.client_id = tid;
        st.activated = true;
        st.prefs.shift_behavior = user_prefs.shift_behavior;
        st.prefs.caps_lock_behavior = user_prefs.caps_lock_behavior;
        st.space_cycle_max = user_prefs.space_cycle_count;
        st.space_cycle_remaining = user_prefs.space_cycle_count;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_modifiers() -> (bool, bool, bool) {
    let shift = (unsafe { GetKeyState(VK_SHIFT as i32) } & 0x8000u16 as i16) != 0;
    let ctrl = (unsafe { GetKeyState(VK_CONTROL as i32) } & 0x8000u16 as i16) != 0;
    let caps_lock = (unsafe { GetKeyState(0x14) } & 1) != 0;
    (shift, ctrl, caps_lock)
}

impl QBopomofoTextService_Impl {
    /// Build the preedit display string.
    fn build_display(&self) -> String {
        let ed = self.editor.borrow();
        let editor = match ed.as_ref() {
            Some(e) => e,
            None => return String::new(),
        };
        let sess = self.session.borrow();

        let base = if sess.has_mixed_content() {
            sess.build_display(&editor.display(), &editor.syllable_buffer_display())
        } else {
            let display = editor.display();
            let syllable = editor.syllable_buffer_display();
            if syllable.is_empty() { display } else { format!("{}{}", display, syllable) }
        };

        qb_dbg!("build_display: base={:?} cursor={} len={} is_empty={}", base, editor.cursor(), base.chars().count(), editor.is_empty());

        base
    }

    /// Send an UpdateComposition edit session with the current display.
    fn send_update_composition(&self, pic: &Ref<ITfContext>) -> windows::core::Result<BOOL> {
        let text = self.build_display();
        let composition = self.state.borrow_mut().composition.take();
        let tid = self.state.borrow().client_id;
        let sink: ITfCompositionSink = self.to_interface();
        let context: ITfContext = pic.unwrap().clone();

        let mut caret_pos = None;
        if let Some(EditResult::Composition(new_comp, pos)) =
            edit_session::request_edit_session(
                &context, tid,
                EditOp::UpdateComposition { text, composition, sink },
            )?
        {
            caret_pos = pos;
            self.state.borrow_mut().composition = new_comp;
        }
        self.update_candidate_window(caret_pos);
        Ok(BOOL(1))
    }

    /// Send a CommitText edit session.
    fn send_commit(&self, pic: &Ref<ITfContext>, text: String) -> windows::core::Result<BOOL> {
        let composition = self.state.borrow_mut().composition.take();
        let tid = self.state.borrow().client_id;
        let context: ITfContext = pic.unwrap().clone();

        edit_session::request_edit_session(
            &context, tid,
            EditOp::CommitText { text, composition },
        )?;
        self.state.borrow_mut().composition = None;
        if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
        Ok(BOOL(1))
    }

    /// Reset space cycle state (called on any non-space key).
    fn reset_space_cycle(&self) {
        let mut st = self.state.borrow_mut();
        st.space_cycle_remaining = st.space_cycle_max;
        st.space_cycle_targets.clear();
        st.space_cycle_step = 0;
    }

    /// Handle space cycle selection. Returns `Some(BOOL)` if handled, `None` if
    /// the space should be processed normally by the engine.
    fn handle_space_cycle(&self, pic: &Ref<ITfContext>) -> Option<windows::core::Result<BOOL>> {
        use chewing::input::{KeyboardEvent as KbEvt, keycode};

        let remaining = self.state.borrow().space_cycle_remaining;
        if remaining == 0 {
            return None;
        }

        // Only cycle when there's a buffer and no bopomofo composing
        let (has_buffer, is_composing_syllable, is_selecting) = {
            let ed = self.editor.borrow();
            let editor = ed.as_ref()?;
            (!editor.is_empty(), !editor.syllable_buffer_display().is_empty(), editor.is_selecting())
        };

        if !has_buffer || is_composing_syllable || is_selecting {
            return None;
        }

        let targets_empty = self.state.borrow().space_cycle_targets.is_empty();

        if targets_empty {
            // First space press: enter cand mode, compute targets
            let saved_cursor = self.editor.borrow().as_ref().map_or(0, |e| e.cursor());
            self.state.borrow_mut().space_cycle_saved_cursor = saved_cursor;

            // Get current buffer text
            let current_buf: Vec<char> = self.editor.borrow().as_ref()
                .map_or(String::new(), |e| e.display()).chars().collect();

            // Send Space to engine to enter selecting mode
            let space_evt = KbEvt::builder().code(keycode::KEY_SPACE).build();
            {
                let mut ed = self.editor.borrow_mut();
                let editor = ed.as_mut()?;
                editor.process_keyevent(space_evt);
            }

            let is_now_selecting = self.editor.borrow().as_ref().map_or(false, |e| e.is_selecting());
            if !is_now_selecting {
                self.state.borrow_mut().space_cycle_remaining = 0;
                return None;
            }

            // Get all candidates and compute distinct targets
            let candidates = self.editor.borrow().as_ref()
                .and_then(|e| e.all_candidates().ok())
                .unwrap_or_default();

            // Exclude the current text at the cursor position
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

            let max = self.state.borrow().space_cycle_max as usize;
            let mut seen = excluded;
            let mut targets = Vec::new();
            for cand in &candidates {
                if !seen.contains(cand) {
                    targets.push(cand.clone());
                    seen.insert(cand.clone());
                    if targets.len() >= max { break; }
                }
            }

            if targets.is_empty() {
                // No different candidates — stay in cand mode, show panel
                self.state.borrow_mut().space_cycle_remaining = 0;
                return Some(self.send_update_composition(pic));
            }

            // Select the first target
            let target = targets[0].clone();
            if let Some(idx) = candidates.iter().position(|c| *c == target) {
                let mut ed = self.editor.borrow_mut();
                if let Some(editor) = ed.as_mut() {
                    let _ = editor.select(idx);
                }
            }

            let mut st = self.state.borrow_mut();
            st.space_cycle_targets = targets;
            st.space_cycle_step = 1;
            st.space_cycle_remaining -= 1;

            drop(st);
            return Some(self.send_update_composition(pic));
        }

        // Subsequent space presses: select next target
        let step = self.state.borrow().space_cycle_step;
        let targets_len = self.state.borrow().space_cycle_targets.len();

        if step >= targets_len {
            // Exhausted targets — fall through to normal engine handling
            self.state.borrow_mut().space_cycle_remaining = 0;
            return None;
        }

        // Re-enter selecting mode at saved cursor
        let space_evt = KbEvt::builder().code(keycode::KEY_SPACE).build();
        {
            let mut ed = self.editor.borrow_mut();
            if let Some(editor) = ed.as_mut() {
                editor.process_keyevent(space_evt);
            }
        }

        let is_now_selecting = self.editor.borrow().as_ref().map_or(false, |e| e.is_selecting());
        if !is_now_selecting {
            self.state.borrow_mut().space_cycle_remaining = 0;
            return None;
        }

        let candidates = self.editor.borrow().as_ref()
            .and_then(|e| e.all_candidates().ok())
            .unwrap_or_default();

        let target = self.state.borrow().space_cycle_targets[step].clone();
        if let Some(idx) = candidates.iter().position(|c| *c == target) {
            let mut ed = self.editor.borrow_mut();
            if let Some(editor) = ed.as_mut() {
                let _ = editor.select(idx);
            }
        }

        let mut st = self.state.borrow_mut();
        st.space_cycle_step += 1;
        st.space_cycle_remaining -= 1;

        drop(st);
        Some(self.send_update_composition(pic))
    }

    /// Update the candidate window visibility based on editor state.
    fn update_candidate_window(&self, caret_pos: Option<(i32, i32)>) {
        let ed = self.editor.borrow();
        let editor = match ed.as_ref() {
            Some(e) => e,
            None => return,
        };

        if editor.is_selecting() {
            let cands = editor.paginated_candidates().unwrap_or_default();
            let all_count = editor.all_candidates().map_or(0, |c| c.len());
            let page_no = editor.current_page_no().unwrap_or(0);
            let total_page = editor.total_page().unwrap_or(1);
            qb_dbg!("update_candidate_window: page_cands={} all={} page={}/{}", cands.len(), all_count, page_no+1, total_page);
            let page_info = if total_page > 1 {
                format!("{}/{}", page_no + 1, total_page)
            } else {
                String::new()
            };

            // Limit to selection keys count
            let sel_keys_len = self.candidate_window.borrow().as_ref()
                .map_or(10, |cw| cw.selection_keys_count());
            let cands: Vec<String> = cands.into_iter().take(sel_keys_len).collect();

            if let Some(ref mut cw) = *self.candidate_window.borrow_mut() {
                let (x, y) = caret_pos.unwrap_or_else(|| cw.last_position());
                cw.show(&cands, 0, &page_info, x, y);
            }
        } else if let Some(ref cw) = *self.candidate_window.borrow() {
            cw.hide();
        }
    }

    /// Handle keys during candidate selection mode (aligned with Mac behavior).
    /// Returns Some if handled, None to fall through to engine.
    fn handle_candidate_key(
        &self,
        vkey: u32,
        ch: char,
        pic: &Ref<ITfContext>,
    ) -> Option<windows::core::Result<BOOL>> {
        // Up: move highlight up (no wrap)
        if vkey == VK_UP {
            if let Some(ref mut cw) = *self.candidate_window.borrow_mut() {
                cw.highlight_previous();
            }
            return Some(Ok(BOOL(1)));
        }

        // Down: move highlight down (no wrap)
        if vkey == VK_DOWN {
            if let Some(ref mut cw) = *self.candidate_window.borrow_mut() {
                cw.highlight_next();
            }
            return Some(Ok(BOOL(1)));
        }

        // Left: previous page (via engine)
        if vkey == VK_LEFT {
            let evt = vkey_to_keyboard_event(vkey, '\0', false, false, false)?;
            let mut ed = self.editor.borrow_mut();
            let editor = ed.as_mut()?;
            editor.process_keyevent(evt);
            drop(ed);
            self.update_candidate_window(None);
            return Some(Ok(BOOL(1)));
        }

        // Right / Space: next page (via engine, same as Mac)
        if vkey == VK_RIGHT || vkey == VK_SPACE {
            let evt = vkey_to_keyboard_event(vkey, '\0', false, false, false)?;
            let mut ed = self.editor.borrow_mut();
            let editor = ed.as_mut()?;
            editor.process_keyevent(evt);
            drop(ed);
            self.update_candidate_window(None);
            return Some(Ok(BOOL(1)));
        }

        // Enter: select highlighted candidate
        if vkey == VK_RETURN {
            let idx = self.candidate_window.borrow().as_ref()
                .map_or(0, |cw| cw.highlighted_index());
            let mut ed = self.editor.borrow_mut();
            if let Some(editor) = ed.as_mut() {
                let _ = editor.select(idx);
            }
            drop(ed);
            // After selection, engine may have committed or returned to editing
            let is_still_selecting = self.editor.borrow().as_ref()
                .map_or(false, |e| e.is_selecting());
            if is_still_selecting {
                self.update_candidate_window(None);
            } else {
                if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
            }
            return Some(self.send_update_composition(pic));
        }

        // Escape: close candidate panel without selecting
        if vkey == VK_ESCAPE {
            let mut ed = self.editor.borrow_mut();
            if let Some(editor) = ed.as_mut() {
                let _ = editor.cancel_selecting();
            }
            drop(ed);
            if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
            return Some(self.send_update_composition(pic));
        }

        // Backspace: close candidate panel + delete one character
        if vkey == VK_BACK {
            let mut ed = self.editor.borrow_mut();
            if let Some(editor) = ed.as_mut() {
                let _ = editor.cancel_selecting();
            }
            drop(ed);
            if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
            // Now send backspace to engine for deletion
            let evt = vkey_to_keyboard_event(vkey, '\0', false, false, false)?;
            let mut ed = self.editor.borrow_mut();
            if let Some(editor) = ed.as_mut() {
                editor.process_keyevent(evt);
            }
            drop(ed);
            return Some(self.send_update_composition(pic));
        }

        // Number keys: direct selection by selection key
        let sel_keys: Vec<char> = self.candidate_window.borrow().as_ref()
            .map_or("1234567890".chars().collect(), |cw| cw.get_selection_keys());
        if let Some(idx) = sel_keys.iter().position(|&k| k == ch) {
            let page_count = self.editor.borrow().as_ref()
                .and_then(|e| e.paginated_candidates().ok())
                .map_or(0, |c| c.len());
            if idx < page_count {
                let mut ed = self.editor.borrow_mut();
                if let Some(editor) = ed.as_mut() {
                    let _ = editor.select(idx);
                }
                drop(ed);
                let is_still_selecting = self.editor.borrow().as_ref()
                    .map_or(false, |e| e.is_selecting());
                if is_still_selecting {
                    self.update_candidate_window(None);
                    return Some(self.send_update_composition(pic));
                } else {
                    if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
                    // Check if engine wants to commit
                    let has_commit = self.editor.borrow().as_ref()
                        .map_or(false, |e| !e.display_commit().is_empty());
                    if has_commit {
                        return Some(self.handle_commit(pic));
                    }
                    return Some(self.send_update_composition(pic));
                }
            }
        }

        // Other keys: not handled in candidate mode, fall through
        None
    }

    /// Send an EndComposition edit session.
    fn send_end_composition(&self, pic: &Ref<ITfContext>) -> windows::core::Result<BOOL> {
        let composition = self.state.borrow_mut().composition.take();
        let tid = self.state.borrow().client_id;
        let context: ITfContext = pic.unwrap().clone();

        edit_session::request_edit_session(
            &context, tid,
            EditOp::EndComposition { composition },
        )?;
        self.state.borrow_mut().composition = None;
        if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
        Ok(BOOL(1))
    }
}

// ---------------------------------------------------------------------------
// ITfKeyEventSink
// ---------------------------------------------------------------------------

impl ITfKeyEventSink_Impl for QBopomofoTextService_Impl {
    fn OnSetFocus(&self, _fforeground: BOOL) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        _pic: Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        qb_dbg!("OnTestKeyDown: vk={:#x}", wparam.0);
        if !self.state.borrow().activated {
            return Ok(BOOL(0));
        }

        let vkey = wparam.0 as u32;
        let (shift, ctrl, caps_lock) = get_modifiers();

        if ctrl || vkey == VK_CONTROL || vkey == VK_MENU {
            return Ok(BOOL(0));
        }

        // Eat Shift for SmartToggle
        if vkey == VK_SHIFT {
            return Ok(BOOL(1));
        }

        let ch = translate_char(vkey, lparam.0 as u32, shift);
        let has_content = self.editor.borrow().as_ref().map_or(false, |e| !e.is_empty())
            || self.session.borrow().has_mixed_content();
        let has_composition = self.state.borrow().composition.is_some();

        // Only eat keys we know how to handle:
        // - When composing (has_content or has_composition): eat everything the engine understands
        // - When not composing: only eat printable keys that start input (letters, numbers, symbols)
        let eat = if has_content || has_composition {
            vkey_to_keyboard_event(vkey, ch, shift, ctrl, caps_lock).is_some()
        } else {
            // Not composing: only eat keys that produce bopomofo/symbols
            ch.is_ascii_graphic() || vkey == VK_SPACE
        };

        qb_dbg!("OnTestKeyDown: vk={:#x} eat={} has_content={} has_comp={}", vkey, eat, has_content, has_composition);
        Ok(BOOL(eat as i32))
    }

    fn OnTestKeyUp(
        &self,
        _pic: Ref<ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        if wparam.0 as u32 == VK_SHIFT {
            if self.state.borrow().activated && self.session.borrow().is_shift_held() {
                return Ok(BOOL(1));
            }
        }
        Ok(BOOL(0))
    }

    fn OnKeyDown(
        &self,
        pic: Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        qb_dbg!("OnKeyDown: vk={:#x}", wparam.0);
        let vkey = wparam.0 as u32;
        let (shift, ctrl, caps_lock) = get_modifiers();

        if ctrl || vkey == VK_CONTROL || vkey == VK_MENU {
            return Ok(BOOL(0));
        }

        // Handle Shift key-down: notify session
        if vkey == VK_SHIFT {
            let chinese_buf = self.editor.borrow().as_ref()
                .map_or(String::new(), |e| e.display());
            let prefs = self.state.borrow().prefs.clone();
            self.session.borrow_mut().handle_shift(true, &prefs, &chinese_buf);
            return Ok(BOOL(1));
        }

        let ch = translate_char(vkey, lparam.0 as u32, shift);

        // --- CapsLock toggle: treat as English mode ---
        let caps_english = caps_lock
            && self.state.borrow().prefs.caps_lock_behavior == CapsLockBehavior::ToggleChineseEnglish;

        // --- English mode handling ---
        let is_english = self.session.borrow().is_english_mode();
        let is_shift_held = self.session.borrow().is_shift_held();

        if is_english || caps_english || (shift && is_shift_held) {
            return self.handle_english_key(vkey, ch, shift, &pic);
        }

        // --- Chinese mode handling ---

        // --- Candidate selection mode: intercept keys before engine ---
        let is_selecting = self.editor.borrow().as_ref().map_or(false, |e| e.is_selecting());
        if is_selecting {
            if let Some(result) = self.handle_candidate_key(vkey, ch, &pic) {
                return result;
            }
            // If not handled, fall through to engine
        }

        // Space cycle selection: intercept Space before sending to engine
        if !is_selecting && vkey == VK_SPACE && !shift {
            if let Some(result) = self.handle_space_cycle(&pic) {
                return result;
            }
            // If not handled by space cycle, reset and fall through to engine
        }

        // Non-space key resets space cycle state
        if vkey != VK_SPACE {
            self.reset_space_cycle();
        }

        let evt = match vkey_to_keyboard_event(vkey, ch, shift, ctrl, caps_lock) {
            Some(evt) => evt,
            None => return Ok(BOOL(0)),
        };

        // Mark shift used for non-letter keys
        if shift && !ch.is_ascii_alphabetic() {
            self.session.borrow_mut().mark_shift_used();
        }

        let behavior = {
            let mut ed = self.editor.borrow_mut();
            let editor = match ed.as_mut() {
                Some(e) => e,
                None => return Ok(BOOL(0)),
            };
            editor.process_keyevent(evt)
        };

        qb_dbg!("OnKeyDown: vk={:#x} ch={:#x} behavior={:?}", vkey, ch as u32, behavior);

        match behavior {
            EditorKeyBehavior::Absorb => self.send_update_composition(&pic),
            EditorKeyBehavior::Commit => self.handle_commit(&pic),
            EditorKeyBehavior::Bell | EditorKeyBehavior::Ignore => {
                // Engine has nothing to do, but handle special keys ourselves.
                let has_content = self.editor.borrow().as_ref().map_or(false, |e| !e.is_empty())
                    || self.session.borrow().has_mixed_content();
                let has_composition = self.state.borrow().composition.is_some();

                qb_dbg!("Bell/Ignore: vk={:#x} has_content={} has_comp={}", vkey, has_content, has_composition);

                if vkey == VK_RETURN && (has_content || has_composition) {
                    let chinese_buf = self.editor.borrow().as_ref()
                        .map_or(String::new(), |e| e.display());
                    let commit_str = if self.session.borrow().has_mixed_content() {
                        self.session.borrow_mut().commit_all(&chinese_buf)
                    } else {
                        chinese_buf
                    };
                    if let Some(ref mut ed) = *self.editor.borrow_mut() { ed.clear(); }
                    if !commit_str.is_empty() {
                        return self.send_commit(&pic, commit_str);
                    }
                    return self.send_end_composition(&pic);
                }
                if vkey == VK_ESCAPE && has_composition {
                    self.session.borrow_mut().clear();
                    if let Some(ref mut ed) = *self.editor.borrow_mut() { ed.clear(); }
                    return self.send_end_composition(&pic);
                }
                // Backspace with no engine content: end lingering composition, pass through
                if vkey == VK_BACK && !has_content {
                    if has_composition {
                        let _ = self.send_end_composition(&pic);
                    }
                    return Ok(BOOL(0));
                }
                // For other keys with no content and a lingering composition, clean up
                if !has_content && has_composition {
                    let _ = self.send_end_composition(&pic);
                }
                Ok(BOOL(if has_content || has_composition { 1 } else { 0 }))
            }
        }
    }

    fn OnKeyUp(
        &self,
        pic: Ref<ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        if wparam.0 as u32 != VK_SHIFT {
            return Ok(BOOL(0));
        }

        if !self.state.borrow().activated {
            return Ok(BOOL(0));
        }

        let chinese_buf = self.editor.borrow().as_ref()
            .map_or(String::new(), |e| e.display());
        let prefs = self.state.borrow().prefs.clone();
        let changed = self.session.borrow_mut().handle_shift(false, &prefs, &chinese_buf);

        if changed {
            let has_content = self.session.borrow().has_mixed_content()
                || !chinese_buf.is_empty();
            if has_content {
                return self.send_update_composition(&pic);
            }
        }
        Ok(BOOL(1))
    }

    fn OnPreservedKey(
        &self,
        _pic: Ref<ITfContext>,
        _rguid: *const GUID,
    ) -> windows::core::Result<BOOL> {
        Ok(BOOL(0))
    }
}

// ---------------------------------------------------------------------------
// English mode key handling
// ---------------------------------------------------------------------------

impl QBopomofoTextService_Impl {
    fn handle_english_key(
        &self,
        vkey: u32,
        ch: char,
        shift: bool,
        pic: &Ref<ITfContext>,
    ) -> windows::core::Result<BOOL> {
        // Printable ASCII (letters, digits, punctuation)
        if ch.is_ascii_graphic() || (vkey == VK_SPACE) {
            let actual_ch = if vkey == VK_SPACE { ' ' } else { ch };
            let chinese_buf = self.editor.borrow().as_ref()
                .map_or(String::new(), |e| e.display());
            let direct_commit = self.session.borrow_mut().type_english(actual_ch, &chinese_buf);

            if direct_commit {
                return self.send_commit(pic, actual_ch.to_string());
            }
            return self.send_update_composition(pic);
        }

        // Backspace
        if vkey == VK_BACK {
            let deleted = self.session.borrow_mut().backspace_english();
            if deleted {
                return self.send_update_composition(pic);
            }
            // Fall through — nothing to delete in English buffer
        }

        // Enter — commit all
        if vkey == VK_RETURN {
            let chinese_buf = self.editor.borrow().as_ref()
                .map_or(String::new(), |e| e.display());
            let commit_str = self.session.borrow_mut().commit_all(&chinese_buf);
            if let Some(ref mut ed) = *self.editor.borrow_mut() { ed.clear(); }

            if !commit_str.is_empty() {
                return self.send_commit(pic, commit_str);
            }
            return Ok(BOOL(1));
        }

        // Escape — clear all
        if vkey == VK_ESCAPE {
            self.session.borrow_mut().clear();
            if let Some(ref mut ed) = *self.editor.borrow_mut() { ed.clear(); }
            return self.send_end_composition(pic);
        }

        // Non-letter keys: mark shift used and pass through
        if shift { self.session.borrow_mut().mark_shift_used(); }
        Ok(BOOL(0))
    }

    fn handle_commit(&self, pic: &Ref<ITfContext>) -> windows::core::Result<BOOL> {
        let commit_text = {
            let mut ed = self.editor.borrow_mut();
            let editor = ed.as_mut().unwrap();
            let text = editor.display_commit().to_string();
            editor.ack();
            text
        };

        // If mixed content, commit everything in segment order
        if self.session.borrow().has_mixed_content() {
            let full_commit = self.session.borrow_mut().commit_all(&commit_text);
            if !full_commit.is_empty() {
                return self.send_commit(pic, full_commit);
            }
            return Ok(BOOL(1));
        }

        // Pure Chinese commit
        let has_remaining = self.editor.borrow().as_ref()
            .map_or(false, |e| !e.is_empty());

        if !commit_text.is_empty() {
            let _ = self.send_commit(pic, commit_text)?;
        }

        if has_remaining {
            let _ = self.send_update_composition(pic)?;
        }
        Ok(BOOL(1))
    }
}

// ---------------------------------------------------------------------------
// ITfCompositionSink
// ---------------------------------------------------------------------------

impl ITfCompositionSink_Impl for QBopomofoTextService_Impl {
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: Ref<ITfComposition>,
    ) -> windows::core::Result<()> {
        self.state.borrow_mut().composition = None;
        self.session.borrow_mut().clear();
        if let Some(ref mut ed) = *self.editor.borrow_mut() { ed.clear(); }
        if let Some(ref cw) = *self.candidate_window.borrow() { cw.hide(); }
        Ok(())
    }
}
