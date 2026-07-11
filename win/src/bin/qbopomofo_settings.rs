#![windows_subsystem = "windows"]

use chewing::typing_mode::{CapsLockBehavior, ShiftBehavior};
use qbopomofo_tip::preferences::{CandidateOrdering, Preferences};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, GetSysColorBrush,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, HMENU,
    IDC_ARROW, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG, MessageBoxW,
    PostQuitMessage, RegisterClassExW, SW_SHOW, SendMessageW, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT,
    WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, Result, w};

const ID_ORDERING: usize = 101;
const ID_PAGE_SIZE: usize = 102;
const ID_SELECTION_KEYS: usize = 103;
const ID_SHIFT: usize = 104;
const ID_CAPS: usize = 105;
const ID_SPACE_CYCLE: usize = 106;
const ID_DEBUG_LOG: usize = 107;
const ID_DEFAULTS: usize = 201;
const ID_CANCEL: usize = 202;
const ID_SAVE: usize = 203;

const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const SS_LEFT: u32 = 0x0000;

const PAGE_SIZES: [u32; 6] = [5, 6, 7, 8, 9, 10];

struct Controls {
    ordering: HWND,
    page_size: HWND,
    selection_keys: HWND,
    shift: HWND,
    caps: HWND,
    space_cycle: HWND,
    debug_log: HWND,
}

struct AppState {
    controls: Controls,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn child_menu(id: usize) -> HMENU {
    HMENU(id as *mut _)
}

#[allow(clippy::too_many_arguments)] // Mirrors CreateWindowExW geometry and style arguments.
unsafe fn create_control(
    parent: HWND,
    class_name: PCWSTR,
    text: &str,
    id: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    extra_style: u32,
) -> Result<HWND> {
    let module = unsafe { GetModuleHandleW(None)? };
    let text = wide(text);
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(extra_style),
            x,
            y,
            width,
            height,
            Some(parent),
            Some(child_menu(id)),
            Some(module.into()),
            None,
        )?
    };
    apply_default_font(control);
    Ok(control)
}

fn apply_default_font(control: HWND) {
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    unsafe {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
}

fn add_combo_item(combo: HWND, text: &str) {
    let text = wide(text);
    unsafe {
        SendMessageW(
            combo,
            CB_ADDSTRING,
            None,
            Some(LPARAM(text.as_ptr() as isize)),
        );
    }
}

fn set_combo_index(combo: HWND, index: usize) {
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(index)), None);
    }
}

fn combo_index(combo: HWND) -> usize {
    let result = unsafe { SendMessageW(combo, CB_GETCURSEL, None, None) }.0;
    usize::try_from(result).unwrap_or(0)
}

fn set_checked(button: HWND, checked: bool) {
    unsafe {
        SendMessageW(
            button,
            BM_SETCHECK,
            Some(WPARAM(if checked { BST_CHECKED } else { 0 })),
            None,
        );
    }
}

fn is_checked(button: HWND) -> bool {
    unsafe { SendMessageW(button, BM_GETCHECK, None, None) }.0 as usize == BST_CHECKED
}

fn populate_controls(controls: &Controls) {
    for item in [
        "智慧排序（長詞優先，同長度依詞頻）",
        "依詞頻排序",
        "傳統字典順序",
    ] {
        add_combo_item(controls.ordering, item);
    }
    for page_size in PAGE_SIZES {
        add_combo_item(controls.page_size, &format!("每頁 {page_size} 個"));
    }
    for item in ["1234567890（數字列）", "asdfghjkl;（主鍵盤）"] {
        add_combo_item(controls.selection_keys, item);
    }
    for item in [
        "智慧切換（短按切換、按住輸入英文）",
        "只切換中英文",
        "不使用 Shift 切換",
    ] {
        add_combo_item(controls.shift, item);
    }
    for item in ["不變更輸入模式", "切換中英文"] {
        add_combo_item(controls.caps, item);
    }
    for item in ["停用", "1 次", "2 次", "3 次"] {
        add_combo_item(controls.space_cycle, item);
    }
}

