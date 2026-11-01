//! File › Options › AI with "On this computer": the runtime and the models on offer, downloading
//! and deleting them, and which one answers. Also the commands `ai.local` (what is installed),
//! `ai.download`, `ai.cancelDownload`, `ai.deleteModel` and `ai.deleteRuntime`.
//!
//! Downloads run on a worker thread, one at a time; the pane shows their progress. Nothing is
//! downloaded unless the user (or a command) asks for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use docufy_ai::local::catalog::{self, Build, CatalogModel};
use docufy_ai::local::{self, install, runtime};
use docufy_ai::{Provider, hardware::format_bytes};
use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::ai_ui::{fill, note};

/// What is being downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Runtime,
    Model(&'static CatalogModel, Build),
}

/// A download running on a worker thread.
pub struct Download {
    pub what: What,
    progress: Arc<install::Progress>,
    cancel: Arc<AtomicBool>,
    rx: std::sync::mpsc::Receiver<Result<(), String>>,
}

impl Download {
    fn bytes(&self) -> (u64, u64) {
        (self.progress.done.load(Ordering::Relaxed), self.progress.total.load(Ordering::Relaxed))
    }
}

fn what_json(w: What) -> Value {
    match w {
        What::Runtime => json!({"target": "runtime"}),
        What::Model(m, b) => json!({"target": m.id, "build": b.id()}),
    }
}

/// The local-model commands; `None` for other ids.
pub fn command(app: &mut WordApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    Some(match id {
        "ai.local" => Ok(status(app)),
        "ai.download" => match text("target") {
            Some("runtime") => start(app, What::Runtime),
            Some(id) => match catalog::model(id) {
                Some(m) => {
                    let build = match text("build") {
                        Some(b) => Build::from_id(b).filter(|b| m.variant(*b).is_some()).ok_or_else(|| format!("{} has no `{b}` build", m.name)),
                        None => Ok(planned_build(app, m)),
                    };
                    build.and_then(|b| start(app, What::Model(m, b)))
                }
                None => Err(format!("unknown model `{id}`; one of {}", model_ids())),
            },
            None => Err("`target` must be `runtime` or a model id".into()),
        },
        "ai.cancelDownload" => {
            let running = app.ai.download.as_ref().map(|d| d.cancel.store(true, Ordering::Relaxed)).is_some();
            Ok(json!({"cancelled": running}))
        }
        "ai.deleteModel" => match text("model").and_then(catalog::model) {
            Some(m) => {
                let builds: Vec<Build> = match text("build").map(Build::from_id) {
                    Some(Some(b)) => vec![b],
                    Some(None) => return Some(Err("`build` must be gpu or cpu".into())),
                    None => m.variants.iter().map(|v| v.build).collect(),
                };
                if app.ai.download.as_ref().is_some_and(|d| matches!(d.what, What::Model(dm, b) if dm.id == m.id && builds.contains(&b))) {
                    return Some(Err("the model is being downloaded; cancel that first".into()));
                }
                local::engine::unload();
                builds
                    .iter()
                    .filter_map(|b| local::model_dir(m, *b))
                    .try_for_each(|d| install::remove(&d))
                    .map(|()| json!({"deleted": m.id}))
            }
            None => Err(format!("`model` must be one of {}", model_ids())),
        },
        "ai.deleteRuntime" => {
            if app.ai.download.as_ref().is_some_and(|d| d.what == What::Runtime) {
                return Some(Err("the runtime is being downloaded; cancel that first".into()));
            }
            local::engine::unload();
            match local::runtime_dir() {
                Some(d) => install::remove(&d).map(|()| json!({"deleted": "runtime"})),
                None => Ok(json!({"deleted": null})),
            }
        }
        _ => return None,
    })
}

fn model_ids() -> String {
    catalog::MODELS.iter().map(|m| m.id).collect::<Vec<_>>().join(", ")
}

/// The build that suits this computer for `m` (the graphics build without hardware information).
fn planned_build(app: &WordApp, m: &CatalogModel) -> Build {
    local::plan(&app.ui.ai, m, app.ai.hardware.as_ref()).1
}

fn status(app: &WordApp) -> Value {
    let models: Vec<Value> = catalog::MODELS
        .iter()
        .map(|m| {
            let builds: Vec<Value> = m
                .variants
                .iter()
                .map(|v| json!({"build": v.build.id(), "bytes": v.bytes(), "installed": local::model_ready(m, v.build)}))
                .collect();
            json!({
                "id": m.id, "name": m.name, "maker": m.maker, "licence": m.licence, "licenceUrl": m.licence_url,
                "builds": builds, "forThisComputer": planned_build(app, m).id(),
            })
        })
        .collect();
    let download = app.ai.download.as_ref().map(|d| {
        let (done, total) = d.bytes();
        let mut v = what_json(d.what);
        v["done"] = json!(done);
        v["total"] = json!(total);
        v
    });
    json!({
        "supported": runtime::supported(),
        "runtimeInstalled": local::runtime_ready(),
        "runtimeBytes": runtime::download_bytes(),
        "folder": local::data_dir().map(|d| d.display().to_string()),
        "chosen": app.ui.ai.model_for(Provider::Local),
        "models": models,
        "download": download,
    })
}

