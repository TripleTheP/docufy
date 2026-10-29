<h1 align="center">Docufy</h1>

<p align="center">
  <b>Textverarbeitung und PDF-Werkzeug in einem – quelloffen, schnell und in reinem Rust geschrieben.</b>
</p>

<p align="center">
  <img alt="Geschrieben in Rust" src="https://img.shields.io/badge/geschrieben%20in-Rust-2b47b5?style=flat-square&logo=rust&logoColor=white">
  <img alt="Läuft auf Windows, macOS, Linux und im Browser" src="https://img.shields.io/badge/l%C3%A4uft%20auf-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux%20%C2%B7%20Web-3b5bdb?style=flat-square">
  <img alt="Lizenz: MIT OR Apache-2.0" src="https://img.shields.io/badge/Lizenz-MIT%20%2F%20Apache--2.0-2b47b5?style=flat-square">
</p>

Docufy ist ein Fork von [WordCraft](https://github.com/storytold/wordcraft), einer quelloffenen
Textverarbeitung im Stil von Microsoft Word. Docufy übernimmt deren Funktionsumfang und erweitert
ihn um das, was im Alltag fehlt: PDFs direkt in derselben Anwendung lesen und bearbeiten, Dokumente
in der Cloud speichern und eine durchgängige dunkle Oberfläche.

> [!NOTE]
> Docufy ist in einem frühen Stadium. Die Textverarbeitung aus WordCraft ist bereits gut nutzbar;
> die Docufy-eigenen Funktionen unten entstehen gerade.

## Was Docufy anders macht

| Funktion | Stand |
|---|---|
| **Dunkler Modus** für die gesamte Oberfläche, umschaltbar oder dem System folgend | in Arbeit |
| **PDF-Modus**: PDFs öffnen, lesen, durchsuchen, Seiten drehen/löschen/umsortieren, zusammenführen, teilen, kommentieren und Formulare ausfüllen – auf Basis der Bibliotheken von [PdfCraft](https://github.com/storytold/pdfcraft) | geplant |
| **Cloud-Speicher** mit automatischem Speichern: OneDrive zuerst, danach weitere Dienste (z. B. Google Drive, Dropbox, Nextcloud/WebDAV) | geplant |

## Was aus WordCraft schon funktioniert

- **Schreiben und Formatieren:** Menüband, Formatvorlagen mit Live-Vorschau, Listen, Tabellen,
  Bilder und Formen, Seitenlayout, Kopf- und Fußzeilen, Spalten.
- **Überarbeiten und Verweise:** Änderungen nachverfolgen, Kommentare, Rechtschreib- und
  Grammatikprüfung (offline), Inhaltsverzeichnis, Fußnoten, Zitate und Literaturverzeichnis.
- **Seriendruck:** CSV-Datenquelle, Seriendruckfelder, Umschläge und Etiketten.
- **Dateiformate:** .docx lesen und schreiben, dazu .odt, .rtf, .html, .md, .tex und .txt;
  alte .doc-Dateien importieren; PDF-Export mit auswählbarem Text.
- **Automatisierung:** Jede Aktion ist ein Befehl mit einer ID. Dieselben Befehle steuern
  Menüband, Tastenkürzel, ein Kommandozeilenwerkzeug und einen MCP-Server für KI-Agenten.

## Bauen und starten

Voraussetzungen: [Rust](https://rustup.rs/) 1.95 oder neuer; unter Windows zusätzlich die
Visual Studio C++ Build Tools.

```sh
git clone https://github.com/TripleTheP/docufy
cd docufy
cargo run --release -p docufy -- --sample        # Desktop-App mit Beispieldokument
cargo run --release -p docufy -- bericht.docx    # ein Dokument öffnen
```

Kommandozeile:

```sh
docufy-cli convert bericht.docx bericht.pdf      # docx, pdf, odt, rtf, html, md, tex, txt, png
docufy-cli text bericht.docx                     # reiner Text
docufy-cli mcp                                   # MCP-Server für KI-Agenten
```

Web: `cd apps/docufy-web && trunk serve`, dann <http://127.0.0.1:8771/?sample> öffnen.

## Aufbau

Docufy ist ein Cargo-Workspace mit Schichten, die nur nach unten abhängen:

| Schicht | Crate | Aufgabe |
|---|---|---|
| L0 | `geom` | Einheiten, Rechtecke, Maße |
| L1 | `doc`, `fonts`, `proof` | Dokumentmodell, Schriften, Rechtschreibung |
| L2 | `layout`, `docx`, `formats` | Umbruch und Seitenaufteilung, OOXML, weitere Formate |
| L3 | `render`, `pdf` | Rasterung, PDF-Export |
| L4 | `engine` | Sitzung, Rückgängig, Befehlsregister |
| L5 | `mcp` | MCP-Server |
| L6 | `ui-egui` | Oberfläche (egui) |

Die Apps liegen unter `apps/`: `docufy` (Desktop), `docufy-cli` und `docufy-web`.
Vor jedem Commit: `cargo xtask ci` (Formatierung, Clippy, Tests, Asset- und Schichtenprüfung, WASM).

## Lizenz und Herkunft

Docufy steht wie WordCraft wahlweise unter der [MIT-Lizenz](LICENSE-MIT) oder der
[Apache-Lizenz 2.0](LICENSE-APACHE). Der Großteil des Codes stammt von den WordCraft-Mitwirkenden
und dem ArtCraft-Team; die erforderlichen Hinweise stehen in [NOTICE](NOTICE), die Herkunft jeder
mitgelieferten Datei in [ATTRIBUTION.md](ATTRIBUTION.md).

Docufy ist kein Produkt des ArtCraft-Teams und wird von ihm weder unterstützt noch empfohlen.
Name und Logos von ArtCraft sind Marken des ArtCraft-Teams und nicht Teil von Docufy.
