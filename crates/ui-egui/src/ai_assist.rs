//! The AI features: the AI Assistant pane with rewriting the selection, continuing the text,
//! summarising the document (or, in the desktop app, the PDF shown in PDF mode) and a chat.
//!
//! Everything goes through `ai.*` commands, so agents and the control channel can use it too.
//! What the AI writes is a suggestion: it streams into the pane and changes the document only
//! when the user (or a command) accepts it, as one undo step. A suggestion remembers where it
//! belongs; if that text changed meanwhile, accepting it is refused instead of overwriting the
//! change. Answers come from a worker thread ([`docufy_ai::http::stream_chat`]); [`poll`] takes
//! them in every frame, also while the pane is closed or PDF mode is shown.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use docufy_ai::assist::{self, ChatContext, Rewrite};
use docufy_ai::{AiError, ChatRequest, Message, Provider, Role, Severity, StopReason, StreamEvent, warnings};
use docufy_doc::{Pos, StoryRef};
use docufy_engine::Selection;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::ai_ui::{fill, note, section};

/// Reads a document's text on the worker thread (a PDF's pages take a moment).
pub type TextSource = Box<dyn FnOnce() -> Result<String, String> + Send>;

/// The AI Assistant's state while the app runs.
#[derive(Default)]
pub struct Assist {
    /// Whether the AI Assistant pane is shown.
    pub open: bool,
    /// What the AI wrote for the document, until it is accepted or discarded.
    pub suggestion: Option<Suggestion>,
    /// The chat so far.
    pub chat: Vec<Turn>,
    chat_input: String,
    /// What the chat may see of the document.
    pub chat_context: ChatContext,
    /// The user's own rewrite instruction being typed.
    custom: String,
    job: Option<Job>,
    /// Desktop app: summarise the PDF shown in PDF mode (`ai.summarizePdf`); the host reads it
    /// and calls [`summarize_source`].
    pub pdf_summary_wanted: bool,
    /// The last thing that went wrong outside a suggestion or the chat.
    error: Option<String>,
}

/// One message in the chat.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
    /// The answer stopped with an error (shown instead of text).
    pub failed: bool,
}

/// What a suggestion is for.
#[derive(Debug, Clone, PartialEq)]
pub enum Task {
    Rewrite(Rewrite),
    Continue,
    Summary { title: String },
}

/// Where a suggestion goes when it is accepted.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// In place of the selection it was made from, while that still holds `original`.
    Replace { document: u64, sel: Selection, original: String },
    /// At the caret where it was asked for.
    Insert { document: u64, at: Pos },
    /// Nowhere fixed: at the caret, or in a new document.
    Free,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Sent; no text yet.
    Waiting,
    /// Text is arriving.
    Streaming,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub task: Task,
    /// The answer so far.
    pub text: String,
    pub target: Target,
    pub phase: Phase,
    /// Something to know about the answer (cut off at the length limit).
    pub note: Option<String>,
    /// The request, to ask again; `None` until a PDF's text is read.
    request: Option<ChatRequest>,
}

/// A request on the worker thread.
struct Job {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    /// The answer goes to the chat, not to the suggestion.
    chat: bool,
}

/// What the worker thread reports.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Msg {
    /// The request it built from a [`TextSource`].
    Request(ChatRequest),
    Event(StreamEvent),
    /// The request ended: completely, or with this error.
    Finished(Result<(), AiError>),
    /// The document's text could not be read (`None`: it has none); nothing was sent.
    Unreadable(Option<String>),
}

/// How the worker gets its request.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Prepared {
    Ready(ChatRequest),
    /// Summarise the text `source` reads, of the document called `title`.
    Summary { title: String, source: TextSource },
}

/// Whether a request is running.
pub fn busy(app: &WordApp) -> bool {
    app.ai.assist.job.is_some()
}

