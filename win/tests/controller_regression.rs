use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
};

use qbopomofo_tip::controller::{
    Controller, EditOutcome, InputSink, VK_BACK, VK_DOWN, VK_LEFT, VK_RETURN, VK_SHIFT, VK_UP,
};
use qbopomofo_tip::preferences::{CandidateOrdering, Preferences};
use chewing::typing_mode::CapsLockBehavior;

#[derive(Default)]
struct RecordingSink {
    preedits: RefCell<Vec<String>>,
    cursors_utf16: RefCell<Vec<usize>>,
    commits: RefCell<Vec<String>>,
    candidate_pages: RefCell<Vec<Vec<String>>>,
    candidate_highlights: RefCell<Vec<usize>>,
    preedit_rejections_remaining: Cell<usize>,
    commit_failures_remaining: Cell<usize>,
    commit_rejections_remaining: Cell<usize>,
    context_id: Cell<usize>,
    end_preedit_count: Cell<usize>,
}

impl InputSink for RecordingSink {
    fn update_preedit(
        &self,
        text: &str,
        cursor_utf16: usize,
        _needs_caret_position: bool,
        _update_selection: bool,
    ) -> EditOutcome<Option<(i32, i32)>> {
        let rejections = self.preedit_rejections_remaining.get();
        if rejections > 0 {
            self.preedit_rejections_remaining.set(rejections - 1);
            return EditOutcome::Rejected;
        }
        self.preedits.borrow_mut().push(text.to_string());
        self.cursors_utf16.borrow_mut().push(cursor_utf16);
        EditOutcome::Applied(None)
    }

    fn commit_text(&self, text: &str) -> EditOutcome<()> {
        let rejections = self.commit_rejections_remaining.get();
        if rejections > 0 {
            self.commit_rejections_remaining.set(rejections - 1);
            return EditOutcome::Rejected;
        }
        let failures = self.commit_failures_remaining.get();
        if failures > 0 {
            self.commit_failures_remaining.set(failures - 1);
            return EditOutcome::Retryable;
        }
        self.commits.borrow_mut().push(text.to_string());
        EditOutcome::Applied(())
    }

    fn edit_context_id(&self) -> usize {
        self.context_id.get()
    }

    fn end_preedit(&self) -> bool {
        self.end_preedit_count
            .set(self.end_preedit_count.get() + 1);
        true
    }

    fn show_candidates(
        &self,
        cands: &[String],
        _selection_keys: &[char],
        highlight: usize,
        _page_info: &str,
        _caret_pos: Option<(i32, i32)>,
    ) {
        self.candidate_pages.borrow_mut().push(cands.to_vec());
        self.candidate_highlights.borrow_mut().push(highlight);
    }

    fn hide_candidates(&self) {}
}

#[test]
fn smart_candidates_put_default_first_and_arrow_navigation_wraps() {
    let prefs = Preferences {
        candidate_ordering: CandidateOrdering::Smart,
        ..Preferences::default()
    };
    let Some(mut controller) = activated_controller_with_preferences(&prefs) else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4", &sink); // 測
    controller.on_key_down(VK_DOWN, '\0', false, false, false, &sink);

    let page = sink.candidate_pages.borrow().last().cloned().unwrap();
    assert_eq!(page.first().map(String::as_str), Some("測"));
    assert!(page.len() > 1);

    controller.on_key_down(VK_UP, '\0', false, false, false, &sink);
    assert_eq!(
        sink.candidate_highlights.borrow().last().copied(),
        Some(page.len() - 1)
    );
    controller.on_key_down(VK_DOWN, '\0', false, false, false, &sink);
    assert_eq!(sink.candidate_highlights.borrow().last().copied(), Some(0));

    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert!(!controller.is_selecting());
    assert_eq!(sink.preedits.borrow().last().map(String::as_str), Some("測"));
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

#[test]
fn mixed_chinese_english_chinese_commits_in_display_order() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4g4", &sink); // 測試
    type_temporary_english(&mut controller, "A", &sink);
    type_chars(&mut controller, "hk4", &sink); // 測
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(
        sink.preedits.borrow().last().map(String::as_str),
        Some("測試A測")
    );
    assert_eq!(sink.commits.borrow().as_slice(), ["測試A測"]);
}

