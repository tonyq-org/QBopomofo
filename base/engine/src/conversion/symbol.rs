macro_rules! symbol_map {
    ($($k:expr => $v:expr),* $(,)?) => {{
        [$(($k, $v),)*]
    }};
}

static SPECIAL_SYMBOLS: [(char, char); 29] = symbol_map! {
    '[' => '「', ']' => '」', '{' => '﹃', '}' => '﹄',
    '\'' => '、', '<' => '，', ':' => '：', '\"' => '；',
    '>' => '。', '~' => '～', '!' => '！', '@' => '＠',
    '#' => '＃', '$' => '＄', '%' => '％', '^' => '︿',
    '&' => '＆', '*' => '＊', '(' => '（', ')' => '）',
    '_' => '—', '+' => '＋', '=' => '＝', '\\' => '＼',
    '|' => '｜', '?' => '？', ',' => '，', '.' => '。',
    ';' => '；',
};

static FULL_WIDTH_SYMBOLS: [(char, char); 75] = symbol_map! {
    '0' => '０', '1' => '１', '2' => '２', '3' => '３',
    '4' => '４', '5' => '５', '6' => '６', '7' => '７',
    '8' => '８', '9' => '９', 'a' => 'ａ', 'b' => 'ｂ',
    'c' => 'ｃ', 'd' => 'ｄ', 'e' => 'ｅ', 'f' => 'ｆ',
    'g' => 'ｇ', 'h' => 'ｈ', 'i' => 'ｉ', 'j' => 'ｊ',
    'k' => 'ｋ', 'l' => 'ｌ', 'm' => 'ｍ', 'n' => 'ｎ',
    'o' => 'ｏ', 'p' => 'ｐ', 'q' => 'ｑ', 'r' => 'ｒ',
    's' => 'ｓ', 't' => 'ｔ', 'u' => 'ｕ', 'v' => 'ｖ',
    'w' => 'ｗ', 'x' => 'ｘ', 'y' => 'ｙ', 'z' => 'ｚ',
    'A' => 'Ａ', 'B' => 'Ｂ', 'C' => 'Ｃ', 'D' => 'Ｄ',
    'E' => 'Ｅ', 'F' => 'Ｆ', 'G' => 'Ｇ', 'H' => 'Ｈ',
    'I' => 'Ｉ', 'J' => 'Ｊ', 'K' => 'Ｋ', 'L' => 'Ｌ',
    'M' => 'Ｍ', 'N' => 'Ｎ', 'O' => 'Ｏ', 'P' => 'Ｐ',
    'Q' => 'Ｑ', 'R' => 'Ｒ', 'S' => 'Ｓ', 'T' => 'Ｔ',
    'U' => 'Ｕ', 'V' => 'Ｖ', 'W' => 'Ｗ', 'X' => 'Ｘ',
    'Y' => 'Ｙ', 'Z' => 'Ｚ', ' ' => '　', '\"' => '”',
    '\'' => '’', '/' => '／', '<' => '＜', '>' => '＞',
    '`' => '‵', '[' => '〔', ']' =>'〕', '{' => '｛',
    '}' => '｝', '+' => '＋', '-' => '－',
};

pub(crate) fn special_symbol_input(key: char) -> Option<char> {
    SPECIAL_SYMBOLS
        .iter()
        .find(|item| item.0 == key)
        .map(|item| item.1)
}

pub(crate) fn full_width_symbol_input(key: char) -> Option<char> {
    FULL_WIDTH_SYMBOLS
        .iter()
        .find(|item| item.0 == key)
        .map(|item| item.1)
        .or_else(|| special_symbol_input(key))
}

#[cfg(test)]
mod tests {
    use super::{full_width_symbol_input, special_symbol_input};

    #[test]
    fn shifted_brackets_use_narrow_corner_quotes() {
        assert_eq!(Some('﹃'), special_symbol_input('{'));
        assert_eq!(Some('﹄'), special_symbol_input('}'));
    }

    #[test]
    fn unshifted_brackets_keep_corner_quotes() {
        assert_eq!(Some('「'), special_symbol_input('['));
        assert_eq!(Some('」'), special_symbol_input(']'));
    }

    #[test]
    fn full_width_braces_stay_full_width_braces() {
        assert_eq!(Some('｛'), full_width_symbol_input('{'));
        assert_eq!(Some('｝'), full_width_symbol_input('}'));
    }
}
