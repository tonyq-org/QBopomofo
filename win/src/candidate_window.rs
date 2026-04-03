//! Candidate window for QBopomofo.
//!
//! A floating popup window that displays candidate characters for selection.
//! Uses Win32 API (CreateWindowExW + GDI) for rendering. The window is
//! WS_POPUP + WS_EX_TOPMOST + WS_EX_TOOLWINDOW + WS_EX_NOACTIVATE so it
//! floats above all windows, doesn't appear in taskbar, and doesn't steal focus.

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, DeleteObject, EndPaint, FillRect, GetDC, GetTextExtentPoint32W,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, HFONT, LOGFONTW,
    PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, GetWindowLongPtrW,
    MoveWindow, RegisterClassExW, SetWindowLongPtrW, ShowWindow,
    CS_DROPSHADOW, CW_USEDEFAULT, GWLP_USERDATA, HMENU, SM_CXSCREEN, SM_CYSCREEN,
    SW_HIDE, SW_SHOWNA, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::com::dll_instance;

static CLASS_REGISTERED: OnceLock<bool> = OnceLock::new();

const WINDOW_CLASS: &str = "QBopomofo_CandidateWindow";
const FONT_SIZE: i32 = 20;
const PADDING: i32 = 8;
const ROW_HEIGHT: i32 = 28;
const MIN_WIDTH: i32 = 120;

/// Shared paint data stored on the heap; a raw pointer is placed in GWLP_USERDATA
/// so the static WndProc can reach it during WM_PAINT.
struct PaintData {
    candidates: Vec<String>,
    selection_keys: Vec<char>,
    highlighted: usize,
    page_info: String,
    font: HFONT,
}

/// Candidate window state.
pub struct CandidateWindow {
    hwnd: HWND,
    /// Heap-allocated paint data whose raw pointer lives in GWLP_USERDATA.
    paint_data: *mut PaintData,
    /// Last shown screen position.
    last_pos: (i32, i32),
}

impl CandidateWindow {
    pub fn new() -> Self {
        ensure_class_registered();

        let font = create_font(FONT_SIZE);

        // Allocate paint data on heap
        let paint_data = Box::into_raw(Box::new(PaintData {
            candidates: Vec::new(),
            selection_keys: "1234567890".chars().collect(),
            highlighted: 0,
            page_info: String::new(),
            font,
        }));

        let hwnd = unsafe {
            let class_w = to_wide_null(WINDOW_CLASS);
            let title_w = to_wide_null("");
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(class_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                WS_POPUP,
                CW_USEDEFAULT, CW_USEDEFAULT, 200, 100,
                Some(HWND::default()),
                Some(HMENU::default()),
                Some(dll_instance().into()),
                None,
            )
        };

        let hwnd = hwnd.unwrap_or_default();

        // Store paint data pointer in GWLP_USERDATA
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, paint_data as isize); }

        Self { hwnd, paint_data, last_pos: (100, 100) }
    }

    pub fn set_selection_keys(&mut self, keys: &[char]) {
        unsafe { (*self.paint_data).selection_keys = keys.to_vec(); }
    }

    /// Update candidates and show the window near the given screen coordinates.
    pub fn show(&mut self, candidates: &[String], highlighted: usize, page_info: &str, x: i32, y: i32) {
        unsafe {
            (*self.paint_data).candidates = candidates.to_vec();
            (*self.paint_data).highlighted = highlighted;
            (*self.paint_data).page_info = page_info.to_string();
        }
        self.last_pos = (x, y);

        let (w, h) = self.calc_size();
        let (fx, fy) = clamp_to_screen(x, y + 24, w, h);

        unsafe {
            let _ = MoveWindow(self.hwnd, fx, fy, w, h, true);
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
            // Force synchronous repaint — host app's message pump may not
            // dispatch WM_PAINT to our window promptly. We invalidate then
            // send WM_PAINT directly via SendMessageW.
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, true);
            let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                self.hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0)),
            );
        }
    }

    /// Hide the candidate window.
    pub fn hide(&self) {
        unsafe { let _ = ShowWindow(self.hwnd, SW_HIDE); }
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() }
    }

    pub fn highlighted_index(&self) -> usize {
        unsafe { (*self.paint_data).highlighted }
    }

    #[allow(dead_code)]
    pub fn set_highlighted(&mut self, index: usize) {
        unsafe { (*self.paint_data).highlighted = index; }
        self.invalidate();
    }

    /// Move highlight to next candidate (no wrap).
    pub fn highlight_next(&mut self) {
        let pd = unsafe { &mut *self.paint_data };
        if pd.highlighted + 1 < pd.candidates.len() {
            pd.highlighted += 1;
            self.invalidate();
        }
    }

    /// Move highlight to previous candidate (no wrap).
    pub fn highlight_previous(&mut self) {
        let pd = unsafe { &mut *self.paint_data };
        if pd.highlighted > 0 {
            pd.highlighted -= 1;
            self.invalidate();
        }
    }

    /// Number of selection keys configured.
    pub fn selection_keys_count(&self) -> usize {
        unsafe { (*self.paint_data).selection_keys.len() }
    }

    /// Get selection keys as Vec<char>.
    pub fn get_selection_keys(&self) -> Vec<char> {
        unsafe { (*self.paint_data).selection_keys.clone() }
    }

    /// Last shown position (for re-showing without new caret data).
    pub fn last_position(&self) -> (i32, i32) {
        self.last_pos
    }

    fn invalidate(&self) {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, true);
            let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                self.hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0)),
            );
        }
    }

    fn calc_size(&self) -> (i32, i32) {
        let pd = unsafe { &*self.paint_data };
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let old_font = unsafe { SelectObject(hdc, pd.font.into()) };

        let mut max_w = MIN_WIDTH;
        for (i, cand) in pd.candidates.iter().enumerate() {
            let key_ch = pd.selection_keys.get(i).copied().unwrap_or(' ');
            let label = format!("{}. {}", key_ch, cand);
            let label_w: Vec<u16> = label.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            unsafe { let _ = GetTextExtentPoint32W(hdc, &label_w, &mut size); }
            let w = size.cx + PADDING * 2;
            if w > max_w { max_w = w; }
        }

        if !pd.page_info.is_empty() {
            let info_w: Vec<u16> = pd.page_info.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            unsafe { let _ = GetTextExtentPoint32W(hdc, &info_w, &mut size); }
            let w = size.cx + PADDING * 2;
            if w > max_w { max_w = w; }
        }

        unsafe { SelectObject(hdc, old_font); }
        unsafe { let _ = ReleaseDC(Some(self.hwnd), hdc); }

        let rows = pd.candidates.len() as i32 + if pd.page_info.is_empty() { 0 } else { 1 };
        let h = rows * ROW_HEIGHT + PADDING * 2;
        (max_w, h)
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        unsafe {
            // Clear USERDATA before destroying
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
            // Reclaim the heap allocation
            let pd = Box::from_raw(self.paint_data);
            let _ = DeleteObject(pd.font.into());
        }
    }
}