/// The commands: `ai.pane`, `ai.rewrite`, `ai.continue`, `ai.summarize`, `ai.summarizePdf`,
/// `ai.chat`, `ai.chatClear`, `ai.accept`, `ai.insertAnswer`, `ai.discard`, `ai.cancel` and
/// `ai.status`. `None` for other ids.
pub fn command(app: &mut WordApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    Some(match id {
        "ai.pane" => match p.get("value") {
            None | Some(Value::Null) => {
                app.ai.assist.open = !app.ai.assist.open;
                Ok(json!({"open": app.ai.assist.open}))
            }
            Some(Value::Bool(v)) => {
                app.ai.assist.open = *v;
                Ok(json!({"open": v}))
            }
            Some(_) => Err("`value` must be true or false".into()),
        },
        "ai.rewrite" => {
            let how = match (text("style"), text("instruction")) {
                (Some("custom") | None, Some(own)) => Ok(Rewrite::Custom(own.chars().take(2000).collect())),
                (Some("custom"), None) => Err("a custom rewrite needs an `instruction`".into()),
                (Some(style), _) => Rewrite::from_id(style).ok_or_else(|| format!("unknown `style` `{style}`; one of improve, shorter, longer, formal, simple, fix, custom")),
                (None, None) => Err("give a `style` (improve, shorter, longer, formal, simple, fix) or an `instruction`".into()),
            };
            how.and_then(|how| rewrite(app, how))
        }
        "ai.continue" => continue_writing(app),
        "ai.summarize" => summarize(app),
        "ai.summarizePdf" => match ready(app) {
            Ok(_) => {
                app.ai.assist.pdf_summary_wanted = true;
                Ok(json!({"pending": true}))
            }
            Err(e) => Err(e),
        },
        "ai.chat" => match text("message") {
            Some(message) => {
                let context = match text("context") {
                    None => Ok(app.ai.assist.chat_context),
                    Some(c) => ChatContext::from_id(c).ok_or_else(|| format!("unknown `context` `{c}`; one of none, selection, document")),
                };
                context.and_then(|c| {
                    app.ai.assist.chat_context = c;
                    send_chat(app, message)
                })
            }
            None => Err("`message` must be text".into()),
        },
        "ai.chatClear" => {
            cancel_job(app, true);
            app.ai.assist.chat.clear();
            Ok(json!({"chat": []}))
        }
        "ai.accept" => accept(app, text("as")),
        "ai.insertAnswer" => insert_answer(app, p.get("index").and_then(Value::as_u64)),
        "ai.discard" => {
            if app.ai.assist.job.as_ref().is_some_and(|j| !j.chat) {
                cancel_job(app, false);
            }
            Ok(json!({"discarded": app.ai.assist.suggestion.take().is_some()}))
        }
        "ai.cancel" => {
            let was = busy(app);
            if let Some(job) = &app.ai.assist.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
            Ok(json!({"cancelled": was}))
        }
        "ai.status" => Ok(status(app)),
        _ => return None,
    })
}

fn status(app: &WordApp) -> Value {
    let a = &app.ai.assist;
    let suggestion = a.suggestion.as_ref().map(|s| {
        let (phase, error) = match &s.phase {
            Phase::Waiting => ("waiting", None),
            Phase::Streaming => ("streaming", None),
            Phase::Ready => ("ready", None),
            Phase::Failed(e) => ("failed", Some(e.clone())),
        };
        let task = match &s.task {
            Task::Rewrite(r) => r.id(),
            Task::Continue => "continue",
            Task::Summary { .. } => "summary",
        };
        json!({"task": task, "text": s.text, "phase": phase, "error": error, "note": s.note})
    });
    let chat: Vec<Value> = a
        .chat
        .iter()
        .map(|t| json!({"role": if t.role == Role::User { "user" } else { "assistant" }, "text": t.text, "failed": t.failed}))
        .collect();
    json!({"open": a.open, "busy": a.job.is_some(), "suggestion": suggestion, "chat": chat, "chatContext": a.chat_context.id(), "error": a.error})
}

/// The key to send with, or why AI can't be used now (in words for the user).
fn ready(app: &mut WordApp) -> Result<String, String> {
    if app.safe_mode() {
        return Err(tl!("Docufy runs in safe mode: AI is off until the next normal start.").into());
    }
    let s = &app.ui.ai;
    if cfg!(target_arch = "wasm32") {
        return Err(tl!("AI is available in the desktop app.").into());
    }
    if !s.enabled {
        return Err(tl!("AI is off. Turn it on in File › Options › AI.").into());
    }
    if s.provider == Provider::Local {
        return Err(tl!("Models that run on this computer come in a later version. Choose a provider in File › Options › AI.").into());
    }
    if busy(app) {
        return Err(tl!("The AI is still answering. Wait for it, or cancel it.").into());
    }
    let key = stored_key(s.provider)?;
    let blocking = warnings::for_cloud(s, key.is_some()).into_iter().find(|w| w.severity == Severity::Blocking);
    if let Some(w) = blocking {
        let args: Vec<(&str, String)> = w.args.iter().map(|(n, v)| (*n, tl!(v).to_string())).collect();
        let args: Vec<(&str, &str)> = args.iter().map(|(n, v)| (*n, v.as_str())).collect();
        return Err(fill(tl!(w.template), &args));
    }
    key.ok_or_else(|| tl!("Store an API key first.").to_string())
}

/// Whether a key is stored for `provider` (the self-test asks).
pub(crate) fn key_stored(provider: Provider) -> bool {
    matches!(stored_key(provider), Ok(Some(_)))
}

#[cfg(not(target_arch = "wasm32"))]
fn stored_key(provider: Provider) -> Result<Option<String>, String> {
    match docufy_ai::secrets::load(provider) {
        // Debug builds talking to a local test server need no real key.
        Ok(None) if docufy_ai::http::test_endpoint().is_some() => Ok(Some("test-key".into())),
        other => other,
    }
}

#[cfg(target_arch = "wasm32")]
fn stored_key(_: Provider) -> Result<Option<String>, String> {
    Ok(None)
}

fn max_tokens(app: &WordApp) -> u32 {
    app.ui.ai.max_output_tokens
}

/// Positions that still lead to text in the document.
fn valid(app: &WordApp, p: &Pos) -> bool {
    app.session.doc.para_at(p).is_some_and(|para| p.off <= para.len())
}

fn rewrite(app: &mut WordApp, how: Rewrite) -> Result<Value, String> {
    let key = ready(app)?;
    let sel = app.session.sel.clone();
    let original = app.session.selected_text();
    if sel.is_collapsed() || original.trim().is_empty() {
        return Err(tl!("Select the text to rewrite first.").into());
    }
    let req = assist::rewrite(&how, &original, max_tokens(app));
    let target = Target::Replace { document: app.session.document_id(), sel, original };
    start_suggestion(app, Task::Rewrite(how), target, key, Prepared::Ready(req))
}

