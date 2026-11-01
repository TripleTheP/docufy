//! The AI providers Docufy can use: five cloud APIs, each reached with the user's own API key,
//! and a model running on this computer.

use serde::{Deserialize, Serialize};

/// Who answers a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    /// Anthropic's Claude (Messages API).
    #[default]
    Anthropic,
    /// OpenAI's GPT models (Chat Completions API).
    OpenAi,
    /// Google's Gemini (Gemini API).
    Gemini,
    /// Mistral AI, hosted in the EU (Chat Completions API).
    Mistral,
    /// OpenAI models in the user's own Microsoft Azure resource: the models behind Microsoft
    /// Copilot, without a Copilot licence.
    AzureOpenAi,
    /// A model running on this computer; nothing leaves it.
    Local,
}

impl Provider {
    /// Every provider, in menu order.
    pub const ALL: [Provider; 6] =
        [Provider::Anthropic, Provider::OpenAi, Provider::Gemini, Provider::Mistral, Provider::AzureOpenAi, Provider::Local];

    /// The id used in settings, commands and the credential store.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "open-ai",
            Provider::Gemini => "gemini",
            Provider::Mistral => "mistral",
            Provider::AzureOpenAi => "azure-open-ai",
            Provider::Local => "local",
        }
    }

    /// The provider with id `id`.
    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The name shown in the provider menu (English; the interface translates it).
    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic (Claude)",
            Provider::OpenAi => "OpenAI (ChatGPT)",
            Provider::Gemini => "Google (Gemini)",
            Provider::Mistral => "Mistral (EU)",
            Provider::AzureOpenAi => "Microsoft (Azure OpenAI)",
            Provider::Local => "On this computer",
        }
    }

    /// Whether requests leave this computer.
    pub fn is_cloud(self) -> bool {
        self != Provider::Local
    }

    /// Whether the provider needs an API key.
    pub fn needs_key(self) -> bool {
        self.is_cloud()
    }

    /// The provider's page where the user signs in and creates an API key.
    pub fn key_page(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("https://console.anthropic.com/settings/keys"),
            Provider::OpenAi => Some("https://platform.openai.com/api-keys"),
            Provider::Gemini => Some("https://aistudio.google.com/app/apikey"),
            Provider::Mistral => Some("https://console.mistral.ai/api-keys"),
            Provider::AzureOpenAi => Some("https://portal.azure.com/"),
            Provider::Local => None,
        }
    }

    /// The model preselected for the provider; `None` where the account decides (Azure
    /// deployments) or the list is loaded from the provider first.
    pub fn default_model(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("claude-opus-5-5"),
            Provider::Mistral => Some("mistral-large-latest"),
            Provider::Gemini => Some("gemini-flash-latest"),
            Provider::Local => Some(crate::local::catalog::DEFAULT_MODEL),
            Provider::OpenAi | Provider::AzureOpenAi => None,
        }
    }

    /// Where the provider processes requests, for the privacy notice.
    pub fn region(self) -> &'static str {
        match self {
            Provider::Mistral => "EU",
            Provider::AzureOpenAi => "the region of your Azure resource",
            Provider::Local => "this computer",
            Provider::Anthropic | Provider::OpenAi | Provider::Gemini => "USA",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_are_unique() {
        for p in Provider::ALL {
            assert_eq!(Provider::from_id(p.id()), Some(p));
            assert_eq!(serde_json::to_value(p).unwrap(), serde_json::json!(p.id()));
        }
        assert_eq!(Provider::from_id("copilot"), None);
    }

    #[test]
    fn only_cloud_providers_need_a_key_page() {
        for p in Provider::ALL {
            assert_eq!(p.needs_key(), p.key_page().is_some(), "{p:?}");
            if let Some(url) = p.key_page() {
                assert!(url.starts_with("https://"), "{url}");
            }
        }
    }
}
