# Changelog

All notable public changes to Laut are documented here.

## 0.2.0 — 2026-10-26

Second public Rust desktop alpha.

### Included

- New teal waveform identity used consistently for the app icon, in-app brand mark and GitHub wordmark.
- Cross-platform Rust/egui desktop application and command-line interface.
- Local CPU transcription with Whisper.cpp and resumable five-minute checkpoints.
- Local speaker diarization with reviewable mappings and protected manual corrections.
- File, video and public-link audio import through a persistent queue, including individual YouTube/Vimeo videos, compatible podcast or media-library pages and direct media URLs.
- Readable transcript workspace with search, chapters, full-text copy, waveform and pitch-preserving playback speed.
- PDF text/OCR import, reviewed slide references, reversible video-insert exclusions and conference research export.
- Literal and multilingual semantic library search plus local source-linked text analysis.
- Search-result evidence labels and literal-term highlighting, conservative handling of weak similarities, duplicate collapsing and return navigation after opening a source.
- Local event/topic collections that filter the library and provide one shared scope for search, questions and resumable full analysis, plus task-based model setup with grouped installs and readiness checks.
- Microphone and meeting capture adapters for macOS and Windows; Linux microphone support.
- Project copies, verified backups, recovery drafts and reversible library trash.
- German and English interface.

### Interface

- The Laut wordmark and application header no longer use a tagline.
- An active recording remains controllable from a compact bottom player while navigating Sources, Search, Models or Settings; the bar provides play/pause, five-second skips, seeking, original time and a return to the transcript.
- A more compact transcript workspace keeps import, source metadata, waveform, playback and reading text visible with less vertical chrome.
- Event/topic collections appear as folder rows in the sidebar, filter the recording list and carry the same scope into Search & AI.
- Guided model cards explain the required local components for transcription, link import, speaker separation, semantic search and local analysis.

### Known limits

- Release archives are unsigned and not notarized.
- Windows and Linux builds need broader real-device testing.
- macOS meeting system-audio permission/hardware validation remains open.
- A complete seven-hour end-to-end benchmark and forced-failure tests remain open.

See [portable/VALIDATION.md](portable/VALIDATION.md) for the complete evidence and limitations.
