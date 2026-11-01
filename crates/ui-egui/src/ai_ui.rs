//! File › Options › AI: turning AI on, the provider and its API key, the model, the device local
//! models run on, what this computer offers, and the notices and warnings about using AI.
//!
//! Settings live in [`crate::UiState::ai`] and change through the `ai.settings` command, so
//! agents can set them too. API keys go to the system's credential store and are never shown
//! whole, returned by a command or written to `ui.json`. Reading the hardware, loading a
//! provider's models and testing the connection run on worker threads.

use std::collections::HashMap;

use docufy_ai::hardware::format_bytes;
use docufy_ai::{AiSettings, DeviceKind, DevicePreference, Hardware, Provider, Severity, warnings};
use egui::{RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::semibold;

/// The AI pane's state while the app runs.
#[derive(Default)]
pub struct AiState {
    /// The key being typed.
    key_input: String,
    /// The stored key per provider, masked; `None` when none is stored. Read once per provider.
    keys: HashMap<Provider, Option<String>>,
    /// What the hardware probe found.
    pub(crate) hardware: Option<Hardware>,
    /// Models loaded from each provider.
    models: HashMap<Provider, Vec<String>>,
    /// Work running on a thread.
    #[cfg(not(target_arch = "wasm32"))]
    job: Option<std::sync::mpsc::Receiver<Done>>,
    /// The last outcome to show: (succeeded, text).
    pub(crate) outcome: Option<(bool, String)>,
    /// A runtime or model being downloaded.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) download: Option<crate::ai_local::Download>,
    /// The AI features: the AI Assistant pane, its suggestion and chat.
    pub assist: crate::ai_assist::Assist,
}

/// A finished worker job.
#[cfg(not(target_arch = "wasm32"))]
enum Done {
    Hardware(Hardware),
    Models(Provider, Result<Vec<String>, docufy_ai::AiError>),
    Test(Result<String, docufy_ai::AiError>),
}

#[cfg(not(target_arch = "wasm32"))]
impl AiState {
    fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// `ai.settings`: with no parameters, the AI settings (never a key); with any of `enabled`,
/// `provider`, `model`, `azureEndpoint`, `azureApiVersion`, `device`, `maxOutputTokens`,
/// `pauseOnBattery`, `dismiss` (a notice id) or `showNotices` (true: show dismissed notices
/// again), changes them. A bad value changes nothing.
pub fn settings_command(app: &mut WordApp, p: &Value) -> Result<Value, String> {
    let mut s = app.ui.ai.clone();
    let text = |k: &str| -> Result<Option<String>, String> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(v)) => Ok(Some(v.trim().to_string())),
            Some(_) => Err(format!("`{k}` must be a string")),
        }
    };
    let flag = |k: &str| -> Result<Option<bool>, String> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(v)) => Ok(Some(*v)),
            Some(_) => Err(format!("`{k}` must be true or false")),
        }
    };
    if let Some(v) = flag("enabled")? {
        s.enabled = v;
    }
    if let Some(id) = text("provider")? {
        s.provider = Provider::from_id(&id).ok_or_else(|| format!("unknown provider `{id}`; one of {}", provider_ids()))?;
    }
    if let Some(m) = text("model")? {
        if m.chars().count() > 200 || m.chars().any(char::is_control) {
            return Err("`model` must be a model name".into());
        }
        if s.provider == Provider::Local && docufy_ai::local::catalog::model(&m).is_none() {
            let ids: Vec<&str> = docufy_ai::local::catalog::MODELS.iter().map(|m| m.id).collect();
            return Err(format!("unknown local model `{m}`; one of {}", ids.join(", ")));
        }
        s.models.insert(s.provider.id().to_string(), m);
    }
    if let Some(e) = text("azureEndpoint")? {
        if !e.is_empty() && !e.starts_with("https://") {
            return Err("`azureEndpoint` must start with https://".into());
        }
        s.azure_endpoint = e;
    }
    if let Some(v) = text("azureApiVersion")? {
        s.azure_api_version = if v.is_empty() { docufy_ai::settings::AZURE_API_VERSION.to_string() } else { v };
    }
    if let Some(d) = text("device")? {
        s.device = device_from_id(&d).ok_or("`device` must be auto, npu, integrated-gpu, discrete-gpu or cpu")?;
    }
    match p.get("maxOutputTokens") {
        None | Some(Value::Null) => {}
        Some(v) => s.max_output_tokens = v.as_u64().filter(|n| (256..=128_000).contains(n)).ok_or("`maxOutputTokens` must be 256 to 128000")? as u32,
    }
    if let Some(v) = flag("pauseOnBattery")? {
        s.pause_on_battery = v;
    }
    if let Some(id) = text("dismiss")? {
        if !warnings::IDS.contains(&id.as_str()) {
            return Err(format!("unknown notice `{id}`"));
        }
        s.dismissed.insert(id);
    }
    if flag("showNotices")? == Some(true) {
        s.dismissed.clear();
    }
    // Another local model or device: the helper process frees the loaded one.
    #[cfg(not(target_arch = "wasm32"))]
    if (s.provider, s.model_for(Provider::Local), s.device) != (app.ui.ai.provider, app.ui.ai.model_for(Provider::Local), app.ui.ai.device) {
        docufy_ai::local::engine::unload();
    }
    app.ui.ai = s;
    Ok(settings_json(&app.ui.ai))
}