fn continue_writing(app: &mut WordApp) -> Result<Value, String> {
    let key = ready(app)?;
    let (_, at) = app.session.sel.ordered();
    let at = at.clone();
    let start = app.session.doc.start_of(at.story);
    let before = app.session.doc.copy_range(&start, &at).plain_text();
    if before.trim().is_empty() {
        return Err(tl!("Write a few sentences first: the AI continues the text before the cursor.").into());
    }
    let req = assist::continue_writing(&before, max_tokens(app));
    let target = Target::Insert { document: app.session.document_id(), at };
    start_suggestion(app, Task::Continue, target, key, Prepared::Ready(req))
}

fn summarize(app: &mut WordApp) -> Result<Value, String> {
    let key = ready(app)?;
    let selected = if app.session.sel.is_collapsed() { String::new() } else { app.session.selected_text() };
    let (title, text) = if selected.trim().is_empty() {
        (app.prompt_name(), app.session.doc.plain_text(StoryRef::Body))
    } else {
        (tl!("Selected text").to_string(), selected)
    };
    if text.trim().is_empty() {
        return Err(tl!("The document has no text to summarise.").into());
    }
    let req = assist::summarize(&title, &text, max_tokens(app));
    start_suggestion(app, Task::Summary { title }, Target::Free, key, Prepared::Ready(req))
}

impl WordApp {
    /// Desktop app: whether `ai.summarizePdf` asked to summarise the PDF in PDF mode (once).
    pub fn take_pdf_summary_request(&mut self) -> bool {
        std::mem::take(&mut self.ai.assist.pdf_summary_wanted)
    }
}

/// Summarise a document the word processor doesn't hold (the desktop app's PDF mode): `source`
/// reads its text on the worker thread. Opens the AI Assistant pane to show the summary.
pub fn summarize_source(app: &mut WordApp, title: String, source: TextSource) -> Result<Value, String> {
    let key = ready(app)?;
    app.ai.assist.open = true;
    start_suggestion(app, Task::Summary { title: title.clone() }, Target::Free, key, Prepared::Summary { title, source })
}

fn start_suggestion(app: &mut WordApp, task: Task, target: Target, key: String, prepared: Prepared) -> Result<Value, String> {
    let request = match &prepared {
        Prepared::Ready(req) => Some(req.clone()),
        Prepared::Summary { .. } => None,
    };
    app.ai.assist.error = None;
    app.ai.assist.suggestion = Some(Suggestion { task, text: String::new(), target, phase: Phase::Waiting, note: None, request });
    start_job(app, key, prepared, false)?;
    Ok(json!({"started": true}))
}

/// Ask again for the current suggestion.
fn retry(app: &mut WordApp) -> Result<Value, String> {
    let key = ready(app)?;
    let Some(s) = app.ai.assist.suggestion.as_mut() else { return Err("no suggestion to ask again for".into()) };
    let Some(req) = s.request.clone() else { return Err("the text could not be read".into()) };
    s.text.clear();
    s.note = None;
    s.phase = Phase::Waiting;
    start_job(app, key, Prepared::Ready(req), false)?;
    Ok(json!({"started": true}))
}

fn send_chat(app: &mut WordApp, message: &str) -> Result<Value, String> {
    let key = ready(app)?;
    let context = match app.ai.assist.chat_context {
        ChatContext::None => None,
        ChatContext::Selection => {
            let t = if app.session.sel.is_collapsed() { String::new() } else { app.session.selected_text() };
            if t.trim().is_empty() {
                return Err(tl!("Select the text to send first, or choose to send no document text.").into());
            }
            Some((ChatContext::Selection, t))
        }
        ChatContext::Document => Some((ChatContext::Document, app.session.doc.plain_text(StoryRef::Body))),
    };
    let max = max_tokens(app);
    let chat = &mut app.ai.assist.chat;
    chat.push(Turn { role: Role::User, text: message.chars().take(20_000).collect(), failed: false });
    // Earlier failed answers aren't part of the conversation the model sees.
    let history: Vec<Message> = chat
        .iter()
        .filter(|t| !t.failed && !t.text.is_empty())
        .map(|t| if t.role == Role::User { Message::user(t.text.clone()) } else { Message::assistant(t.text.clone()) })
        .collect();
    let req = assist::chat(&history, context.as_ref().map(|(k, t)| (*k, t.as_str())), max);
    chat.push(Turn { role: Role::Assistant, text: String::new(), failed: false });
    app.ai.assist.error = None;
    start_job(app, key, Prepared::Ready(req), true)?;
    Ok(json!({"started": true}))
}

