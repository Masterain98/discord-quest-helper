//! Translation provider module
//!
//! Translates Discord message text through configurable external providers
//! (Google Translate keyless, Microsoft Translator, DeepL) and runs
//! composer-translation / channel-summary prompts through an
//! OpenAI-compatible LLM endpoint. Message text is made markdown-aware
//! before it leaves the machine: code fences, inline code, links and line
//! prefixes are protected so providers cannot mangle Discord formatting.
//!
//! Provider API keys are stored in the OS-native secret store (Windows
//! Credential Manager / macOS Keychain / Linux Secret Service). When the
//! platform secret store is unavailable the keys fall back to a plaintext
//! file in the app data directory and the fallback is logged, never silent.
//!
//! Provider calls never touch the Discord user token and never run inside
//! the Discord renderer, so a provider failure cannot disturb the CDP
//! connection or the quest pipeline.

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::logger::{log, LogCategory, LogLevel};

/// Maximum entries kept in the persistent translation cache.
const MAX_TRANSLATION_CACHE_ENTRIES: usize = 120;
/// How long a cached translation stays valid.
const TRANSLATION_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
/// Version of the on-disk cache format.
const TRANSLATION_CACHE_VERSION: u32 = 1;
/// Maximum concurrent provider HTTP calls.
const MAX_PROVIDER_CONCURRENCY: usize = 2;
/// Hard cap on the text length accepted per translation request.
const MAX_TRANSLATION_INPUT_CHARS: usize = 4_000;
/// Hard cap on the text length accepted per LLM request.
const MAX_LLM_INPUT_CHARS: usize = 24_000;

const KEYRING_SERVICE: &str = "discord-quest-helper";
/// Persistent translation cache file name, resolved under the app data dir.
pub const TRANSLATION_CACHE_FILE: &str = "translation-cache.json";
const TRANSLATION_KEYS_FALLBACK_FILE: &str = "translation-keys.json";

/// Where a provider API key was actually persisted. Only used for logging;
/// the frontend just sees "a key is set".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStorage {
    OsSecretStore,
    FallbackFile,
}

/// Identifies which secret a keyring/fallback operation touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKeyRef {
    Microsoft,
    DeepL,
    Llm,
}

impl ProviderKeyRef {
    fn account(self) -> &'static str {
        match self {
            ProviderKeyRef::Microsoft => "translation.microsoft",
            ProviderKeyRef::DeepL => "translation.deepl",
            ProviderKeyRef::Llm => "translation.llm",
        }
    }
}

/// Message translation providers, mirroring the reference extension ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranslationProvider {
    GoogleTranslate,
    MicrosoftTranslator,
    DeepL,
}

impl TranslationProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            TranslationProvider::GoogleTranslate => "google-translate",
            TranslationProvider::MicrosoftTranslator => "microsoft-translator",
            TranslationProvider::DeepL => "deepl",
        }
    }
}

/// OpenAI-compatible endpoint used for composer translation and summaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    pub base_url: String,
    pub model: String,
}

/// Localized labels handed to the injected Discord JS so buttons and panels
/// speak the user's app language. `{count}` in `summarize`/`summary_subtitle`
/// is replaced with the configured message count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationLabels {
    pub translate: String,
    pub translating: String,
    pub summarize: String,
    pub summarizing: String,
    pub summary_title: String,
    pub summary_subtitle: String,
    pub copy: String,
    pub copied: String,
    pub close: String,
    pub error_prefix: String,
    pub composer_empty: String,
    pub llm_missing: String,
}

impl Default for TranslationLabels {
    fn default() -> Self {
        Self {
            translate: "Translate".to_string(),
            translating: "Translating...".to_string(),
            summarize: "Summarize {count}".to_string(),
            summarizing: "Summarizing...".to_string(),
            summary_title: "Channel Summary".to_string(),
            summary_subtitle: "Based on the last {count} messages".to_string(),
            copy: "Copy".to_string(),
            copied: "Copied".to_string(),
            close: "\u{00d7}".to_string(),
            error_prefix: "Translation error".to_string(),
            composer_empty: "Write something in the composer before translating.".to_string(),
            llm_missing: "Configure an LLM endpoint in the app settings to use this.".to_string(),
        }
    }
}

/// Full runtime configuration for the translation feature, passed from the
/// frontend on every start (the app keeps settings client-side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationConfig {
    pub provider: TranslationProvider,
    /// BCP-47-ish target language tag ("pt-BR", "en", "auto" resolved by the frontend).
    pub target_language: String,
    pub auto_translate: bool,
    /// "below" renders original + translation, "replace" hides the original.
    pub display_mode: TranslationDisplayMode,
    /// CSS color for the rendered translation text.
    pub accent_color: String,
    /// Message count for channel summaries (20 or 50).
    pub summary_count: u32,
    pub microsoft_region: Option<String>,
    pub llm: Option<LlmConfig>,
    pub labels: TranslationLabels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationDisplayMode {
    Below,
    Replace,
}

