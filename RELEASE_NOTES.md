# Laut 0.2.0

This is the second public alpha of the cross-platform Rust application.

Laut transcribes files and public media links locally, separates speakers, provides a readable and editable transcript workspace, links reviewed PDF slides and supports local search and source-linked AI analysis. No account or cloud transcription is required. Optional external text analysis can use the user's own OpenAI/Responses-compatible API access after an exact source preview; credentials stay in the operating-system vault or memory for the current session.

Version 0.2.0 adds event/topic collections, guided local-model setup, stronger evidence-oriented search, a more compact transcript workspace and a persistent mini player while navigating Sources, Search, Models or Settings.

## Before using it for important work

- Keep the original recording and a separate library backup.
- Install FFmpeg and FFprobe, then select them under **Models → Local programs** if Laut does not find them.
- Install a Whisper model explicitly from **Models**.
- Review names, speaker assignments, numbers, OCR, slide matches and generated text.
- The packages are unsigned and this remains an alpha release.

macOS Apple Silicon is the main tested platform. Windows x64 and Linux x64 packages pass the shared automated suite, but need broader real-machine and audio-device testing. Meeting system-audio validation on macOS remains open.

See the [README](https://github.com/seutech/laut-app#readme), [validation record](https://github.com/seutech/laut-app/blob/main/portable/VALIDATION.md) and [changelog](https://github.com/seutech/laut-app/blob/main/CHANGELOG.md) before reporting results.
