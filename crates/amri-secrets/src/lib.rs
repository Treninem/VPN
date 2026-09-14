use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::Zeroizing;

/// Secret bytes that are zeroed when dropped and never exposed through Debug.
pub struct SecretValue(Zeroizing<Vec<u8>>);

impl SecretValue {
    pub fn from_bytes(value: impl Into<Vec<u8>>) -> Result<Self, SecretStoreError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SecretStoreError::EmptySecret);
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub fn from_text(value: impl AsRef<str>) -> Result<Self, SecretStoreError> {
        Self::from_bytes(value.as_ref().as_bytes().to_vec())
    }

    pub fn expose_secret(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn expose_text(&self) -> Result<&str, SecretStoreError> {
        std::str::from_utf8(self.expose_secret()).map_err(|_| SecretStoreError::InvalidUtf8)
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecretStoreError {
    #[error("secret key is invalid")]
    InvalidKey,
    #[error("secret value is empty")]
    EmptySecret,
    #[error("secret value is not valid UTF-8")]
    InvalidUtf8,
    #[error("secret is too large")]
    SecretTooLarge,
    #[error("secure secret storage is not available on this platform")]
    UnsupportedPlatform,
    #[error("secure secret storage operation failed")]
    Backend,
}

/// Secure persistence boundary for credential-bearing values.
pub trait SecretStore: Send + Sync {
    fn put(&self, key: &str, value: &SecretValue) -> Result<(), SecretStoreError>;
    fn get(&self, key: &str) -> Result<Option<SecretValue>, SecretStoreError>;
    fn delete(&self, key: &str) -> Result<bool, SecretStoreError>;
}

/// Windows secret store backed by DPAPI-protected files.
///
/// Ciphertext may be stored on disk, but plaintext is protected by Windows for the current user.
/// The logical key is hashed before it is used as a filename, so subscription IDs and labels are
/// not disclosed by the directory listing.
pub struct WindowsDpapiSecretStore {
    root: PathBuf,
}

impl WindowsDpapiSecretStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for_key(&self, key: &str) -> Result<PathBuf, SecretStoreError> {
        validate_key(key)?;
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let filename = format!("{}.bin", hex::encode(hasher.finalize()));
        Ok(self.root.join(filename))
    }
}

impl SecretStore for WindowsDpapiSecretStore {
    fn put(&self, key: &str, value: &SecretValue) -> Result<(), SecretStoreError> {
        let path = self.path_for_key(key)?;

        #[cfg(windows)]
        {
            std::fs::create_dir_all(&self.root).map_err(|_| SecretStoreError::Backend)?;
            let encrypted = windows_dpapi::protect(value.expose_secret())?;
            std::fs::write(path, encrypted).map_err(|_| SecretStoreError::Backend)
        }

        #[cfg(not(windows))]
        {
            let _ = (path, value);
            Err(SecretStoreError::UnsupportedPlatform)
        }
    }

    fn get(&self, key: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        let path = self.path_for_key(key)?;
        if !path.exists() {
            return Ok(None);
        }

        #[cfg(windows)]
        {
            let encrypted = std::fs::read(path).map_err(|_| SecretStoreError::Backend)?;
            let plaintext = windows_dpapi::unprotect(&encrypted)?;
            SecretValue::from_bytes(plaintext).map(Some)
        }

        #[cfg(not(windows))]
        {
            let _ = path;
            Err(SecretStoreError::UnsupportedPlatform)
        }
    }

    fn delete(&self, key: &str) -> Result<bool, SecretStoreError> {
        let path = self.path_for_key(key)?;
        if !path.exists() {
            return Ok(false);
        }

        #[cfg(windows)]
        {
            std::fs::remove_file(path).map_err(|_| SecretStoreError::Backend)?;
            Ok(true)
        }

        #[cfg(not(windows))]
        {
            let _ = path;
            Err(SecretStoreError::UnsupportedPlatform)
        }
    }
}

fn validate_key(key: &str) -> Result<(), SecretStoreError> {
    if key.is_empty()
        || key.len() > 256
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(SecretStoreError::InvalidKey);
    }
    Ok(())
}

#[cfg(windows)]
mod windows_dpapi {
    use super::SecretStoreError;
    use std::ffi::c_void;
    use std::ptr;
    use std::slice;

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(
            data_in: *const DataBlob,
            data_description: *const u16,
            optional_entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt_struct: *mut c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            data_in: *const DataBlob,
            data_description: *mut *mut u16,
            optional_entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt_struct: *mut c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "Kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub fn protect(input: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
        crypt(input, true)
    }

    pub fn unprotect(input: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
        crypt(input, false)
    }

    fn crypt(input: &[u8], protect: bool) -> Result<Vec<u8>, SecretStoreError> {
        if input.is_empty() {
            return Err(SecretStoreError::EmptySecret);
        }
        let input_len = u32::try_from(input.len()).map_err(|_| SecretStoreError::SecretTooLarge)?;
        let input_blob = DataBlob {
            cb_data: input_len,
            pb_data: input.as_ptr() as *mut u8,
        };
        let mut output_blob = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        let result = unsafe {
            if protect {
                CryptProtectData(
                    &input_blob,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output_blob,
                )
            } else {
                CryptUnprotectData(
                    &input_blob,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output_blob,
                )
            }
        };

        if result == 0 || output_blob.pb_data.is_null() {
            return Err(SecretStoreError::Backend);
        }

        let output = unsafe {
            slice::from_raw_parts(output_blob.pb_data, output_blob.cb_data as usize).to_vec()
        };
        unsafe {
            LocalFree(output_blob.pb_data.cast());
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let value = SecretValue::from_text("vpn://super-secret-token").unwrap();
        let debug = format!("{value:?}");
        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn invalid_keys_are_rejected_before_storage() {
        let store = WindowsDpapiSecretStore::new(std::env::temp_dir());
        let value = SecretValue::from_text("secret").unwrap();
        assert_eq!(
            store.put("../escape", &value).unwrap_err(),
            SecretStoreError::InvalidKey
        );
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_store_round_trips_secret_without_plaintext_file() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("amri-dpapi-test-{}-{nonce}", std::process::id()));
        let store = WindowsDpapiSecretStore::new(&root);
        let secret = SecretValue::from_text("https://subscription.example/token-123").unwrap();

        store.put("subscription:primary", &secret).unwrap();
        let loaded = store.get("subscription:primary").unwrap().unwrap();
        assert_eq!(loaded.expose_text().unwrap(), secret.expose_text().unwrap());

        let stored_bytes =
            std::fs::read(store.path_for_key("subscription:primary").unwrap()).unwrap();
        assert!(!stored_bytes
            .windows(b"token-123".len())
            .any(|window| window == b"token-123"));

        assert!(store.delete("subscription:primary").unwrap());
        assert!(store.get("subscription:primary").unwrap().is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
