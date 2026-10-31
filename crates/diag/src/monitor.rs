//! Sampling Docufy's and the computer's resources ([`Sampler`]), and a thread that does it every
//! so often while someone looks ([`spawn`]).

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use sysinfo::{CpuRefreshKind, Gpus, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

use crate::{GpuLoad, Sample};

/// Reads the resources in use. Processor shares need two readings: the first [`Sampler::sample`]
/// after [`Sampler::new`] reports 0 % for them.
pub struct Sampler {
    sys: Option<System>,
    gpus: Option<Gpus>,
    pid: Option<Pid>,
    cores: usize,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        let sys = System::new_with_specifics(RefreshKind::nothing()).ok();
        let cores = sys.as_ref().map_or(1, |s| s.cpus().len()).max(1);
        let gpus = Gpus::new().ok();
        Sampler { sys, gpus, pid: sysinfo::get_current_pid().ok(), cores }
    }

    pub fn sample(&mut self) -> Sample {
        let mut s = Sample::default();
        if let Some(sys) = self.sys.as_mut() {
            sys.refresh_memory();
            sys.refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
            s.ram_used = sys.used_memory();
            s.ram_total = sys.total_memory();
            s.cpu = sys.global_cpu_usage();
            if let Some(pid) = self.pid {
                let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_gpu_usage().with_gpu_memory();
                sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, kind);
                if let Some(p) = sys.process(pid) {
                    // sysinfo counts each core as 100 %.
                    s.app_cpu = (p.cpu_usage() / self.cores as f32).clamp(0.0, 100.0);
                    s.app_ram = p.memory();
                    s.app_gpu = p.gpu_usage();
                    s.app_vram = p.gpu_memory();
                }
            }
        }
        if let Some(gpus) = self.gpus.as_mut() {
            gpus.refresh(true);
            s.gpus = gpus
                .iter()
                .filter_map(|g| {
                    let name = g.model().map(str::trim).filter(|n| !n.is_empty())?.to_string();
                    // Microsoft's software renderer is no graphics chip.
                    if name.contains("Basic Render") || name.contains("Basic Display") {
                        return None;
                    }
                    Some(GpuLoad { name, vram_used: g.used_memory(), vram_total: g.total_memory() })
                })
                .collect();
        }
        s
    }
}

/// The computer, for the system information.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Machine {
    /// Operating system and version, e.g. "Windows 11 Pro 25H2 (26300)".
    pub os: String,
    pub cpu: String,
    /// Logical cores.
    pub cores: usize,
}

pub fn machine() -> Machine {
    let os = System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.to_string());
    let kernel = System::kernel_version().map(|k| format!(" ({k})")).unwrap_or_default();
    let mut m = Machine { os: format!("{os}{kernel}, {}", System::cpu_arch()), ..Default::default() };
    if let Ok(mut sys) = System::new_with_specifics(RefreshKind::nothing()) {
        sys.refresh_cpu_list(CpuRefreshKind::nothing());
        m.cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        m.cores = sys.cpus().len();
    }
    m
}

/// Samples every `every` on a thread of its own and sends each [`Sample`], calling `wake` after
/// it (to repaint). The thread ends when the receiver is dropped. `None` when no thread starts.
pub fn spawn(every: Duration, wake: impl Fn() + Send + 'static) -> Option<Receiver<Sample>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("docufy-monitor".into())
        .spawn(move || {
            let mut sampler = Sampler::new();
            // The first processor reading only sets the baseline.
            sampler.sample();
            loop {
                std::thread::sleep(every);
                if tx.send(sampler.sample()).is_err() {
                    return;
                }
                wake();
            }
        })
        .ok()?;
    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_sees_this_process_and_the_computer() {
        let mut sampler = Sampler::new();
        sampler.sample();
        std::thread::sleep(Duration::from_millis(200));
        let s = sampler.sample();
        assert!(s.ram_total > 0 && s.ram_used > 0 && s.ram_used <= s.ram_total, "{s:?}");
        assert!(s.app_ram > 0, "the test process uses memory: {s:?}");
        assert!((0.0..=100.0).contains(&s.app_cpu) && (0.0..=100.0).contains(&s.cpu), "{s:?}");
    }

    #[test]
    fn the_machine_is_named() {
        let m = machine();
        assert!(!m.os.is_empty() && !m.cpu.is_empty() && m.cores > 0, "{m:?}");
    }

    #[test]
    fn the_sampling_thread_stops_when_nobody_listens() {
        let rx = spawn(Duration::from_millis(20), || {}).unwrap();
        let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(first.ram_total > 0);
        drop(rx);
    }
}
