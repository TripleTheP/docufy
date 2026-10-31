//! The self-test: checks of what Docufy needs to work well, each with a status and, for a
//! problem, what to do ([`run`]). The checks are pure; [`writable`] and [`free_space`] read the
//! file system for their inputs.
//!
//! Texts are English templates with `{name}` placeholders, for the front end to translate and fill.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: &'static str,
    pub status: Status,
    /// What was checked.
    pub title: &'static str,
    /// What was found.
    pub template: &'static str,
    pub args: Vec<(&'static str, String)>,
    /// What to do about a problem.
    pub fix: Option<&'static str>,
}

impl Check {
    fn new(id: &'static str, status: Status, title: &'static str, template: &'static str) -> Check {
        Check { id, status, title, template, args: Vec::new(), fix: None }
    }

    fn arg(mut self, name: &'static str, value: impl Into<String>) -> Check {
        self.args.push((name, value.into()));
        self
    }

    fn fix(mut self, fix: &'static str) -> Check {
        self.fix = Some(fix);
        self
    }

    /// `template` filled with the args (untranslated).
    pub fn text(&self) -> String {
        fill(self.template, &self.args)
    }
}

/// `template` with each `{name}` replaced by its value.
pub fn fill(template: &str, args: &[(&str, String)]) -> String {
    args.iter().fold(template.to_string(), |t, (name, value)| t.replace(&format!("{{{name}}}"), value))
}

/// The graphics adapter the window draws with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graphics {
    pub name: String,
    pub backend: String,
    /// Drawn by the processor (WARP, llvmpipe): no graphics chip is used.
    pub software: bool,
}

/// What the AI check looks at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiState {
    pub enabled: bool,
    /// The provider runs on this computer (models come later).
    pub local: bool,
    pub has_key: bool,
    pub provider: String,
}

/// Everything the checks look at.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// Where preferences, recovery copies and logs go, and whether it can be written (`Err`: why not).
    pub settings: Option<(PathBuf, Result<(), String>)>,
    /// Free space on the settings folder's drive.
    pub disk_free: Option<u64>,
    pub ram_available: u64,
    pub ram_total: u64,
    pub graphics: Option<Graphics>,
    /// The log file, if there is one.
    pub log_file: Option<PathBuf>,
    /// Font families the word processor can use.
    pub fonts: usize,
    pub ai: AiState,
    pub safe_mode: bool,
}

const GB: u64 = 1_000_000_000;

/// The checks, worst first.
pub fn run(i: &Inputs) -> Vec<Check> {
    let mut out = vec![settings(i), disk(i), memory(i), graphics(i), fonts(i), log(i), ai(i)];
    if i.safe_mode {
        out.push(
            Check::new("safe-mode", Status::Warn, "Safe mode", "Docufy runs in safe mode: without graphics acceleration and without AI.")
                .fix("Restart Docufy normally once the problem is solved."),
        );
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.status));
    out
}

fn settings(i: &Inputs) -> Check {
    const TITLE: &str = "Settings folder";
    match &i.settings {
        None => Check::new("settings", Status::Warn, TITLE, "Docufy keeps no settings in this run: preferences, AutoRecover copies and the log aren't saved.")
            .fix("Start Docufy without DOCUFY_NO_PREFS."),
        Some((dir, Ok(()))) => Check::new("settings", Status::Ok, TITLE, "{path} can be written.").arg("path", dir.display().to_string()),
        Some((dir, Err(e))) => Check::new("settings", Status::Fail, TITLE, "{path} can't be written ({error}): preferences and AutoRecover copies are lost.")
            .arg("path", dir.display().to_string())
            .arg("error", e.clone())
            .fix("Check the folder's permissions and that the drive isn't full or read-only; antivirus software can block it too."),
    }
}

