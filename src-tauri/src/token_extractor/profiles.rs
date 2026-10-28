//! Client metadata and deterministic profile discovery. Adding a client should
//! not require changing the storage scanner or an OS decryption implementation.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ClientSpec {
    pub display_name: &'static str,
    pub directory_names: &'static [&'static str],
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub flatpak_ids: &'static [&'static str],
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub secret_applications: &'static [&'static str],
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub keychain_names: &'static [&'static str],
}

// Directory case matters on Linux and on case-sensitive macOS volumes. Both
// current product names and existing lowercase profiles are retained.
pub(super) const CLIENTS: &[ClientSpec] = &[
    ClientSpec {
        display_name: "Discord Stable",
        directory_names: &["discord"],
        flatpak_ids: &["com.discordapp.Discord"],
        secret_applications: &["discord"],
        keychain_names: &["discord"],
    },
    ClientSpec {
        display_name: "Discord Canary",
        directory_names: &["discordcanary"],
        flatpak_ids: &["com.discordapp.DiscordCanary"],
        secret_applications: &["discordcanary"],
        keychain_names: &["discordcanary"],
    },
    ClientSpec {
        display_name: "Discord PTB",
        directory_names: &["discordptb"],
        flatpak_ids: &["com.discordapp.DiscordPTB"],
        secret_applications: &["discordptb"],
        keychain_names: &["discordptb"],
    },
    ClientSpec {
        display_name: "Equibop",
        directory_names: &["equibop", "Equibop"],
        flatpak_ids: &["org.equicord.equibop", "io.github.equicord.equibop"],
        secret_applications: &["equibop", "equicord", "discord"],
        keychain_names: &["Equibop", "equibop"],
    },
    ClientSpec {
        display_name: "Legcord",
        directory_names: &["legcord", "Legcord"],
        flatpak_ids: &["app.legcord.Legcord"],
        secret_applications: &["legcord", "discord"],
        keychain_names: &["Legcord", "legcord"],
    },
    ClientSpec {
        display_name: "Vesktop",
        directory_names: &["vesktop", "Vesktop"],
        flatpak_ids: &["dev.vencord.Vesktop"],
        secret_applications: &["vesktop", "discord"],
        keychain_names: &["Vesktop", "vesktop"],
    },
    ClientSpec {
        display_name: "ArmCord",
        directory_names: &["ArmCord", "armcord"],
        flatpak_ids: &["xyz.armcord.ArmCord"],
        secret_applications: &["armcord", "discord"],
        keychain_names: &["ArmCord", "armcord"],
    },
    ClientSpec {
        display_name: "WebCord",
        directory_names: &["WebCord", "webcord"],
        flatpak_ids: &["io.github.spacingbat3.webcord"],
        secret_applications: &["webcord", "discord"],
        keychain_names: &["WebCord", "webcord"],
    },
];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ChromiumProfile {
    pub client: &'static ClientSpec,
    pub root: PathBuf,
}

pub(super) fn profiles_for_root(config_root: &Path) -> Vec<ChromiumProfile> {
    CLIENTS
        .iter()
        .flat_map(|client| {
            client
                .directory_names
                .iter()
                .map(move |directory| ChromiumProfile {
                    client,
                    root: config_root.join(directory),
                })
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn linux_config_root(home: &Path, xdg_config_home: Option<&Path>) -> PathBuf {
    xdg_config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
}

#[cfg(any(target_os = "linux", test))]
fn linux_profiles(home: &Path, config_root: &Path) -> Vec<ChromiumProfile> {
    let mut profiles = profiles_for_root(config_root);
    for client in CLIENTS {
        for app_id in client.flatpak_ids {
            let sandbox_root = home.join(".var/app").join(app_id).join("config");
            for directory in client.directory_names {
                profiles.push(ChromiumProfile {
                    client,
                    root: sandbox_root.join(directory),
                });
            }
        }
    }
    profiles.push(ChromiumProfile {
        client: &CLIENTS[0],
        root: home.join("snap/discord/current/.config/discord"),
    });
    profiles
}

pub(super) fn discover_profiles() -> Result<Vec<ChromiumProfile>> {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .context("Could not get APPDATA environment variable")?;
        Ok(profiles_for_root(Path::new(&appdata)))
    }
    #[cfg(target_os = "macos")]
    {
        let home = home_dir()?;
        Ok(profiles_for_root(&home.join("Library/Application Support")))
    }
    #[cfg(target_os = "linux")]
    {
        let home = home_dir()?;
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        Ok(linux_profiles(
            &home,
            &linux_config_root(&home, xdg.as_deref()),
        ))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    anyhow::bail!("Token extraction is only supported on Windows, macOS, and Linux")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("Could not get HOME environment variable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_profiles_include_every_client_for_each_os_root() {
        for root in [
            "/appdata",
            "/home/test/.config",
            "/Users/test/Library/Application Support",
        ] {
            let profiles = profiles_for_root(Path::new(root));
            for client in CLIENTS {
                assert!(profiles.iter().any(|profile| profile.client == client));
            }
            assert!(profiles
                .iter()
                .any(|profile| profile.root == Path::new(root).join("discord")));
            assert!(profiles
                .iter()
                .any(|profile| profile.root == Path::new(root).join("WebCord")));
            assert!(profiles
                .iter()
                .all(|profile| profile.root.starts_with(root)));
        }
    }

    #[test]
    fn flatpak_profiles_keep_legacy_equibop_and_the_published_armcord_id() {
        let home = Path::new("/home/test");
        let profiles = linux_profiles(home, Path::new("/custom/config"));
        for suffix in [
            ".var/app/io.github.equicord.equibop/config/equibop",
            ".var/app/org.equicord.equibop/config/Equibop",
            ".var/app/xyz.armcord.ArmCord/config/ArmCord",
            ".var/app/app.legcord.Legcord/config/legcord",
            "snap/discord/current/.config/discord",
        ] {
            assert!(
                profiles
                    .iter()
                    .any(|profile| profile.root == home.join(suffix)),
                "{suffix}"
            );
        }
        assert!(!profiles.iter().any(|profile| profile
            .root
            .to_string_lossy()
            .contains("com.github.ArmCord")));
        assert!(profiles
            .iter()
            .any(|profile| profile.root == Path::new("/custom/config/discord")));
    }

    #[test]
    fn config_root_ignores_relative_xdg_paths() {
        let home = Path::new("/home/test");
        let absolute_config = std::env::temp_dir().join("dqh-config-test");
        assert_eq!(
            linux_config_root(home, Some(&absolute_config)),
            absolute_config
        );
        assert_eq!(
            linux_config_root(home, Some(Path::new("relative/config"))),
            home.join(".config")
        );
        assert_eq!(linux_config_root(home, None), home.join(".config"));
    }
}
