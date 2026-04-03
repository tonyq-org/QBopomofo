//! Diagnostic: check Q注音 TSF registration and try to activate it.

use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, ITfInputProcessorProfiles,
    CLSID_TF_InputProcessorProfiles, TF_INPUTPROCESSORPROFILE,
};
use windows::core::{Interface, GUID};

const CLSID_QBOPOMOFO: GUID = GUID::from_values(
    0xA7E3B4C1, 0x9F2D, 0x4E5A,
    [0xB8, 0xC6, 0x1D, 0x3F, 0x5A, 0x7E, 0x9B, 0x2C],
);
const GUID_PROFILE: GUID = GUID::from_values(
    0xB8D1E2F3, 0x6A4C, 0x5D7E,
    [0x9F, 0x0A, 0x2B, 0x4C, 0x6D, 0x8E, 0x0F, 0x1A],
);
const LANG_ID: u16 = 0x0404;

fn guid_str(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1, g.data2, g.data3,
        g.data4[0], g.data4[1], g.data4[2], g.data4[3],
        g.data4[4], g.data4[5], g.data4[6], g.data4[7],
    )
}

fn main() {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    let profiles: ITfInputProcessorProfiles = unsafe {
        CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
    }
    .expect("Failed to create ITfInputProcessorProfiles");

    println!("=== Q注音 Status ===");

    // Check enabled
    match unsafe { profiles.IsEnabledLanguageProfile(&CLSID_QBOPOMOFO, LANG_ID, &GUID_PROFILE) } {
        Ok(b) => println!("IsEnabled: {}", b.as_bool()),
        Err(e) => println!("IsEnabled error: {:?}", e),
    }

    // Get description
    match unsafe { profiles.GetLanguageProfileDescription(&CLSID_QBOPOMOFO, LANG_ID, &GUID_PROFILE) } {
        Ok(desc) => println!("Description: {:?}", desc),
        Err(e) => println!("Description error: {:?}", e),
    }

    // Enumerate ALL profiles for langid 0x0404 via ProfileMgr
    println!("\n=== All 0x0404 Profiles (via ProfileMgr) ===");
    let mgr: ITfInputProcessorProfileMgr = profiles.cast().expect("cast to ProfileMgr");

    match unsafe { mgr.EnumProfiles(LANG_ID) } {
        Ok(enumerator) => {
            loop {
                let mut profile = TF_INPUTPROCESSORPROFILE::default();
                let mut fetched = 0u32;
                let hr = unsafe { enumerator.Next(std::slice::from_mut(&mut profile), &mut fetched) };
                if hr.is_err() || fetched == 0 {
                    break;
                }
                let is_ours = profile.clsid == CLSID_QBOPOMOFO;
                println!(
                    "{}CLSID={} Profile={} type={} lang={:#06x} enabled={} hkl={:?}",
                    if is_ours { ">>> " } else { "    " },
                    guid_str(&profile.clsid),
                    guid_str(&profile.guidProfile),
                    profile.dwProfileType,
                    profile.langid,
                    profile.dwFlags,
                    profile.hkl,
                );
            }
        }
        Err(e) => println!("EnumProfiles error: {:?}", e),
    }

    // Try to activate
    println!("\n=== Activating Q注音 ===");
    match unsafe { profiles.ActivateLanguageProfile(&CLSID_QBOPOMOFO, LANG_ID, &GUID_PROFILE) } {
        Ok(()) => println!("ActivateLanguageProfile: OK"),
        Err(e) => println!("ActivateLanguageProfile error: {:?}", e),
    }

    // Check what's active now
    let mut active_lang = 0u16;
    let mut active_profile = GUID::zeroed();
    let _ = unsafe { profiles.GetActiveLanguageProfile(&CLSID_QBOPOMOFO, &mut active_lang, &mut active_profile) };
    println!("Active lang={:#06x} profile={}", active_lang, guid_str(&active_profile));

    println!("\nDone. Press Enter to exit...");
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).ok();
}
