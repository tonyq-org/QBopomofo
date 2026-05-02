//! Candidate window for QBopomofo.
//!
//! A floating popup that displays bopomofo candidates for selection. Visual
//! goals (aligned with Mac `CandidatePanel.swift`):
//!   - Multi-monitor aware (`MonitorFromPoint` + `GetMonitorInfoW`).
//!   - HiDPI scaling (`GetDpiForWindow`) — font size + padding + row height.
//!   - Light / dark theme following `AppsUseLightTheme`.
//!   - Rounded corners via `CreateRoundRectRgn` + `SetWindowRgn`.
//!   - Double-buffered, jitter-free paint (`InvalidateRect` + `UpdateWindow`).
//!
//! Safety:
//!   - `PaintData` is stored behind a raw pointer in `GWLP_USERDATA`. The
//!     WndProc validates a magic number before dereferencing to guard against
//!     stale window messages after destruction.

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, FrameRect,
    GetDC, GetMonitorInfoW, GetTextExtentPoint32W, InvalidateRect, MonitorFromPoint, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TextOutW, UpdateWindow, HBITMAP, HBRUSH,
    HDC, HFONT, LOGFONTW, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, IsWindowVisible,
    MoveWindow, RegisterClassExW, SetWindowLongPtrW, ShowWindow, CS_DROPSHADOW, CW_USEDEFAULT,
    GWLP_USERDATA, HMENU, SW_HIDE, SW_SHOWNA, WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::com::dll_instance;

static CLASS_REGISTERED: OnceLock<bool> = OnceLock::new();

const WINDOW_CLASS: &str = "QBopomofo_CandidateWindow";

/// Base metrics at 96 DPI. Scaled by `dpi / 96` at paint time.
const BASE_FONT_PT: i32 = 14;
const BASE_ROW_HEIGHT: i32 = 28;
const BASE_PADDING: i32 = 8;
const BASE_MIN_WIDTH: i32 = 140;
const BASE_CORNER_RADIUS: i32 = 8;

/// Magic number written into `PaintData` so WndProc can reject stale pointers.
const PAINT_DATA_MAGIC: u32 = 0x51_50_4D_4F; // "QPMO" little-endian-ish

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Theme {
    bg: COLORREF,
    fg: COLORREF,
    fg_dim: COLORREF,
    border: COLORREF,
    highlight_bg: COLORREF,
    highlight_fg: COLORREF,
}

impl Theme {
    fn light() -> Self {
        Self {
            bg: COLORREF(0x00F7_F7F7),
            fg: COLORREF(0x0020_2020),
            fg_dim: COLORREF(0x0080_8080),
            border: COLORREF(0x00C0_C0C0),
            highlight_bg: COLORREF(0x00F0_B070),
            highlight_fg: COLORREF(0x0020_2020),
        }
    }

    fn dark() -> Self {
        Self {
            bg: COLORREF(0x0028_2828),
            fg: COLORREF(0x00F0_F0F0),
            fg_dim: COLORREF(0x0090_9090),
            border: COLORREF(0x0040_4040),
            highlight_bg: COLORREF(0x0080_5020),
            highlight_fg: COLORREF(0x00FF_FFFF),
        }
    }

    fn current() -> Self {
        if is_dark_mode() { Self::dark() } else { Self::light() }
    }
}

fn is_dark_mode() -> bool {
    // Read HKCU\...\Themes\Personalize\AppsUseLightTheme (DWORD).
    // 0 = dark, 1 = light. Default to light on read failure.
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };
    let sub = to_wide_null(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
    );
    let name = to_wide_null("AppsUseLightTheme");
    let mut hkey = HKEY::default();
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
    };
    if open.0 != 0 {
        return false;
    }
    let mut data = [0u8; 4];
    let mut size = 4u32;
    let q = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(hkey) };
    if q.0 != 0 {
        return false;
    }
    u32::from_le_bytes(data) == 0
}

// ---------------------------------------------------------------------------
// Paint data — heap-allocated, raw ptr in GWLP_USERDATA
// ---------------------------------------------------------------------------

struct PaintData {
    magic: u32,
    candidates: Vec<String>,
    selection_keys: Vec<char>,
    highlighted: usize,
    page_info: String,
    font: HFONT,
    dpi: u32,
}

impl PaintData {
    fn new(dpi: u32) -> Self {
        Self {
            magic: PAINT_DATA_MAGIC,
            candidates: Vec::new(),
            selection_keys: "1234567890".chars().collect(),
            highlighted: 0,
            page_info: String::new(),
            font: create_font(scale(BASE_FONT_PT, dpi)),
            dpi,
        }
    }

    fn ensure_font(&mut self, new_dpi: u32) {
        if new_dpi != self.dpi {
            unsafe { let _ = DeleteObject(self.font.into()); }
            self.font = create_font(scale(BASE_FONT_PT, new_dpi));
            self.dpi = new_dpi;
        }
    }
}

