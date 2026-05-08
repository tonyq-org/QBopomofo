use std::cmp::{Reverse, min};

use crate::{
    conversion::{Composition, Gap, Interval},
    dictionary::{Dictionary, Layered, LookupStrategy, Phrase},
    editor::{EditorError, EditorErrorKind, SharedState},
    zhuyin::Syllable,
};

#[derive(Debug)]
pub(crate) struct PhraseSelector {
    begin: usize,
    end: usize,
    forward_select: bool,
    orig: usize,
    lookup_strategy: LookupStrategy,
    com: Composition,
}

impl PhraseSelector {
    pub(crate) fn new(
        forward_select: bool,
        lookup_strategy: LookupStrategy,
        com: Composition,
    ) -> PhraseSelector {
        PhraseSelector {
            begin: 0,
            end: com.len(),
            forward_select,
            orig: 0,
            lookup_strategy,
            com,
        }
    }

    pub(crate) fn init<D: Dictionary>(&mut self, cursor: usize, dict: &D) {
        self.orig = cursor;
        if self.forward_select {
            self.begin = if cursor == self.com.len() {
                cursor - 1
            } else {
                cursor
            };
            self.end = self.next_break_point(cursor);
        } else {
            self.end = min(cursor + 1, self.com.len());
            self.begin = self.after_previous_break_point(cursor);
        }
        loop {
            let symbols = &self.com.symbols()[self.begin..self.end];
            let syllables: Vec<Syllable> = symbols
                .iter()
                .map(|s| s.to_syllable().unwrap_or_default())
                .collect();
            debug_assert!(
                !syllables.is_empty(),
                "should not enter here if there's no syllable in range"
            );
            if !dict.lookup(&syllables, self.lookup_strategy).is_empty() {
                break;
            }
            if self.forward_select {
                self.end -= 1;
            } else {
                self.begin += 1;
            }
        }
    }

    pub(crate) fn init_single_word(&mut self, cursor: usize) {
        self.orig = cursor;
        self.end = min(cursor, self.com.len());
        self.begin = self.end - 1;
    }

    pub(crate) fn begin(&self) -> usize {
        self.begin
    }

    pub(crate) fn next_selection_point<D: Dictionary>(&self, dict: &D) -> Option<(usize, usize)> {
        let (mut begin, mut end) = (self.begin, self.end);
        loop {
            if self.forward_select {
                end -= 1;
                if begin == end {
                    return None;
                }
            } else {
                begin += 1;
                if begin == end {
                    return None;
                }
            }
            let symbols = &self.com.symbols()[begin..end];
            let syllables: Vec<Syllable> = symbols
                .iter()
                .map(|s| s.to_syllable().unwrap_or_default())
                .collect();
            if !dict.lookup(&syllables, self.lookup_strategy).is_empty() {
                return Some((begin, end));
            }
        }
    }
    pub(crate) fn prev_selection_point<D: Dictionary>(&self, dict: &D) -> Option<(usize, usize)> {
        let (mut begin, mut end) = (self.begin, self.end);
        loop {
            if self.forward_select {
                if end == self.com.len() {
                    return None;
                }
                end += 1;
                if end > self.next_break_point(self.orig) {
                    return None;
                }
            } else {
                if begin == 0 {
                    return None;
                }
                begin -= 1;
                if begin < self.after_previous_break_point(self.orig) {
                    return None;
                }
            }
            let symbols = &self.com.symbols()[begin..end];
            let syllables: Vec<Syllable> = symbols
                .iter()
                .map(|s| s.to_syllable().unwrap_or_default())
                .collect();
            if !dict.lookup(&syllables, self.lookup_strategy).is_empty() {
                return Some((begin, end));
            }
        }
    }
    pub(crate) fn jump_to_next_selection_point<D: Dictionary>(
        &mut self,
        dict: &D,
    ) -> Result<(), EditorError> {
        if let Some((begin, end)) = self.next_selection_point(dict) {
            self.begin = begin;
            self.end = end;
            Ok(())
        } else {
            Err(EditorError::new(EditorErrorKind::Impossible))
        }
    }
    pub(crate) fn jump_to_prev_selection_point<D: Dictionary>(
        &mut self,
        dict: &D,
    ) -> Result<(), EditorError> {
        if let Some((begin, end)) = self.prev_selection_point(dict) {
            self.begin = begin;
            self.end = end;
            Ok(())
        } else {
            Err(EditorError::new(EditorErrorKind::Impossible))
        }
    }
    pub(crate) fn jump_to_first_selection_point<D: Dictionary>(&mut self, dict: &D) {
        self.init(self.orig, dict);
    }
    pub(crate) fn jump_to_last_selection_point<D: Dictionary>(&mut self, dict: &D) {
        while self.next_selection_point(dict).is_some() {
            let _ = self.jump_to_next_selection_point(dict);
        }
    }