/// One unit of work delivered from the injected Discord JS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationJob {
    pub id: String,
    pub kind: TranslationJobKind,
    pub text: String,
    /// Identity of the message in Discord's DOM (`chat-messages-...`).
    pub stable_id: String,
    pub target_language: String,
    /// Channel context for composer translation and summaries.
    pub channel_name: Option<String>,
    pub channel_topic: Option<String>,
    pub recent_messages: Option<String>,
    /// Discord channel id, only set for summary jobs (REST fetch source).
    pub channel_id: Option<String>,
    pub message_count: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationJobKind {
    Message,
    Composer,
    Summary,
}

/// Result of one processed job, delivered back into the Discord page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationJobResult {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TranslationJobKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntry {
    translated_text: String,
    created_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCache {
    version: u32,
    entries: HashMap<String, CacheEntry>,
}

/// Persistent translation cache backed by a JSON file in the app data dir.
pub struct TranslationCache {
    path: PathBuf,
    inner: tokio::sync::Mutex<Option<PersistedCache>>,
}

/// Shared HTTP plumbing for provider calls.
pub struct TranslationHttp {
    client: reqwest::Client,
    permits: Arc<Semaphore>,
}

impl TranslationHttp {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .build()
            .context("Could not create translation HTTP client")?;
        Ok(Self {
            client,
            permits: Arc::new(Semaphore::new(MAX_PROVIDER_CONCURRENCY)),
        })
    }

    async fn send_with_retry_after(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let response = build().send().await?;
        if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(response);
        }
        let retry_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<f64>().ok())
            .unwrap_or(1.0)
            .clamp(0.0, 30.0);
        log(
            LogLevel::Warn,
            LogCategory::Translation,
            &format!("Translation provider rate limited; retrying in {retry_secs:.1}s"),
            None,
        );
        tokio::time::sleep(Duration::from_secs_f64(retry_secs)).await;
        Ok(build().send().await?)
    }
}

impl Default for TranslationHttp {
    fn default() -> Self {
        Self::new().expect("translation HTTP client construction cannot fail with static config")
    }
}

// ---------------------------------------------------------------------------
// Cache key
// ---------------------------------------------------------------------------

/// Mirrors the reference `hashString`: 32-bit `(hash << 5) - hash + unit`
/// over UTF-16 code units, rendered in base36. Kept byte-compatible with the
/// extension so cache semantics stay identical across the two projects.
pub fn hash_string(value: &str) -> String {
    let mut hash: i32 = 0;
    for unit in value.encode_utf16() {
        hash = hash
            .wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(i32::from(unit));
    }
    let magnitude = (hash as i64).abs();
    to_base36(magnitude as u64)
}

fn to_base36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut buffer = [0u8; 13];
    let mut length = 0;
    while value > 0 {
        buffer[length] = DIGITS[(value % 36) as usize];
        value /= 36;
        length += 1;
    }
    buffer[..length]
        .iter()
        .rev()
        .map(|byte| *byte as char)
        .collect()
}

/// Message-identity key: `<stableId>:<sourceHash>`, or the bare hash when the
/// message has no stable DOM id.
pub fn build_request_key(stable_message_id: &str, source_text: &str) -> String {
    let text_hash = hash_string(source_text);
    if stable_message_id.is_empty() {
        text_hash
    } else {
        format!("{}:{}", stable_message_id, text_hash)
    }
}

/// Persistent-cache key: provider + target language + message identity.
pub fn build_scoped_cache_key(
    provider: TranslationProvider,
    target_language: &str,
    stable_message_id: &str,
    source_text: &str,
) -> String {
    format!(
        "{}:{}:{}",
        provider.as_str(),
        target_language,
        build_request_key(stable_message_id, source_text)
    )
}

// ---------------------------------------------------------------------------
// Markdown-aware translation
// ---------------------------------------------------------------------------

struct ProtectedToken {
    placeholder: String,
    value: String,
}

fn protect_markdown_syntax(source: &str) -> (String, Vec<ProtectedToken>) {
    let mut tokens: Vec<ProtectedToken> = Vec::new();
    let mut text = source.to_string();
    let patterns = [
        r#"```[\s\S]*?```"#,
        r"`[^`\n]+`",
        r"\[[^\]]+\]\(https?://[^\s)]+\)",
        r"https?://[^\s)]+",
    ];
    for pattern in patterns {
        let Ok(re) = Regex::new(pattern) else {
            continue;
        };
        text = re
            .replace_all(&text, |captures: &regex::Captures| {
                let placeholder =
                    format!("\u{00a7}\u{00a7}DQHMDTOKEN{}\u{00a7}\u{00a7}", tokens.len());
                tokens.push(ProtectedToken {
                    placeholder: placeholder.clone(),
                    value: captures[0].to_string(),
                });
                placeholder
            })
            .into_owned();
    }
    (text, tokens)
}

