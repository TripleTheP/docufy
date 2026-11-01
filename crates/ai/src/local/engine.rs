//! Talking to `docufy-ai-host`, the process that runs local models (its protocol is described in
//! `apps/docufy-ai-host/src/main.rs`). One host serves the whole app: started on the first
//! request, kept with its model loaded for the next one, started again if it stopped.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::catalog::Family;
use crate::AiError;
use crate::chat::{ChatRequest, StopReason, StreamEvent};

/// What to run: a model folder, its chat format, the provider and the longest conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub model_dir: PathBuf,
    pub family: Family,
    /// `dml` or `cpu`.
    pub provider: &'static str,
    pub context: u32,
}

struct Engine {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    /// The model and provider loaded now.
    loaded: Option<(PathBuf, &'static str)>,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
/// Set by [`unload`] while an answer is running: the helper is ended before the next request.
static STALE: AtomicBool = AtomicBool::new(false);

/// The host program: `DOCUFY_AI_HOST`, else beside the running program.
pub fn host_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DOCUFY_AI_HOST") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let host = exe.parent()?.join(format!("docufy-ai-host{}", std::env::consts::EXE_SUFFIX));
    host.is_file().then_some(host)
}

fn local(e: impl Into<String>) -> AiError {
    AiError::Local(e.into())
}

impl Engine {
    fn start(runtime: &Path) -> Result<Engine, AiError> {
        let host = host_path().ok_or_else(|| local("docufy-ai-host is missing beside Docufy; reinstall Docufy"))?;
        let mut cmd = Command::new(&host);
        cmd.arg("--runtime").arg(runtime).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window for the helper.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| local(format!("{}: {e}", host.display())))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(local("the helper process has no pipes"));
        };
        let (tx, lines) = channel();
        std::thread::Builder::new()
            .name("docufy-ai-host-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(v) = serde_json::from_str::<Value>(&line)
                        && tx.send(v).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| local(e.to_string()))?;
        let engine = Engine { child, stdin, lines, loaded: None };
        let first = engine.wait(Duration::from_secs(60))?;
        match first["op"].as_str() {
            Some("ready") => Ok(engine),
            _ => Err(local(first["message"].as_str().unwrap_or("the helper process did not start").to_string())),
        }
    }

    fn send(&mut self, v: &Value) -> Result<(), AiError> {
        writeln!(self.stdin, "{v}").and_then(|()| self.stdin.flush()).map_err(|e| local(format!("the helper process stopped: {e}")))
    }

    fn wait(&self, timeout: Duration) -> Result<Value, AiError> {
        match self.lines.recv_timeout(timeout) {
            Ok(v) => Ok(v),
            Err(RecvTimeoutError::Timeout) => Err(local("the helper process does not answer")),
            Err(RecvTimeoutError::Disconnected) => Err(local("the helper process stopped")),
        }
    }

    fn load(&mut self, target: &Target) -> Result<(), AiError> {
        if self.loaded.as_ref().is_some_and(|(dir, p)| *dir == target.model_dir && *p == target.provider) {
            return Ok(());
        }
        self.loaded = None;
        self.send(&json!({"op": "load", "model": target.model_dir.to_string_lossy(), "provider": target.provider}))?;
        // A large model takes a while to load the first time.
        let v = self.wait(Duration::from_secs(600))?;
        match v["op"].as_str() {
            Some("loaded") => {
                log::info!("ai: local model {} loaded on {}", target.model_dir.display(), v["device"]);
                self.loaded = Some((target.model_dir.clone(), target.provider));
                Ok(())
            }
            _ => Err(local(v["message"].as_str().unwrap_or("the model could not be loaded").to_string())),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "{}", json!({"op": "quit"}));
        let _ = self.stdin.flush();
        // A host stuck in the library is ended rather than waited for.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Answers `req` with the local model `target`, calling `on_event` with each piece as it arrives.
/// `runtime` is the folder the runtime is installed in.
pub fn stream_chat(
    runtime: &Path,
    target: &Target,
    req: &ChatRequest,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<(), AiError> {
    let mut slot = ENGINE.lock().unwrap_or_else(PoisonError::into_inner);
    if STALE.swap(false, Ordering::Relaxed) {
        *slot = None;
    }
    if let Some(e) = slot.as_mut()
        && !matches!(e.child.try_wait(), Ok(None))
    {
        log::warn!("ai: the local AI helper had stopped; starting it again");
        *slot = None;
    }
    if slot.is_none() {
        *slot = Some(Engine::start(runtime)?);
    }
    let Some(engine) = slot.as_mut() else { return Err(local("the helper process did not start")) };
    if let Err(e) = engine.load(target) {
        *slot = None;
        return Err(e);
    }
    let prompt = super::template::prompt(target.family, req);
    engine.send(&json!({"op": "generate", "prompt": prompt, "maxNewTokens": req.max_tokens, "maxLength": target.context}))?;
    let mut cancel_sent = false;
    loop {
        if cancel.load(Ordering::Relaxed) && !cancel_sent {
            engine.send(&json!({"op": "cancel"}))?;
            cancel_sent = true;
        }
        let v = match engine.lines.recv_timeout(Duration::from_millis(200)) {
            Ok(v) => v,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                *slot = None;
                return Err(local("the helper process stopped while answering"));
            }
        };
        match v["op"].as_str() {
            Some("text") => on_event(StreamEvent::Text(v["text"].as_str().unwrap_or_default().to_string())),
            Some("done") => {
                log::info!("ai: local answer: {} tokens in {:.1} s", v["tokens"], v["seconds"].as_f64().unwrap_or(0.0));
                return match v["reason"].as_str() {
                    Some("cancelled") => Err(AiError::Cancelled),
                    Some("length") => {
                        on_event(StreamEvent::Stop(StopReason::MaxTokens));
                        Ok(())
                    }
                    _ => {
                        on_event(StreamEvent::Stop(StopReason::Done));
                        Ok(())
                    }
                };
            }
            Some("error") => return Err(local(v["message"].as_str().unwrap_or("unknown error").to_string())),
            _ => {}
        }
    }
}

/// Ends the helper and frees the model's memory (settings changed, a model was deleted). Never
/// waits: while an answer is running, the helper is ended before the next request instead.
pub fn unload() {
    match ENGINE.try_lock() {
        Ok(mut slot) => *slot = None,
        Err(std::sync::TryLockError::Poisoned(p)) => *p.into_inner() = None,
        Err(std::sync::TryLockError::WouldBlock) => STALE.store(true, Ordering::Relaxed),
    }
}
