#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
const WM_TRAY_ICON: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 1;
#[cfg(target_os = "windows")]
const WM_OPEN_AMRI: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 2;
#[cfg(target_os = "windows")]
const TRAY_ICON_ID: u32 = 1;
#[cfg(target_os = "windows")]
const MENU_OPEN: u32 = 1001;
#[cfg(target_os = "windows")]
const MENU_EXIT_TRAY: u32 = 1002;

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = run_tray_companion() {
        show_error(&error);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn run_tray_companion() -> Result<(), String> {
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        ExtractIconExW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO,
        NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyIcon, DispatchMessageW, FindWindowW, GetMessageW, PostMessageW,
        RegisterClassW, TranslateMessage, MSG, WNDCLASSW,
    };

    let tray_only = std::env::var("AMRI_TRAY_ONLY").ok().as_deref() == Some("1");
    let class_name = wide("AMRI_VPN_TRAY_COMPANION_V1");
    let existing = unsafe { FindWindowW(class_name.as_ptr(), null()) };
    if !existing.is_null() {
        if !tray_only {
            unsafe {
                PostMessageW(existing, WM_OPEN_AMRI, 0, 0);
            }
        }
        return Ok(());
    }

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err("failed to obtain the AMRI launcher module handle".into());
    }

    let mut window_class: WNDCLASSW = unsafe { zeroed() };
    window_class.lpfnWndProc = Some(tray_window_proc);
    window_class.hInstance = instance;
    window_class.lpszClassName = class_name.as_ptr();
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err("failed to register the AMRI tray window class".into());
    }

    let hwnd: HWND = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
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
        return Err("failed to create the AMRI tray message window".into());
    }

    let launcher = std::env::current_exe()
        .map_err(|error| format!("failed to locate AMRI launcher: {error}"))?;
    let launcher_wide = wide_os(launcher.as_os_str());
    let mut tray_icon = null_mut();
    let extracted = unsafe {
        ExtractIconExW(
            launcher_wide.as_ptr(),
            0,
            null_mut(),
            &mut tray_icon,
            1,
        )
    };
    if extracted == 0 || tray_icon.is_null() {
        return Err("failed to load the AMRI application icon for the system tray".into());
    }

    let mut notify: NOTIFYICONDATAW = unsafe { zeroed() };
    notify.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    notify.hWnd = hwnd;
    notify.uID = TRAY_ICON_ID;
    notify.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_INFO;
    notify.uCallbackMessage = WM_TRAY_ICON;
    notify.hIcon = tray_icon;
    copy_wide(&mut notify.szTip, "AMRI VPN");
    copy_wide(&mut notify.szInfoTitle, "AMRI VPN");
    copy_wide(
        &mut notify.szInfo,
        "AMRI VPN is available from the system tray.",
    );
    notify.dwInfoFlags = NIIF_INFO;

    if unsafe { Shell_NotifyIconW(NIM_ADD, &notify) } == 0 {
        unsafe {
            DestroyIcon(tray_icon);
        }
        return Err("Windows refused to add the AMRI VPN tray icon".into());
    }

    if !tray_only {
        if let Err(error) = open_or_launch_main() {
            show_error(&error);
        }
    }

    let mut message: MSG = unsafe { zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result == 0 {
            break;
        }
        if result == -1 {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &notify);
                DestroyIcon(tray_icon);
            }
            return Err("Windows tray message loop failed".into());
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &notify);
        DestroyIcon(tray_icon);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn tray_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, PostQuitMessage, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP,
    };

    match message {
        WM_OPEN_AMRI => {
            if let Err(error) = open_or_launch_main() {
                show_error(&error);
            }
            0
        }
        WM_TRAY_ICON => {
            match lparam as u32 {
                WM_LBUTTONUP => {
                    if let Err(error) = open_or_launch_main() {
                        show_error(&error);
                    }
                }
                WM_RBUTTONUP => show_tray_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
unsafe fn show_tray_menu(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, GetCursorPos, SetForegroundWindow,
        TrackPopupMenu, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    };

    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let open_text = wide("Open AMRI VPN");
    let exit_text = wide("Exit tray");
    AppendMenuW(menu, MF_STRING, MENU_OPEN as usize, open_text.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(
        menu,
        MF_STRING,
        MENU_EXIT_TRAY as usize,
        exit_text.as_ptr(),
    );

    let mut point = POINT { x: 0, y: 0 };
    if GetCursorPos(&mut point) != 0 {
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            null(),
        ) as u32;
        match command {
            MENU_OPEN => {
                if let Err(error) = open_or_launch_main() {
                    show_error(&error);
                }
            }
            MENU_EXIT_TRAY => {
                DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
    DestroyMenu(menu);
}

#[cfg(target_os = "windows")]
fn open_or_launch_main() -> Result<(), String> {
    use std::ptr::null;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let title = wide("AMRI VPN");
    let existing = unsafe { FindWindowW(null(), title.as_ptr()) };
    if !existing.is_null() {
        unsafe {
            ShowWindow(existing, SW_RESTORE);
            SetForegroundWindow(existing);
        }
        return Ok(());
    }
    launch_elevated()
}

#[cfg(target_os = "windows")]
fn launch_elevated() -> Result<(), String> {
    use std::ffi::OsStr;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let launcher = std::env::current_exe()
        .map_err(|error| format!("failed to locate AMRI launcher: {error}"))?;
    let install_dir = launcher
        .parent()
        .ok_or_else(|| "failed to locate AMRI installation directory".to_string())?;
    let target = install_dir.join("AMRI-VPN.exe");
    if !target.is_file() {
        return Err("AMRI-VPN.exe is missing from the installation directory".into());
    }

    let operation = wide_os(OsStr::new("runas"));
    let target_wide = wide_os(target.as_os_str());
    let directory_wide = wide_os(install_dir.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            operation.as_ptr(),
            target_wide.as_ptr(),
            null(),
            directory_wide.as_ptr(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize <= 32 {
        return Err(format!(
            "Windows did not grant administrator access to AMRI VPN (ShellExecuteW code {})",
            result as isize
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn copy_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    let encoded = value.encode_utf16();
    for (slot, unit) in target
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(encoded)
    {
        *slot = unit;
    }
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn wide_os(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn show_error(message: &str) {
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let message = wide(message);
    let title = wide("AMRI VPN");
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
