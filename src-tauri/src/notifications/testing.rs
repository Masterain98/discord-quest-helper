//! Test-only adapters and vault; no real accounts, native notifications or network deliveries.
use super::{
    atomic,
    channel::{
        Descriptor, Draft, Field, HelpLink, InputKind, NotificationChannel, Receipt, Registry,
        ResolvedConfig,
    },
    channels,
    config::{self, ChannelConfig, Config},
    error::{Code, Error, Result},
    message::{self, Message},
    model::{EventKind, NotificationEvent},
    secrets::Secrets,
    service::NotificationService,
};
use futures_util::future::BoxFuture;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Default)]
pub(super) struct MemorySecrets {
    values: Mutex<BTreeMap<String, String>>,
    locked: AtomicBool,
    fail_set_at: AtomicUsize,
    sets: AtomicUsize,
    fail_delete_at: AtomicUsize,
    deletes: AtomicUsize,
}
impl Secrets for MemorySecrets {
    fn get(&self, r: &str) -> Result<String> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(Code::SecretStore.into());
        }
        self.values
            .lock()
            .unwrap()
            .get(r)
            .cloned()
            .ok_or(Code::SecretStore.into())
    }
    fn set(&self, r: &str, value: &str) -> Result<()> {
        let count = self.sets.fetch_add(1, Ordering::SeqCst) + 1;
        if self.locked.load(Ordering::SeqCst) || count == self.fail_set_at.load(Ordering::SeqCst) {
            return Err(Code::SecretStore.into());
        }
        self.values.lock().unwrap().insert(r.into(), value.into());
        Ok(())
    }
    fn delete(&self, r: &str) -> Result<()> {
        let count = self.deletes.fetch_add(1, Ordering::SeqCst) + 1;
        if self.locked.load(Ordering::SeqCst) || count == self.fail_delete_at.load(Ordering::SeqCst)
        {
            return Err(Code::SecretStore.into());
        }
        self.values.lock().unwrap().remove(r);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Behavior {
    Success,
    Failure,
    Pending,
    Panic,
}
pub(super) struct FakeChannel {
    id: &'static str,
    behavior: Behavior,
    calls: Arc<AtomicUsize>,
    fields: &'static [Field],
}
const FIELDS: &[Field] = &[
    Field {
        id: "endpoint",
        label_key: "notifications.webhook_url",
        kind: InputKind::Url,
        required: true,
        placeholder: Some("https://example.invalid/push"),
    },
    Field {
        id: "api_key",
        label_key: "notifications.bot_token",
        kind: InputKind::Password,
        required: true,
        placeholder: None,
    },
    Field {
        id: "signing_key",
        label_key: "notifications.secret_saved",
        kind: InputKind::Password,
        required: true,
        placeholder: None,
    },
    Field {
        id: "recipient",
        label_key: "notifications.chat_id",
        kind: InputKind::Numeric,
        required: false,
        placeholder: None,
    },
];
impl FakeChannel {
    pub fn system() -> Self {
        Self {
            id: "system",
            behavior: Behavior::Success,
            calls: Arc::default(),
            fields: &[],
        }
    }
    fn fixture() -> Self {
        Self {
            id: "fixture",
            fields: FIELDS,
            ..Self::system()
        }
    }
}
impl NotificationChannel for FakeChannel {
    fn descriptor(&self) -> Descriptor {
        let system = self.id == "system";
        Descriptor {
            id: self.id,
            name_key: if system {
                "notifications.system"
            } else {
                "notifications.message_test"
            },
            description_key: system.then_some("notifications.system_desc"),
            icon: Some(if system { "monitor" } else { "future_icon" }),
            fields: self.fields,
            help_links: if system {
                &[]
            } else {
                &[HelpLink {
                    label_key: "notifications.setup_help",
                    url: "https://example.invalid/help",
                }]
            },
            enabled_by_default: system,
        }
    }
    fn validate(&self, values: &ResolvedConfig) -> Result<()> {
        if !self.fields.is_empty() && !values.get("endpoint").starts_with("https://") {
            return Err(Error::new(Code::InvalidConfig).at("endpoint"));
        }
        Ok(())
    }
    fn send<'a>(&'a self, _: &'a ResolvedConfig, _: &'a Message) -> BoxFuture<'a, Result<Receipt>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.behavior {
                Behavior::Success => Ok(Receipt::Sent),
                Behavior::Failure => Err(Code::Rejected.into()),
                Behavior::Pending => std::future::pending().await,
                Behavior::Panic => panic!("test adapter panic"),
            }
        })
    }
}
struct Fixture {
    root: PathBuf,
    secrets: Arc<MemorySecrets>,
    registry: Registry,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dqh-channels-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let base = channels::headless();
        let mut adapters: Vec<_> = base.iter().cloned().collect();
        adapters.push(Arc::new(FakeChannel::fixture()));
        Self {
            root,
            secrets: Arc::default(),
            registry: Registry::new(adapters).unwrap(),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.join("notifications.json")
    }
    fn cfg(&self) -> Config {
        Config::defaults(&self.registry)
    }
    fn update(&self, cfg: &mut Config, draft: &Draft) -> Result<()> {
        config::update(
            cfg,
            &self.path(),
            self.registry.get("fixture")?.as_ref(),
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
fn draft(fields: &[(&str, &str)]) -> Draft {
    Draft {
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}
fn valid() -> Draft {
    draft(&[
        ("endpoint", "https://example.invalid/push"),
        ("api_key", "fixture-api-value"),
        ("signing_key", "fixture-signing-value"),
        ("recipient", "18446744073709551615"),
    ])
}
pub(super) fn event(kind: EventKind) -> NotificationEvent {
    NotificationEvent {
        id: "fixture:completed".into(),
        kind,
        locale: "en".into(),
        quest_name: "Quest _[special](name)!_".into(),
        reward_image: Some("https://cdn.discordapp.com/reward.png".into()),
        estimated_complete_at: Some("2026-10-18T15:05:00Z".into()),
        succeeded: 2,
        failed: 1,
        skipped: 3,
    }
}
pub(super) fn response(status: u16, body: &str) -> reqwest::Response {
    ::http::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body.to_string())
        .unwrap()
        .into()
}
pub(super) fn resolved(fields: &[(&str, &str)]) -> ResolvedConfig {
    ResolvedConfig(
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), zeroize::Zeroizing::new(v.to_string())))
            .collect(),
    )
}

