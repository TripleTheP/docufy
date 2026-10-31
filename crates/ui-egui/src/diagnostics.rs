//! Help › Diagnostics: a pane for technical users and support. A live resource monitor (Docufy's
//! and the computer's processor, memory and graphics memory, frame times), what Docufy is doing,
//! a self-test with what to do about each problem, the system information for a bug report
//! (without personal paths), the log, safe mode and resetting the settings at the next start.
//!
//! Everything is also a `help.*` command. The desktop app reports what only it knows (graphics
//! adapter, log file, settings folder, safe mode, PDF mode) through
//! [`WordApp::set_host_diagnostics`] and the frame times through [`WordApp::record_frame`].

use std::path::PathBuf;

use docufy_diag::history::{Frames, History};
use docufy_diag::selftest::{self, Check, Status};
use docufy_diag::{Sample, format_bytes, startup};
use egui::{Color32, RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::WordApp;
use crate::ai_ui::{fill, note, section};

/// Samples kept: two minutes at one a second.
const KEPT: usize = 120;

/// What only the desktop app knows.
#[derive(Debug, Clone, Default)]
pub struct HostDiagnostics {
    /// The adapter the window draws with.
    pub graphics: Option<selftest::Graphics>,
    /// Its type and driver, as the graphics driver reports them.
    pub graphics_detail: String,
    /// Build identification (commit).
    pub build: String,
    pub log_file: Option<PathBuf>,
    /// Where preferences live; `None` when this run keeps none.
    pub settings_dir: Option<PathBuf>,
    pub safe_mode: bool,
    /// PDFs open in PDF mode.
    pub pdf_documents: usize,
    /// PDF mode is shown.
    pub pdf_mode: bool,
}

pub struct Diagnostics {
    /// Whether the Diagnostics pane is shown.
    pub open: bool,
    history: History,
    frames: Frames,
    #[cfg(not(target_arch = "wasm32"))]
    sampler: Option<std::sync::mpsc::Receiver<Sample>>,
    checks: Option<Vec<Check>>,
    pub host: HostDiagnostics,
    /// Reset was clicked once; the second click confirms.
    confirm_reset: bool,
    /// The last outcome to show: (succeeded, text).
    outcome: Option<(bool, String)>,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Diagnostics {
            open: false,
            history: History::new(KEPT),
            frames: Frames::default(),
            #[cfg(not(target_arch = "wasm32"))]
            sampler: None,
            checks: None,
            host: HostDiagnostics::default(),
            confirm_reset: false,
            outcome: None,
        }
    }
}

impl WordApp {
    /// The desktop app's part of the diagnostics.
    pub fn set_host_diagnostics(&mut self, host: HostDiagnostics) {
        self.diagnostics.host = host;
    }

    /// Update the PDF mode's part of the diagnostics (every frame; cheap).
    pub fn set_pdf_diagnostics(&mut self, documents: usize, shown: bool) {
        self.diagnostics.host.pdf_documents = documents;
        self.diagnostics.host.pdf_mode = shown;
    }

    /// A frame drawn at `at` seconds (a clock that only grows) that took `took` seconds of
    /// processor time.
    pub fn record_frame(&mut self, at: f64, took: f32) {
        self.diagnostics.frames.record(at, took);
    }

    /// Docufy runs in safe mode: drawn by the processor, without AI.
    pub fn safe_mode(&self) -> bool {
        self.diagnostics.host.safe_mode
    }

    /// Open the Diagnostics pane (PDF mode's Help menu).
    pub fn open_diagnostics(&mut self) {
        self.diagnostics.open = true;
    }

    /// Tests: a resource sample as the monitor would deliver it.
    #[cfg(test)]
    pub(crate) fn push_resource_sample(&mut self, s: Sample) {
        self.diagnostics.history.push(s);
    }
}

