//! Chat requests to each provider's streaming API, and reading the answers as they arrive.
//!
//! [`chat_call`] builds the HTTP request (URL, headers, JSON body) for a [`ChatRequest`];
//! [`StreamParser`] turns the provider's server-sent events into [`StreamEvent`]s. Both are pure,
//! so the formats are tested without a network; [`crate::http`] sends the request.
//!
//! - Anthropic: Messages API (`anthropic-version: 2026-10-21`), with the server-side fallback to
//!   another model when a safety classifier declines the request, on models that offer it.
//! - OpenAI, Mistral and Azure OpenAI: Chat Completions.
//! - Google: Gemini `streamGenerateContent` with `alt=sse`.

use serde_json::{Value, json};

use crate::AiError;
use crate::provider::Provider;
use crate::settings::AiSettings;

/// The Anthropic API version requests are written for.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";

/// Claude models that offer the server-side fallback in its `"default"` form.
const ANTHROPIC_FALLBACK_MODELS: [&str; 4] = ["claude-opus-5-5", "claude-opus-5", "claude-fable-5-1", "claude-sonnet-5-5"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message { role: Role::User, text: text.into() }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Message { role: Role::Assistant, text: text.into() }
    }
}

/// One request: instructions for the model, the conversation so far (ending with the user's
/// turn), and the longest answer wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
}

/// Why an answer ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished.
    Done,
    /// The answer reached the length limit.
    MaxTokens,
    /// The provider declined the request (safety filter); its reason when given.
    Refused(Option<String>),
    /// Something else, as the provider named it.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// More of the answer.
    Text(String),
    /// The answer is complete.
    Stop(StopReason),
}

/// An HTTP request, ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpCall {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    /// `Value::Null` for a GET.
    pub body: Value,
}

/// The request for `req` to the provider and model in `settings`, signed with `key`.
pub fn chat_call(settings: &AiSettings, key: &str, req: &ChatRequest) -> Result<HttpCall, AiError> {
    let provider = settings.provider;
    let model = settings.model().ok_or_else(|| AiError::Rejected("no model chosen".into()))?;
    let max_tokens = req.max_tokens.max(1);
    let call = match provider {
        Provider::Anthropic => {
            let messages: Vec<Value> = req.messages.iter().map(|m| json!({"role": role(m.role, "assistant"), "content": m.text})).collect();
            let mut body = json!({"model": model, "max_tokens": max_tokens, "stream": true, "messages": messages});
            if !req.system.is_empty() {
                body["system"] = json!(req.system);
            }
            let mut headers = vec![("x-api-key", key.to_string()), ("anthropic-version", ANTHROPIC_VERSION.to_string())];
            if ANTHROPIC_FALLBACK_MODELS.contains(&model.as_str()) {
                body["fallbacks"] = json!("default");
                headers.push(("anthropic-beta", "server-side-fallback-2026-07-01".to_string()));
            }
            HttpCall { url: "https://api.anthropic.com/v1/messages".into(), headers, body }
        }
        Provider::OpenAi | Provider::Mistral | Provider::AzureOpenAi => {
            let mut messages = Vec::new();
            if !req.system.is_empty() {
                messages.push(json!({"role": "system", "content": req.system}));
            }
            messages.extend(req.messages.iter().map(|m| json!({"role": role(m.role, "assistant"), "content": m.text})));
            let mut body = json!({"model": model, "stream": true, "messages": messages});
            // Mistral names the limit `max_tokens`; OpenAI's reasoning models accept only the newer name.
            let limit = if provider == Provider::Mistral { "max_tokens" } else { "max_completion_tokens" };
            body[limit] = json!(max_tokens);
            match provider {
                Provider::AzureOpenAi => {
                    let endpoint = settings.azure_endpoint.trim().trim_end_matches('/');
                    if !endpoint.starts_with("https://") {
                        return Err(AiError::Rejected("the Azure OpenAI endpoint must start with https://".into()));
                    }
                    let url = format!(
                        "{endpoint}/openai/deployments/{}/chat/completions?api-version={}",
                        url_segment(&model),
                        url_segment(settings.azure_api_version.trim())
                    );
                    HttpCall { url, headers: vec![("api-key", key.to_string())], body }
                }
                Provider::Mistral => HttpCall { url: "https://api.mistral.ai/v1/chat/completions".into(), headers: bearer(key), body },
                _ => HttpCall { url: "https://api.openai.com/v1/chat/completions".into(), headers: bearer(key), body },
            }
        }
        Provider::Gemini => {
            let contents: Vec<Value> = req.messages.iter().map(|m| json!({"role": role(m.role, "model"), "parts": [{"text": m.text}]})).collect();
            let mut body = json!({"contents": contents, "generationConfig": {"maxOutputTokens": max_tokens}});
            if !req.system.is_empty() {
                body["systemInstruction"] = json!({"parts": [{"text": req.system}]});
            }
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse", url_segment(&model));
            HttpCall { url, headers: vec![("x-goog-api-key", key.to_string())], body }
        }
        Provider::Local => return Err(AiError::Rejected("local models run inside Docufy, not over HTTP".into())),
    };
    Ok(call)
}

