//! AI assistance for Docufy: which provider answers ([`Provider`]), the settings the user chose
//! ([`AiSettings`], off until they turn it on), what this computer can run ([`Hardware`],
//! [`DeviceKind`]), the warnings shown before and while AI is used ([`warnings`]), and the
//! requests and streamed answers of each provider's chat API ([`chat`]).
//!
//! Nothing here sends anything by itself: the front end asks, with the user's consent, and API
//! keys live in the operating system's credential store ([`secrets`]), never in settings files.
//! The pure parts (settings, warnings, request bodies, stream parsing) build for every target; the
//! network, credential store and hardware probe only on the desktop.

pub mod chat;
pub mod hardware;
pub mod provider;
pub mod settings;
pub mod warnings;

#[cfg(not(target_arch = "wasm32"))]
pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod probe;
#[cfg(not(target_arch = "wasm32"))]
pub mod secrets;

pub use chat::{ChatRequest, Message, Role, StopReason, StreamEvent};
pub use hardware::{DeviceKind, DevicePreference, Gpu, Hardware, LocalModel, Npu};
pub use provider::Provider;
pub use settings::AiSettings;
pub use warnings::{Severity, Warning};

/// Why a request to a provider failed, sorted by what the user can do about it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    /// The key is missing, wrong or revoked (401/403).
    #[error("the provider did not accept the API key: {0}")]
    Auth(String),
    /// Too many requests or no credit left (429, 402): wait, or check the account.
    #[error("the provider's limit was reached: {0}")]
    RateLimit(String),
    /// The provider is busy or failing (5xx, 529): try again later.
    #[error("the provider is unavailable: {0}")]
    Unavailable(String),
    /// The request was refused as invalid (400, 404): usually a model name the account can't use.
    #[error("the provider rejected the request: {0}")]
    Rejected(String),
    /// No connection, or it broke off.
    #[error("no connection to the provider: {0}")]
    Network(String),
    /// The answer could not be read.
    #[error("the provider's answer could not be read: {0}")]
    Protocol(String),
    /// Stopped by the user.
    #[error("cancelled")]
    Cancelled,
}

impl AiError {
    /// Whether trying the same request again later may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, AiError::RateLimit(_) | AiError::Unavailable(_) | AiError::Network(_))
    }

    /// The error for an HTTP status and the provider's error text.
    pub fn from_status(status: u16, message: String) -> AiError {
        match status {
            401 | 403 => AiError::Auth(message),
            402 | 429 => AiError::RateLimit(message),
            500..=599 => AiError::Unavailable(message),
            _ => AiError::Rejected(message),
        }
    }
}
