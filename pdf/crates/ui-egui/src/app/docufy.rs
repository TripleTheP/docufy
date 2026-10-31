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

    /// The active PDF's name and a reader for its text, all pages in reading order, for an AI
    /// summary. The reader parses the file again, so it runs off the interface thread.
    #[allow(clippy::type_complexity)]
    pub fn active_pdf_text(&self) -> Option<(String, Box<dyn FnOnce() -> Result<String, String> + Send>)> {
        let view = self.views.get(self.active?)?;
        let doc = self.session.get(view.id)?;
        let (bytes, pages) = (doc.bytes.clone(), doc.info.pages.len());
        let password: Option<std::sync::Arc<str>> = doc.password.as_deref().map(std::sync::Arc::from);
        Some((doc.name.clone(), Box::new(move || pdf_text(bytes, password, pages, PDF_TEXT_LIMIT))))
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

/// The most text read from a PDF for a summary (bytes); the AI request takes less still.
const PDF_TEXT_LIMIT: usize = 1 << 20;

/// The text of a PDF's pages, each after a `[Page n]` line, until `limit` bytes are read. Pages
/// without text are left out; empty when no page has any (a scan without OCR).
fn pdf_text(bytes: std::sync::Arc<Vec<u8>>, password: Option<std::sync::Arc<str>>, pages: usize, limit: usize) -> Result<String, String> {
    use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};
    let mut r = PageRenderer::new(bytes, RenderConfig { password, ..Default::default() });
    let mut out = String::new();
    for page in 0..pages {
        let rendered = r.render(RenderRequest { page, kind: RequestKind::Text, tile: None, scale: 1.0, tag: 0, preview: false });
        match (rendered.error, rendered.text) {
            // A file that can't be read at all fails on its first page.
            (Some(e), _) if page == 0 => return Err(e),
            (Some(e), _) => log::warn!("page {}: {e}", page + 1),
            (None, Some(text)) => {
                let text = text.plain_text();
                if !text.trim().is_empty() {
                    out.push_str(&format!("[Page {}]\n{}\n\n", page + 1, text.trim()));
                }
            }
            (None, None) => {}
        }
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
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

    /// A one-page PDF showing `text` in Helvetica (an empty `text`: a page with no text).
    fn text_pdf(text: &str) -> Vec<u8> {
        let content = if text.is_empty() { String::new() } else { format!("BT /F1 12 Tf 20 250 Td ({text}) Tj ET") };
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 300] >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    /// An AI summary reads the active PDF's text, page by page, off the interface thread.
    #[test]
    fn the_active_pdfs_text_is_read_for_a_summary() {
        let mut app = PdfCraftApp::new();
        assert!(app.active_pdf_text().is_none(), "no PDF open");
        app.open_bytes("Brief.pdf", None, text_pdf("Hallo Welt")).unwrap();
        let (name, read) = app.active_pdf_text().unwrap();
        assert_eq!(name, "Brief.pdf");
        let text = std::thread::spawn(read).join().unwrap().unwrap();
        assert!(text.starts_with("[Page 1]\n") && text.contains("Hallo Welt"), "{text:?}");
        app.open_bytes("Scan.pdf", None, text_pdf("")).unwrap();
        let (_, read) = app.active_pdf_text().unwrap();
        assert_eq!(read().unwrap(), "", "a page without text adds nothing");
        assert!(pdf_text(std::sync::Arc::new(b"not a pdf".to_vec()), None, 1, PDF_TEXT_LIMIT).is_err());
    }
}
