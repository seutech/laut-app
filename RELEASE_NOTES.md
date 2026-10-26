# Laut 0.1.0 alpha

This is the first public build of the cross-platform Rust application.

Laut transcribes files and public YouTube audio locally, separates speakers, provides a readable and editable transcript workspace, links reviewed PDF slides and supports local search and source-linked AI analysis. No account or cloud transcription is required.

## Before using it for important work

- Keep the original recording and a separate library backup.
- Install FFmpeg and FFprobe, then select them under **Models → Local programs** if Laut does not find them.
- Install a Whisper model explicitly from **Models**.
- Review names, speaker assignments, numbers, OCR, slide matches and generated text.
- The packages are unsigned and this remains an alpha release.

macOS Apple Silicon is the main tested platform. Windows x64 and Linux x64 packages pass the shared automated suite, but need broader real-machine and audio-device testing. Meeting system-audio validation on macOS remains open.

See the [README](https://github.com/seutech/laut-app#readme), [validation record](https://github.com/seutech/laut-app/blob/main/portable/VALIDATION.md) and [changelog](https://github.com/seutech/laut-app/blob/main/CHANGELOG.md) before reporting results.
