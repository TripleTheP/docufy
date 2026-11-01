<h1 align="center">Docufy</h1>

<p align="center">
  <b>Word processor and PDF tool in one – open source, fast, written in pure Rust.</b>
</p>

<p align="center">
  <a href="https://github.com/TripleTheP/docufy/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/TripleTheP/docufy?style=flat-square&color=2b47b5"></a>
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-2b47b5?style=flat-square&logo=rust&logoColor=white">
  <img alt="Licence: MIT OR Apache-2.0" src="https://img.shields.io/badge/licence-MIT%20%2F%20Apache--2.0-2b47b5?style=flat-square">
</p>

<p align="center"><b>English</b> · <a href="README.de.md">Deutsch</a></p>

Docufy edits Word documents and PDFs in the same window. It builds on
[WordCraft](https://github.com/storytold/wordcraft), an open-source word processor in the style
of Microsoft Word, and on the libraries of [PdfCraft](https://github.com/storytold/pdfcraft).
On top of these, Docufy adds a PDF mode, optional AI assistance and a diagnostics page.

> [!NOTE]
> Docufy is at an early stage (version 0.1). The word processor taken over from WordCraft is
> already very usable; please report problems under [Issues](https://github.com/TripleTheP/docufy/issues).

## Download

**[Latest release](https://github.com/TripleTheP/docufy/releases/latest)** for Windows (x64):

| File | |
|---|---|
| `docufy-<version>-windows-x64.msi` | Installer. Installs for the current user without administrator rights by default; optionally for everyone on the computer. |
| `docufy-<version>-windows-x64-portable.zip` | No installation: unpack and start `docufy.exe`. |

Checksums are in `SHA256SUMS.txt`. The files are **not code-signed** yet, so Windows SmartScreen
warns on the first start ("More info" → "Run anyway"). macOS and Linux: build from source (below).

## What Docufy does

### Word processor (from WordCraft)
- **Writing and formatting:** ribbon, styles with live preview, lists, tables, pictures and
  shapes, page layout, headers and footers, columns.
- **Reviewing and references:** track changes, comments, offline spelling and grammar checking,
  table of contents, footnotes, citations and bibliography.
- **Mail merge:** CSV data source, merge fields, envelopes and labels.
- **File formats:** read and write .docx, plus .odt, .rtf, .html, .md, .tex and .txt; import
  old .doc files; PDF export with selectable text.
- **Interface in 27 languages**, including English and German.

### PDF mode
The **PDF** button opens the PDF tools in the same window: read and search PDFs, rotate, delete
and reorder pages, merge and split, comment, fill in forms, OCR. The word processor keeps working
in the background (saving, AutoSave, further windows); **Documents** switches back.

### AI assistance – optional, off by default
Nothing is sent anywhere until you turn AI on in *File › Options › AI*.

- **Providers:** Anthropic (Claude), OpenAI, Google Gemini, Mistral (EU) and Azure OpenAI with
  your own API key, kept in the Windows Credential Manager.
- **On this computer:** Phi-4-mini, Qwen2.5 1.5B/3B, Mistral 7B and Llama 3.2 3B, run with
  ONNX Runtime GenAI on the graphics card (DirectML) or the processor. Runtime and models are
  downloaded only when you ask, checked against pinned SHA-256 checksums, and your text never
  leaves the computer. (Windows x64.)
- **AI Assistant** (*Home › AI*): rewrite the selected text (improve, shorter, longer, more
  formal, simpler, fix mistakes, your own instruction), continue writing, summarise the document
  or a PDF, and chat. Results are always suggestions; the document changes only when you accept.
- Notices on privacy, costs, memory, battery and on AI answers being possibly wrong.

### Diagnostics
*Help › Troubleshooting › Diagnostics*: live resource monitor (processor, memory, graphics
memory, frame time), self-test with suggested fixes, copy system information for bug reports
(without your user name or personal folders), open the log, safe mode for one start
(`--safe-mode`: no graphics acceleration, no AI) and resetting the settings.

### Automation
Every action is a command with an ID. The same commands drive the ribbon, shortcuts, a command
line tool, a local control channel and an MCP server for AI agents.

## Build from source

Requirements: [Rust](https://rustup.rs/) 1.95 or newer; on Windows also the Visual Studio C++
Build Tools.

```sh
git clone https://github.com/TripleTheP/docufy
cd docufy
cargo run --release -p docufy -- --sample        # desktop app with a sample document
cargo run --release -p docufy -- report.docx     # open a document
```

Command line:

```sh
docufy-cli convert report.docx report.pdf        # docx, pdf, odt, rtf, html, md, tex, txt, png
docufy-cli text report.docx                      # plain text
docufy-cli mcp                                   # MCP server for AI agents
```

Web: `cd apps/docufy-web && trunk serve`, then open <http://127.0.0.1:8771/?sample>.
Windows packages: `pwsh packaging/windows/package.ps1` (needs WiX v5).

## Architecture

Docufy is a Cargo workspace of layers that only depend downwards:

| Layer | Crate | Purpose |
|---|---|---|
| L0 | `geom`, `ai`, `diag` | units and geometry; AI providers, local models and warnings; diagnostics |
| L1 | `doc`, `fonts`, `proof` | document model, fonts, spelling |
| L2 | `layout`, `docx`, `formats` | line breaking and pagination, OOXML, further formats |
| L3 | `render`, `pdf` | rasterisation, PDF export |
| L4 | `engine` | session, undo, command registry |
| L5 | `mcp` | MCP server |
| L6 | `ui-egui` | user interface (egui) |

The PDF part lives in `pdf/` (PdfCraft's crates). The apps are in `apps/`: `docufy` (desktop),
`docufy-cli`, `docufy-web` and `docufy-ai-host`, the helper process that runs local AI models,
so a crash in the AI library never takes your documents with it. It is the only place with
`unsafe` code (the FFI to ONNX Runtime GenAI).

Before each commit: `cargo xtask ci` (formatting, Clippy, tests, asset and layer checks, WASM).

## Licence and origin

Like WordCraft, Docufy is available under the [MIT licence](LICENSE-MIT) or the
[Apache licence 2.0](LICENSE-APACHE), at your choice. Most of the code comes from the WordCraft
contributors and the ArtCraft team; the required notices are in [NOTICE](NOTICE), and the origin
of every bundled file is in [ATTRIBUTION.md](ATTRIBUTION.md). AI models downloaded in the app
come with their own licences, linked next to each model.

Docufy is not a product of the ArtCraft team and is neither supported nor endorsed by it.
The ArtCraft name and logos are trademarks of the ArtCraft team and not part of Docufy.
