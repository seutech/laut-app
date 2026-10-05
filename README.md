# Laut

**Local transcription for your Mac. Your audio stays yours.**

Laut is a native SwiftUI app for Apple Silicon. Import a recording, transcribe it locally, then identify and correct speakers without running speech recognition again. No account, cloud transcription, analytics, or Google connection.

**Status: early developer alpha.** This is working source, not a notarized consumer release. File transcription and offline speaker analysis have been exercised on an M1 with 16 GB RAM. Microphone capture, system-audio capture and cross-app insertion still need interactive permission/device testing. The UI currently uses German labels.

## What is implemented

- Audio/video import through AVFoundation, including multiple files and drag-and-drop.
- Phonon-2, multilingual Parakeet v3 and a Qwen3-ASR adapter; explicit model downloads or compatible local model directories.
- A persistent, network-blocked model process: a loaded model stays available between transcriptions. Switch models or explicitly unload to reclaim memory.
- Separate local speaker diarization through FluidAudio/Core ML. Rename speakers, reassign segments, split at a text/time boundary, merge segments or speakers. Manual assignments and named speakers are protected from reanalysis.
- Editable transcripts, original transcript retention, playback, searchable local history and quick notes.
- Personal vocabulary: Phonon/Qwen hotwords and explicit spelling replacements.
- Optional local MLX-LM text editing with custom instructions; results remain separate from the original.
- Microphone recording and meeting recording with microphone + system audio. Transcription runs **after stopping**; live partial meeting transcripts are not implemented yet.
- Global dictation toggle: **Control–Option–Space**. Edit selected text with the local LLM: **Control–Option–R**.
- TXT, Markdown, SRT and JSON export. Delete audio separately while keeping the text.

## Build and run

