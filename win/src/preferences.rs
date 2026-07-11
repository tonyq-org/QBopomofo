//! User preferences stored in Windows Registry.
//!
//! Registry key: HKCU\Software\QBopomofo
//! Values:
//! - CandidatesPerPage (DWORD): 5-10 (default: 10)
//! - ShiftBehavior (DWORD): 0=None, 1=SmartToggle, 2=ToggleOnly (default: 1)
//! - SelectionKeys (REG_SZ): "1234567890" or "asdfghjkl;" (default: "1234567890")
//! - SpaceCycleCount (DWORD): 0-3 (default: 0)
//! - CapsLockBehavior (DWORD): 0=None, 1=ToggleChineseEnglish, 2=ToggleFullHalfWidth (default: 0)
//! - CandidateOrdering (DWORD): 0=Dictionary, 1=Frequency, 2=Smart (default: 2)
//! - DebugLogging (DWORD): 0=disabled, 1=enabled (default: 0)

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};

use chewing::typing_mode::{CapsLockBehavior, ShiftBehavior};

const REG_KEY: &str = "Software\\QBopomofo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateOrdering {
    Dictionary,
    Frequency,
    Smart,
}

impl CandidateOrdering {
    fn from_registry(value: u32) -> Self {
        match value {
            0 => Self::Dictionary,
            1 => Self::Frequency,
            2 => Self::Smart,
            _ => Self::Smart,
        }
    }

    fn registry_value(self) -> u32 {
        match self {
            Self::Dictionary => 0,
            Self::Frequency => 1,
            Self::Smart => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Preferences {
    pub candidates_per_page: u32,
    pub shift_behavior: ShiftBehavior,
    pub caps_lock_behavior: CapsLockBehavior,
    pub selection_keys: String,
    pub space_cycle_count: u32,
    pub candidate_ordering: CandidateOrdering,
    pub debug_logging: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            candidates_per_page: 10,
            shift_behavior: ShiftBehavior::SmartToggle,
            caps_lock_behavior: CapsLockBehavior::None,
            selection_keys: "1234567890".to_string(),
            space_cycle_count: 0,
            candidate_ordering: CandidateOrdering::Smart,
            debug_logging: false,
        }
    }
}

impl Preferences {
    /// Load preferences from the registry. Falls back to defaults for missing values.
    pub fn load() -> Self {
        let mut prefs = Preferences::default();

        let hkey = match open_key(KEY_READ) {
            Some(k) => k,
            None => return prefs,
        };

        if let Some(v) = read_dword(hkey, "CandidatesPerPage")
            && (5..=10).contains(&v)
        {
            prefs.candidates_per_page = v;
        }

        if let Some(v) = read_dword(hkey, "ShiftBehavior") {
            prefs.shift_behavior = match v {
                0 => ShiftBehavior::None,
                1 => ShiftBehavior::SmartToggle,
                2 => ShiftBehavior::ToggleOnly,
                _ => ShiftBehavior::SmartToggle,
            };
        }

        if let Some(v) = read_dword(hkey, "CapsLockBehavior") {
            prefs.caps_lock_behavior = match v {
                0 => CapsLockBehavior::None,
                1 => CapsLockBehavior::ToggleChineseEnglish,
                2 => CapsLockBehavior::ToggleFullHalfWidth,
                _ => CapsLockBehavior::None,
            };
        }

        if let Some(s) = read_string(hkey, "SelectionKeys")
            && matches!(s.as_str(), "1234567890" | "asdfghjkl;")
        {
            prefs.selection_keys = s;
        }

        if let Some(v) = read_dword(hkey, "SpaceCycleCount")
            && v <= 3
        {
            prefs.space_cycle_count = v;
        }

        if let Some(v) = read_dword(hkey, "CandidateOrdering") {
            prefs.candidate_ordering = CandidateOrdering::from_registry(v);
        }

        if let Some(v) = read_dword(hkey, "DebugLogging") {
            prefs.debug_logging = v != 0;
        }

        unsafe { let _ = RegCloseKey(hkey); }
        prefs
    }

