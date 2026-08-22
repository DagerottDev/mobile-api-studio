const SERVICE_NAME: &str = "dev.mobileapistudio.environment";

#[derive(Debug, Clone, Default)]
pub struct SecretStore;

impl SecretStore {
    pub fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }

    pub fn set(&self, reference: &str, secret: &str) -> Result<(), SecretStoreError> {
        platform::set(reference, secret)
    }

    pub fn get(&self, reference: &str) -> Result<Option<String>, SecretStoreError> {
        platform::get(reference)
    }

    pub fn delete(&self, reference: &str) -> Result<(), SecretStoreError> {
        platform::delete(reference)
    }
}

#[derive(Debug, Clone)]
pub struct SecretStoreError {
    pub code: String,
    pub message: String,
}

impl SecretStoreError {
    fn unsupported() -> Self {
        Self {
            code: "secure_store_unsupported".into(),
            message: "Native secure environment-variable storage is currently implemented for macOS.".into(),
        }
    }

    fn platform(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SecretStoreError {}

#[cfg(target_os = "macos")]
mod platform {
    use super::{SecretStoreError, SERVICE_NAME};
    use keyring::{Entry, Error};

    pub fn set(reference: &str, secret: &str) -> Result<(), SecretStoreError> {
        entry(reference)?
            .set_password(secret)
            .map_err(|error| SecretStoreError::platform("secure_store_write_failed", error.to_string()))
    }

    pub fn get(reference: &str) -> Result<Option<String>, SecretStoreError> {
        match entry(reference)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretStoreError::platform(
                "secure_store_read_failed",
                error.to_string(),
            )),
        }
    }

    pub fn delete(reference: &str) -> Result<(), SecretStoreError> {
        match entry(reference)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretStoreError::platform(
                "secure_store_delete_failed",
                error.to_string(),
            )),
        }
    }

    fn entry(reference: &str) -> Result<Entry, SecretStoreError> {
        Entry::new(SERVICE_NAME, reference).map_err(|error| {
            SecretStoreError::platform("secure_store_entry_failed", error.to_string())
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::SecretStoreError;

    pub fn set(_reference: &str, _secret: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::unsupported())
    }

    pub fn get(_reference: &str) -> Result<Option<String>, SecretStoreError> {
        Err(SecretStoreError::unsupported())
    }

    pub fn delete(_reference: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::unsupported())
    }
}
