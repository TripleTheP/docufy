//! Diagnostics for Docufy: what technical users and support need when something goes wrong.
//!
//! - [`monitor`]: Docufy's and the computer's use of processor, memory and graphics memory,
//!   sampled on a thread of its own (desktop only).
//! - [`history`]: the last minutes of samples and frame times, for the live view's curves.
//! - [`selftest`]: checks with a status and what to do about a problem.
//! - [`info`]: the system information a bug report needs, with personal paths removed.
//! - [`startup`]: safe mode and default settings for the next start.
//!
//! Nothing here sends anything anywhere: the front end shows it, and copies the system information
//! to the clipboard only when the user asks.

pub mod history;
pub mod info;
pub mod selftest;
pub mod startup;

#[cfg(not(target_arch = "wasm32"))]
pub mod monitor;

/// One measurement of the resources in use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    /// Docufy's share of the whole processor, 0–100 (all cores together).
    pub app_cpu: f32,
    /// Docufy's memory (working set), in bytes.
    pub app_ram: u64,
    /// Docufy's share of the graphics chip, 0–100, where the system reports it (Windows).
    pub app_gpu: Option<f32>,
    /// Graphics memory Docufy holds, in bytes, where the system reports it (Windows).
    pub app_vram: Option<u64>,
    /// The whole computer's processor use, 0–100.
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub gpus: Vec<GpuLoad>,
}

/// One graphics chip's memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GpuLoad {
    pub name: String,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
}

/// `bytes` for people: "512 MB", "3.4 GB".
pub fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;
    let b = bytes as f64;
    if b >= GB { format!("{:.1} GB", b / GB) } else { format!("{:.0} MB", b / MB) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bytes_read_as_megabytes_or_gigabytes() {
        assert_eq!(super::format_bytes(312_400_000), "312 MB");
        assert_eq!(super::format_bytes(3_420_000_000), "3.4 GB");
        assert_eq!(super::format_bytes(0), "0 MB");
    }
}
