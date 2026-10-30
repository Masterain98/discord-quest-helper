use super::super::{
    channel::{
        Descriptor, Field, InputKind, NotificationChannel, Receipt, ResolvedConfig, TemplateField,
        TemplateVariable,
    },
    error::{Code, Error, Result},
    http,
    message::Message,
    model::{messages, EventKind},
};
use chrono::{DateTime, SecondsFormat, Utc};
use futures_util::future::BoxFuture;
use regex::{Captures, Regex};
use serde::Serialize;
use std::{collections::BTreeMap, sync::LazyLock};

pub(super) struct CustomWebhookChannel {
    client: http::Client,
}
impl CustomWebhookChannel {
    pub fn new(client: http::Client) -> Self {
        Self { client }
    }
}

const ALL: &[EventKind] = &[
    EventKind::QuestStarted,
    EventKind::QuestCompleted,
    EventKind::QueueCompleted,
];
const QUEST: &[EventKind] = &[EventKind::QuestStarted, EventKind::QuestCompleted];
const VARIABLES: &[TemplateVariable] = &[
    TemplateVariable {
        name: "quest-name",
        description_key: "notifications.variable_quest_name",
        events: QUEST,
    },
    TemplateVariable {
        name: "event-type",
        description_key: "notifications.variable_event_type",
        events: ALL,
    },
    TemplateVariable {
        name: "event-id",
        description_key: "notifications.variable_event_id",
        events: ALL,
    },
    TemplateVariable {
        name: "timestamp",
        description_key: "notifications.variable_timestamp",
        events: ALL,
    },
    TemplateVariable {
        name: "estimated-complete-at",
        description_key: "notifications.variable_estimate",
        events: &[EventKind::QuestStarted],
    },
    TemplateVariable {
        name: "reward-image",
        description_key: "notifications.variable_reward_image",
        events: QUEST,
    },
    TemplateVariable {
        name: "succeeded",
        description_key: "notifications.succeeded",
        events: &[EventKind::QueueCompleted],
    },
    TemplateVariable {
        name: "failed",
        description_key: "notifications.failed",
        events: &[EventKind::QueueCompleted],
    },
    TemplateVariable {
        name: "skipped",
        description_key: "notifications.skipped",
        events: &[EventKind::QueueCompleted],
    },
];
const fn template(
    id: &'static str,
    event: EventKind,
    part: &'static str,
    default_key: &'static str,
    label_key: &'static str,
    kind: InputKind,
) -> Field {
    Field {
        id,
        label_key,
        kind,
        required: false,
        placeholder: None,
        template: Some(TemplateField {
            event,
            part,
            default_key,
        }),
    }
}
const FIELDS: &[Field] = &[
    Field {
        id: "endpoint",
        label_key: "notifications.webhook_endpoint",
        kind: InputKind::Password,
        required: true,
        placeholder: Some("https://example.com/webhook"),
        template: None,
    },
    Field {
        id: "bearer_token",
        label_key: "notifications.bearer_token",
        kind: InputKind::Password,
        required: false,
        placeholder: None,
        template: None,
    },
    template(
        "quest_started_title",
        EventKind::QuestStarted,
        "title",
        "notifications.webhook_started_title",
        "notifications.template_title",
        InputKind::Text,
    ),
    template(
        "quest_started_content",
        EventKind::QuestStarted,
        "content",
        "notifications.webhook_started_content",
        "notifications.template_content",
        InputKind::Textarea,
    ),
    template(
        "quest_completed_title",
        EventKind::QuestCompleted,
        "title",
        "notifications.webhook_completed_title",
        "notifications.template_title",
        InputKind::Text,
    ),
    template(
        "quest_completed_content",
        EventKind::QuestCompleted,
        "content",
        "notifications.webhook_completed_content",
        "notifications.template_content",
        InputKind::Textarea,
    ),
    template(
        "queue_completed_title",
        EventKind::QueueCompleted,
        "title",
        "notifications.webhook_queue_title",
        "notifications.template_title",
        InputKind::Text,
    ),
    template(
        "queue_completed_content",
        EventKind::QueueCompleted,
        "content",
        "notifications.webhook_queue_content",
        "notifications.template_content",
        InputKind::Textarea,
    ),
];

struct WebhookConfig<'a> {
    endpoint: url::Url,
    authorization: Option<reqwest::header::HeaderValue>,
    values: &'a ResolvedConfig,
}
impl<'a> WebhookConfig<'a> {
    fn parse(values: &'a ResolvedConfig) -> Result<Self> {
        let invalid = || Error::new(Code::InvalidConfig).at("endpoint");
        let endpoint = url::Url::parse(values.get("endpoint")).map_err(|_| invalid())?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(invalid());
        }
        let token = values.get("bearer_token");
        let authorization = if token.is_empty() {
            None
        } else {
            let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| Error::new(Code::InvalidConfig).at("bearer_token"))?;
            header.set_sensitive(true);
            Some(header)
        };
        Ok(Self {
            endpoint,
            authorization,
            values,
        })
    }
}

