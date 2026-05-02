//! TSF Text Service implementation.
//!
//! Thin COM wrapper around [`Controller`]. All input logic lives in
//! `controller.rs`; this module only implements the TSF COM interfaces,
//! bridges `Controller` events (via `InputSink`) to TSF edit sessions, and
//! owns the candidate window.
//!
//! Every COM method body is wrapped in `com_method_*!` so Rust panics cannot
//! cross the `extern "system"` FFI boundary.

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

use crate::candidate_window::CandidateWindow;
use crate::controller::{Controller, InputSink};
use crate::edit_session::{self, EditOp, EditResult};
use crate::key_event::translate_char;
use crate::{com_method_bool, com_method_unit};

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_CAPITAL: u32 = 0x14;

// ---------------------------------------------------------------------------
// COM-side state (everything that depends on TSF types lives here).
// ---------------------------------------------------------------------------

struct TsfState {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    activated: bool,
}

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfKeyEventSink,
    ITfCompositionSink,
)]
pub struct QBopomofoTextService {
    controller: RefCell<Controller>,
    state: RefCell<TsfState>,
    composition: RefCell<Option<ITfComposition>>,
    candidate_window: RefCell<Option<CandidateWindow>>,
}

impl QBopomofoTextService {
    pub fn new() -> Self {
        Self {
            controller: RefCell::new(Controller::new()),
            state: RefCell::new(TsfState {
                thread_mgr: None,
                client_id: 0,
                activated: false,
            }),
            composition: RefCell::new(None),
            candidate_window: RefCell::new(None),
        }
    }
}

fn get_modifiers() -> (bool, bool, bool) {
    let shift = (unsafe { GetKeyState(VK_SHIFT as i32) } & 0x8000u16 as i16) != 0;
    let ctrl = (unsafe { GetKeyState(VK_CONTROL as i32) } & 0x8000u16 as i16) != 0;
    let caps_lock = (unsafe { GetKeyState(VK_CAPITAL as i32) } & 1) != 0;
    (shift, ctrl, caps_lock)
}

// ---------------------------------------------------------------------------
// ITfTextInputProcessor / ITfTextInputProcessorEx
// ---------------------------------------------------------------------------

impl ITfTextInputProcessor_Impl for QBopomofoTextService_Impl {
    fn Activate(&self, _ptim: Ref<ITfThreadMgr>, _tid: u32) -> windows::core::Result<()> {
        com_method_unit!("Activate", { Ok(()) })
    }