fn provider_ids() -> String {
    Provider::ALL.iter().map(|p| p.id()).collect::<Vec<_>>().join(", ")
}

fn settings_json(s: &AiSettings) -> Value {
    json!({
        "enabled": s.enabled,
        "provider": s.provider.id(),
        "model": s.model(),
        "azureEndpoint": s.azure_endpoint,
        "azureApiVersion": s.azure_api_version,
        "device": device_id(s.device),
        "maxOutputTokens": s.max_output_tokens,
        "pauseOnBattery": s.pause_on_battery,
        "dismissed": s.dismissed,
    })
}

const DEVICES: [(&str, DevicePreference); 5] = [
    ("auto", DevicePreference::Auto),
    ("discrete-gpu", DevicePreference::Only(DeviceKind::DiscreteGpu)),
    ("npu", DevicePreference::Only(DeviceKind::Npu)),
    ("integrated-gpu", DevicePreference::Only(DeviceKind::IntegratedGpu)),
    ("cpu", DevicePreference::Only(DeviceKind::Cpu)),
];

fn device_id(d: DevicePreference) -> &'static str {
    DEVICES.iter().find(|(_, v)| *v == d).map_or("auto", |(id, _)| id)
}

fn device_from_id(id: &str) -> Option<DevicePreference> {
    DEVICES.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

fn device_name(d: DevicePreference) -> &'static str {
    match d {
        DevicePreference::Auto => "Automatic (best available)",
        DevicePreference::Only(kind) => kind.name(),
    }
}

/// Changes AI settings through `ai.settings`.
fn set(app: &mut WordApp, params: Value) {
    if let Err(e) = app.run("ai.settings", params) {
        app.status(e);
    }
}

pub(crate) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(tl!(title)).font(semibold(15.0)));
    ui.add_space(2.0);
}