#[cfg(not(target_arch = "wasm32"))]
fn start_job(app: &mut WordApp, key: String, prepared: Prepared, chat: bool) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let settings = app.ui.ai.clone();
    let ctx = app.ctx.clone();
    let stop = cancel.clone();
    let max = max_tokens(app);
    let started = std::thread::Builder::new().name("docufy-ai-assist".into()).spawn(move || {
        let repaint = || {
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        };
        let req = match prepared {
            Prepared::Ready(req) => req,
            Prepared::Summary { title, source } => match source() {
                Ok(text) if !text.trim().is_empty() => {
                    let req = assist::summarize(&title, &text, max);
                    let _ = tx.send(Msg::Request(req.clone()));
                    req
                }
                Ok(_) => {
                    let _ = tx.send(Msg::Unreadable(None));
                    repaint();
                    return;
                }
                Err(e) => {
                    let _ = tx.send(Msg::Unreadable(Some(e)));
                    repaint();
                    return;
                }
            },
        };
        let result = docufy_ai::http::stream_chat(&settings, &key, &req, &stop, |e| {
            let _ = tx.send(Msg::Event(e));
            repaint();
        });
        let _ = tx.send(Msg::Finished(result));
        repaint();
    });
    match started {
        Ok(_) => {
            app.ai.assist.job = Some(Job { rx, cancel, chat });
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn start_job(_: &mut WordApp, _: String, _: Prepared, _: bool) -> Result<(), String> {
    Err(tl!("AI is available in the desktop app.").into())
}

/// Stop the running request; `chat` says whether it is the chat's (whose empty answer is
/// dropped) or the suggestion's (which is left as it is).
fn cancel_job(app: &mut WordApp, chat: bool) {
    if let Some(job) = app.ai.assist.job.take() {
        job.cancel.store(true, Ordering::Relaxed);
        if job.chat && chat && app.ai.assist.chat.last().is_some_and(|t| t.role == Role::Assistant && t.text.is_empty()) {
            app.ai.assist.chat.pop();
        }
    }
}

/// Take in what the worker thread sent. Runs every frame.
pub fn poll(app: &mut WordApp) {
    loop {
        let Some(job) = &app.ai.assist.job else { return };
        let chat = job.chat;
        let msg = match job.rx.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Msg::Finished(Err(AiError::Protocol("the request stopped unexpectedly".into()))),
        };
        if matches!(msg, Msg::Finished(_) | Msg::Unreadable(_)) {
            app.ai.assist.job = None;
        }
        apply(app, chat, msg);
    }
}

fn apply(app: &mut WordApp, chat: bool, msg: Msg) {
    let a = &mut app.ai.assist;
    if chat {
        let Some(turn) = a.chat.last_mut().filter(|t| t.role == Role::Assistant) else { return };
        match msg {
            Msg::Request(_) => {}
            Msg::Event(StreamEvent::Text(t)) => turn.text.push_str(&t),
            Msg::Event(StreamEvent::Stop(reason)) => {
                if let Some(n) = stop_note(&reason) {
                    turn.text.push_str(&format!("\n\n({n})"));
                }
            }
            Msg::Finished(Ok(())) | Msg::Unreadable(_) => {}
            Msg::Finished(Err(e)) => {
                turn.failed = true;
                turn.text = crate::ai_ui::error_text(&e);
            }
        }
        return;
    }
    let Some(s) = a.suggestion.as_mut() else { return };
    match msg {
        Msg::Request(req) => s.request = Some(req),
        Msg::Event(StreamEvent::Text(t)) => {
            s.text.push_str(&t);
            s.phase = Phase::Streaming;
        }
        Msg::Event(StreamEvent::Stop(reason)) => {
            s.note = stop_note(&reason);
            if let StopReason::Refused(_) = reason {
                s.phase = Phase::Failed(s.note.take().unwrap_or_default());
            }
        }
        Msg::Finished(Ok(())) => {
            if !matches!(s.phase, Phase::Failed(_)) {
                s.phase = if assist::clean_answer(&s.text).is_empty() { Phase::Failed(tl!("The AI gave no answer.").into()) } else { Phase::Ready };
            }
        }
        Msg::Finished(Err(e)) => s.phase = Phase::Failed(crate::ai_ui::error_text(&e)),
        Msg::Unreadable(None) => {
            s.phase = Phase::Failed(tl!("The document has no text to summarise. If it is a scan, recognise its text first (OCR).").into());
        }
        Msg::Unreadable(Some(e)) => s.phase = Phase::Failed(fill(tl!("The document's text could not be read: {error}"), &[("error", &e)])),
    }
}

/// What to tell the user about how an answer ended; `None` when it simply finished.
fn stop_note(reason: &StopReason) -> Option<String> {
    match reason {
        StopReason::Done => None,
        StopReason::MaxTokens => Some(tl!("The answer reached the length limit and may be cut off (File › Options › AI).").into()),
        StopReason::Refused(why) => {
            let why = why.clone().unwrap_or_default();
            Some(fill(tl!("The provider declined to answer this request. {reason}"), &[("reason", &why)]).trim().to_string())
        }
        StopReason::Other(what) => Some(fill(tl!("The answer ended early ({reason})."), &[("reason", what)])),
    }
}

/// Accept the suggestion: `how` is `replace` (the selection it was made from), `insert` (at the
/// caret, or where Continue Writing was asked for) or `newDocument`; by default where it belongs.
fn accept(app: &mut WordApp, how: Option<&str>) -> Result<Value, String> {
    let Some(s) = app.ai.assist.suggestion.clone() else { return Err("there is no suggestion to accept".into()) };
    if s.phase != Phase::Ready {
        return Err(tl!("The suggestion isn't complete yet.").into());
    }
    let text = paragraphs(&assist::clean_answer(&s.text));
    let how = how.unwrap_or(match s.target {
        Target::Replace { .. } => "replace",
        Target::Insert { .. } | Target::Free => "insert",
    });
    let changed = || tl!("The text changed after the suggestion was made. Ask again.").to_string();
    match (how, &s.target) {
        ("replace", Target::Replace { document, sel, original }) => {
            if app.session.document_id() != *document || !valid(app, &sel.anchor) || !valid(app, &sel.focus) {
                return Err(changed());
            }
            let before = std::mem::replace(&mut app.session.sel, sel.clone());
            if app.session.selected_text() != *original {
                app.session.sel = before;
                return Err(changed());
            }
            app.run("edit.pasteText", json!({"text": text}))?;
        }
        ("insert", Target::Insert { document, at }) => {
            if app.session.document_id() != *document || !valid(app, at) {
                return Err(changed());
            }
            app.session.sel = Selection::caret(at.clone());
            // The continuation starts a paragraph of its own after text on the caret's line.
            let text = if at.off > 0 { format!("\n{text}") } else { text };
            app.run("edit.pasteText", json!({"text": text}))?;
        }
        ("insert", _) => {
            app.run("edit.pasteText", json!({"text": text}))?;
        }
        ("newDocument", _) => {
            app.run("file.new", json!({}))?;
            app.run("edit.pasteText", json!({"text": text}))?;
        }
        ("replace", _) => return Err("this suggestion replaces no selection; use `insert` or `newDocument`".into()),
        (other, _) => return Err(format!("unknown `as` `{other}`; one of replace, insert, newDocument")),
    }
    app.ai.assist.suggestion = None;
    Ok(json!({"accepted": how}))
}

/// The chat answer at `index` (by default the last one) inserted at the caret.
fn insert_answer(app: &mut WordApp, index: Option<u64>) -> Result<Value, String> {
    let chat = &app.ai.assist.chat;
    let turn = match index {
        Some(i) => chat.get(usize::try_from(i).unwrap_or(usize::MAX)),
        None => chat.iter().rev().find(|t| t.role == Role::Assistant && !t.failed && !t.text.is_empty()),
    };
    let Some(turn) = turn.filter(|t| t.role == Role::Assistant && !t.failed && !t.text.is_empty()) else {
        return Err("there is no answer to insert".into());
    };
    if index.is_some_and(|i| i as usize + 1 == chat.len()) && busy(app) {
        return Err(tl!("The AI is still answering. Wait for it, or cancel it.").into());
    }
    let text = paragraphs(&assist::clean_answer(&turn.text));
    app.run("edit.pasteText", json!({"text": text}))?;
    Ok(json!({"inserted": true}))
}

/// Text as the document takes it: one paragraph per line, blank lines between paragraphs dropped.
fn paragraphs(text: &str) -> String {
    text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// Run `id` and show what went wrong.
fn act(app: &mut WordApp, id: &str, params: Value) {
    if let Err(e) = app.run(id, params) {
        app.ai.assist.error = Some(e);
    }
}

/// The AI Assistant pane.
pub fn pane(app: &mut WordApp, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(ui.available_width());
        body(app, ui);
    });
}

