# Docufy roadmap

**Stage:** early (0.1). The word processor and the PDF tools come from WordCraft and PdfCraft;
their detailed parity assessments stay in [`docs/`](docs/) (for example
[`docs/target-app-parity.md`](docs/target-app-parity.md) and [`docs/gaps.md`](docs/gaps.md)).
This page lists what Docufy adds and what comes next.

## Done

| Version | What |
|---|---|
| 0.1.0 | Word processor and PDF tools in one window (PDF mode), both keep working in the background |
| 0.1.0 | Optional AI assistance, off by default: cloud providers with your own key, or models on this computer (ONNX Runtime GenAI, DirectML or processor, separate helper process) |
| 0.1.0 | AI Assistant: rewrite, continue writing, summarise documents and PDFs, chat; results are suggestions |
| 0.1.0 | Diagnostics: resource monitor, self-test, system information without personal data, safe mode, settings reset |
| 0.1.0 | German interface for the whole word processor (27 interface languages) |

## Next

- **Signed releases** and an automated release workflow (GitHub Actions) for Windows, then
  macOS and Linux packages.
- **Local AI on more hardware:** NPUs (Intel, AMD, Qualcomm), choosing between integrated and
  dedicated graphics, local models on Linux and macOS.
- **More from the AI Assistant:** proofreading suggestions inline in the text, translating the
  selection.
- Keep following WordCraft and PdfCraft improvements.

Ideas and bug reports: [Issues](https://github.com/TripleTheP/docufy/issues).
