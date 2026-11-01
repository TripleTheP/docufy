<h1 align="center">Docufy</h1>

<p align="center">
  <b>Textverarbeitung und PDF-Werkzeug in einem – quelloffen, schnell und in reinem Rust geschrieben.</b>
</p>

<p align="center">
  <a href="https://github.com/TripleTheP/docufy/releases/latest"><img alt="Neueste Version" src="https://img.shields.io/github/v/release/TripleTheP/docufy?style=flat-square&color=2b47b5&label=Version"></a>
  <img alt="Geschrieben in Rust" src="https://img.shields.io/badge/geschrieben%20in-Rust-2b47b5?style=flat-square&logo=rust&logoColor=white">
  <img alt="Lizenz: MIT OR Apache-2.0" src="https://img.shields.io/badge/Lizenz-MIT%20%2F%20Apache--2.0-2b47b5?style=flat-square">
</p>

<p align="center"><a href="README.md">English</a> · <b>Deutsch</b></p>

Docufy bearbeitet Word-Dokumente und PDFs im selben Fenster. Es baut auf
[WordCraft](https://github.com/storytold/wordcraft) auf, einer quelloffenen Textverarbeitung im
Stil von Microsoft Word, und auf den Bibliotheken von [PdfCraft](https://github.com/storytold/pdfcraft).
Docufy ergänzt einen PDF-Modus, eine optionale KI-Unterstützung und eine Diagnoseseite.

> [!NOTE]
> Docufy ist in einem frühen Stadium (Version 0.1). Die aus WordCraft übernommene Textverarbeitung
> ist bereits gut nutzbar; Probleme bitte unter [Issues](https://github.com/TripleTheP/docufy/issues) melden.

## Download

**[Neueste Version](https://github.com/TripleTheP/docufy/releases/latest)** für Windows (x64):

| Datei | |
|---|---|
| `docufy-<version>-windows-x64.msi` | Installer. Installiert standardmäßig nur für den aktuellen Benutzer, ohne Administratorrechte; wahlweise für alle Benutzer. |
| `docufy-<version>-windows-x64-portable.zip` | Ohne Installation: entpacken und `docufy.exe` starten. |

Prüfsummen stehen in `SHA256SUMS.txt`. Die Dateien sind noch **nicht signiert**; Windows
SmartScreen warnt deshalb beim ersten Start („Weitere Informationen" → „Trotzdem ausführen").
macOS und Linux: aus dem Quellcode bauen (siehe unten).

## Was Docufy kann

### Textverarbeitung (aus WordCraft)
- **Schreiben und Formatieren:** Menüband, Formatvorlagen mit Live-Vorschau, Listen, Tabellen,
  Bilder und Formen, Seitenlayout, Kopf- und Fußzeilen, Spalten.
- **Überarbeiten und Verweise:** Änderungen nachverfolgen, Kommentare, Rechtschreib- und
  Grammatikprüfung (offline), Inhaltsverzeichnis, Fußnoten, Zitate und Literaturverzeichnis.
- **Seriendruck:** CSV-Datenquelle, Seriendruckfelder, Umschläge und Etiketten.
- **Dateiformate:** .docx lesen und schreiben, dazu .odt, .rtf, .html, .md, .tex und .txt;
  alte .doc-Dateien importieren; PDF-Export mit auswählbarem Text.
- **Oberfläche in 27 Sprachen**, darunter Deutsch und Englisch.

### PDF-Modus
Die Schaltfläche **PDF** öffnet die PDF-Werkzeuge im selben Fenster: PDFs lesen und durchsuchen,
Seiten drehen, löschen und umsortieren, zusammenführen und teilen, kommentieren, Formulare
ausfüllen, Texterkennung (OCR). Die Textverarbeitung arbeitet im Hintergrund weiter (Speichern,
AutoSpeichern, weitere Fenster); **Dokumente** wechselt zurück.

### KI-Unterstützung – optional, standardmäßig aus
Nichts wird gesendet, bevor Sie die KI unter *Datei › Optionen › KI* einschalten.

- **Anbieter:** Anthropic (Claude), OpenAI, Google Gemini, Mistral (EU) und Azure OpenAI mit
  Ihrem eigenen API-Schlüssel, gespeichert in der Windows-Anmeldeinformationsverwaltung.
- **Auf diesem Computer:** Phi-4-mini, Qwen2.5 1.5B/3B, Mistral 7B und Llama 3.2 3B, ausgeführt
  mit ONNX Runtime GenAI auf der Grafikkarte (DirectML) oder dem Prozessor. Laufzeit und Modelle
  werden erst auf Wunsch heruntergeladen und gegen feste SHA-256-Prüfsummen geprüft; Ihr Text
  verlässt den Computer nicht. (Windows x64.)
- **KI-Assistent** (*Start › KI*): markierten Text umschreiben (verbessern, kürzer, länger,
  förmlicher, einfacher, Fehler korrigieren, eigene Anweisung), weiterschreiben, Dokument oder
  PDF zusammenfassen und chatten. Ergebnisse sind immer Vorschläge; das Dokument ändert sich erst,
  wenn Sie übernehmen.
- Hinweise zu Datenschutz, Kosten, Speicher, Akku und dazu, dass KI-Antworten falsch sein können.

### Diagnose
*Hilfe › Problembehandlung › Diagnose*: Live-Ressourcenmonitor (Prozessor, Arbeitsspeicher,
Grafikspeicher, Bildzeit), Selbsttest mit Lösungsvorschlägen, Systeminformationen für
Fehlerberichte kopieren (ohne Benutzernamen und persönliche Ordner), Protokoll öffnen,
abgesicherter Modus für einen Start (`--safe-mode`: ohne Grafikbeschleunigung und ohne KI) und
Einstellungen zurücksetzen.

### Automatisierung
Jede Aktion ist ein Befehl mit einer ID. Dieselben Befehle steuern Menüband, Tastenkürzel, ein
Kommandozeilenwerkzeug, einen lokalen Steuerkanal und einen MCP-Server für KI-Agenten.

## Aus dem Quellcode bauen

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
Windows-Pakete: `pwsh packaging/windows/package.ps1` (benötigt WiX v5).

## Aufbau

Docufy ist ein Cargo-Workspace mit Schichten, die nur nach unten abhängen:

| Schicht | Crate | Aufgabe |
|---|---|---|
| L0 | `geom`, `ai`, `diag` | Einheiten und Geometrie; KI-Anbieter, lokale Modelle und Warnungen; Diagnose |
| L1 | `doc`, `fonts`, `proof` | Dokumentmodell, Schriften, Rechtschreibung |
| L2 | `layout`, `docx`, `formats` | Umbruch und Seitenaufteilung, OOXML, weitere Formate |
| L3 | `render`, `pdf` | Rasterung, PDF-Export |
| L4 | `engine` | Sitzung, Rückgängig, Befehlsregister |
| L5 | `mcp` | MCP-Server |
| L6 | `ui-egui` | Oberfläche (egui) |

Der PDF-Teil liegt in `pdf/` (die Crates von PdfCraft). Die Apps liegen unter `apps/`: `docufy`
(Desktop), `docufy-cli`, `docufy-web` und `docufy-ai-host`, der Hilfsprozess für lokale
KI-Modelle – ein Absturz der KI-Bibliothek kann so nie Ihre Dokumente mitreißen. Er ist die
einzige Stelle mit `unsafe`-Code (die Anbindung an ONNX Runtime GenAI).

Vor jedem Commit: `cargo xtask ci` (Formatierung, Clippy, Tests, Asset- und Schichtenprüfung, WASM).

## Lizenz und Herkunft

Docufy steht wie WordCraft wahlweise unter der [MIT-Lizenz](LICENSE-MIT) oder der
[Apache-Lizenz 2.0](LICENSE-APACHE). Der Großteil des Codes stammt von den WordCraft-Mitwirkenden
und dem ArtCraft-Team; die erforderlichen Hinweise stehen in [NOTICE](NOTICE), die Herkunft jeder
mitgelieferten Datei in [ATTRIBUTION.md](ATTRIBUTION.md). In der App heruntergeladene KI-Modelle
haben eigene Lizenzen, die neben jedem Modell verlinkt sind.

Docufy ist kein Produkt des ArtCraft-Teams und wird von ihm weder unterstützt noch empfohlen.
Name und Logos von ArtCraft sind Marken des ArtCraft-Teams und nicht Teil von Docufy.
