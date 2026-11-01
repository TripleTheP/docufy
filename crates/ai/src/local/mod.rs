//! AI on this computer: the models on offer ([`catalog`]), the runtime they need ([`runtime`]),
//! downloading both ([`install`]), the chat format of each model ([`template`]) and the helper
//! process that runs them ([`engine`]).
//!
//! Models run in `docufy-ai-host`, a process of its own, with ONNX Runtime GenAI: on the graphics
//! chip through DirectML (dedicated or integrated) or on the processor. NPUs come later.

pub mod catalog;
pub mod runtime;
pub mod template;

#[cfg(not(target_arch = "wasm32"))]
pub mod engine;
#[cfg(not(target_arch = "wasm32"))]
pub mod install;

use std::path::PathBuf;

use catalog::{Build, CatalogModel};

use crate::hardware::{DeviceKind, Hardware, choose_device};
use crate::settings::AiSettings;

/// Where runtime and models are kept: `DOCUFY_AI_DIR`, else the local (not roaming) application
/// data folder: `%LOCALAPPDATA%\Docufy\AI`, `~/Library/Application Support/Docufy/AI`,
/// `$XDG_DATA_HOME/docufy/ai`.
pub fn data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = env("DOCUFY_AI_DIR") {
        return Some(d);
    }
    if cfg!(windows) {
        env("LOCALAPPDATA").map(|d| d.join("Docufy").join("AI"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/Docufy/AI"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share"))).map(|d| d.join("docufy/ai"))
    }
}

/// The runtime's folder.
pub fn runtime_dir() -> Option<PathBuf> {
    Some(data_dir()?.join("runtime").join(runtime::VERSION))
}

/// A model build's folder.
pub fn model_dir(model: &CatalogModel, build: Build) -> Option<PathBuf> {
    Some(data_dir()?.join("models").join(model.id).join(build.id()))
}

/// Whether the runtime is installed.
#[cfg(not(target_arch = "wasm32"))]
pub fn runtime_ready() -> bool {
    runtime_dir().is_some_and(|d| install::is_installed(&d))
}

/// Whether `model`'s `build` is installed.
#[cfg(not(target_arch = "wasm32"))]
pub fn model_ready(model: &CatalogModel, build: Build) -> bool {
    model_dir(model, build).is_some_and(|d| install::is_installed(&d))
}

/// The build that suits `device`: the graphics build for graphics chips, else the processor's.
pub fn build_for(model: &CatalogModel, device: DeviceKind) -> Build {
    match device {
        DeviceKind::DiscreteGpu | DeviceKind::IntegratedGpu if model.variant(Build::Gpu).is_some() => Build::Gpu,
        _ => Build::Cpu,
    }
}

/// The model the settings choose for this computer.
pub fn chosen(settings: &AiSettings) -> Option<&'static CatalogModel> {
    catalog::model(&settings.model_for(crate::Provider::Local)?)
}

/// The device and build a model runs with on `hw` under `settings`; without hardware
/// information, the graphics build when there is one.
pub fn plan(settings: &AiSettings, model: &CatalogModel, hw: Option<&Hardware>) -> (Option<DeviceKind>, Build) {
    let device = match hw {
        Some(hw) => choose_device(hw, &model.local_model(), settings.device),
        None => Some(DeviceKind::DiscreteGpu),
    };
    (device, build_for(model, device.unwrap_or(DeviceKind::Cpu)))
}

/// What to run for `settings` on `hw`: the build that suits the chosen device, or the other one
/// when only that is installed. `Err` (in words for the log; the front end checks first) when
/// the runtime or the model isn't installed or local AI must pause on battery.
#[cfg(not(target_arch = "wasm32"))]
pub fn target(settings: &AiSettings, hw: Option<&Hardware>) -> Result<(PathBuf, engine::Target), String> {
    let model = chosen(settings).ok_or("no local model is chosen")?;
    if settings.pause_on_battery && hw.is_some_and(|h| h.on_battery == Some(true)) {
        return Err("the computer runs on battery and local AI is set to pause".into());
    }
    let runtime = runtime_dir().filter(|d| install::is_installed(d)).ok_or("the runtime for local models is not installed")?;
    let (_, wanted) = plan(settings, model, hw);
    let other = if wanted == Build::Gpu { Build::Cpu } else { Build::Gpu };
    let build = [wanted, other].into_iter().find(|b| model_ready(model, *b)).ok_or_else(|| format!("{} is not downloaded", model.name))?;
    let model_dir = model_dir(model, build).ok_or("no folder for models")?;
    Ok((runtime, engine::Target { model_dir, family: model.family, provider: build.provider(), context: model.context }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::Gpu;

    #[test]
    fn every_model_has_a_processor_build_and_checked_files() {
        for m in &catalog::MODELS {
            assert!(m.variant(Build::Cpu).is_some(), "{}", m.id);
            for v in m.variants {
                assert!(v.revision.len() == 40, "{} pinned to a commit", m.id);
                assert!(v.files.iter().any(|f| f.name == "genai_config.json"), "{}", m.id);
                assert!(v.files.iter().any(|f| f.name == "model.onnx.data" && f.size > 500_000_000), "{}", m.id);
                assert!(v.files.iter().all(|f| f.sha256.len() == 64 && f.sha256.chars().all(|c| c.is_ascii_hexdigit())), "{}", m.id);
                assert!(v.files.iter().all(|f| !f.name.ends_with(".py")), "no code is downloaded: {}", m.id);
            }
        }
        assert!(catalog::model(catalog::DEFAULT_MODEL).is_some());
    }

    #[test]
    fn graphics_chips_get_the_graphics_build() {
        let phi = catalog::model("phi-4-mini").unwrap();
        let hw = |vram: u64| Hardware {
            ram_total: 32_000_000_000,
            ram_available: 20_000_000_000,
            gpus: vec![Gpu { name: "RTX".into(), kind: DeviceKind::DiscreteGpu, vram_total: Some(vram), vram_used: Some(0) }],
            ..Default::default()
        };
        let s = AiSettings::default();
        assert_eq!(plan(&s, phi, Some(&hw(16_000_000_000))), (Some(DeviceKind::DiscreteGpu), Build::Gpu));
        // Too little graphics memory: the processor, with its build.
        assert_eq!(plan(&s, phi, Some(&hw(2_000_000_000))), (Some(DeviceKind::Cpu), Build::Cpu));
        let cpu_only = AiSettings { device: crate::DevicePreference::Only(DeviceKind::Cpu), ..Default::default() };
        assert_eq!(plan(&cpu_only, phi, Some(&hw(16_000_000_000))).1, Build::Cpu);
    }

    #[test]
    fn models_live_in_the_local_data_folder() {
        let m = catalog::model("qwen2.5-1.5b").unwrap();
        if let Some(d) = model_dir(m, Build::Gpu) {
            assert!(d.ends_with(std::path::Path::new("models").join("qwen2.5-1.5b").join("gpu")), "{}", d.display());
        }
        assert!(runtime_dir().is_none_or(|d| d.ends_with(runtime::VERSION)));
    }
}
