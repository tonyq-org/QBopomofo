use super::{ChewingEngine, ConversionEngine, Outcome};
use crate::dictionary::LookupStrategy;

/// Same conversion method as Chewing but uses fuzzy phrase search.
#[derive(Debug, Default)]
pub struct FuzzyChewingEngine {
    inner: ChewingEngine,
}

impl FuzzyChewingEngine {
    /// Creates a new conversion engine.
    pub fn new() -> FuzzyChewingEngine {
        let mut inner = ChewingEngine::new();
        inner.lookup_strategy = LookupStrategy::FuzzyPartialPrefix;
        inner.abbreviated_mode = false;
        FuzzyChewingEngine { inner }
    }
}

impl ConversionEngine for FuzzyChewingEngine {
    fn convert<'a>(
        &'a self,
        dict: &'a dyn crate::dictionary::Dictionary,
        comp: &'a super::Composition,
    ) -> Vec<Outcome> {
        ChewingEngine::convert(&self.inner, dict, comp)
    }
}