pub(crate) fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// `template` with each `{name}` replaced by its value.
pub(crate) fn fill(template: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(template.to_string(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
}

/// The AI pane.
pub fn pane(app: &mut WordApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    poll(app, ui.ctx());
    section(ui, "AI assistance");
    let mut enabled = app.ui.ai.enabled;
    if ui.checkbox(&mut enabled, tl!("Use AI")).changed() {
        set(app, json!({"enabled": enabled}));
    }
    note(ui, tl!("Off: Docufy sends nothing to an AI provider and runs no AI model. Nothing changes until you turn it on."));
    if cfg!(target_arch = "wasm32") {
        note(ui, tl!("AI is available in the desktop app."));
        return;
    }
    if !app.ui.ai.enabled {
        return;
    }
    provider_section(app, ui);
    if app.ui.ai.provider.is_cloud() {
        key_section(app, ui);
        model_section(app, ui);
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::ai_local::section(app, ui);
            ui.horizontal(|ui| {
                let busy = app.ai.busy();
                if ui.add_enabled(!busy, egui::Button::new(tl!("Test the model"))).clicked() {
                    start_test(app, ui.ctx());
                }
                if busy {
                    ui.spinner();
                }
            });
        }
    }
    device_section(app, ui);
    hardware_section(app, ui);
    notices_section(app, ui);
    if let Some((ok, text)) = &app.ai.outcome {
        ui.add_space(6.0);
        let color = if *ok { ui.visuals().text_color() } else { ui.visuals().error_fg_color };
        ui.label(RichText::new(text).color(color));
    }
}

fn provider_section(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Provider");
    let current = app.ui.ai.provider;
    let mut chosen = current;
    ui.horizontal(|ui| {
        let label = ui.label(tl!("AI provider:"));
        craft_ui::ComboBox::from_id_salt("ai_provider")
            .selected_text(tl!(current.name()))
            .width(260.0)
            .show_ui(ui, |ui| {
                for p in Provider::ALL {
                    ui.selectable_value(&mut chosen, p, tl!(p.name()));
                }
            })
            .response
            .labelled_by(label.id);
    });
    if chosen != current {
        app.ai.key_input.clear();
        app.ai.outcome = None;
        set(app, json!({"provider": chosen.id()}));
    }
}

fn key_section(app: &mut WordApp, ui: &mut Ui) {
    let provider = app.ui.ai.provider;
    section(ui, "Sign-in (API key)");
    note(ui, tl!("The provider gives you an API key after you sign in on its page. Docufy keeps it in the system's credential store."));
    if let Some(url) = provider.key_page() {
        let label = fill(tl!("Sign in to {provider} and get a key…"), &[("provider", tl!(provider.name()))]);
        if ui.button(label).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let stored = app.ai.keys.entry(provider).or_insert_with(|| match docufy_ai::secrets::load(provider) {
            Ok(key) => key.map(|k| docufy_ai::secrets::masked(&k)),
            Err(e) => {
                log::warn!("ai: {e}");
                None
            }
        });
        if let Some(masked) = stored.clone() {
            ui.horizontal(|ui| {
                ui.label(fill(tl!("Key stored: {key}"), &[("key", &masked)]));
                if ui.button(tl!("Remove key")).clicked() {
                    match docufy_ai::secrets::delete(provider) {
                        Ok(()) => {
                            app.ai.keys.insert(provider, None);
                            app.ai.outcome = None;
                        }
                        Err(e) => app.ai.outcome = Some((false, e)),
                    }
                }
            });
        } else {
            ui.horizontal(|ui| {
                let label = ui.label(tl!("API key:"));
                ui.add(egui::TextEdit::singleline(&mut app.ai.key_input).password(true).desired_width(300.0)).labelled_by(label.id);
                let key = app.ai.key_input.trim().to_string();
                if ui.add_enabled(!key.is_empty(), egui::Button::new(tl!("Save key"))).clicked() {
                    match docufy_ai::secrets::store(provider, &key) {
                        Ok(()) => {
                            app.ai.keys.insert(provider, Some(docufy_ai::secrets::masked(&key)));
                            app.ai.outcome = Some((true, tl!("The key is stored.").to_string()));
                        }
                        Err(e) => app.ai.outcome = Some((false, e)),
                    }
                    app.ai.key_input.clear();
                }
            });
        }
    }
}

fn model_section(app: &mut WordApp, ui: &mut Ui) {
    let provider = app.ui.ai.provider;
    section(ui, if provider == Provider::AzureOpenAi { "Resource and deployment" } else { "Model" });
    if provider == Provider::AzureOpenAi {
        let mut endpoint = app.ui.ai.azure_endpoint.clone();
        let mut changed = false;
        egui::Grid::new("ai_azure").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            let label = ui.label(tl!("Endpoint:"));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut endpoint).hint_text("https://<name>.openai.azure.com").desired_width(320.0))
                .labelled_by(label.id)
                .lost_focus();
            ui.end_row();
        });
        if changed && endpoint.trim() != app.ui.ai.azure_endpoint {
            set(app, json!({"azureEndpoint": endpoint.trim()}));
        }
    }
    let mut model = app.ui.ai.model().unwrap_or_default();
    let before = model.clone();
    let loaded = app.ai.models.get(&provider).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        let label = ui.label(tl!(if provider == Provider::AzureOpenAi { "Deployment name:" } else { "Model:" }));
        ui.add(egui::TextEdit::singleline(&mut model).desired_width(240.0)).labelled_by(label.id);
        if !loaded.is_empty() {
            craft_ui::ComboBox::from_id_salt("ai_models").selected_text(tl!("Choose…")).width(200.0).show_ui(ui, |ui| {
                for m in &loaded {
                    ui.selectable_value(&mut model, m.clone(), m);
                }
            });
        }
    });
    if model.trim() != before.trim() && !model.trim().is_empty() {
        set(app, json!({"model": model.trim()}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    ui.horizontal(|ui| {
        let busy = app.ai.busy();
        if provider != Provider::AzureOpenAi && ui.add_enabled(!busy, egui::Button::new(tl!("Load models"))).clicked() {
            start_models(app, ui.ctx());
        }
        if ui.add_enabled(!busy, egui::Button::new(tl!("Test connection"))).clicked() {
            start_test(app, ui.ctx());
        }
        if busy {
            ui.spinner();
        }
    });
}

fn device_section(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Device for local models");
    let current = app.ui.ai.device;
    let mut chosen = current;
    let hw = app.ai.hardware.clone();
    ui.horizontal(|ui| {
        let label = ui.label(tl!("Run on:"));
        craft_ui::ComboBox::from_id_salt("ai_device")
            .selected_text(tl!(device_name(current)))
            .width(280.0)
            .show_ui(ui, |ui| {
                for (_, d) in DEVICES {
                    let missing = matches!((d, &hw), (DevicePreference::Only(kind), Some(hw)) if !hw.has(kind));
                    let text = if missing { format!("{} ({})", tl!(device_name(d)), tl!("not found")) } else { tl!(device_name(d)).to_string() };
                    ui.selectable_value(&mut chosen, d, text);
                }
            })
            .response
            .labelled_by(label.id);
    });
    if chosen != current {
        set(app, json!({"device": device_id(chosen)}));
    }
    note(ui, tl!("Automatic uses the graphics card if the model fits its memory, then the NPU, then integrated graphics, then the processor."));
    let mut pause = app.ui.ai.pause_on_battery;
    if ui.checkbox(&mut pause, tl!("Pause local AI while on battery")).changed() {
        set(app, json!({"pauseOnBattery": pause}));
    }
}

fn hardware_section(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "This computer");
    #[cfg(not(target_arch = "wasm32"))]
    if app.ai.hardware.is_none() && !app.ai.busy() {
        start_probe(app, ui.ctx());
    }
    let Some(hw) = app.ai.hardware.clone() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tl!("Reading the hardware…"));
        });
        return;
    };
    egui::Grid::new("ai_hardware").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
        ui.label(tl!("Processor:"));
        ui.label(fill(tl!("{name}, {cores} cores"), &[("name", &hw.cpu), ("cores", &hw.cpu_cores.to_string())]));
        ui.end_row();
        ui.label(tl!("Memory (RAM):"));
        ui.label(fill(tl!("{total}, {free} free"), &[("total", &format_bytes(hw.ram_total)), ("free", &format_bytes(hw.ram_available))]));
        ui.end_row();
        for g in &hw.gpus {
            ui.label(tl!(g.kind.name()));
            let memory = match g.vram_total {
                Some(total) => fill(tl!("{name}, {total} own memory"), &[("name", &g.name), ("total", &format_bytes(total))]),
                None => fill(tl!("{name}, uses the computer's RAM"), &[("name", &g.name)]),
            };
            ui.label(memory);
            ui.end_row();
        }
        ui.label(tl!("NPU (AI processor):"));
        if hw.npus.is_empty() {
            ui.label(tl!("none found"));
        } else {
            ui.label(hw.npus.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(", "));
        }
        ui.end_row();
        ui.label(tl!("Power:"));
        ui.label(match (hw.on_battery, hw.battery_percent) {
            (Some(true), Some(p)) => fill(tl!("on battery ({percent} %)"), &[("percent", &format!("{p:.0}"))]),
            (Some(true), None) => tl!("on battery").to_string(),
            _ => tl!("plugged in").to_string(),
        });
        ui.end_row();
    });
    #[cfg(not(target_arch = "wasm32"))]
    if ui.add_enabled(!app.ai.busy(), egui::Button::new(tl!("Read again"))).clicked() {
        start_probe(app, ui.ctx());
    }
}