#[test]
fn mixed_visible_length_auto_flushes_without_repeating_prefix() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_syllable_n(&mut controller, "hk4", 5, &sink);
    type_temporary_english(&mut controller, "A", &sink);
    type_syllable_n(&mut controller, "hk4", 15, &sink);

    let expected = format!("{}A{}", "測".repeat(5), "測".repeat(15));
    assert_eq!(
        sink.commits.borrow().as_slice(),
        std::slice::from_ref(&expected)
    );
    assert!(
        !controller.has_content(),
        "auto-flush must clear editor and mixed session together"
    );

    // Continue after the automatic flush. The next commit must contain only
    // the newly typed character, never chewing's old retained prefix.
    type_chars(&mut controller, "hk4", &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert_eq!(
        sink.commits.borrow().as_slice(),
        [expected, "測".to_string()]
    );
    assert_eq!(
        sink.commits.borrow().concat(),
        format!("{}A{}", "測".repeat(5), "測".repeat(16))
    );
}

#[test]
fn mixed_backspace_after_returning_to_chinese_deletes_english() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4", &sink); // 測
    type_temporary_english(&mut controller, "A", &sink);
    controller.on_key_down(VK_BACK, '\0', false, false, false, &sink);

    assert_eq!(
        sink.preedits.borrow().last().map(String::as_str),
        Some("測")
    );
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert_eq!(sink.commits.borrow().as_slice(), ["測"]);
}

#[test]
fn mixed_cursor_can_insert_inside_english_run() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4g4", &sink); // 測試
    type_temporary_english(&mut controller, "A", &sink);
    controller.on_key_down(VK_LEFT, '\0', false, false, false, &sink);

    // Short Shift toggles persistent English mode, then b is inserted before A.
    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    controller.on_key_up(VK_SHIFT, &sink);
    controller.on_key_down(0x42, 'b', false, false, false, &sink);

    assert_eq!(
        sink.preedits.borrow().last().map(String::as_str),
        Some("測試bA")
    );
    assert_eq!(sink.cursors_utf16.borrow().last().copied(), Some(3));
}

#[test]
fn mixed_cursor_can_delete_an_earlier_english_run() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4", &sink); // 測
    type_temporary_english(&mut controller, "A", &sink);
    type_chars(&mut controller, "g4", &sink); // 試
    type_temporary_english(&mut controller, "B", &sink);
    controller.on_key_down(VK_LEFT, '\0', false, false, false, &sink);
    controller.on_key_down(VK_LEFT, '\0', false, false, false, &sink);
    controller.on_key_down(VK_BACK, '\0', false, false, false, &sink);

    assert_eq!(
        sink.preedits.borrow().last().map(String::as_str),
        Some("測試B")
    );
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert_eq!(sink.commits.borrow().as_slice(), ["測試B"]);
}

#[test]
fn english_without_composition_is_passed_through() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    controller.on_key_up(VK_SHIFT, &sink);

    assert!(!controller.should_eat_key_down(0x41, 'a', false, false));
    assert!(!controller.on_key_down(0x41, 'a', false, false, false, &sink));
    assert!(sink.commits.borrow().is_empty());
}

#[test]
fn temporary_shift_english_without_composition_is_committed_by_tip() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    assert!(controller.should_eat_key_down(0x41, 'A', false, false));
    assert!(controller.on_key_down(0x41, 'A', true, false, false, &sink));
    controller.on_key_up(VK_SHIFT, &sink);

    assert_eq!(sink.commits.borrow().as_slice(), ["A"]);
    assert!(controller.should_eat_key_down(0x48, 'h', false, false));
}