/// The `help.*` diagnostics commands; `None` for other ids.
pub fn command(app: &mut WordApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let flag = |k: &str| match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(v)) => Ok(Some(*v)),
        Some(_) => Err(format!("`{k}` must be true or false")),
    };
    Some(match id {
        "help.diagnostics" => flag("value").map(|v| {
            app.diagnostics.open = v.unwrap_or(!app.diagnostics.open);
            json!({"open": app.diagnostics.open})
        }),
        "help.resources" => Ok(resources(app)),
        "help.selfTest" => {
            let checks = run_self_test(app);
            Ok(json!({"checks": checks.iter().map(check_json).collect::<Vec<_>>()}))
        }
        "help.systemInfo" => flag("copy").map(|copy| {
            let text = system_info(app);
            if copy == Some(true)
                && let Some(ctx) = &app.ctx
            {
                ctx.copy_text(text.clone());
            }
            json!({"text": text})
        }),
        "help.safeMode" => flag("value").and_then(|v| next_start(app, startup::SAFE_MODE, v)),
        "help.resetSettings" => flag("value").and_then(|v| next_start(app, startup::RESET_SETTINGS, v)),
        "help.openLog" => flag("folder").and_then(|folder| open_log(app, folder == Some(true))),
        _ => return None,
    })
}

/// Read or set what the next start does (`marker`).
fn next_start(app: &WordApp, marker: &str, value: Option<bool>) -> Result<Value, String> {
    let Some(dir) = app.diagnostics.host.settings_dir.clone() else {
        return Err("this run keeps no settings, so there is nothing to change at the next start".into());
    };
    if let Some(on) = value {
        startup::set(&dir, marker, on)?;
    }
    Ok(json!({"nextStart": startup::is_set(&dir, marker), "safeModeNow": app.diagnostics.host.safe_mode}))
}

fn open_log(app: &WordApp, folder: bool) -> Result<Value, String> {
    let Some(file) = app.diagnostics.host.log_file.clone() else { return Err("this run writes no log file".into()) };
    let target = if folder { file.parent().map(PathBuf::from).unwrap_or(file) } else { file };
    #[cfg(not(target_arch = "wasm32"))]
    open::that_detached(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(json!({"opened": target.display().to_string()}))
}

fn check_json(c: &Check) -> Value {
    let status = match c.status {
        Status::Ok => "ok",
        Status::Warn => "warning",
        Status::Fail => "problem",
    };
    json!({"id": c.id, "status": status, "title": c.title, "text": c.text(), "fix": c.fix})
}

/// The latest sample (a fresh one when the monitor isn't running) and the frame times.
fn resources(app: &WordApp) -> Value {
    #[cfg(not(target_arch = "wasm32"))]
    let fresh = app.diagnostics.history.latest().is_none().then(|| docufy_diag::monitor::Sampler::new().sample());
    #[cfg(target_arch = "wasm32")]
    let fresh: Option<Sample> = None;
    let s = app.diagnostics.history.latest().cloned().or(fresh).unwrap_or_default();
    let (median, p95) = app.diagnostics.frames.times_ms().unwrap_or((0.0, 0.0));
    json!({
        "app": {"cpuPercent": s.app_cpu, "ramBytes": s.app_ram, "gpuPercent": s.app_gpu, "vramBytes": s.app_vram},
        "system": {"cpuPercent": s.cpu, "ramUsedBytes": s.ram_used, "ramTotalBytes": s.ram_total},
        "gpus": s.gpus.iter().map(|g| json!({"name": g.name, "vramUsedBytes": g.vram_used, "vramTotalBytes": g.vram_total})).collect::<Vec<_>>(),
        "frames": {"perSecond": app.diagnostics.frames.per_second(), "medianMs": median, "p95Ms": p95},
        "documents": app.session.documents().len(),
        "windows": app.window_count(),
        "pdfDocuments": app.diagnostics.host.pdf_documents,
        "jobs": jobs(app),
        "safeMode": app.diagnostics.host.safe_mode,
    })
}

/// What is running in the background, in English (for commands and the system information).
fn jobs(app: &WordApp) -> Vec<&'static str> {
    let mut out = Vec::new();
    if app.printing.busy() {
        out.push("printing");
    }
    if crate::ai_assist::busy(app) {
        out.push("AI request");
    }
    if app.services.async_saves.as_ref().is_some_and(|s| s.pending() > 0) {
        out.push("saving");
    }
    out
}

