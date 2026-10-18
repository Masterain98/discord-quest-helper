use super::{
    atomic::save,
    channel::{
        validate_required, Descriptor, Draft, InputKind, NotificationChannel, Registry,
        ResolvedConfig,
    },
    error::{Code, Result},
    model::Events,
    secrets::Secrets,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use zeroize::Zeroizing;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct ChannelConfig {
    pub enabled: bool,
    pub values: BTreeMap<String, String>,
    pub secret_refs: BTreeMap<String, String>,
}
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Config {
    pub version: u8,
    pub events: Events,
    pub channels: BTreeMap<String, ChannelConfig>,
}
impl Config {
    pub fn defaults(registry: &Registry) -> Self {
        Self {
            version: 2,
            events: Events::default(),
            channels: registry
                .iter()
                .map(|c| {
                    let d = c.descriptor();
                    (
                        d.id.into(),
                        ChannelConfig {
                            enabled: d.enabled_by_default,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        }
    }
}

pub(super) fn load(path: &Path, registry: &Registry) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::defaults(registry));
    }
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|_| Code::Storage)?)
            .map_err(|_| Code::Storage)?;
    let version = match raw.get("version") {
        None => 1,
        Some(value) => value.as_u64().ok_or(Code::Storage)?,
    };
    match version {
        1 => {
            let legacy: LegacyConfig = serde_json::from_value(raw).map_err(|_| Code::Storage)?;
            let mut config = Config::defaults(registry);
            config.events = legacy.events;
            config.channels.insert(
                "system".into(),
                ChannelConfig {
                    enabled: legacy.system_enabled,
                    ..Default::default()
                },
            );
            for (id, value, secret, target, thread) in [
                ("telegram", legacy.telegram, "token", "chat_id", "topic_id"),
                ("discord", legacy.discord, "webhook_url", "", "thread_id"),
            ] {
                let mut values = BTreeMap::from([(thread.into(), value.thread)]);
                if !target.is_empty() {
                    values.insert(target.into(), value.target);
                }
                config.channels.insert(
                    id.into(),
                    ChannelConfig {
                        enabled: value.enabled,
                        values,
                        secret_refs: value
                            .secret_ref
                            .map(|r| BTreeMap::from([(secret.into(), r)]))
                            .unwrap_or_default(),
                    },
                );
            }
            // Migration never reads or rewrites credentials; replace the file atomically.
            save(path, &config)?;
            Ok(config)
        }
        2 => serde_json::from_value(raw).map_err(|_| Code::Storage.into()),
        _ => Err(Code::Storage.into()),
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct LegacyConfig {
    events: Events,
    system_enabled: bool,
    telegram: LegacyNetwork,
    discord: LegacyNetwork,
}
impl Default for LegacyConfig {
    fn default() -> Self {
        Self {
            events: Events::default(),
            system_enabled: true,
            telegram: LegacyNetwork::default(),
            discord: LegacyNetwork::default(),
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct LegacyNetwork {
    enabled: bool,
    secret_ref: Option<String>,
    target: String,
    thread: String,
}

pub(super) fn configured(d: &Descriptor, saved: &ChannelConfig) -> bool {
    d.fields.iter().filter(|f| f.required).all(|f| {
        if f.kind == InputKind::Password {
            saved.secret_refs.contains_key(f.id)
        } else {
            saved.values.get(f.id).is_some_and(|v| !v.trim().is_empty())
        }
    })
}

pub(super) fn resolve(
    d: &Descriptor,
    saved: &ChannelConfig,
    draft: &Draft,
    secrets: &dyn Secrets,
) -> Result<ResolvedConfig> {
    if draft
        .fields
        .keys()
        .any(|key| !d.fields.iter().any(|f| f.id == key))
    {
        return Err(Code::InvalidConfig.into());
    }
    let mut values = BTreeMap::new();
    for field in d.fields {
        let input = draft.fields.get(field.id).map(|v| v.trim());
        let value = if field.kind == InputKind::Password {
            match input.filter(|v| !v.is_empty()) {
                Some(value) => value.into(),
                None => saved
                    .secret_refs
                    .get(field.id)
                    .map(|r| secrets.get(r))
                    .transpose()?
                    .unwrap_or_default(),
            }
        } else {
            input
                .or_else(|| saved.values.get(field.id).map(String::as_str))
                .unwrap_or("")
                .trim()
                .into()
        };
        values.insert(field.id.into(), Zeroizing::new(value));
    }
    let resolved = ResolvedConfig(values);
    validate_required(d, &resolved)?;
    Ok(resolved)
}

/// Commit a candidate configuration before updating the runtime. New credentials
/// are staged separately; failure leaves the old file and old references intact.
pub(super) fn update(
    cfg: &mut Config,
    path: &Path,
    channel: &dyn NotificationChannel,
    draft: &Draft,
    secrets: &dyn Secrets,
) -> Result<()> {
    let d = channel.descriptor();
    let old = cfg.channels.get(d.id).cloned().unwrap_or_default();
    let values = resolve(&d, &old, draft, secrets)?;
    channel.validate(&values)?;
    let mut next = old.clone();
    let mut staged: Vec<String> = Vec::new();
    for field in d.fields {
        if field.kind == InputKind::Password {
            next.values.remove(field.id);
            if draft
                .fields
                .get(field.id)
                .is_some_and(|v| !v.trim().is_empty())
            {
                let reference = uuid::Uuid::new_v4().to_string();
                if let Err(error) = secrets.set(&reference, values.get(field.id)) {
                    for r in staged {
                        let _ = secrets.delete(&r);
                    }
                    return Err(error);
                }
                next.secret_refs.insert(field.id.into(), reference.clone());
                staged.push(reference);
            }
        } else {
            next.values
                .insert(field.id.into(), values.get(field.id).into());
        }
    }
    let mut candidate = cfg.clone();
    candidate.channels.insert(d.id.into(), next.clone());
    if let Err(error) = save(path, &candidate) {
        for r in staged {
            let _ = secrets.delete(&r);
        }
        return Err(error);
    }
    *cfg = candidate;
    for (field, reference) in old.secret_refs {
        if next.secret_refs.get(&field) != Some(&reference) {
            let _ = secrets.delete(&reference);
        }
    }
    Ok(())
}

pub(super) fn clear(cfg: &mut Config, path: &Path, id: &str, secrets: &dyn Secrets) -> Result<()> {
    let old = cfg.channels.get(id).cloned().unwrap_or_default();
    let credentials: Vec<_> = old
        .secret_refs
        .values()
        .map(|r| Ok((r.clone(), Zeroizing::new(secrets.get(r)?))))
        .collect::<Result<_>>()?;
    let restore = || -> Result<()> {
        for (r, value) in &credentials {
            secrets.set(r, value)?;
        }
        Ok(())
    };
    for (r, _) in &credentials {
        if let Err(error) = secrets.delete(r) {
            restore()?;
            return Err(error);
        }
    }
    let mut candidate = cfg.clone();
    candidate
        .channels
        .insert(id.into(), ChannelConfig::default());
    if let Err(error) = save(path, &candidate) {
        restore()?;
        return Err(error);
    }
    *cfg = candidate;
    Ok(())
}