#[test]
fn temporary_shift_english_replaces_lone_unfinished_bopomofo() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "h", &sink); // unfinished ㄘ only
    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    assert!(controller.should_eat_key_down(0x41, 'A', false, false));
    assert!(controller.on_key_down(0x41, 'A', true, false, false, &sink));
    controller.on_key_up(VK_SHIFT, &sink);

    assert_eq!(sink.commits.borrow().as_slice(), ["A"]);
    assert!(!controller.has_content());
}

#[test]
fn rejected_platform_commit_is_retried_without_duplication() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();
    sink.commit_failures_remaining.set(1);

    type_chars(&mut controller, "hk4", &sink); // 測
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert!(sink.commits.borrow().is_empty());
    assert!(
        controller.has_content(),
        "pending commit must keep the next key routed to us"
    );

    // Even an unmapped key is routed through us once, so the pending commit
    // cannot remain stranded or be reordered behind application input.
    assert!(controller.should_eat_key_down(0x70, '\0', false, false)); // F1
    assert!(!controller.on_key_down(0x70, '\0', false, false, false, &sink));
    assert_eq!(sink.commits.borrow().as_slice(), ["測"]);

    controller.on_key_down(0x48, 'h', false, false, false, &sink);
    assert_eq!(
        sink.preedits.borrow().last().map(String::as_str),
        Some("ㄘ")
    );
}

#[test]
fn terminal_commit_rejection_does_not_trap_enter() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();
    sink.commit_rejections_remaining.set(1);

    type_chars(&mut controller, "hk4", &sink); // 測
    assert!(controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink));

    assert!(sink.commits.borrow().is_empty());
    assert!(!controller.has_content());
    assert!(!controller.should_eat_key_down(VK_RETURN, '\r', false, false));
    assert!(!controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink));
}

#[test]
fn repeated_retryable_commit_failure_is_bounded() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();
    sink.commit_failures_remaining.set(2);

    type_chars(&mut controller, "hk4", &sink); // 測
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert!(controller.has_content(), "first transient failure is pending");

    assert!(controller.should_eat_key_down(VK_RETURN, '\r', false, false));
    assert!(!controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink));
    assert!(!controller.has_content(), "failed retry must release input");
    assert!(sink.commits.borrow().is_empty());
}

#[test]
fn pending_commit_is_not_replayed_into_another_context() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();
    sink.context_id.set(1);
    sink.commit_failures_remaining.set(1);

    type_chars(&mut controller, "hk4", &sink); // 測
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);
    assert!(controller.has_content());

    sink.context_id.set(2);
    assert!(controller.should_eat_key_down(0x70, '\0', false, false)); // F1
    assert!(!controller.on_key_down(0x70, '\0', false, false, false, &sink));
    assert!(!controller.has_content());
    assert!(sink.commits.borrow().is_empty());
}

#[test]
fn rejected_preedit_clears_invisible_controller_input() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();
    sink.preedit_rejections_remaining.set(1);

    assert!(controller.on_key_down(0x48, 'h', false, false, false, &sink));

    assert!(!controller.has_content());
    assert!(sink.preedits.borrow().is_empty());
    assert!(!controller.should_eat_key_down(VK_RETURN, '\r', false, false));
}

#[test]
fn numpad_digit_is_inserted_inline_during_chinese_composition() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4h", &sink); // 測 + unfinished ㄘ
    assert!(controller.should_eat_key_down(0x61, '1', false, false));
    controller.on_key_down(0x61, '1', false, false, false, &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.commits.borrow().as_slice(), ["測1"]);
}

#[test]
fn temporary_english_replaces_unfinished_bopomofo_after_chinese() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4h", &sink); // 測 + unfinished ㄘ
    type_temporary_english(&mut controller, "A", &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.commits.borrow().as_slice(), ["測A"]);
}