impl NotificationChannel for CustomWebhookChannel {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "custom_webhook",
            name_key: "notifications.custom_webhook",
            description_key: Some("notifications.custom_webhook_desc"),
            icon: Some("send"),
            enabled_by_default: false,
            fields: FIELDS,
            help_links: &[],
            template_variables: VARIABLES,
        }
    }
    fn validate(&self, config: &ResolvedConfig) -> Result<()> {
        WebhookConfig::parse(config).map(|_| ())
    }
    fn send<'a>(
        &'a self,
        config: &'a ResolvedConfig,
        message: &'a Message,
    ) -> BoxFuture<'a, Result<Receipt>> {
        Box::pin(async move {
            let response = http::submit(self.request(config, message)?).await?;
            check_response(response)
        })
    }
}
impl CustomWebhookChannel {
    fn request(
        &self,
        values: &ResolvedConfig,
        message: &Message,
    ) -> Result<reqwest::RequestBuilder> {
        let config = WebhookConfig::parse(values)?;
        let mut request = self
            .client
            .get()?
            .post(config.endpoint.clone())
            .json(&payload(config.values, message));
        if let Some(header) = config.authorization {
            request = request.header(reqwest::header::AUTHORIZATION, header);
        }
        Ok(request)
    }
}
fn check_response(response: reqwest::Response) -> Result<Receipt> {
    match response.status().as_u16() {
        202 => Ok(Receipt::Submitted),
        200..=299 => Ok(Receipt::Sent),
        401 => Err(Code::Unauthorized.into()),
        403 => Err(Code::Forbidden.into()),
        404 => Err(Code::NotFound.into()),
        429 => Err(Code::RateLimited.into()),
        _ => Err(Code::Rejected.into()),
    }
}

#[derive(Serialize)]
struct QuestData {
    name: String,
    reward_image: Option<String>,
    estimated_complete_at: Option<String>,
}
#[derive(Serialize)]
struct QueueData {
    succeeded: u32,
    failed: u32,
    skipped: u32,
}
#[derive(Serialize)]
struct Payload {
    schema_version: u8,
    event_id: String,
    event_type: EventKind,
    timestamp: String,
    locale: String,
    is_test: bool,
    title: String,
    content: String,
    quest: Option<QuestData>,
    queue: Option<QueueData>,
}
fn payload(config: &ResolvedConfig, message: &Message) -> Payload {
    let event = &message.event;
    let mut payload = Payload {
        schema_version: 1,
        event_id: event.id.clone(),
        event_type: event.kind,
        timestamp: message
            .timestamp
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        locale: event.locale.clone(),
        is_test: message.is_test,
        title: String::new(),
        content: String::new(),
        quest: (event.kind != EventKind::QueueCompleted).then(|| QuestData {
            name: event.quest_name.clone(),
            reward_image: event.reward_image.clone(),
            estimated_complete_at: if event.kind == EventKind::QuestStarted {
                event
                    .estimated_complete_at
                    .as_deref()
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .map(|date| {
                        date.with_timezone(&Utc)
                            .to_rfc3339_opts(SecondsFormat::Millis, true)
                    })
            } else {
                None
            },
        }),
        queue: (event.kind == EventKind::QueueCompleted).then_some(QueueData {
            succeeded: event.succeeded,
            failed: event.failed,
            skipped: event.skipped,
        }),
    };
    let values = variable_values(&payload);
    let defaults = messages(&event.locale);
    for field in FIELDS {
        let Some(template) = &field.template else {
            continue;
        };
        if template.event != event.kind {
            continue;
        }
        let text = if config.contains(field.id) {
            config.get(field.id)
        } else {
            defaults[template.default_key.strip_prefix("notifications.").unwrap()]
                .as_str()
                .unwrap_or("")
        };
        let rendered = render_template(text, &values);
        if template.part == "title" {
            payload.title = rendered;
        } else {
            payload.content = rendered;
        }
    }
    payload
}
fn variable_values(payload: &Payload) -> BTreeMap<&'static str, String> {
    let quest = payload.quest.as_ref();
    let queue = payload.queue.as_ref();
    BTreeMap::from([
        (
            "quest-name",
            quest.map(|q| q.name.clone()).unwrap_or_default(),
        ),
        (
            "event-type",
            serde_json::to_value(payload.event_type)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
        ),
        ("event-id", payload.event_id.clone()),
        ("timestamp", payload.timestamp.clone()),
        (
            "estimated-complete-at",
            quest
                .and_then(|q| q.estimated_complete_at.clone())
                .unwrap_or_default(),
        ),
        (
            "reward-image",
            quest
                .and_then(|q| q.reward_image.clone())
                .unwrap_or_default(),
        ),
        (
            "succeeded",
            queue.map(|q| q.succeeded.to_string()).unwrap_or_default(),
        ),
        (
            "failed",
            queue.map(|q| q.failed.to_string()).unwrap_or_default(),
        ),
        (
            "skipped",
            queue.map(|q| q.skipped.to_string()).unwrap_or_default(),
        ),
    ])
}

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{\{([A-Za-z][A-Za-z0-9_-]*)\}\}|\{([A-Za-z][A-Za-z0-9_-]*)\}").unwrap()
});
fn render_template(template: &str, values: &BTreeMap<&str, String>) -> String {
    TOKENS
        .replace_all(template, |captures: &Captures<'_>| {
            if let Some(escaped) = captures.get(1) {
                format!("{{{}}}", escaped.as_str())
            } else {
                values
                    .get(captures.get(2).unwrap().as_str())
                    .cloned()
                    .unwrap_or_else(|| captures[0].into())
            }
        })
        .into_owned()
}

#[cfg(test)]
#[path = "custom_webhook_tests.rs"]
mod tests;
