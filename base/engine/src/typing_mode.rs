//! TypingMode — complete input mode definition
//!
//! A TypingMode bundles together a keyboard layout, conversion engine,
//! and metadata into a switchable input mode. This allows runtime switching
//! between modes (e.g. standard bopomofo, fuzzy bopomofo, hsu layout)
//! without reloading dictionaries.
//!
//! Dictionaries are shared across modes — only the layout and conversion
//! strategy change. Zero performance overhead for mode switching.

use crate::conversion::{ChewingEngine, ConversionEngine, FuzzyChewingEngine, SimpleEngine};
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
}

/// A complete typing mode definition.
///
/// Combines layout + conversion engine + metadata.
/// Dictionaries are NOT part of the mode — they are shared across all modes.
#[derive(Debug, Clone)]
pub struct TypingMode {
    pub info: TypingModeInfo,
    pub layout: KeyboardLayout,
    pub conversion: ConversionKind,
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
            Self::hsu_bopomofo(),
        ]
    }
}
