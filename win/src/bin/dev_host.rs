//! Dev harness with GUI — NOT a real TSF host.
//!
//! Drives [`Controller`] directly from a plain Win32 window's message pump,
//! routing commits into an `EDIT` child control and candidates into our
//! [`CandidateWindow`]. Preedit is surfaced in the window title (visual
//! only, no TSF composition).
//!
//! Why bypass TSF:
//!   - `ITfKeystrokeMgr::AdviseKeyEventSink` requires the TIP's `tfClientId`
//!     to be the one currently being activated by the TSF manager. That only
//!     happens through `ITfInputProcessorProfiles::Register` +
//!     `ActivateLanguageProfile`, both of which write HKLM and need admin.
//!   - For daily iteration on controller logic, candidate rendering, HiDPI
//!     / dark mode / multi-monitor — we only need keys → Controller →
//!     CandidateWindow. No admin, no regsvr32, no sandbox.
//!
//! For *real* TSF integration testing (edit sessions, composition lifecycle,
//! cross-app behaviour), use `install.ps1` + actual apps.

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::Input::Ime::ImmDisableIME;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    LoadCursorW, MoveWindow, PostQuitMessage, RegisterClassExW, SendMessageW, SetWindowTextW,
    ShowWindow, TranslateMessage, CW_USEDEFAULT, HMENU, IDC_ARROW, MSG, SW_SHOW, WM_DESTROY,
    WM_KEYDOWN, WM_KEYUP, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    WS_VSCROLL,
};

use qbopomofo_tip::candidate_window::CandidateWindow;
use qbopomofo_tip::controller::{Controller, InputSink};
use qbopomofo_tip::key_event::translate_char;

const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_CAPITAL: i32 = 0x14;
const VK_PROCESSKEY: u32 = 0xE5;

// EDIT / RichEdit control messages we care about.
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;

fn get_modifiers() -> (bool, bool, bool) {
    let shift = (unsafe { GetKeyState(VK_SHIFT) } & 0x8000u16 as i16) != 0;
    let ctrl = (unsafe { GetKeyState(VK_CONTROL) } & 0x8000u16 as i16) != 0;
    let caps = (unsafe { GetKeyState(VK_CAPITAL) } & 1) != 0;
    (shift, ctrl, caps)
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Sink: bridges Controller events to our window
// ---------------------------------------------------------------------------

/// Tracks the UTF-16 range of the currently-displayed preedit text in the
/// EDIT control. Each `update_preedit` replaces this range; `commit_text`
/// also replaces it (with committed text); `end_preedit` just clears the
/// tracker without touching the edit contents (the commit already finalized
/// the text).
#[derive(Clone, Copy)]
struct PreeditRange {
    start: u32,
    len: u32,
}

struct GuiSink<'a> {
    parent: HWND,
    edit: HWND,
    candidate_window: &'a RefCell<CandidateWindow>,
    preedit: &'a RefCell<Option<PreeditRange>>,
}

impl<'a> GuiSink<'a> {
    /// Select the current preedit range (or current caret if no preedit) and
    /// replace it with `text`. Returns the start position so callers can
    /// update tracking.
    fn replace_preedit_with(&self, text: &str) -> u32 {
        let mut pr = self.preedit.borrow_mut();
        let start = match *pr {
            Some(r) => {
                unsafe {
                    SendMessageW(
                        self.edit,
                        EM_SETSEL,
                        Some(WPARAM(r.start as usize)),
                        Some(LPARAM((r.start + r.len) as isize)),
                    );
                }
                r.start
            }
            None => {
                // No active preedit — capture current caret position.
                // EM_GETSEL packs start in the low WORD (16 bits) and end in
                // the high WORD. Mask to get just the start.
                let sel = unsafe {
                    SendMessageW(self.edit, EM_GETSEL, None, None)
                };
                (sel.0 as u32) & 0xFFFF
            }
        };
        let w = to_wide_null(text);
        unsafe {
            SendMessageW(
                self.edit,
                EM_REPLACESEL,
                Some(WPARAM(1)),
                Some(LPARAM(w.as_ptr() as isize)),
            );
        }
        let len = text.encode_utf16().count() as u32;
        if text.is_empty() {
            *pr = None;
        } else {
            *pr = Some(PreeditRange { start, len });
        }
        start
    }
}

