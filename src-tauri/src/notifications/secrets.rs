use super::error::{Code, Result};
const SERVICE: &str = "com.masterain.discord-quest-helper.notifications";

pub(super) trait Secrets: Send + Sync {
    fn get(&self, reference: &str) -> Result<String>;
    fn set(&self, reference: &str, secret: &str) -> Result<()>;
    fn delete(&self, reference: &str) -> Result<()>;
}
pub(super) struct NativeSecrets;
impl Secrets for NativeSecrets {
    fn get(&self, reference: &str) -> Result<String> {
        keyring::Entry::new(SERVICE, reference)
            .and_then(|e| e.get_password())
            .map_err(|_| Code::SecretStore.into())
    }
    fn set(&self, reference: &str, secret: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, reference)
            .and_then(|e| e.set_password(secret))
            .map_err(|_| Code::SecretStore.into())
    }
    fn delete(&self, reference: &str) -> Result<()> {
        match keyring::Entry::new(SERVICE, reference).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            _ => Err(Code::SecretStore.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "writes and deletes a temporary native credential; run explicitly on a desktop"]
    fn native_credential_smoke() {
        let reference = format!("test-{}", uuid::Uuid::new_v4());
        NativeSecrets
            .set(&reference, "notification-smoke-test")
            .unwrap();
        let read = NativeSecrets.get(&reference);
        let cleanup = NativeSecrets.delete(&reference);
        assert_eq!(read.unwrap(), "notification-smoke-test");
        cleanup.unwrap();
    }
}
