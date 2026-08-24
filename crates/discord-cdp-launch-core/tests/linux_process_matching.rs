#![cfg(target_os = "linux")]

use discord_cdp_launch_core::{
    classify_linux_process, DiscordChannel, DiscordInstall, LinuxProcessInfo,
};
use std::ffi::OsString;
use std::path::PathBuf;

fn install(channel: DiscordChannel, path: &str) -> DiscordInstall {
    let path = PathBuf::from(path);
    let working_dir = path.parent().map(ToOwned::to_owned).unwrap_or_default();
    DiscordInstall {
        channel,
        executable_path: path,
        working_dir,
    }
}

fn process(pid: u32, exe: Option<&str>, argv: &[&str], comm: Option<&str>) -> LinuxProcessInfo {
    LinuxProcessInfo {
        pid,
        executable: exe.map(PathBuf::from),
        comm: comm.map(str::to_string),
        cmdline: argv.iter().map(OsString::from).collect(),
    }
}

#[test]
fn matches_by_executable_path_including_renderer_children() {
    let installs = [install(DiscordChannel::Stable, "/opt/Discord/Discord")];
    let main = process(
        10,
        Some("/opt/Discord/Discord"),
        &["/opt/Discord/Discord"],
        Some("Discord"),
    );
    let child = process(
        11,
        Some("/opt/Discord/Discord"),
        &["/opt/Discord/Discord", "--type=renderer"],
        Some("Discord"),
    );
    assert_eq!(
        classify_linux_process(&main, &installs),
        Some(DiscordChannel::Stable)
    );
    assert_eq!(
        classify_linux_process(&child, &installs),
        Some(DiscordChannel::Stable)
    );
}

#[test]
fn matches_ptb_and_canary_by_argv0_without_installs() {
    let installs: [DiscordInstall; 0] = [];
    let ptb = process(12, None, &["/usr/bin/DiscordPTB"], None);
    assert_eq!(
        classify_linux_process(&ptb, &installs),
        Some(DiscordChannel::Ptb)
    );
    let canary = process(13, None, &["discord-canary"], None);
    assert_eq!(
        classify_linux_process(&canary, &installs),
        Some(DiscordChannel::Canary)
    );
}

#[test]
fn matches_by_comm_when_cmdline_missing() {
    let installs: [DiscordInstall; 0] = [];
    let ptb = process(14, None, &[], Some("DiscordPTB"));
    assert_eq!(
        classify_linux_process(&ptb, &installs),
        Some(DiscordChannel::Ptb)
    );
}

#[test]
fn controlled_runtime_identities_are_never_classified_as_discord() {
    let installs = [install(DiscordChannel::Stable, "/opt/app/meridian")];
    let main = process(
        15,
        Some("/opt/app/meridian"),
        &["/opt/app/meridian"],
        Some("meridian"),
    );
    let runner = process(16, None, &["/tmp/games/stagecraft"], Some("stagecraft"));
    let bridge = process(
        17,
        None,
        &["waybridge", "--port", "9223"],
        Some("waybridge"),
    );
    assert_eq!(classify_linux_process(&main, &installs), None);
    assert_eq!(classify_linux_process(&runner, &installs), None);
    assert_eq!(classify_linux_process(&bridge, &installs), None);
}

#[test]
fn identity_matching_is_exact_and_does_not_hide_real_games() {
    let installs = [install(DiscordChannel::Stable, "/games/meridian-online")];
    let real_game = process(
        18,
        Some("/games/meridian-online"),
        &["/games/meridian-online"],
        Some("meridian-online"),
    );
    assert_eq!(
        classify_linux_process(&real_game, &installs),
        Some(DiscordChannel::Stable)
    );
}

#[test]
fn does_not_match_unrelated_processes_or_discord_arguments() {
    let installs = [install(DiscordChannel::Stable, "/opt/Discord/Discord")];
    let firefox = process(
        19,
        Some("/usr/bin/firefox"),
        &["/usr/bin/firefox", "https://discord.com/app"],
        Some("firefox"),
    );
    let pwa = process(
        20,
        Some("/usr/bin/chrome"),
        &["/usr/bin/chrome", "--app=https://discord.com"],
        Some("chrome"),
    );
    assert_eq!(classify_linux_process(&firefox, &installs), None);
    assert_eq!(classify_linux_process(&pwa, &installs), None);
}

#[test]
fn stable_name_does_not_capture_ptb_or_canary() {
    let installs: [DiscordInstall; 0] = [];
    // Exact-match guarantee: the generic "discord" Stable name must not swallow
    // channel-specific process names.
    let ptb = process(20, None, &["discord-ptb"], Some("discord-ptb"));
    assert_eq!(
        classify_linux_process(&ptb, &installs),
        Some(DiscordChannel::Ptb)
    );
    let canary = process(21, None, &["discord-canary"], Some("discord-canary"));
    assert_eq!(
        classify_linux_process(&canary, &installs),
        Some(DiscordChannel::Canary)
    );
}
