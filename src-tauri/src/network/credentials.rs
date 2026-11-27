use super::CommandResult;
use reqwest::Proxy;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "com.masterain.discord-quest-helper.network";

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum Edit {
    Keep,
    Replace {
        #[serde(flatten)]
        credentials: Credentials,
    },
    Remove,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Credentials {
    pub(super) username: String,
    pub(super) password: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
    }
}

impl Credentials {
    pub(super) fn validate(&self, scheme: &str) -> CommandResult<()> {
        if matches!(scheme, "socks5" | "socks5h") {
            if !(1..=255).contains(&self.username.len())
                || !(1..=255).contains(&self.password.len())
            {
                return Err("invalid_socks_credentials");
            }
        } else if (self.username.is_empty() && self.password.is_empty())
            || self.username.contains(':')
            || self.username.chars().any(char::is_control)
            || self.password.chars().any(char::is_control)
        {
            return Err("invalid_http_credentials");
        }
        Ok(())
    }

    pub(super) fn apply(&self, proxy: Proxy) -> Proxy {
        // reqwest places these values in URL userinfo and decodes it once.
        // The URL setters escape separators but leave literal percent signs alone.
        let username = Zeroizing::new(self.username.replace('%', "%25"));
        let password = Zeroizing::new(self.password.replace('%', "%25"));
        proxy.basic_auth(&username, &password)
    }
}

pub(super) trait Vault {
    fn read(&self, reference: &str) -> CommandResult<Credentials>;
    fn write(&self, reference: &str, credentials: &Credentials) -> CommandResult<()>;
    fn remove(&self, reference: &str);
}

pub(super) struct NativeVault;

impl Vault for NativeVault {
    fn read(&self, reference: &str) -> CommandResult<Credentials> {
        let data = crate::credential_store::read(SERVICE, reference)
            .map(Zeroizing::new)
            .map_err(|_| "credential_store_error")?;
        serde_json::from_str(&data).map_err(|_| "credential_store_error")
    }

    fn write(&self, reference: &str, credentials: &Credentials) -> CommandResult<()> {
        let data = Zeroizing::new(
            serde_json::to_string(credentials).map_err(|_| "credential_store_error")?,
        );
        crate::credential_store::write(SERVICE, reference, &data)
            .map_err(|_| "credential_store_error")
    }

    fn remove(&self, reference: &str) {
        let _ = crate::credential_store::remove(SERVICE, reference);
    }
}