fn start(app: &mut WordApp, what: What) -> Result<Value, String> {
    if !runtime::supported() {
        return Err(tl!("Models on this computer need Windows (x64) for now.").into());
    }
    if let Some(d) = &app.ai.download {
        return Err(format!("already downloading {}", what_json(d.what)));
    }
    let dir = match what {
        What::Runtime => local::runtime_dir(),
        What::Model(m, b) => local::model_dir(m, b),
    }
    .ok_or("no folder for local models")?;
    let progress = Arc::new(install::Progress::default());
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let (p, c, ctx) = (progress.clone(), cancel.clone(), app.ctx.clone());
    std::thread::Builder::new()
        .name("docufy-ai-download".into())
        .spawn(move || {
            let result = match what {
                What::Runtime => install::install_runtime(&dir, &c, &p),
                What::Model(m, b) => install::install_model(m, b, &dir, &c, &p),
            };
            let _ = tx.send(result);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        })
        .map_err(|e| e.to_string())?;
    app.ai.download = Some(Download { what, progress, cancel, rx });
    app.ai.outcome = None;
    Ok(json!({"started": what_json(what)}))
}

/// Takes in a finished download and keeps the progress moving while one runs.
pub fn poll(app: &mut WordApp) {
    let Some(d) = &app.ai.download else { return };
    let result = match d.rx.try_recv() {
        Ok(r) => r,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            if let Some(ctx) = &app.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            return;
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the download stopped unexpectedly".into()),
    };
    let what = d.what;
    app.ai.download = None;
    app.ai.outcome = Some(match (&result, what) {
        (Ok(()), What::Runtime) => (true, tl!("The runtime for local models is installed.").to_string()),
        (Ok(()), What::Model(m, _)) => (true, fill(tl!("{model} is downloaded and checked."), &[("model", m.name)])),
        (Err(e), _) if e == "cancelled" => (false, tl!("The download was stopped. Downloading again continues where it stopped.").to_string()),
        (Err(e), _) => (false, fill(tl!("The download failed: {error}"), &[("error", e)])),
    });
    // A model just downloaded becomes the one that answers when none was chosen yet.
    if let (Ok(()), What::Model(m, _)) = (&result, what)
        && !app.ui.ai.models.contains_key(Provider::Local.id())
        && let Err(e) = app.run("ai.settings", json!({"model": m.id}))
    {
        log::warn!("ai: {e}");
    }
}

/// The "On this computer" part of File › Options › AI.
pub fn section(app: &mut WordApp, ui: &mut Ui) {
    section_title(ui);
    if !runtime::supported() {
        note(ui, tl!("Models on this computer need Windows (x64) for now."));
        return;
    }
    note(ui, tl!("Your text stays on this computer. The runtime and the models are downloaded once, from Microsoft and from Hugging Face, and checked before they are used."));
    runtime_row(app, ui);
    ui.add_space(6.0);
    let chosen = app.ui.ai.model_for(Provider::Local).unwrap_or_default();
    for m in &catalog::MODELS {
        model_row(app, ui, m, m.id == chosen);
    }
    if let Some(dir) = local::data_dir() {
        ui.horizontal_wrapped(|ui| {
            note(ui, &fill(tl!("Stored in {folder}"), &[("folder", &dir.display().to_string())]));
            if dir.is_dir() && ui.small_button(tl!("Open folder")).clicked() {
                let _ = open::that_detached(&dir);
            }
        });
    }
}

fn section_title(ui: &mut Ui) {
    crate::ai_ui::section(ui, "Models on this computer");
}

/// The progress of `what` when it is being downloaded: a bar and Stop.
fn progress(app: &mut WordApp, ui: &mut Ui, what: What) -> bool {
    let Some(d) = app.ai.download.as_ref().filter(|d| d.what == what) else { return false };
    let (done, total) = d.bytes();
    let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
    let text = fill(tl!("{done} of {total}"), &[("done", &format_bytes(done)), ("total", &format_bytes(total))]);
    ui.horizontal(|ui| {
        ui.add(egui::ProgressBar::new(fraction).text(text).desired_width(260.0));
        if ui.button(tl!("Stop")).clicked() {
            let _ = app.run("ai.cancelDownload", json!({}));
        }
    });
    true
}

