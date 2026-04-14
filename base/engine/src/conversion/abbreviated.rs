use super::{ChewingEngine, ConversionEngine, Outcome};
use crate::dictionary::LookupStrategy;

/// Abbreviated input mode: matches phrases by initials (聲母) only.
///
/// Uses FuzzyPartialPrefix lookup so that partial syllables (e.g. just the
/// initial consonant) can match full dictionary entries. The scoring weights
/// are tuned to strongly prefer multi-character phrases over single-character
/// matches, which are very noisy when only initials are provided.
#[derive(Debug, Default)]
pub struct AbbreviatedChewingEngine {
    inner: ChewingEngine,
}

impl AbbreviatedChewingEngine {
    /// Creates a new abbreviated conversion engine.
    pub fn new() -> AbbreviatedChewingEngine {
        AbbreviatedChewingEngine {
            inner: ChewingEngine {
                lookup_strategy: LookupStrategy::FuzzyPartialPrefix,
                abbreviated_mode: true,
            },
        }
    }
}

impl ConversionEngine for AbbreviatedChewingEngine {
    fn convert<'a>(
        &'a self,
        dict: &'a dyn crate::dictionary::Dictionary,
        comp: &'a super::Composition,
    ) -> Vec<Outcome> {
        ChewingEngine::convert(&self.inner, dict, comp)
    }
}
