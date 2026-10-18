use super::error::{Code, Result};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
const SERVICE: &str = "com.masterain.discord-quest-helper.notifications";

pub(super) trait Secrets: Send + Sync {
    fn get(&self, reference: &str) -> Result<String>;
    fn set(&self, reference: &str, secret: &str) -> Result<()>;
    fn delete(&self, reference: &str) -> Result<()>;
}
// Track accepted configuration snapshots. Retiring an old credential must not
// wait for a slow vault read, nor invalidate a read that has not started yet.
#[derive(Default)]
struct LeaseState {
    readers: usize,
    retired: bool,
}
pub(super) struct LeasedSecrets {
    inner: Arc<dyn Secrets>,
    leases: Mutex<BTreeMap<String, LeaseState>>,
}
pub(super) struct CredentialLease {
    store: Arc<LeasedSecrets>,
    references: Vec<String>,
}
impl LeasedSecrets {
    pub fn new(inner: Arc<dyn Secrets>) -> Self {
        Self {
            inner,
            leases: Default::default(),
        }
    }
    // Called while the service holds its configuration lock, before an updater
    // can replace that snapshot and retire its references.
    pub fn lease(self: &Arc<Self>, references: impl Iterator<Item = String>) -> CredentialLease {
        let references: Vec<_> = references.collect();
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        for reference in &references {
            leases.entry(reference.clone()).or_default().readers += 1;
        }
        CredentialLease {
            store: self.clone(),
            references,
        }
    }
}
impl Secrets for LeasedSecrets {
    fn get(&self, reference: &str) -> Result<String> {
        self.inner.get(reference)
    }
    fn set(&self, reference: &str, value: &str) -> Result<()> {
        self.inner.set(reference, value)
    }
    fn delete(&self, reference: &str) -> Result<()> {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = leases.get_mut(reference) {
            state.retired = true;
            return Ok(());
        }
        drop(leases);
        self.inner.delete(reference)
    }
}
impl Drop for CredentialLease {
    fn drop(&mut self) {
        let mut retired = Vec::new();
        {
            let mut leases = self.store.leases.lock().unwrap_or_else(|e| e.into_inner());
            for reference in &self.references {
                if let Some(state) = leases.get_mut(reference) {
                    state.readers -= 1;
                    if state.readers == 0 {
                        if state.retired {
                            retired.push(reference.clone());
                        }
                        leases.remove(reference);
                    }
                }
            }
        }
        if !retired.is_empty() {
            let inner = self.store.inner.clone();
            // A lease can also be dropped by a cancelled async task. Never run
            // native vault cleanup on the async executor. Shutdown may leave
            // harmless orphans, as with any best-effort post-commit cleanup.
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(move || {
                    for reference in retired {
                        let _ = inner.delete(&reference);
                    }
                });
            }
        }
    }
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