fn disk(i: &Inputs) -> Check {
    const TITLE: &str = "Disk space";
    let Some(free) = i.disk_free else { return Check::new("disk", Status::Ok, TITLE, "Not checked.") };
    let free_text = crate::format_bytes(free);
    if free < GB / 5 {
        Check::new("disk", Status::Fail, TITLE, "Only {free} free: saving and AutoRecover can fail.").arg("free", free_text).fix("Free up space on the drive.")
    } else if free < 2 * GB {
        Check::new("disk", Status::Warn, TITLE, "{free} free: little room for large documents and AutoRecover copies.").arg("free", free_text).fix("Free up space on the drive.")
    } else {
        Check::new("disk", Status::Ok, TITLE, "{free} free.").arg("free", free_text)
    }
}

fn memory(i: &Inputs) -> Check {
    const TITLE: &str = "Memory";
    if i.ram_total == 0 {
        return Check::new("memory", Status::Ok, TITLE, "Not checked.");
    }
    let (free, total) = (crate::format_bytes(i.ram_available), crate::format_bytes(i.ram_total));
    if i.ram_available < GB / 2 {
        Check::new("memory", Status::Warn, TITLE, "{free} of {total} free: Docufy and other programs can slow down.")
            .arg("free", free)
            .arg("total", total)
            .fix("Close programs or documents you don't need.")
    } else {
        Check::new("memory", Status::Ok, TITLE, "{free} of {total} free.").arg("free", free).arg("total", total)
    }
}

fn graphics(i: &Inputs) -> Check {
    const TITLE: &str = "Graphics";
    match &i.graphics {
        None => Check::new("graphics", Status::Ok, TITLE, "Not checked."),
        Some(g) if g.software && i.safe_mode => {
            Check::new("graphics", Status::Ok, TITLE, "{name} ({backend}): drawn by the processor, as safe mode asks.").arg("name", g.name.clone()).arg("backend", g.backend.clone())
        }
        Some(g) if g.software => Check::new("graphics", Status::Warn, TITLE, "{name} ({backend}) draws on the processor: scrolling and zooming can lag.")
            .arg("name", g.name.clone())
            .arg("backend", g.backend.clone())
            .fix("Install the current driver for your graphics chip from its maker (Intel, AMD, NVIDIA)."),
        Some(g) => Check::new("graphics", Status::Ok, TITLE, "{name} ({backend}).").arg("name", g.name.clone()).arg("backend", g.backend.clone()),
    }
}

fn fonts(i: &Inputs) -> Check {
    const TITLE: &str = "Fonts";
    if i.fonts == 0 {
        Check::new("fonts", Status::Warn, TITLE, "No fonts were found: documents show in a substitute font.").fix("Check that the system's fonts folder can be read.")
    } else {
        Check::new("fonts", Status::Ok, TITLE, "{count} font families.").arg("count", i.fonts.to_string())
    }
}

fn log(i: &Inputs) -> Check {
    const TITLE: &str = "Log file";
    match &i.log_file {
        Some(p) => Check::new("log", Status::Ok, TITLE, "{path}").arg("path", p.display().to_string()),
        None => Check::new("log", Status::Warn, TITLE, "Docufy writes no log file in this run: a problem leaves no record.")
            .fix("Check that the settings folder can be written."),
    }
}

fn ai(i: &Inputs) -> Check {
    const TITLE: &str = "AI";
    let a = &i.ai;
    if !a.enabled {
        Check::new("ai", Status::Ok, TITLE, "Off.")
    } else if i.safe_mode {
        Check::new("ai", Status::Ok, TITLE, "Off in safe mode.")
    } else if a.local {
        Check::new("ai", Status::Warn, TITLE, "Models on this computer come in a later version.").fix("Choose a provider in File › Options › AI.")
    } else if !a.has_key {
        Check::new("ai", Status::Fail, TITLE, "No API key is stored for {provider}.").arg("provider", a.provider.clone()).fix("Store a key in File › Options › AI.")
    } else {
        Check::new("ai", Status::Ok, TITLE, "{provider}, key stored.").arg("provider", a.provider.clone())
    }
}

