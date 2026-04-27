//! Lightweight construction-pattern scoring.
//!
//! This module is intentionally small and deterministic. It only scores the
//! conversion paths that already survived normal dictionary lookup; it never
//! expands candidates or parses external rule files at runtime.

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ConstructionPattern {
    pub(crate) first_anchor: &'static str,
    pub(crate) second_anchor: &'static str,
    pub(crate) min_gap: usize,
    pub(crate) max_gap: usize,
    pub(crate) bonus: PatternBonus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatternBonus(pub(crate) f64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatternReranker {
    patterns: &'static [ConstructionPattern],
}

impl Default for PatternReranker {
    fn default() -> Self {
        Self {
            patterns: DEFAULT_PATTERNS,
        }
    }
}

impl PatternReranker {
    pub(crate) const MAX_TOKENS: usize = 16;

    pub(crate) fn score(&self, tokens: &[&str]) -> f64 {
        let mut score = 0.0;
        for pattern in self.patterns {
            if matches_pattern(tokens, pattern) {
                score += pattern.bonus.0;
            }
        }
        score
    }
}

const DEFAULT_PATTERNS: &[ConstructionPattern] = &[
    ConstructionPattern {
        first_anchor: "越",
        second_anchor: "越",
        min_gap: 1,
        max_gap: 4,
        bonus: PatternBonus(8.0),
    },
    ConstructionPattern {
        first_anchor: "一邊",
        second_anchor: "一邊",
        min_gap: 1,
        max_gap: 8,
        bonus: PatternBonus(6.0),
    },
    ConstructionPattern {
        first_anchor: "不但",
        second_anchor: "而且",
        min_gap: 1,
        max_gap: 12,
        bonus: PatternBonus(6.0),
    },
    ConstructionPattern {
        first_anchor: "如果",
        second_anchor: "就",
        min_gap: 1,
        max_gap: 12,
        bonus: PatternBonus(5.0),
    },
    ConstructionPattern {
        first_anchor: "不是",
        second_anchor: "而是",
        min_gap: 1,
        max_gap: 12,
        bonus: PatternBonus(6.0),
    },
    ConstructionPattern {
        first_anchor: "與其",
        second_anchor: "不如",
        min_gap: 1,
        max_gap: 12,
        bonus: PatternBonus(6.0),
    },
    ConstructionPattern {
        first_anchor: "連",
        second_anchor: "都",
        min_gap: 1,
        max_gap: 8,
        bonus: PatternBonus(5.0),
    },
];

fn matches_pattern(tokens: &[&str], pattern: &ConstructionPattern) -> bool {
    for first in 0..tokens.len() {
        if tokens[first] != pattern.first_anchor {
            continue;
        }
        let min_second = first + pattern.min_gap + 1;
        let max_second = first + pattern.max_gap + 1;
        for second in min_second..=max_second.min(tokens.len().saturating_sub(1)) {
            if tokens[second] == pattern.second_anchor {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::PatternReranker;

    #[test]
    fn scores_matching_anchor_pair_inside_gap() {
        let reranker = PatternReranker::default();
        assert!(reranker.score(&["越", "看", "越"]) > 0.0);
    }

    #[test]
    fn ignores_anchor_pair_outside_gap() {
        let reranker = PatternReranker::default();
        assert_eq!(
            0.0,
            reranker.score(&["越", "甲", "乙", "丙", "丁", "戊", "越"])
        );
    }

    #[test]
    fn scores_generic_patterns_through_same_mechanism() {
        let reranker = PatternReranker::default();
        assert!(reranker.score(&["不但", "快", "而且", "穩"]) > 0.0);
        assert!(reranker.score(&["如果", "下雨", "就", "回家"]) > 0.0);
    }
}
