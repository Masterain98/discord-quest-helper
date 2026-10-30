use super::*;
use crate::notifications::{
    atomic,
    channel::{Draft, Registry},
    config::{self, ChannelConfig, Config},
    message,
    secrets::Secrets,
    service::NotificationService,
    testing::{event, resolved, response, MemorySecrets},
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn endpoints_and_headers_are_validated_without_echoing_credentials() {
    let adapter = CustomWebhookChannel::new(http::Client::default());
    for endpoint in [
        "http://localhost:8787/hook?key=fixture-secret",
        "http://127.0.0.1:1234/",
        "http://[::1]:1234/",
        "https://192.168.1.2:8443/hook",
        "https://example.invalid/hook?a=1&b=2",
    ] {
        assert!(adapter
            .validate(&resolved(&[("endpoint", endpoint)]))
            .is_ok());
    }
    for endpoint in [
        "file:///tmp/secret",
        "ftp://example.invalid",
        "/relative",
        "https://",
        "https://user:fixture-secret@example.invalid/",
        "https://user@example.invalid/",
        "https://example.invalid/#fragment",
    ] {
        let error = adapter
            .validate(&resolved(&[("endpoint", endpoint)]))
            .unwrap_err();
        assert_eq!(error.field.as_deref(), Some("endpoint"));
        assert!(!serde_json::to_string(&error)
            .unwrap()
            .contains("fixture-secret"));
    }
    let error = adapter
        .validate(&resolved(&[
            ("endpoint", "https://example.invalid/"),
            ("bearer_token", "fixture-secret\r\nx: y"),
        ]))
        .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("bearer_token"));
}

#[test]
fn shared_template_cases_are_single_pass_and_preserve_text() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/notification-template-cases.json"
    ))
    .unwrap();
    let values = BTreeMap::from([
        ("quest-name", "Quest {timestamp}".into()),
        ("timestamp", "2026-10-31T08:00:00.000Z".into()),
        ("succeeded", "2".into()),
        ("failed", "0".into()),
        ("skipped", "".into()),
    ]);
    for case in cases.as_array().unwrap() {
        assert_eq!(
            render_template(case["text"].as_str().unwrap(), &values),
            case["expected"]
        );
    }
}

#[test]
fn payloads_keep_raw_business_context_and_exact_template_text() {
    for kind in ALL {
        let mut e = event(*kind);
        e.quest_name = "  Long \"quest\"\n{timestamp} 🐾  ".repeat(30);
        e.reward_image = Some("/icons/orbs.png".into());
        e.estimated_complete_at = Some("2026-10-31T16:05:00+08:00".into());
        let m = message::render(&e, false);
        let field = FIELDS
            .iter()
            .find(|f| {
                f.template
                    .as_ref()
                    .is_some_and(|t| t.event == *kind && t.part == "content")
            })
            .unwrap();
        let title = FIELDS
            .iter()
            .find(|f| {
                f.template
                    .as_ref()
                    .is_some_and(|t| t.event == *kind && t.part == "title")
            })
            .unwrap();
        let cfg = resolved(&[
            (
                field.id,
                " \n{quest-name}|{succeeded}|{estimated-complete-at}|{reward-image}\n ",
            ),
            (title.id, ""),
        ]);
        let p = payload(&cfg, &m);
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["event_id"], e.id);
        assert_eq!(json["locale"], "en");
        assert_eq!(json["is_test"], false);
        assert_eq!(p.title, "");
        assert!(p.content.starts_with(" \n") && p.content.ends_with("\n "));
        if *kind == EventKind::QueueCompleted {
            assert!(json["quest"].is_null());
            assert_eq!(
                json["queue"],
                serde_json::json!({"succeeded":2,"failed":1,"skipped":3})
            );
            assert_eq!(p.content, " \n|2||\n ");
        } else {
            assert!(json["queue"].is_null());
            assert_eq!(json["quest"]["name"], e.quest_name);
            assert_eq!(json["quest"]["reward_image"], "/icons/orbs.png");
            assert!(p.content.contains("{timestamp}")); // Values are never expanded again.
            if *kind == EventKind::QuestStarted {
                assert_eq!(
                    json["quest"]["estimated_complete_at"],
                    "2026-10-31T08:05:00.000Z"
                );
            } else {
                assert!(json["quest"]["estimated_complete_at"].is_null());
            }
        }
        assert!(p.timestamp.ends_with('Z'));
    }
    let mut e = event(EventKind::QuestStarted);
    e.reward_image = None;
    e.estimated_complete_at = Some("invalid".into());
    let json = serde_json::to_value(payload(&resolved(&[]), &message::render(&e, false))).unwrap();
    assert!(json["quest"]["reward_image"].is_null());
    assert!(json["quest"]["estimated_complete_at"].is_null());
}