/// The request listing the models `settings.provider` offers the key's account; `None` where the
/// provider has no such list (Azure deployments, local models).
pub fn models_call(settings: &AiSettings, key: &str) -> Option<HttpCall> {
    let call = |url: &str, headers| HttpCall { url: url.to_string(), headers, body: Value::Null };
    match settings.provider {
        Provider::Anthropic => Some(call(
            "https://api.anthropic.com/v1/models?limit=1000",
            vec![("x-api-key", key.to_string()), ("anthropic-version", ANTHROPIC_VERSION.to_string())],
        )),
        Provider::OpenAi => Some(call("https://api.openai.com/v1/models", bearer(key))),
        Provider::Mistral => Some(call("https://api.mistral.ai/v1/models", bearer(key))),
        Provider::Gemini => {
            Some(call("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000", vec![("x-goog-api-key", key.to_string())]))
        }
        Provider::AzureOpenAi | Provider::Local => None,
    }
}

/// The chat models in a models list answer, sorted.
pub fn parse_models(provider: Provider, answer: &Value) -> Vec<String> {
    let mut out: Vec<String> = match provider {
        Provider::Gemini => answer["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|v| v == "generateContent")))
            .filter_map(|m| m["name"].as_str())
            .map(|n| n.trim_start_matches("models/").to_string())
            .collect(),
        _ => answer["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).map(str::to_string).collect(),
    };
    if provider == Provider::OpenAi {
        // The list also holds embedding, image, audio and moderation models.
        out.retain(|id| {
            let chat =
                id.starts_with("gpt-") || id.starts_with("chatgpt-") || (id.starts_with('o') && id[1..].starts_with(|c: char| c.is_ascii_digit()));
            chat && !["audio", "realtime", "transcribe", "tts", "image", "search"].iter().any(|w| id.contains(w))
        });
    }
    out.sort();
    out.dedup();
    out
}

/// The provider's error text from an error response body (or the body itself, shortened).
pub fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let found = parsed.as_ref().and_then(|v| {
        v["error"]["message"].as_str().or_else(|| v["message"].as_str()).or_else(|| v["error"].as_str()).or_else(|| v["detail"].as_str())
    });
    let text = found.map_or_else(|| body.trim().to_string(), str::to_string);
    let mut short: String = text.chars().take(300).collect();
    if text.chars().count() > 300 {
        short.push('…');
    }
    short
}

fn role(r: Role, assistant: &'static str) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => assistant,
    }
}

fn bearer(key: &str) -> Vec<(&'static str, String)> {
    vec![("authorization", format!("Bearer {key}"))]
}

