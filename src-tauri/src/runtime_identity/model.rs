use serde::{Deserialize, Serialize};

pub const RUNTIME_MAIN_NAME: &str = "meridian";
pub const RUNTIME_BRIDGE_NAME: &str = "waybridge";
pub const RUNTIME_NAMESPACE: &str = "blueorbit";
pub const RUNTIME_RUNNER_BUILD_NAME: &str = "stagecraft";

pub const PRODUCT_TOKENS: &[&str] = &[
    "discord-quest-helper",
    "discord_quest_helper",
    "discord quest helper",
    "discord-cdp-launcher",
    "discord-quest-runner",
    "discordquesthelper",
    "dqh",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeIdentityLevel {
    Full,
    Degraded,
    Disabled,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityStatus {
    pub platform: String,
    pub level: RuntimeIdentityLevel,
    pub main_executable_ok: bool,
    pub helper_identity_ok: Option<bool>,
    pub package_signature_ok: Option<bool>,
    pub desktop_integration_ok: Option<bool>,
    pub reasons: Vec<String>,
}

impl RuntimeIdentityStatus {
    pub fn disabled(platform: &str, reason: impl Into<String>) -> Self {
        Self {
            platform: platform.to_string(),
            level: RuntimeIdentityLevel::Disabled,
            main_executable_ok: false,
            helper_identity_ok: None,
            package_signature_ok: None,
            desktop_integration_ok: None,
            reasons: vec![reason.into()],
        }
    }

    pub fn recompute_level(&mut self) {
        if self.level == RuntimeIdentityLevel::Disabled
            || self.level == RuntimeIdentityLevel::NotApplicable
        {
            return;
        }
        let optional_checks_ok = [
            self.helper_identity_ok,
            self.package_signature_ok,
            self.desktop_integration_ok,
        ]
        .into_iter()
        .all(|check| check.unwrap_or(true));
        self.level = if self.main_executable_ok && optional_checks_ok && self.reasons.is_empty() {
            RuntimeIdentityLevel::Full
        } else {
            RuntimeIdentityLevel::Degraded
        };
    }
}

pub fn contains_product_token(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    PRODUCT_TOKENS
        .iter()
        .any(|token| normalized.contains(&token.to_ascii_lowercase()))
}

pub fn valid_internal_name(name: &str) -> bool {
    (6..=14).contains(&name.len())
        && name.bytes().all(|byte| byte.is_ascii_lowercase())
        && !name.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !contains_product_token(name)
}

pub fn configured_internal_names_are_valid() -> bool {
    [
        RUNTIME_MAIN_NAME,
        RUNTIME_BRIDGE_NAME,
        RUNTIME_NAMESPACE,
        RUNTIME_RUNNER_BUILD_NAME,
    ]
    .into_iter()
    .all(valid_internal_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_names_are_stable_neutral_and_fit_linux_comm() {
        for name in [
            RUNTIME_MAIN_NAME,
            RUNTIME_BRIDGE_NAME,
            RUNTIME_NAMESPACE,
            RUNTIME_RUNNER_BUILD_NAME,
        ] {
            assert!(
                valid_internal_name(name),
                "invalid runtime identity: {name}"
            );
            assert!(name.len() <= 15, "Linux comm would truncate {name}");
        }
        assert_ne!(RUNTIME_MAIN_NAME, RUNTIME_BRIDGE_NAME);
    }

    #[test]
    fn product_tokens_are_case_insensitive() {
        assert!(contains_product_token("/opt/DiscordQuestHelper/bin"));
        assert!(contains_product_token("DISCORD-CDP-LAUNCHER"));
        assert!(!contains_product_token("/opt/blueorbit/waybridge"));
    }

    #[test]
    fn failed_optional_check_degrades_status() {
        let mut status = RuntimeIdentityStatus {
            platform: "linux".into(),
            level: RuntimeIdentityLevel::Full,
            main_executable_ok: true,
            helper_identity_ok: Some(false),
            package_signature_ok: None,
            desktop_integration_ok: Some(true),
            reasons: vec!["helper verification failed".into()],
        };
        status.recompute_level();
        assert_eq!(status.level, RuntimeIdentityLevel::Degraded);
    }
}
