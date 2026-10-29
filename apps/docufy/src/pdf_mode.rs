//! The PDF mode: the PDF interface (`pdf/`, from PdfCraft) shown in the window in place of the
//! word processor. Both interfaces share the window and its egui context; one is shown at a
//! time, and each reinstalls its fonts and theme when it comes back ([`WordApp::restyle`],
//! [`PdfCraftApp::restyle`]). PDFs the word processor is asked to open come here, and Word
//! documents opened here go to the word processor.

use std::path::PathBuf;

use docufy_ui_egui::WordApp;
use pdfcraft_ui_egui::{PdfCraftApp, RecoveryStore};

/// The PDF interface's settings file, in Docufy's settings folder.
const SETTINGS_FILE: &str = "pdf-mode.json";

pub struct PdfMode {
    pub app: PdfCraftApp,
    /// Whether the window shows the PDF interface.
    pub shown: bool,
    /// Docufy's settings folder; `None` keeps no settings or recovery copies (agents' test runs).
    settings: Option<PathBuf>,
}

impl PdfMode {
    pub fn new(settings: Option<PathBuf>) -> Self {
        let mut app = PdfCraftApp::new();
        app.units = pdfcraft_ui_egui::units::Unit::system();
        if let Some(json) = settings.as_ref().and_then(|d| std::fs::read_to_string(d.join(SETTINGS_FILE)).ok()) {
            app.restore(&json);
        }
        app.integrated_titlebar = cfg!(target_os = "macos");
        if let Some(dir) = &settings {
            app.enable_recovery(RecoveryStore::new(dir.join("pdf-recovery")));
        }
        Self { app, shown: false, settings }
    }

    /// Hands files and mode switches between the two interfaces; runs before each frame's logic.
    pub fn route(&mut self, word: &mut WordApp, ctx: &egui::Context) {
        for path in std::mem::take(&mut word.pdf_requests) {
            self.app.open_path(&path);
        }
        if std::mem::take(&mut word.switch_to_pdf) {
            self.show(word, ctx, true);
        }
        for path in std::mem::take(&mut self.app.document_requests) {
            if let Err(e) = word.open_file(&path, None) {
                log::warn!("{path}: {e}");
            }
        }
        if std::mem::take(&mut self.app.switch_to_documents) {
            self.show(word, ctx, false);
        }
    }

    /// Show the PDF interface (`pdf`) or the word processor.
    pub fn show(&mut self, word: &mut WordApp, ctx: &egui::Context, pdf: bool) {
        if self.shown == pdf {
            return;
        }
        self.shown = pdf;
        if pdf {
            self.app.restyle();
        } else {
            word.restyle();
        }
        ctx.request_repaint();
    }

    /// Write the PDF interface's settings for the next start.
    pub fn save(&self) {
        let Some(dir) = &self.settings else { return };
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(SETTINGS_FILE), self.app.persist())) {
            log::warn!("PDF mode settings not saved: {e}");
        }
    }
}