#[test]
fn registry_rejects_duplicate_channel_and_field_ids() {
    assert!(Registry::new(vec![
        Arc::new(FakeChannel::system()),
        Arc::new(FakeChannel::system())
    ])
    .is_err());
    let mut invalid = FakeChannel::fixture();
    invalid.id = "bad-id";
    assert!(Registry::new(vec![Arc::new(invalid)]).is_err());
    let mut invalid = FakeChannel::fixture();
    invalid.fields = &[
        Field {
            id: "same",
            label_key: "",
            kind: InputKind::Text,
            required: false,
            placeholder: None,
        },
        Field {
            id: "same",
            label_key: "",
            kind: InputKind::Text,
            required: false,
            placeholder: None,
        },
    ];
    assert!(Registry::new(vec![Arc::new(invalid)]).is_err());
}

#[test]
fn registered_descriptors_have_translations_in_every_supported_locale() {
    let registry = channels::headless();
    for locale in [
        "en", "zh", "zh-TW", "de", "es", "fr", "id", "ja", "ko", "pl", "pt-BR", "pt-PT", "ru",
        "th", "tr", "vi",
    ] {
        let labels = super::model::messages(locale);
        for channel in registry.iter() {
            let descriptor = channel.descriptor();
            let keys = std::iter::once(descriptor.name_key)
                .chain(descriptor.description_key)
                .chain(descriptor.fields.iter().map(|f| f.label_key))
                .chain(descriptor.help_links.iter().map(|link| link.label_key));
            for key in keys {
                let key = key
                    .strip_prefix("notifications.")
                    .expect("channel metadata belongs to notifications translations");
                assert!(
                    labels[key].as_str().is_some_and(|value| !value.is_empty()),
                    "Missing channel translation: {locale}/{key}"
                );
            }
        }
    }
}
#[test]
fn migration_preserves_event_switches_values_references_and_unknown_channels() {
    let f = Fixture::new();
    let legacy = serde_json::json!({"version":1,"events":{"quest_started":true,"quest_completed":false,"queue_completed":true},"system_enabled":false,"telegram":{"enabled":true,"secret_ref":"existing-tg-reference","target":"-100123","thread":"42"},"discord":{"enabled":true,"secret_ref":"existing-discord-reference","thread":"99"}});
    std::fs::write(f.path(), legacy.to_string()).unwrap();
    let cfg = config::load(&f.path(), &f.registry).unwrap();
    assert_eq!(cfg.version, 2);
    assert!(cfg.events.quest_started);
    assert!(!cfg.events.quest_completed);
    assert!(!cfg.channels["system"].enabled);
    assert_eq!(cfg.channels["telegram"].values["chat_id"], "-100123");
    assert_eq!(
        cfg.channels["telegram"].secret_refs["token"],
        "existing-tg-reference"
    );
    assert_eq!(
        cfg.channels["discord"].secret_refs["webhook_url"],
        "existing-discord-reference"
    );
    assert_eq!(f.secrets.sets.load(Ordering::SeqCst), 0);
    let mut cfg = cfg;
    cfg.channels.insert(
        "future".into(),
        ChannelConfig {
            enabled: true,
            values: BTreeMap::from([("option".into(), "retained".into())]),
            ..Default::default()
        },
    );
    atomic::save(&f.path(), &cfg).unwrap();
    assert_eq!(
        config::load(&f.path(), &f.registry).unwrap().channels["future"].values["option"],
        "retained"
    );
    assert!(!cfg.channels["fixture"].enabled);
}
#[test]
fn unknown_or_malformed_versions_and_migration_write_failures_do_not_reset_files() {
    let f = Fixture::new();
    for version in [
        serde_json::json!(9),
        serde_json::json!(null),
        serde_json::json!("2"),
    ] {
        let raw = serde_json::json!({"version":version}).to_string();
        std::fs::write(f.path(), &raw).unwrap();
        assert!(config::load(&f.path(), &f.registry).is_err());
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), raw);
    }
    let raw = "{\"version\":1}";
    std::fs::write(f.path(), raw).unwrap();
    std::fs::create_dir(f.path().with_extension("json.tmp")).unwrap();
    assert!(config::load(&f.path(), &f.registry).is_err());
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), raw);
}
#[test]
fn multiple_secrets_are_preserved_replaced_and_cleared_without_plaintext_persistence() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    f.update(&mut cfg, &valid()).unwrap();
    assert_eq!(f.secrets.values.lock().unwrap().len(), 2);
    let old = cfg.channels["fixture"].secret_refs.clone();
    f.update(
        &mut cfg,
        &draft(&[("api_key", ""), ("endpoint", "https://example.invalid/new")]),
    )
    .unwrap();
    assert_eq!(cfg.channels["fixture"].secret_refs, old);
    f.update(&mut cfg, &draft(&[("api_key", "replacement-api-value")]))
        .unwrap();
    assert_ne!(
        cfg.channels["fixture"].secret_refs["api_key"],
        old["api_key"]
    );
    assert_eq!(
        cfg.channels["fixture"].secret_refs["signing_key"],
        old["signing_key"]
    );
    let file = std::fs::read_to_string(f.path()).unwrap();
    for secret in [
        "fixture-api-value",
        "fixture-signing-value",
        "replacement-api-value",
    ] {
        assert!(!file.contains(secret));
    }
    assert_eq!(
        cfg.channels["fixture"].values["recipient"],
        "18446744073709551615"
    );
    config::clear(&mut cfg, &f.path(), "fixture", f.secrets.as_ref()).unwrap();
    assert!(f.secrets.values.lock().unwrap().is_empty());
    assert!(!cfg.channels["fixture"].enabled);
}
#[test]
fn failed_file_or_second_credential_write_preserves_the_old_configuration() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    f.update(&mut cfg, &valid()).unwrap();
    let before = std::fs::read(f.path()).unwrap();
    let refs = cfg.channels["fixture"].secret_refs.clone();
    f.secrets.fail_set_at.store(4, Ordering::SeqCst);
    assert!(f.update(&mut cfg, &valid()).is_err());
    assert_eq!(f.secrets.values.lock().unwrap().len(), 2);
    assert_eq!(std::fs::read(f.path()).unwrap(), before);
    let blocker = f.root.join("blocker");
    std::fs::write(&blocker, "file").unwrap();
    assert!(config::update(
        &mut cfg,
        &blocker.join("config"),
        f.registry.get("fixture").unwrap().as_ref(),
        &valid(),
        f.secrets.as_ref()
    )
    .is_err());
    assert_eq!(cfg.channels["fixture"].secret_refs, refs);
    assert_eq!(f.secrets.values.lock().unwrap().len(), 2);
}
#[test]
fn clear_commits_before_cleanup_and_file_failure_preserves_credentials() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    f.update(&mut cfg, &valid()).unwrap();
    let before = std::fs::read(f.path()).unwrap();
    let blocker = f.root.join("blocker");
    std::fs::write(&blocker, "file").unwrap();
    assert!(config::clear(
        &mut cfg,
        &blocker.join("config"),
        "fixture",
        f.secrets.as_ref()
    )
    .is_err());
    assert_eq!(f.secrets.values.lock().unwrap().len(), 2);
    assert_eq!(f.secrets.deletes.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(f.path()).unwrap(), before);
    f.secrets.locked.store(true, Ordering::SeqCst);
    assert!(f
        .update(
            &mut cfg,
            &draft(&[("endpoint", "https://example.invalid/other")])
        )
        .is_err());
    assert_eq!(std::fs::read(f.path()).unwrap(), before);
    config::clear(&mut cfg, &f.path(), "fixture", f.secrets.as_ref()).unwrap();
    assert!(cfg.channels["fixture"].secret_refs.is_empty());
    assert_eq!(f.secrets.values.lock().unwrap().len(), 2);
    assert!(
        config::load(&f.path(), &f.registry).unwrap().channels["fixture"]
            .secret_refs
            .is_empty()
    );
}