fn restore_markdown_syntax(source: &str, tokens: &[ProtectedToken]) -> String {
    let mut result = source.to_string();
    for token in tokens {
        if result.contains(&token.placeholder) {
            result = result.replace(&token.placeholder, &token.value);
        }
    }
    result
}

/// Splits list/quote/heading prefixes so only the actual content line is
/// translated.
fn split_markdown_line_prefix(line: &str) -> (&str, &str) {
    let indent_length = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_length);
    // Longest prefixes first so "## " wins over "# " and ">>> " over "> ".
    const PREFIXES: [&str; 8] = [">>> ", ">>>", "> ", "### ", "## ", "# ", "- ", "* "];
    for prefix in PREFIXES {
        if let Some(content) = rest.strip_prefix(prefix) {
            let consumed = line.len() - content.len();
            return (&line[..consumed], content);
        }
    }
    // Numbered lists: "12. content"
    if let Some(dot_position) = rest.find(". ") {
        let digits = &rest[..dot_position];
        if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
            let consumed = indent_length + dot_position + 2;
            return (&line[..consumed], &line[consumed..]);
        }
    }
    (indent, rest)
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

fn normalize_target_language(language: &str) -> String {
    let trimmed = language.trim().to_lowercase();
    if trimmed.is_empty() || trimmed == "auto" {
        "en".to_string()
    } else {
        trimmed
    }
}

fn normalize_deepl_language(language: &str) -> String {
    match language {
        "pt" => "PT-BR".to_string(),
        "en" => "EN-US".to_string(),
        other => other.to_uppercase(),
    }
}

