// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    discord_quest_helper_lib::ensure_stealth_and_run();
}
