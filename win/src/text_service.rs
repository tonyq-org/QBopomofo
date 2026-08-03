//! TSF Text Service implementation.
//!
//! Thin COM wrapper around [`Controller`]. All input logic lives in
//! `controller.rs`; this module only implements the TSF COM interfaces,
//! bridges `Controller` events (via `InputSink`) to TSF edit sessions, and
//! owns the candidate window.
//!
//! Every COM method body is wrapped in `com_method_*!` so Rust panics cannot
//! cross the `extern "system"` FFI boundary.

use std::{
    cell::{Cell, RefCell},
    os::windows::ffi::OsStrExt,
};

use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfFnConfigure,
    ITfFnConfigure_Impl, ITfFunctionProvider, ITfFunctionProvider_Impl,
    ITfFunction_Impl, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfSourceSingle,
    ITfTextInputProcessor, ITfTextInputProcessor_Impl, ITfTextInputProcessorEx,
    ITfTextInputProcessorEx_Impl, ITfThreadMgr, TF_E_DISCONNECTED, TF_E_READONLY,
    TS_SD_READONLY,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{BSTR, BOOL, GUID, IUnknown, IUnknownImpl, Interface, PCWSTR, Ref, implement, w};

use crate::candidate_window::CandidateWindow;
use crate::controller::{Controller, EditOutcome, InputSink};
use crate::edit_session::{self, EditOp, EditResult};
use crate::key_event::translate_char;
use crate::{com_method, com_method_bool, com_method_unit};

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
    function_provider_advised: bool,
}

#[derive(Clone, Copy)]
struct TestedKey {
    vkey: u32,
    lparam: u32,
    ch: char,
}

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfFunctionProvider,
    ITfFnConfigure
)]
pub struct QBopomofoTextService {
    controller: RefCell<Controller>,
    state: RefCell<TsfState>,
    composition: RefCell<Option<ITfComposition>>,
    candidate_window: RefCell<Option<CandidateWindow>>,
    tested_key: RefCell<Option<TestedKey>>,
    self_terminating_composition: Cell<bool>,
    composition_termination_pending: Cell<bool>,
}

impl QBopomofoTextService {
    pub fn new() -> Self {
        Self {
            controller: RefCell::new(Controller::new()),
            state: RefCell::new(TsfState {
                thread_mgr: None,
                client_id: 0,
                activated: false,
                function_provider_advised: false,
            }),
            composition: RefCell::new(None),
            candidate_window: RefCell::new(None),
            tested_key: RefCell::new(None),
            self_terminating_composition: Cell::new(false),
            composition_termination_pending: Cell::new(false),
        }
    }
}

impl Default for QBopomofoTextService {
    fn default() -> Self {
        Self::new()
    }
}

impl QBopomofoTextService_Impl {
    fn clear_input_without_edit(&self) {
        // A read-only/disconnected context cannot accept any cleanup edit
        // session. Forget the stale COM composition handle and clear the
        // controller so it cannot keep consuming keys for pending text.
        *self.composition.borrow_mut() = None;
        *self.tested_key.borrow_mut() = None;
        let null_sink = NullSink {
            candidate_window: &self.candidate_window,
        };
        if let Ok(mut controller) = self.controller.try_borrow_mut() {
            controller.on_composition_terminated(&null_sink);
        } else {
            self.composition_termination_pending.set(true);
        }
    }

    fn apply_pending_composition_termination(&self) {
        if !self.composition_termination_pending.replace(false) {
            return;
        }
        // The callback can arrive re-entrantly while an update edit session is
        // still unwinding. Do not let that session's returned handle revive a
        // composition the host has already terminated.
        self.clear_input_without_edit();
    }
}

fn context_is_read_only(context: &ITfContext) -> bool {
    unsafe { context.GetStatus() }
        .is_ok_and(|status| status_flags_are_read_only(status.dwDynamicFlags))
}

fn status_flags_are_read_only(dynamic_flags: u32) -> bool {
    dynamic_flags & TS_SD_READONLY != 0
}

