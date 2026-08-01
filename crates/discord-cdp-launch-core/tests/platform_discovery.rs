#[cfg(any(target_os = "windows", target_os = "macos"))]
fn unique_temp_root(label: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{label}-{}-{nonce}", std::process::id()))
}

#[cfg(target_os = "windows")]
#[test]
fn windows_discovery_uses_numeric_versions_and_direct_fallback() {
    use discord_cdp_launch_core::{discover_windows_installs_in, DiscordChannel};
    use std::fs;

    let root = unique_temp_root("discord-cdp-windows-discovery");
    let old = root.join("Discord").join("app-1.0.9999");
    let new = root.join("Discord").join("app-1.0.10000");
    let ptb = root.join("DiscordPTB");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::create_dir_all(&ptb).unwrap();
    fs::write(old.join("Discord.exe"), []).unwrap();
    fs::write(new.join("Discord.exe"), []).unwrap();
    fs::write(ptb.join("DiscordPTB.exe"), []).unwrap();

    let installs = discover_windows_installs_in(&root);
    assert_eq!(installs.len(), 2);
    assert_eq!(installs[0].channel, DiscordChannel::Stable);
    assert_eq!(installs[0].executable_path, new.join("Discord.exe"));
    assert_eq!(installs[1].channel, DiscordChannel::Ptb);
    assert_eq!(installs[1].executable_path, ptb.join("DiscordPTB.exe"));

    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_discovery_supports_all_channels_and_fallback_executable() {
    use discord_cdp_launch_core::{discover_macos_installs_in, DiscordChannel};
    use std::fs;

    let root = unique_temp_root("discord-cdp-macos-discovery");
    let cases = [
        ("Discord.app", "Discord"),
        ("Discord PTB.app", "Discord PTB"),
        ("Discord Canary.app", "Discord"),
    ];
    for (bundle, executable) in cases {
        let directory = root.join(bundle).join("Contents").join("MacOS");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(executable), []).unwrap();
    }

    let installs = discover_macos_installs_in(std::slice::from_ref(&root));
    assert_eq!(
        installs
            .iter()
            .map(|install| install.channel)
            .collect::<Vec<_>>(),
        vec![
            DiscordChannel::Stable,
            DiscordChannel::Ptb,
            DiscordChannel::Canary
        ]
    );

    fs::remove_dir_all(root).unwrap();
}
