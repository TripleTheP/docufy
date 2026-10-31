//! What the next start does differently, chosen in the diagnostics: safe mode, and settings put
//! back to their defaults. Each is a marker file in the settings folder that the next start reads
//! and removes ([`take`]), so it happens once.

use std::path::{Path, PathBuf};

/// Start in safe mode: drawn by the processor, without AI.
pub const SAFE_MODE: &str = "safe-mode-next-start";
/// Start with default settings ([`reset_settings`]).
pub const RESET_SETTINGS: &str = "reset-settings-next-start";

/// The settings files a reset sets aside: the word processor's and the PDF mode's.
pub const SETTINGS_FILES: [&str; 2] = ["ui.json", "pdf-mode.json"];

/// Ask for (`on`) or call off what `marker` stands for at the next start.
pub fn set(dir: &Path, marker: &str, on: bool) -> Result<(), String> {
    let path = dir.join(marker);
    if on {
        std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, b"")).map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Whether `marker` is asked for at the next start.
pub fn is_set(dir: &Path, marker: &str) -> bool {
    dir.join(marker).is_file()
}

/// At start: whether `marker` was asked for; it is removed, so it applies to this start only.
pub fn take(dir: &Path, marker: &str) -> bool {
    let path = dir.join(marker);
    if !path.is_file() {
        return false;
    }
    if let Err(e) = std::fs::remove_file(&path) {
        // A marker that can't be removed would apply at every start; better not at all.
        log_warning(&format!("{}: {e}", path.display()));
        return false;
    }
    true
}

/// Set the settings files aside as `<name>.bak` (replacing an older one), so the app starts with
/// defaults and the old settings can still be restored by hand. Documents, AutoRecover copies,
/// the dictionary and API keys stay. Returns the files set aside.
pub fn reset_settings(dir: &Path) -> Vec<PathBuf> {
    let mut moved = Vec::new();
    for name in SETTINGS_FILES {
        let from = dir.join(name);
        if !from.is_file() {
            continue;
        }
        let to = dir.join(format!("{name}.bak"));
        match std::fs::rename(&from, &to) {
            Ok(()) => moved.push(to),
            Err(e) => log_warning(&format!("{}: {e}", from.display())),
        }
    }
    moved
}

fn log_warning(text: &str) {
    log::warn!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("docufy-startup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_marker_applies_to_one_start() {
        let d = dir("marker");
        assert!(!is_set(&d, SAFE_MODE));
        set(&d, SAFE_MODE, true).unwrap();
        assert!(is_set(&d, SAFE_MODE));
        assert!(take(&d, SAFE_MODE));
        assert!(!take(&d, SAFE_MODE), "only once");
        set(&d, SAFE_MODE, true).unwrap();
        set(&d, SAFE_MODE, false).unwrap();
        set(&d, SAFE_MODE, false).unwrap();
        assert!(!is_set(&d, SAFE_MODE));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_reset_sets_settings_aside_and_keeps_the_rest() {
        let d = dir("reset");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("ui.json"), b"{\"ai\":{}}").unwrap();
        std::fs::write(d.join("ui.json.bak"), b"older").unwrap();
        std::fs::write(d.join("custom-dictionary.txt"), b"Docufy").unwrap();
        let moved = reset_settings(&d);
        assert_eq!(moved, vec![d.join("ui.json.bak")]);
        assert!(!d.join("ui.json").exists());
        assert_eq!(std::fs::read(d.join("ui.json.bak")).unwrap(), b"{\"ai\":{}}");
        assert!(d.join("custom-dictionary.txt").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
