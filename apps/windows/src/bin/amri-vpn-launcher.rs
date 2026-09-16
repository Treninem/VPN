#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = launch_elevated() {
        show_error(&error);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn launch_elevated() -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

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
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target_wide.as_ptr(),
            std::ptr::null(),
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