impl TranslationHttp {
    async fn translate_with_google(&self, text: &str, target_language: &str) -> Result<String> {
        let url = url::Url::parse_with_params(
            "https://translate.googleapis.com/translate_a/single",
            &[
                ("client", "gtx"),
                ("sl", "auto"),
                ("tl", target_language),
                ("dt", "t"),
                ("q", text),
            ],
        )?;
        let response = self
            .send_with_retry_after(|| self.client.get(url.as_str()))
            .await?;
        let status = response.status();
        let payload: serde_json::Value = response
            .json()
            .await
            .with_context(|| format!("Google Translate returned invalid JSON (status {status})"))?;
        let segments = payload
            .get(0)
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow!("Google Translate returned an unexpected payload shape"))?;
        let mut translated = String::new();
        for segment in segments {
            if let Some(piece) = segment.get(0).and_then(|value| value.as_str()) {
                translated.push_str(piece);
            }
        }
        if translated.trim().is_empty() {
            return Err(anyhow!("Google Translate returned an empty response"));
        }
        Ok(translated.trim().to_string())
    }

    async fn translate_with_microsoft(
        &self,
        text: &str,
        target_language: &str,
        api_key: &str,
        region: &str,
    ) -> Result<String> {
        if api_key.is_empty() {
            return Err(anyhow!("Microsoft Translator key is not configured"));
        }
        if region.is_empty() {
            return Err(anyhow!("Microsoft Translator region is not configured"));
        }
        let endpoint = format!(
            "https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to={}",
            url::form_urlencoded::byte_serialize(target_language.as_bytes()).collect::<String>()
        );
        let response = self
            .send_with_retry_after(|| {
                self.client
                    .post(&endpoint)
                    .header("Ocp-Apim-Subscription-Key", api_key)
                    .header("Ocp-Apim-Subscription-Region", region)
                    .json(&serde_json::json!([{ "text": text }]))
            })
            .await?;
        let status = response.status();
        let payload: serde_json::Value = response.json().await.unwrap_or_else(|_| {
            serde_json::json!({ "error": { "message": format!("Microsoft Translator returned status {status}") } })
        });
        if let Some(translated) = payload
            .get("translations")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("text"))
            .and_then(|value| value.as_str())
        {
            if !translated.trim().is_empty() {
                return Ok(translated.trim().to_string());
            }
        }
        let message = payload
            .pointer("/error/message")
            .and_then(|value| value.as_str())
            .unwrap_or("Microsoft Translator request failed");
        Err(anyhow!("{}", message))
    }

    async fn translate_with_deepl(
        &self,
        text: &str,
        target_language: &str,
        api_key: &str,
    ) -> Result<String> {
        if api_key.is_empty() {
            return Err(anyhow!("DeepL key is not configured"));
        }
        let target_lang = normalize_deepl_language(target_language);
        let form_body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("text", text)
            .append_pair("target_lang", &target_lang)
            .finish();
        for endpoint in [
            "https://api-free.deepl.com/v2/translate",
            "https://api.deepl.com/v2/translate",
        ] {
            let response = self
                .send_with_retry_after(|| {
                    self.client
                        .post(endpoint)
                        .header("Authorization", format!("DeepL-Auth-Key {api_key}"))
                        .header(
                            reqwest::header::CONTENT_TYPE,
                            "application/x-www-form-urlencoded",
                        )
                        .body(form_body.clone())
                })
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    log(
                        LogLevel::Debug,
                        LogCategory::Translation,
                        &format!("DeepL endpoint {endpoint} failed: {error:#}"),
                        None,
                    );
                    continue;
                }
            };
            let status = response.status();
            let payload: serde_json::Value = match response.json().await {
                Ok(payload) => payload,
                Err(_) => continue,
            };
            if let Some(translated) = payload
                .get("translations")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("text"))
                .and_then(|value| value.as_str())
            {
                if !translated.trim().is_empty() {
                    return Ok(translated.trim().to_string());
                }
            }
            if status.is_success() {
                return Err(anyhow!("DeepL returned an empty response"));
            }
        }
        Err(anyhow!("DeepL request failed on both API hosts"))
    }

    /// Single-source-of-truth text translation for one line of content.
    async fn translate_line(
        &self,
        provider: TranslationProvider,
        text: &str,
        target_language: &str,
        credentials: &ProviderCredentials,
    ) -> Result<String> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|error| anyhow!("Translation semaphore closed: {error}"))?;
        match provider {
            TranslationProvider::GoogleTranslate => {
                self.translate_with_google(text, target_language).await
            }
            TranslationProvider::MicrosoftTranslator => {
                self.translate_with_microsoft(
                    text,
                    target_language,
                    &credentials.microsoft_key,
                    &credentials.microsoft_region,
                )
                .await
            }
            TranslationProvider::DeepL => {
                self.translate_with_deepl(text, target_language, &credentials.deepl_key)
                    .await
            }
        }
    }

    /// Markdown-aware translation: protect syntax, translate line by line,
    /// restore syntax. Multi-line text issues one provider call per
    /// non-empty content line, matching the reference behavior.
    pub async fn translate_markdown_aware(
        &self,
        provider: TranslationProvider,
        text: &str,
        target_language: &str,
        credentials: &ProviderCredentials,
    ) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(anyhow!("No text received for translation"));
        }
        if text.chars().count() > MAX_TRANSLATION_INPUT_CHARS {
            return Err(anyhow!(
                "Message exceeds the {} character translation limit",
                MAX_TRANSLATION_INPUT_CHARS
            ));
        }
        let target_language = normalize_target_language(target_language);
        let (protected, tokens) = protect_markdown_syntax(text);
        let mut translated_lines: Vec<String> = Vec::new();
        for line in protected.split('\n') {
            if line.trim().is_empty() {
                translated_lines.push(String::new());
                continue;
            }
            let (prefix, content) = split_markdown_line_prefix(line);
            if content.trim().is_empty() {
                translated_lines.push(line.to_string());
                continue;
            }
            let translated_line = self
                .translate_line(provider, content, &target_language, credentials)
                .await?;
            translated_lines.push(format!("{}{}", prefix, translated_line));
        }
        Ok(restore_markdown_syntax(
            &translated_lines.join("\n"),
            &tokens,
        ))
    }

    /// Runs an OpenAI-compatible chat completion.
    pub async fn llm_complete(
        &self,
        llm: &LlmConfig,
        api_key: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        if api_key.is_empty() {
            return Err(anyhow!("LLM API key is not configured"));
        }
        let base_url = validate_llm_base_url(&llm.base_url)?;
        if llm.model.trim().is_empty() {
            return Err(anyhow!("LLM model is not configured"));
        }
        if user_prompt.chars().count() > MAX_LLM_INPUT_CHARS {
            return Err(anyhow!(
                "Input exceeds the {} character LLM limit",
                MAX_LLM_INPUT_CHARS
            ));
        }
        let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|error| anyhow!("Translation semaphore closed: {error}"))?;
        let response = self
            .send_with_retry_after(|| {
                self.client
                    .post(&endpoint)
                    .bearer_auth(api_key)
                    .json(&serde_json::json!({
                        "model": llm.model.trim(),
                        "temperature": 0.2,
                        "stream": false,
                        "messages": [
                            { "role": "system", "content": system_prompt },
                            { "role": "user", "content": user_prompt }
                        ]
                    }))
            })
            .await?;
        let status = response.status();
        let payload: serde_json::Value = response
            .json()
            .await
            .with_context(|| format!("LLM endpoint returned invalid JSON (status {status})"))?;
        if let Some(message) = payload
            .pointer("/error/message")
            .and_then(|value| value.as_str())
        {
            return Err(anyhow!("LLM provider error: {}", message));
        }
        let content = payload
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("message"))
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if content.trim().is_empty() {
            return Err(anyhow!("LLM endpoint returned an empty completion"));
        }
        Ok(content.trim().to_string())
    }
}

