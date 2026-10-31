//! Reads what this computer offers a local model ([`Hardware`]): memory, processor, graphics chips
//! with their memory, NPUs, battery and free disk space. Takes a moment; run it off the interface
//! thread.

use std::path::Path;

use sysinfo::{CpuRefreshKind, Disks, Gpus, RefreshKind, System};

use crate::hardware::{DeviceKind, Gpu, Hardware, Npu};

/// What this computer has now. `known_gpus`: graphics chips the caller already classified (the
/// app's graphics driver reports integrated or discrete), by name. `models_dir`: where models are
/// stored, for the free disk space.
pub fn probe(known_gpus: &[(String, DeviceKind)], models_dir: &Path) -> Hardware {
    let mut hw = Hardware::default();
    if let Ok(mut sys) = System::new_with_specifics(RefreshKind::nothing()) {
        sys.refresh_memory();
        sys.refresh_cpu_list(CpuRefreshKind::nothing());
        hw.ram_total = sys.total_memory();
        hw.ram_available = sys.available_memory();
        hw.cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        hw.cpu_cores = System::physical_core_count().unwrap_or_else(|_| sys.cpus().len());
    }
    hw.gpus = gpus(known_gpus);
    hw.npus = npus();
    (hw.on_battery, hw.battery_percent) = battery();
    hw.disk_free = disk_free(models_dir);
    hw
}

fn gpus(known: &[(String, DeviceKind)]) -> Vec<Gpu> {
    let Ok(mut list) = Gpus::new() else { return Vec::new() };
    list.refresh(true);
    list.iter()
        .filter_map(|g| {
            let name = g.model().map(str::trim).filter(|n| !n.is_empty())?.to_string();
            // Microsoft's software renderer is no graphics chip.
            if name.contains("Basic Render") || name.contains("Basic Display") {
                return None;
            }
            let kind = known.iter().find(|(k, _)| same_chip(k, &name)).map(|(_, kind)| *kind).unwrap_or_else(|| guess_kind(&name, g.total_memory()));
            let (vram_total, vram_used) = if kind == DeviceKind::DiscreteGpu { (g.total_memory(), g.used_memory()) } else { (None, None) };
            Some(Gpu { name, kind, vram_total, vram_used })
        })
        .collect()
}

/// Whether two names of a graphics chip (driver and system report them slightly differently)
/// name the same chip.
fn same_chip(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().replace("(r)", "").replace("(tm)", "").split_whitespace().collect::<Vec<_>>().join(" ");
    let (a, b) = (norm(a), norm(b));
    a == b || a.contains(&b) || b.contains(&a)
}

/// Integrated or discrete, from the name and its own memory, when the driver didn't say.
pub(crate) fn guess_kind(name: &str, vram: Option<u64>) -> DeviceKind {
    let n = name.to_lowercase();
    // Intel's graphics cards are "Arc A770", "Arc B580"; the chips in Core Ultra processors are
    // "Arc Graphics" or "Arc 140V".
    let intel_card = n.contains("arc")
        && n.split_whitespace().any(|w| w.len() >= 4 && (w.starts_with('a') || w.starts_with('b')) && w[1..].chars().all(|c| c.is_ascii_digit()));
    let amd_apu = n.contains("radeon")
        && (n.contains("radeon(tm) graphics")
            || n.contains("radeon graphics")
            || n.split_whitespace().any(|w| w.ends_with('m') && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit()) && w.len() == 4)
            || n.contains("8060s")
            || n.contains("8050s"));
    let integrated = n.starts_with("apple")
        || (n.contains("intel") && !intel_card)
        || amd_apu
        || n.contains("adreno")
        || n.contains("mali")
        || vram.is_some_and(|v| v < 1_000_000_000);
    if integrated { DeviceKind::IntegratedGpu } else { DeviceKind::DiscreteGpu }
}