fn runtime_row(app: &mut WordApp, ui: &mut Ui) {
    let busy = app.ai.download.is_some();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(tl!("Runtime:")).strong());
        ui.label(fill(tl!("{runtime}"), &[("runtime", "ONNX Runtime GenAI (DirectML)")]));
    });
    if progress(app, ui, What::Runtime) {
        return;
    }
    if local::runtime_ready() {
        ui.horizontal_wrapped(|ui| {
            ui.label(tl!("Installed ✔"));
            if ui.small_button(tl!("Remove")).clicked()
                && let Err(e) = app.run("ai.deleteRuntime", json!({}))
            {
                app.ai.outcome = Some((false, e));
            }
        });
    } else {
        let label = fill(tl!("Download runtime ({size})"), &[("size", &format_bytes(runtime::download_bytes()))]);
        if ui.add_enabled(!busy, egui::Button::new(label)).clicked()
            && let Err(e) = app.run("ai.download", json!({"target": "runtime"}))
        {
            app.ai.outcome = Some((false, e));
        }
        note(ui, tl!("About 42 MB stay on the disk; the rest of the download is unpacked and removed."));
    }
}

fn model_row(app: &mut WordApp, ui: &mut Ui, m: &'static CatalogModel, chosen: bool) {
    let build = planned_build(app, m);
    let bytes = m.variant(build).map(|v| v.bytes()).unwrap_or_default();
    let installed: Vec<Build> = m.variants.iter().map(|v| v.build).filter(|b| local::model_ready(m, *b)).collect();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            if ui.radio(chosen, RichText::new(fill(tl!("{model}"), &[("model", m.name)])).strong()).clicked()
                && !chosen
                && let Err(e) = app.run("ai.settings", json!({"model": m.id}))
            {
                app.ai.outcome = Some((false, e));
            }
            note(ui, &fill(tl!("{maker} · {count} parameters"), &[("maker", m.maker), ("count", m.parameters)]));
        });
        note(ui, tl!(m.about));
        ui.horizontal_wrapped(|ui| {
            let kind = if build == Build::Gpu { tl!("Graphics build") } else { tl!("Processor build") };
            ui.label(fill(tl!("{kind}, {size}"), &[("kind", kind), ("size", &format_bytes(bytes))]));
            ui.hyperlink_to(fill(tl!("Licence: {licence}"), &[("licence", m.licence)]), m.licence_url);
        });
        if progress(app, ui, What::Model(m, build)) {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if installed.contains(&build) {
                ui.label(tl!("Downloaded ✔"));
            } else {
                let busy = app.ai.download.is_some();
                if ui
                    .add_enabled(!busy, egui::Button::new(tl!("Accept licence and download")))
                    .on_hover_text(tl!("By downloading you accept the model's licence (link above)."))
                    .clicked()
                    && let Err(e) = app.run("ai.download", json!({"target": m.id, "build": build.id()}))
                {
                    app.ai.outcome = Some((false, e));
                }
                if let Some(other) = installed.first() {
                    let text = if *other == Build::Gpu { tl!("The graphics build is downloaded.") } else { tl!("The processor build is downloaded.") };
                    note(ui, text);
                }
            }
            if !installed.is_empty()
                && ui.small_button(tl!("Delete")).clicked()
                && let Err(e) = app.run("ai.deleteModel", json!({"model": m.id}))
            {
                app.ai.outcome = Some((false, e));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> WordApp {
        WordApp::new(docufy_engine::Session::new(docufy_doc::Document::new()), crate::Services::default())
    }

    #[test]
    fn the_status_lists_every_model_and_nothing_downloads_by_itself() {
        let mut a = app();
        let v = a.run("ai.local", json!({})).unwrap();
        assert_eq!(v["models"].as_array().unwrap().len(), catalog::MODELS.len());
        assert_eq!(v["download"], Value::Null);
        assert!(a.ai.download.is_none());
        assert!(v["runtimeBytes"].as_u64().unwrap() > 200_000_000);
    }

    #[test]
    fn bad_downloads_are_refused() {
        let mut a = app();
        assert!(a.run("ai.download", json!({})).unwrap_err().contains("target"));
        assert!(a.run("ai.download", json!({"target": "gpt-5"})).unwrap_err().contains("unknown model"));
        assert!(a.run("ai.download", json!({"target": "qwen2.5-1.5b", "build": "gpu"})).unwrap_err().contains("no `gpu` build"));
        assert!(a.run("ai.deleteModel", json!({"model": "nope"})).is_err());
        assert_eq!(a.run("ai.cancelDownload", json!({})).unwrap()["cancelled"], false);
    }

    #[test]
    fn a_local_model_must_come_from_the_catalog() {
        let mut a = app();
        a.run("ai.settings", json!({"provider": "local"})).unwrap();
        assert!(a.run("ai.settings", json!({"model": "llama-70b"})).is_err());
        a.run("ai.settings", json!({"model": "mistral-7b"})).unwrap();
        assert_eq!(a.ui.ai.model_for(Provider::Local).as_deref(), Some("mistral-7b"));
    }
}