/// Accepts https:// endpoints always; plain http:// only for localhost, so a
/// mistyped URL cannot ship message content off-machine in the clear.
pub fn validate_llm_base_url(base_url: &str) -> Result<String> {
    let trimmed = base_url.trim();
    let parsed = url::Url::parse(trimmed).context("LLM base URL is not a valid URL")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("LLM base URL has no host"))?
        .to_lowercase();
    let is_local = host == "localhost" || host == "127.0.0.1" || host == "[::1]";
    match parsed.scheme() {
        "https" => Ok(trimmed.trim_end_matches('/').to_string()),
        "http" if is_local => Ok(trimmed.trim_end_matches('/').to_string()),
        _ => Err(anyhow!(
            "LLM base URL must use https:// (http:// is only allowed for localhost)"
        )),
    }
}

// ---------------------------------------------------------------------------
// Prompts (ported from the reference extension)
// ---------------------------------------------------------------------------

pub fn composer_translation_system_prompt() -> String {
    [
        "Translation Mode: Your only job is to translate the existing draft into the requested target language.",
        "Translate faithfully: preserve the exact meaning, intent, register, and structure of every sentence. Only the language changes.",
        "Do not answer the draft. Do not continue the conversation. Do not roleplay as the sender or recipient.",
        "Do not rewrite, improve, or rephrase the draft. Do not add new ideas.",
        "Do not add hashtags, calls to action, signatures, explanations, labels, or extra commentary.",
        "Do not invent mentions, emojis, links, or formatting that were not already present.",
        "Preserve the original intent, meaning, register, tone, mentions, emojis, formatting, markdown, and line breaks exactly.",
        "Return only the final translated message text.",
    ]
    .join("\n")
}

pub fn composer_translation_user_prompt(
    draft: &str,
    target_language: &str,
    channel_name: Option<&str>,
    channel_topic: Option<&str>,
    recent_messages: Option<&str>,
) -> String {
    [
        channel_name.map(|name| format!("Channel: #{name}")),
        channel_topic.map(|topic| format!("Channel topic: {topic}")),
        recent_messages.map(|recent| format!("Recent conversation:\n{recent}")),
        Some(format!("Target language: {target_language}")),
        Some(format!("Current draft:\n{draft}")),
        Some(
            "Translate this draft into the target language exactly as written. Preserve the meaning, intent, mentions, emojis, formatting, and nuance of every sentence; do not add, remove or change ideas.".to_string(),
        ),
        Some("Make the result sound native and natural in the target language, not overly literal.".to_string()),
        Some("Do not answer the draft or add new content beyond the translation.".to_string()),
        Some("Return only the translated message text. No JSON, no quotes, no explanations.".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n\n")
}

pub fn channel_summary_system_prompt() -> String {
    [
        "You summarize Discord channel context for another assistant.",
        "Return only plain text, no JSON, no markdown bullets unless necessary.",
        "Keep the summary compact, high-signal, and focused on what a reply writer must know.",
    ]
    .join(" ")
}

pub fn channel_summary_user_prompt(
    messages: &str,
    limit: u32,
    channel_name: Option<&str>,
    channel_topic: Option<&str>,
) -> String {
    [
        channel_name.map(|name| format!("Channel: #{name}")),
        channel_topic.map(|topic| format!("Channel topic: {topic}")),
        Some(format!(
            "Summarize these last {limit} Discord messages before a reply is written."
        )),
        Some(
            "Capture the main topic, recent turns, open questions, sentiment, and anything the responder should avoid repeating.".to_string(),
        ),
        Some("Keep it under 6 short lines.".to_string()),
        Some(String::new()),
        Some(messages.to_string()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n")
}

// ---------------------------------------------------------------------------
// Persistent cache
// ---------------------------------------------------------------------------

impl TranslationCache {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            inner: tokio::sync::Mutex::new(None),
        }
    }

    async fn ensure_loaded(&self) -> tokio::sync::MutexGuard<'_, Option<PersistedCache>> {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            let persisted = load_cache_from_disk(&self.path);
            *guard = Some(persisted);
        }
        guard
    }

    /// Looks up `key`, translating and persisting on miss. Returns the
    /// translated text plus whether it came from cache.
    pub async fn translate_with_cache(
        &self,
        http: &TranslationHttp,
        provider: TranslationProvider,
        text: &str,
        target_language: &str,
        stable_message_id: &str,
        credentials: &ProviderCredentials,
    ) -> Result<(String, String, bool)> {
        let scoped_key = build_scoped_cache_key(provider, target_language, stable_message_id, text);
        {
            let guard = self.ensure_loaded().await;
            if let Some(cache) = guard.as_ref() {
                if let Some(entry) = cache.entries.get(&scoped_key) {
                    if is_entry_fresh(entry) {
                        return Ok((entry.translated_text.clone(), scoped_key, true));
                    }
                }
            }
        }

        let translated = http
            .translate_markdown_aware(provider, text, target_language, credentials)
            .await?;

        let mut guard = self.ensure_loaded().await;
        if let Some(cache) = guard.as_mut() {
            cache.entries.insert(
                scoped_key.clone(),
                CacheEntry {
                    translated_text: translated.clone(),
                    created_at: chrono::Utc::now().timestamp_millis(),
                },
            );
            normalize_cache_entries(cache);
            let path = self.path.clone();
            let snapshot =
                serde_json::to_vec(&*cache).context("Could not serialize translation cache")?;
            // Blocking file IO off the async runtime; a failed write only
            // costs cache persistence, never the translation result.
            let _ =
                tokio::task::spawn_blocking(move || write_cache_to_disk(&path, &snapshot)).await;
        }
        Ok((translated, scoped_key, false))
    }
}

