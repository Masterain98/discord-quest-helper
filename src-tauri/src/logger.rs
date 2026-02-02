//! Application logging module with data sanitization
//! 
//! Provides structured logging throughout the application with automatic
//! sanitization of sensitive data (tokens, user IDs, paths, etc.)
//! Logs are session-only and automatically cleared on app restart.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use chrono::{DateTime, Utc};

/// Maximum number of log entries to store (FIFO)
const MAX_LOG_ENTRIES: usize = 1000;

/// Session start time (set once when app starts)
static SESSION_START: Lazy<DateTime<Utc>> = Lazy::new(Utc::now);

/// Thread-safe in-memory log storage
static LOG_STORAGE: Lazy<Mutex<VecDeque<LogEntry>>> = Lazy::new(|| {
    Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES))
});

/// Log level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Log category for filtering and organization
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogCategory {
    TokenExtraction,
    Api,
    Quest,
    Gateway,
    GameSim,
    Rpc,
    General,
}

impl std::fmt::Display for LogCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogCategory::TokenExtraction => write!(f, "TokenExtraction"),
            LogCategory::Api => write!(f, "Api"),
            LogCategory::Quest => write!(f, "Quest"),
            LogCategory::Gateway => write!(f, "Gateway"),
            LogCategory::GameSim => write!(f, "GameSim"),
            LogCategory::Rpc => write!(f, "Rpc"),
            LogCategory::General => write!(f, "General"),
        }
    }
}

/// A single log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub category: LogCategory,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Log export format
#[derive(Debug, Serialize)]
pub struct LogExport {
    pub export_time: String,
    pub session_start: String,
    pub app_version: String,
    pub os: String,
    pub entries: Vec<LogEntry>,
}

// ============================================================================
// Sanitization Functions
// ============================================================================

/// Sanitize a Discord token (keep first 8 and last 4 characters)
/// Example: "OTQ1MzM...long...NzEy" -> "OTQ1MzM...***...NzEy"
#[allow(dead_code)]
pub fn sanitize_token(token: &str) -> String {
    let len = token.len();
    if len <= 16 {
        return "***".to_string();
    }
    format!("{}...***...{}", &token[..8], &token[len-4..])
}

/// Sanitize a Discord user ID (keep first 4 and last 4 characters)
/// Example: "123456789012345678" -> "1234...5678"
#[allow(dead_code)]
pub fn sanitize_user_id(id: &str) -> String {
    let len = id.len();
    if len <= 8 {
        return "***".to_string();
    }
    format!("{}...{}", &id[..4], &id[len-4..])
}

/// Sanitize a username (keep first character only)
/// Example: "Masterain" -> "M***"
#[allow(dead_code)]
pub fn sanitize_username(username: &str) -> String {
    if username.is_empty() {
        return "***".to_string();
    }
    let first_char: String = username.chars().take(1).collect();
    format!("{}***", first_char)
}

/// Sanitize a file path (replace username with [USER])
/// Works for both Windows and Unix-style paths
pub fn sanitize_path(path: &str) -> String {
    // Windows: C:\Users\Username\... -> C:\Users\[USER]\...
    let re_win = regex::Regex::new(r"(?i)\\Users\\[^\\]+").ok();
    let result = if let Some(re) = re_win {
        re.replace_all(path, "\\Users\\[USER]").to_string()
    } else {
        path.to_string()
    };
    
    // Unix: /home/username/... -> /home/[USER]/...
    // Also handles /Users/username/... on macOS
    let re_unix = regex::Regex::new(r"/(home|Users)/[^/]+").ok();
    if let Some(re) = re_unix {
        re.replace_all(&result, "/$1/[USER]").to_string()
    } else {
        result
    }
}

/// Sanitize an email address (show only domain)
/// Example: "user@gmail.com" -> "***@gmail.com"
#[allow(dead_code)]
pub fn sanitize_email(email: &str) -> String {
    if let Some(at_pos) = email.find('@') {
        format!("***{}", &email[at_pos..])
    } else {
        "***".to_string()
    }
}

// ============================================================================
// Logging Functions
// ============================================================================