fn run_self_test(app: &mut WordApp) -> Vec<Check> {
    let host = &app.diagnostics.host;
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut i = selftest::Inputs {
        settings: host.settings_dir.clone().map(|d| {
            let ok = selftest::writable(&d);
            (d, ok)
        }),
        graphics: host.graphics.clone(),
        log_file: host.log_file.clone(),
        fonts: docufy_fonts::FontDb::global().families().len(),
        safe_mode: host.safe_mode,
        ai: selftest::AiState {
            enabled: app.ui.ai.enabled,
            local: !app.ui.ai.provider.is_cloud(),
            has_key: crate::ai_assist::key_stored(app.ui.ai.provider),
            provider: app.ui.ai.provider.name().to_string(),
        },
        ..Default::default()
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        i.disk_free = host.settings_dir.as_deref().and_then(selftest::free_space);
        let s = docufy_diag::monitor::Sampler::new().sample();
        i.ram_total = s.ram_total;
        i.ram_available = s.ram_total.saturating_sub(s.ram_used);
    }
    let checks = selftest::run(&i);
    app.diagnostics.checks = Some(checks.clone());
    checks
}

/// The system information for a bug report, in English, without the user's home folder or name.
pub fn system_info(app: &WordApp) -> String {
    let d = &app.diagnostics;
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut add = |k: &str, v: String| lines.push((k.to_string(), v));
    add("Docufy", format!("{} {}", env!("CARGO_PKG_VERSION"), d.host.build).trim().to_string());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let m = docufy_diag::monitor::machine();
        add("System", m.os);
        add("Processor", format!("{} ({} logical cores)", m.cpu, m.cores));
    }
    #[cfg(target_arch = "wasm32")]
    add("System", "web".into());
    if let Some(s) = d.history.latest() {
        add("Memory", format!("{} of {} used; Docufy {}", format_bytes(s.ram_used), format_bytes(s.ram_total), format_bytes(s.app_ram)));
        for g in &s.gpus {
            let vram = match (g.vram_used, g.vram_total) {
                (Some(u), Some(t)) => format!("{} of {} graphics memory used", format_bytes(u), format_bytes(t)),
                _ => "shares the computer's memory".into(),
            };
            add("Graphics chip", format!("{}, {vram}", g.name));
        }
    }
    match &d.host.graphics {
        Some(g) => add(
            "Drawing with",
            format!("{} ({}{}){}", g.name, g.backend, if g.software { ", software" } else { "" }, if d.host.graphics_detail.is_empty() { String::new() } else { format!(", {}", d.host.graphics_detail) }),
        ),
        None => add("Drawing with", "unknown".into()),
    }
    if let Some((median, p95)) = d.frames.times_ms() {
        add("Frame time", format!("median {median:.1} ms, 95% under {p95:.1} ms"));
    }
    add("Interface language", app.ui.language.clone());
    add("Documents", format!("{} open in {} window(s); {} PDF(s) open", app.session.documents().len(), app.window_count(), d.host.pdf_documents));
    let jobs = jobs(app);
    add("Background", if jobs.is_empty() { "idle".into() } else { jobs.join(", ") });
    let ai = &app.ui.ai;
    add("AI", if ai.enabled { format!("on, {} ({})", ai.provider.name(), ai.model().unwrap_or_default()) } else { "off".into() });
    add("Safe mode", if d.host.safe_mode { "yes".into() } else { "no".into() });
    if let Some(checks) = &d.checks {
        for c in checks.iter().filter(|c| c.status != Status::Ok) {
            add(&format!("Self-test {}", c.title), c.text());
        }
    }
    add("Log file", d.host.log_file.as_ref().map_or_else(|| "none".into(), |p| p.display().to_string()));
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok();
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).ok();
    docufy_diag::info::report(&lines, home.as_deref(), user.as_deref())
}

/// Take in the monitor's samples while the pane is open; start and stop the monitor with it.
#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
pub fn poll(app: &mut WordApp) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let d = &mut app.diagnostics;
        if !d.open {
            // Dropping the receiver ends the sampling thread.
            d.sampler = None;
            return;
        }
        if d.sampler.is_none() {
            let ctx = app.ctx.clone();
            d.sampler = docufy_diag::monitor::spawn(std::time::Duration::from_secs(1), move || {
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            });
        }
        if let Some(rx) = &d.sampler {
            while let Ok(s) = rx.try_recv() {
                d.history.push(s);
            }
        }
    }
}

fn act(app: &mut WordApp, id: &str, params: Value) {
    match app.run(id, params) {
        Ok(_) => {}
        Err(e) => app.diagnostics.outcome = Some((false, e)),
    }
}