impl<'a> InputSink for GuiSink<'a> {
    fn update_preedit(&self, text: &str) -> Option<(i32, i32)> {
        self.replace_preedit_with(text);
        // Also reflect in the title for visibility while testing.
        let title = if text.is_empty() {
            String::from("QBopomofo dev_host")
        } else {
            format!("QBopomofo dev_host — preedit: {}", text)
        };
        let w = to_wide_null(&title);
        unsafe {
            let _ = SetWindowTextW(self.parent, PCWSTR(w.as_ptr()));
        }
        caret_screen_pos(self.edit)
    }

    fn commit_text(&self, text: &str) {
        // Replace preedit (if any) with the committed text and clear tracker.
        self.replace_preedit_with(text);
        *self.preedit.borrow_mut() = None;
    }

    fn end_preedit(&self) {
        *self.preedit.borrow_mut() = None;
        unsafe {
            let _ = SetWindowTextW(self.parent, w!("QBopomofo dev_host"));
        }
    }

    fn show_candidates(
        &self,
        cands: &[String],
        selection_keys: &[char],
        highlight: usize,
        page_info: &str,
        caret: Option<(i32, i32)>,
    ) {
        let (x, y) = caret
            .or_else(|| caret_screen_pos(self.edit))
            .unwrap_or((100, 100));
        let mut cw = self.candidate_window.borrow_mut();
        cw.set_selection_keys(selection_keys);
        cw.show(cands, highlight, page_info, x, y);
    }

    fn hide_candidates(&self) {
        self.candidate_window.borrow().hide();
    }
}

