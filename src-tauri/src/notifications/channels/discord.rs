use super::super::model::EventKind;
use super::super::{
    channel::{
        Descriptor, Field, HelpLink, InputKind, NotificationChannel, Receipt, ResolvedConfig,
    },
    error::{Code, Error, Result},
    http,
    message::Message,
};
use futures_util::future::BoxFuture;

pub(super) struct DiscordChannel {
    client: http::Client,
}
impl DiscordChannel {
    pub fn new(client: http::Client) -> Self {
        Self { client }
    }
}
struct DiscordConfig {
    webhook: url::Url,
}
impl DiscordConfig {
    fn parse(config: &ResolvedConfig) -> Result<Self> {
        let mut webhook = webhook(config.get("webhook_url"))?;
        webhook.query_pairs_mut().append_pair("wait", "true");
        let thread = config.get("thread_id");
        if !thread.is_empty() {
            if !numeric(thread) || thread.parse::<u64>().ok().filter(|id| *id > 0).is_none() {
                return Err(Error::new(Code::InvalidThread).at("thread_id"));
            }
            webhook.query_pairs_mut().append_pair("thread_id", thread);
        }
        Ok(Self { webhook })
    }
}
fn numeric(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|c| c.is_ascii_digit())
}
fn webhook(raw: &str) -> Result<url::Url> {
    let error = || Error::new(Code::InvalidWebhook).at("webhook_url");
    let mut url = url::Url::parse(raw).map_err(|_| error())?;
    let host = url.host_str().ok_or_else(error)?;
    if url.scheme() != "https"
        || ![
            "discord.com",
            "canary.discord.com",
            "ptb.discord.com",
            "discordapp.com",
        ]
        .contains(&host)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(error());
    }
    let parts: Vec<_> = url.path_segments().ok_or_else(error)?.collect();
    let (id, token) = match parts.as_slice() {
        ["api", "webhooks", id, token] | ["api", "v10", "webhooks", id, token] => (*id, *token),
        _ => return Err(error()),
    };
    if !numeric(id)
        || token.is_empty()
        || !token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        || url.query_pairs().any(|(key, _)| key != "wait")
    {
        return Err(error());
    }
    url.set_query(None);
    Ok(url)
}
impl NotificationChannel for DiscordChannel {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            template_variables: &[],
            id: "discord",
            name_key: "notifications.discord",
            description_key: None,
            icon: Some("bell"),
            enabled_by_default: false,
            fields: &[
                Field {
                    id: "webhook_url",
                    label_key: "notifications.webhook_url",
                    kind: InputKind::Password,
                    required: true,
                    placeholder: None,
                    template: None,
                },
                Field {
                    id: "thread_id",
                    label_key: "notifications.thread_id",
                    kind: InputKind::Numeric,
                    required: false,
                    placeholder: None,
                    template: None,
                },
            ],
            help_links: &[HelpLink {
                label_key: "notifications.setup_help",
                url: "https://support.discord.com/hc/en-us/articles/228383668-Intro-to-Webhooks",
            }],
        }
    }
    fn validate(&self, config: &ResolvedConfig) -> Result<()> {
        DiscordConfig::parse(config).map(|_| ())
    }
    fn send<'a>(
        &'a self,
        config: &'a ResolvedConfig,
        message: &'a Message,
    ) -> BoxFuture<'a, Result<Receipt>> {
        Box::pin(async move {
            let response = http::submit(self.request(config, message)?).await?;
            check_response(response).await?;
            Ok(Receipt::Sent)
        })
    }
}
impl DiscordChannel {
    fn request(
        &self,
        config: &ResolvedConfig,
        message: &Message,
    ) -> Result<reqwest::RequestBuilder> {
        let config = DiscordConfig::parse(config)?;
        Ok(self
            .client
            .get()?
            .post(config.webhook)
            .json(&format(message)))
    }
}
async fn check_response(response: reqwest::Response) -> Result<()> {
    if !http::json(response).await?["id"].is_string() {
        return Err(Code::InvalidResponse.into());
    }
    Ok(())
}

