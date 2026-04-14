//! TypingMode — complete input mode definition
//!
//! A TypingMode bundles together a keyboard layout, conversion engine,
//! and metadata into a switchable input mode. This allows runtime switching
//! between modes (e.g. standard bopomofo, fuzzy bopomofo, hsu layout)
//! without reloading dictionaries.
//!
//! Dictionaries are shared across modes — only the layout and conversion
//! strategy change. Zero performance overhead for mode switching.

use crate::conversion::{
    AbbreviatedChewingEngine, ChewingEngine, ConversionEngine, FuzzyChewingEngine, SimpleEngine,
};
use crate::editor::zhuyin_layout::{
    DaiChien26, Et, Et26, GinYieh, Hsu, Ibm, Pinyin, Standard, SyllableEditor,
};
use crate::editor::Editor;

/// Metadata for a typing mode.
#[derive(Debug, Clone)]
pub struct TypingModeInfo {
    /// Unique identifier (e.g. "bopomofo-standard")
    pub id: String,
    /// Display name (e.g. "標準注音")
    pub name: String,
    /// Description
    pub description: String,
}

/// Keyboard layout identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardLayout {
    Standard,
    Hsu,
    Ibm,
    GinYieh,
    Et,
    Et26,
    DaiChien26,
    HanyuPinyin,
    ThlPinyin,
    Mps2Pinyin,
}

/// Conversion engine identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionKind {
    Simple,
    Chewing,
    FuzzyChewing,
    AbbreviatedChewing,
}

/// What action Shift key performs in this mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftBehavior {
    /// Shift does nothing special (default chewing behavior)
    None,
    /// Smart Shift: short press toggles Chinese/English mode,
    /// hold down for temporary English (release returns to Chinese).
    /// This is the default for Q注音.
    SmartToggle,
    /// Shift only toggles between Chinese and English input (no hold behavior)
    ToggleOnly,
}

/// What action Caps Lock performs in this mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapsLockBehavior {
    /// Caps Lock does nothing special
    None,
    /// Caps Lock toggles between Chinese and English input
    ToggleChineseEnglish,
    /// Caps Lock toggles between full-width and half-width characters
    ToggleFullHalfWidth,
}

/// Per-mode preferences that control input behavior.
/// Each TypingMode can have its own preference set.
#[derive(Debug, Clone)]
pub struct ModePreferences {
    /// What Shift key does
    pub shift_behavior: ShiftBehavior,
    /// What Caps Lock does
    pub caps_lock_behavior: CapsLockBehavior,
    /// Number of candidates per page (1-10)
    pub candidates_per_page: u8,
    /// Whether Space selects the first candidate
    pub space_as_selection: bool,
    /// Whether Esc clears the entire buffer
    pub esc_clear_all: bool,
    /// Whether to auto-learn user phrases
    pub auto_learn: bool,
}

impl Default for ModePreferences {
    fn default() -> Self {
        Self {
            shift_behavior: ShiftBehavior::SmartToggle,
            caps_lock_behavior: CapsLockBehavior::None,
            candidates_per_page: 9,
            space_as_selection: true,
            esc_clear_all: true,
            auto_learn: true,
        }
    }
}

/// A complete typing mode definition.
///
/// Combines layout + conversion engine + preferences + metadata.
/// Dictionaries are NOT part of the mode — they are shared across all modes.
#[derive(Debug, Clone)]
pub struct TypingMode {
    pub info: TypingModeInfo,
    pub layout: KeyboardLayout,
    pub conversion: ConversionKind,
    pub preferences: ModePreferences,
}

impl TypingMode {
    /// QBopomofo default mode — standard bopomofo with custom phrase tuning.
    /// All our custom phrase adjustments (via /tune) are applied to this mode.
    /// This is the primary mode for QBopomofo.
    pub fn q_bopomofo() -> Self {
        Self {
            info: TypingModeInfo {
                id: "q-bopomofo".into(),
                name: "Q注音".into(),
                description: "QBopomofo 預設模式 — 標準注音 + 自訂詞頻調校".into(),
            },
            layout: KeyboardLayout::Standard,
            conversion: ConversionKind::Chewing,
            preferences: ModePreferences {
                shift_behavior: ShiftBehavior::SmartToggle,
                caps_lock_behavior: CapsLockBehavior::None,
                ..Default::default()
            },
        }
    }