fn is_terminal_edit_error(error: &windows::core::Error) -> bool {
    matches!(error.code(), TF_E_READONLY | TF_E_DISCONNECTED)
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
            let (thread_mgr, client_id, function_provider_advised) = {
                let mut st = self.state.borrow_mut();
                (
                    st.thread_mgr.take(),
                    st.client_id,
                    std::mem::take(&mut st.function_provider_advised),
                )
            };

            if let Some(ref tm) = thread_mgr {
                if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                    let _ = unsafe { km.UnadviseKeyEventSink(client_id) };
                }
                if function_provider_advised
                    && let Ok(source) = tm.cast::<ITfSourceSingle>()
                {
                    let _ = unsafe {
                        source.UnadviseSingleSink(client_id, &ITfFunctionProvider::IID)
                    };
                }
            }

            *self.composition.borrow_mut() = None;
            *self.candidate_window.borrow_mut() = None;
            *self.tested_key.borrow_mut() = None;
            self.self_terminating_composition.set(false);
            self.composition_termination_pending.set(false);
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

            let function_provider_advised = if let Ok(source) = thread_mgr.cast::<ITfSourceSingle>()
            {
                let provider: ITfFunctionProvider = self.to_interface();
                match unsafe {
                    source.AdviseSingleSink(tid, &ITfFunctionProvider::IID, &provider)
                } {
                    Ok(()) => true,
                    Err(error) => {
                        qb_dbg!("ActivateEx: function provider registration failed: {:?}", error);
                        false
                    }
                }
            } else {
                false
            };

            let mut st = self.state.borrow_mut();
            st.thread_mgr = Some(thread_mgr);
            st.client_id = tid;
            st.activated = true;
            st.function_provider_advised = function_provider_advised;
            Ok(())
        })
    }
}

impl ITfFunctionProvider_Impl for QBopomofoTextService_Impl {
    fn GetType(&self) -> windows::core::Result<GUID> {
        com_method!("ITfFunctionProvider::GetType", {
            Ok(crate::com::CLSID_QBOPOMOFO)
        })
    }

    fn GetDescription(&self) -> windows::core::Result<BSTR> {
        com_method!("ITfFunctionProvider::GetDescription", {
            Ok(BSTR::from(crate::com::DISPLAY_NAME))
        })
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)] // COM trait uses REFGUID/REFIID raw pointers.
    fn GetFunction(
        &self,
        rguid: *const GUID,
        riid: *const GUID,
    ) -> windows::core::Result<IUnknown> {
        com_method!("ITfFunctionProvider::GetFunction", {
            if rguid.is_null() || riid.is_null() {
                return Err(windows::core::Error::from(E_INVALIDARG));
            }
            // COM validates these REFGUID/REFIID pointers before dispatching
            // into this implementation; the explicit null check above keeps
            // direct callers safe as well.
            let function_group = unsafe { copy_guid(rguid) };
            let requested_interface = unsafe { copy_guid(riid) };
            if function_group != GUID::zeroed() || requested_interface != ITfFnConfigure::IID {
                return Err(windows::core::Error::from(E_NOINTERFACE));
            }
            let configure: ITfFnConfigure = self.to_interface();
            configure.cast()
        })
    }
}

unsafe fn copy_guid(value: *const GUID) -> GUID {
    unsafe { value.read() }
}

impl ITfFunction_Impl for QBopomofoTextService_Impl {
    fn GetDisplayName(&self) -> windows::core::Result<BSTR> {
        com_method!("ITfFunction::GetDisplayName", {
            Ok(BSTR::from("Q注音設定"))
        })
    }
}