/// Whether files can be created in `dir` (it is created if missing): writes and removes a small
/// file.
pub fn writable(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let probe = dir.join(format!(".docufy-selftest-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| e.to_string())?;
    std::fs::remove_file(&probe).map_err(|e| e.to_string())
}

/// Free space on the drive that holds `dir`.
#[cfg(not(target_arch = "wasm32"))]
pub fn free_space(dir: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list().ok()?;
    let dir = dir.ancestors().find(|p| p.exists()).unwrap_or(dir);
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    // Windows writes canonical paths as `\\?\C:\…`; drive letters may differ in case.
    let dir = dir.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
    // The disk mounted deepest above `dir`.
    disks
        .iter()
        .filter(|d| dir.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Inputs {
        Inputs {
            settings: Some((PathBuf::from("/s"), Ok(()))),
            disk_free: Some(100 * GB),
            ram_available: 8 * GB,
            ram_total: 16 * GB,
            graphics: Some(Graphics { name: "RTX".into(), backend: "Dx12".into(), software: false }),
            log_file: Some(PathBuf::from("/s/logs/docufy.log")),
            fonts: 120,
            ai: AiState::default(),
            safe_mode: false,
        }
    }

    fn status(checks: &[Check], id: &str) -> Status {
        checks.iter().find(|c| c.id == id).map(|c| c.status).unwrap()
    }

    #[test]
    fn a_healthy_computer_passes() {
        let c = run(&good());
        assert!(c.iter().all(|c| c.status == Status::Ok && c.fix.is_none()), "{c:#?}");
        assert_eq!(c.len(), 7);
    }

    #[test]
    fn problems_come_first_with_what_to_do() {
        let mut i = good();
        i.settings = Some((PathBuf::from("/s"), Err("access denied".into())));
        i.disk_free = Some(GB);
        i.ram_available = GB / 4;
        i.graphics = Some(Graphics { name: "Microsoft Basic Render Driver".into(), backend: "Dx12".into(), software: true });
        i.ai = AiState { enabled: true, local: false, has_key: false, provider: "Anthropic".into() };
        let c = run(&i);
        assert_eq!(c[0].status, Status::Fail);
        assert!(c.windows(2).all(|w| w[0].status >= w[1].status), "worst first");
        assert_eq!(status(&c, "settings"), Status::Fail);
        assert_eq!(status(&c, "ai"), Status::Fail);
        assert_eq!(status(&c, "disk"), Status::Warn);
        assert_eq!(status(&c, "memory"), Status::Warn);
        assert_eq!(status(&c, "graphics"), Status::Warn);
        assert!(c.iter().filter(|c| c.status != Status::Ok).all(|c| c.fix.is_some()));
        let settings = c.iter().find(|c| c.id == "settings").unwrap();
        assert_eq!(settings.text(), "/s can't be written (access denied): preferences and AutoRecover copies are lost.");
    }

    #[test]
    fn safe_mode_explains_software_drawing_and_ai_off() {
        let mut i = good();
        i.safe_mode = true;
        i.graphics = Some(Graphics { name: "WARP".into(), backend: "Dx12".into(), software: true });
        i.ai.enabled = true;
        let c = run(&i);
        assert_eq!(status(&c, "graphics"), Status::Ok);
        assert_eq!(status(&c, "ai"), Status::Ok);
        assert_eq!(c[0].id, "safe-mode");
    }

    #[test]
    fn a_folder_is_tried_for_writing() {
        let dir = std::env::temp_dir().join(format!("docufy-diag-test-{}", std::process::id()));
        assert_eq!(writable(&dir), Ok(()));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the probe file is removed");
        std::fs::remove_dir(&dir).unwrap();
        assert!(free_space(&std::env::temp_dir()).is_some_and(|f| f > 0));
    }
}