fn discord_text(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if "\\*_~`|[]<>".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

pub(super) fn format(message: &Message) -> serde_json::Value {
    let mut fields: Vec<_> = message.fields().into_iter().map(|(name, value)| serde_json::json!({"name": name, "value": value, "inline": message.kind == EventKind::QueueCompleted})).collect();
    if let Some(date) = message.estimated_at {
        if let Some(field) = fields.first_mut() {
            field["value"] = serde_json::json!(format!(
                "<t:{}:F>\n<t:{}:R>",
                date.timestamp(),
                date.timestamp()
            ));
        }
    }
    let color = match message.kind {
        EventKind::QuestStarted => 0x5865f2,
        EventKind::QuestCompleted => 0x22c55e,
        EventKind::QueueCompleted if message.counts[1] > 0 => 0xf59e0b,
        EventKind::QueueCompleted => 0x22c55e,
    };
    let mut embed = serde_json::json!({"title": message.title, "description": discord_text(&message.detail), "color": color,
            "fields": fields, "timestamp": message.timestamp.to_rfc3339(), "footer": {"text": message.footer()}});
    if let Some(url) = message
        .reward_image
        .as_ref()
        .filter(|url| url.starts_with("https://"))
    {
        embed["thumbnail"] = serde_json::json!({"url": url});
    }
    serde_json::json!({"embeds": [embed], "allowed_mentions": {"parse": []}})
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        message,
        model::EventKind,
        testing::{event, resolved, response},
    };
    use super::*;
    #[test]
    fn accepts_only_official_webhooks_and_valid_threads() {
        for url in [
            "http://discord.com/api/webhooks/123/token",
            "https://discord.com.evil.test/api/webhooks/123/token",
            "https://user@discord.com/api/webhooks/123/token",
            "https://discord.com/api/webhooks/123/token/extra",
            "https://discord.com/api/webhooks/123/token?thread_id=42",
            "https://discord.com/api/webhooks/123/token#fragment",
        ] {
            assert!(webhook(url).is_err());
        }
        assert!(webhook("https://canary.discord.com/api/v10/webhooks/123/token").is_ok());
        let config = resolved(&[
            (
                "webhook_url",
                "https://discord.com/api/webhooks/123/test-token?wait=false",
            ),
            ("thread_id", "42"),
        ]);
        let channel = DiscordChannel::new(http::Client::default());
        let message = message::render(&event(EventKind::QuestStarted), false);
        let request = channel.request(&config, &message).unwrap().build().unwrap();
        assert_eq!(request.url().query(), Some("wait=true&thread_id=42"));
        let json: serde_json::Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(json["allowed_mentions"]["parse"], serde_json::json!([]));
        assert!(json.get("content").is_none());
        assert_eq!(
            json["embeds"][0]["thumbnail"]["url"],
            "https://cdn.discordapp.com/reward.png"
        );
        assert!(json["embeds"][0]["fields"][0]["value"]
            .as_str()
            .unwrap()
            .contains(":R>"));
        assert!(json["embeds"][0]["timestamp"].is_string());
        let mut queue = message::render(&event(EventKind::QueueCompleted), false);
        queue.is_test = true;
        let json = format(&queue);
        assert_eq!(json["embeds"][0]["fields"].as_array().unwrap().len(), 3);
        assert_eq!(json["embeds"][0]["color"], 0xf59e0b);
        assert!(json["embeds"][0]["footer"]["text"]
            .as_str()
            .unwrap()
            .contains("Notification test"));
    }
    #[tokio::test]
    async fn checks_delivery_confirmation_and_error_codes_without_response_bodies() {
        assert!(check_response(response(200, r#"{"id":"123"}"#))
            .await
            .is_ok());
        assert_eq!(
            check_response(response(204, "")).await.unwrap_err().code,
            Code::InvalidResponse
        );
        for (status, code) in [
            (401, Code::Unauthorized),
            (403, Code::Forbidden),
            (404, Code::NotFound),
            (429, Code::RateLimited),
            (500, Code::Rejected),
            (302, Code::Rejected),
        ] {
            assert_eq!(
                check_response(response(status, "not-for-logs"))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
    }
}
