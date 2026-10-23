#![cfg(target_os = "linux")]

use discord_cdp_launch_core::{discover_linux_installs_in, DiscordChannel};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{label}-{}-{nonce}", std::process::id()))
}

fn write_executable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"#!/bin/sh\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn write_plain(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"data").unwrap();
}

#[test]
fn discovers_channels_by_priority_and_orders_stable_ptb_canary() {
    let root = unique_temp_root("dqh-linux-discovery");
    let usr_bin = root.join("usr/bin");
    let opt = root.join("opt");
    let user_bin = root.join("home/.local/bin");

    // Stable exists both as a system bin and under opt: the system bin must win.
    write_executable(&usr_bin.join("discord"));
    write_executable(&opt.join("Discord/Discord"));
    // PTB only under opt.
    write_executable(&opt.join("DiscordPTB/DiscordPTB"));
    // Canary only via a PATH directory.
    write_executable(&user_bin.join("discord-canary"));
    // A non-executable decoy that must be ignored.
    write_plain(&usr_bin.join("discord-ptb"));

    let installs = discover_linux_installs_in(
        &[usr_bin.clone(), opt.clone()],
        &[],
        std::slice::from_ref(&user_bin),
    );

    let channels: Vec<_> = installs.iter().map(|install| install.channel).collect();
    assert_eq!(
        channels,
        vec![
            DiscordChannel::Stable,
            DiscordChannel::Ptb,
            DiscordChannel::Canary
        ]
    );
    assert_eq!(installs[0].executable_path, usr_bin.join("discord"));
    assert_eq!(
        installs[1].executable_path,
        opt.join("DiscordPTB/DiscordPTB")
    );
    assert_eq!(installs[2].executable_path, user_bin.join("discord-canary"));
    assert_eq!(
        installs[1].working_dir,
        fs::canonicalize(installs[1].executable_path.parent().unwrap()).unwrap()
    );

    fs::remove_dir_all(&root).ok();
}

#[test]
fn ignores_non_executable_files() {
    let root = unique_temp_root("dqh-linux-nonexec");
    let usr_bin = root.join("usr/bin");
    write_plain(&usr_bin.join("discord"));

    let installs = discover_linux_installs_in(std::slice::from_ref(&usr_bin), &[], &[]);
    assert!(installs.is_empty());

    fs::remove_dir_all(&root).ok();
}

#[test]
fn symlink_launcher_survives_target_updates_and_dedupes() {
    let root = unique_temp_root("dqh-linux-symlink");
    let real = root.join("opt/Discord/Discord");
    write_executable(&real);
    let bin = root.join("usr/bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&real, bin.join("discord")).unwrap();

    // Both a bin symlink and the opt target resolve to the same canonical path;
    // Stable must appear exactly once.
    let installs = discover_linux_installs_in(&[bin.clone(), root.join("opt")], &[], &[]);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].channel, DiscordChannel::Stable);
    assert_eq!(installs[0].executable_path, bin.join("discord"));

    use discord_cdp_launch_core::{
        custom_executable_installation, refresh_automatic_installation, DiscoverySource,
        LaunchTarget, ProviderId,
    };
    let mut saved =
        custom_executable_installation(&ProviderId::official_discord(), bin.join("discord"))
            .unwrap();
    saved.source = DiscoverySource::StandardPath;
    let saved = refresh_automatic_installation(&saved).unwrap().unwrap();
    let updated = root.join("versions/next/Discord");
    write_executable(&updated);
    fs::remove_file(bin.join("discord")).unwrap();
    std::os::unix::fs::symlink(&updated, bin.join("discord")).unwrap();
    fs::remove_file(&real).unwrap();
    let current = refresh_automatic_installation(&saved).unwrap().unwrap();
    let expected_working_dir = fs::canonicalize(updated.parent().unwrap()).unwrap();
    assert_eq!(current.id, saved.id);
    assert!(
        matches!(current.launch_target, LaunchTarget::Executable { path, working_dir, .. } if path == bin.join("discord") && working_dir == expected_working_dir)
    );

    fs::remove_dir_all(&root).ok();
}

#[test]
fn relative_path_entry_keeps_an_absolute_symlink_launcher() {
    let relative_root = PathBuf::from(unique_temp_root("dqh-linux-relative").file_name().unwrap());
    let root = std::env::current_dir().unwrap().join(&relative_root);
    let target = root.join("versions/current/Discord");
    write_executable(&target);
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&target, bin.join("discord")).unwrap();

    let installs = discover_linux_installs_in(&[], &[], &[relative_root.join("bin")]);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].executable_path, bin.join("discord"));
    assert!(installs[0].executable_path.is_absolute());
    assert_eq!(installs[0].working_dir, target.parent().unwrap());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn empty_when_nothing_present() {
    let root = unique_temp_root("dqh-linux-empty");
    fs::create_dir_all(&root).unwrap();
    let installs = discover_linux_installs_in(&[root.join("usr/bin")], &[root.join("home")], &[]);
    assert!(installs.is_empty());
    fs::remove_dir_all(&root).ok();
}