/// The Diagnostics pane.
pub fn pane(app: &mut WordApp, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(ui.available_width());
        if app.diagnostics.host.safe_mode {
            ui.label(RichText::new(tl!("Docufy runs in safe mode: without graphics acceleration and without AI.")).color(ui.visuals().warn_fg_color));
        }
        resources_ui(app, ui);
        docufy_ui(app, ui);
        self_test_ui(app, ui);
        troubleshooting_ui(app, ui);
        if let Some((ok, text)) = &app.diagnostics.outcome {
            let color = if *ok { ui.visuals().text_color() } else { ui.visuals().error_fg_color };
            ui.label(RichText::new(text).color(color));
        }
    });
}

/// A small curve of `values` (0..=`max`), newest on the right.
fn sparkline(ui: &mut Ui, values: &[f32], max: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(260.0), 26.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_stroke(rect, 2.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    if values.len() < 2 || max <= 0.0 {
        return;
    }
    let step = rect.width() / (KEPT - 1) as f32;
    let x0 = rect.right() - step * (values.len() - 1) as f32;
    let points: Vec<egui::Pos2> =
        values.iter().enumerate().map(|(i, v)| egui::pos2(x0 + step * i as f32, rect.bottom() - 2.0 - (v / max).clamp(0.0, 1.0) * (rect.height() - 4.0))).collect();
    p.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
}

fn resources_ui(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Resources");
    let d = &app.diagnostics;
    let Some(s) = d.history.latest().cloned() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tl!("Measuring…"));
        });
        return;
    };
    let accent = ui.visuals().hyperlink_color;
    let cpu = d.history.series(|s| s.app_cpu);
    let ram = d.history.series(|s| s.app_ram as f32);
    let ram_peak = d.history.peak(|s| s.app_ram as f32);
    ui.label(fill(tl!("Docufy processor: {value} %"), &[("value", &format!("{:.1}", s.app_cpu))]));
    sparkline(ui, &cpu, cpu.iter().copied().fold(5.0, f32::max), accent);
    ui.label(fill(tl!("Docufy memory: {value} (peak {peak})"), &[("value", &format_bytes(s.app_ram)), ("peak", &format_bytes(ram_peak as u64))]));
    sparkline(ui, &ram, ram_peak * 1.2, accent);
    if let Some(gpu) = s.app_gpu {
        let vram = s.app_vram.map(format_bytes).unwrap_or_default();
        ui.label(fill(tl!("Docufy graphics: {value} %, {memory} graphics memory"), &[("value", &format!("{gpu:.0}")), ("memory", &vram)]));
    }
    ui.add_space(4.0);
    row(ui, tl!("Computer processor:"), &format!("{:.0} %", s.cpu));
    row(ui, tl!("Computer memory:"), &fill(tl!("{used} of {total}"), &[("used", &format_bytes(s.ram_used)), ("total", &format_bytes(s.ram_total))]));
    for g in &s.gpus {
        let memory = match (g.vram_used, g.vram_total) {
            (Some(u), Some(t)) => fill(tl!("{used} of {total} graphics memory"), &[("used", &format_bytes(u)), ("total", &format_bytes(t))]),
            _ => tl!("uses the computer's memory").to_string(),
        };
        row(ui, &fill(tl!("{name}:"), &[("name", &g.name)]), &memory);
    }
    let frames = match d.frames.times_ms() {
        Some((median, p95)) => fill(
            tl!("{median} ms (95 % under {p95} ms), {fps} frames/s"),
            &[("median", &format!("{median:.1}")), ("p95", &format!("{p95:.1}")), ("fps", &d.frames.per_second().to_string())],
        ),
        None => "–".into(),
    };
    row(ui, tl!("Frame time:"), &frames);
    note(ui, tl!("Docufy draws only when something changes, so a still window shows few frames per second."));
}

/// "Label value", wrapping in a narrow pane.
fn row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(label).strong());
        ui.label(value);
    });
}

