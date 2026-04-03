//! COM DLL entry points for Windows TSF registration.
//!
//! A TSF input method is a COM DLL that exports:
//! - DllGetClassObject: Returns the class factory
//! - DllCanUnloadNow: Whether the DLL can be unloaded
//! - DllRegisterServer / DllUnregisterServer: System registration

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{implement, Interface, GUID, HRESULT, IUnknown, Ref, BOOL, PCWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_NOINTERFACE, HMODULE, S_FALSE, S_OK, WIN32_ERROR,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CLASSES_ROOT,
    KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfCategoryMgr, ITfInputProcessorProfileMgr, ITfInputProcessorProfiles,
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles,
    GUID_TFCAT_TIP_KEYBOARD,
};

use crate::text_service::QBopomofoTextService;

/// CLSID for QBopomofo text service COM class.
pub const CLSID_QBOPOMOFO: GUID = GUID::from_values(
    0xA7E3B4C1,
    0x9F2D,
    0x4E5A,
    [0xB8, 0xC6, 0x1D, 0x3F, 0x5A, 0x7E, 0x9B, 0x2C],
);

/// GUID for the language profile.
pub const GUID_PROFILE: GUID = GUID::from_values(
    0xB8D1E2F3,
    0x6A4C,
    0x5D7E,
    [0x9F, 0x0A, 0x2B, 0x4C, 0x6D, 0x8E, 0x0F, 0x1A],
);

pub const DISPLAY_NAME: &str = "Q注音輸入法";
pub const LANG_ID: u16 = 0x0404;

pub(crate) static DLL_REF_COUNT: AtomicU32 = AtomicU32::new(0);

static mut DLL_INSTANCE: HMODULE = HMODULE(std::ptr::null_mut());

pub fn dll_instance() -> HMODULE {
    unsafe { DLL_INSTANCE }
}

fn win32_ok(err: WIN32_ERROR) -> windows::core::Result<()> {
    if err.0 == 0 {
        Ok(())
    } else {
        Err(windows::core::Error::from(HRESULT::from_win32(err.0)))
    }
}