#[test]
fn clear_recovers_missing_credentials_and_tolerates_partial_cleanup_failure() {
    for missing in [true, false] {
        let f = Fixture::new();
        let mut cfg = f.cfg();
        f.update(&mut cfg, &valid()).unwrap();
        if missing {
            f.secrets
                .delete(&cfg.channels["fixture"].secret_refs["api_key"])
                .unwrap();
        } else {
            f.secrets.fail_delete_at.store(2, Ordering::SeqCst);
        }
        config::clear(&mut cfg, &f.path(), "fixture", f.secrets.as_ref()).unwrap();
        assert!(cfg.channels["fixture"].secret_refs.is_empty());
        assert!(cfg.channels["fixture"].values.is_empty());
        assert!(!cfg.channels["fixture"].enabled);
        assert_eq!(
            f.secrets.values.lock().unwrap().len(),
            usize::from(!missing)
        );
        assert!(
            config::load(&f.path(), &f.registry).unwrap().channels["fixture"]
                .secret_refs
                .is_empty()
        );
    }
}

#[tokio::test]
async fn directory_sync_failure_keeps_new_and_old_credentials_and_runtime_matches_disk() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    f.update(&mut cfg, &valid()).unwrap();
    let old_refs = cfg.channels["fixture"].secret_refs.clone();
    let registry = channels::headless();
    let mut adapters: Vec<_> = registry.iter().cloned().collect();
    adapters.push(Arc::new(FakeChannel::fixture()));
    let service = Arc::new(NotificationService::new(
        Registry::new(adapters).unwrap(),
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let error = match service
        .transaction(f.path(), |cfg, path, registry, secrets| {
            config::update_with_save(
                cfg,
                path,
                registry.get("fixture")?.as_ref(),
                &draft(&[
                    ("endpoint", "https://example.invalid/replaced"),
                    ("api_key", "new-api-value"),
                    ("signing_key", "new-signing-value"),
                ]),
                secrets,
                |path, cfg| {
                    atomic::save_with_sync(path, cfg, |_| {
                        Err(std::io::Error::other("injected sync failure"))
                    })
                },
            )
        })
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("expected storage failure"),
    };
    assert_eq!(error.code, Code::Storage);
    assert!(error.file_replaced);
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({"code":"storage"})
    );
    let disk = config::load(&f.path(), &f.registry).unwrap();
    assert_ne!(disk.channels["fixture"].secret_refs, old_refs);
    {
        let vault = f.secrets.values.lock().unwrap();
        assert_eq!(vault.len(), 4);
        for reference in old_refs
            .values()
            .chain(disk.channels["fixture"].secret_refs.values())
        {
            assert!(vault.contains_key(reference));
        }
    }
    let settings = service.settings(f.path()).await.unwrap();
    let view = settings
        .channels
        .iter()
        .find(|c| c.descriptor.id == "fixture")
        .unwrap();
    assert_eq!(view.values["endpoint"], "https://example.invalid/replaced");
}

