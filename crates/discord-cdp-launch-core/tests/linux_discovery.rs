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

    let installs =
        discover_linux_installs_in(&[usr_bin.clone(), opt.clone()], &[], &[user_bin.clone()]);

    let channels: Vec<_> = installs.iter().map(|install| install.channel).collect();
    assert_eq!(
        channels,
        vec![
            DiscordChannel::Stable,
            DiscordChannel::Ptb,
            DiscordChannel::Canary
        ]
    );
    assert_eq!(
        installs[0].executable_path,
        fs::canonicalize(usr_bin.join("discord")).unwrap()
    );
    assert_eq!(
        installs[1].executable_path,
        fs::canonicalize(opt.join("DiscordPTB/DiscordPTB")).unwrap()
    );
    assert_eq!(
        installs[2].executable_path,
        fs::canonicalize(user_bin.join("discord-canary")).unwrap()
    );
    assert_eq!(
        installs[1].working_dir,
        installs[1].executable_path.parent().unwrap()
    );

    fs::remove_dir_all(&root).ok();
}

#[test]
fn ignores_non_executable_files() {
    let root = unique_temp_root("dqh-linux-nonexec");
    let usr_bin = root.join("usr/bin");
    write_plain(&usr_bin.join("discord"));

    let installs = discover_linux_installs_in(&[usr_bin.clone()], &[], &[]);
    assert!(installs.is_empty());

    fs::remove_dir_all(&root).ok();
}

#[test]
fn resolves_symlink_to_canonical_target_and_dedupes() {
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
    assert_eq!(
        installs[0].executable_path,
        fs::canonicalize(&real).unwrap()
    );

    fs::remove_dir_all(&root).ok();
}

#[test]
fn empty_when_nothing_present() {
    let root = unique_temp_root("dqh-linux-empty");
    fs::create_dir_all(&root).unwrap();
    let installs = discover_linux_installs_in(&[root.join("usr/bin")], &[root.join("home")], &[]);
    assert!(installs.is_empty());
    fs::remove_dir_all(&root).ok();
}
