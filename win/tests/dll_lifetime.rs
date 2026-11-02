//! Exercise the actual COM exports without registering or activating the IME.
use std::sync::Mutex;

use qbopomofo_tip::candidate_window::CandidateWindow;
use qbopomofo_tip::com::{CLSID_QBOPOMOFO, dll_instance};
use windows::Win32::Foundation::{LPARAM, S_FALSE, S_OK, WPARAM};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::TextServices::ITfFunctionProvider;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetClassInfoExW, SendMessageW, WINDOW_EX_STYLE, WM_ACTIVATEAPP,
    WNDCLASSEXW, WS_POPUP,
};
use windows::core::{GUID, HRESULT, IUnknown, Interface, w};

unsafe extern "system" {
    fn DllGetClassObject(
        clsid: *const GUID,
        iid: *const GUID,
        object: *mut *mut std::ffi::c_void,
    ) -> HRESULT;
    fn DllCanUnloadNow() -> HRESULT;
}

// The exports describe process-wide state, so these scenarios must not overlap.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn factory() -> IClassFactory {
    unsafe {
        let mut object = std::ptr::null_mut();
        DllGetClassObject(&CLSID_QBOPOMOFO, &IClassFactory::IID, &mut object)
            .ok()
            .unwrap();
        IClassFactory::from_raw(object)
    }
}

#[test]
fn factory_prevents_unload_until_its_last_reference_is_released() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
    let first = factory();
    let retained = first.clone();
    let while_alive = unsafe { DllCanUnloadNow() };
    drop(first);
    let while_retained = unsafe { DllCanUnloadNow() };
    drop(retained);
    assert_eq!(while_alive, S_FALSE);
    assert_eq!(while_retained, S_FALSE);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
}

#[test]
fn text_service_outlives_factory_and_keeps_dll_loaded() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let factory = factory();
    let service: IUnknown = unsafe { factory.CreateInstance(None) }.unwrap();
    let retained: ITfFunctionProvider = service.cast().unwrap();
    drop(factory);
    drop(service);
    let while_retained = unsafe { DllCanUnloadNow() };
    assert!(unsafe { retained.GetDescription() }.is_ok());
    drop(retained);
    assert_eq!(while_retained, S_FALSE);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
}

#[test]
fn explicit_server_lock_survives_factory_release() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let first = factory();
    unsafe { first.LockServer(true) }.unwrap();
    drop(first);
    let while_locked = unsafe { DllCanUnloadNow() };
    let second = factory();
    unsafe { second.LockServer(false) }.unwrap();
    let while_factory_alive = unsafe { DllCanUnloadNow() };
    drop(second);
    assert_eq!(while_locked, S_FALSE);
    assert_eq!(while_factory_alive, S_FALSE);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
}

#[test]
fn unbalanced_unlock_cannot_cancel_a_live_objects_protection() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let factory = factory();
    let invalid_unlock = unsafe { factory.LockServer(false) };
    let while_alive = unsafe { DllCanUnloadNow() };
    drop(factory);
    assert!(invalid_unlock.is_err());
    assert_eq!(while_alive, S_FALSE);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
}

fn candidate_class_exists() -> bool {
    let mut class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        ..Default::default()
    };
    // In this rlib test host, DllMain does not run. USER32 associates a
    // registration with a null hInstance with the executable's module.
    let instance = if dll_instance().is_invalid() {
        unsafe { GetModuleHandleW(None) }.unwrap()
    } else {
        dll_instance()
    };
    unsafe {
        GetClassInfoExW(
            Some(instance.into()),
            w!("QBopomofo_CandidateWindow"),
            &mut class,
        )
        .is_ok()
    }
}

#[test]
fn candidate_windows_prevent_unload_and_class_can_be_recreated() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for _ in 0..3 {
        let first = CandidateWindow::try_new().unwrap();
        let second = CandidateWindow::try_new().unwrap();
        assert!(candidate_class_exists());
        assert_eq!(unsafe { DllCanUnloadNow() }, S_FALSE);
        drop(first);
        assert_eq!(unsafe { DllCanUnloadNow() }, S_FALSE);
        drop(second);
        assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
        assert!(
            !candidate_class_exists(),
            "WndProc must not survive DLL unload"
        );
    }
}

#[test]
fn surviving_native_window_blocks_unload_even_without_a_rust_owner() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let owner = CandidateWindow::try_new().unwrap();
    // Model a native HWND surviving its Rust/TSF owner. USER32 must keep the
    // DLL loaded until this last callback target has actually been destroyed.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("QBopomofo_CandidateWindow"),
            w!(""),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(dll_instance().into()),
            None,
        )
    }
    .unwrap();
    drop(owner);
    let while_native_window_alive = unsafe { DllCanUnloadNow() };
    unsafe {
        // The Explorer dump faulted on this message (0x1c) during Alt+Tab.
        SendMessageW(hwnd, WM_ACTIVATEAPP, Some(WPARAM(1)), Some(LPARAM(0)));
        DestroyWindow(hwnd).unwrap();
    }
    assert_eq!(while_native_window_alive, S_FALSE);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
    assert!(!candidate_class_exists());
}

#[test]
fn windows_on_another_ui_thread_also_prevent_unload() {
    let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let main_window = CandidateWindow::try_new().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let window = CandidateWindow::try_new().unwrap();
        ready_tx.send(()).unwrap();
        // A timeout prevents a failed test from leaving a blocked UI thread.
        let _ = finish_rx.recv_timeout(std::time::Duration::from_secs(5));
        drop(window);
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    drop(main_window);
    let while_worker_window_alive = unsafe { DllCanUnloadNow() };
    finish_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(while_worker_window_alive, S_FALSE);
    assert_eq!(unsafe { DllCanUnloadNow() }, S_OK);
    assert!(!candidate_class_exists());
}