struct PausedSecrets {
    inner: Arc<MemorySecrets>,
    started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: (Mutex<bool>, std::sync::Condvar),
}
impl Secrets for PausedSecrets {
    fn get(&self, reference: &str) -> Result<String> {
        let sender = self.started.lock().unwrap().take();
        if let Some(sender) = sender {
            let _ = sender.send(());
            let (released, signal) = &self.release;
            let (_guard, timeout) = signal
                .wait_timeout_while(released.lock().unwrap(), Duration::from_secs(3), |ready| {
                    !*ready
                })
                .unwrap();
            if timeout.timed_out() {
                return Err(Code::Timeout.into());
            }
        }
        self.inner.get(reference)
    }
    fn set(&self, reference: &str, value: &str) -> Result<()> {
        self.inner.set(reference, value)
    }
    fn delete(&self, reference: &str) -> Result<()> {
        self.inner.delete(reference)
    }
}

#[tokio::test]
async fn accepted_dispatch_and_preview_keep_credentials_until_resolution_finishes() {
    for preview in [false, true] {
        let f = Fixture::new();
        let mut cfg = f.cfg();
        f.update(&mut cfg, &valid()).unwrap();
        cfg.channels.get_mut("fixture").unwrap().enabled = true;
        atomic::save(&f.path(), &cfg).unwrap();
        let old = cfg.channels["fixture"].secret_refs.clone();
        let (started, waiting) = tokio::sync::oneshot::channel();
        let secrets = Arc::new(PausedSecrets {
            inner: f.secrets.clone(),
            started: Mutex::new(Some(started)),
            release: (Mutex::new(false), std::sync::Condvar::new()),
        });
        let registry = channels::headless();
        let mut adapters: Vec<_> = registry.iter().cloned().collect();
        adapters.push(Arc::new(FakeChannel::fixture()));
        let service = Arc::new(NotificationService::new(
            Registry::new(adapters).unwrap(),
            secrets.clone(),
            Arc::new(|_| {}),
        ));
        let sending = service.clone();
        let path = f.path();
        let delivery = tokio::spawn(async move {
            if preview {
                vec![sending
                    .test(path, "fixture".into(), Draft::default(), "en".into(), None)
                    .await
                    .unwrap()]
            } else {
                futures_util::future::join_all(
                    sending
                        .dispatch(path, event(EventKind::QuestCompleted))
                        .await
                        .unwrap(),
                )
                .await
                .into_iter()
                .map(|r| r.unwrap())
                .collect()
            }
        });
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let editing = service.clone();
        let path = f.path();
        let mut edit = tokio::spawn(async move {
            editing
                .transaction(path, move |cfg, path, registry, secrets| {
                    if preview {
                        config::clear(cfg, path, "fixture", secrets)
                    } else {
                        config::update(
                            cfg,
                            path,
                            registry.get("fixture")?.as_ref(),
                            &valid(),
                            secrets,
                        )
                    }
                })
                .await
        });
        let outcome = tokio::time::timeout(Duration::from_secs(1), &mut edit).await;
        let edited_without_waiting = outcome.is_ok();
        let old_retained = old
            .values()
            .all(|r| f.secrets.values.lock().unwrap().contains_key(r));
        *secrets.release.0.lock().unwrap() = true;
        secrets.release.1.notify_all();
        match outcome {
            Ok(result) => {
                result.unwrap().unwrap();
            }
            Err(_) => {
                edit.await.unwrap().unwrap();
            }
        }
        let results = delivery.await.unwrap();
        assert!(
            edited_without_waiting,
            "settings edits must not wait for accepted reads"
        );
        assert!(
            old_retained,
            "accepted reads retain the old credential snapshot"
        );
        assert!(results.iter().all(|r| r.success));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if old
                    .values()
                    .all(|r| !f.secrets.values.lock().unwrap().contains_key(r))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn settings_edits_do_not_wait_for_network_delivery() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    f.update(&mut cfg, &valid()).unwrap();
    cfg.channels.get_mut("fixture").unwrap().enabled = true;
    atomic::save(&f.path(), &cfg).unwrap();
    let mut channel = FakeChannel::fixture();
    channel.behavior = Behavior::Pending;
    let calls = channel.calls.clone();
    let registry = Registry::new(vec![Arc::new(channel)]).unwrap();
    let service = Arc::new(NotificationService::new(
        registry,
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let handles = service
        .dispatch(f.path(), event(EventKind::QuestCompleted))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let editing = tokio::time::timeout(
        Duration::from_secs(1),
        service.transaction(f.path(), |cfg, path, registry, secrets| {
            config::update(
                cfg,
                path,
                registry.get("fixture")?.as_ref(),
                &valid(),
                secrets,
            )
        }),
    )
    .await;
    for handle in handles {
        handle.abort();
    }
    assert!(editing.unwrap().is_ok());
}

#[test]
fn required_fields_and_unknown_draft_keys_are_rejected_before_persistence() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    let error = f
        .update(
            &mut cfg,
            &draft(&[("endpoint", "https://example.invalid/push")]),
        )
        .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("api_key"));
    assert!(f
        .update(&mut cfg, &draft(&[("unregistered", "value")]))
        .is_err());
    assert!(!f.path().exists());
}

#[tokio::test]
async fn fourth_channel_settings_and_test_contract_match_shared_frontend_fixture() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    f.update(&mut cfg, &valid()).unwrap();
    let registry = channels::headless();
    let mut adapters: Vec<_> = registry.iter().cloned().collect();
    adapters.push(Arc::new(FakeChannel::fixture()));
    let service = Arc::new(NotificationService::new(
        Registry::new(adapters).unwrap(),
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let settings = service.settings(f.path()).await.unwrap();
    let actual = serde_json::to_value(&settings).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/notification-channels.json"
    ))
    .unwrap();
    assert_eq!(actual, fixture);
    let serialized = actual.to_string();
    assert!(!serialized.contains("fixture-api-value"));
    assert!(!serialized.contains("fixture-signing-value"));
    let before = std::fs::read(f.path()).unwrap();
    let result = service
        .test(
            f.path(),
            "fixture".into(),
            draft(&[("endpoint", "https://example.invalid/test-only")]),
            "zh".into(),
            None,
        )
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(std::fs::read(f.path()).unwrap(), before);
    service
        .set_enabled(f.path(), "fixture".into(), true)
        .await
        .unwrap();
    let handles = service
        .dispatch(f.path(), event(EventKind::QuestCompleted))
        .await
        .unwrap();
    let results = futures_util::future::join_all(handles).await;
    assert!(results
        .iter()
        .any(|r| r.as_ref().unwrap().channel == "fixture" && r.as_ref().unwrap().success));
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
    assert!(service
        .test(
            f.path(),
            "missing".into(),
            Draft::default(),
            "en".into(),
            None
        )
        .await
        .is_err());
}