fn body(app: &mut WordApp, ui: &mut Ui) {
    if app.safe_mode() {
        ui.label(tl!("Docufy runs in safe mode: AI is off until the next normal start."));
        return;
    }
    if !app.ui.ai.enabled || cfg!(target_arch = "wasm32") {
        ui.label(tl!("AI is off. Docufy sends nothing to an AI provider until you turn it on."));
        if !cfg!(target_arch = "wasm32") && ui.button(tl!("Turn on AI…")).clicked() {
            act(app, "ui.backstage", json!({"value": true, "page": "options", "pane": "ai"}));
        }
        return;
    }
    let model = app.ui.ai.model().unwrap_or_default();
    let header = fill(tl!("{provider} · {model}"), &[("provider", tl!(app.ui.ai.provider.name())), ("model", &model)]);
    ui.horizontal_wrapped(|ui| {
        note(ui, &header);
        if ui.small_button(tl!("Settings…")).clicked() {
            act(app, "ui.backstage", json!({"value": true, "page": "options", "pane": "ai"}));
        }
    });
    if let Some(e) = app.ai.assist.error.clone() {
        ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
    }
    let busy = busy(app);
    let selected = !app.session.sel.is_collapsed();
    section(ui, "Selected text");
    if !selected {
        note(ui, tl!("Select text in the document to rewrite it."));
    }
    ui.horizontal_wrapped(|ui| {
        for how in Rewrite::FIXED {
            if ui.add_enabled(selected && !busy, egui::Button::new(tl!(how.label()))).clicked() {
                act(app, "ai.rewrite", json!({"style": how.id()}));
            }
        }
    });
    ui.add(
        egui::TextEdit::singleline(&mut app.ai.assist.custom)
            .hint_text(tl!("Your own instruction, e.g. “as a bulleted list”"))
            .desired_width(f32::INFINITY),
    );
    let own = app.ai.assist.custom.trim().to_string();
    if ui.add_enabled(selected && !busy && !own.is_empty(), egui::Button::new(tl!("Rewrite"))).clicked() {
        act(app, "ai.rewrite", json!({"instruction": own}));
    }
    section(ui, "Write and summarise");
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!busy, egui::Button::new(tl!("Continue writing"))).on_hover_text(tl!("The AI writes on from the cursor.")).clicked() {
            act(app, "ai.continue", json!({}));
        }
        let what = if selected { tl!("Summarise the selection") } else { tl!("Summarise the document") };
        if ui.add_enabled(!busy, egui::Button::new(what)).clicked() {
            act(app, "ai.summarize", json!({}));
        }
    });
    suggestion_ui(app, ui);
    chat_ui(app, ui);
    ui.add_space(8.0);
    note(ui, tl!("AI answers can be wrong or made up. Docufy offers them as suggestions you check before you accept them."));
}