Requirements: Apple Silicon, macOS 14+, Swift 6.2+ command-line tools, Python 3.12 and [uv](https://docs.astral.sh/uv/). The setup command can obtain Python 3.12 through uv if needed. Allow several GB for the Python runtime and optional model downloads.

```sh
git clone https://github.com/seutech/laut-app.git
cd laut-app
./scripts/setup-runtime.sh
./scripts/build-app.sh
open dist/Laut.app
```

In **Modelle**, download a speech model. Downloads connect to Hugging Face/GitHub as needed; no login is required. Laut automatically loads the selected installed model at startup and after a model change, then warms its inference kernels using a short, generated test signal (never microphone or user audio). Wait for **Phonon-2 bereit** (or the selected model's name) before recording. Initial preparation can still take tens of seconds; it happens before the first dictation instead of after it. **Gewähltes Modell vorladen** retries preparation after cancellation or manual unloading. Import a file and click **Transkribieren**. For speaker labels, install the speaker models, open the recording's **Sprecher** tab and run **Sprecher erkennen**.

New transcripts show total processing time, model loading, audio preparation and speech recognition separately. Older recordings retain their original total time; missing timing breakdowns are not estimated retroactively. Cancelling preparation or explicitly unloading the model prevents automatic retries until a new model is selected or preparation is requested manually.

The local build is ad-hoc signed. It is not notarized for distribution. The runtime remains in this checkout's `.runtime` folder; moving the app does not bundle Python. If you move the checkout, select its new `.runtime` directory under **Einstellungen** and rebuild the virtual environment if needed. A self-contained installer is future work.

## Models and trade-offs

| Backend | Intended use | Timing support |
| --- | --- | --- |
| [Phonon-2](https://huggingface.co/FermionResearch/Phonon-2) | Very small download; published evaluations focus on English. German works in a short smoke test but included spelling/language errors. Compare against Parakeet for German. | Word timestamps |
| [Parakeet TDT 0.6B v3](https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3) | 25 European languages including German; larger model | Word timestamps reconstructed from aligned subword tokens |
| [Qwen3-ASR 0.6B](https://huggingface.co/mlx-community/Qwen3-ASR-0.6B-8bit) | Alternative multilingual backend; experimental adapter | Chunk timestamps, less precise speaker assignment |
| [FluidAudio diarization](https://github.com/FluidInference/FluidAudio) | Speaker separation, independent of ASR | Speaker turns |
| MLX-LM | Optional text correction/custom instructions; default Qwen3 1.7B 4-bit | Text only |

An arbitrary `.onnx`, `.bin` or `.gguf` file is not interchangeable with these backends. Local imports must be compatible model directories. Downloads are separate from inference; a missing model produces an error rather than a hidden download.

## Privacy and storage

- Recordings, transcripts, vocabulary and settings live in `~/Library/Application Support/Laut/`.
- Models and the isolated Python runtime live in `.runtime/` by default. Neither belongs in Git.
- Python inference runs under a macOS sandbox that denies all network operations. Communication uses anonymous pipes, with no HTTP service or listening port.
- FluidAudio runs in offline mode except during an explicit model download. It does not receive or upload transcripts to a server.
- No app telemetry or crash upload. No account integration. Public model downloads reveal the ordinary request metadata (such as IP address) to their hosting service.
- Cross-app insertion uses macOS Accessibility, not the clipboard. Some apps do not support this; the result then remains in Laut. Dictation audio is deleted after successful transcription by default; transcripts remain in history.
- Local data is not separately encrypted by Laut. Protect your Mac and use FileVault as appropriate. Deletion is normal filesystem deletion, not a secure erase.
- Recording and accessibility permissions are requested only for their respective features. The app never starts recording on launch. Meeting audio is captured locally, including other applications' audible output.

## Verification

The checks are plain Swift executables, so they also work with Command Line Tools installations without XCTest:

```sh
swift run LautCoreChecks
python3 -m unittest discover -s Tests/Python -v
swift run LautDiagnostics /path/to/test-audio.wav
swift run LautDiagnostics --download-speakers  # explicit download
swift run LautDiagnostics /path/to/test-audio.wav --diarize
```

The ASR diagnostic performs a cold and a warm run. Only synthetic test speech has been used in the initial smoke tests; no personal recordings are included. Initial M1 measurement: 11.09 seconds of German synthetic speech took 22.76 seconds with Phonon including the first load, then 0.27 seconds with the same model already loaded. Parakeet v3 transcribed that clip correctly in 13.58 seconds cold and 1.42 seconds warm while a build was running. These are smoke tests, not a controlled speed comparison, representative meeting benchmark or accuracy guarantee. Offline diarization separated a 46-second alternating two-voice synthetic clip into two speakers and four turns. Both ASR engines and diarization were exercised with networking denied by macOS; Qwen and text generation still need end-to-end validation.

A separate synthetic six-minute file (362.59 seconds) produced 12 ASR chunks and 750 timed words in 27.79 seconds including loading. Its last word reached 362.53 seconds and the backend reported no truncation. This exercises chunking and end-of-file coverage, not realistic conversational accuracy.

With startup preparation enabled, a three-second synthetic speech clip took 0.20 seconds on the first ASR request after readiness and 0.11 seconds on the second. Preparation itself took 24.95 seconds before recording; this cost has been moved to startup, not eliminated. Both requests reported zero model reload time. Reproduce with `swift run LautDiagnostics /path/to/test.wav --preload`. The warmup uses a generated non-silent signal because Phonon's silence gate otherwise skips inference.

Before a stable release: real-world long-meeting tests, overlapping speakers, permission recovery, cancellation/device interruptions, cross-app caret handling, UI verification, English localization, a bundled runtime, signed/notarized releases, and an opt-in live transcript view.

## Architecture

- `LautCore`: recording/library formats, editing rules, vocabulary and exports.
- `LautAudio`: streaming AVFoundation conversion, process isolation, model adapters, meeting capture and speaker analysis.
- `Laut`: native SwiftUI library/editor/settings and global shortcuts.
- `Resources/mlx_worker.py`: serial JSON-lines inference worker. One model is retained at a time; model loading uses local directories only. Fermion's internal adapter is pinned to its tested version.

Please use synthetic/redacted fixtures in issues and pull requests. Do not upload private recordings to GitHub.

## License

Laut's own code is MIT-licensed. Dependencies and model weights retain their own licenses; see [THIRD_PARTY.md](THIRD_PARTY.md). Model weights are not distributed with this repository.
