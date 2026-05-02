use std::{cell::RefCell, path::PathBuf};

use qbopomofo_tip::controller::{Controller, InputSink, VK_RETURN};

#[derive(Default)]
struct RecordingSink {
    preedits: RefCell<Vec<String>>,
    commits: RefCell<Vec<String>>,
}

impl InputSink for RecordingSink {
    fn update_preedit(&self, text: &str) -> Option<(i32, i32)> {
        self.preedits.borrow_mut().push(text.to_string());
        None
    }

    fn commit_text(&self, text: &str) {
        self.commits.borrow_mut().push(text.to_string());
    }

    fn end_preedit(&self) {}

    fn show_candidates(
        &self,
        _cands: &[String],
        _selection_keys: &[char],
        _highlight: usize,
        _page_info: &str,
        _caret_pos: Option<(i32, i32)>,
    ) {
    }

    fn hide_candidates(&self) {}
}

#[test]
fn custom_phrase_zhe_ge_shi_is_default_commit() {
    let dict_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("data-provider")
        .join("output");

    if !dict_path.join("tsi.dat").exists() || !dict_path.join("word.dat").exists() {
        eprintln!(
            "skipping: generated dictionaries are missing at {}",
            dict_path.display()
        );
        return;
    }

    let sink = RecordingSink::default();
    let mut controller = Controller::new();
    controller.activate(Some(dict_path.to_string_lossy().into_owned()));

    type_chars(&mut controller, "5k4ek4g4", &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.preedits.borrow().last().map(String::as_str), Some("這個是"));
    assert_eq!(sink.commits.borrow().concat(), "這個是");
}

#[test]
fn custom_phrase_ling_yi_ge_is_default_commit() {
    let dict_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("data-provider")
        .join("output");

    if !dict_path.join("tsi.dat").exists() || !dict_path.join("word.dat").exists() {
        eprintln!(
            "skipping: generated dictionaries are missing at {}",
            dict_path.display()
        );
        return;
    }

    let sink = RecordingSink::default();
    let mut controller = Controller::new();
    controller.activate(Some(dict_path.to_string_lossy().into_owned()));

    type_chars(&mut controller, "xu/4u", &sink);
    controller.on_key_down(0x20, ' ', false, false, false, &sink);
    type_chars(&mut controller, "ek4", &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.preedits.borrow().last().map(String::as_str), Some("另一個"));
    assert_eq!(sink.commits.borrow().concat(), "另一個");
}

#[test]
fn tuned_single_char_zhu_prefers_zhu3_master_over_cook() {
    let dict_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("data-provider")
        .join("output");

    if !dict_path.join("tsi.dat").exists() || !dict_path.join("word.dat").exists() {
        eprintln!(
            "skipping: generated dictionaries are missing at {}",
            dict_path.display()
        );
        return;
    }

    let sink = RecordingSink::default();
    let mut controller = Controller::new();
    controller.activate(Some(dict_path.to_string_lossy().into_owned()));

    type_chars(&mut controller, "5j3", &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.preedits.borrow().last().map(String::as_str), Some("主"));
    assert_eq!(sink.commits.borrow().concat(), "主");
}

fn type_chars(controller: &mut Controller, input: &str, sink: &RecordingSink) {
    for ch in input.chars() {
        let (vkey, shift) = char_to_vkey(ch);
        controller.on_key_down(vkey, ch, shift, false, false, sink);
    }
}

fn char_to_vkey(ch: char) -> (u32, bool) {
    match ch {
        'a'..='z' => (ch as u32 - 'a' as u32 + 0x41, false),
        'A'..='Z' => (ch as u32 - 'A' as u32 + 0x41, true),
        '0'..='9' => (ch as u32 - '0' as u32 + 0x30, false),
        ' ' => (0x20, false),
        ';' => (0xBA, false),
        ':' => (0xBA, true),
        '=' => (0xBB, false),
        '+' => (0xBB, true),
        ',' => (0xBC, false),
        '<' => (0xBC, true),
        '-' => (0xBD, false),
        '_' => (0xBD, true),
        '.' => (0xBE, false),
        '>' => (0xBE, true),
        '/' => (0xBF, false),
        '?' => (0xBF, true),
        '`' => (0xC0, false),
        '~' => (0xC0, true),
        '[' => (0xDB, false),
        '{' => (0xDB, true),
        '\\' => (0xDC, false),
        '|' => (0xDC, true),
        ']' => (0xDD, false),
        '}' => (0xDD, true),
        '\'' => (0xDE, false),
        '"' => (0xDE, true),
        '!' => (0x31, true),
        '@' => (0x32, true),
        '#' => (0x33, true),
        '$' => (0x34, true),
        '%' => (0x35, true),
        '^' => (0x36, true),
        '&' => (0x37, true),
        '*' => (0x38, true),
        '(' => (0x39, true),
        ')' => (0x30, true),
        _ => (0, false),
    }
}