#[cfg(windows)]
fn npus() -> Vec<Npu> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    // Device class "ComputeAccelerator": the NPUs of Intel (AI Boost), AMD (NPU) and Qualcomm (Hexagon).
    const CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{F01A9D53-3FF6-48D2-9F97-C8A7004BE10C}";
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(CLASS) else { return Vec::new() };
    let mut out: Vec<Npu> = class
        .enum_keys()
        .flatten()
        .filter(|k| k.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|k| class.open_subkey(&k).ok()?.get_value::<String, _>("DriverDesc").ok())
        .map(|name| Npu { name: name.trim().to_string() })
        .collect();
    out.dedup_by(|a, b| a.name == b.name);
    out
}

#[cfg(target_os = "macos")]
fn npus() -> Vec<Npu> {
    if cfg!(target_arch = "aarch64") { vec![Npu { name: "Apple Neural Engine".into() }] } else { Vec::new() }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn npus() -> Vec<Npu> {
    // Linux lists NPUs (intel_vpu, amdxdna, qaic) under the compute accelerator subsystem.
    let Ok(dir) = std::fs::read_dir("/sys/class/accel") else { return Vec::new() };
    dir.flatten()
        .map(|e| {
            let driver =
                std::fs::read_link(e.path().join("device/driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
            Npu { name: driver.unwrap_or_else(|| e.file_name().to_string_lossy().into_owned()) }
        })
        .collect()
}

#[cfg(not(any(windows, unix)))]
fn npus() -> Vec<Npu> {
    Vec::new()
}

fn battery() -> (Option<bool>, Option<f32>) {
    let Ok(manager) = starship_battery::Manager::new() else { return (None, None) };
    let Ok(mut batteries) = manager.batteries() else { return (None, None) };
    match batteries.next() {
        Some(Ok(b)) => (Some(b.state() == starship_battery::State::Discharging), Some(b.state_of_charge().value * 100.0)),
        _ => (Some(false), None),
    }
}

fn disk_free(dir: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list().ok()?;
    let dir = dir.ancestors().find(|p| p.exists()).unwrap_or(dir);
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let dir = dir.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
    disks
        .iter()
        .filter(|d| dir.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_and_discrete_chips_are_told_apart_by_name() {
        for igpu in [
            "Intel(R) UHD Graphics 770",
            "Intel(R) Arc(TM) Graphics",
            "Intel(R) Arc(TM) 140V GPU (16GB)",
            "AMD Radeon(TM) Graphics",
            "AMD Radeon 780M Graphics",
            "Apple M3 Pro",
            "Qualcomm(R) Adreno(TM) X1-85 GPU",
        ] {
            assert_eq!(guess_kind(igpu, None), DeviceKind::IntegratedGpu, "{igpu}");
        }
        for card in ["NVIDIA GeForce RTX 4070", "AMD Radeon RX 7800 XT", "Intel(R) Arc(TM) A770 Graphics", "Intel(R) Arc(TM) B580 Graphics"] {
            assert_eq!(guess_kind(card, Some(8_000_000_000)), DeviceKind::DiscreteGpu, "{card}");
        }
        assert_eq!(guess_kind("Some GPU", Some(512_000_000)), DeviceKind::IntegratedGpu, "little own memory");
    }

    #[test]
    fn chip_names_match_across_reports() {
        assert!(same_chip("NVIDIA GeForce RTX 4060 Laptop GPU", "NVIDIA GeForce RTX 4060 Laptop GPU"));
        assert!(same_chip("Intel(R) Arc(TM) Graphics", "Intel Arc Graphics"));
        assert!(!same_chip("Intel Arc Graphics", "NVIDIA GeForce RTX 4060"));
    }

    #[test]
    fn this_computer_can_be_probed() {
        let hw = probe(&[], &std::env::temp_dir());
        eprintln!("{hw:#?}");
        assert!(hw.ram_total > 0 && hw.ram_available <= hw.ram_total, "{hw:?}");
        assert!(hw.disk_free.is_some(), "{hw:?}");
    }
}