fn is_entry_fresh(entry: &CacheEntry) -> bool {
    let now = chrono::Utc::now().timestamp_millis();
    entry
        .created_at
        .checked_sub(now)
        .map(|age| age.abs() <= TRANSLATION_CACHE_TTL_MS)
        .unwrap_or(false)
}

fn normalize_cache_entries(cache: &mut PersistedCache) {
    let before = cache.entries.len();
    cache.entries.retain(|key, entry| {
        !key.trim().is_empty() && !entry.translated_text.trim().is_empty() && is_entry_fresh(entry)
    });
    if cache.entries.len() > MAX_TRANSLATION_CACHE_ENTRIES {
        let mut newest: Vec<(String, CacheEntry)> = cache.entries.drain().collect();
        newest.sort_by_key(|entry| std::cmp::Reverse(entry.1.created_at));
        newest.truncate(MAX_TRANSLATION_CACHE_ENTRIES);
        cache.entries = newest.into_iter().collect();
    }
    if before != cache.entries.len() || cache.version != TRANSLATION_CACHE_VERSION {
        cache.version = TRANSLATION_CACHE_VERSION;
    }
}

fn load_cache_from_disk(path: &Path) -> PersistedCache {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return PersistedCache::default(),
    };
    let mut cache: PersistedCache = match serde_json::from_slice(&bytes) {
        Ok(cache) => cache,
        Err(error) => {
            // Preserve the unreadable file for diagnosis, like
            // simulation_history does, and start fresh.
            let quarantine = path.with_extension(format!("json.corrupt-{}", uuid::Uuid::new_v4()));
            if std::fs::rename(path, &quarantine).is_ok() {
                log(
                    LogLevel::Warn,
                    LogCategory::Translation,
                    &format!(
                        "Translation cache at {} was unreadable ({error}); quarantined and restarted empty",
                        path.display()
                    ),
                    None,
                );
            }
            return PersistedCache::default();
        }
    };
    normalize_cache_entries(&mut cache);
    cache
}

fn write_cache_to_disk(path: &Path, snapshot: &[u8]) {
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log(
                LogLevel::Warn,
                LogCategory::Translation,
                &format!(
                    "Could not create cache directory {}: {error}",
                    parent.display()
                ),
                None,
            );
            return;
        }
    }
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, snapshot).is_ok() {
        let _ = std::fs::rename(&temp, path);
    }
}

// ---------------------------------------------------------------------------
// Provider API keys (OS secret store with plaintext fallback)
// ---------------------------------------------------------------------------

fn fallback_keys_path(app_dir: &Path) -> PathBuf {
    app_dir.join(TRANSLATION_KEYS_FALLBACK_FILE)
}