#[test]
fn defaults_follow_locale_but_overrides_including_empty_text_do_not() {
    let mut e = event(EventKind::QuestCompleted);
    let english = payload(&resolved(&[]), &message::render(&e, false));
    for locale in ["zh", "zh-TW", "ja"] {
        e.locale = locale.into();
        let localized = payload(&resolved(&[]), &message::render(&e, false));
        assert_ne!(localized.title, english.title);
        let overridden = payload(
            &resolved(&[
                ("quest_completed_title", " 固定标题 "),
                ("quest_completed_content", ""),
            ]),
            &message::render(&e, false),
        );
        assert_eq!(overridden.title, " 固定标题 ");
        assert_eq!(overridden.content, "");
    }
    e.locale = "unsupported".into();
    assert_eq!(
        payload(&resolved(&[]), &message::render(&e, false)).title,
        english.title
    );
}

struct Fixture {
    root: PathBuf,
    adapter: CustomWebhookChannel,
    secrets: Arc<MemorySecrets>,
    config: Config,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir().join(format!("dqh-webhook-{}", uuid::Uuid::new_v4())),
            adapter: CustomWebhookChannel::new(http::Client::default()),
            secrets: Arc::new(MemorySecrets::default()),
            config: Config {
                version: 2,
                events: Default::default(),
                channels: Default::default(),
            },
        }
    }
    fn path(&self) -> PathBuf {
        self.root.join("notifications.json")
    }
    fn save(&mut self, draft: &Draft) -> Result<()> {
        let path = self.path();
        config::update(
            &mut self.config,
            &path,
            &self.adapter,
            draft,
            self.secrets.as_ref(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn draft(fields: &[(&str, &str)], reset: &[&str]) -> Draft {
    Draft {
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        reset_fields: reset.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn sparse_overrides_and_optional_secret_resets_are_transactional() {
    let mut f = Fixture::new();
    f.save(&draft(
        &[
            ("endpoint", "http://localhost:8787/?fixture-secret"),
            ("bearer_token", "fixture-token"),
            ("quest_completed_content", " \n{quest-name}\n "),
        ],
        &[],
    ))
    .unwrap();
    let saved = f.config.channels["custom_webhook"].clone();
    assert_eq!(
        saved.values["quest_completed_content"],
        " \n{quest-name}\n "
    );
    assert!(!saved.values.contains_key("quest_started_title"));
    assert!(!std::fs::read_to_string(f.path())
        .unwrap()
        .contains("fixture-secret"));
    assert!(!std::fs::read_to_string(f.path())
        .unwrap()
        .contains("fixture-token"));
    f.save(&draft(&[("quest_completed_content", "")], &[]))
        .unwrap();
    assert_eq!(
        f.config.channels["custom_webhook"].values["quest_completed_content"],
        ""
    );
    let reset = draft(&[], &["quest_completed_content", "bearer_token"]);
    let path = f.path();
    let before = std::fs::read(&path).unwrap();
    let error = config::update_with_save(
        &mut f.config,
        &path,
        &f.adapter,
        &reset,
        f.secrets.as_ref(),
        |_, _| Err(Code::Storage.into()),
    )
    .unwrap_err();
    assert_eq!(error.code, Code::Storage);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(f.secrets.get(&saved.secret_refs["bearer_token"]).is_ok());
    f.save(&reset).unwrap();
    assert!(!f.config.channels["custom_webhook"]
        .values
        .contains_key("quest_completed_content"));
    assert!(!f.config.channels["custom_webhook"]
        .secret_refs
        .contains_key("bearer_token"));
    assert!(f.secrets.get(&saved.secret_refs["bearer_token"]).is_err());
    assert!(f.secrets.get(&saved.secret_refs["endpoint"]).is_ok());
    for invalid in [
        draft(
            &[("quest_completed_content", "")],
            &["quest_completed_content"],
        ),
        draft(&[], &["endpoint"]),
        draft(&[], &["missing"]),
        draft(&[], &["bearer_token", "bearer_token"]),
    ] {
        assert!(f.save(&invalid).is_err());
    }
    // Explicit clearing must not read the credential being removed.
    f.config
        .channels
        .get_mut("custom_webhook")
        .unwrap()
        .secret_refs
        .insert("bearer_token".into(), "missing-vault-entry".into());
    f.save(&draft(&[], &["bearer_token"])).unwrap();
}

struct Captured {
    headers: String,
    body: serde_json::Value,
    additional_requests: bool,
}
async fn receiver(status: u16, stall: bool) -> (String, tokio::task::JoinHandle<Captured>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let (headers, end, length) = loop {
            let mut chunk = [0u8; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (headers, end + 4, length);
            }
        };
        while bytes.len() < end + length {
            let mut chunk = [0; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
        let body = serde_json::from_slice(&bytes[end..end + length]).unwrap();
        if stall {
            tokio::time::sleep(Duration::from_millis(250)).await;
        } else {
            let reply = format!("HTTP/1.1 {status} Response\r\nContent-Length: 0\r\nLocation: http://{addr}/redirected\r\nConnection: close\r\n\r\n");
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
        let additional_requests =
            tokio::time::timeout(Duration::from_millis(80), listener.accept())
                .await
                .is_ok();
        Captured {
            headers,
            body,
            additional_requests,
        }
    });
    (format!("http://{addr}/hook?key=fixture-secret"), task)
}

#[tokio::test]
async fn local_receiver_checks_real_post_status_empty_body_auth_and_no_redirects() {
    let adapter = CustomWebhookChannel::new(http::Client::default());
    for status in [200, 201, 202, 204, 299, 302, 401, 403, 404, 429, 500] {
        let (endpoint, capture) = receiver(status, false).await;
        let result = adapter
            .send(
                &resolved(&[("endpoint", &endpoint), ("bearer_token", "fixture-token")]),
                &message::render(&event(EventKind::QuestCompleted), false),
            )
            .await;
        let captured = capture.await.unwrap();
        assert!(captured
            .headers
            .starts_with("POST /hook?key=fixture-secret HTTP/1.1"));
        let headers = captured.headers.to_ascii_lowercase();
        assert!(headers.contains("content-type: application/json"));
        assert!(headers.contains("authorization: bearer fixture-token"));
        assert_eq!(captured.body["event_type"], "quest_completed");
        assert!(!captured.additional_requests);
        match status {
            202 => assert!(matches!(result.unwrap(), Receipt::Submitted)),
            200..=299 => assert!(matches!(result.unwrap(), Receipt::Sent)),
            _ => {
                let error = result.err().unwrap();
                let expected = match status {
                    401 => Code::Unauthorized,
                    403 => Code::Forbidden,
                    404 => Code::NotFound,
                    429 => Code::RateLimited,
                    _ => Code::Rejected,
                };
                assert_eq!(error.code, expected);
                let error = serde_json::to_string(&error).unwrap();
                assert!(!error.contains("fixture-secret") && !error.contains("fixture-token"));
            }
        }
    }
    assert!(matches!(
        check_response(response(200, "not JSON")).unwrap(),
        Receipt::Sent
    ));
}

#[tokio::test]
async fn timeout_does_not_retry_or_expose_endpoint() {
    let (endpoint, capture) = receiver(200, true).await;
    let adapter = CustomWebhookChannel::new(http::Client::default());
    let request = adapter
        .request(
            &resolved(&[("endpoint", &endpoint)]),
            &message::render(&event(EventKind::QuestCompleted), false),
        )
        .unwrap()
        .timeout(Duration::from_millis(100));
    let error = http::submit(request).await.err().unwrap();
    assert_eq!(error.code, Code::Timeout);
    assert!(!serde_json::to_string(&error)
        .unwrap()
        .contains("fixture-secret"));
    let captured = capture.await.unwrap();
    assert!(!captured
        .headers
        .to_ascii_lowercase()
        .contains("authorization:"));
    assert!(!captured.additional_requests);
}

#[tokio::test]
async fn tests_use_sample_templates_without_saving_and_dispatch_still_filters_deduplicates_and_isolates(
) {
    let mut f = Fixture::new();
    let (endpoint, capture) = receiver(204, false).await;
    f.save(&draft(
        &[("endpoint", &endpoint), ("quest_completed_title", "saved")],
        &[],
    ))
    .unwrap();
    let before = std::fs::read(f.path()).unwrap();
    let registry = super::super::headless();
    let service = Arc::new(NotificationService::new(
        registry,
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let result = service
        .test(
            f.path(),
            "custom_webhook".into(),
            draft(&[("quest_completed_title", "草稿 {quest-name}")], &[]),
            "zh".into(),
            None,
        )
        .await
        .unwrap();
    assert!(result.success);
    let sample = capture.await.unwrap().body;
    assert_eq!(sample["is_test"], true);
    assert_eq!(sample["event_type"], "quest_completed");
    assert_eq!(sample["locale"], "zh");
    assert_eq!(
        sample["title"],
        format!("草稿 {}", messages("zh")["sample_quest"].as_str().unwrap())
    );
    assert!(!sample["event_id"].as_str().unwrap().is_empty());
    assert_eq!(std::fs::read(f.path()).unwrap(), before);
    let view = serde_json::to_value(service.settings(f.path()).await.unwrap()).unwrap();
    let webhook = view["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "custom_webhook")
        .unwrap();
    assert_eq!(
        webhook["values"],
        serde_json::json!({"quest_completed_title":"saved"})
    );
    assert!(!view.to_string().contains("fixture-secret"));

    // A failed Webhook must not hold up another channel or disable event filtering/deduplication.
    f.config.channels.insert(
        "custom_webhook".into(),
        ChannelConfig {
            enabled: true,
            ..f.config.channels["custom_webhook"].clone()
        },
    );
    atomic::save(&f.path(), &f.config).unwrap();
    let service = Arc::new(NotificationService::new(
        Registry::new(vec![
            Arc::new(crate::notifications::testing::FakeChannel::system()),
            Arc::new(CustomWebhookChannel::new(http::Client::unavailable())),
        ])
        .unwrap(),
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let results = futures_util::future::join_all(
        service
            .dispatch(f.path(), event(EventKind::QuestCompleted))
            .await
            .unwrap(),
    )
    .await;
    assert!(results
        .iter()
        .any(|r| r.as_ref().unwrap().channel == "system" && r.as_ref().unwrap().success));
    assert!(results
        .iter()
        .any(|r| r.as_ref().unwrap().channel == "custom_webhook"
            && r.as_ref().unwrap().code == "network"));
    assert!(service
        .dispatch(f.path(), event(EventKind::QuestCompleted))
        .await
        .unwrap()
        .is_empty());
    assert!(service
        .dispatch(f.path(), event(EventKind::QuestStarted))
        .await
        .unwrap()
        .is_empty());
}
