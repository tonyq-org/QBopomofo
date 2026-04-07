/// Test that the engine selects the correct character based on frequency.
/// Uses the QBopomofo dictionary data from data-provider/output/.
use std::error::Error;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::c_int;
use std::path::Path;
use std::ptr::null_mut;

use chewing_capi::input::chewing_handle_Default;
use chewing_capi::input::chewing_handle_Enter;
use chewing_capi::output::chewing_buffer_String;
use chewing_capi::output::chewing_commit_Check;
use chewing_capi::output::chewing_commit_String;
use chewing_capi::setup::chewing_delete;
use chewing_capi::setup::chewing_new2;
use tempfile::tempdir;

fn qbopomofo_syspath() -> Result<CString, Box<dyn Error>> {
    // Use the QBopomofo built dictionary data
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../data-provider/output");
    if !path.join("word.dat").exists() {
        panic!(
            "Dictionary data not found at {:?}. Run `bash data-provider/build.sh` first.",
            path
        );
    }
    Ok(CString::new(path.display().to_string())?)
}

/// Type a key sequence and return the preedit buffer content.
unsafe fn type_keys_and_get_preedit(
    keys: &str,
) -> Result<String, Box<dyn Error>> {
    let syspath = qbopomofo_syspath()?;
    let tmpdir = tempdir()?;
    let userpath = CString::new(
        tmpdir.path().join("chewing.dat").display().to_string(),
    )?;

    unsafe {
        let ctx = chewing_new2(syspath.as_ptr(), userpath.as_ptr(), None, null_mut());
        assert!(!ctx.is_null(), "Failed to create chewing context");

        for ch in keys.bytes() {
            chewing_handle_Default(ctx, ch as c_int);
        }

        let preedit = chewing_buffer_String(ctx);
        let result = CStr::from_ptr(preedit).to_str()?.to_string();

        chewing_delete(ctx);
        Ok(result)
    }
}

/// Type a key sequence, press Enter, and return committed text.
unsafe fn type_keys_and_commit(
    keys: &str,
) -> Result<String, Box<dyn Error>> {
    let syspath = qbopomofo_syspath()?;
    let tmpdir = tempdir()?;
    let userpath = CString::new(
        tmpdir.path().join("chewing.dat").display().to_string(),
    )?;

    unsafe {
        let ctx = chewing_new2(syspath.as_ptr(), userpath.as_ptr(), None, null_mut());
        assert!(!ctx.is_null(), "Failed to create chewing context");

        for ch in keys.bytes() {
            chewing_handle_Default(ctx, ch as c_int);
        }

        chewing_handle_Enter(ctx);

        let result = if chewing_commit_Check(ctx) != 0 {
            let commit = chewing_commit_String(ctx);
            CStr::from_ptr(commit).to_str()?.to_string()
        } else {
            String::new()
        };

        chewing_delete(ctx);
        Ok(result)
    }
}

#[test]
fn yong_not_yong_servant() -> Result<(), Box<dyn Error>> {
    // hk4 = ㄘㄜˋ (測), g4 = ㄕˋ (試), m/4 = ㄩㄥˋ (用)
    // Should produce 測試用, NOT 測試佣
    let preedit = unsafe { type_keys_and_get_preedit("hk4g4m/4")? };
    eprintln!("preedit for hk4g4m/4: {}", preedit);
    assert!(
        preedit.contains("用"),
        "Expected 用 (freq=61256) not 佣 (freq=89), got: {}",
        preedit
    );
    assert!(
        !preedit.contains("佣"),
        "Should not contain 佣, got: {}",
        preedit
    );
    Ok(())
}

#[test]
fn common_single_chars_by_freq() -> Result<(), Box<dyn Error>> {
    // Test several common characters that should beat their homophones
    let cases = [
        ("su3", "你", "妳"),         // ㄋㄧˇ — 你 should win
        ("ji3", "我", ""),           // ㄨㄛˇ — 我
        ("m/4", "用", "佣"),         // ㄩㄥˋ — 用 should beat 佣
    ];
    for (keys, expected, not_expected) in cases {
        let preedit = unsafe { type_keys_and_get_preedit(keys)? };
        eprintln!("preedit for {}: {}", keys, preedit);
        assert!(
            preedit.contains(expected),
            "For keys '{}': expected '{}', got '{}'",
            keys, expected, preedit
        );
        if !not_expected.is_empty() {
            assert!(
                !preedit.contains(not_expected),
                "For keys '{}': should not contain '{}', got '{}'",
                keys, not_expected, preedit
            );
        }
    }
    Ok(())
}