fn read_fallback_keys(app_dir: &Path) -> HashMap<String, String> {
    std::fs::read(fallback_keys_path(app_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_fallback_keys(app_dir: &Path, keys: &HashMap<String, String>) -> Result<()> {
    std::fs::create_dir_all(app_dir)
        .with_context(|| format!("Could not create app data directory {}", app_dir.display()))?;
    let payload = serde_json::to_vec_pretty(keys).context("Could not serialize provider keys")?;
    let path = fallback_keys_path(app_dir);
    std::fs::write(&path, payload).with_context(|| format!("Could not write {}", path.display()))
}

/// Stores a provider API key in the OS secret store, falling back to a
/// plaintext file when the platform store is unavailable. The storage choice
/// is always logged.
pub fn set_provider_key(
    key_ref: ProviderKeyRef,
    api_key: &str,
    app_dir: &Path,
) -> Result<KeyStorage> {
    let outcome = keyring::Entry::new(KEYRING_SERVICE, key_ref.account())
        .and_then(|entry| entry.set_password(api_key));
    match outcome {
        Ok(()) => {
            log(
                LogLevel::Info,
                LogCategory::Translation,
                &format!(
                    "Provider key {} stored in the OS secret store",
                    key_ref.account()
                ),
                None,
            );
            Ok(KeyStorage::OsSecretStore)
        }
        Err(error) => {
            log(
                LogLevel::Warn,
                LogCategory::Translation,
                &format!(
                    "OS secret store unavailable ({}); provider key {} stored in plaintext fallback file",
                    error, key_ref.account()
                ),
                None,
            );
            let mut keys = read_fallback_keys(app_dir);
            keys.insert(key_ref.account().to_string(), api_key.to_string());
            write_fallback_keys(app_dir, &keys)?;
            Ok(KeyStorage::FallbackFile)
        }
    }
}

/// Removes a provider API key from both the OS secret store and the
/// fallback file. Missing keys are treated as already cleared.
pub fn clear_provider_key(key_ref: ProviderKeyRef, app_dir: &Path) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, key_ref.account()) {
        let _ = entry.delete_credential();
    }
    let mut keys = read_fallback_keys(app_dir);
    if keys.remove(key_ref.account()).is_some() {
        let _ = write_fallback_keys(app_dir, &keys);
    }
}

/// Returns true when a key for `key_ref` is set in either store.
pub fn has_provider_key(key_ref: ProviderKeyRef, app_dir: &Path) -> bool {
    load_provider_key(key_ref, app_dir)
        .map(|key| !key.is_empty())
        .unwrap_or(false)
}

/// Reads a provider API key from the OS secret store, falling back to the
/// plaintext file. Empty string means "not configured".
fn load_provider_key(key_ref: ProviderKeyRef, app_dir: &Path) -> Result<String> {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, key_ref.account()) {
        match entry.get_password() {
            Ok(password) if !password.trim().is_empty() => return Ok(password),
            _ => {}
        }
    }
    Ok(read_fallback_keys(app_dir)
        .get(key_ref.account())
        .cloned()
        .unwrap_or_default())
}

/// Resolves every secret a config run needs, in one place.
/// Secrets and region needed by message-translation provider calls.
#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    pub microsoft_key: String,
    pub microsoft_region: String,
    pub deepl_key: String,
}

/// Every secret a config run needs, resolved in one place.
pub struct ResolvedKeys {
    pub credentials: ProviderCredentials,
    pub llm_key: String,
}

