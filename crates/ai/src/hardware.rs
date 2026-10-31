//! What this computer offers a local model: memory, graphics chips, a neural processing unit
//! (NPU), battery; and which of them runs the model ([`choose_device`]).
//!
//! An integrated GPU (iGPU) has no memory of its own: it borrows the computer's RAM, so a model on
//! it needs free RAM, not VRAM. A dedicated GPU (dGPU) brings its own VRAM. An NPU runs only models
//! built for it, and uses RAM too.

use serde::{Deserialize, Serialize};

/// Where a local model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    /// The neural processing unit (Intel AI Boost, AMD XDNA, Qualcomm Hexagon, Apple Neural Engine).
    Npu,
    /// Graphics built into the processor, sharing the computer's RAM.
    IntegratedGpu,
    /// A graphics card with its own memory.
    DiscreteGpu,
    /// The processor alone: works everywhere, slowest.
    Cpu,
}

impl DeviceKind {
    /// The name shown in settings (English; the interface translates it).
    pub fn name(self) -> &'static str {
        match self {
            DeviceKind::Npu => "NPU (AI processor)",
            DeviceKind::IntegratedGpu => "Integrated graphics (iGPU)",
            DeviceKind::DiscreteGpu => "Graphics card (dGPU)",
            DeviceKind::Cpu => "Processor (CPU)",
        }
    }
}

/// The user's choice of device for local models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DevicePreference {
    /// Docufy picks: graphics card, then NPU, then integrated graphics, then processor.
    #[default]
    Auto,
    /// This device, falling back (with a warning) when it can't run the model.
    Only(DeviceKind),
}

/// One graphics chip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    /// Integrated or discrete.
    pub kind: DeviceKind,
    /// Its own memory in bytes; `None` for integrated graphics or when unknown.
    pub vram_total: Option<u64>,
    /// Of that, in use now (all programs).
    pub vram_used: Option<u64>,
}

impl Gpu {
    /// Free VRAM in bytes, when known.
    pub fn vram_free(&self) -> Option<u64> {
        Some(self.vram_total?.saturating_sub(self.vram_used.unwrap_or(0)))
    }
}

/// One neural processing unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npu {
    pub name: String,
}

/// What [`crate::probe`] found. Every field may be unknown on some systems.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Hardware {
    pub cpu: String,
    pub cpu_cores: usize,
    pub ram_total: u64,
    pub ram_available: u64,
    pub gpus: Vec<Gpu>,
    pub npus: Vec<Npu>,
    /// `Some(true)` while running on battery.
    pub on_battery: Option<bool>,
    /// Charge in percent, when there is a battery.
    pub battery_percent: Option<f32>,
    /// Free space where models are stored, in bytes.
    pub disk_free: Option<u64>,
}

impl Hardware {
    /// The graphics card with the most VRAM.
    pub fn best_dgpu(&self) -> Option<&Gpu> {
        self.gpus.iter().filter(|g| g.kind == DeviceKind::DiscreteGpu).max_by_key(|g| g.vram_total.unwrap_or(0))
    }

    /// The first integrated graphics chip.
    pub fn igpu(&self) -> Option<&Gpu> {
        self.gpus.iter().find(|g| g.kind == DeviceKind::IntegratedGpu)
    }

    /// Whether the computer has the device at all.
    pub fn has(&self, kind: DeviceKind) -> bool {
        match kind {
            DeviceKind::Npu => !self.npus.is_empty(),
            DeviceKind::IntegratedGpu => self.igpu().is_some(),
            DeviceKind::DiscreteGpu => self.best_dgpu().is_some(),
            DeviceKind::Cpu => true,
        }
    }
}

/// A model that runs on this computer, as far as choosing a device and warning about memory goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    /// Download size in bytes.
    pub download_bytes: u64,
    /// Memory the model needs while it answers (weights plus working memory), in bytes.
    pub memory_bytes: u64,
    /// Whether a build of the model for NPUs exists.
    pub npu_ready: bool,
}

/// Whether `model` fits on `device` of `hw` right now.
pub fn fits(hw: &Hardware, model: &LocalModel, device: DeviceKind) -> bool {
    match device {
        DeviceKind::DiscreteGpu => hw.best_dgpu().and_then(Gpu::vram_free).is_some_and(|free| free >= model.memory_bytes),
        DeviceKind::Npu => model.npu_ready && hw.has(DeviceKind::Npu) && hw.ram_available >= model.memory_bytes,
        DeviceKind::IntegratedGpu => hw.has(DeviceKind::IntegratedGpu) && hw.ram_available >= model.memory_bytes,
        DeviceKind::Cpu => hw.ram_available >= model.memory_bytes,
    }
}