    fn Deactivate(&self) -> windows::core::Result<()> {
        com_method_unit!("Deactivate", {
            let (thread_mgr, client_id) = {
                let mut st = self.state.borrow_mut();
                (st.thread_mgr.take(), st.client_id)
            };

            if let Some(ref tm) = thread_mgr {
                if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                    let _ = unsafe { km.UnadviseKeyEventSink(client_id) };
                }
            }

            *self.composition.borrow_mut() = None;
            *self.candidate_window.borrow_mut() = None;
            self.controller.borrow_mut().deactivate();

            self.state.borrow_mut().activated = false;
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for QBopomofoTextService_Impl {
    fn ActivateEx(
        &self,
        ptim: Ref<ITfThreadMgr>,
        tid: u32,
        _dwflags: u32,
    ) -> windows::core::Result<()> {
        com_method_unit!("ActivateEx", {
            qb_dbg!("ActivateEx: start, tid={}", tid);

            let Some(thread_mgr) = ptim.clone() else {
                qb_dbg!("ActivateEx: ptim is null!");
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_POINTER,
                ));
            };

            let dict_path = crate::com::dll_dir();
            qb_dbg!("ActivateEx: dict_path={:?}", dict_path);

            self.controller.borrow_mut().activate(dict_path);

            let mut cw = CandidateWindow::new();
            {
                let c = self.controller.borrow();
                cw.set_selection_keys(c.selection_keys());
            }
            *self.candidate_window.borrow_mut() = Some(cw);

            let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;
            let self_sink: ITfKeyEventSink = self.to_interface();
            unsafe { keystroke_mgr.AdviseKeyEventSink(tid, &self_sink, true)? };

            let mut st = self.state.borrow_mut();
            st.thread_mgr = Some(thread_mgr);
            st.client_id = tid;
            st.activated = true;
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// ITfKeyEventSink
// ---------------------------------------------------------------------------

impl ITfKeyEventSink_Impl for QBopomofoTextService_Impl {
    fn OnSetFocus(&self, _fforeground: BOOL) -> windows::core::Result<()> {
        com_method_unit!("OnSetFocus", { Ok(()) })
    }

    fn OnTestKeyDown(
        &self,
        _pic: Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        com_method_bool!("OnTestKeyDown", {
            if !self.state.borrow().activated {
                return Ok(BOOL(0));
            }
            let vkey = wparam.0 as u32;
            let (shift, ctrl, _caps) = get_modifiers();
            let ch = translate_char(vkey, lparam.0 as u32, shift);
            let eat = self.controller.borrow().should_eat_key_down(vkey, ch, ctrl);
            qb_dbg!(
                "OnTestKeyDown: vk={:#x} ch={:?} eat={}",
                vkey, ch, eat
            );
            Ok(BOOL(if eat { 1 } else { 0 }))
        })
    }

    fn OnTestKeyUp(
        &self,
        _pic: Ref<ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        com_method_bool!("OnTestKeyUp", {
            let vkey = wparam.0 as u32;
            let eat = self.controller.borrow().should_eat_key_up(vkey);
            Ok(BOOL(if eat { 1 } else { 0 }))
        })
    }

    fn OnKeyDown(
        &self,
        pic: Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        com_method_bool!("OnKeyDown", {
            let vkey = wparam.0 as u32;
            let (shift, ctrl, caps) = get_modifiers();
            let ch = translate_char(vkey, lparam.0 as u32, shift);
            qb_dbg!(
                "OnKeyDown: vk={:#x} ch={:?} shift={} ctrl={} caps={}",
                vkey, ch, shift, ctrl, caps
            );

            let Some(context) = pic.clone() else {
                return Ok(BOOL(0));
            };
            let tid = self.state.borrow().client_id;
            let comp_sink: ITfCompositionSink = self.to_interface();

            let sink = TsfSink {
                context,
                tid,
                comp_sink,
                composition: &self.composition,
                candidate_window: &self.candidate_window,
            };
            let handled =
                self.controller
                    .borrow_mut()
                    .on_key_down(vkey, ch, shift, ctrl, caps, &sink);
            Ok(BOOL(if handled { 1 } else { 0 }))
        })
    }

    fn OnKeyUp(
        &self,
        pic: Ref<ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        com_method_bool!("OnKeyUp", {
            let vkey = wparam.0 as u32;
            let Some(context) = pic.clone() else {
                return Ok(BOOL(0));
            };
            let tid = self.state.borrow().client_id;
            let comp_sink: ITfCompositionSink = self.to_interface();
            let sink = TsfSink {
                context,
                tid,
                comp_sink,
                composition: &self.composition,
                candidate_window: &self.candidate_window,
            };
            let handled = self.controller.borrow_mut().on_key_up(vkey, &sink);
            Ok(BOOL(if handled { 1 } else { 0 }))
        })
    }

    fn OnPreservedKey(
        &self,
        _pic: Ref<ITfContext>,
        _rguid: *const GUID,
    ) -> windows::core::Result<BOOL> {
        com_method_bool!("OnPreservedKey", { Ok(BOOL(0)) })
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
        com_method_unit!("OnCompositionTerminated", {
            *self.composition.borrow_mut() = None;
            let null_sink = NullSink {
                candidate_window: &self.candidate_window,
            };
            self.controller
                .borrow_mut()
                .on_composition_terminated(&null_sink);
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// TsfSink — bridges Controller events to TSF edit sessions + candidate window.
// Borrows slots on self so multiple keys can reuse the same QBopomofoTextService.
// ---------------------------------------------------------------------------

struct TsfSink<'a> {
    context: ITfContext,
    tid: u32,
    comp_sink: ITfCompositionSink,
    composition: &'a RefCell<Option<ITfComposition>>,
    candidate_window: &'a RefCell<Option<CandidateWindow>>,
}

impl<'a> InputSink for TsfSink<'a> {
    fn update_preedit(&self, text: &str) -> Option<(i32, i32)> {
        let composition = self.composition.borrow_mut().take();
        let result = edit_session::request_edit_session(
            &self.context,
            self.tid,
            EditOp::UpdateComposition {
                text: text.to_string(),
                composition,
                sink: self.comp_sink.clone(),
            },
        );
        match result {
            Ok(Some(EditResult::Composition(new_comp, pos))) => {
                *self.composition.borrow_mut() = new_comp;
                pos
            }
            Ok(_) => None,
            Err(e) => {
                qb_dbg!("update_preedit: edit session failed: {:?}", e);
                None
            }
        }
    }

    fn commit_text(&self, text: &str) {
        let composition = self.composition.borrow_mut().take();
        if let Err(e) = edit_session::request_edit_session(
            &self.context,
            self.tid,
            EditOp::CommitText {
                text: text.to_string(),
                composition,
            },
        ) {
            qb_dbg!("commit_text: edit session failed: {:?}", e);
        }
        *self.composition.borrow_mut() = None;
        if let Some(cw) = self.candidate_window.borrow().as_ref() {
            cw.hide();
        }
    }

    fn end_preedit(&self) {
        let composition = self.composition.borrow_mut().take();
        if let Err(e) = edit_session::request_edit_session(
            &self.context,
            self.tid,
            EditOp::EndComposition { composition },
        ) {
            qb_dbg!("end_preedit: edit session failed: {:?}", e);
        }
        *self.composition.borrow_mut() = None;
        if let Some(cw) = self.candidate_window.borrow().as_ref() {
            cw.hide();
        }
    }

    fn show_candidates(
        &self,
        cands: &[String],
        selection_keys: &[char],
        highlight: usize,
        page_info: &str,
        caret_pos: Option<(i32, i32)>,
    ) {
        let mut cw_slot = self.candidate_window.borrow_mut();
        let Some(cw) = cw_slot.as_mut() else { return };
        cw.set_selection_keys(selection_keys);
        let (x, y) = caret_pos.unwrap_or_else(|| cw.last_position());
        cw.show(cands, highlight, page_info, x, y);
    }

    fn hide_candidates(&self) {
        if let Some(cw) = self.candidate_window.borrow().as_ref() {
            cw.hide();
        }
    }
}

// ---------------------------------------------------------------------------
// NullSink — used when no live TSF context is available (e.g. inside
// OnCompositionTerminated). Swallows preedit/commit; still hides candidates.
// ---------------------------------------------------------------------------

struct NullSink<'a> {
    candidate_window: &'a RefCell<Option<CandidateWindow>>,
}

impl<'a> InputSink for NullSink<'a> {
    fn update_preedit(&self, _text: &str) -> Option<(i32, i32)> {
        None
    }
    fn commit_text(&self, _text: &str) {}
    fn end_preedit(&self) {}
    fn show_candidates(
        &self,
        _cands: &[String],
        _selection_keys: &[char],
        _highlight: usize,
        _page_info: &str,
        _caret_pos: Option<(i32, i32)>,
    ) {
    }
    fn hide_candidates(&self) {
        if let Some(cw) = self.candidate_window.borrow().as_ref() {
            cw.hide();
        }
    }
}