fn apply_preferences(controls: &Controls, prefs: &Preferences) {
    set_combo_index(
        controls.ordering,
        match prefs.candidate_ordering {
            CandidateOrdering::Smart => 0,
            CandidateOrdering::Frequency => 1,
            CandidateOrdering::Dictionary => 2,
        },
    );
    let page_index = PAGE_SIZES
        .iter()
        .position(|value| *value == prefs.candidates_per_page)
        .unwrap_or(PAGE_SIZES.len() - 1);
    set_combo_index(controls.page_size, page_index);
    set_combo_index(
        controls.selection_keys,
        usize::from(prefs.selection_keys == "asdfghjkl;"),
    );
    set_combo_index(
        controls.shift,
        match prefs.shift_behavior {
            ShiftBehavior::SmartToggle => 0,
            ShiftBehavior::ToggleOnly => 1,
            ShiftBehavior::None => 2,
        },
    );
    set_combo_index(
        controls.caps,
        usize::from(prefs.caps_lock_behavior == CapsLockBehavior::ToggleChineseEnglish),
    );
    set_combo_index(
        controls.space_cycle,
        prefs.space_cycle_count.min(3) as usize,
    );
    set_checked(controls.debug_log, prefs.debug_logging);
}

fn preferences_from_controls(controls: &Controls) -> Preferences {
    Preferences {
        candidate_ordering: match combo_index(controls.ordering) {
            1 => CandidateOrdering::Frequency,
            2 => CandidateOrdering::Dictionary,
            _ => CandidateOrdering::Smart,
        },
        candidates_per_page: PAGE_SIZES
            .get(combo_index(controls.page_size))
            .copied()
            .unwrap_or(10),
        selection_keys: if combo_index(controls.selection_keys) == 1 {
            "asdfghjkl;".to_string()
        } else {
            "1234567890".to_string()
        },
        shift_behavior: match combo_index(controls.shift) {
            1 => ShiftBehavior::ToggleOnly,
            2 => ShiftBehavior::None,
            _ => ShiftBehavior::SmartToggle,
        },
        caps_lock_behavior: if combo_index(controls.caps) == 1 {
            CapsLockBehavior::ToggleChineseEnglish
        } else {
            CapsLockBehavior::None
        },
        space_cycle_count: combo_index(controls.space_cycle).min(3) as u32,
        debug_logging: is_checked(controls.debug_log),
    }
}

