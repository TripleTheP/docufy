//! The runtime local models run with: Microsoft's ONNX Runtime GenAI (DirectML build), ONNX
//! Runtime with the DirectML provider, and DirectML. Downloaded once from Microsoft's GitHub
//! release and NuGet, each package checked against its SHA-256 before the libraries are taken out
//! of it. Windows x64 only for now.

/// The GenAI version the host (`docufy-ai-host`) is written for.
pub const VERSION: &str = "0.14.0";

/// A package the runtime's libraries come out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Package {
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// (path in the package, file name in the runtime folder).
    pub entries: &'static [(&'static str, &'static str)],
}

pub static PACKAGES: [Package; 3] = [
    Package {
        name: "ONNX Runtime GenAI 0.14.0 (DirectML)",
        url: "https://github.com/microsoft/onnxruntime-genai/releases/download/v0.14.0/onnxruntime-genai-0.14.0-win-x64-dml.zip",
        size: 17_587_651,
        sha256: "3ad50e0b978e9095909dadd4c8fcc7c2431f0d3be035ea2c7d142b71b403ea99",
        entries: &[("onnxruntime-genai-0.14.0-win-x64-dml/lib/onnxruntime-genai.dll", "onnxruntime-genai.dll")],
    },
    // GenAI 0.14 needs ONNX Runtime's API version 24, so 1.24.
    Package {
        name: "ONNX Runtime 1.24.4 (DirectML)",
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg",
        size: 12_458_649,
        sha256: "57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681",
        entries: &[
            ("runtimes/win-x64/native/onnxruntime.dll", "onnxruntime.dll"),
            ("runtimes/win-x64/native/onnxruntime_providers_shared.dll", "onnxruntime_providers_shared.dll"),
        ],
    },
    // The package holds DirectML for many platforms (200 MB); only the Windows library is kept.
    Package {
        name: "DirectML 1.15.4",
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg",
        size: 202_292_617,
        sha256: "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9",
        entries: &[("bin/x64-win/DirectML.dll", "DirectML.dll")],
    },
];

/// Whether this system can run the runtime at all.
pub fn supported() -> bool {
    cfg!(all(windows, target_arch = "x86_64"))
}

/// The bytes to download for the runtime.
pub fn download_bytes() -> u64 {
    PACKAGES.iter().map(|p| p.size).sum()
}

/// The files the runtime folder holds once installed.
pub fn files() -> impl Iterator<Item = &'static str> {
    PACKAGES.iter().flat_map(|p| p.entries.iter().map(|(_, name)| *name))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_runtime_has_the_host_library_and_its_dependencies() {
        let files: Vec<&str> = super::files().collect();
        assert_eq!(files, ["onnxruntime-genai.dll", "onnxruntime.dll", "onnxruntime_providers_shared.dll", "DirectML.dll"]);
        assert!(super::PACKAGES.iter().all(|p| p.url.starts_with("https://") && p.sha256.len() == 64));
        assert_eq!(super::download_bytes(), 17_587_651 + 12_458_649 + 202_292_617);
    }
}
