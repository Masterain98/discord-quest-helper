use super::super::{
    channel::{
        Descriptor, Field, HelpLink, InputKind, NotificationChannel, Receipt, ResolvedConfig,
    },
    error::{Code, Error, Result},
    http,
    message::Message,
};
use chrono::Local;
use futures_util::future::BoxFuture;

pub(super) struct TelegramChannel {
    client: http::Client,
}
impl TelegramChannel {
    pub fn new(client: http::Client) -> Self {
        Self { client }
    }
}

struct TelegramConfig<'a> {
    token: &'a str,
    chat: &'a str,
    topic: Option<u64>,
}
impl<'a> TelegramConfig<'a> {
    fn parse(config: &'a ResolvedConfig) -> Result<Self> {
        let token = config.get("token");
        let (id, secret) = token
            .split_once(':')
            .ok_or_else(|| Error::new(Code::InvalidToken).at("token"))?;
        if !numeric(id)
            || secret.is_empty()
            || !secret
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(Error::new(Code::InvalidToken).at("token"));
        }
        let chat = config.get("chat_id");
        let username = chat.strip_prefix('@').is_some_and(|v| {
            !v.is_empty()
                && v.len() <= 64
                && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        });
        if !numeric(chat.strip_prefix('-').unwrap_or(chat)) && !username {
            return Err(Error::new(Code::InvalidTarget).at("chat_id"));
        }
        let value = config.get("topic_id");
        let topic = if value.is_empty() {
            None
        } else {
            Some(
                value
                    .parse::<u64>()
                    .ok()
                    .filter(|id| *id > 0 && numeric(value))
                    .ok_or_else(|| Error::new(Code::InvalidThread).at("topic_id"))?,
            )
        };
        Ok(Self { token, chat, topic })
    }
}
fn numeric(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|c| c.is_ascii_digit())
}

impl NotificationChannel for TelegramChannel {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "telegram",
            name_key: "notifications.telegram",
            description_key: None,
            icon: Some("send"),
            enabled_by_default: false,
            fields: &[
                Field {
                    id: "token",
                    label_key: "notifications.bot_token",
                    kind: InputKind::Password,
                    required: true,
                    placeholder: None,
                },
                Field {
                    id: "chat_id",
                    label_key: "notifications.chat_id",
                    kind: InputKind::Text,
                    required: true,
                    placeholder: Some("123456789 / -100123456789 / @channel"),
                },
                Field {
                    id: "topic_id",
                    label_key: "notifications.topic_id",
                    kind: InputKind::Numeric,
                    required: false,
                    placeholder: None,
                },
            ],
            help_links: &[
                HelpLink {
                    label_key: "notifications.setup_help",
                    url: "https://core.telegram.org/bots/tutorial",
                },
                HelpLink {
                    label_key: "notifications.target_help",
                    url: "https://core.telegram.org/bots/api#getupdates",
                },
            ],
        }
    }
    fn validate(&self, config: &ResolvedConfig) -> Result<()> {
        TelegramConfig::parse(config).map(|_| ())
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
impl TelegramChannel {
    fn request(
        &self,
        config: &ResolvedConfig,
        message: &Message,
    ) -> Result<reqwest::RequestBuilder> {
        let config = TelegramConfig::parse(config)?;
        let mut payload = serde_json::json!({"chat_id": config.chat, "text": format(message), "parse_mode": "MarkdownV2"});
        if let Some(topic) = config.topic {
            payload["message_thread_id"] = topic.into();
        }
        Ok(self
            .client
            .get()?
            .post(format!(
                "https://api.telegram.org/bot{}/sendMessage",
                config.token
            ))
            .json(&payload))
    }
}
async fn check_response(response: reqwest::Response) -> Result<()> {
    if http::json(response).await?["ok"] != true {
        return Err(Code::Rejected.into());
    }
    Ok(())
}

pub(super) fn markdown_v2(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\_*[]()~`>#+-=|{}.!".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}
pub(super) fn format(message: &Message) -> String {
    let mut blocks = vec![format!("*{}*", markdown_v2(&message.title))];
    if !message.detail.is_empty() {
        blocks.push(markdown_v2(&message.detail));
    }
    let fields: Vec<_> = message
        .fields()
        .into_iter()
        .map(|(label, value)| format!("*{}:* {}", markdown_v2(&label), markdown_v2(&value)))
        .collect();
    if !fields.is_empty() {
        blocks.push(fields.join("\n"));
    }
    blocks.push(format!(
        "_{}_\n_{}_",
        markdown_v2(&message.footer()),
        markdown_v2(
            &message
                .timestamp
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string()
        )
    ));
    blocks.join("\n\n")
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
    fn validates_token_chat_and_topic_and_preserves_formatted_messages() {
        let channel = TelegramChannel::new(http::Client::default());
        let config = resolved(&[
            ("token", "123456:dummy-test-token"),
            ("chat_id", "-100123456"),
            ("topic_id", "42"),
        ]);
        let message = message::render(&event(EventKind::QuestStarted), false);
        let request = channel.request(&config, &message).unwrap().build().unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(json["chat_id"], "-100123456");
        assert_eq!(json["message_thread_id"], 42);
        assert_eq!(json["parse_mode"], "MarkdownV2");
        assert!(json["text"]
            .as_str()
            .unwrap()
            .contains("\\_\\[special\\]\\(name\\)\\!\\_"));
        assert_eq!(
            markdown_v2("\\_*[]()~`>#+-=|{}.! ").chars().count(),
            19 * 2 + 1
        );
        assert_eq!(markdown_v2("奖励 你好: 10"), "奖励 你好: 10");
        for (key, value, code) in [
            ("token", "bad/token", Code::InvalidToken),
            ("chat_id", "not a chat", Code::InvalidTarget),
            ("topic_id", "-1", Code::InvalidThread),
            ("topic_id", "0", Code::InvalidThread),
        ] {
            let mut invalid = resolved(&[("token", "123456:dummy"), ("chat_id", "@channel_name")]);
            invalid
                .0
                .insert(key.into(), zeroize::Zeroizing::new(value.into()));
            let error = channel.validate(&invalid).unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.field.as_deref(), Some(key));
        }
    }
    #[tokio::test]
    async fn checks_provider_response_without_exposing_body() {
        assert!(check_response(response(200, r#"{"ok":true}"#))
            .await
            .is_ok());
        assert_eq!(
            check_response(response(
                200,
                r#"{"ok":false,"description":"not-for-logs"}"#
            ))
            .await
            .unwrap_err()
            .code,
            Code::Rejected
        );
        assert_eq!(
            check_response(response(200, "invalid"))
                .await
                .unwrap_err()
                .code,
            Code::InvalidResponse
        );
    }
}