/// Approximate screen position for the candidate window anchor — uses the
/// top-left of the edit control. Real TSF would report the actual caret.
fn caret_screen_pos(edit: HWND) -> Option<(i32, i32)> {
    let mut rect = windows::Win32::Foundation::RECT::default();
    if unsafe { GetClientRect(edit, &mut rect).is_err() } {
        return None;
    }
    let mut pt = windows::Win32::Foundation::POINT {
        x: rect.left + 8,
        y: rect.bottom - 24,
    };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(edit, &mut pt);
    }
    Some((pt.x, pt.y))
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> windows::core::Result<()> {
    // dict path: let the Controller figure it out via CHEWING_PATH or the dll_dir
    // fallback. Rust logging respects RUST_LOG.
    eprintln!("[info] dev_host starting");

    // Disable IMM/TSF for the entire process. If a QBopomofo TIP is installed
    // system-wide (via install.ps1), the EDIT control will otherwise route
    // keys through the system TIP before we see them — WM_KEYDOWN arrives
    // with vkey=VK_PROCESSKEY(0xE5) and our Controller gets bypassed. Calling
    // ImmDisableIME(-1) before any window is created prevents the IME/TSF
    // from attaching to this process.
    unsafe { let _ = ImmDisableIME(u32::MAX); }

    let (hwnd, edit_hwnd) = create_main_window()?;
    eprintln!("[info] window hwnd={:?} edit={:?}", hwnd, edit_hwnd);

    let mut controller = Controller::new();
    let dict_path = std::env::var("CHEWING_PATH")
        .ok()
        .or_else(qbopomofo_tip::com::dll_dir);
    eprintln!("[info] dict_path={:?}", dict_path);
    controller.activate(dict_path);

    let mut cw = CandidateWindow::new();
    cw.set_selection_keys(controller.selection_keys());
    let candidate_window = RefCell::new(cw);

    unsafe {
        let _ = SetFocus(Some(edit_hwnd));
    }

    // Intercept WM_KEYDOWN before DispatchMessage routes it to the EDIT
    // control. If Controller handles the key, swallow it; otherwise fall
    // through to normal processing.
    let preedit = RefCell::new(None);
    let sink = GuiSink {
        parent: hwnd,
        edit: edit_hwnd,
        candidate_window: &candidate_window,
        preedit: &preedit,
    };
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if msg.message == WM_KEYDOWN {
                let vkey = msg.wParam.0 as u32;
                if vkey == VK_PROCESSKEY {
                    eprintln!(
                        "[warn] VK_PROCESSKEY received — another IME is intercepting keys. \
                         Unregister the QBopomofo TIP (regsvr32 /u qbopomofo_tip.dll) \
                         and relaunch."
                    );
                    continue;
                }
                let lparam = msg.lParam.0 as u32;
                let (shift, ctrl, caps) = get_modifiers();
                let ch = translate_char(vkey, lparam, shift);
                let handled =
                    controller.on_key_down(vkey, ch, shift, ctrl, caps, &sink);
                if handled {
                    continue;
                }
            }
            if msg.message == WM_KEYUP {
                let vkey = msg.wParam.0 as u32;
                // Feed key-up to controller so shift-toggle (SmartToggle) can
                // fire on release — without this, English mode never exits.
                if controller.on_key_up(vkey, &sink) {
                    continue;
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    controller.deactivate();
    Ok(())
}

// ---------------------------------------------------------------------------
// Win32 window plumbing
// ---------------------------------------------------------------------------

fn create_main_window() -> windows::core::Result<(HWND, HWND)> {
    let hinstance = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?
    };

    let class_name = w!("QBopomofo_DevHost");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(host_wnd_proc),
        hInstance: hinstance.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class_name,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };

    let title = w!("QBopomofo dev_host");
    let hwnd = unsafe {
        CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            class_name,
            title,
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            720,
            480,
            Some(HWND::default()),
            Some(HMENU::default()),
            Some(hinstance.into()),
            None,
        )?
    };

    let edit_class = w!("EDIT");
    let empty = w!("");
    let edit_hwnd = unsafe {
        CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            edit_class,
            empty,
            WS_CHILD
                | WS_VISIBLE
                | WS_VSCROLL
                | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                    // ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN
                    0x0004 | 0x0040 | 0x1000,
                ),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            600,
            400,
            Some(hwnd),
            Some(HMENU(1 as *mut _)),
            Some(hinstance.into()),
            None,
        )?
    };
    resize_child(hwnd, edit_hwnd);
    apply_edit_font(edit_hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            edit_hwnd.0 as isize,
        );
    }
    Ok((hwnd, edit_hwnd))
}

/// Set a normal-weight Microsoft JhengHei UI font on the EDIT control. The
/// default EDIT font on a Chinese Windows locale tends to render CJK at a
/// heavier weight that's hard to read while testing.
fn apply_edit_font(edit: HWND) {
    use windows::Win32::Graphics::Gdi::{
        CreateFontW, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DEFAULT_PITCH,
        FF_DONTCARE, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, FW_NORMAL,
    };
    const WM_SETFONT: u32 = 0x0030;
    let face = to_wide_null("Microsoft JhengHei UI");
    let pitch_family = (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32;
    let hfont = unsafe {
        CreateFontW(
            -18,
            0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            pitch_family,
            PCWSTR(face.as_ptr()),
        )
    };
    if !hfont.is_invalid() {
        unsafe {
            SendMessageW(
                edit,
                WM_SETFONT,
                Some(WPARAM(hfont.0 as usize)),
                Some(LPARAM(1)),
            );
        }
    }
}

fn resize_child(parent: HWND, child: HWND) {
    let mut rect = windows::Win32::Foundation::RECT::default();
    if unsafe { GetClientRect(parent, &mut rect).is_ok() } {
        let _ = unsafe {
            MoveWindow(
                child,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                true,
            )
        };
    }
}

unsafe extern "system" fn host_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            let child = unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                )
            };
            if child != 0 {
                resize_child(hwnd, HWND(child as *mut _));
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
