use super::{
    atomic::save,
    channel::{Descriptor, Draft, InputKind, Receipt, Registry},
    config::{self, ChannelConfig, Config},
    error::{Code, Result},
    message::{self, Message},
    model::{Events, NotificationEvent},
    secrets::Secrets,
};
use futures_util::FutureExt;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);
#[derive(Clone, Serialize)]
pub struct DeliveryResult {
    pub channel: String,
    pub success: bool,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub at: String,
}
#[derive(Serialize)]
pub struct ChannelView {
    #[serde(flatten)]
    pub descriptor: Descriptor,
    pub enabled: bool,
    pub configured: bool,
    pub values: BTreeMap<String, String>,
    pub secret_fields_set: Vec<String>,
}
#[derive(Serialize)]
pub struct Settings {
    pub events: Events,
    pub channels: Vec<ChannelView>,
    pub results: BTreeMap<String, DeliveryResult>,
}
#[derive(Default)]
struct Runtime {
    config: Option<Config>,
    results: BTreeMap<String, DeliveryResult>,
    seen: HashSet<String>,
}
impl Runtime {
    fn claim(&mut self, event: &NotificationEvent) -> Option<Config> {
        let config = self.config.as_ref()?;
        if !config.events.allows(event.kind) || !self.seen.insert(event.id.clone()) {
            return None;
        }
        Some(config.clone())
    }
}
type ResultSink = Arc<dyn Fn(&DeliveryResult) + Send + Sync>;
pub struct NotificationService {
    registry: Arc<Registry>,
    secrets: Arc<dyn Secrets>,
    runtime: Mutex<Runtime>,
    sink: ResultSink,
}
impl NotificationService {
    pub(super) fn new(registry: Registry, secrets: Arc<dyn Secrets>, sink: ResultSink) -> Self {
        Self {
            registry: Arc::new(registry),
            secrets,
            runtime: Mutex::default(),
            sink,
        }
    }
    fn view(&self, cfg: &Config, results: &BTreeMap<String, DeliveryResult>) -> Settings {
        Settings {
            events: cfg.events.clone(),
            results: results.clone(),
            channels: self
                .registry
                .iter()
                .map(|c| {
                    let d = c.descriptor();
                    let saved = cfg.channels.get(d.id).cloned().unwrap_or(ChannelConfig {
                        enabled: d.enabled_by_default,
                        ..Default::default()
                    });
                    // Whitelist descriptor fields. Password values can never appear in views.
                    let values = d
                        .fields
                        .iter()
                        .filter(|f| f.kind != InputKind::Password)
                        .map(|f| {
                            (
                                f.id.into(),
                                saved.values.get(f.id).cloned().unwrap_or_default(),
                            )
                        })
                        .collect();
                    let secret_fields_set = d
                        .fields
                        .iter()
                        .filter(|f| {
                            f.kind == InputKind::Password && saved.secret_refs.contains_key(f.id)
                        })
                        .map(|f| f.id.into())
                        .collect();
                    ChannelView {
                        enabled: saved.enabled,
                        configured: config::configured(&d, &saved),
                        values,
                        secret_fields_set,
                        descriptor: d,
                    }
                })
                .collect(),
        }
    }
    pub(super) async fn transaction<F>(
        self: &Arc<Self>,
        path: PathBuf,
        operation: F,
    ) -> Result<Settings>
    where
        F: FnOnce(&mut Config, &Path, &Registry, &dyn Secrets) -> Result<()> + Send + 'static,
    {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut runtime = service.runtime.lock().map_err(|_| Code::Storage)?;
            if runtime.config.is_none() {
                runtime.config = Some(config::load(&path, &service.registry)?);
            }
            let mut next = runtime.config.as_ref().ok_or(Code::Storage)?.clone();
            operation(
                &mut next,
                &path,
                &service.registry,
                service.secrets.as_ref(),
            )?;
            let settings = service.view(&next, &runtime.results);
            runtime.config = Some(next);
            Ok(settings)
        })
        .await
        .map_err(|_| Code::Storage)?
    }
    pub(super) async fn settings(self: &Arc<Self>, path: PathBuf) -> Result<Settings> {
        self.transaction(path, |_, _, _, _| Ok(())).await
    }
    pub(super) async fn set_enabled(
        self: &Arc<Self>,
        path: PathBuf,
        id: String,
        enabled: bool,
    ) -> Result<Settings> {
        self.transaction(path, move |cfg, path, registry, secrets| {
            let channel = registry.get(&id)?;
            let saved = cfg.channels.entry(id).or_default();
            if enabled {
                let resolved =
                    config::resolve(&channel.descriptor(), saved, &Draft::default(), secrets)?;
                channel.validate(&resolved)?;
            }
            saved.enabled = enabled;
            save(path, cfg)
        })
        .await
    }
    pub(super) async fn test(
        self: &Arc<Self>,
        path: PathBuf,
        id: String,
        draft: Draft,
        locale: String,
        event: Option<NotificationEvent>,
    ) -> Result<DeliveryResult> {
        self.registry.get(&id)?;
        self.settings(path).await?;
        let saved = self
            .runtime
            .lock()
            .map_err(|_| Code::Storage)?
            .config
            .as_ref()
            .ok_or(Code::Storage)?
            .channels
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let generic_test = event.is_none();
        let mut event = event.unwrap_or_else(|| NotificationEvent::sample(locale.clone()));
        event.locale = locale;
        let mut message = message::render(&event, generic_test);
        message.is_test = true;
        Ok(self.deliver(id, saved, draft, message).await)
    }
    pub(super) async fn dispatch(
        self: &Arc<Self>,
        path: PathBuf,
        event: NotificationEvent,
    ) -> Result<Vec<tokio::task::JoinHandle<DeliveryResult>>> {
        if event.id.is_empty() || event.id.len() > 200 {
            return Err(Code::InvalidEvent.into());
        }
        self.settings(path).await?;
        let config = match self
            .runtime
            .lock()
            .map_err(|_| Code::Storage)?
            .claim(&event)
        {
            Some(cfg) => cfg,
            None => return Ok(vec![]),
        };
        let message = message::render(&event, false);
        let mut pending = Vec::new();
        for channel in self.registry.iter() {
            let d = channel.descriptor();
            let saved = config.channels.get(d.id).cloned().unwrap_or(ChannelConfig {
                enabled: d.enabled_by_default,
                ..Default::default()
            });
            if !saved.enabled {
                continue;
            }
            let (service, id, message) = (self.clone(), d.id.to_string(), message.clone());
            pending.push(tokio::spawn(async move {
                service.deliver(id, saved, Draft::default(), message).await
            }));
        }
        Ok(pending)
    }
    async fn deliver(
        self: &Arc<Self>,
        id: String,
        saved: ChannelConfig,
        draft: Draft,
        message: Message,
    ) -> DeliveryResult {
        self.deliver_with_timeout(id, saved, draft, message, DELIVERY_TIMEOUT)
            .await
    }
    pub(super) async fn deliver_with_timeout(
        self: &Arc<Self>,
        id: String,
        saved: ChannelConfig,
        draft: Draft,
        message: Message,
        deadline: Duration,
    ) -> DeliveryResult {
        let attempt = async {
            let channel = self.registry.get(&id)?;
            let (adapter, secrets) = (channel.clone(), self.secrets.clone());
            let resolved = tokio::task::spawn_blocking(move || {
                let resolved =
                    config::resolve(&adapter.descriptor(), &saved, &draft, secrets.as_ref())?;
                adapter.validate(&resolved)?;
                Ok::<_, super::error::Error>(resolved)
            })
            .await
            .map_err(|_| Code::Unknown)??;
            channel.send(&resolved, &message).await
        };
        let outcome =
            match tokio::time::timeout(deadline, AssertUnwindSafe(attempt).catch_unwind()).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) => Err(Code::Unknown.into()),
                Err(_) => Err(Code::Timeout.into()),
            };
        let (success, code, field) = match outcome {
            Ok(Receipt::Sent) => (true, "sent".to_string(), None),
            Ok(Receipt::Submitted) => (true, "submitted".to_string(), None),
            Err(e) => (
                false,
                serde_json::to_value(e.code)
                    .expect("error codes are serializable")
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                e.field,
            ),
        };
        let result = DeliveryResult {
            channel: id.clone(),
            success,
            code,
            field,
            at: chrono::Utc::now().to_rfc3339(),
        };
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.results.insert(id, result.clone());
        }
        (self.sink)(&result);
        result
    }
}
