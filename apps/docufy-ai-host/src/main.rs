//! `docufy-ai-host`: runs local AI models for Docufy in a process of its own, so a crash or a
//! stall in the AI library never takes the documents with it. Docufy starts it with
//! `--runtime <folder>` (where it installed ONNX Runtime GenAI) and talks to it in JSON lines:
//!
//! - Docufy → host (stdin):
//!   - `{"op":"load","model":"<folder>","provider":"dml"|"cpu","options":{"device_id":"0"}}`
//!   - `{"op":"generate","prompt":"<formatted prompt>","maxNewTokens":N,"maxLength":N}`
//!   - `{"op":"cancel"}` stops the answer being generated; `{"op":"quit"}` or the end of stdin ends
//!     the host.
//! - host → Docufy (stdout): `{"op":"ready"}` once the library is loaded; `{"op":"loaded",
//!   "device":"DML"}`; `{"op":"text","text":"…"}` per piece of an answer; `{"op":"done",
//!   "reason":"stop"|"length"|"cancelled","tokens":N,"promptTokens":N,"seconds":S}`;
//!   `{"op":"error","message":"…"}`.
//!
//! The prompt arrives already in the model's chat format (Docufy's `docufy_ai::local::template`).

mod ffi;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use serde_json::{Value, json};

fn send(v: Value) {
    let mut out = std::io::stdout().lock();
    // Docufy reading no more means it is gone; the end of stdin ends the host next.
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn error(message: impl Into<String>) {
    send(json!({"op": "error", "message": message.into()}));
}

/// Reads requests from stdin on a thread of its own, so `cancel` arrives while an answer is
/// being generated.
fn requests(cancel: Arc<AtomicBool>) -> Receiver<Value> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                error(format!("not a JSON request: {line}"));
                continue;
            };
            if v["op"] == "cancel" {
                cancel.store(true, Ordering::Relaxed);
                continue;
            }
            if tx.send(v).is_err() {
                break;
            }
        }
    });
    rx
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut runtime = None;
    while let Some(a) = args.next() {
        if a == "--runtime" {
            runtime = args.next().map(PathBuf::from);
        }
    }
    let Some(runtime) = runtime else {
        error("usage: docufy-ai-host --runtime <folder with onnxruntime-genai>");
        std::process::exit(2);
    };
    let genai = match ffi::Genai::load(&runtime) {
        Ok(g) => g,
        Err(e) => {
            error(format!("the AI runtime could not be loaded: {e}"));
            std::process::exit(1);
        }
    };
    send(json!({"op": "ready"}));
    let cancel = Arc::new(AtomicBool::new(false));
    let rx = requests(cancel.clone());
    let mut model: Option<(ffi::Model, ffi::Tokenizer)> = None;
    while let Ok(req) = rx.recv() {
        match req["op"].as_str() {
            Some("load") => {
                // The old model's memory is freed before the new one is loaded.
                model = None;
                match load(&genai, &req) {
                    Ok((m, t, device)) => {
                        model = Some((m, t));
                        send(json!({"op": "loaded", "device": device}));
                    }
                    Err(e) => error(e),
                }
            }
            Some("generate") => {
                cancel.store(false, Ordering::Relaxed);
                match &model {
                    Some((m, t)) => {
                        if let Err(e) = generate(m, t, &req, &cancel) {
                            error(e);
                        }
                    }
                    None => error("no model is loaded"),
                }
            }
            Some("quit") => break,
            _ => error(format!("unknown request: {req}")),
        }
    }
    drop(model);
    genai.shutdown();
}

fn load(genai: &ffi::Genai, req: &Value) -> Result<(ffi::Model, ffi::Tokenizer, String), String> {
    let dir = req["model"].as_str().ok_or("`model` must be a folder")?;
    let provider = req["provider"].as_str().unwrap_or("cpu");
    let options: Vec<(String, String)> =
        req["options"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    let options: Vec<(&str, &str)> = options.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let model = genai.model(std::path::Path::new(dir), provider, &options)?;
    let tokenizer = model.tokenizer()?;
    let device = model.device_type().unwrap_or_default();
    Ok((model, tokenizer, device))
}

fn generate(model: &ffi::Model, tokenizer: &ffi::Tokenizer, req: &Value, cancel: &AtomicBool) -> Result<(), String> {
    let prompt = req["prompt"].as_str().ok_or("`prompt` must be text")?;
    let max_new = req["maxNewTokens"].as_u64().unwrap_or(1024) as usize;
    let max_length = req["maxLength"].as_u64().unwrap_or(4096) as usize;
    let started = Instant::now();
    let tokens = tokenizer.encode(prompt)?;
    let prompt_tokens = tokens.len();
    if prompt_tokens >= max_length {
        return Err(format!("the request is too long for the model: {prompt_tokens} tokens, at most {max_length}"));
    }
    let limit = (prompt_tokens + max_new).min(max_length);
    let mut generator = model.generator(&tokens, limit)?;
    let mut stream = tokenizer.stream()?;
    let mut generated = 0usize;
    let mut reason = "stop";
    while !generator.is_done() {
        if cancel.load(Ordering::Relaxed) {
            reason = "cancelled";
            break;
        }
        let Some(token) = generator.next_token()? else { break };
        generated += 1;
        let piece = stream.decode(token)?;
        if !piece.is_empty() {
            send(json!({"op": "text", "text": piece}));
        }
    }
    if reason == "stop" && prompt_tokens + generated >= limit {
        reason = "length";
    }
    send(json!({
        "op": "done",
        "reason": reason,
        "tokens": generated,
        "promptTokens": prompt_tokens,
        "seconds": started.elapsed().as_secs_f64(),
    }));
    Ok(())
}
