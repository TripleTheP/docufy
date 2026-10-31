//! Sending [`HttpCall`]s: a streamed chat answer ([`stream_chat`]) and a provider's model list
//! ([`list_models`]). Blocking; the front end calls them on a worker thread.

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::AiError;
use crate::chat::{ChatRequest, HttpCall, StopReason, StreamEvent, StreamParser, chat_call, error_message, models_call, parse_models};
use crate::settings::AiSettings;

/// Attempts before a busy provider's error is shown (429, 5xx, broken connection, all before any
/// text arrived).
const ATTEMPTS: u32 = 3;
/// Longest wait between attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(20);
/// Largest error body read.
const ERROR_BODY_LIMIT: u64 = 64 * 1024;

fn agent(stream: bool) -> ureq::Agent {
    // The OS checks certificates (and trusts what company proxies and antivirus install).
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(ureq::Proxy::try_from_env())
        .http_status_as_error(false)
        .user_agent(concat!("Docufy/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(120)));
    // A streamed answer can take minutes; a stall of two minutes between chunks is an error.
    let config = if stream { config.timeout_per_read(Some(Duration::from_secs(120))) } else { config.timeout_global(Some(Duration::from_secs(30))) };
    config.build().new_agent()
}

fn send(agent: &ureq::Agent, call: &HttpCall) -> Result<ureq::http::Response<ureq::Body>, AiError> {
    let result = if call.body.is_null() {
        let mut req = agent.get(&call.url);
        for (name, value) in &call.headers {
            req = req.header(*name, value);
        }
        req.call()
    } else {
        let mut req = agent.post(&call.url).header("content-type", "application/json");
        for (name, value) in &call.headers {
            req = req.header(*name, value);
        }
        req.send(call.body.to_string())
    };
    result.map_err(|e| AiError::Network(e.to_string()))
}

/// The error for a non-2xx response.
fn status_error(resp: ureq::http::Response<ureq::Body>) -> AiError {
    let status = resp.status().as_u16();
    let mut body = String::new();
    let _ = resp.into_body().into_reader().take(ERROR_BODY_LIMIT).read_to_string(&mut body);
    let message = error_message(&body);
    // Anthropic's 529 "overloaded" is not a standard status.
    AiError::from_status(status, if message.is_empty() { format!("HTTP {status}") } else { message })
}

/// How long to wait before attempt `attempt` (1-based), honouring `retry-after`.
fn backoff(attempt: u32, retry_after: Option<&str>) -> Duration {
    let asked = retry_after.and_then(|v| v.trim().parse::<u64>().ok()).map(Duration::from_secs);
    asked.unwrap_or(Duration::from_secs(1 << attempt.min(4))).min(MAX_BACKOFF)
}

/// Sends `req` with `settings` and `key`, calling `on_event` with each piece of the answer as it
/// arrives. Stops early when `cancel` is set. Busy providers are retried before any text arrives.
pub fn stream_chat(
    settings: &AiSettings,
    key: &str,
    req: &ChatRequest,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<(), AiError> {
    let call = chat_call(settings, key, req)?;
    let agent = agent(true);
    let mut attempt = 0;
    let resp = loop {
        attempt += 1;
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let outcome = send(&agent, &call);
        let wait = match &outcome {
            Ok(r) if r.status().is_success() => None,
            Ok(r) => {
                let s = r.status().as_u16();
                (matches!(s, 429 | 500..=599) && attempt < ATTEMPTS)
                    .then(|| backoff(attempt, r.headers().get("retry-after").and_then(|v| v.to_str().ok())))
            }
            Err(e) if e.is_retryable() && attempt < ATTEMPTS => Some(backoff(attempt, None)),
            Err(_) => None,
        };
        match (outcome, wait) {
            (_, Some(wait)) => {
                log::info!("ai: provider busy, retrying in {wait:?}");
                std::thread::sleep(wait);
            }
            (Ok(r), None) if r.status().is_success() => break r,
            (Ok(r), None) => return Err(status_error(r)),
            (Err(e), None) => return Err(e),
        }
    };
    let mut parser = StreamParser::new(settings.provider);
    let reader = BufReader::new(resp.into_body().into_reader());
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let line = line.map_err(|e| AiError::Network(e.to_string()))?;
        for event in parser.feed(line.trim_end_matches('\r'))? {
            on_event(event);
        }
    }
    if !parser.stopped() {
        // The connection ended without the provider saying the answer was complete.
        on_event(StreamEvent::Stop(StopReason::Other("connection closed".into())));
    }
    Ok(())
}

/// The chat models the provider offers the key's account; empty where it has no list.
pub fn list_models(settings: &AiSettings, key: &str) -> Result<Vec<String>, AiError> {
    let Some(call) = models_call(settings, key) else { return Ok(Vec::new()) };
    let resp = send(&agent(false), &call)?;
    if !resp.status().is_success() {
        return Err(status_error(resp));
    }
    let body = resp.into_body().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| AiError::Network(e.to_string()))?;
    let answer: Value = serde_json::from_str(&body).map_err(|e| AiError::Protocol(e.to_string()))?;
    Ok(parse_models(settings.provider, &answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_honours_retry_after_and_is_capped() {
        assert_eq!(backoff(1, None), Duration::from_secs(2));
        assert_eq!(backoff(2, None), Duration::from_secs(4));
        assert_eq!(backoff(1, Some("7")), Duration::from_secs(7));
        assert_eq!(backoff(1, Some("3600")), MAX_BACKOFF);
        assert_eq!(backoff(9, None), Duration::from_secs(16));
    }
}