fn suggestion_ui(app: &mut WordApp, ui: &mut Ui) {
    let Some(s) = app.ai.assist.suggestion.clone() else { return };
    let title = match &s.task {
        Task::Rewrite(how) => fill(tl!("Suggestion: {kind}"), &[("kind", tl!(how.label()))]),
        Task::Continue => tl!("Suggestion: continued text").to_string(),
        Task::Summary { title } => fill(tl!("Summary of {name}"), &[("name", title)]),
    };
    ui.add_space(8.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong());
        if s.phase == Phase::Waiting && s.request.is_none() {
            note(ui, tl!("Reading the document's text…"));
        }
        if !s.text.is_empty() {
            egui::ScrollArea::vertical().id_salt("ai_suggestion").max_height(320.0).show(ui, |ui| {
                ui.add(egui::Label::new(assist::clean_answer(&s.text)).selectable(true).wrap());
            });
        }
        if let Some(n) = &s.note {
            ui.label(RichText::new(n).small().color(ui.visuals().warn_fg_color));
        }
        if let Phase::Failed(e) = &s.phase {
            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
        }
        ui.horizontal_wrapped(|ui| match &s.phase {
            Phase::Waiting | Phase::Streaming => {
                ui.spinner();
                if ui.button(tl!("Stop")).clicked() {
                    act(app, "ai.cancel", json!({}));
                }
            }
            Phase::Ready => {
                match s.target {
                    Target::Replace { .. } => {
                        if ui.button(tl!("Replace selection")).clicked() {
                            act(app, "ai.accept", json!({}));
                        }
                    }
                    Target::Insert { .. } => {
                        if ui.button(tl!("Insert")).clicked() {
                            act(app, "ai.accept", json!({}));
                        }
                    }
                    Target::Free => {
                        if ui.button(tl!("Insert at cursor")).clicked() {
                            act(app, "ai.accept", json!({"as": "insert"}));
                        }
                        if ui.button(tl!("Open as new document")).clicked() {
                            act(app, "ai.accept", json!({"as": "newDocument"}));
                        }
                    }
                }
                if ui.button(tl!("Copy")).clicked() {
                    ui.ctx().copy_text(assist::clean_answer(&s.text));
                }
                if ui.button(tl!("Try again")).clicked()
                    && let Err(e) = retry(app)
                {
                    app.ai.assist.error = Some(e);
                }
                if ui.button(tl!("Discard")).clicked() {
                    act(app, "ai.discard", json!({}));
                }
            }
            Phase::Failed(_) => {
                if s.request.is_some()
                    && ui.button(tl!("Try again")).clicked()
                    && let Err(e) = retry(app)
                {
                    app.ai.assist.error = Some(e);
                }
                if ui.button(tl!("Discard")).clicked() {
                    act(app, "ai.discard", json!({}));
                }
            }
        });
    });
}