impl ITfFnConfigure_Impl for QBopomofoTextService_Impl {
    fn Show(
        &self,
        hwndparent: windows::Win32::Foundation::HWND,
        _langid: u16,
        _rguidprofile: *const GUID,
    ) -> windows::core::Result<()> {
        com_method_unit!("ITfFnConfigure::Show", {
            let settings_path = crate::com::dll_dir()
                .map(std::path::PathBuf::from)
                .map(|dir| dir.join("qbopomofo_settings.exe"))
                .ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
            let settings_wide: Vec<u16> = settings_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let directory_wide: Vec<u16> = settings_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let result = unsafe {
                ShellExecuteW(
                    Some(hwndparent),
                    w!("open"),
                    PCWSTR(settings_wide.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR(directory_wide.as_ptr()),
                    SW_SHOWNORMAL,
                )
            };
            if result.0 as usize <= 32 {
                return Err(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    format!(
                        "failed to launch settings (ShellExecute code {})",
                        result.0 as usize
                    ),
                ));
            }
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
        pic: Ref<ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        com_method_bool!("OnTestKeyDown", {
            if !self.state.borrow().activated {
                return Ok(BOOL(0));
            }
            if let Some(context) = pic.clone()
                && context_is_read_only(&context)
            {
                qb_dbg!("OnTestKeyDown: read-only context; releasing input state");
                self.clear_input_without_edit();
                return Ok(BOOL(0));
            }
            let vkey = wparam.0 as u32;
            let (shift, ctrl, caps) = get_modifiers();
            let ch = translate_char(vkey, lparam.0 as u32, shift);
            let eat = self
                .controller
                .borrow()
                .should_eat_key_down(vkey, ch, ctrl, caps);
            *self.tested_key.borrow_mut() = eat.then_some(TestedKey {
                vkey,
                lparam: lparam.0 as u32,
                ch,
            });
            qb_dbg!("OnTestKeyDown: vk={:#x} ch={:?} eat={}", vkey, ch, eat);
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
            let raw_lparam = lparam.0 as u32;
            let ch = self
                .tested_key
                .borrow_mut()
                .take()
                .filter(|tested| tested.vkey == vkey && tested.lparam == raw_lparam)
                .map_or_else(
                    || translate_char(vkey, raw_lparam, shift),
                    |tested| tested.ch,
                );
            qb_dbg!(
                "OnKeyDown: vk={:#x} ch={:?} shift={} ctrl={} caps={}",
                vkey,
                ch,
                shift,
                ctrl,
                caps
            );

            let Some(context) = pic.clone() else {
                return Ok(BOOL(0));
            };
            if context_is_read_only(&context) {
                qb_dbg!("OnKeyDown: read-only context; key passed through");
                self.clear_input_without_edit();
                return Ok(BOOL(0));
            }
            let tid = self.state.borrow().client_id;
            let comp_sink: ITfCompositionSink = self.to_interface();

            let sink = TsfSink {
                context,
                tid,
                comp_sink,
                composition: &self.composition,
                candidate_window: &self.candidate_window,
                self_terminating_composition: &self.self_terminating_composition,
            };
            let handled = {
                let mut controller = self.controller.borrow_mut();
                controller.on_key_down(vkey, ch, shift, ctrl, caps, &sink)
            };
            self.apply_pending_composition_termination();
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
                self_terminating_composition: &self.self_terminating_composition,
            };
            let handled = {
                let mut controller = self.controller.borrow_mut();
                controller.on_key_up(vkey, &sink)
            };
            self.apply_pending_composition_termination();
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
        pcomposition: Ref<ITfComposition>,
    ) -> windows::core::Result<()> {
        com_method_unit!("OnCompositionTerminated", {
            if self.self_terminating_composition.get() {
                return Ok(());
            }

            let Some(terminated) = pcomposition.clone() else {
                return Ok(());
            };
            let is_current = self
                .composition
                .borrow()
                .as_ref()
                .is_some_and(|current| current.as_raw() == terminated.as_raw());
            if !is_current {
                qb_dbg!("OnCompositionTerminated: ignored stale composition");
                return Ok(());
            }

            *self.composition.borrow_mut() = None;
            let null_sink = NullSink {
                candidate_window: &self.candidate_window,
            };
            if let Ok(mut controller) = self.controller.try_borrow_mut() {
                controller.on_composition_terminated(&null_sink);
            } else {
                self.composition_termination_pending.set(true);
            }
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
    self_terminating_composition: &'a Cell<bool>,
}

struct ScopedTerminationFlag<'a> {
    flag: &'a Cell<bool>,
    previous: bool,
}

impl<'a> ScopedTerminationFlag<'a> {
    fn new(flag: &'a Cell<bool>) -> Self {
        let previous = flag.replace(true);
        Self { flag, previous }
    }
}

impl Drop for ScopedTerminationFlag<'_> {
    fn drop(&mut self) {
        self.flag.set(self.previous);
    }
}

impl<'a> InputSink for TsfSink<'a> {
    fn update_preedit(
        &self,
        text: &str,
        cursor_utf16: usize,
        needs_caret_position: bool,
        update_selection: bool,
    ) -> EditOutcome<Option<(i32, i32)>> {
        if context_is_read_only(&self.context) {
            qb_dbg!("update_preedit: context is read-only");
            *self.composition.borrow_mut() = None;
            return EditOutcome::Rejected;
        }
        let composition = self.composition.borrow().clone();
        let _termination_guard = text
            .is_empty()
            .then(|| ScopedTerminationFlag::new(self.self_terminating_composition));
        let result = edit_session::request_edit_session(
            &self.context,
            self.tid,
            EditOp::UpdateComposition {
                text: text.to_string(),
                cursor_utf16,
                needs_caret_position,
                update_selection,
                composition,
                sink: self.comp_sink.clone(),
            },
        );
        match result {
            Ok(EditResult::Composition(new_comp, pos)) => {
                *self.composition.borrow_mut() = new_comp;
                EditOutcome::Applied(pos)
            }
            Err(e) => {
                qb_dbg!("update_preedit: edit session failed: {:?}", e);
                if is_terminal_edit_error(&e) {
                    *self.composition.borrow_mut() = None;
                }
                EditOutcome::Rejected
            }
        }
    }

    fn commit_text(&self, text: &str) -> EditOutcome<()> {
        if context_is_read_only(&self.context) {
            qb_dbg!("commit_text: context is read-only; rejecting without retry");
            *self.composition.borrow_mut() = None;
            return EditOutcome::Rejected;
        }
        let composition = self.composition.borrow().clone();
        let _termination_guard = ScopedTerminationFlag::new(self.self_terminating_composition);
        match edit_session::request_edit_session(
            &self.context,
            self.tid,
            EditOp::CommitText {
                text: text.to_string(),
                composition,
            },
        ) {
            Ok(EditResult::Composition(None, _)) => {
                *self.composition.borrow_mut() = None;
                if let Some(cw) = self.candidate_window.borrow().as_ref() {
                    cw.hide();
                }
                EditOutcome::Applied(())
            }
            Ok(EditResult::Composition(Some(comp), _)) => {
                *self.composition.borrow_mut() = Some(comp);
                qb_dbg!("commit_text: edit session unexpectedly kept composition");
                EditOutcome::Retryable
            }
            Err(e) => {
                qb_dbg!("commit_text: edit session failed: {:?}", e);
                if is_terminal_edit_error(&e) {
                    *self.composition.borrow_mut() = None;
                    EditOutcome::Rejected
                } else {
                    EditOutcome::Retryable
                }
            }
        }
    }

    fn edit_context_id(&self) -> usize {
        self.context
            .cast::<IUnknown>()
            .map_or(self.context.as_raw() as usize, |identity| {
                identity.as_raw() as usize
            })
    }

    fn end_preedit(&self) -> bool {
        let composition = self.composition.borrow().clone();
        let _termination_guard = ScopedTerminationFlag::new(self.self_terminating_composition);
        match edit_session::request_edit_session(
            &self.context,
            self.tid,
            EditOp::EndComposition { composition },
        ) {
            Ok(EditResult::Composition(None, _)) => {
                *self.composition.borrow_mut() = None;
                if let Some(cw) = self.candidate_window.borrow().as_ref() {
                    cw.hide();
                }
                true
            }
            Ok(EditResult::Composition(Some(comp), _)) => {
                *self.composition.borrow_mut() = Some(comp);
                false
            }
            Err(e) => {
                qb_dbg!("end_preedit: edit session failed: {:?}", e);
                if is_terminal_edit_error(&e) {
                    *self.composition.borrow_mut() = None;
                }
                false
            }
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
    fn update_preedit(
        &self,
        _text: &str,
        _cursor_utf16: usize,
        _needs_caret_position: bool,
        _update_selection: bool,
    ) -> EditOutcome<Option<(i32, i32)>> {
        EditOutcome::Applied(None)
    }
    fn commit_text(&self, _text: &str) -> EditOutcome<()> {
        EditOutcome::Applied(())
    }
    fn end_preedit(&self) -> bool {
        true
    }
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

#[cfg(test)]
mod tests {
    use super::{QBopomofoTextService, status_flags_are_read_only};
    use crate::com::CLSID_QBOPOMOFO;
    use windows::Win32::UI::TextServices::{
        ITfFnConfigure, ITfFunctionProvider, TS_SD_LOADING, TS_SD_READONLY,
    };
    use windows::core::{GUID, IUnknown, Interface};

    #[test]
    fn exposes_tsf_configuration_function() {
        let service = QBopomofoTextService::new();
        let unknown: IUnknown = service.into();
        let provider: ITfFunctionProvider = unknown.cast().expect("function provider");

        assert_eq!(unsafe { provider.GetType().unwrap() }, CLSID_QBOPOMOFO);
        let function = unsafe {
            provider
                .GetFunction(&GUID::zeroed(), &ITfFnConfigure::IID)
                .expect("configure function")
        };
        assert!(function.cast::<ITfFnConfigure>().is_ok());
    }

    #[test]
    fn recognizes_read_only_context_status() {
        assert!(!status_flags_are_read_only(0));
        assert!(!status_flags_are_read_only(TS_SD_LOADING));
        assert!(status_flags_are_read_only(TS_SD_READONLY));
        assert!(status_flags_are_read_only(TS_SD_LOADING | TS_SD_READONLY));
    }
}
