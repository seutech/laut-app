# Changelog

All notable public changes to Laut are documented here.

## 0.1.0-alpha.1 — 2026-10-26

First public Rust desktop alpha.

### Included

- Cross-platform Rust/egui desktop application and command-line interface.
- Local CPU transcription with Whisper.cpp and resumable five-minute checkpoints.
- Local speaker diarization with reviewable mappings and protected manual corrections.
- File, video and public YouTube audio import through a persistent queue.
- Readable transcript workspace with search, chapters, full-text copy, waveform and pitch-preserving playback speed.
- PDF text/OCR import, reviewed slide references, reversible video-insert exclusions and conference research export.
- Literal and multilingual semantic library search plus local source-linked text analysis.
- Microphone and meeting capture adapters for macOS and Windows; Linux microphone support.
- Project copies, verified backups, recovery drafts and reversible library trash.
- German and English interface.

### Known limits

- Release archives are unsigned and not notarized.
- Windows and Linux builds need broader real-device testing.
- macOS meeting system-audio permission/hardware validation remains open.
- A complete seven-hour end-to-end benchmark and forced-failure tests remain open.

See [portable/VALIDATION.md](portable/VALIDATION.md) for the complete evidence and limitations.
