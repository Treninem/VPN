use amri_secrets::SecretValue;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JByteArray, JClass, JObject};
use jni::sys::jint;
use jni::{jni_sig, jni_str, Env, EnvUnowned, JValue};

const ROUTE_PROOF_KEY_SLOT: &str = "route-proof:key";
const ROUTE_PROOF_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RouteProofKeyStatus {
    Existing = 0,
    Created = 1,
    InvalidStoredKey = 2,
    EntropyFailure = 3,
}

impl RouteProofKeyStatus {
    pub const fn code(self) -> jint {
        self as jint
    }
}

enum SlotValue {
    Missing,
    Present(SecretValue),
    Invalid,
}

trait SecretSlotBackend {
    type Error;

    fn get(&mut self, key: &str) -> Result<SlotValue, Self::Error>;
    fn put(&mut self, key: &str, value: &SecretValue) -> Result<(), Self::Error>;
}

fn ensure_route_proof_key<B: SecretSlotBackend>(
    backend: &mut B,
) -> Result<RouteProofKeyStatus, B::Error> {
    match backend.get(ROUTE_PROOF_KEY_SLOT)? {
        SlotValue::Present(value) if value.expose_secret().len() == ROUTE_PROOF_KEY_BYTES => {
            Ok(RouteProofKeyStatus::Existing)
        }
        SlotValue::Present(_) | SlotValue::Invalid => Ok(RouteProofKeyStatus::InvalidStoredKey),
        SlotValue::Missing => {
            let mut bytes = vec![0_u8; ROUTE_PROOF_KEY_BYTES];
            if getrandom::fill(&mut bytes).is_err() {
                bytes.fill(0);
                return Ok(RouteProofKeyStatus::EntropyFailure);
            }

            let generated = match SecretValue::from_bytes(bytes) {
                Ok(value) => value,
                Err(_) => return Ok(RouteProofKeyStatus::EntropyFailure),
            };
            backend.put(ROUTE_PROOF_KEY_SLOT, &generated)?;
            Ok(RouteProofKeyStatus::Created)
        }
    }
}

struct JniSecretSlot<'env, 'local> {
    env: &'env mut Env<'local>,
    store: &'env JObject<'local>,
}

impl SecretSlotBackend for JniSecretSlot<'_, '_> {
    type Error = jni::errors::Error;

    fn get(&mut self, key: &str) -> Result<SlotValue, Self::Error> {
        let key = self.env.new_string(key)?;
        let value = self
            .env
            .call_method(
                self.store,
                jni_str!("get"),
                jni_sig!("(Ljava/lang/String;)[B"),
                &[JValue::Object(key.as_ref())],
            )?
            .into_object()?;

        if value.is_null() {
            return Ok(SlotValue::Missing);
        }

        let array = self.env.cast_local::<JByteArray>(value)?;
        let bytes = self.env.convert_byte_array(&array)?;

        // AndroidKeystoreSecretStore#get returns a fresh plaintext ByteArray. Clear that Java-side
        // copy as soon as Rust has moved the bytes into SecretValue's zeroizing allocation.
        let zeros = vec![0_i8; bytes.len()];
        array.set_region(self.env, 0, &zeros)?;

        if bytes.is_empty() {
            return Ok(SlotValue::Invalid);
        }

        match SecretValue::from_bytes(bytes) {
            Ok(value) => Ok(SlotValue::Present(value)),
            Err(_) => Ok(SlotValue::Invalid),
        }
    }

    fn put(&mut self, key: &str, value: &SecretValue) -> Result<(), Self::Error> {
        let key = self.env.new_string(key)?;
        let bytes = self.env.byte_array_from_slice(value.expose_secret())?;

        self.env
            .call_method(
                self.store,
                jni_str!("put"),
                jni_sig!("(Ljava/lang/String;[B)V"),
                &[
                    JValue::Object(key.as_ref()),
                    JValue::Object(bytes.as_ref()),
                ],
            )?
            .into_void()?;

        // Kotlin encrypts synchronously inside put(); wipe the temporary JNI byte[] afterwards.
        let zeros = vec![0_i8; value.expose_secret().len()];
        bytes.set_region(self.env, 0, &zeros)?;
        Ok(())
    }
}

/// Initializes the installation-local Route Proof key through the Android Keystore adapter.
///
/// This JNI boundary deliberately accepts only the already-constructed
/// `AndroidKeystoreSecretStore`. It neither enumerates nor serializes the credential store and it
/// never returns the Route Proof key to Kotlin.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ru_amri_vpn_nativebridge_AmriNativeBridge_nativeEnsureRouteProofKey<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    store: JObject<'caller>,
) -> jint {
    unowned_env
        .with_env(|env| -> JniResult<jint> {
            let mut backend = JniSecretSlot {
                env,
                store: &store,
            };
            ensure_route_proof_key(&mut backend).map(RouteProofKeyStatus::code)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    #[derive(Default)]
    struct MemorySlot {
        value: Option<Vec<u8>>,
        put_count: usize,
    }

    impl SecretSlotBackend for MemorySlot {
        type Error = Infallible;

        fn get(&mut self, key: &str) -> Result<SlotValue, Self::Error> {
            assert_eq!(key, ROUTE_PROOF_KEY_SLOT);
            Ok(match self.value.clone() {
                Some(value) if value.is_empty() => SlotValue::Invalid,
                Some(value) => SlotValue::Present(SecretValue::from_bytes(value).unwrap()),
                None => SlotValue::Missing,
            })
        }

        fn put(&mut self, key: &str, value: &SecretValue) -> Result<(), Self::Error> {
            assert_eq!(key, ROUTE_PROOF_KEY_SLOT);
            self.put_count += 1;
            self.value = Some(value.expose_secret().to_vec());
            Ok(())
        }
    }

    #[test]
    fn creates_exactly_one_route_proof_key() {
        let mut slot = MemorySlot::default();

        assert_eq!(
            ensure_route_proof_key(&mut slot).unwrap(),
            RouteProofKeyStatus::Created
        );
        assert_eq!(slot.value.as_ref().unwrap().len(), ROUTE_PROOF_KEY_BYTES);
        assert_eq!(slot.put_count, 1);

        assert_eq!(
            ensure_route_proof_key(&mut slot).unwrap(),
            RouteProofKeyStatus::Existing
        );
        assert_eq!(slot.put_count, 1);
    }

    #[test]
    fn wrong_existing_key_length_fails_closed_without_overwrite() {
        let original = vec![7_u8; 16];
        let mut slot = MemorySlot {
            value: Some(original.clone()),
            put_count: 0,
        };

        assert_eq!(
            ensure_route_proof_key(&mut slot).unwrap(),
            RouteProofKeyStatus::InvalidStoredKey
        );
        assert_eq!(slot.value.as_ref(), Some(&original));
        assert_eq!(slot.put_count, 0);
    }

    #[test]
    fn empty_existing_value_is_invalid_not_missing() {
        let mut slot = MemorySlot {
            value: Some(Vec::new()),
            put_count: 0,
        };

        assert_eq!(
            ensure_route_proof_key(&mut slot).unwrap(),
            RouteProofKeyStatus::InvalidStoredKey
        );
        assert_eq!(slot.put_count, 0);
    }
}
