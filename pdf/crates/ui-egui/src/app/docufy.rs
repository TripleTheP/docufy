//! Docufy: what the word processor's window needs to host the PDF interface beside its own.

use super::{PdfCraftApp, i18n};

/// Files the word processor opens rather than the PDF interface.
const DOCUMENT_EXTS: &[&str] = &["docx", "docm", "dotx", "dotm", "doc", "dot", "odt", "rtf", "md", "markdown", "tex", "latex", "ltx"];

/// Whether `path` names a word-processor document (by its extension).
pub fn is_word_document(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| DOCUMENT_EXTS.iter().any(|d| d.eq_ignore_ascii_case(e)))
}

impl PdfCraftApp {
    /// Reinstall this interface's fonts and theme on its next frame: the word processor shares the
    /// egui context and installs its own while it is shown.
    pub fn restyle(&mut self) {
        self.styled = false;
        self.fonts_ready = false;
    }

    /// Whether a PDF has changes that closing would lose (typed field text counts).
    pub fn has_unsaved_pdfs(&self) -> bool {
        self.first_dirty().is_some()
    }

    /// Start quitting: asks about unsaved PDFs first, as closing the window does. Returns false
    /// while that question waits for an answer.
    pub fn begin_quit(&mut self, ctx: &egui::Context) -> bool {
        self.request_quit(ctx)
    }

    /// Per-frame work while the word processor is shown instead: opens on the way finish,
    /// recovery copies of edited PDFs go on being written, and running jobs (print, export, OCR,
    /// optimise, tool actions) are followed. Fonts, theme, input, menus and dropped files wait
    /// until this interface is shown again ([`Self::restyle`]).
    pub fn background_logic(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
        i18n::set_current(i18n::Lang::from_pref(&self.language));
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(worker) = &self.open_worker {
                worker.set_context(ctx);
            }
            self.poll_pending_opens();
        }
        let failed: Vec<(String, String)> = self.failed_inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for (name, e) in failed {
            self.notify_fmt("Couldn't open {name}: {e}", &[("name", &name), ("e", &e)]);
        }
        self.autosave_tick(ctx, web_time::Instant::now());
        self.poll_print();
        self.poll_export();
        self.poll_ocr();
        self.poll_optimize();
        self.poll_action();
    }

    /// Hand a word-processor file to the word processor instead of opening it here.
    pub(crate) fn route_document(&mut self, path: &str) -> bool {
        if !is_word_document(path) {
            return false;
        }
        self.document_requests.push(path.to_owned());
        self.switch_to_documents = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Out of sight the PDF interface keeps its work going but installs no fonts or theme in the
    /// context the word processor is drawing with.
    #[test]
    fn background_logic_leaves_the_interface_alone() {
        let mut app = PdfCraftApp::new();
        let ctx = egui::Context::default();
        app.background_logic(&ctx);
        assert!(!app.styled && !app.fonts_ready, "fonts wait until the PDF interface shows");
        assert!(app.ctx.is_some(), "jobs that repaint have the context");
    }

    #[test]
    fn word_documents_go_to_the_word_processor() {
        let mut app = PdfCraftApp::new();
        assert!(app.route_document("C:/Briefe/Antrag.DOCX"));
        assert!(app.route_document("notes.md"));
        assert!(!app.route_document("scan.pdf"));
        assert!(!app.route_document("photo.png"));
        assert_eq!(app.document_requests, vec!["C:/Briefe/Antrag.DOCX".to_owned(), "notes.md".to_owned()]);
        assert!(app.switch_to_documents);
    }
}