// ---------------------------------------------------------------------------
// DLL entry point
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe extern "system" fn DllMain(
    hinst: HMODULE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> BOOL {
    if reason == 1 {
        unsafe { DLL_INSTANCE = hinst };
        // Install panic hook to write crashes to log file
        std::panic::set_hook(Box::new(|info| {
            let path = std::env::temp_dir().join("qbopomofo_crash.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write;
                let _ = writeln!(f, "PANIC: {}", info);
            }
        }));
    }
    BOOL(1)
}

// ---------------------------------------------------------------------------
// COM DLL exports
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    let rclsid = unsafe { &*rclsid };
    if ppv.is_null() {
        return E_NOINTERFACE;
    }
    unsafe { *ppv = std::ptr::null_mut() };
    if *rclsid != CLSID_QBOPOMOFO {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = QBopomofoClassFactory.into();
    unsafe { factory.query(riid, ppv) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if DLL_REF_COUNT.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllRegisterServer() -> HRESULT {
    match register_server() {
        Ok(()) => S_OK,
        Err(e) => e.into(),
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
    match unregister_server() {
        Ok(()) => S_OK,
        Err(e) => e.into(),
    }
}

// ---------------------------------------------------------------------------
// Class Factory
// ---------------------------------------------------------------------------

#[implement(IClassFactory)]
struct QBopomofoClassFactory;

impl IClassFactory_Impl for QBopomofoClassFactory_Impl {
    fn CreateInstance(
        &self,
        _punkouter: Ref<IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut std::ffi::c_void,
    ) -> windows::core::Result<()> {
        unsafe { *ppvobject = std::ptr::null_mut() };
        let service = QBopomofoTextService::new();
        let unknown: IUnknown = service.into();
        unsafe { unknown.query(riid, ppvobject).ok() }
    }

    fn LockServer(&self, flock: BOOL) -> windows::core::Result<()> {
        if flock.as_bool() {
            DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Registration helpers
// ---------------------------------------------------------------------------

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn get_dll_path() -> windows::core::Result<String> {
    let mut buf = [0u16; 260];
    let len = unsafe { GetModuleFileNameW(Some(dll_instance()), &mut buf) } as usize;
    if len == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(String::from_utf16_lossy(&buf[..len]))
}

/// Get the directory containing this DLL (for locating dictionary files).
pub fn dll_dir() -> Option<String> {
    let path = get_dll_path().ok()?;
    let p = std::path::Path::new(&path);
    p.parent().map(|d| d.to_string_lossy().into_owned())
}

fn clsid_string() -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        CLSID_QBOPOMOFO.data1, CLSID_QBOPOMOFO.data2, CLSID_QBOPOMOFO.data3,
        CLSID_QBOPOMOFO.data4[0], CLSID_QBOPOMOFO.data4[1],
        CLSID_QBOPOMOFO.data4[2], CLSID_QBOPOMOFO.data4[3],
        CLSID_QBOPOMOFO.data4[4], CLSID_QBOPOMOFO.data4[5],
        CLSID_QBOPOMOFO.data4[6], CLSID_QBOPOMOFO.data4[7],
    )
}

fn register_server() -> windows::core::Result<()> {
    let dll_path = get_dll_path()?;
    register_com_server(&dll_path)?;
    register_tsf_category()?;
    register_tsf_profile()?;
    Ok(())
}

fn unregister_server() -> windows::core::Result<()> {
    let _ = unregister_tsf_category();
    let _ = unregister_tsf_profile();
    let _ = unregister_com_server();
    Ok(())
}

fn register_com_server(dll_path: &str) -> windows::core::Result<()> {
    let key_path = to_wide_null(&format!("CLSID\\{}\\InprocServer32", clsid_string()));
    let mut hkey = HKEY::default();

    win32_ok(unsafe {
        RegCreateKeyExW(
            HKEY_CLASSES_ROOT,
            PCWSTR(key_path.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
    })?;

    // Default value = DLL path
    let dll_path_w = to_wide_null(dll_path);
    win32_ok(unsafe {
        RegSetValueExW(
            hkey,
            None,
            Some(0),
            REG_SZ,
            Some(std::slice::from_raw_parts(
                dll_path_w.as_ptr() as *const u8,
                dll_path_w.len() * 2,
            )),
        )
    })?;

    // ThreadingModel = Apartment
    let name = to_wide_null("ThreadingModel");
    let value = to_wide_null("Apartment");
    win32_ok(unsafe {
        RegSetValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            Some(0),
            REG_SZ,
            Some(std::slice::from_raw_parts(
                value.as_ptr() as *const u8,
                value.len() * 2,
            )),
        )
    })?;

    unsafe { let _ = RegCloseKey(hkey); }
    Ok(())
}

fn unregister_com_server() -> windows::core::Result<()> {
    let key_path = to_wide_null(&format!("CLSID\\{}", clsid_string()));
    unsafe { let _ = RegDeleteTreeW(HKEY_CLASSES_ROOT, PCWSTR(key_path.as_ptr())); }
    Ok(())
}

fn register_tsf_category() -> windows::core::Result<()> {
    let cat_mgr: ITfCategoryMgr = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CLSID_TF_CategoryMgr,
            None,
            CLSCTX_INPROC_SERVER,
        )?
    };
    unsafe {
        cat_mgr.RegisterCategory(&CLSID_QBOPOMOFO, &GUID_TFCAT_TIP_KEYBOARD, &CLSID_QBOPOMOFO)?;
    }
    Ok(())
}

fn unregister_tsf_category() -> windows::core::Result<()> {
    let cat_mgr: ITfCategoryMgr = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CLSID_TF_CategoryMgr,
            None,
            CLSCTX_INPROC_SERVER,
        )?
    };
    unsafe {
        cat_mgr.UnregisterCategory(&CLSID_QBOPOMOFO, &GUID_TFCAT_TIP_KEYBOARD, &CLSID_QBOPOMOFO)?;
    }
    Ok(())
}

fn register_tsf_profile() -> windows::core::Result<()> {
    let display_name_w: Vec<u16> = DISPLAY_NAME.encode_utf16().collect();
    let empty: Vec<u16> = Vec::new();

    // Step 1: Register CLSID + AddLanguageProfile via legacy API
    // (required for Windows to recognize the TIP)
    let profiles: ITfInputProcessorProfiles = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        )?
    };
    unsafe {
        profiles.Register(&CLSID_QBOPOMOFO)?;
        profiles.AddLanguageProfile(
            &CLSID_QBOPOMOFO,
            LANG_ID,
            &GUID_PROFILE,
            &display_name_w,
            &empty,
            0,
        )?;
    }

    // Step 2: Also register via the newer ProfileMgr API for Windows 8+
    let profile_mgr: ITfInputProcessorProfileMgr = profiles.cast()?;
    unsafe {
        profile_mgr.RegisterProfile(
            &CLSID_QBOPOMOFO,
            LANG_ID,
            &GUID_PROFILE,
            &display_name_w,
            &empty,
            0,
            HKL::default(),
            0,
            true,
            0,
        )?;
    }
    Ok(())
}

fn unregister_tsf_profile() -> windows::core::Result<()> {
    let profiles: ITfInputProcessorProfiles = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        )?
    };
    unsafe {
        let _ = profiles.RemoveLanguageProfile(&CLSID_QBOPOMOFO, LANG_ID, &GUID_PROFILE);
        let _ = profiles.Unregister(&CLSID_QBOPOMOFO);
    }
    // Also unregister via Mgr API
    if let Ok(profile_mgr) = profiles.cast::<ITfInputProcessorProfileMgr>() {
        unsafe { let _ = profile_mgr.UnregisterProfile(&CLSID_QBOPOMOFO, LANG_ID, &GUID_PROFILE, 0); }
    }
    Ok(())
}