unsafe fn create_settings_window() -> Result<HWND> {
    let module = unsafe { GetModuleHandleW(None)? };
    let class_name = w!("QBopomofo_Settings");
    let window_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(settings_wnd_proc),
        hInstance: module.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        lpszClassName: class_name,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&window_class) };

    let title = w!("Q注音設定");
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            title,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            640,
            570,
            None,
            None,
            Some(module.into()),
            None,
        )?
    };

    unsafe {
        create_control(
            hwnd,
            w!("STATIC"),
            "候選字與選字方式",
            0,
            28,
            20,
            560,
            28,
            SS_LEFT,
        )?;
        create_control(hwnd, w!("STATIC"), "候選排序", 0, 28, 65, 140, 24, SS_LEFT)?;
    }
    let ordering = unsafe {
        create_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            ID_ORDERING,
            180,
            60,
            400,
            180,
            CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0,
        )?
    };
    unsafe {
        create_control(
            hwnd,
            w!("STATIC"),
            "建議使用智慧排序：完整詞組先出現，同長度候選再依常用程度排列。",
            0,
            180,
            92,
            410,
            36,
            SS_LEFT,
        )?;
        create_control(
            hwnd,
            w!("STATIC"),
            "每頁候選數",
            0,
            28,
            137,
            140,
            24,
            SS_LEFT,
        )?;
    }
    let page_size = unsafe {
        create_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            ID_PAGE_SIZE,
            180,
            132,
            200,
            180,
            CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0,
        )?
    };
    unsafe {
        create_control(hwnd, w!("STATIC"), "選字鍵", 0, 28, 182, 140, 24, SS_LEFT)?;
    }
    let selection_keys = unsafe {
        create_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            ID_SELECTION_KEYS,
            180,
            177,
            300,
            160,
            CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0,
        )?
    };

    unsafe {
        create_control(
            hwnd,
            w!("STATIC"),
            "按鍵與切換方式",
            0,
            28,
            235,
            560,
            28,
            SS_LEFT,
        )?;
        create_control(hwnd, w!("STATIC"), "Shift 鍵", 0, 28, 278, 140, 24, SS_LEFT)?;
    }
    let shift = unsafe {
        create_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            ID_SHIFT,
            180,
            273,
            400,
            160,
            CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0,
        )?
    };
    unsafe {
        create_control(
            hwnd,
            w!("STATIC"),
            "Caps Lock",
            0,
            28,
            323,
            140,
            24,
            SS_LEFT,
        )?;
    }
    let caps = unsafe {
        create_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            ID_CAPS,
            180,
            318,
            300,
            140,
            CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0,
        )?
    };
    unsafe {
        create_control(
            hwnd,
            w!("STATIC"),
            "空白鍵快速換字",
            0,
            28,
            368,
            140,
            24,
            SS_LEFT,
        )?;
    }
    let space_cycle = unsafe {
        create_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            ID_SPACE_CYCLE,
            180,
            363,
            200,
            140,
            CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0,
        )?
    };
    let debug_log = unsafe {
        create_control(
            hwnd,
            w!("BUTTON"),
            "記錄候選與選字（%TEMP%\\qbopomofo.log）",
            ID_DEBUG_LOG,
            180,
            405,
            360,
            26,
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
        )?
    };

    unsafe {
        create_control(
            hwnd,
            w!("BUTTON"),
            "恢復預設",
            ID_DEFAULTS,
            28,
            475,
            110,
            34,
            WS_TABSTOP.0,
        )?;
        create_control(
            hwnd,
            w!("BUTTON"),
            "取消",
            ID_CANCEL,
            390,
            475,
            90,
            34,
            WS_TABSTOP.0,
        )?;
        create_control(
            hwnd,
            w!("BUTTON"),
            "儲存",
            ID_SAVE,
            490,
            475,
            90,
            34,
            BS_DEFPUSHBUTTON as u32 | WS_TABSTOP.0,
        )?;
    }

    let controls = Controls {
        ordering,
        page_size,
        selection_keys,
        shift,
        caps,
        space_cycle,
        debug_log,
    };
    populate_controls(&controls);
    apply_preferences(&controls, &Preferences::load());

    let state = Box::new(AppState { controls });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    Ok(hwnd)
}

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    qbopomofo_tip::panic_guard::guard("settings_wnd_proc", LRESULT(0), || unsafe {
        settings_wnd_proc_inner(hwnd, message, wparam, lparam)
    })
}

unsafe fn settings_wnd_proc_inner(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut AppState;
            if state_ptr.is_null() {
                return LRESULT(0);
            }
            let state = unsafe { &*state_ptr };
            match id {
                ID_DEFAULTS => apply_preferences(&state.controls, &Preferences::default()),
                ID_CANCEL => {
                    let _ = unsafe { DestroyWindow(hwnd) };
                }
                ID_SAVE => {
                    let prefs = preferences_from_controls(&state.controls);
                    if prefs.save() {
                        let message =
                            wide("設定已儲存。切換到其他輸入法再切回，或重新開啟應用程式後生效。");
                        let title = wide("Q注音設定");
                        unsafe {
                            MessageBoxW(
                                Some(hwnd),
                                PCWSTR(message.as_ptr()),
                                PCWSTR(title.as_ptr()),
                                MB_OK | MB_ICONINFORMATION,
                            );
                            let _ = DestroyWindow(hwnd);
                        }
                    } else {
                        show_error(Some(hwnd), "無法寫入使用者設定，請確認登錄檔權限。\0");
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut AppState;
            if !state_ptr.is_null() {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(state_ptr));
                }
            }
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn show_error(parent: Option<HWND>, message: &str) {
    let message = wide(message.trim_end_matches('\0'));
    let title = wide("Q注音設定");
    unsafe {
        MessageBoxW(
            parent,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn run() -> Result<()> {
    let _window = unsafe { create_settings_window()? };
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        show_error(None, &format!("無法開啟設定頁：{error}"));
    }
}