    pub(crate) fn next<D: Dictionary>(&mut self, dict: &D) {
        loop {
            if self.forward_select {
                self.end -= 1;
                if self.begin == self.end {
                    self.end = self.next_break_point(self.begin);
                }
            } else {
                self.begin += 1;
                if self.begin == self.end {
                    self.begin -= 1;
                    self.begin = self.after_previous_break_point(self.begin);
                }
            }
            let symbols = &self.com.symbols()[self.begin..self.end];
            let syllables: Vec<Syllable> = symbols
                .iter()
                .map(|s| s.to_syllable().unwrap_or_default())
                .collect();
            if !dict.lookup(&syllables, self.lookup_strategy).is_empty() {
                break;
            }
        }
    }

    fn next_break_point(&self, mut cursor: usize) -> usize {
        loop {
            if self.com.len() == cursor {
                break;
            }
            if let Some(sym) = self.com.symbol(cursor) {
                if !sym.is_syllable() {
                    break;
                }
            }
            cursor += 1;
        }
        cursor
    }

    fn after_previous_break_point(&self, mut cursor: usize) -> usize {
        loop {
            if cursor == 0 {
                return 0;
            }
            if let Some(Gap::Break) = self.com.gap(cursor) {
                break;
            }
            if let Some(sym) = self.com.symbol(cursor - 1) {
                if !sym.is_syllable() {
                    break;
                }
            }
            cursor -= 1;
        }
        cursor
    }

    /// Build the candidate phrase list (dict-only, no alt-syllable expansion).
    ///
    /// For a range of length R starting at `begin`, this returns:
    ///   * all phrases of length R covering [begin, end)
    ///   * for R > 1, all phrases of intermediate lengths 2..R starting at `begin`
    ///     (longest first)
    ///   * for R > 1, all single-character phrases at the cursor position
    ///
    /// Splitting this out lets us unit-test multi-length lookup without
    /// constructing a full `SharedState`.
    pub(crate) fn candidate_phrases(&self, dict: &Layered) -> Vec<Phrase> {
        let range = self.end - self.begin;
        let syllables: Vec<Syllable> = self.com.symbols()[self.begin..self.end]
            .iter()
            .map(|s| s.to_syllable().unwrap_or_default())
            .collect();
        let mut out = dict.lookup(&syllables, self.lookup_strategy);
        if range > 1 {
            for len in (2..range).rev() {
                out.extend(
                    dict.lookup(&syllables[..len], self.lookup_strategy)
                        .into_iter(),
                );
            }
            let single_pos = if self.orig < self.end { self.orig } else { self.begin };
            if let Some(sym) = self.com.symbol(single_pos) {
                if let Some(syl) = sym.to_syllable() {
                    out.extend(dict.lookup(&[syl], self.lookup_strategy).into_iter());
                }
            }
        }
        out
    }

    pub(crate) fn candidates(&self, editor: &SharedState, dict: &Layered) -> Vec<String> {
        let mut candidates = self.candidate_phrases(dict);
        let range = self.end - self.begin;
        if range > 1 {
            let single_pos = if self.orig < self.end { self.orig } else { self.begin };
            if let Some(sym) = self.com.symbol(single_pos) {
                if let Some(syl) = sym.to_syllable() {
                    let alt = editor.syl.alt_syllables(syl);
                    for &alt_syl in alt {
                        candidates.extend(dict.lookup(&[alt_syl], self.lookup_strategy).into_iter());
                    }
                }
            }
        } else {
            let alt = editor
                .syl
                .alt_syllables(self.com.symbol(self.begin).unwrap().to_syllable().unwrap());
            for &syl in alt {
                candidates.extend(dict.lookup(&[syl], self.lookup_strategy).into_iter())
            }
        }
        if editor.options.sort_candidates_by_frequency {
            candidates.sort_by_key(|ph| Reverse(ph.freq()));
        }
        candidates.into_iter().map(|ph| ph.into()).collect()
    }

