pub(crate) fn read(service: &str, reference: &str) -> keyring::Result<String> {
    keyring::Entry::new(service, reference)?.get_password()
}

pub(crate) fn write(service: &str, reference: &str, secret: &str) -> keyring::Result<()> {
    keyring::Entry::new(service, reference)?.set_password(secret)
}

pub(crate) fn remove(service: &str, reference: &str) -> keyring::Result<()> {
    match keyring::Entry::new(service, reference).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        error => error,
    }
}