pub fn resolve_keys(config: &TranslationConfig, app_dir: &Path) -> ResolvedKeys {
    ResolvedKeys {
        credentials: ProviderCredentials {
            microsoft_key: load_provider_key(ProviderKeyRef::Microsoft, app_dir)
                .unwrap_or_default(),
            microsoft_region: config.microsoft_region.clone().unwrap_or_default(),
            deepl_key: load_provider_key(ProviderKeyRef::DeepL, app_dir).unwrap_or_default(),
        },
        llm_key: load_provider_key(ProviderKeyRef::Llm, app_dir).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_string_matches_reference_semantics() {
        // hashString("a") in JS: 97 -> "2p"; hashString("ab") -> "2e9".
        assert_eq!(hash_string("a"), "2p");
        assert_eq!(hash_string("ab"), "2e9");
        assert_eq!(hash_string(""), "0");
        assert_ne!(hash_string("hello"), hash_string("hellp"));
    }

    #[test]
    fn scoped_cache_key_composes_provider_target_and_identity() {
        let key = build_scoped_cache_key(
            TranslationProvider::DeepL,
            "pt-BR",
            "chat-messages-1-2",
            "hello",
        );
        assert_eq!(
            key,
            format!("deepl:pt-BR:chat-messages-1-2:{}", hash_string("hello"))
        );
        let keyless = build_scoped_cache_key(TranslationProvider::GoogleTranslate, "en", "", "x");
        assert_eq!(keyless, format!("google-translate:en:{}", hash_string("x")));
    }

    #[test]
    fn markdown_protection_round_trips() {
        let source = "- See `cargo test` and [docs](https://example.com) at https://example.com/x\n```rust\nfn main() {}\n```";
        let (protected, tokens) = protect_markdown_syntax(source);
        assert!(!protected.contains("cargo test"));
        assert!(!protected.contains("https://example.com/x"));
        let restored = restore_markdown_syntax(&protected, &tokens);
        assert_eq!(restored, source);
    }

    #[test]
    fn markdown_protection_translates_content_but_keeps_fences() {
        let source = "Hello\n```\ncode block\n```\nWorld";
        let (protected, tokens) = protect_markdown_syntax(source);
        assert!(protected.contains("Hello"));
        assert!(!protected.contains("code block"));
        let restored = restore_markdown_syntax(&protected, &tokens);
        assert_eq!(restored, source);
    }

    #[test]
    fn line_prefix_split_preserves_markers() {
        assert_eq!(split_markdown_line_prefix("- item"), ("- ", "item"));
        assert_eq!(split_markdown_line_prefix("> quote"), ("> ", "quote"));
        assert_eq!(
            split_markdown_line_prefix("12. numbered"),
            ("12. ", "numbered")
        );
        assert_eq!(split_markdown_line_prefix("plain"), ("", "plain"));
        assert_eq!(
            split_markdown_line_prefix("   indented"),
            ("   ", "indented")
        );
    }

    #[test]
    fn deepl_language_normalization_matches_reference() {
        assert_eq!(normalize_deepl_language("pt"), "PT-BR");
        assert_eq!(normalize_deepl_language("en"), "EN-US");
        assert_eq!(normalize_deepl_language("ja"), "JA");
    }

    #[test]
    fn target_language_normalization() {
        assert_eq!(normalize_target_language("auto"), "en");
        assert_eq!(normalize_target_language(""), "en");
        assert_eq!(normalize_target_language("PT-BR"), "pt-br");
    }

    #[test]
    fn llm_base_url_validation() {
        assert!(validate_llm_base_url("https://api.openai.com/v1/").is_ok());
        assert!(validate_llm_base_url("http://localhost:1234/v1").is_ok());
        assert!(validate_llm_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_llm_base_url("http://api.example.com/v1").is_err());
        assert!(validate_llm_base_url("ftp://api.example.com").is_err());
        assert!(validate_llm_base_url("not a url").is_err());
        assert_eq!(
            validate_llm_base_url("https://api.openai.com/v1/").unwrap(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn cache_normalization_enforces_ttl_and_cap() {
        let mut cache = PersistedCache {
            version: 1,
            entries: HashMap::new(),
        };
        let now = chrono::Utc::now().timestamp_millis();
        for index in 0..(MAX_TRANSLATION_CACHE_ENTRIES + 10) {
            cache.entries.insert(
                format!("key-{index}"),
                CacheEntry {
                    translated_text: format!("text-{index}"),
                    created_at: now - index as i64 * 1000,
                },
            );
        }
        cache.entries.insert(
            "expired".to_string(),
            CacheEntry {
                translated_text: "old".to_string(),
                created_at: now - TRANSLATION_CACHE_TTL_MS - 1,
            },
        );
        normalize_cache_entries(&mut cache);
        assert_eq!(cache.entries.len(), MAX_TRANSLATION_CACHE_ENTRIES);
        assert!(!cache.entries.contains_key("expired"));
        // Newest entries survive the cap.
        assert!(cache.entries.contains_key("key-0"));
        assert!(!cache
            .entries
            .contains_key(&format!("key-{}", MAX_TRANSLATION_CACHE_ENTRIES + 9)));
    }

    #[test]
    fn cache_file_round_trip() {
        let directory =
            std::env::temp_dir().join(format!("dqh-translation-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(TRANSLATION_CACHE_FILE);
        let cache = PersistedCache {
            version: TRANSLATION_CACHE_VERSION,
            entries: HashMap::from([(
                "k".to_string(),
                CacheEntry {
                    translated_text: "v".to_string(),
                    created_at: chrono::Utc::now().timestamp_millis(),
                },
            )]),
        };
        let snapshot = serde_json::to_vec(&cache).unwrap();
        write_cache_to_disk(&path, &snapshot);
        let loaded = load_cache_from_disk(&path);
        assert_eq!(loaded.entries["k"].translated_text, "v");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn corrupt_cache_file_is_quarantined() {
        let directory =
            std::env::temp_dir().join(format!("dqh-translation-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(TRANSLATION_CACHE_FILE);
        std::fs::write(&path, b"{not json").unwrap();
        let loaded = load_cache_from_disk(&path);
        assert!(loaded.entries.is_empty());
        assert!(!path.exists(), "corrupt file should have been renamed");
        assert!(std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt")));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn prompts_mention_target_language_and_limits() {
        let user = composer_translation_user_prompt("hi", "pt-BR", Some("general"), None, None);
        assert!(user.contains("Target language: pt-BR"));
        assert!(user.contains("Current draft:\nhi"));
        let summary = channel_summary_user_prompt("m1\nm2", 50, Some("dev"), Some("topic"));
        assert!(summary.contains("Summarize these last 50 Discord messages"));
        assert!(summary.contains("Channel: #dev"));
    }

    #[test]
    fn key_fallback_file_round_trip() {
        let directory =
            std::env::temp_dir().join(format!("dqh-translation-test-{}", uuid::Uuid::new_v4()));
        let keys = HashMap::from([(
            ProviderKeyRef::DeepL.account().to_string(),
            "abc".to_string(),
        )]);
        write_fallback_keys(&directory, &keys).unwrap();
        assert_eq!(
            read_fallback_keys(&directory)
                .get("translation.deepl")
                .map(String::as_str),
            Some("abc")
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
