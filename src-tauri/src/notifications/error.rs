use serde::Serialize;

/// Safe, closed error vocabulary. Never attach provider bodies, URLs or credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    Storage,
    SecretStore,
    NotConfigured,
    InvalidChannel,
    InvalidConfig,
    InvalidEvent,
    InvalidWebhook,
    InvalidThread,
    InvalidToken,
    InvalidTarget,
    SystemUnavailable,
    Network,
    Timeout,
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited,
    Rejected,
    InvalidResponse,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Error {
    pub code: Code,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// The file was replaced, but durability could not be confirmed.
    /// Keep both generations of credentials and align runtime state with disk.
    #[serde(skip)]
    pub(super) file_replaced: bool,
}
impl Error {
    pub fn new(code: Code) -> Self {
        Self {
            code,
            field: None,
            file_replaced: false,
        }
    }
    pub fn at(mut self, field: &str) -> Self {
        self.field = Some(field.into());
        self
    }
}
impl From<Code> for Error {
    fn from(code: Code) -> Self {
        Self::new(code)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