/// The device a local model runs on: the user's choice when it can run the model, otherwise the
/// best that can (graphics card, NPU, integrated graphics, processor). `None` when not even the
/// processor has enough free memory.
pub fn choose_device(hw: &Hardware, model: &LocalModel, pref: DevicePreference) -> Option<DeviceKind> {
    if let DevicePreference::Only(kind) = pref
        && fits(hw, model, kind)
    {
        return Some(kind);
    }
    [DeviceKind::DiscreteGpu, DeviceKind::Npu, DeviceKind::IntegratedGpu, DeviceKind::Cpu].into_iter().find(|&d| fits(hw, model, d))
}

/// Bytes as "3.2 GB" (decimal units, as Windows Explorer and model pages show sizes).
pub fn format_bytes(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let b = bytes as f64;
    if b >= GB { format!("{:.1} GB", b / GB) } else { format!("{:.0} MB", b / MB) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const GB: u64 = 1_000_000_000;

    pub fn laptop() -> Hardware {
        Hardware {
            cpu: "Intel Core Ultra 7".into(),
            cpu_cores: 16,
            ram_total: 32 * GB,
            ram_available: 18 * GB,
            gpus: vec![
                Gpu { name: "Intel Arc Graphics".into(), kind: DeviceKind::IntegratedGpu, vram_total: None, vram_used: None },
                Gpu { name: "NVIDIA GeForce RTX 4060".into(), kind: DeviceKind::DiscreteGpu, vram_total: Some(8 * GB), vram_used: Some(GB) },
            ],
            npus: vec![Npu { name: "Intel AI Boost".into() }],
            on_battery: Some(false),
            battery_percent: Some(80.0),
            disk_free: Some(200 * GB),
        }
    }

    pub fn model(memory: u64, npu_ready: bool) -> LocalModel {
        LocalModel { id: "m".into(), name: "Model".into(), download_bytes: memory, memory_bytes: memory, npu_ready }
    }

    #[test]
    fn auto_prefers_the_graphics_card_then_npu_then_igpu_then_cpu() {
        let hw = laptop();
        assert_eq!(choose_device(&hw, &model(4 * GB, true), DevicePreference::Auto), Some(DeviceKind::DiscreteGpu));
        // 7 GB free VRAM is too little for 10 GB: the NPU takes it (18 GB RAM free).
        assert_eq!(choose_device(&hw, &model(10 * GB, true), DevicePreference::Auto), Some(DeviceKind::Npu));
        // No NPU build: integrated graphics, which uses RAM.
        assert_eq!(choose_device(&hw, &model(10 * GB, false), DevicePreference::Auto), Some(DeviceKind::IntegratedGpu));
        let mut no_gpu = hw.clone();
        no_gpu.gpus.clear();
        no_gpu.npus.clear();
        assert_eq!(choose_device(&no_gpu, &model(10 * GB, false), DevicePreference::Auto), Some(DeviceKind::Cpu));
        assert_eq!(choose_device(&no_gpu, &model(20 * GB, false), DevicePreference::Auto), None);
    }

    #[test]
    fn a_chosen_device_is_kept_when_it_fits_and_left_when_not() {
        let hw = laptop();
        let pref = DevicePreference::Only(DeviceKind::Cpu);
        assert_eq!(choose_device(&hw, &model(4 * GB, true), pref), Some(DeviceKind::Cpu));
        let pref = DevicePreference::Only(DeviceKind::Npu);
        assert_eq!(choose_device(&hw, &model(4 * GB, false), pref), Some(DeviceKind::DiscreteGpu), "no NPU build: falls back");
    }

    #[test]
    fn integrated_graphics_count_free_ram_not_vram() {
        let hw = laptop();
        assert!(fits(&hw, &model(17 * GB, false), DeviceKind::IntegratedGpu));
        assert!(!fits(&hw, &model(19 * GB, false), DeviceKind::IntegratedGpu));
    }

    #[test]
    fn sizes_read_like_explorer() {
        assert_eq!(format_bytes(3_200_000_000), "3.2 GB");
        assert_eq!(format_bytes(450_000_000), "450 MB");
    }
}