fn notices_section(app: &mut WordApp, ui: &mut Ui) {
    let s = app.ui.ai.clone();
    let list = if s.provider.is_cloud() {
        let has_key = app.ai.keys.get(&s.provider).is_some_and(Option::is_some);
        warnings::for_cloud(&s, has_key)
    } else {
        match (docufy_ai::local::chosen(&s), &app.ai.hardware) {
            (Some(m), Some(hw)) => warnings::for_local(&s, hw, &m.local_model()),
            _ => Vec::new(),
        }
    };
    if list.is_empty() && s.dismissed.is_empty() {
        return;
    }
    section(ui, "Notices");
    let mut dismiss = None;
    for w in &list {
        ui.horizontal_wrapped(|ui| {
            let (mark, color) = match w.severity {
                Severity::Blocking => ("⛔", ui.visuals().error_fg_color),
                Severity::Warning => ("⚠", ui.visuals().warn_fg_color),
                Severity::Info => ("ℹ", ui.visuals().text_color()),
            };
            ui.label(RichText::new(mark).color(color));
            let args: Vec<(&str, String)> = w.args.iter().map(|(n, v)| (*n, tl!(v).to_string())).collect();
            let args: Vec<(&str, &str)> = args.iter().map(|(n, v)| (*n, v.as_str())).collect();
            ui.label(fill(tl!(w.template), &args));
            if w.severity == Severity::Info && ui.small_button(tl!("Don't show again")).clicked() {
                dismiss = Some(w.id);
            }
        });
    }
    if let Some(id) = dismiss {
        set(app, json!({"dismiss": id}));
    }
    if !s.dismissed.is_empty() && ui.small_button(tl!("Show all notices again")).clicked() {
        set(app, json!({"showNotices": true}));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(app: &mut WordApp, ctx: &egui::Context, work: impl FnOnce() -> Done + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let started = std::thread::Builder::new().name("docufy-ai".into()).spawn(move || {
        let _ = tx.send(work());
        ctx.request_repaint();
    });
    match started {
        Ok(_) => app.ai.job = Some(rx),
        Err(e) => app.ai.outcome = Some((false, e.to_string())),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_probe(app: &mut WordApp, ctx: &egui::Context) {
    // Free disk space where models are kept.
    let dir = docufy_ai::local::data_dir().unwrap_or_else(std::env::temp_dir);
    spawn(app, ctx, move || Done::Hardware(docufy_ai::probe::probe(&[], &dir)));
}

/// The stored key for the current provider, or an outcome saying there is none.
#[cfg(not(target_arch = "wasm32"))]
fn key(app: &mut WordApp) -> Option<String> {
    if app.ui.ai.provider == Provider::Local {
        return Some(String::new());
    }
    match docufy_ai::secrets::load(app.ui.ai.provider) {
        Ok(Some(k)) => Some(k),
        Ok(None) => {
            app.ai.outcome = Some((false, tl!("Store an API key first.").to_string()));
            None
        }
        Err(e) => {
            app.ai.outcome = Some((false, e));
            None
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_models(app: &mut WordApp, ctx: &egui::Context) {
    let Some(key) = key(app) else { return };
    let settings = app.ui.ai.clone();
    app.ai.outcome = None;
    spawn(app, ctx, move || Done::Models(settings.provider, docufy_ai::http::list_models(&settings, &key)));
}

#[cfg(not(target_arch = "wasm32"))]
fn start_test(app: &mut WordApp, ctx: &egui::Context) {
    let Some(key) = key(app) else { return };
    let settings = app.ui.ai.clone();
    let hw = app.ai.hardware.clone();
    app.ai.outcome = None;
    spawn(app, ctx, move || {
        let req = docufy_ai::ChatRequest {
            system: String::new(),
            messages: vec![docufy_ai::Message::user("Reply with the single word OK.")],
            max_tokens: 256,
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut answer = String::new();
        let mut stop = None;
        let sent = docufy_ai::stream_chat(&settings, &key, hw.as_ref(), &req, &cancel, |e| match e {
            docufy_ai::StreamEvent::Text(t) => answer.push_str(&t),
            docufy_ai::StreamEvent::Stop(s) => stop = Some(s),
        });
        Done::Test(sent.map(|()| match stop {
            Some(docufy_ai::StopReason::Refused(why)) => format!("refused: {}", why.unwrap_or_default()),
            _ => answer.trim().to_string(),
        }))
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn poll(app: &mut WordApp, ctx: &egui::Context) {
    let Some(rx) = &app.ai.job else { return };
    let done = match rx.try_recv() {
        Ok(done) => done,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            app.ai.job = None;
            return;
        }
    };
    app.ai.job = None;
    match done {
        Done::Hardware(hw) => app.ai.hardware = Some(hw),
        Done::Models(p, Ok(list)) => {
            let n = list.len().to_string();
            app.ai.models.insert(p, list);
            app.ai.outcome = Some((true, fill(tl!("{count} models loaded."), &[("count", &n)])));
        }
        Done::Models(_, Err(e)) | Done::Test(Err(e)) => app.ai.outcome = Some((false, error_text(&e))),
        Done::Test(Ok(answer)) => {
            app.ai.outcome = Some((true, fill(tl!("Connected. The model answered: {answer}"), &[("answer", &answer)])));
        }
    }
}

/// What went wrong, in words the user can act on.
pub(crate) fn error_text(e: &docufy_ai::AiError) -> String {
    use docufy_ai::AiError;
    let (template, detail) = match e {
        AiError::Auth(d) => ("The provider did not accept the key. Check it, or create a new one. ({detail})", d.as_str()),
        AiError::RateLimit(d) => ("The provider's limit or your credit is used up. Wait, or check your account. ({detail})", d.as_str()),
        AiError::Unavailable(d) => ("The provider is busy or unavailable. Try again later. ({detail})", d.as_str()),
        AiError::Rejected(d) => ("The provider rejected the request; check the model name. ({detail})", d.as_str()),
        AiError::Network(d) => ("No connection to the provider. Check the internet connection and proxy. ({detail})", d.as_str()),
        AiError::Protocol(d) => ("The provider's answer could not be read. ({detail})", d.as_str()),
        AiError::Local(d) => ("The AI on this computer could not answer: {detail}", d.as_str()),
        AiError::Cancelled => ("Cancelled.", ""),
    };
    fill(tl!(template), &[("detail", detail)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_settings_change_through_the_command_and_never_show_a_key() {
        let mut a = WordApp::new(docufy_engine::Session::new(docufy_doc::Document::new()), crate::Services::default());
        let v = a.run("ai.settings", json!({})).unwrap();
        assert_eq!(v["enabled"], false, "off until turned on");
        let v = a.run("ai.settings", json!({"enabled": true, "provider": "mistral", "model": "mistral-small-latest", "device": "npu"})).unwrap();
        assert_eq!(
            (v["enabled"].clone(), v["provider"].clone(), v["model"].clone(), v["device"].clone()),
            (json!(true), json!("mistral"), json!("mistral-small-latest"), json!("npu"))
        );
        assert!(!v.to_string().to_lowercase().contains("key"), "{v}");
        assert!(a.ui.ai.enabled);
        // A bad value changes nothing.
        assert!(a.run("ai.settings", json!({"provider": "copilot", "enabled": false})).is_err());
        assert!(a.run("ai.settings", json!({"azureEndpoint": "http://x", "enabled": false})).is_err());
        assert!(a.run("ai.settings", json!({"maxOutputTokens": 5})).is_err());
        assert!(a.run("ai.settings", json!({"dismiss": "everything"})).is_err());
        assert!(a.ui.ai.enabled);
        a.run("ai.settings", json!({"dismiss": "cloud-costs"})).unwrap();
        assert!(a.ui.ai.dismissed.contains("cloud-costs"));
        a.run("ai.settings", json!({"showNotices": true})).unwrap();
        assert!(a.ui.ai.dismissed.is_empty());
    }

    #[test]
    fn ai_settings_are_saved_with_the_preferences() {
        let mut a = WordApp::new(docufy_engine::Session::new(docufy_doc::Document::new()), crate::Services::default());
        a.run("ai.settings", json!({"enabled": true, "provider": "gemini"})).unwrap();
        let saved = serde_json::to_vec(&a.prefs()).unwrap();
        let back = crate::UiState::from_json(&saved).unwrap();
        assert_eq!(back.ai, a.ui.ai);
        // A damaged AI section reads as AI off, and the rest of the file still loads.
        let mut v: Value = serde_json::from_slice(&saved).unwrap();
        v["ai"] = json!("broken");
        v["recentCount"] = json!(7);
        let back = crate::UiState::from_json(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(!back.ai.enabled);
        assert_eq!(back.recent_count, 7);
    }
}