/// `s` percent-encoded for one URL path segment or query value.
fn url_segment(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Reads one provider's server-sent events. Feed it each line of the response body.
#[derive(Debug)]
pub struct StreamParser {
    provider: Provider,
    /// The `event:` name of the event being read (Anthropic).
    event: String,
    stopped: bool,
}

impl StreamParser {
    pub fn new(provider: Provider) -> Self {
        StreamParser { provider, event: String::new(), stopped: false }
    }

    /// Whether a [`StreamEvent::Stop`] was produced.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// The events in one line of the body (without its line ending).
    pub fn feed(&mut self, line: &str) -> Result<Vec<StreamEvent>, AiError> {
        if let Some(name) = line.strip_prefix("event:") {
            self.event = name.trim().to_string();
            return Ok(Vec::new());
        }
        let Some(data) = line.strip_prefix("data:") else { return Ok(Vec::new()) };
        let data = data.trim();
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data == "[DONE]" {
            return Ok(self.stop(StopReason::Done));
        }
        let v: Value = serde_json::from_str(data).map_err(|e| AiError::Protocol(e.to_string()))?;
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            return Err(stream_error(err));
        }
        Ok(match self.provider {
            Provider::Anthropic => self.anthropic(&v),
            Provider::Gemini => self.gemini(&v),
            _ => self.chat_completions(&v),
        })
    }

    fn stop(&mut self, reason: StopReason) -> Vec<StreamEvent> {
        if std::mem::replace(&mut self.stopped, true) { Vec::new() } else { vec![StreamEvent::Stop(reason)] }
    }

    fn anthropic(&mut self, v: &Value) -> Vec<StreamEvent> {
        let kind = v["type"].as_str().map_or_else(|| self.event.clone(), str::to_string);
        match kind.as_str() {
            // Thinking and the fallback marker are other block types: only text is shown.
            "content_block_delta" if v["delta"]["type"] == "text_delta" => text(&v["delta"]["text"]),
            "message_delta" => match v["delta"]["stop_reason"].as_str() {
                Some("end_turn" | "stop_sequence") => self.stop(StopReason::Done),
                Some("max_tokens") => self.stop(StopReason::MaxTokens),
                Some("refusal") => {
                    let details = v["delta"]["stop_details"].as_object().or_else(|| v["stop_details"].as_object());
                    let field = |name: &str| details.and_then(|d| d.get(name)).and_then(Value::as_str).filter(|s| !s.is_empty());
                    let why = field("explanation").or_else(|| field("category"));
                    self.stop(StopReason::Refused(why.map(str::to_string)))
                }
                Some(other) => self.stop(StopReason::Other(other.to_string())),
                None => Vec::new(),
            },
            "message_stop" => self.stop(StopReason::Done),
            _ => Vec::new(),
        }
    }

    fn chat_completions(&mut self, v: &Value) -> Vec<StreamEvent> {
        let choice = &v["choices"][0];
        let mut out = text(&choice["delta"]["content"]);
        match choice["finish_reason"].as_str() {
            Some("stop") => out.extend(self.stop(StopReason::Done)),
            Some("length") => out.extend(self.stop(StopReason::MaxTokens)),
            Some("content_filter") => out.extend(self.stop(StopReason::Refused(Some("content filter".into())))),
            Some(other) => out.extend(self.stop(StopReason::Other(other.to_string()))),
            None => {}
        }
        out
    }

    fn gemini(&mut self, v: &Value) -> Vec<StreamEvent> {
        if let Some(block) = v["promptFeedback"]["blockReason"].as_str() {
            return self.stop(StopReason::Refused(Some(block.to_string())));
        }
        let candidate = &v["candidates"][0];
        let mut out: Vec<StreamEvent> = candidate["content"]["parts"].as_array().into_iter().flatten().flat_map(|p| text(&p["text"])).collect();
        match candidate["finishReason"].as_str() {
            Some("STOP") => out.extend(self.stop(StopReason::Done)),
            Some("MAX_TOKENS") => out.extend(self.stop(StopReason::MaxTokens)),
            Some(r @ ("SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII")) => {
                out.extend(self.stop(StopReason::Refused(Some(r.to_string()))))
            }
            Some(other) => out.extend(self.stop(StopReason::Other(other.to_string()))),
            None => {}
        }
        out
    }
}

fn text(v: &Value) -> Vec<StreamEvent> {
    v.as_str().filter(|t| !t.is_empty()).map(|t| vec![StreamEvent::Text(t.to_string())]).unwrap_or_default()
}

