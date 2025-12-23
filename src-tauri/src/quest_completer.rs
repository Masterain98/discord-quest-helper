use crate::discord_api::DiscordApiClient;
use anyhow::Result;
use rand::Rng;
use std::time::Duration;
use tauri::Emitter;
use tokio::time::sleep;

/// Complete a video quest
/// 
/// Simulates watching a video by incrementally sending video progress
/// Based on power0matin's approach: POST { timestamp: seconds } to /quests/{id}/video-progress
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
    let speed = speed_multiplier;
    // Interval: how often to send updates (in real seconds)
    let interval = heartbeat_interval;
    
    // Convert initial progress (percentage) to seconds
    let mut current_seconds = (initial_progress / 100.0 * seconds_needed as f64) as f64;
    
    println!("Starting video quest: quest_id={}, target={}s, current_progress={:.1}s", 
             quest_id, seconds_needed, current_seconds);
    
    loop {
        // Check cancel signal
        if cancel_rx.try_recv().is_ok() {
            println!("Video quest cancelled");
            let _ = app_handle.emit("quest-stopped", ());
            return Ok(());
        }
        
        // Advance timestamp based on speed and interval
        // e.g. if speed is 1x and interval is 3s, we should advance 3s
        current_seconds += speed * (interval as f64);
        let timestamp = current_seconds.min(seconds_needed as f64);
        
        // Add some randomness to look more natural
        let timestamp_with_jitter = timestamp + rand::thread_rng().gen_range(0.0..0.5);
        
        // Send progress update
        match client.update_video_progress(&quest_id, timestamp_with_jitter).await {
            Ok(completed) => {
                // Calculate and emit progress percentage
                let progress = (timestamp / seconds_needed as f64 * 100.0).min(100.0);
                let _ = app_handle.emit("quest-progress", progress);
                
                println!("Video quest progress: {:.1}% ({:.0}/{} s)", progress, timestamp, seconds_needed);
                
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
        
        // Wait before next update
        tokio::select! {
            _ = sleep(Duration::from_secs(interval)) => {},
            _ = cancel_rx.recv() => {
                println!("Video quest cancelled");
                let _ = app_handle.emit("quest-stopped", ());
                return Ok(());
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
    let total_heartbeats = (seconds_needed + heartbeat_interval - 1) / heartbeat_interval;
    
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
    
    let total_heartbeats = (seconds_needed as u64 + HEARTBEAT_INTERVAL - 1) / HEARTBEAT_INTERVAL;
    
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
        match client.send_game_heartbeat(&quest_id, &application_id, false).await {
            Ok(completed) => {
                // Calculate and send progress percentage
                let progress = ((i + 1) as f64 / total_heartbeats as f64) * 100.0;
                let _ = app_handle.emit("quest-progress", progress);
                
                println!("Game quest progress: {:.1}% (heartbeat {}/{})", progress, i + 1, total_heartbeats);

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

#[allow(dead_code)]
fn generate_stream_key() -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;

    let random_string: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
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
}
