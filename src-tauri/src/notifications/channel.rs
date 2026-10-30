use super::{
    error::{Code, Error, Result},
    message::Message,
};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Text,
    Password,
    #[allow(dead_code)]
    // Available to new adapters; current built-ins keep URLs in password fields.
    Url,
    Numeric,
    Textarea,
}

#[derive(Clone, Serialize)]
pub struct TemplateField {
    pub event: super::model::EventKind,
    pub part: &'static str,
    pub default_key: &'static str,
}
#[derive(Clone, Serialize)]
pub struct TemplateVariable {
    pub name: &'static str,
    pub description_key: &'static str,
    pub events: &'static [super::model::EventKind],
}
#[derive(Clone, Serialize)]
pub struct Field {
    pub id: &'static str,
    pub label_key: &'static str,
    pub kind: InputKind,
    pub required: bool,
    pub placeholder: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<TemplateField>,
}
#[derive(Clone, Serialize)]
pub struct HelpLink {
    pub label_key: &'static str,
    pub url: &'static str,
}
#[derive(Clone, Serialize)]
pub struct Descriptor {
    pub id: &'static str,
    pub name_key: &'static str,
    pub description_key: Option<&'static str>,
    pub icon: Option<&'static str>,
    pub fields: &'static [Field],
    pub help_links: &'static [HelpLink],
    #[serde(skip_serializing_if = "<[TemplateVariable]>::is_empty")]
    pub template_variables: &'static [TemplateVariable],
    #[serde(skip)]
    pub enabled_by_default: bool,
}

/// Draft values are never serialized back to the UI or logged.
#[derive(Clone, Default, Deserialize)]
pub struct Draft {
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub reset_fields: Vec<String>,
}
impl Drop for Draft {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.fields.values_mut().for_each(String::zeroize);
    }
}

/// Deliberately lacks Serialize and Debug. Only one channel's values are supplied.
pub struct ResolvedConfig(pub(super) BTreeMap<String, Zeroizing<String>>);
impl ResolvedConfig {
    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(|v| v.as_str()).unwrap_or("")
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Receipt {
    Sent,
    Submitted,
}

/// Compile-time extension point. Adapters own protocol validation and formatting;
/// they cannot mutate notification runtime/configuration through this interface.
pub trait NotificationChannel: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn validate(&self, config: &ResolvedConfig) -> Result<()>;
    fn send<'a>(
        &'a self,
        config: &'a ResolvedConfig,
        message: &'a Message,
    ) -> BoxFuture<'a, Result<Receipt>>;
}

pub struct Registry {
    channels: Vec<Arc<dyn NotificationChannel>>,
}
impl Registry {
    pub fn new(channels: Vec<Arc<dyn NotificationChannel>>) -> Result<Self> {
        let mut ids = HashSet::new();
        for channel in &channels {
            let d = channel.descriptor();
            let mut fields = HashSet::new();
            if !identifier(d.id)
                || !ids.insert(d.id)
                || d.fields
                    .iter()
                    .any(|f| !identifier(f.id) || !fields.insert(f.id))
            {
                return Err(Code::InvalidConfig.into());
            }
        }
        Ok(Self { channels })
    }
    pub fn get(&self, id: &str) -> Result<Arc<dyn NotificationChannel>> {
        self.channels
            .iter()
            .find(|c| c.descriptor().id == id)
            .cloned()
            .ok_or_else(|| Code::InvalidChannel.into())
    }
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn NotificationChannel>> {
        self.channels.iter()
    }
}
fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

pub fn validate_required(descriptor: &Descriptor, config: &ResolvedConfig) -> Result<()> {
    for field in descriptor.fields {
        if field.required && config.get(field.id).trim().is_empty() {
            return Err(Error::new(Code::NotConfigured).at(field.id));
        }
    }
    Ok(())
}