/// An error reported inside the stream.
fn stream_error(err: &Value) -> AiError {
    let message = err["message"].as_str().or_else(|| err.as_str()).unwrap_or("error").to_string();
    let kind = err["type"].as_str().or_else(|| err["status"].as_str()).unwrap_or_default();
    match kind {
        "overloaded_error" | "api_error" | "server_error" | "UNAVAILABLE" | "INTERNAL" => AiError::Unavailable(message),
        "rate_limit_error" | "RESOURCE_EXHAUSTED" => AiError::RateLimit(message),
        "authentication_error" | "permission_error" | "PERMISSION_DENIED" | "UNAUTHENTICATED" => AiError::Auth(message),
        _ => {
            err["code"].as_u64().and_then(|c| u16::try_from(c).ok()).map_or(AiError::Rejected(message.clone()), |c| AiError::from_status(c, message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ChatRequest {
        ChatRequest {
            system: "Be brief.".into(),
            messages: vec![Message::user("Hi"), Message::assistant("Hello"), Message::user("Again")],
            max_tokens: 500,
        }
    }

    fn settings(p: Provider) -> AiSettings {
        let mut s = AiSettings { provider: p, ..AiSettings::default() };
        s.models.insert(p.id().into(), "m-1".into());
        s
    }

    fn feed(p: Provider, body: &str) -> Result<Vec<StreamEvent>, AiError> {
        let mut parser = StreamParser::new(p);
        let mut out = Vec::new();
        for line in body.lines() {
            out.extend(parser.feed(line)?);
        }
        Ok(out)
    }

    fn joined(events: &[StreamEvent]) -> String {
        events.iter().filter_map(|e| if let StreamEvent::Text(t) = e { Some(t.as_str()) } else { None }).collect()
    }

    #[test]
    fn anthropic_request_uses_the_current_api_and_fallback() {
        let s = AiSettings::default();
        let c = chat_call(&s, "sk-ant", &req()).unwrap();
        assert_eq!(c.url, "https://api.anthropic.com/v1/messages");
        assert!(c.headers.contains(&("x-api-key", "sk-ant".into())));
        assert!(c.headers.contains(&("anthropic-version", "2026-10-21".into())));
        assert!(c.headers.contains(&("anthropic-beta", "server-side-fallback-2026-07-01".into())));
        assert_eq!(c.body["model"], "claude-opus-5-5");
        assert_eq!(c.body["fallbacks"], "default");
        assert_eq!(c.body["system"], "Be brief.");
        assert_eq!(c.body["messages"][1]["role"], "assistant");
        for gone in ["temperature", "top_p", "top_k", "thinking"] {
            assert!(c.body.get(gone).is_none(), "{gone} is rejected or not wanted");
        }
        // Older models don't offer the "default" fallback.
        let c = chat_call(&settings(Provider::Anthropic), "k", &req()).unwrap();
        assert!(c.body.get("fallbacks").is_none());
        assert!(!c.headers.iter().any(|(h, _)| *h == "anthropic-beta"));
    }

    #[test]
    fn chat_completions_requests() {
        let c = chat_call(&settings(Provider::OpenAi), "sk", &req()).unwrap();
        assert_eq!(c.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(c.headers, vec![("authorization", "Bearer sk".to_string())]);
        assert_eq!(c.body["messages"][0], json!({"role": "system", "content": "Be brief."}));
        assert_eq!(c.body["max_completion_tokens"], 500);
        let c = chat_call(&settings(Provider::Mistral), "k", &req()).unwrap();
        assert_eq!(c.url, "https://api.mistral.ai/v1/chat/completions");
        assert_eq!(c.body["max_tokens"], 500);
        let mut az = settings(Provider::AzureOpenAi);
        az.models.insert("azure-open-ai".into(), "my gpt".into());
        az.azure_endpoint = "https://acme.openai.azure.com/".into();
        let c = chat_call(&az, "k", &req()).unwrap();
        assert_eq!(c.url, "https://acme.openai.azure.com/openai/deployments/my%20gpt/chat/completions?api-version=2024-10-21");
        assert_eq!(c.headers, vec![("api-key", "k".to_string())]);
        az.azure_endpoint = "http://acme".into();
        assert!(chat_call(&az, "k", &req()).is_err(), "never send the key unencrypted");
    }

    #[test]
    fn gemini_request() {
        let c = chat_call(&AiSettings { provider: Provider::Gemini, ..AiSettings::default() }, "g", &req()).unwrap();
        assert_eq!(c.url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:streamGenerateContent?alt=sse");
        assert_eq!(c.body["contents"][1]["role"], "model");
        assert_eq!(c.body["systemInstruction"]["parts"][0]["text"], "Be brief.");
        assert_eq!(c.body["generationConfig"]["maxOutputTokens"], 500);
    }

    #[test]
    fn anthropic_stream() {
        let body = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-opus-5-5"}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hal"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"lo"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}

event: message_stop
data: {"type":"message_stop"}"#;
        let ev = feed(Provider::Anthropic, body).unwrap();
        assert_eq!(joined(&ev), "Hallo");
        assert_eq!(ev.last(), Some(&StreamEvent::Stop(StopReason::Done)));
        assert_eq!(ev.iter().filter(|e| matches!(e, StreamEvent::Stop(_))).count(), 1, "one stop");
    }

    #[test]
    fn anthropic_refusal_and_errors() {
        let refusal = r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":null}}}"#;
        assert_eq!(feed(Provider::Anthropic, refusal).unwrap(), [StreamEvent::Stop(StopReason::Refused(Some("cyber".into())))]);
        let err = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}";
        assert_eq!(feed(Provider::Anthropic, err), Err(AiError::Unavailable("Overloaded".into())));
    }

    #[test]
    fn chat_completions_stream() {
        let body = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Gu\"}}]}\ndata: {\"choices\":[{\"delta\":{\"content\":\"ten Tag\"},\"finish_reason\":null}]}\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\ndata: [DONE]";
        let ev = feed(Provider::OpenAi, body).unwrap();
        assert_eq!(joined(&ev), "Guten Tag");
        assert_eq!(ev.last(), Some(&StreamEvent::Stop(StopReason::MaxTokens)), "[DONE] adds no second stop");
        let filtered = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}";
        assert!(matches!(feed(Provider::AzureOpenAi, filtered).unwrap()[..], [StreamEvent::Stop(StopReason::Refused(_))]));
    }

    #[test]
    fn gemini_stream() {
        let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi \"}],\"role\":\"model\"}}]}\r\n\r\ndata: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"there\"}]},\"finishReason\":\"STOP\"}]}";
        let ev = feed(Provider::Gemini, body).unwrap();
        assert_eq!(joined(&ev), "Hi there");
        assert_eq!(ev.last(), Some(&StreamEvent::Stop(StopReason::Done)));
        let blocked = "data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"}}";
        assert_eq!(feed(Provider::Gemini, blocked).unwrap(), [StreamEvent::Stop(StopReason::Refused(Some("SAFETY".into())))]);
    }

    #[test]
    fn model_lists() {
        let anthropic = json!({"data": [{"id": "claude-opus-5-5"}, {"id": "claude-haiku-5-5"}], "has_more": false});
        assert_eq!(parse_models(Provider::Anthropic, &anthropic), ["claude-haiku-5-5", "claude-opus-5-5"]);
        let openai =
            json!({"data": [{"id": "gpt-5"}, {"id": "text-embedding-3-large"}, {"id": "gpt-4o-audio-preview"}, {"id": "o3"}, {"id": "dall-e-3"}]});
        assert_eq!(parse_models(Provider::OpenAi, &openai), ["gpt-5", "o3"]);
        let gemini = json!({"models": [
            {"name": "models/gemini-flash-latest", "supportedGenerationMethods": ["generateContent", "countTokens"]},
            {"name": "models/text-embedding-004", "supportedGenerationMethods": ["embedContent"]}
        ]});
        assert_eq!(parse_models(Provider::Gemini, &gemini), ["gemini-flash-latest"]);
        let az = AiSettings { provider: Provider::AzureOpenAi, ..AiSettings::default() };
        assert!(models_call(&az, "k").is_none());
    }

    #[test]
    fn error_messages_are_read_from_every_format() {
        assert_eq!(error_message(r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#), "invalid x-api-key");
        assert_eq!(error_message(r#"{"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}"#), "Incorrect API key provided");
        assert_eq!(error_message(r#"[{"error":{"code":400}}]"#), r#"[{"error":{"code":400}}]"#);
        assert_eq!(error_message(&"x".repeat(400)).chars().count(), 301);
    }
}
