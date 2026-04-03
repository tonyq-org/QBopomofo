//! Keyboard event handling for QBopomofo Windows TSF
//!
//! Maps Windows virtual key codes to chewing engine KeyboardEvent.

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardState, ToUnicode, MapVirtualKeyW, MAPVK_VK_TO_VSC,
};

use chewing::input::keycode::{self, Keycode};
use chewing::input::keysym::{self, Keysym};
use chewing::input::KeyboardEvent;

/// Translate a Windows virtual key code to a character using ToUnicode.
///
/// This calls the Win32 `ToUnicode` API to get the character that corresponds
/// to the given virtual key with the current keyboard layout and modifier state.
#[cfg(windows)]
pub fn translate_char(vkey: u32, _lparam: u32, _shift: bool) -> char {
    let scan_code = unsafe { MapVirtualKeyW(vkey, MAPVK_VK_TO_VSC) };
    let mut keyboard_state = [0u8; 256];
    unsafe { let _ = GetKeyboardState(&mut keyboard_state); }

    let mut buf = [0u16; 4];
    let result = unsafe {
        ToUnicode(vkey, scan_code, Some(&keyboard_state), &mut buf, 0)
    };

    if result == 1 {
        char::decode_utf16(buf.iter().copied())
            .next()
            .and_then(|r| r.ok())
            .unwrap_or('\0')
    } else {
        '\0'
    }
}

/// Fallback for non-Windows platforms (testing).
#[cfg(not(windows))]
pub fn translate_char(vkey: u32, _lparam: u32, shift: bool) -> char {
    match vkey {
        0x30..=0x39 if !shift => (b'0' + (vkey - 0x30) as u8) as char,
        0x41..=0x5A if shift => (b'A' + (vkey - 0x41) as u8) as char,
        0x41..=0x5A => (b'a' + (vkey - 0x41) as u8) as char,
        0x20 => ' ',
        _ => '\0',
    }
}

/// Convert a Windows virtual key code to a chewing KeyboardEvent.
///
/// # Arguments
/// * `vkey` - Windows virtual key code (VK_*)
/// * `ch` - The translated character, or '\0' if none
/// * `shift` - Whether Shift is held
/// * `ctrl` - Whether Control is held
/// * `caps_lock` - Whether Caps Lock is on
///
/// Returns `None` if the key should be passed through to the application.
pub fn vkey_to_keyboard_event(
    vkey: u32,
    ch: char,
    shift: bool,
    ctrl: bool,
    caps_lock: bool,
) -> Option<KeyboardEvent> {
    // Pass through Ctrl+key (system shortcuts)
    if ctrl {
        return None;
    }

    let code = vkey_to_keycode(vkey)?;

    // Map character to keysym — special keys need X11-style keysyms,
    // printable ASCII maps directly to its code point.
    let ksym = match vkey_to_keysym(vkey) {
        Some(sym) => sym,
        None if (ch as u32) > 0 && (ch as u32) <= 0x7F => Keysym(ch as u32),
        None => keysym::SYM_NONE,
    };

    let evt = KeyboardEvent::builder()
        .code(code)
        .ksym(ksym)
        .shift_if(shift)
        .control_if(ctrl)
        .caps_lock_if(caps_lock)
        .build();

    Some(evt)
}

/// Map Windows VK_* to chewing Keycode (xkbcommon encoding).
fn vkey_to_keycode(vkey: u32) -> Option<Keycode> {
    let code = match vkey {
        // Control keys
        0x08 => keycode::KEY_BACKSPACE,
        0x09 => keycode::KEY_TAB,
        0x0D => keycode::KEY_ENTER,
        0x1B => keycode::KEY_ESC,
        0x20 => keycode::KEY_SPACE,
        0x2E => keycode::KEY_DELETE,
        // Navigation
        0x21 => keycode::KEY_PAGEUP,
        0x22 => keycode::KEY_PAGEDOWN,
        0x23 => keycode::KEY_END,
        0x24 => keycode::KEY_HOME,
        0x25 => keycode::KEY_LEFT,
        0x26 => keycode::KEY_UP,
        0x27 => keycode::KEY_RIGHT,
        0x28 => keycode::KEY_DOWN,
        // Number row: VK_0(0x30)..VK_9(0x39)
        0x30 => keycode::KEY_0,
        0x31 => keycode::KEY_1,
        0x32 => keycode::KEY_2,
        0x33 => keycode::KEY_3,
        0x34 => keycode::KEY_4,
        0x35 => keycode::KEY_5,
        0x36 => keycode::KEY_6,
        0x37 => keycode::KEY_7,
        0x38 => keycode::KEY_8,
        0x39 => keycode::KEY_9,
        // Letter keys: VK_A(0x41)..VK_Z(0x5A)
        0x41 => keycode::KEY_A,
        0x42 => keycode::KEY_B,
        0x43 => keycode::KEY_C,
        0x44 => keycode::KEY_D,
        0x45 => keycode::KEY_E,
        0x46 => keycode::KEY_F,
        0x47 => keycode::KEY_G,
        0x48 => keycode::KEY_H,
        0x49 => keycode::KEY_I,
        0x4A => keycode::KEY_J,
        0x4B => keycode::KEY_K,
        0x4C => keycode::KEY_L,
        0x4D => keycode::KEY_M,
        0x4E => keycode::KEY_N,
        0x4F => keycode::KEY_O,
        0x50 => keycode::KEY_P,
        0x51 => keycode::KEY_Q,
        0x52 => keycode::KEY_R,
        0x53 => keycode::KEY_S,
        0x54 => keycode::KEY_T,
        0x55 => keycode::KEY_U,
        0x56 => keycode::KEY_V,
        0x57 => keycode::KEY_W,
        0x58 => keycode::KEY_X,
        0x59 => keycode::KEY_Y,
        0x5A => keycode::KEY_Z,
        // OEM keys
        0xBA => keycode::KEY_SEMICOLON,
        0xBB => keycode::KEY_EQUAL,
        0xBC => keycode::KEY_COMMA,
        0xBD => keycode::KEY_MINUS,
        0xBE => keycode::KEY_DOT,
        0xBF => keycode::KEY_SLASH,
        0xC0 => keycode::KEY_GRAVE,
        0xDB => keycode::KEY_LEFTBRACE,
        0xDC => keycode::KEY_BACKSLASH,
        0xDD => keycode::KEY_RIGHTBRACE,
        0xDE => keycode::KEY_APOSTROPHE,
        _ => return None,
    };
    Some(code)
}

/// Map Windows VK_* to chewing Keysym for special (non-printable) keys.
/// Printable ASCII is handled separately via the `ch` parameter.
fn vkey_to_keysym(vkey: u32) -> Option<Keysym> {
    let sym = match vkey {
        0x08 => keysym::SYM_BACKSPACE,
        0x09 => keysym::SYM_TAB,
        0x0D => keysym::SYM_RETURN,
        0x1B => keysym::SYM_ESC,
        0x2E => keysym::SYM_DELETE,
        0x21 => keysym::SYM_PAGEUP,
        0x22 => keysym::SYM_PAGEDOWN,
        0x23 => keysym::SYM_END,
        0x24 => keysym::SYM_HOME,
        0x25 => keysym::SYM_LEFT,
        0x26 => keysym::SYM_UP,
        0x27 => keysym::SYM_RIGHT,
        0x28 => keysym::SYM_DOWN,
        _ => return None,
    };
    Some(sym)
}