fn scale(base: i32, dpi: u32) -> i32 {
    (base * dpi as i32) / 96
}

// ---------------------------------------------------------------------------
// CandidateWindow
// ---------------------------------------------------------------------------

pub struct CandidateWindow {
    hwnd: HWND,
    paint_data: *mut PaintData,
    last_pos: (i32, i32),
}

impl CandidateWindow {
    pub fn new() -> Self {
        ensure_class_registered();

        // Initial DPI — we'll refresh per-show in case the window moves
        // to a different monitor.
        let paint_data = Box::into_raw(Box::new(PaintData::new(96)));

        let hwnd = unsafe {
            let class_w = to_wide_null(WINDOW_CLASS);
            let title_w = to_wide_null("");
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(class_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                WS_POPUP,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                200,
                100,
                Some(HWND::default()),
                Some(HMENU::default()),
                Some(dll_instance().into()),
                None,
            )
        };
        let hwnd = hwnd.unwrap_or_default();

        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, paint_data as isize);
        }

        Self {
            hwnd,
            paint_data,
            last_pos: (100, 100),
        }
    }

    pub fn set_selection_keys(&mut self, keys: &[char]) {
        if self.paint_data.is_null() {
            return;
        }
        unsafe { (*self.paint_data).selection_keys = keys.to_vec() };
    }

    pub fn show(
        &mut self,
        candidates: &[String],
        highlighted: usize,
        page_info: &str,
        x: i32,
        y: i32,
    ) {
        if self.paint_data.is_null() {
            return;
        }

        // Refresh DPI for this monitor & reallocate font if needed.
        let dpi = current_dpi(self.hwnd);
        unsafe {
            let pd = &mut *self.paint_data;
            pd.ensure_font(dpi);
            pd.candidates = candidates.to_vec();
            pd.highlighted = highlighted.min(candidates.len().saturating_sub(1));
            pd.page_info = page_info.to_string();
        }
        self.last_pos = (x, y);

        let (w, h) = self.calc_size(dpi);
        let (fx, fy) = clamp_to_monitor(x, y + scale(24, dpi), w, h);

        // Apply rounded-corner region.
        unsafe {
            let radius = scale(BASE_CORNER_RADIUS, dpi);
            let rgn = CreateRoundRectRgn(0, 0, w, h, radius, radius);
            let _ = SetWindowRgn(self.hwnd, Some(rgn), true);
            // SetWindowRgn takes ownership of rgn — do not DeleteObject it.
        }

        unsafe {
            let _ = MoveWindow(self.hwnd, fx, fy, w, h, true);
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
            let _ = InvalidateRect(Some(self.hwnd), None, true);
            let _ = UpdateWindow(self.hwnd);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    pub fn last_position(&self) -> (i32, i32) {
        self.last_pos
    }

    fn calc_size(&self, dpi: u32) -> (i32, i32) {
        if self.paint_data.is_null() {
            return (scale(BASE_MIN_WIDTH, dpi), scale(BASE_ROW_HEIGHT, dpi));
        }
        let pd = unsafe { &*self.paint_data };
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let old_font = unsafe { SelectObject(hdc, pd.font.into()) };

        let padding = scale(BASE_PADDING, dpi);
        let row_h = scale(BASE_ROW_HEIGHT, dpi);
        let min_w = scale(BASE_MIN_WIDTH, dpi);

        let mut max_w = min_w;
        for (i, cand) in pd.candidates.iter().enumerate() {
            let key_ch = pd.selection_keys.get(i).copied().unwrap_or(' ');
            let label = format!("{}. {}", key_ch, cand);
            let label_w: Vec<u16> = label.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            unsafe {
                let _ = GetTextExtentPoint32W(hdc, &label_w, &mut size);
            }
            let w = size.cx + padding * 2;
            if w > max_w {
                max_w = w;
            }
        }

        if !pd.page_info.is_empty() {
            let info_w: Vec<u16> = pd.page_info.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            unsafe {
                let _ = GetTextExtentPoint32W(hdc, &info_w, &mut size);
            }
            let w = size.cx + padding * 2;
            if w > max_w {
                max_w = w;
            }
        }

        unsafe { SelectObject(hdc, old_font) };
        unsafe {
            let _ = ReleaseDC(Some(self.hwnd), hdc);
        }

        let extra_row = if pd.page_info.is_empty() { 0 } else { 1 };
        let rows = pd.candidates.len() as i32 + extra_row;
        let h = rows * row_h + padding * 2;
        (max_w, h)
    }
}

impl Default for CandidateWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        unsafe {
            // Clear USERDATA before destroying so any in-flight WM_PAINT
            // sees null and bails out.
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
            if !self.paint_data.is_null() {
                let pd = Box::from_raw(self.paint_data);
                let _ = DeleteObject(pd.font.into());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Window class + WndProc
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
        unsafe {
            RegisterClassExW(&wc);
        }
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
        WM_ERASEBKGND => {
            // We paint the entire client area in WM_PAINT → skip default
            // background erase to prevent flicker.
            LRESULT(1)
        }
        WM_PAINT => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if ptr == 0 {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            // Validate magic before trusting the pointer.
            let magic = unsafe { (*(ptr as *const PaintData)).magic };
            if magic != PAINT_DATA_MAGIC {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            let pd = unsafe { &*(ptr as *const PaintData) };
            paint_candidates(hwnd, pd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint_candidates(hwnd: HWND, pd: &PaintData) {
    let theme = Theme::current();
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

    // Double-buffer to a memory DC to avoid flicker.
    let mem_dc = unsafe { CreateCompatibleDC(Some(hdc)) };
    let client_w = ps.rcPaint.right - ps.rcPaint.left;
    let client_h = ps.rcPaint.bottom - ps.rcPaint.top;
    let mem_bmp: HBITMAP = unsafe { CreateCompatibleBitmap(hdc, client_w, client_h) };
    let old_bmp = unsafe { SelectObject(mem_dc, mem_bmp.into()) };

    let full_rect = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };

    // Background.
    let bg_brush: HBRUSH = unsafe { CreateSolidBrush(theme.bg) };
    unsafe { FillRect(mem_dc, &full_rect, bg_brush) };
    unsafe { let _ = DeleteObject(bg_brush.into()); }

    // Border.
    let border_brush: HBRUSH = unsafe { CreateSolidBrush(theme.border) };
    unsafe { FrameRect(mem_dc, &full_rect, border_brush) };
    unsafe { let _ = DeleteObject(border_brush.into()); }

    let old_font = unsafe { SelectObject(mem_dc, pd.font.into()) };
    unsafe { SetBkMode(mem_dc, TRANSPARENT) };

    let padding = scale(BASE_PADDING, pd.dpi);
    let row_h = scale(BASE_ROW_HEIGHT, pd.dpi);

    let mut y = padding;
    for (i, cand) in pd.candidates.iter().enumerate() {
        let key_ch = pd.selection_keys.get(i).copied().unwrap_or(' ');
        let label = format!("{}. {}", key_ch, cand);
        let label_w: Vec<u16> = label.encode_utf16().collect();

        let (fg, drew_highlight) = if i == pd.highlighted {
            let hl_rect = RECT {
                left: padding / 2,
                top: y,
                right: client_w - padding / 2,
                bottom: y + row_h,
            };
            let hl_brush: HBRUSH = unsafe { CreateSolidBrush(theme.highlight_bg) };
            unsafe { FillRect(mem_dc, &hl_rect, hl_brush) };
            unsafe { let _ = DeleteObject(hl_brush.into()); }
            (theme.highlight_fg, true)
        } else {
            (theme.fg, false)
        };
        let _ = drew_highlight;

        unsafe { SetTextColor(mem_dc, fg) };
        unsafe { let _ = TextOutW(mem_dc, padding, y + padding / 4, &label_w); }
        y += row_h;
    }

    if !pd.page_info.is_empty() {
        let info_w: Vec<u16> = pd.page_info.encode_utf16().collect();
        unsafe {
            SetTextColor(mem_dc, theme.fg_dim);
            let _ = TextOutW(mem_dc, padding, y + padding / 4, &info_w);
        }
    }

    // Blit memory DC to the real DC.
    unsafe {
        let _ = BitBlt(
            hdc,
            ps.rcPaint.left,
            ps.rcPaint.top,
            client_w,
            client_h,
            Some(mem_dc),
            0,
            0,
            SRCCOPY,
        );
    }

    unsafe {
        SelectObject(mem_dc, old_font);
        SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(mem_bmp.into());
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
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

    // Prefer Microsoft JhengHei UI; fall back to JhengHei.
    let face = "Microsoft JhengHei UI";
    let face_w: Vec<u16> = face.encode_utf16().collect();
    for (i, &ch) in face_w.iter().enumerate() {
        if i < 31 {
            lf.lfFaceName[i] = ch;
        }
    }
    unsafe { CreateFontIndirectW(&lf) }
}

fn current_dpi(hwnd: HWND) -> u32 {
    // GetDpiForWindow works on Win10 1607+; fall back to 96.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

fn clamp_to_monitor(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    let pt = POINT { x, y };
    let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GetMonitorInfoW(hmon, &mut info) };
    if !ok.as_bool() {
        return (x.max(0), y.max(0));
    }
    let work = info.rcWork;

    let mut fx = x;
    let mut fy = y;
    if fx + w > work.right {
        fx = work.right - w;
    }
    if fy + h > work.bottom {
        // Flip above the caret.
        fy = y - h - 30;
    }
    if fx < work.left {
        fx = work.left;
    }
    if fy < work.top {
        fy = work.top;
    }
    (fx, fy)
}

// Silence unused-variable warnings for HDC import (kept for readability).
#[allow(dead_code)]
fn _touch(_: HDC) {}
