//! Headless CLI test harness for the controller.
//!
//! Drives `qbopomofo_tip::controller::Controller` with keystrokes read from
//! stdin and prints preedit/commit/candidate events to stdout. Has no
//! COM, no TSF, no window — perfect for iterating on input logic.
//!
//! ## Input format (one command per line)
//!
//! ```
//! KEY <hex> [flags]        # raw Windows VK; flags: shift,ctrl,caps
//! TYPE <string>            # send each printable char as a key
//! #  ... anything          # comment, ignored
//!                          # blank line, ignored
//! ```
//!
//! Example:
//! ```
//! TYPE 5j/
//! KEY 0x0D                 # Enter to commit
//! ```
//!
//! ## Output format
//!
//! ```
//! PREEDIT: "ㄊㄞˊ"
//! COMMIT: "台灣"
//! END_PREEDIT
//! CAND: 1.台 2.臺 *3.抬    (page 1/3)
//! HIDE
//! ```
//!
//! The caret position is reported as `None` since there's no window.
//!
//! ## Dict path
//!
//! Set `CHEWING_PATH` env var to point at the dictionary directory
//! (typically `data-provider/output/`).

use std::io::{self, BufRead, Write};

use qbopomofo_tip::controller::{Controller, InputSink};

struct StdoutSink;

impl StdoutSink {
    fn println(&self, s: &str) {
        let stdout = io::stdout();
        let mut h = stdout.lock();
        let _ = writeln!(h, "{}", s);
        let _ = h.flush();
    }
}

impl InputSink for StdoutSink {
    fn update_preedit(&self, text: &str) -> Option<(i32, i32)> {
        if text.is_empty() {
            self.println("PREEDIT: \"\"");
        } else {
            self.println(&format!("PREEDIT: {:?}", text));
        }
        None
    }

    fn commit_text(&self, text: &str) {
        self.println(&format!("COMMIT: {:?}", text));
    }

    fn end_preedit(&self) {
        self.println("END_PREEDIT");
    }

    fn show_candidates(
        &self,
        cands: &[String],
        selection_keys: &[char],
        highlight: usize,
        page_info: &str,
        _caret: Option<(i32, i32)>,
    ) {
        let mut parts: Vec<String> = Vec::with_capacity(cands.len());
        for (i, c) in cands.iter().enumerate() {
            let key = selection_keys.get(i).copied().unwrap_or(' ');
            let star = if i == highlight { "*" } else { "" };
            parts.push(format!("{}{}.{}", star, key, c));
        }
        let page_suffix = if page_info.is_empty() {
            String::new()
        } else {
            format!("    (page {})", page_info)
        };
        self.println(&format!("CAND: {}{}", parts.join(" "), page_suffix));
    }

    fn hide_candidates(&self) {
        self.println("HIDE");
    }
}

fn main() {
    let dict_path = std::env::var("CHEWING_PATH").ok();
    if dict_path.is_none() {
        eprintln!(
            "[warn] CHEWING_PATH not set; dictionaries may not load. \
            Set it to data-provider/output/"
        );
    }

    let mut controller = Controller::new();
    controller.activate(dict_path);
    let sink = StdoutSink;

    eprintln!("[info] dev_harness ready. Type commands on stdin (KEY, TYPE, #comment).");

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("TYPE ").or_else(|| line.strip_prefix("TYPE\t")) {
            for ch in rest.chars() {
                send_char(&mut controller, ch, &sink);
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("KEY ").or_else(|| line.strip_prefix("KEY\t")) {
            let mut parts = rest.split(|c: char| c == ',' || c.is_whitespace());
            let Some(vkey_token) = parts.next() else {
                eprintln!("[error] KEY missing vkey: {}", line);
                continue;
            };
            let vkey = match parse_u32(vkey_token) {
                Some(v) => v,
                None => {
                    eprintln!("[error] bad vkey {:?}", vkey_token);
                    continue;
                }
            };
            let mut shift = false;
            let mut ctrl = false;
            let mut caps = false;
            for flag in parts {
                match flag.trim().to_ascii_lowercase().as_str() {
                    "" => continue,
                    "shift" => shift = true,
                    "ctrl" => ctrl = true,
                    "caps" => caps = true,
                    other => eprintln!("[warn] unknown flag {:?}", other),
                }
            }
            let ch = vkey_to_char(vkey, shift);
            controller.on_key_down(vkey, ch, shift, ctrl, caps, &sink);
            continue;
        }

        eprintln!("[warn] unknown command: {}", line);
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn send_char(controller: &mut Controller, ch: char, sink: &StdoutSink) {
    let (vkey, shift) = char_to_vkey(ch);
    controller.on_key_down(vkey, ch, shift, false, false, sink);
}

/// Map a character to its Windows VK + shift flag. Rough US-layout mapping —
/// enough for the common bopomofo test sequences.
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

/// Inverse of `char_to_vkey` for use when KEY commands specify only vkey.
fn vkey_to_char(vkey: u32, shift: bool) -> char {
    match vkey {
        0x30..=0x39 if !shift => (b'0' + (vkey - 0x30) as u8) as char,
        0x41..=0x5A if shift => (b'A' + (vkey - 0x41) as u8) as char,
        0x41..=0x5A => (b'a' + (vkey - 0x41) as u8) as char,
        0x20 => ' ',
        0xBA => if shift { ':' } else { ';' },
        0xBB => if shift { '+' } else { '=' },
        0xBC => if shift { '<' } else { ',' },
        0xBD => if shift { '_' } else { '-' },
        0xBE => if shift { '>' } else { '.' },
        0xBF => if shift { '?' } else { '/' },
        0xC0 => if shift { '~' } else { '`' },
        0xDB => if shift { '{' } else { '[' },
        0xDC => if shift { '|' } else { '\\' },
        0xDD => if shift { '}' } else { ']' },
        0xDE => if shift { '"' } else { '\'' },
        0x31 if shift => '!',
        0x32 if shift => '@',
        0x33 if shift => '#',
        0x34 if shift => '$',
        0x35 if shift => '%',
        0x36 if shift => '^',
        0x37 if shift => '&',
        0x38 if shift => '*',
        0x39 if shift => '(',
        _ => '\0',
    }
}
