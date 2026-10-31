//! The PDF mode: the PDF interface (`pdf/`, from PdfCraft) shown in the window in place of the
//! word processor. Both interfaces share the window and its egui context; one is shown at a
//! time, and each reinstalls its fonts and theme when it comes back ([`WordApp::restyle`],
//! [`PdfCraftApp::restyle`]). PDFs the word processor is asked to open come here, and Word
//! documents opened here go to the word processor.

use std::path::PathBuf;
use std::sync::Arc;

use docufy_ui_egui::WordApp;
use pdfcraft_ui_egui::{PdfCraftApp, RecoveryStore};

/// The PDF interface's settings file, in Docufy's settings folder.
const SETTINGS_FILE: &str = "pdf-mode.json";

/// The shared context's look: both themes' styles and which of them shows.
struct Look {
    light: Arc<egui::Style>,
    dark: Arc<egui::Style>,
    preference: egui::ThemePreference,
    fallback: egui::Theme,
}

impl Look {
    fn of(ctx: &egui::Context) -> Look {
        let (preference, fallback) = ctx.options(|o| (o.theme_preference, o.fallback_theme));
        Look { light: ctx.style_of(egui::Theme::Light), dark: ctx.style_of(egui::Theme::Dark), preference, fallback }
    }

    /// Keep this look's text options under another theme: egui rebuilds the shared glyph atlas
    /// when they change, and text the main window already laid out this frame would then point
    /// into the new one.
    fn lend_text_options(&self, ctx: &egui::Context) {
        ctx.style_mut_of(egui::Theme::Light, |s| s.visuals.text_options = self.light.visuals.text_options);
        ctx.style_mut_of(egui::Theme::Dark, |s| s.visuals.text_options = self.dark.visuals.text_options);
    }

    fn put_back(self, ctx: &egui::Context) {
        ctx.set_style_of(egui::Theme::Light, self.light);
        ctx.set_style_of(egui::Theme::Dark, self.dark);
        ctx.options_mut(|o| {
            o.theme_preference = self.preference;
            o.fallback_theme = self.fallback;
        });
    }
}

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
        // AI: the PDF interface offers a summary while AI is on in the word processor. The
        // summary streams into the word processor's AI Assistant, which is shown for it.
        self.app.ai_available = word.ui.ai.enabled;
        if std::mem::take(&mut self.app.summarize_requested) | word.take_pdf_summary_request() {
            match self.app.active_pdf_text() {
                Some((name, read)) => match docufy_ui_egui::ai_assist::summarize_source(word, name, read) {
                    Ok(_) => self.show(word, ctx, false),
                    Err(e) if self.shown => self.app.notify(e),
                    Err(e) => word.status(e),
                },
                None if self.shown => self.app.notify(pdfcraft_ui_egui::i18n::t("Open a PDF to summarise first.")),
                None => word.status(docufy_ui_egui::i18n::t("Open a PDF in PDF mode to summarise it.")),
            }
        }
        if std::mem::take(&mut self.app.switch_to_documents) | std::mem::take(&mut word.leave_pdf) {
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

    /// The word processor's other windows while the PDF interface has the main one: they stay
    /// open and work. The fonts and text options are the PDF interface's (both name their
    /// families alike); the word processor's theme is put in for them and the PDF interface's
    /// back afterwards.
    pub fn word_windows(&self, word: &mut WordApp, ui: &mut egui::Ui) {
        if !word.has_other_windows() {
            word.other_windows_ui(ui);
            return;
        }
        let ctx = ui.ctx().clone();
        let pdf_look = Look::of(&ctx);
        word.install_theme(&ctx);
        pdf_look.lend_text_options(&ctx);
        word.other_windows_ui(ui);
        pdf_look.put_back(&ctx);
    }

    /// Write the PDF interface's settings for the next start.
    pub fn save(&self) {
        let Some(dir) = &self.settings else { return };
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(SETTINGS_FILE), self.app.persist())) {
            log::warn!("PDF mode settings not saved: {e}");
        }
    }
}