    /// Save current preferences to the registry.
    pub fn save(&self) -> bool {
        let hkey = match open_key(KEY_WRITE) {
            Some(k) => k,
            None => return false,
        };

        let mut saved = write_dword(hkey, "CandidatesPerPage", self.candidates_per_page);

        let shift_val = match self.shift_behavior {
            ShiftBehavior::None => 0u32,
            ShiftBehavior::SmartToggle => 1,
            ShiftBehavior::ToggleOnly => 2,
        };
        saved &= write_dword(hkey, "ShiftBehavior", shift_val);

        let caps_val = match self.caps_lock_behavior {
            CapsLockBehavior::None => 0u32,
            CapsLockBehavior::ToggleChineseEnglish => 1,
            CapsLockBehavior::ToggleFullHalfWidth => 2,
        };
        saved &= write_dword(hkey, "CapsLockBehavior", caps_val);
        saved &= write_string(hkey, "SelectionKeys", &self.selection_keys);
        saved &= write_dword(hkey, "SpaceCycleCount", self.space_cycle_count);
        saved &= write_dword(
            hkey,
            "CandidateOrdering",
            self.candidate_ordering.registry_value(),
        );
        saved &= write_dword(hkey, "DebugLogging", u32::from(self.debug_logging));

        unsafe { let _ = RegCloseKey(hkey); }
        saved
    }
}

// ---------------------------------------------------------------------------
// Registry helpers
// ---------------------------------------------------------------------------

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_key(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let key_path = to_wide_null(REG_KEY);
    let mut hkey = HKEY::default();
    let err = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            &mut hkey,
            None,
        )
    };
    if err.0 == 0 { Some(hkey) } else { None }
}

fn read_dword(hkey: HKEY, name: &str) -> Option<u32> {
    let name_w = to_wide_null(name);
    let mut data = [0u8; 4];
    let mut data_size = 4u32;
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            None,
            Some(data.as_mut_ptr()),
            Some(&mut data_size),
        )
    };
    if err.0 == 0 && data_size == 4 {
        Some(u32::from_le_bytes(data))
    } else {
        None
    }
}

fn read_string(hkey: HKEY, name: &str) -> Option<String> {
    let name_w = to_wide_null(name);
    let mut data_size = 0u32;
    // First call to get size
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            None,
            None,
            Some(&mut data_size),
        )
    };
    if err.0 != 0 || data_size == 0 { return None; }

    let mut buf = vec![0u8; data_size as usize];
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr()),
            Some(&mut data_size),
        )
    };
    if err.0 != 0 { return None; }

    // REG_SZ is null-terminated UTF-16
    let wide: Vec<u16> = buf.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16_lossy(&wide);
    Some(s.trim_end_matches('\0').to_string())
}

fn write_dword(hkey: HKEY, name: &str, value: u32) -> bool {
    let name_w = to_wide_null(name);
    let data = value.to_le_bytes();
    let result = unsafe {
        RegSetValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            Some(0),
            REG_DWORD,
            Some(&data),
        )
    };
    result.0 == 0
}

fn write_string(hkey: HKEY, name: &str, value: &str) -> bool {
    let name_w = to_wide_null(name);
    let value_w = to_wide_null(value);
    let result = unsafe {
        RegSetValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            Some(0),
            REG_SZ,
            Some(std::slice::from_raw_parts(
                value_w.as_ptr() as *const u8,
                value_w.len() * 2,
            )),
        )
    };
    result.0 == 0
}

#[cfg(test)]
mod tests {
    use super::CandidateOrdering;

    #[test]
    fn candidate_ordering_registry_values_are_stable() {
        assert_eq!(CandidateOrdering::from_registry(0), CandidateOrdering::Dictionary);
        assert_eq!(CandidateOrdering::from_registry(1), CandidateOrdering::Frequency);
        assert_eq!(CandidateOrdering::from_registry(2), CandidateOrdering::Smart);
        assert_eq!(CandidateOrdering::from_registry(999), CandidateOrdering::Smart);
        assert_eq!(CandidateOrdering::Smart.registry_value(), 2);
    }
}
