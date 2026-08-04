use crate::discord_api::DiscordApiClient;
use anyhow::Result;
use rand::RngExt;
use std::time::Duration;
use tauri::Emitter;
use tokio::time::{sleep, sleep_until, Instant};

use crate::models::PlayActivityHeartbeatStatus;

const PLAY_ACTIVITY_RETRY_DELAY_SECS: u64 = 5;
const PLAY_ACTIVITY_MAX_CONSECUTIVE_ERRORS: u32 = 3;
const PLAY_ACTIVITY_TIMEOUT_GRACE_SECS: u64 = 300;

fn play_activity_progress_pct(progress_seconds: f64, seconds_needed: u32, completed: bool) -> f64 {
    if completed {
        return 100.0;
    }
    if seconds_needed == 0 {
        return 0.0;
    }
    (progress_seconds / seconds_needed as f64 * 100.0).clamp(0.0, 99.0)
}

fn play_activity_reached_target(status: PlayActivityHeartbeatStatus, seconds_needed: u32) -> bool {
    status.completed || status.progress_seconds >= seconds_needed as f64
}

async fn confirm_play_activity_via_api(
    client: &DiscordApiClient,
    quest_id: &str,
    seconds_needed: u32,
    status: PlayActivityHeartbeatStatus,
    app_handle: &tauri::AppHandle,
    cancel_rx: &mut tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    let terminal_status = client
        .send_play_activity_heartbeat(quest_id, None, true)
        .await
        .ok();
    let mut confirmed = status.completed
        || terminal_status
            .map(|terminal| terminal.completed)
            .unwrap_or(false);

    for attempt in 1..=6 {
        if confirmed {
            break;
        }

        if let Ok((progress, completed)) = client.get_quest_progress(quest_id).await {
            let _ = app_handle.emit(
                "quest-progress",
                play_activity_progress_pct(progress, seconds_needed, completed),
            );
            confirmed = completed;
        }

        if !confirmed && attempt < 6 {
            tokio::select! {
                _ = sleep(Duration::from_secs(2)) => {},
                _ = cancel_rx.recv() => {
                    let _ = app_handle.emit("quest-stopped", ());
                    return Ok(());
                }
            }
        }
    }

    if confirmed {
        let _ = app_handle.emit("quest-progress", 100.0f64);
        let _ = app_handle.emit("quest-complete", ());
        return Ok(());
    }

    anyhow::bail!("PLAY_ACTIVITY reached its target, but Discord did not confirm completion")
}