    /// Build interval for the selected phrase.
    /// For single-character selections from a multi-char range, narrow the interval
    /// to just the character at the cursor position.
    pub(crate) fn interval(&self, phrase: impl Into<Box<str>>) -> Interval {
        let text: Box<str> = phrase.into();
        let phrase_chars = text.chars().count();
        let range_len = self.end - self.begin;
        if phrase_chars < range_len {
            let start = if self.orig < self.end { self.orig } else { self.begin };
            Interval {
                start,
                end: start + phrase_chars,
                is_phrase: true,
                text,
            }
        } else {
            Interval {
                start: self.begin,
                end: self.end,
                is_phrase: true,
                text,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PhraseSelector;
    use crate::{
        conversion::{Composition, Symbol},
        dictionary::{LookupStrategy, TrieBuf},
        syl,
        zhuyin::Bopomofo::*,
    };

    #[test]
    fn init_when_cursor_end_of_buffer_syllable() {
        let mut com = Composition::new();
        com.push(Symbol::from(syl![C, E, TONE4]));
        let mut sel = PhraseSelector {
            begin: 0,
            end: 1,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };
        let dict = TrieBuf::from([(vec![syl![C, E, TONE4]], vec![("測", 100)])]);
        sel.init(1, &dict);

        assert_eq!(0, sel.begin);
        assert_eq!(1, sel.end);
    }

    #[test]
    #[should_panic]
    fn init_when_cursor_end_of_buffer_not_syllable() {
        let mut com = Composition::new();
        com.push(Symbol::from(','));
        let mut sel = PhraseSelector {
            begin: 0,
            end: 1,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };
        let dict = TrieBuf::from([(vec![syl![C, E, TONE4]], vec![("測", 100)])]);
        sel.init(1, &dict);
    }

    #[test]
    fn init_forward_select_when_cursor_end_of_buffer_syllable() {
        let mut com = Composition::new();
        com.push(Symbol::from(syl![C, E, TONE4]));
        let mut sel = PhraseSelector {
            begin: 0,
            end: 1,
            forward_select: true,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };
        let dict = TrieBuf::from([(vec![syl![C, E, TONE4]], vec![("測", 100)])]);
        sel.init(1, &dict);

        assert_eq!(0, sel.begin);
        assert_eq!(1, sel.end);
    }

    #[test]
    #[should_panic]
    fn init_forward_select_when_cursor_end_of_buffer_not_syllable() {
        let mut com = Composition::new();
        com.push(Symbol::from(','));
        let mut sel = PhraseSelector {
            begin: 0,
            end: 1,
            forward_select: true,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };
        let dict = TrieBuf::from([(vec![syl![C, E, TONE4]], vec![("測", 100)])]);
        sel.init(1, &dict);
    }

    #[test]
    fn should_stop_at_left_boundary() {
        let mut com = Composition::new();
        for sym in [
            Symbol::from(syl![C, E, TONE4]),
            Symbol::from(syl![C, E, TONE4]),
        ] {
            com.push(sym);
        }
        let sel = PhraseSelector {
            begin: 0,
            end: 2,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };

        assert_eq!(0, sel.after_previous_break_point(0));
        assert_eq!(0, sel.after_previous_break_point(1));
        assert_eq!(0, sel.after_previous_break_point(2));
    }

    #[test]
    fn candidate_phrases_includes_intermediate_lengths_for_3char_range() {
        use crate::dictionary::Layered;

        // Buffer: ㄧˋ ㄕˋ ㄐㄧㄝˋ
        let mut com = Composition::new();
        com.push(Symbol::from(syl![I, TONE4]));
        com.push(Symbol::from(syl![SH, TONE4]));
        com.push(Symbol::from(syl![J, I, EH, TONE4]));

        // Dict has phrases at all three lengths starting at position 0.
        let trie = TrieBuf::from([
            (
                vec![syl![I, TONE4], syl![SH, TONE4], syl![J, I, EH, TONE4]],
                vec![("異世界", 5000)],
            ),
            (
                vec![syl![I, TONE4], syl![SH, TONE4]],
                vec![("意識", 2273), ("亦是", 3000)],
            ),
            (vec![syl![I, TONE4]], vec![("異", 3257), ("意", 14786)]),
        ]);
        let dict = Layered::new(vec![Box::new(trie)]);

        let sel = PhraseSelector {
            begin: 0,
            end: 3,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };

        let phrases = sel.candidate_phrases(&dict);
        let texts: Vec<&str> = phrases.iter().map(|p| p.as_str()).collect();

        assert!(
            texts.contains(&"異世界"),
            "missing 3-char phrase: {:?}",
            texts
        );
        assert!(
            texts.contains(&"意識") || texts.contains(&"亦是"),
            "missing 2-char phrase at begin: {:?}",
            texts
        );
        assert!(
            texts.contains(&"異") || texts.contains(&"意"),
            "missing 1-char phrase at cursor: {:?}",
            texts
        );
    }

    #[test]
    fn candidate_phrases_2char_range_unchanged() {
        // Range == 2: no intermediate lengths exist, behavior should match
        // the pre-change semantics (full-range phrases + 1-char at cursor).
        use crate::dictionary::Layered;

        let mut com = Composition::new();
        com.push(Symbol::from(syl![SH, TONE4]));
        com.push(Symbol::from(syl![J, I, EH, TONE4]));

        let trie = TrieBuf::from([
            (
                vec![syl![SH, TONE4], syl![J, I, EH, TONE4]],
                vec![("世界", 30817), ("視界", 470)],
            ),
            (vec![syl![SH, TONE4]], vec![("世", 100), ("是", 200)]),
        ]);
        let dict = Layered::new(vec![Box::new(trie)]);

        let sel = PhraseSelector {
            begin: 0,
            end: 2,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };

        let phrases = sel.candidate_phrases(&dict);
        let texts: Vec<&str> = phrases.iter().map(|p| p.as_str()).collect();

        assert!(texts.contains(&"世界"), "missing 世界: {:?}", texts);
        assert!(
            texts.contains(&"世") || texts.contains(&"是"),
            "missing 1-char at cursor: {:?}",
            texts
        );
    }

    #[test]
    fn candidate_phrases_1char_range_only_full_lookup() {
        // Range == 1: only the full-range lookup runs; no intermediate, no
        // separate cursor-1-char branch (it's the same as the full lookup).
        use crate::dictionary::Layered;

        let mut com = Composition::new();
        com.push(Symbol::from(syl![I, TONE4]));

        let trie = TrieBuf::from([(vec![syl![I, TONE4]], vec![("異", 3257), ("意", 14786)])]);
        let dict = Layered::new(vec![Box::new(trie)]);

        let sel = PhraseSelector {
            begin: 0,
            end: 1,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };

        let phrases = sel.candidate_phrases(&dict);
        let texts: Vec<&str> = phrases.iter().map(|p| p.as_str()).collect();

        assert!(texts.contains(&"異"), "missing 異: {:?}", texts);
        assert!(texts.contains(&"意"), "missing 意: {:?}", texts);
        // No phrase should be longer than 1 char.
        for t in &texts {
            assert_eq!(
                t.chars().count(),
                1,
                "unexpected multi-char phrase in 1-char range: {}",
                t
            );
        }
    }

    #[test]
    fn should_stop_after_first_non_syllable() {
        let mut com = Composition::new();
        for sym in [Symbol::from(','), Symbol::from(syl![C, E, TONE4])] {
            com.push(sym);
        }
        let sel = PhraseSelector {
            begin: 0,
            end: 2,
            forward_select: false,
            orig: 0,
            lookup_strategy: LookupStrategy::Standard,
            com,
        };

        assert_eq!(0, sel.after_previous_break_point(0));
        assert_eq!(1, sel.after_previous_break_point(1));
        assert_eq!(1, sel.after_previous_break_point(2));
    }
}