    /// Create the standard bopomofo mode (DaChen keyboard + Chewing engine).
    pub fn standard_bopomofo() -> Self {
        Self {
            info: TypingModeInfo {
                id: "bopomofo-standard".into(),
                name: "標準注音".into(),
                description: "大千鍵盤標準注音".into(),
            },
            layout: KeyboardLayout::Standard,
            conversion: ConversionKind::Chewing,
            preferences: ModePreferences::default(),
        }
    }

    /// Create the fuzzy bopomofo mode (DaChen keyboard + fuzzy engine).
    pub fn fuzzy_bopomofo() -> Self {
        Self {
            info: TypingModeInfo {
                id: "bopomofo-fuzzy".into(),
                name: "模糊注音".into(),
                description: "容許相近發音的注音模式".into(),
            },
            layout: KeyboardLayout::Standard,
            conversion: ConversionKind::FuzzyChewing,
            preferences: ModePreferences::default(),
        }
    }

    /// Create the abbreviated bopomofo mode (initials-only matching).
    ///
    /// Users type only the initial consonant (聲母) of each syllable and the
    /// engine fuzzy-matches against full dictionary entries. Scoring weights
    /// are tuned to strongly prefer multi-character phrases over single chars.
    pub fn abbreviated_bopomofo() -> Self {
        Self {
            info: TypingModeInfo {
                id: "bopomofo-abbreviated".into(),
                name: "簡拼注音".into(),
                description: "只打聲母即可匹配詞組的簡拼模式".into(),
            },
            layout: KeyboardLayout::Standard,
            conversion: ConversionKind::AbbreviatedChewing,
            preferences: ModePreferences {
                shift_behavior: ShiftBehavior::SmartToggle,
                caps_lock_behavior: CapsLockBehavior::None,
                ..Default::default()
            },
        }
    }

    /// Create the Hsu bopomofo mode.
    pub fn hsu_bopomofo() -> Self {
        Self {
            info: TypingModeInfo {
                id: "bopomofo-hsu".into(),
                name: "許氏注音".into(),
                description: "許氏鍵盤注音".into(),
            },
            layout: KeyboardLayout::Hsu,
            conversion: ConversionKind::Chewing,
            preferences: ModePreferences::default(),
        }
    }

    /// Create a SyllableEditor instance for this mode's keyboard layout.
    pub fn create_syllable_editor(&self) -> Box<dyn SyllableEditor> {
        match self.layout {
            KeyboardLayout::Standard => Box::new(Standard::new()),
            KeyboardLayout::Hsu => Box::new(Hsu::new()),
            KeyboardLayout::Ibm => Box::new(Ibm::new()),
            KeyboardLayout::GinYieh => Box::new(GinYieh::new()),
            KeyboardLayout::Et => Box::new(Et::new()),
            KeyboardLayout::Et26 => Box::new(Et26::new()),
            KeyboardLayout::DaiChien26 => Box::new(DaiChien26::new()),
            KeyboardLayout::HanyuPinyin => Box::new(Pinyin::hanyu()),
            KeyboardLayout::ThlPinyin => Box::new(Pinyin::thl()),
            KeyboardLayout::Mps2Pinyin => Box::new(Pinyin::mps2()),
        }
    }

    /// Create a ConversionEngine instance for this mode.
    pub fn create_conversion_engine(&self) -> Box<dyn ConversionEngine> {
        match self.conversion {
            ConversionKind::Simple => Box::new(SimpleEngine::new()),
            ConversionKind::Chewing => Box::new(ChewingEngine::new()),
            ConversionKind::FuzzyChewing => Box::new(FuzzyChewingEngine::new()),
            ConversionKind::AbbreviatedChewing => Box::new(AbbreviatedChewingEngine::new()),
        }
    }

    /// Apply this mode to an existing Editor.
    /// Switches layout and conversion engine. Dictionaries are not affected.
    pub fn apply_to(&self, editor: &mut Editor) {
        editor.set_syllable_editor(self.create_syllable_editor());
        editor.set_conversion_engine(self.create_conversion_engine());
    }

    /// List all built-in typing modes. Q注音 is first (default).
    pub fn all_modes() -> Vec<TypingMode> {
        vec![
            Self::q_bopomofo(),
            Self::standard_bopomofo(),
            Self::fuzzy_bopomofo(),
            Self::abbreviated_bopomofo(),
            Self::hsu_bopomofo(),
        ]
    }
}