/// Complete a video quest
///
/// Simulates watching a video by incrementally sending video progress
/// Based on power0matin's approach: POST { timestamp: seconds } to /quests/{id}/video-progress
#[allow(clippy::too_many_arguments)]
pub async fn complete_video_quest(
    client: &DiscordApiClient,
    quest_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    speed_multiplier: f64,
    heartbeat_interval: u64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    // Progress control parameters (based on power0matin research)
    // Speed: how many seconds to advance per update (configurable)
    if speed_multiplier <= 0.0 {
        anyhow::bail!("speed_multiplier must be greater than 0");
    }
    let speed = speed_multiplier;
    // Interval: how often to send updates (in real seconds)
    let interval = heartbeat_interval;

    // Convert initial progress (percentage) to seconds
    let mut current_seconds = initial_progress / 100.0 * seconds_needed as f64;

    println!("Starting video quest: quest_id={}, target={}s, current_progress={:.1}s, speed={:.1}x, interval={}s", 
             quest_id, seconds_needed, current_seconds, speed, interval);

    loop {
        // Calculate the remaining simulated seconds, then the real wait time
        let remaining_sim_seconds = (seconds_needed as f64) - current_seconds;
        let real_seconds_to_finish = if speed > 0.0 {
            remaining_sim_seconds / speed
        } else {
            interval as f64
        };
        let wait_secs = (real_seconds_to_finish.ceil() as u64).min(interval).max(1);

        // Wait before advancing progress (prevents immediate jump on first iteration)
        tokio::select! {
            _ = sleep(Duration::from_secs(wait_secs)) => {},
            _ = cancel_rx.recv() => {
                println!("Video quest cancelled");
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }

        // Advance timestamp based on speed and actual wait time
        current_seconds += speed * (wait_secs as f64);
        let timestamp = current_seconds.min(seconds_needed as f64);

        // Add some randomness to look more natural
        let timestamp_with_jitter = timestamp + rand::rng().random_range(0.0..0.5);

        // Send progress update
        match client
            .update_video_progress(&quest_id, timestamp_with_jitter)
            .await
        {
            Ok(completed) => {
                // Calculate and emit progress percentage
                let progress = (timestamp / seconds_needed as f64 * 100.0).min(100.0);
                let _ = app_handle.emit("quest-progress", progress);

                println!(
                    "Video quest progress: {:.1}% ({:.0}/{} s)",
                    progress, timestamp, seconds_needed
                );

                if completed || timestamp >= seconds_needed as f64 {
                    let _ = app_handle.emit("quest-complete", ());
                    println!("Video quest completed!");
                    return Ok(());
                }
            }
            Err(e) => {
                println!("Video progress update failed: {}", e);
                let _ = app_handle.emit("quest-error", e.to_string());
                return Err(e);
            }
        }
    }
}

/// Complete a stream quest
///
/// Maintains streaming status by periodically sending heartbeats
pub async fn complete_stream_quest(
    client: &DiscordApiClient,
    quest_id: String,
    stream_key: String,
    seconds_needed: u32,
    initial_progress: f64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    // Heartbeat interval (30 seconds)
    let heartbeat_interval = 30;
    let total_heartbeats = seconds_needed.div_ceil(heartbeat_interval);

    // Start from initial progress
    let start_heartbeat = (initial_progress / 100.0 * total_heartbeats as f64) as u32;

    for i in start_heartbeat..total_heartbeats {
        // Check cancel signal
        if cancel_rx.try_recv().is_ok() {
            println!("Stream quest cancelled");
            return Ok(());
        }

        // Send heartbeat
        client.send_stream_heartbeat(&quest_id, &stream_key).await?;

        // Calculate and send progress percentage
        let progress = ((i + 1) as f64 / total_heartbeats as f64) * 100.0;
        let _ = app_handle.emit("quest-progress", progress);

        println!("Stream quest progress: {:.1}%", progress);

        if i == total_heartbeats - 1 {
            let _ = app_handle.emit("quest-complete", ());
            println!("Stream quest completed!");
            break;
        }

        // Wait for next heartbeat
        tokio::select! {
            _ = sleep(Duration::from_secs(heartbeat_interval as u64)) => {},
            _ = cancel_rx.recv() => {
                println!("Stream quest cancelled");
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Complete a game quest by sending direct heartbeat requests
///
/// This is an alternative to running a simulated game executable.
/// Based on HAR analysis: POST { application_id, terminal: false } every 60 seconds
pub async fn complete_game_quest_via_heartbeat(
    client: &DiscordApiClient,
    quest_id: String,
    application_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    // Fixed heartbeat interval: 60 seconds (based on Discord client behavior)
    const HEARTBEAT_INTERVAL: u64 = 60;

    let total_heartbeats = (seconds_needed as u64).div_ceil(HEARTBEAT_INTERVAL);

    // Start from initial progress
    let start_heartbeat = (initial_progress / 100.0 * total_heartbeats as f64) as u64;

    println!("Starting game quest via heartbeat: quest_id={}, app_id={}, target={}s, interval={}s, total_beats={}", 
             quest_id, application_id, seconds_needed, HEARTBEAT_INTERVAL, total_heartbeats);

    for i in start_heartbeat..total_heartbeats {
        // Check cancel signal
        if cancel_rx.try_recv().is_ok() {
            println!("Game quest cancelled");
            let _ = app_handle.emit("quest-stopped", ());
            return Ok(());
        }

        // Determine if this is the last heartbeat (terminal)
        let is_last = i == total_heartbeats - 1;

        // Send heartbeat
        match client
            .send_game_heartbeat(&quest_id, &application_id, is_last)
            .await
        {
            Ok(completed) => {
                // Calculate and send progress percentage
                let progress = ((i + 1) as f64 / total_heartbeats as f64) * 100.0;
                let _ = app_handle.emit("quest-progress", progress);

                println!(
                    "Game quest progress: {:.1}% (heartbeat {}/{})",
                    progress,
                    i + 1,
                    total_heartbeats
                );

                if completed || is_last {
                    let _ = app_handle.emit("quest-complete", ());
                    println!("Game quest completed!");
                    return Ok(());
                }
            }
            Err(e) => {
                println!("Game heartbeat failed: {}", e);
                let _ = app_handle.emit("quest-error", e.to_string());
                return Err(e);
            }
        }

        // Wait for next heartbeat (60 seconds)
        tokio::select! {
            _ = sleep(Duration::from_secs(HEARTBEAT_INTERVAL)) => {},
            _ = cancel_rx.recv() => {
                println!("Game quest cancelled");
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Complete a PLAY_ACTIVITY cloud-game quest via the authenticated API client.
///
/// Progress is server-timed between heartbeat requests. Unlike the legacy game
/// heartbeat runner, this always trusts `progress.PLAY_ACTIVITY.value` and only
/// emits completion after Discord reports `completed_at`.
#[allow(clippy::too_many_arguments)]
pub async fn complete_play_activity_via_heartbeat(
    client: &DiscordApiClient,
    quest_id: String,
    application_id: String,
    seconds_needed: u32,
    initial_progress: f64,
    heartbeat_interval_secs: u64,
    progress_polling_interval_secs: u64,
    app_handle: tauri::AppHandle,
    mut cancel_rx: tokio::sync::mpsc::Receiver<()>,
) -> Result<()> {
    if seconds_needed == 0 {
        anyhow::bail!("PLAY_ACTIVITY target must be greater than zero");
    }
    if heartbeat_interval_secs == 0 || progress_polling_interval_secs == 0 {
        anyhow::bail!("PLAY_ACTIVITY intervals must be greater than zero");
    }

    let remaining_seconds = (seconds_needed as f64 - initial_progress.max(0.0))
        .max(0.0)
        .ceil() as u64;
    let max_duration =
        Duration::from_secs(remaining_seconds.saturating_add(PLAY_ACTIVITY_TIMEOUT_GRACE_SECS));
    let timeout_at = Instant::now() + max_duration;
    let heartbeat_interval = Duration::from_secs(heartbeat_interval_secs);
    let progress_polling_interval = Duration::from_secs(progress_polling_interval_secs);
    let mut next_progress_poll = Instant::now() + progress_polling_interval;
    let mut session_started = false;
    let mut consecutive_errors = 0u32;

    let _ = app_handle.emit(
        "quest-progress",
        play_activity_progress_pct(initial_progress, seconds_needed, false),
    );

    loop {
        if cancel_rx.try_recv().is_ok() {
            if session_started {
                let _ = client
                    .send_play_activity_heartbeat(&quest_id, None, true)
                    .await;
            }
            let _ = app_handle.emit("quest-stopped", ());
            return Ok(());
        }

        if Instant::now() >= timeout_at {
            if session_started {
                let _ = client
                    .send_play_activity_heartbeat(&quest_id, None, true)
                    .await;
            }
            anyhow::bail!("PLAY_ACTIVITY timed out before Discord confirmed completion");
        }

        let status = match client
            .send_play_activity_heartbeat(&quest_id, Some(&application_id), false)
            .await
        {
            Ok(status) => {
                session_started = true;
                consecutive_errors = 0;
                status
            }
            Err(error) => {
                consecutive_errors += 1;
                if consecutive_errors >= PLAY_ACTIVITY_MAX_CONSECUTIVE_ERRORS {
                    if session_started {
                        let _ = client
                            .send_play_activity_heartbeat(&quest_id, None, true)
                            .await;
                    }
                    return Err(error.context("PLAY_ACTIVITY heartbeat failed three times"));
                }

                tokio::select! {
                    _ = sleep(Duration::from_secs(PLAY_ACTIVITY_RETRY_DELAY_SECS)) => {},
                    _ = cancel_rx.recv() => {
                        if session_started {
                            let _ = client.send_play_activity_heartbeat(&quest_id, None, true).await;
                        }
                        let _ = app_handle.emit("quest-stopped", ());
                        return Ok(());
                    }
                }
                continue;
            }
        };

        if play_activity_reached_target(status, seconds_needed) {
            return confirm_play_activity_via_api(
                client,
                &quest_id,
                seconds_needed,
                status,
                &app_handle,
                &mut cancel_rx,
            )
            .await;
        }

        let next_heartbeat = Instant::now() + heartbeat_interval;
        loop {
            let wake_at = next_heartbeat.min(next_progress_poll).min(timeout_at);
            tokio::select! {
                _ = sleep_until(wake_at) => {},
                _ = cancel_rx.recv() => {
                    let _ = client.send_play_activity_heartbeat(&quest_id, None, true).await;
                    let _ = app_handle.emit("quest-stopped", ());
                    return Ok(());
                }
            }

            let now = Instant::now();
            if now >= timeout_at {
                let _ = client
                    .send_play_activity_heartbeat(&quest_id, None, true)
                    .await;
                anyhow::bail!("PLAY_ACTIVITY timed out before Discord confirmed completion");
            }

            if now >= next_progress_poll {
                if let Ok((progress, completed)) = client.get_quest_progress(&quest_id).await {
                    consecutive_errors = 0;
                    let polled_status = PlayActivityHeartbeatStatus {
                        progress_seconds: progress,
                        completed,
                    };
                    let _ = app_handle.emit(
                        "quest-progress",
                        play_activity_progress_pct(progress, seconds_needed, completed),
                    );
                    if play_activity_reached_target(polled_status, seconds_needed) {
                        return confirm_play_activity_via_api(
                            client,
                            &quest_id,
                            seconds_needed,
                            polled_status,
                            &app_handle,
                            &mut cancel_rx,
                        )
                        .await;
                    }
                }
                next_progress_poll = Instant::now() + progress_polling_interval;
            }

            if Instant::now() >= next_heartbeat {
                break;
            }
        }
    }
}

#[allow(dead_code)]
fn generate_stream_key() -> String {
    use rand::distr::Alphanumeric;
    use rand::RngExt;

    let random_string: String = rand::rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    format!("stream_{}", random_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_stream_key() {
        let key1 = generate_stream_key();
        let key2 = generate_stream_key();

        assert!(key1.starts_with("stream_"));
        assert!(key2.starts_with("stream_"));
        assert_ne!(key1, key2);
        assert_eq!(key1.len(), 39); // "stream_" + 32 chars
    }

    #[test]
    fn play_activity_progress_stays_below_complete_until_server_confirmation() {
        assert_eq!(play_activity_progress_pct(450.0, 900, false), 50.0);
        assert_eq!(play_activity_progress_pct(900.0, 900, false), 99.0);
        assert_eq!(play_activity_progress_pct(900.0, 900, true), 100.0);
    }

    #[test]
    fn play_activity_target_uses_server_seconds_or_completion() {
        assert!(!play_activity_reached_target(
            PlayActivityHeartbeatStatus {
                progress_seconds: 899.0,
                completed: false,
            },
            900,
        ));
        assert!(play_activity_reached_target(
            PlayActivityHeartbeatStatus {
                progress_seconds: 900.0,
                completed: false,
            },
            900,
        ));
    }
}