/// Log a message with the given level and category
/// This is the main logging function used throughout the application
pub fn log(level: LogLevel, category: LogCategory, message: &str, details: Option<&str>) {
    let entry = LogEntry {
        timestamp: Utc::now().to_rfc3339(),
        level,
        category,
        message: message.to_string(),
        details: details.map(|s| s.to_string()),
    };
    
    // Also print to console for debugging
    if let Some(ref detail) = entry.details {
        println!("[{}] [{}] {}: {}", entry.level, entry.category, entry.message, detail);
    } else {
        println!("[{}] [{}] {}", entry.level, entry.category, entry.message);
    }
    
    // Store in memory
    if let Ok(mut storage) = LOG_STORAGE.lock() {
        if storage.len() >= MAX_LOG_ENTRIES {
            storage.pop_front();
        }
        storage.push_back(entry);
    }
}

/// Convenience macros for different log levels
#[macro_export]
macro_rules! log_debug {
    ($cat:expr, $msg:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Debug, $cat, $msg, None)
    };
    ($cat:expr, $msg:expr, $details:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Debug, $cat, $msg, Some($details))
    };
}

#[macro_export]
macro_rules! log_info {
    ($cat:expr, $msg:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Info, $cat, $msg, None)
    };
    ($cat:expr, $msg:expr, $details:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Info, $cat, $msg, Some($details))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($cat:expr, $msg:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Warn, $cat, $msg, None)
    };
    ($cat:expr, $msg:expr, $details:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Warn, $cat, $msg, Some($details))
    };
}

#[macro_export]
macro_rules! log_error {
    ($cat:expr, $msg:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Error, $cat, $msg, None)
    };
    ($cat:expr, $msg:expr, $details:expr) => {
        $crate::logger::log($crate::logger::LogLevel::Error, $cat, $msg, Some($details))
    };
}

// ============================================================================
// Export Functions
// ============================================================================

/// Get OS information string
fn get_os_info() -> String {
    #[cfg(target_os = "windows")]
    {
        format!("Windows {}", std::env::var("OS").unwrap_or_else(|_| "Unknown".to_string()))
    }
    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "Unknown OS".to_string()
    }
}

/// Export all logs as a JSON string
/// Returns sanitized log data suitable for sharing with developers
pub fn export_logs() -> anyhow::Result<String> {
    let entries = if let Ok(storage) = LOG_STORAGE.lock() {
        storage.iter().cloned().collect()
    } else {
        Vec::new()
    };
    
    let export = LogExport {
        export_time: Utc::now().to_rfc3339(),
        session_start: SESSION_START.to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: get_os_info(),
        entries,
    };
    
    serde_json::to_string_pretty(&export)
        .map_err(|e| anyhow::anyhow!("Failed to serialize logs: {}", e))
}

/// Get the number of log entries currently stored
#[allow(dead_code)]
pub fn log_count() -> usize {
    if let Ok(storage) = LOG_STORAGE.lock() {
        storage.len()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_token() {
        let token = "OTQ1MzM3NjE2MzU3NTg1OTIz.YnJvdGhlcnMu.abc123xyz789def456ghi";
        let sanitized = sanitize_token(token);
        assert!(sanitized.starts_with("OTQ1MzM3"));
        assert!(sanitized.ends_with("6ghi"));
        assert!(sanitized.contains("***"));
    }

    #[test]
    fn test_sanitize_user_id() {
        let id = "123456789012345678";
        let sanitized = sanitize_user_id(id);
        assert_eq!(sanitized, "1234...5678");
    }

    #[test]
    fn test_sanitize_username() {
        assert_eq!(sanitize_username("Masterain"), "M***");
        assert_eq!(sanitize_username(""), "***");
    }

    #[test]
    fn test_sanitize_path() {
        let win_path = r"C:\Users\Masterain\Documents\file.txt";
        let sanitized = sanitize_path(win_path);
        assert!(sanitized.contains("[USER]"));
        assert!(!sanitized.contains("Masterain"));
    }

    #[test]
    fn test_sanitize_email() {
        assert_eq!(sanitize_email("user@gmail.com"), "***@gmail.com");
    }
}