#[tokio::test]
async fn failing_pending_and_panicking_channels_do_not_block_other_deliveries() {
    let f = Fixture::new();
    let mut adapters: Vec<Arc<dyn NotificationChannel>> = Vec::new();
    for (id, behavior) in [
        ("success", Behavior::Success),
        ("failure", Behavior::Failure),
        ("slow", Behavior::Pending),
        ("panic", Behavior::Panic),
    ] {
        adapters.push(Arc::new(FakeChannel {
            id,
            behavior,
            fields: &[],
            calls: Arc::default(),
        }));
    }
    let registry = Registry::new(adapters).unwrap();
    let service = Arc::new(NotificationService::new(
        registry,
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let message = message::render(&event(EventKind::QuestCompleted), false);
    let attempts = ["success", "failure", "slow", "panic"].map(|id| {
        service.deliver_with_timeout(
            id.into(),
            ChannelConfig::default(),
            Draft::default(),
            message.clone(),
            Duration::from_millis(100),
        )
    });
    let results = futures_util::future::join_all(attempts).await;
    assert!(results[0].success);
    assert_eq!(results[1].code, "rejected");
    assert_eq!(results[2].code, "timeout");
    assert_eq!(results[3].code, "unknown");
    assert_eq!(service.settings(f.path()).await.unwrap().results.len(), 4);
}

#[tokio::test]
async fn dispatch_skips_unknown_channels_and_reports_invalid_enabled_configuration_independently() {
    let f = Fixture::new();
    let mut cfg = f.cfg();
    cfg.channels.get_mut("telegram").unwrap().enabled = true;
    cfg.channels.insert(
        "future".into(),
        ChannelConfig {
            enabled: true,
            ..Default::default()
        },
    );
    atomic::save(&f.path(), &cfg).unwrap();
    let service = Arc::new(NotificationService::new(
        channels::headless(),
        f.secrets.clone(),
        Arc::new(|_| {}),
    ));
    let handles = service
        .dispatch(f.path(), event(EventKind::QuestCompleted))
        .await
        .unwrap();
    assert_eq!(handles.len(), 2);
    let results: Vec<_> = futures_util::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert!(results.iter().any(|r| r.channel == "system" && r.success));
    assert!(results
        .iter()
        .any(|r| r.channel == "telegram" && !r.success && r.code == "not_configured"));
    assert!(config::load(&f.path(), &f.registry).unwrap().channels["future"].enabled);
}

#[test]
fn all_locales_render_summaries_and_frontend_test_requests_deserialize() {
    for locale in [
        "en", "zh", "zh-TW", "de", "es", "fr", "id", "ja", "ko", "pl", "pt-BR", "pt-PT", "ru",
        "th", "tr", "vi",
    ] {
        let mut e = event(EventKind::QueueCompleted);
        e.locale = locale.into();
        let message = message::render(&e, false);
        assert!(!message.title.is_empty());
        assert!(message.plain_body().contains('2'));
        assert!(!message.plain_body().contains('{'));
    }
    let requests: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/notification-test-requests.json"
    ))
    .unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests {
        let _: Draft = serde_json::from_value(request["draft"].clone()).unwrap();
        let event: Option<NotificationEvent> =
            serde_json::from_value(request["event"].clone()).unwrap();
        if let Some(event) = event {
            assert_eq!(event.locale, request["locale"]);
        }
    }
}