fn docufy_ui(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Docufy");
    let host = app.diagnostics.host.clone();
    let jobs = jobs(app);
    row(ui, tl!("Version:"), format!("{} {}", env!("CARGO_PKG_VERSION"), host.build).trim());
    let drawing = match &host.graphics {
        Some(g) if g.software => fill(tl!("{name} ({backend}, on the processor)"), &[("name", &g.name), ("backend", &g.backend)]),
        Some(g) => format!("{} ({})", g.name, g.backend),
        None => "–".into(),
    };
    row(ui, tl!("Drawing with:"), &drawing);
    let open = fill(
        tl!("{documents} documents in {windows} windows, {pdfs} PDFs"),
        &[("documents", &app.session.documents().len().to_string()), ("windows", &app.window_count().to_string()), ("pdfs", &host.pdf_documents.to_string())],
    );
    row(ui, tl!("Open:"), &open);
    let background = if jobs.is_empty() { tl!("nothing").to_string() } else { jobs.iter().map(|j| tl!(j)).collect::<Vec<_>>().join(", ") };
    row(ui, tl!("In the background:"), &background);
    let ai = &app.ui.ai;
    let ai_text = if !ai.enabled {
        tl!("off").to_string()
    } else if host.safe_mode {
        tl!("off in safe mode").to_string()
    } else {
        format!("{} · {}", tl!(ai.provider.name()), ai.model().unwrap_or_default())
    };
    row(ui, tl!("AI:"), &ai_text);
}

fn self_test_ui(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Self-test");
    if ui.button(tl!("Run self-test")).clicked() {
        act(app, "help.selfTest", json!({}));
    }
    let Some(checks) = app.diagnostics.checks.clone() else {
        note(ui, tl!("Checks the settings folder, disk space, memory, graphics, fonts, the log and AI."));
        return;
    };
    for c in &checks {
        let (mark, color) = match c.status {
            Status::Ok => ("✔", Color32::from_rgb(0x2E, 0x8B, 0x57)),
            Status::Warn => ("⚠", ui.visuals().warn_fg_color),
            Status::Fail => ("✖", ui.visuals().error_fg_color),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(mark).color(color));
            ui.label(RichText::new(tl!(c.title)).strong());
            let args: Vec<(&str, &str)> = c.args.iter().map(|(n, v)| (*n, v.as_str())).collect();
            ui.label(fill(tl!(c.template), &args));
        });
        if let Some(fix) = c.fix {
            ui.label(RichText::new(tl!(fix)).small().italics());
        }
    }
}

