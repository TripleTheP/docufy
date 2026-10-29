//! Docufy: what the word processor's window needs to host the PDF interface beside its own.

use super::PdfCraftApp;

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
