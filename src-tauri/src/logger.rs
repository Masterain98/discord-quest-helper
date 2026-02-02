//! Application logging module with data sanitization
//! 
//! Provides structured logging throughout the application with automatic
//! sanitization of sensitive data (tokens, user IDs, paths, etc.)
//! Logs are session-only and automatically cleared on app restart.

use once_cell::sync::Lazy;
use regex::Regex;
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

// Pre-compiled regex patterns for path sanitization
static PATH_REGEX_WIN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\\Users\\[^\\]+").expect("Invalid Windows path regex")
});
static PATH_REGEX_UNIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"/(home|Users)/[^/]+").expect("Invalid Unix path regex")
});

/// Sanitize a file path (replace username with [USER])
/// Works for both Windows and Unix-style paths
pub fn sanitize_path(path: &str) -> String {
    // Windows: C:\Users\Username\... -> C:\Users\[USER]\...
    let result = PATH_REGEX_WIN.replace_all(path, "\\Users\\[USER]").to_string();
    
    // Unix: /home/username/... -> /home/[USER]/...
    // Also handles /Users/username/... on macOS
    PATH_REGEX_UNIX.replace_all(&result, "/$1/[USER]").to_string()
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

// Pre-compiled regex patterns for message sanitization
static TOKEN_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Match Discord token patterns (base64-like strings of significant length)
    Regex::new(r"[A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,}").expect("Invalid token regex")
});
static USER_ID_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Match Discord user IDs (17-19 digit numbers)
    Regex::new(r"\b\d{17,19}\b").expect("Invalid user ID regex")
});

/// Sanitize a message string by removing/masking sensitive patterns
fn sanitize_message(message: &str) -> String {
    // Apply path sanitization
    let result = sanitize_path(message);
    
    // Mask any Discord tokens
    let result = TOKEN_REGEX.replace_all(&result, "[TOKEN]").to_string();
    
    // Mask Discord user IDs
    USER_ID_REGEX.replace_all(&result, "[USER_ID]").to_string()
}

/// Log a message with the given level and category
/// Messages and details are automatically sanitized before storage
pub fn log(level: LogLevel, category: LogCategory, message: &str, details: Option<&str>) {
    // Force SESSION_START initialization on first log call
    // This ensures session_start reflects app startup, not export time
    let _ = *SESSION_START;
    
    // Sanitize message and details before storing
    let sanitized_message = sanitize_message(message);
    let sanitized_details = details.map(|s| sanitize_message(s));
    
    let entry = LogEntry {
        timestamp: Utc::now().to_rfc3339(),
        level,
        category,
        message: sanitized_message,
        details: sanitized_details,
    };
    
    // Also print to console for debugging (already sanitized)
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

/// Get OS information string with version details
fn get_os_info() -> String {
    #[cfg(target_os = "windows")]
    {
        // Try to get Windows build number from registry or environment
        // The "OS" env var gives Windows_NT, but we want the actual build
        if let Ok(output) = std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .output()
        {
            let version_output = String::from_utf8_lossy(&output.stdout);
            // Parse "Microsoft Windows [Version 10.0.22631.4751]" format
            if let Some(start) = version_output.find('[') {
                if let Some(end) = version_output.find(']') {
                    let version_part = &version_output[start + 1..end];
                    return format!("Windows ({})", version_part.trim());
                }
            }
        }
        "Windows".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        // Get macOS version using sw_vers command
        if let Ok(output) = std::process::Command::new("sw_vers")
            .args(["-productVersion"])
            .output()
        {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim();
            if !version.is_empty() {
                return format!("macOS {}", version);
            }
        }
        "macOS".to_string()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Get Linux distribution info if available
        if let Ok(output) = std::process::Command::new("uname")
            .args(["-sr"])
            .output()
        {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim();
            if !version.is_empty() {
                return format!("Linux ({})", version);
            }
        }
        "Linux".to_string()
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
