/// Test that the engine selects the correct character/phrase based on frequency.
/// Uses the QBopomofo dictionary data from data-provider/output/.
use std::error::Error;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::c_int;
use std::path::Path;
use std::ptr::null_mut;

use chewing_capi::candidates::{
    chewing_cand_open, chewing_cand_string_by_index_static, chewing_cand_TotalChoice,
};
use chewing_capi::input::chewing_handle_Default;
use chewing_capi::output::chewing_buffer_String;
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

/// Type a key sequence, open candidates, and return the first candidate.
unsafe fn first_candidate_for(keys: &str) -> Result<String, Box<dyn Error>> {
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

        assert_eq!(chewing_cand_open(ctx), 0, "Failed to open candidate list");
        assert!(
            chewing_cand_TotalChoice(ctx) > 0,
            "Expected at least one candidate"
        );

        let candidate = chewing_cand_string_by_index_static(ctx, 0);
        let result = CStr::from_ptr(candidate).to_str()?.to_string();

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
fn bu_tone4_candidate_prefers_not() -> Result<(), Box<dyn Error>> {
    // 1j4 = ㄅㄨˋ.  The common word 不 should not be buried behind 部/布/步.
    let candidate = unsafe { first_candidate_for("1j4")? };
    assert_eq!("不", candidate);
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

#[test]
fn jiu_yi_zhi_bei_not_jiu_yi_zhi_bei_medical() -> Result<(), Box<dyn Error>> {
    // ㄐㄧㄡˋ ㄧ ㄓˊ ㄅㄟˋ → should be 就一直被, NOT 就醫植被
    // Standard keyboard layout:
    //   r=ㄐ u=ㄧ .=ㄡ 4=ˋ → ㄐㄧㄡˋ (就)
    //   u=ㄧ (space to commit single medial) → ㄧ (一)
    //   5=ㄓ 6=ˊ → ㄓˊ (直)
    //   1=ㄅ o=ㄟ 4=ˋ → ㄅㄟˋ (被)
    let preedit = unsafe { type_keys_and_get_preedit("ru.4u 561o4")? };
    eprintln!("preedit for 就一直被: {}", preedit);
    assert!(
        !preedit.contains("就醫"),
        "Should NOT contain 就醫, got: {}",
        preedit
    );
    assert!(
        !preedit.contains("植被"),
        "Should NOT contain 植被, got: {}",
        preedit
    );
    assert!(
        preedit.contains("一直"),
        "Should contain 一直, got: {}",
        preedit
    );
    Ok(())
}

#[test]
fn shi_qing_shi_not_qing_shi() -> Result<(), Box<dyn Error>> {
    // ㄕˋ ㄑㄧㄥˊ ㄕˋ ㄅㄨˋ ㄋㄥˊ → should be 事情是不能, NOT 事情勢不能
    // Standard keyboard layout:
    //   g=ㄕ 4=ˋ → ㄕˋ (事)
    //   f=ㄑ u=ㄧ ;=ㄤ 6=ˊ → ㄑㄧㄥˊ ... wait, ㄥ=/
    //   f=ㄑ u=ㄧ /=ㄥ 6=ˊ → ㄑㄧㄥˊ (情)
    //   g=ㄕ 4=ˋ → ㄕˋ (是)
    //   1=ㄅ j=ㄨ 4=ˋ → ㄅㄨˋ (不)
    //   s=ㄋ /=ㄥ 6=ˊ → ㄋㄥˊ (能)
    let preedit = unsafe { type_keys_and_get_preedit("g4fu/6g41j4s/6")? };
    eprintln!("preedit for 事情是不能: {}", preedit);
    assert!(
        preedit.contains("事情"),
        "Should contain 事情, got: {}",
        preedit
    );
    assert!(
        !preedit.contains("情勢"),
        "Should NOT contain 情勢, got: {}",
        preedit
    );
    assert!(
        preedit.contains("是"),
        "Should contain 是, got: {}",
        preedit
    );
    Ok(())
}

#[test]
fn zhe_jian_not_zhe_jian_build() -> Result<(), Box<dyn Error>> {
    // ㄓㄜˋ ㄐㄧㄢˋ ㄕˋ → should be 這件事, NOT 這建事
    // Standard keyboard layout:
    //   5=ㄓ k=ㄜ 4=ˋ → ㄓㄜˋ (這)
    //   r=ㄐ u=ㄧ 0=ㄢ 4=ˋ → ㄐㄧㄢˋ (件)
    //   g=ㄕ 4=ˋ → ㄕˋ (事)
    let preedit = unsafe { type_keys_and_get_preedit("5k4ru04g4")? };
    eprintln!("preedit for 這件事: {}", preedit);
    assert!(
        preedit.contains("這件"),
        "Should contain 這件, got: {}",
        preedit
    );
    assert!(
        !preedit.contains("建"),
        "Should NOT contain 建, got: {}",
        preedit
    );
    Ok(())
}