#[test]
fn mixed_backspace_clears_unfinished_bopomofo_before_english() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4", &sink); // 測
    type_temporary_english(&mut controller, "A", &sink);
    type_chars(&mut controller, "h", &sink); // unfinished ㄘ after A
    controller.on_key_down(VK_BACK, '\0', false, false, false, &sink);

    assert_eq!(
        sink.preedits.borrow().last().map(String::as_str),
        Some("測A")
    );
}

#[test]
fn switching_to_english_clears_partial_bopomofo() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk", &sink); // ㄘㄜ, not committed yet
    assert!(controller.has_content());
    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    controller.on_key_up(VK_SHIFT, &sink);

    assert!(!controller.has_content());
    assert_eq!(sink.end_preedit_count.get(), 1);
}

#[test]
fn caps_english_run_stays_between_surrounding_chinese() {
    let prefs = Preferences {
        caps_lock_behavior: CapsLockBehavior::ToggleChineseEnglish,
        ..Preferences::default()
    };
    let Some(mut controller) = activated_controller_with_preferences(&prefs) else {
        return;
    };
    let sink = RecordingSink::default();

    type_chars(&mut controller, "hk4", &sink); // 測
    controller.on_key_down(0x42, 'B', false, false, true, &sink);
    type_chars(&mut controller, "g4", &sink); // Caps off, then 試
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.commits.borrow().as_slice(), ["測B試"]);
}

#[test]
fn shift_punctuation_remains_chinese_and_does_not_toggle_mode() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    controller.on_key_down(0xBC, '<', true, false, false, &sink);
    controller.on_key_up(VK_SHIFT, &sink);
    controller.on_key_down(VK_RETURN, '\r', false, false, false, &sink);

    assert_eq!(sink.commits.borrow().as_slice(), ["，"]);
    assert!(controller.should_eat_key_down(0x48, 'h', false, false));
}

#[test]
fn shift_modified_passthrough_key_does_not_toggle_english_mode() {
    let Some(mut controller) = activated_controller() else {
        return;
    };
    let sink = RecordingSink::default();

    controller.on_key_down(VK_SHIFT, '\0', true, false, false, &sink);
    assert!(controller.should_eat_key_down(VK_RETURN, '\r', false, false));
    assert!(!controller.on_key_down(VK_RETURN, '\r', true, false, false, &sink));
    controller.on_key_up(VK_SHIFT, &sink);

    // Chinese mode still wants a normal letter; an accidental Shift toggle
    // would make this native English and return false.
    assert!(controller.should_eat_key_down(0x48, 'h', false, false));
}

fn type_chars(controller: &mut Controller, input: &str, sink: &RecordingSink) {
    for ch in input.chars() {
        let (vkey, shift) = char_to_vkey(ch);
        controller.on_key_down(vkey, ch, shift, false, false, sink);
    }
}

fn type_temporary_english(controller: &mut Controller, input: &str, sink: &RecordingSink) {
    controller.on_key_down(VK_SHIFT, '\0', true, false, false, sink);
    type_chars(controller, input, sink);
    controller.on_key_up(VK_SHIFT, sink);
}

fn type_syllable_n(
    controller: &mut Controller,
    syllable: &str,
    count: usize,
    sink: &RecordingSink,
) {
    for _ in 0..count {
        type_chars(controller, syllable, sink);
    }
}

fn activated_controller() -> Option<Controller> {
    activated_controller_with_preferences(&Preferences::default())
}

fn activated_controller_with_preferences(prefs: &Preferences) -> Option<Controller> {
    let dict_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("data-provider")
        .join("output");
    if !dict_path.join("tsi.dat").exists() || !dict_path.join("word.dat").exists() {
        eprintln!(
            "skipping: generated dictionaries are missing at {}",
            dict_path.display()
        );
        return None;
    }
    let mut controller = Controller::new();
    controller.activate_with_preferences(Some(dict_path.to_string_lossy().into_owned()), prefs);
    Some(controller)
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