// ---------------------------------------------------------------------------
// Window class registration and WndProc
// ---------------------------------------------------------------------------

fn ensure_class_registered() {
    CLASS_REGISTERED.get_or_init(|| {
        let class_w = to_wide_null(WINDOW_CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(candidate_wnd_proc),
            hInstance: dll_instance().into(),
            lpszClassName: PCWSTR(class_w.as_ptr()),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc); }
        true
    });
}

unsafe extern "system" fn candidate_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if ptr != 0 {
                let pd = unsafe { &*(ptr as *const PaintData) };
                paint_candidates(hwnd, pd);
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Paint candidate list using GDI. Called from WndProc during WM_PAINT.
fn paint_candidates(hwnd: HWND, pd: &PaintData) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

    // White background
    let bg_brush = unsafe {
        windows::Win32::Graphics::Gdi::CreateSolidBrush(
            windows::Win32::Foundation::COLORREF(0x00FFFFFF),
        )
    };
    unsafe { FillRect(hdc, &ps.rcPaint, bg_brush); }
    unsafe { let _ = DeleteObject(bg_brush.into()); }

    let old_font = unsafe { SelectObject(hdc, pd.font.into()) };
    unsafe { SetBkMode(hdc, TRANSPARENT); }

    let mut y = PADDING;
    for (i, cand) in pd.candidates.iter().enumerate() {
        let key_ch = pd.selection_keys.get(i).copied().unwrap_or(' ');
        let label = format!("{}. {}", key_ch, cand);
        let label_w: Vec<u16> = label.encode_utf16().collect();

        if i == pd.highlighted {
            let hl_brush = unsafe {
                windows::Win32::Graphics::Gdi::CreateSolidBrush(
                    windows::Win32::Foundation::COLORREF(0x00FFD0A0),
                )
            };
            let hl_rect = RECT {
                left: 0, top: y, right: ps.rcPaint.right, bottom: y + ROW_HEIGHT,
            };
            unsafe { FillRect(hdc, &hl_rect, hl_brush); }
            unsafe { let _ = DeleteObject(hl_brush.into()); }
        }

        unsafe { SetTextColor(hdc, windows::Win32::Foundation::COLORREF(0x00000000)); }
        unsafe { let _ = TextOutW(hdc, PADDING, y + 2, &label_w); }
        y += ROW_HEIGHT;
    }

    // Page info
    if !pd.page_info.is_empty() {
        let info_w: Vec<u16> = pd.page_info.encode_utf16().collect();
        unsafe {
            SetTextColor(hdc, windows::Win32::Foundation::COLORREF(0x00808080));
            let _ = TextOutW(hdc, PADDING, y + 2, &info_w);
        }
    }

    unsafe { SelectObject(hdc, old_font); }
    unsafe { let _ = EndPaint(hwnd, &ps); }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_font(size: i32) -> HFONT {
    let mut lf = LOGFONTW::default();
    lf.lfHeight = -size;
    lf.lfWeight = 400;
    lf.lfCharSet = windows::Win32::Graphics::Gdi::FONT_CHARSET(136); // CHINESEBIG5_CHARSET
    let face = "Microsoft JhengHei";
    let face_w: Vec<u16> = face.encode_utf16().collect();
    for (i, &ch) in face_w.iter().enumerate() {
        if i < 32 { lf.lfFaceName[i] = ch; }
    }
    unsafe { CreateFontIndirectW(&lf) }
}

fn clamp_to_screen(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    let scr_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let scr_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let fx = if x + w > scr_w { scr_w - w } else { x };
    let fy = if y + h > scr_h { y - h - 30 } else { y }; // flip above if off-screen
    (fx.max(0), fy.max(0))
}
