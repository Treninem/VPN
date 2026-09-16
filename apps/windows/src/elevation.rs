use std::ffi::OsStr;
use std::mem::{size_of, zeroed};

pub(crate) const CONNECT_INDEX_ARG: &str = "--amri-connect-index=";

pub(crate) fn requested_connect_index() -> Option<usize> {
    std::env::args().find_map(|argument| {
        argument
            .strip_prefix(CONNECT_INDEX_ARG)
            .and_then(|value| value.parse::<usize>().ok())
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn is_process_elevated() -> Result<bool, String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!(
                "failed to inspect Windows process privileges: {}",
                std::io::Error::last_os_error()
            ));
        }

        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let error = if ok == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        let _ = CloseHandle(token);

        match error {
            Some(error) => Err(format!("failed to inspect Windows elevation: {error}")),
            None => Ok(elevation.TokenIsElevated != 0),
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn is_process_elevated() -> Result<bool, String> {
    Ok(true)
}

#[cfg(target_os = "windows")]
pub(crate) fn restart_elevated_for_connect(selected_index: usize) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to locate AMRI executable for elevation: {error}"))?;
    let operation = wide(OsStr::new("runas"));
    let executable_wide = wide(executable.as_os_str());
    let parameters = wide(OsStr::new(&format!(
        "{CONNECT_INDEX_ARG}{selected_index}"
    )));

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            executable_wide.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize <= 32 {
        return Err(format!(
            "Windows administrator elevation was not granted (ShellExecuteW code {})",
            result as isize
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn restart_elevated_for_connect(_selected_index: usize) -> Result<(), String> {
    Err("Windows elevation is unavailable on this platform".into())
}

#[cfg(target_os = "windows")]
fn wide(value: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_index_argument_contains_no_credentials() {
        let argument = format!("{CONNECT_INDEX_ARG}7");
        assert_eq!(argument, "--amri-connect-index=7");
        assert!(!argument.contains("://"));
        assert!(!argument.contains('@'));
    }
}