fn troubleshooting_ui(app: &mut WordApp, ui: &mut Ui) {
    section(ui, "Troubleshooting");
    ui.horizontal_wrapped(|ui| {
        if ui.button(tl!("Copy system information")).on_hover_text(tl!("For a bug report: without your name or personal folders.")).clicked() {
            act(app, "help.systemInfo", json!({"copy": true}));
            app.diagnostics.outcome = Some((true, tl!("The system information is on the clipboard.").into()));
        }
        let has_log = app.diagnostics.host.log_file.is_some();
        if ui.add_enabled(has_log, egui::Button::new(tl!("Open log"))).clicked() {
            act(app, "help.openLog", json!({}));
        }
        if ui.add_enabled(has_log, egui::Button::new(tl!("Open log folder"))).clicked() {
            act(app, "help.openLog", json!({"folder": true}));
        }
    });
    let Some(dir) = app.diagnostics.host.settings_dir.clone() else {
        note(ui, tl!("This run keeps no settings: safe mode and resetting aren't available."));
        return;
    };
    ui.add_space(4.0);
    let mut safe = startup::is_set(&dir, startup::SAFE_MODE);
    if ui.checkbox(&mut safe, tl!("Start in safe mode next time")).changed() {
        act(app, "help.safeMode", json!({"value": safe}));
    }
    note(ui, tl!("For one start: drawn by the processor instead of the graphics chip, and without AI. Helps when Docufy shows garbled graphics or closes at start."));
    ui.add_space(4.0);
    let reset = startup::is_set(&dir, startup::RESET_SETTINGS);
    if reset {
        ui.label(tl!("The settings will be reset at the next start."));
        if ui.button(tl!("Keep the settings")).clicked() {
            act(app, "help.resetSettings", json!({"value": false}));
        }
    } else if app.diagnostics.confirm_reset {
        ui.label(tl!("Reset all settings at the next start? Documents, AutoRecover copies, your dictionary and API keys stay. The old settings are kept as a backup (.bak)."));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl!("Reset at next start")).clicked() {
                app.diagnostics.confirm_reset = false;
                act(app, "help.resetSettings", json!({"value": true}));
            }
            if ui.button(tl!("Cancel")).clicked() {
                app.diagnostics.confirm_reset = false;
            }
        });
    } else if ui.button(tl!("Reset settings…")).clicked() {
        app.diagnostics.confirm_reset = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> WordApp {
        WordApp::new(docufy_engine::Session::new(docufy_doc::Document::from_text("Hallo")), crate::Services::default())
    }

    #[test]
    fn the_pane_opens_and_reports_resources() {
        let mut a = app();
        assert_eq!(a.run("help.diagnostics", json!({})).unwrap()["open"], true);
        let v = a.run("help.resources", json!({})).unwrap();
        assert!(v["system"]["ramTotalBytes"].as_u64().unwrap() > 0, "{v}");
        assert!(v["app"]["ramBytes"].as_u64().unwrap() > 0, "{v}");
        assert_eq!(v["documents"], 1);
        assert_eq!(a.run("help.diagnostics", json!({"value": false})).unwrap()["open"], false);
        assert!(a.run("help.diagnostics", json!({"value": "yes"})).is_err());
    }

    #[test]
    fn the_self_test_finds_problems_and_says_what_to_do() {
        let mut a = app();
        a.set_host_diagnostics(HostDiagnostics {
            graphics: Some(selftest::Graphics { name: "Microsoft Basic Render Driver".into(), backend: "Dx12".into(), software: true }),
            ..Default::default()
        });
        a.run("ai.settings", json!({"enabled": true, "provider": "local"})).unwrap();
        let v = a.run("help.selfTest", json!({})).unwrap();
        let checks = v["checks"].as_array().unwrap();
        let find = |id: &str| checks.iter().find(|c| c["id"] == id).unwrap().clone();
        assert_eq!(find("graphics")["status"], "warning");
        assert!(find("graphics")["fix"].as_str().unwrap().contains("driver"));
        assert_eq!(find("settings")["status"], "warning", "a run without settings says so");
        assert_eq!(find("ai")["status"], "warning");
        assert!(a.diagnostics.checks.is_some());
    }

    #[test]
    fn system_information_leaves_out_the_user() {
        let mut a = app();
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
        a.set_host_diagnostics(HostDiagnostics {
            log_file: Some(PathBuf::from(&home).join("AppData").join("docufy.log")),
            build: "abc1234".into(),
            ..Default::default()
        });
        let text = a.run("help.systemInfo", json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(text.contains("Docufy: ") && text.contains("abc1234"), "{text}");
        assert!(text.contains("Log file: ~"), "{text}");
        assert!(!text.contains(&home), "{text}");
        assert!(text.contains("AI: off"), "{text}");
    }

    #[test]
    fn next_start_choices_need_a_settings_folder() {
        let mut a = app();
        assert!(a.run("help.safeMode", json!({"value": true})).is_err());
        let dir = std::env::temp_dir().join(format!("docufy-diag-ui-{}", std::process::id()));
        a.set_host_diagnostics(HostDiagnostics { settings_dir: Some(dir.clone()), ..Default::default() });
        assert_eq!(a.run("help.safeMode", json!({"value": true})).unwrap()["nextStart"], true);
        assert!(startup::is_set(&dir, startup::SAFE_MODE));
        assert_eq!(a.run("help.safeMode", json!({})).unwrap()["nextStart"], true);
        a.run("help.safeMode", json!({"value": false})).unwrap();
        a.run("help.resetSettings", json!({"value": true})).unwrap();
        assert!(startup::is_set(&dir, startup::RESET_SETTINGS));
        a.run("help.resetSettings", json!({"value": false})).unwrap();
        assert!(!startup::is_set(&dir, startup::RESET_SETTINGS));
        assert!(a.run("help.openLog", json!({})).is_err(), "no log file in this run");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_mode_keeps_ai_off() {
        let mut a = app();
        a.run("ai.settings", json!({"enabled": true})).unwrap();
        a.set_host_diagnostics(HostDiagnostics { safe_mode: true, ..Default::default() });
        assert!(a.safe_mode());
        let e = a.run("ai.summarize", json!({})).unwrap_err();
        assert!(e.contains("safe mode"), "{e}");
    }
}
