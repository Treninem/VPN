#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
const MAIN_WINDOW_TITLE: [u16; 9] = [65, 77, 82, 73, 32, 86, 80, 78, 0];
#[cfg(target_os = "windows")]
const TRAY_CLASS_NAME: [u16; 19] = [65, 77, 82, 73, 95, 86, 80, 78, 95, 84, 82, 65, 89, 95, 72, 79, 83, 84, 0];
#[cfg(target_os = "windows")]
const TRAY_ICON_ID: u32 = 1;
#[cfg(target_os = "windows")]
const WM_TRAY_CALLBACK: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 1;
#[cfg(target_os = "windows")]
const WM_CHILD_EXITED: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 2;

#[cfg(target_os = "windows")]
fn main() {
    match launch_elevated() {
        Ok(Some(process)) => {
            if let Err(error) = run_tray(process) {
                unsafe {
                    windows_sys::Win32::Foundation::CloseHandle(process);
                }
                show_error(&error);
            }
        }
        Ok(None) => {}
        Err(error) => show_error(&error),
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn launch_elevated() -> Result<Option<windows_sys::Win32::Foundation::HANDLE>, String> {
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
    };

    if let Some(window) = existing_main_window() {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        return Ok(None);
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let launcher = std::env::current_exe()
        .map_err(|error| format!("failed to locate AMRI launcher: {error}"))?;
    let install_dir = launcher
        .parent()
        .ok_or_else(|| "failed to locate AMRI installation directory".to_string())?;
    let target = install_dir.join("AMRI-VPN.exe");
    if !target.is_file() {
        return Err("AMRI-VPN.exe is missing from the installation directory".into());
    }

    let operation = wide(OsStr::new("runas"));
    let target_wide = wide(target.as_os_str());
    let directory_wide = wide(install_dir.as_os_str());
    let mut execute: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    execute.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    execute.fMask = SEE_MASK_NOCLOSEPROCESS;
    execute.hwnd = null_mut();
    execute.lpVerb = operation.as_ptr();
    execute.lpFile = target_wide.as_ptr();
    execute.lpParameters = null();
    execute.lpDirectory = directory_wide.as_ptr();
    execute.nShow = SW_SHOWNORMAL as i32;

    let launched = unsafe { ShellExecuteExW(&mut execute) };
    if launched == 0 || execute.hProcess.is_null() {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "Windows did not start AMRI VPN with administrator access (error {error})"
        ));
    }

    let _ = FindWindowW;
    Ok(Some(execute.hProcess))
}

#[cfg(target_os = "windows")]
fn run_tray(process: windows_sys::Win32::Foundation::HANDLE) -> Result<(), String> {
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{
        ExtractIconExW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
        NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, LoadIconW, PostMessageW, RegisterClassW,
        TranslateMessage, WNDCLASSW, IDI_APPLICATION, MSG,
    };

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err("failed to initialize the AMRI tray host".into());
    }

    let mut window_class: WNDCLASSW = unsafe { std::mem::zeroed() };
    window_class.lpfnWndProc = Some(tray_window_proc);
    window_class.hInstance = instance;
    window_class.lpszClassName = TRAY_CLASS_NAME.as_ptr();
    if unsafe { RegisterClassW(&window_class) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!("failed to register AMRI tray window (error {error})"));
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            TRAY_CLASS_NAME.as_ptr(),
            MAIN_WINDOW_TITLE.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if hwnd.is_null() {
        let error = unsafe { GetLastError() };
        return Err(format!("failed to create AMRI tray host (error {error})"));
    }

    let mut large_icon = null_mut();
    let mut small_icon = null_mut();
    let launcher_path = std::env::current_exe()
        .map_err(|error| format!("failed to locate AMRI launcher icon: {error}"))?;
    let launcher_wide = wide_os(launcher_path.as_os_str());
    unsafe {
        ExtractIconExW(
            launcher_wide.as_ptr(),
            0,
            &mut large_icon,
            &mut small_icon,
            1,
        );
    }
    let icon = if !small_icon.is_null() {
        small_icon
    } else if !large_icon.is_null() {
        large_icon
    } else {
        unsafe { LoadIconW(null_mut(), IDI_APPLICATION) }
    };

    let mut tray: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray.hWnd = hwnd;
    tray.uID = TRAY_ICON_ID;
    tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    tray.uCallbackMessage = WM_TRAY_CALLBACK;
    tray.hIcon = icon;
    write_fixed_wide(&mut tray.szTip, "AMRI VPN");
    if unsafe { Shell_NotifyIconW(NIM_ADD, &tray) } == 0 {
        let error = unsafe { GetLastError() };
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
        }
        return Err(format!("failed to add AMRI tray icon (error {error})"));
    }
    show_started_notification(hwnd);

    let child_handle = process as usize;
    let tray_window = hwnd as usize;
    std::thread::spawn(move || unsafe {
        let child = child_handle as windows_sys::Win32::Foundation::HANDLE;
        WaitForSingleObject(child, INFINITE);
        windows_sys::Win32::Foundation::CloseHandle(child);
        let target = tray_window as windows_sys::Win32::Foundation::HWND;
        PostMessageW(target, WM_CHILD_EXITED, 0, 0);
    });

    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn tray_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, DestroyWindow, PostQuitMessage, WM_DESTROY, WM_LBUTTONDBLCLK,
        WM_LBUTTONUP, WM_RBUTTONUP,
    };

    match message {
        WM_TRAY_CALLBACK => {
            let event = lparam as u32;
            if matches!(event, WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONUP) {
                focus_main_window();
            }
            0
        }
        WM_CHILD_EXITED => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
fn existing_main_window() -> Option<windows_sys::Win32::Foundation::HWND> {
    use std::ptr::null;
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

    let window = unsafe { FindWindowW(null(), MAIN_WINDOW_TITLE.as_ptr()) };
    (!window.is_null()).then_some(window)
}

#[cfg(target_os = "windows")]
fn focus_main_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    if let Some(window) = existing_main_window() {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
    }
}

#[cfg(target_os = "windows")]
fn show_started_notification(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_INFO, NIIF_INFO, NIM_MODIFY, NOTIFYICONDATAW,
    };

    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ICON_ID;
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    write_fixed_wide(&mut data.szInfoTitle, "AMRI VPN");
    write_fixed_wide(&mut data.szInfo, "AMRI VPN started and is available from the notification area.");
    unsafe {
        Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

#[cfg(target_os = "windows")]
fn remove_tray_icon(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIM_DELETE, NOTIFYICONDATAW};

    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ICON_ID;
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

#[cfg(target_os = "windows")]
fn write_fixed_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let encoded = OsStr::new(value)
        .encode_wide()
        .take(N.saturating_sub(1))
        .collect::<Vec<_>>();
    target[..encoded.len()].copy_from_slice(&encoded);
    if encoded.len() < N {
        target[encoded.len()] = 0;
    }
}

#[cfg(target_os = "windows")]
fn wide_os(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn show_error(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let wide = |value: &OsStr| {
        value
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let message = wide(OsStr::new(message));
    let title = wide(OsStr::new("AMRI VPN"));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
