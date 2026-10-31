//! The user's AI settings. AI is off until the user turns it on, and API keys are never part of
//! these settings (see [`crate::secrets`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::hardware::DevicePreference;
use crate::provider::Provider;

/// The Azure OpenAI API version requests use unless the user sets another.
pub const AZURE_API_VERSION: &str = "2024-10-21";

/// Longest answer asked for, in tokens, unless the user sets another.
pub const MAX_OUTPUT_TOKENS: u32 = 8192;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// AI features are shown and used at all. Off by default.
    pub enabled: bool,
    /// Who answers.
    pub provider: Provider,
    /// The model chosen for each provider, by provider id. A provider without an entry uses its
    /// [`Provider::default_model`].
    pub models: BTreeMap<String, String>,
    /// Azure OpenAI: the resource's endpoint, `https://<name>.openai.azure.com`.
    pub azure_endpoint: String,
    /// Azure OpenAI: the API version.
    pub azure_api_version: String,
    /// Where local models run.
    pub device: DevicePreference,
    /// Longest answer asked for, in tokens.
    pub max_output_tokens: u32,
    /// Local models pause on battery instead of draining it.
    pub pause_on_battery: bool,
    /// Notices the user has read and asked not to see again, by [`crate::Warning::id`].
    pub dismissed: BTreeSet<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            enabled: false,
            provider: Provider::default(),
            models: BTreeMap::new(),
            azure_endpoint: String::new(),
            azure_api_version: AZURE_API_VERSION.to_string(),
            device: DevicePreference::Auto,
            max_output_tokens: MAX_OUTPUT_TOKENS,
            pause_on_battery: true,
            dismissed: BTreeSet::new(),
        }
    }
}

impl AiSettings {
    /// The model used with `provider`: the one chosen, else the provider's default.
    pub fn model_for(&self, provider: Provider) -> Option<String> {
        self.models
            .get(provider.id())
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .or_else(|| provider.default_model().map(str::to_string))
    }

    /// The model used now.
    pub fn model(&self) -> Option<String> {
        self.model_for(self.provider)
    }

    /// Settings read leniently: unknown or broken values fall back to their defaults, so a
    /// damaged settings file never turns AI on or loses the other settings.
    pub fn from_json(text: &str) -> AiSettings {
        Self::from_value(serde_json::from_str(text).unwrap_or_default())
    }

    /// [`Self::from_json`] for an already parsed value.
    pub fn from_value(value: serde_json::Value) -> AiSettings {
        let mut s: AiSettings = serde_json::from_value(value).unwrap_or_default();
        if s.max_output_tokens == 0 {
            s.max_output_tokens = MAX_OUTPUT_TOKENS;
        }
        if s.azure_api_version.trim().is_empty() {
            s.azure_api_version = AZURE_API_VERSION.to_string();
        }
        s
    }
}

/// For `#[serde(deserialize_with = "docufy_ai::settings::lenient")]` on a field holding
/// [`AiSettings`] inside a larger settings file: a broken AI section gives the defaults (AI off)
/// instead of failing the whole file.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<AiSettings, D::Error> {
    let value = serde_json::Value::deserialize(d).unwrap_or_default();
    Ok(AiSettings::from_value(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::DeviceKind;

    #[test]
    fn a_broken_section_leaves_the_rest_of_the_file() {
        #[derive(Deserialize)]
        struct File {
            #[serde(deserialize_with = "lenient")]
            ai: AiSettings,
            other: u32,
        }
        let f: File = serde_json::from_str(r#"{"ai": [1, 2], "other": 7}"#).unwrap();
        assert_eq!((f.ai, f.other), (AiSettings::default(), 7));
    }

    #[test]
    fn ai_is_off_until_turned_on() {
        assert!(!AiSettings::default().enabled);
        assert!(!AiSettings::from_json("").enabled);
        assert!(!AiSettings::from_json("{\"enabled\": \"yes\"}").enabled, "a broken value never turns AI on");
        assert!(AiSettings::from_json("{\"enabled\": true}").enabled);
    }

    #[test]
    fn settings_round_trip_without_keys() {
        let mut s = AiSettings { enabled: true, provider: Provider::Mistral, ..AiSettings::default() };
        s.models.insert("mistral".into(), "mistral-small-latest".into());
        s.device = DevicePreference::Only(DeviceKind::Npu);
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.to_lowercase().contains("key"), "no key in the settings file: {json}");
        assert_eq!(AiSettings::from_json(&json), s);
    }

    #[test]
    fn a_chosen_model_wins_over_the_default() {
        let mut s = AiSettings::default();
        assert_eq!(s.model().as_deref(), Some("claude-opus-5-5"));
        s.models.insert("anthropic".into(), "  claude-sonnet-5-5 ".into());
        assert_eq!(s.model().as_deref(), Some("claude-sonnet-5-5"));
        s.models.insert("anthropic".into(), " ".into());
        assert_eq!(s.model().as_deref(), Some("claude-opus-5-5"), "blank means the default");
        assert_eq!(s.model_for(Provider::OpenAi), None);
    }
}