fn chat_ui(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Chat");
    let busy_chat = app.ai.assist.job.as_ref().is_some_and(|j| j.chat);
    let turns = app.ai.assist.chat.clone();
    let last = turns.len().saturating_sub(1);
    for (i, t) in turns.iter().enumerate() {
        let (who, color) = match t.role {
            Role::User => (tl!("You"), ui.visuals().text_color()),
            Role::Assistant => (tl!("AI"), ui.visuals().hyperlink_color),
        };
        ui.label(RichText::new(who).small().strong().color(color));
        if t.failed {
            ui.label(RichText::new(&t.text).color(ui.visuals().error_fg_color));
        } else if t.text.is_empty() && i == last && busy_chat {
            ui.spinner();
        } else {
            ui.add(egui::Label::new(assist::clean_answer(&t.text)).selectable(true).wrap());
        }
        if t.role == Role::Assistant && !t.failed && !t.text.is_empty() && !(i == last && busy_chat) {
            ui.horizontal(|ui| {
                if ui.small_button(tl!("Insert at cursor")).clicked() {
                    act(app, "ai.insertAnswer", json!({"index": i}));
                }
                if ui.small_button(tl!("Copy")).clicked() {
                    ui.ctx().copy_text(assist::clean_answer(&t.text));
                }
            });
        }
        ui.add_space(4.0);
    }
    let mut context = app.ai.assist.chat_context;
    craft_ui::ComboBox::from_id_salt("ai_chat_context").selected_text(tl!(context.label())).width(ui.available_width().min(260.0)).show_ui(
        ui,
        |ui| {
            for c in ChatContext::ALL {
                ui.selectable_value(&mut context, c, tl!(c.label()));
            }
        },
    );
    app.ai.assist.chat_context = context;
    let input = ui.add(
        egui::TextEdit::multiline(&mut app.ai.assist.chat_input)
            .hint_text(tl!("Ask the AI… (Ctrl+Enter sends)"))
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    let typed = app.ai.assist.chat_input.trim().to_string();
    let enter = input.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    let busy = busy(app);
    ui.horizontal(|ui| {
        let send = ui.add_enabled(!busy && !typed.is_empty(), egui::Button::new(tl!("Send"))).clicked();
        if (send || (enter && !busy)) && !typed.is_empty() {
            match app.run("ai.chat", json!({"message": typed})) {
                Ok(_) => app.ai.assist.chat_input.clear(),
                Err(e) => app.ai.assist.error = Some(e),
            }
        }
        if busy_chat && ui.button(tl!("Stop")).clicked() {
            act(app, "ai.cancel", json!({}));
        }
        if !app.ai.assist.chat.is_empty() && ui.button(tl!("New chat")).clicked() {
            act(app, "ai.chatClear", json!({}));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> WordApp {
        WordApp::new(docufy_engine::Session::new(docufy_doc::Document::from_text(text)), crate::Services::default())
    }

    /// A finished job whose answer is `events`, as the worker thread would send it.
    fn answered(app: &mut WordApp, chat: bool, events: Vec<StreamEvent>) {
        let (tx, rx) = std::sync::mpsc::channel();
        for e in events {
            tx.send(Msg::Event(e)).unwrap();
        }
        tx.send(Msg::Finished(Ok(()))).unwrap();
        app.ai.assist.job = Some(Job { rx, cancel: Arc::new(AtomicBool::new(false)), chat });
        poll(app);
    }

    fn suggest(app: &mut WordApp, task: Task, target: Target) {
        app.ai.assist.suggestion = Some(Suggestion { task, text: String::new(), target, phase: Phase::Waiting, note: None, request: None });
    }

    #[test]
    fn nothing_is_sent_while_ai_is_off() {
        let mut a = app("Hallo Welt");
        a.run("select.all", json!({})).unwrap();
        for (id, p) in [
            ("ai.rewrite", json!({"style": "shorter"})),
            ("ai.continue", json!({})),
            ("ai.summarize", json!({})),
            ("ai.summarizePdf", json!({})),
            ("ai.chat", json!({"message": "Hallo"})),
        ] {
            let e = a.run(id, p).unwrap_err();
            assert!(e.contains("AI is off"), "{id}: {e}");
        }
        assert!(!busy(&a) && a.ai.assist.suggestion.is_none() && a.ai.assist.chat.is_empty());
        assert!(!a.ai.assist.pdf_summary_wanted);
        // The pane opens and closes either way.
        assert_eq!(a.run("ai.pane", json!({"value": true})).unwrap()["open"], true);
        assert_eq!(a.run("ai.pane", json!({})).unwrap()["open"], false);
    }

    #[test]
    fn bad_parameters_are_refused() {
        let mut a = app("Hallo");
        assert!(a.run("ai.rewrite", json!({})).unwrap_err().contains("style"));
        assert!(a.run("ai.rewrite", json!({"style": "rude"})).unwrap_err().contains("unknown"));
        assert!(a.run("ai.chat", json!({"message": "  "})).unwrap_err().contains("message"));
        assert!(a.run("ai.chat", json!({"message": "x", "context": "everything"})).unwrap_err().contains("context"));
        assert!(a.run("ai.pane", json!({"value": 1})).is_err());
        assert!(a.run("ai.accept", json!({})).unwrap_err().contains("no suggestion"));
        assert!(a.run("ai.insertAnswer", json!({})).unwrap_err().contains("no answer"));
    }

    #[test]
    fn a_rewrite_replaces_the_text_it_was_made_from_in_one_undo_step() {
        let mut a = app("Das ist ein sehr langer und umständlicher Satz.");
        a.run("select.all", json!({})).unwrap();
        let sel = a.session.sel.clone();
        let original = a.session.selected_text();
        let document = a.session.document_id();
        suggest(&mut a, Task::Rewrite(Rewrite::Shorter), Target::Replace { document, sel, original });
        answered(&mut a, false, vec![StreamEvent::Text("Ein kurzer ".into()), StreamEvent::Text("Satz.".into()), StreamEvent::Stop(StopReason::Done)]);
        assert_eq!(a.ai.assist.suggestion.as_ref().map(|s| s.phase.clone()), Some(Phase::Ready));
        assert_eq!(a.run("ai.status", json!({})).unwrap()["suggestion"]["text"], "Ein kurzer Satz.");
        // The caret moved meanwhile: the suggestion still goes where it was made from.
        a.run("select.collapse", json!({})).unwrap();
        a.run("ai.accept", json!({})).unwrap();
        assert_eq!(a.session.doc.plain_text(StoryRef::Body).trim(), "Ein kurzer Satz.");
        assert!(a.ai.assist.suggestion.is_none());
        a.run("edit.undo", json!({})).unwrap();
        assert_eq!(a.session.doc.plain_text(StoryRef::Body).trim(), "Das ist ein sehr langer und umständlicher Satz.");
    }

    #[test]
    fn a_rewrite_of_text_that_changed_meanwhile_is_refused() {
        let mut a = app("Erster Text.");
        a.run("select.all", json!({})).unwrap();
        let sel = a.session.sel.clone();
        let original = a.session.selected_text();
        let document = a.session.document_id();
        suggest(&mut a, Task::Rewrite(Rewrite::Improve), Target::Replace { document, sel, original });
        answered(&mut a, false, vec![StreamEvent::Text("Besser.".into()), StreamEvent::Stop(StopReason::Done)]);
        a.run("text.insert", json!({"text": "Anderer Text."})).unwrap();
        let e = a.run("ai.accept", json!({})).unwrap_err();
        assert!(e.contains("changed"), "{e}");
        assert_eq!(a.session.doc.plain_text(StoryRef::Body).trim(), "Anderer Text.");
        assert!(a.ai.assist.suggestion.is_some(), "the suggestion stays to copy");
    }

    #[test]
    fn continued_text_starts_a_new_paragraph_after_the_caret() {
        let mut a = app("Es war einmal.");
        a.run("select.all", json!({})).unwrap();
        let (_, end) = a.session.sel.ordered();
        let at = end.clone();
        let document = a.session.document_id();
        suggest(&mut a, Task::Continue, Target::Insert { document, at });
        answered(&mut a, false, vec![StreamEvent::Text("Ein König.\n\nEr lebte lang.".into()), StreamEvent::Stop(StopReason::Done)]);
        a.run("ai.accept", json!({})).unwrap();
        let paras: Vec<String> = a.session.doc.plain_text(StoryRef::Body).lines().map(str::to_string).collect();
        assert_eq!(paras, ["Es war einmal.", "Ein König.", "Er lebte lang."]);
    }

    #[test]
    fn a_summary_can_open_as_a_new_document() {
        let mut a = app("Langer Bericht.");
        suggest(&mut a, Task::Summary { title: "Bericht".into() }, Target::Free);
        answered(&mut a, false, vec![StreamEvent::Text("<text>Kurz: ein Bericht.</text>".into()), StreamEvent::Stop(StopReason::Done)]);
        let first = a.session.document_id();
        assert!(a.run("ai.accept", json!({"as": "replace"})).is_err());
        a.run("ai.accept", json!({"as": "newDocument"})).unwrap();
        assert_ne!(a.session.document_id(), first);
        assert_eq!(a.session.doc.plain_text(StoryRef::Body).trim(), "Kurz: ein Bericht.");
    }

    #[test]
    fn errors_refusals_and_cut_off_answers_are_explained() {
        let mut a = app("Text");
        suggest(&mut a, Task::Continue, Target::Free);
        answered(&mut a, false, vec![StreamEvent::Text("Halb".into()), StreamEvent::Stop(StopReason::MaxTokens)]);
        let s = a.ai.assist.suggestion.clone().unwrap();
        assert_eq!(s.phase, Phase::Ready);
        assert!(s.note.unwrap().contains("length limit"));
        suggest(&mut a, Task::Continue, Target::Free);
        answered(&mut a, false, vec![StreamEvent::Stop(StopReason::Refused(Some("cyber".into())))]);
        assert!(matches!(&a.ai.assist.suggestion.as_ref().unwrap().phase, Phase::Failed(e) if e.contains("declined") && e.contains("cyber")));
        assert!(a.run("ai.accept", json!({})).is_err(), "a failed suggestion can't be accepted");
        suggest(&mut a, Task::Continue, Target::Free);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Msg::Finished(Err(AiError::Auth("invalid x-api-key".into())))).unwrap();
        a.ai.assist.job = Some(Job { rx, cancel: Arc::new(AtomicBool::new(false)), chat: false });
        poll(&mut a);
        assert!(matches!(&a.ai.assist.suggestion.as_ref().unwrap().phase, Phase::Failed(e) if e.contains("did not accept the key")));
        assert!(!busy(&a));
    }

    #[test]
    fn chat_answers_stream_in_and_can_be_inserted() {
        let mut a = app("");
        a.ai.assist.chat.push(Turn { role: Role::User, text: "Gruß?".into(), failed: false });
        a.ai.assist.chat.push(Turn { role: Role::Assistant, text: String::new(), failed: false });
        answered(&mut a, true, vec![StreamEvent::Text("Mit freundlichen ".into()), StreamEvent::Text("Grüßen".into()), StreamEvent::Stop(StopReason::Done)]);
        let v = a.run("ai.status", json!({})).unwrap();
        assert_eq!(v["chat"][1]["text"], "Mit freundlichen Grüßen");
        assert_eq!(v["busy"], false);
        assert!(a.run("ai.insertAnswer", json!({"index": 0})).is_err(), "the user's own message isn't an answer");
        a.run("ai.insertAnswer", json!({})).unwrap();
        assert_eq!(a.session.doc.plain_text(StoryRef::Body).trim(), "Mit freundlichen Grüßen");
        a.run("ai.chatClear", json!({})).unwrap();
        assert!(a.ai.assist.chat.is_empty());
    }

    #[test]
    fn paragraphs_drop_blank_lines() {
        assert_eq!(paragraphs("Eins.\n\n\nZwei.  \n"), "Eins.\nZwei.");
    }
}
