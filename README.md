<p align="center">
  <img src="Resources/Branding/wordmark.png" alt="Laut" width="480">
</p>

# Laut

**Local transcription for your Mac. Your audio stays yours.**

Laut is a native SwiftUI app for Apple Silicon. Import a recording, transcribe it locally, then identify and correct speakers without running speech recognition again. No account, cloud transcription, analytics, or Google connection.

**Status: early developer alpha.** This is working source, not a notarized consumer release. File transcription and offline speaker analysis have been exercised on an M1 with 16 GB RAM. Microphone capture, system-audio capture and cross-app insertion still need interactive permission/device testing. The UI currently uses German labels.

## What is implemented

- Audio/video import through AVFoundation, including multiple files and drag-and-drop. Imports validate the audio track, copy off the UI thread with progress and cancellation, and remove incomplete copies.
- YouTube audio and direct HTTPS audio-file imports, optional automatic local transcription, and a persisted sequential job list. Finished downloads survive a later transcription failure; unfinished jobs pause after restart until explicitly resumed.
- Source URL, original title, publisher, publication date and reported audio language are retained when supplied by the source. New imported audio has a readable `YYYY-MM-DD TITLE.ext` filename and can be revealed in Finder or opened externally.
- Phonon-2, multilingual Parakeet v3 and a Qwen3-ASR adapter; explicit model downloads or compatible local model directories.
- A persistent, network-blocked model process: a loaded model stays available between transcriptions. Switch models or explicitly unload to reclaim memory.
- Local speaker diarization through FluidAudio/Core ML, enabled by default directly after transcription. Turn **Sprecher automatisch erkennen** off to skip the extra analysis. Rename speakers, reassign segments, split at a text/time boundary, merge segments or speakers. Manual assignments and named speakers are protected from reanalysis.
- Speaker assignment searches the relevant time interval instead of comparing every word against the whole meeting. Creating a new transcription version copies the source off the UI thread, with progress and cancellation, while preserving the existing transcript.
- Editable transcripts, original transcript retention, searchable local history and quick notes. Up to 30 document edits can be undone/redone, including speaker corrections and segment splits, even after restarting.
- A default reading view groups neighboring segments into longer paragraphs per speaker, including existing transcripts, without changing their stored edits or history. Switch to **Bearbeiten** for corrections, manual speaker assignments and splits.
- A locally generated, cached waveform supports click/drag seeking and up to 16× zoom. Click a word in the reading view to seek to its timestamp. Edited text and models without word timestamps fall back to the original segment start. Previous/next segment, five-second jumps, playback speed, transcript/speaker search and playback highlighting remain available.
- Library search with local SQLite FTS5, optional multilingual E5 semantic retrieval and hybrid rank fusion. Filter by recording, date or speaker; snippets distinguish keyword/semantic hits and link transcript results to the source segment's audio timestamp. Notes and refined text open in their respective tabs.
- A model manager separates speech, speaker, text and search models. Hugging Face downloads show byte progress, resume cached partial files, pin one revision per download and check file sizes/LFS SHA-256 values. External model folders are detached rather than deleted; deleting a managed download requires confirmation. Phonon and FluidAudio downloads retain their provider-specific preparation paths and do not expose byte progress.
- Optional local text templates (summary, minutes, tasks, email) and experimental answers grounded in up to eight library search results with clickable source buttons. These need an installed MLX-LM text model; answer quality has not yet been validated end to end.
- Personal vocabulary: Phonon/Qwen hotwords and explicit spelling replacements.
- Optional local MLX-LM text editing with custom instructions; results remain separate from the original.
- Microphone recording and meeting recording with microphone + system audio. Transcription runs **after stopping**; live partial meeting transcripts are not implemented yet.
- Global dictation toggle: **Control–Option–Space**. Edit selected text with the local LLM: **Control–Option–R**.
- TXT, Markdown, SRT and JSON export. Delete audio separately while keeping the text.
- An automatic Markdown archive with `YYYY-MM-DD TITLE.md` filenames, readable speaker paragraphs, timestamps, notes and any saved text-model result. Configure its folder or disable it in Settings.

## Build and run

Requirements: Apple Silicon, macOS 14+, Swift 6.2+ command-line tools, Python 3.12 and [uv](https://docs.astral.sh/uv/). The setup command can obtain Python 3.12 through uv if needed. Allow several GB for the Python runtime and optional model downloads.

```sh
git clone https://github.com/seutech/laut-app.git
cd laut-app
./scripts/setup-runtime.sh
./scripts/build-app.sh
open dist/Laut.app
```

In **Modelle**, download a speech model. New installations select Parakeet v3 by default for German conversations; existing model selections are preserved. Downloads connect to Hugging Face/GitHub as needed; no login is required. Laut automatically loads the selected installed model at startup and after a model change, then warms its inference kernels using a short, generated test signal (never microphone or user audio). Wait for **Phonon-2 bereit** (or the selected model's name) before recording. Initial preparation can still take tens of seconds; it happens before the first dictation instead of after it. **Gewähltes Modell vorladen** retries preparation after cancellation or manual unloading. Import a file and click **Transkribieren**. Install the speaker models once under **Modelle**; speaker analysis then runs automatically after transcription. You can also run it later from the **Sprecher** tab.

Automatic speaker detection defaults to on for new and existing installations. The switch below the transcription controls (also in Settings) persists across restarts. The transcript is saved before speaker analysis starts: if that stage fails, is cancelled or lacks models, the text remains available with an explanatory message. Missing speaker models are never downloaded silently. The speech model stays warm during the sequential speaker stage. Speaker-analysis time is shown separately.

New transcripts show total processing time, model loading, audio preparation and speech recognition separately. Older recordings retain their original total time; missing timing breakdowns are not estimated retroactively. Cancelling preparation or explicitly unloading the model prevents automatic retries until a new model is selected or preparation is requested manually.

Use **Rückgängig** / **Wiederherstellen** or **Command–Option–Z** / **Command–Option–Shift–Z** for saved document edits. Native text-field undo remains on Command–Z. Consecutive keystrokes in the same field are grouped while less than 1.5 seconds apart. The time-jump field accepts seconds, `mm:ss`, or `hh:mm:ss`, including fractional seconds.

The local build is ad-hoc signed. It is not notarized for distribution. The runtime remains in this checkout's `.runtime` folder; moving the app does not bundle Python. If you move the checkout, select its new `.runtime` directory under **Einstellungen** and rebuild the virtual environment if needed. A self-contained installer is future work.

## Audio links and saved jobs

For optional link imports, run `bash scripts/setup-downloads.sh` to install the pinned yt-dlp release with compatible EJS components in a separate `.runtime/download-venv`. Install Deno 2.3+ and FFmpeg/FFprobe separately (for example, `brew install deno ffmpeg`). **Einstellungen → Linkimport** displays discovered executables and allows explicit paths. Discovery checks Laut's runtime first, then common Homebrew paths. No executable is installed automatically when a link is pasted.

Open **Quellen & Aufträge**, paste an individual YouTube video or a direct HTTPS audio-file link, and click **Laden**. Automatic transcription is on by default; turn it off to retain just the imported audio. YouTube watch, short, embed and short-link forms resolve to the same video ID to avoid duplicate jobs. Video links containing a playlist parameter import only that single video. Whole playlists, channels, running livestreams, login/age-restricted sources, Vimeo pages and podcast RSS feeds are not supported in this first version. Public podcast MP3 links are supported. No browser cookies, account credentials, user yt-dlp configuration, third-party plugins or remotely fetched EJS code are used. Some public videos may still fail due to YouTube restrictions; errors appear in the job list and do not trigger a login workaround.

Link imports allow up to **six hours and 2 GiB per source audio file**. The importer chooses an audio-only YouTube format, checks for actual audio/no video, and retains M4A or converts other formats to AAC/M4A. Conversion can be lossy. It checks disk space, bounds retries/timeouts, and verifies duration before publishing a completed download. These are enforced limits, not evidence that six-hour jobs have been validated. Local file import retains its existing behavior and has no new size limit.

Download, library import and transcription run sequentially with saved stages and a preassigned recording ID. **Alle pausieren** stops the active job and pauses waiting jobs. After restarting, open jobs remain paused; **Fortsetzen** reuses a completed download or existing library audio. An interrupted partial download starts again, and interrupted speech recognition starts again from that audio; this is not word-level inference resumption. Transcription uses the model/settings selected when its turn starts. Already saved transcripts are retained; interrupted speaker analysis can be rerun from the speaker controls. Removing a job deletes its remaining download cache, but keeps published library entries. A damaged job ledger is reported and never silently replaced.

Source metadata appears above a transcript and in Markdown/JSON exports. Publication date is distinct from import date; the filename date continues to mean import/recording date. Reported language comes from the provider and is not a guarantee of the original audio language. Editable event dates, reporting periods, multi-document dossiers and full-source research analysis remain future work.

## Models and trade-offs

| Backend | Intended use | Timing support |
| --- | --- | --- |
| [Phonon-2](https://huggingface.co/FermionResearch/Phonon-2) | Very small download; published evaluations focus on English. German works in a short smoke test but included spelling/language errors. Compare against Parakeet for German. | Word timestamps |
| [Parakeet TDT 0.6B v3](https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3) | 25 European languages including German; larger model | Word timestamps reconstructed from aligned subword tokens |
| [Qwen3-ASR 0.6B](https://huggingface.co/mlx-community/Qwen3-ASR-0.6B-8bit) | Alternative multilingual backend; experimental adapter | Chunk timestamps, less precise speaker assignment |
| [FluidAudio diarization](https://github.com/FluidInference/FluidAudio) | Speaker separation, independent of ASR | Speaker turns |
| MLX-LM | Optional text correction/custom instructions; default Qwen3 1.7B 4-bit | Text only |

An arbitrary `.onnx`, `.bin` or `.gguf` file is not interchangeable with these backends. Local imports must be compatible model directories. Downloads are separate from inference; a missing model produces an error rather than a hidden download.

## Library search

Open **Suche** or type in the sidebar. **Volltext** requires no embedding model or running Python worker. It uses accent-insensitive prefix matching and requires all entered words; punctuation is treated as a separator, not SQL/FTS syntax. This is not an exact-phrase/operator query language.

In **Modelle → Suche**, explicitly download Multilingual E5 Small (about 495 MB including tokenizer) or Base (about 1.1 GB). Choose **Bedeutung** or **Hybrid** in Search. Small is the tested starting point for an M1. A completed model already in Laut's cache is discovered without downloading. Use **Index neu aufbauen** to regenerate a damaged or outdated derived index without changing recordings, notes or transcripts. The first index/model load takes time; keyword hits stay available during preparation. Only changed sections need new embeddings. Titles remain searchable as metadata but do not get standalone semantic vectors. When a long transcript segment becomes several search passages, each passage uses its first available word timestamp; edited text without trustworthy alignment falls back to the segment start. New model snapshots use separate vector namespaces; interrupted indexing resumes from completed batches, and incomplete semantic results are not presented as a complete index.

Embedding inference uses a separate network-blocked worker, CPU execution with two Torch threads, bounded batches and normalized E5 vectors. It pauses during recording/transcription/other model operations and unloads after 45 seconds of inactivity or when switching to full-text search. Searches query the existing index without rebuilding it on every keystroke. Semantic similarity indicates relevance, not factual correctness or a calibrated confidence percentage. This first implementation scans locally stored vectors; very large libraries still need performance evaluation.

Text templates use the selected template's instructions. **Eigene Anweisungen** uses your settings unchanged. The experimental answer button gives the local LLM only the first eight current search hits; it cannot inspect the entire library at once. It asks for numbered citations, and the provided sources can be opened independently. Verify generated assertions against those sources. Long text processing retains the existing 24,000-character limit.

The feature design was informed by publicly described workflows in other transcription apps. No implementation code from Wisp, Detto, Humla or TypeWhisper was copied or vendored.

## Privacy and storage

- The automatic Markdown archive defaults to `~/Documents/Laut/Transkripte/`. The filename date is the recording/import date in Laut, not the source file's original creation date. Existing nonempty transcripts and notes are exported too. Corrections in Laut refresh the archive after a short debounce; title changes rename Laut's unchanged copy. Duplicate names receive numeric suffixes. If an exported file was edited externally, that file is retained and a new numbered copy receives Laut's update. External Markdown edits are not imported into the app. Deleting an entry in Laut does not delete its archive copy; changing the archive folder leaves old copies in place. The hidden `.laut-archive.json` tracks ownership and hashes; damaged bookkeeping is reported rather than ignored. The archive contains text, not source audio or full edit history, so it is not a complete library backup. Laut does not upload it; choose a folder outside any OS or third-party cloud synchronization if those copies must stay exclusively on the Mac.
- The derived `search-v1.sqlite` index (and SQLite journal files) stores local text snippets and optional embeddings beside the library. It is not encrypted separately. Deletions/edits remove or invalidate indexed content; the original recording JSON remains authoritative.
- Recordings, transcripts, vocabulary and settings live in `~/Library/Application Support/Laut/`.
- Link imports connect only when explicitly started or resumed; the provider/CDN sees ordinary request metadata including the IP address. Transcription still runs with networking blocked. The job ledger (`import-jobs.json`) stores source links/local paths, stages and errors locally. URL query strings may contain access tokens: avoid sharing that ledger. Finished downloads wait in `import-jobs/<job ID>/` until the job completes; unpublished library copies use `.import-staging/`. Removing an incomplete job clears its download folder; retry replaces that job's partial staging copy. These files are not a separate backup.
- Each recording retains local edit history and one previous valid JSON save. If the current file cannot be read, Laut tries that backup and reports the recovery. Existing damaged data is preserved for inspection. This is local recovery, not a separate backup of your audio/library. JSON exports omit edit history; deleting an entry removes its history and backup along with it.
- Waveform peaks are cached locally beside each recording as `waveform-v1.json`, regenerated when the source changes, and removed when its audio is deleted. Waveform generation streams small audio buffers and keeps at most 60,000 peaks.
- Models and the isolated Python runtime live in `.runtime/` by default. Neither belongs in Git.
- Python inference runs under a macOS sandbox that denies all network operations. Communication uses anonymous pipes, with no HTTP service or listening port.
- FluidAudio runs in offline mode except during an explicit model download. It does not receive or upload transcripts to a server.
- No app telemetry or crash upload. No account integration. Public model downloads reveal the ordinary request metadata (such as IP address) to their hosting service.
- Cross-app insertion uses macOS Accessibility, not the clipboard. Some apps do not support this; the result then remains in Laut. Dictation audio is deleted after successful transcription by default; transcripts remain in history.
- Local data is not separately encrypted by Laut. Protect your Mac and use FileVault as appropriate. Deletion is normal filesystem deletion, not a secure erase.
- Recording and accessibility permissions are requested only for their respective features. The app never starts recording on launch. Meeting audio is captured locally, including other applications' audible output.

## Verification

There is currently no hard file-size or duration limit in the local audio/video importer, but hour-long meetings and large video files have not been validated. Import retains a copy of the source file; processing also needs a temporary 16 kHz mono 16-bit WAV (approximately 115 MB per audio hour), model memory and derived data. Multiple files are transcribed sequentially. Text-model input is limited to fewer than 24,000 characters and output to 4,096 tokens; long-document summarization in multiple passes is not implemented yet.

The checks are plain Swift executables, so they also work with Command Line Tools installations without XCTest:

```sh
swift run LautCoreChecks
python3 -m unittest discover -s Tests/Python -v
swift run LautDiagnostics /path/to/test-audio.wav
swift run LautDiagnostics --download-speakers  # explicit download
swift run LautDiagnostics /path/to/test-audio.wav --diarize
swift run LautDiagnostics /path/to/synthetic-fixture.wav --workflow
swift run LautDiagnostics /path/to/recording.wav --workflow --engine parakeet --output .runtime/evaluations/parakeet-run-1
swift run LautDiagnostics /path/to/recording.wav --workflow --no-speakers
swift run LautDiagnostics --audio-interface-checks
```

The ASR diagnostic performs a cold and a warm run. Only synthetic test speech has been used in the initial smoke tests; no personal recordings are included. Initial M1 measurement: 11.09 seconds of German synthetic speech took 22.76 seconds with Phonon including the first load, then 0.27 seconds with the same model already loaded. Parakeet v3 transcribed that clip correctly in 13.58 seconds cold and 1.42 seconds warm while a build was running. These are smoke tests, not a controlled speed comparison, representative meeting benchmark or accuracy guarantee. Offline diarization separated a 46-second alternating two-voice synthetic clip into two speakers and four turns. Both ASR engines and diarization were exercised with networking denied by macOS; Qwen and text generation still need end-to-end validation.

The 22 core checks cover editing/word retention, manual speaker protection, old document compatibility, persistent undo/redo and branching, bounded history, corrupted/missing-file recovery, save failure preservation, paragraph grouping, Unicode word links, timestamp navigation, migration of the default-on speaker setting, and dictionary replacements across speaker changes. Indexed speaker assignment is checked against exhaustive overlap scoring (including nested/overlapping turns, unsorted inputs and ties) and a 30,000-word / 10,000-turn timeline. Nine audio checks use a generated tone to verify waveform timing, caching, source changes, corrupt-cache recovery and cancellation; they also check skipping disabled speaker analysis, missing model handling and larger ASR paragraphs. These checks do not simulate mouse interaction with SwiftUI.

The `--workflow` diagnostic uses a temporary library to check import, invalid-file rejection, cancellation cleanup, ASR, speaker analysis, manual corrections, reopen, undo/redo, export and creating a separate new transcription version. Choose `--engine phonon`, `--engine parakeet` or `--engine qwen`; it requires that engine's installed model and, unless `--no-speakers` is supplied, the speaker models. `--model /path/to/model` overrides model discovery. It never writes to the user's library or downloads models automatically. `--output` writes TXT, SRT, JSON and a timing report to a new local directory; existing output directories are refused. Exported transcripts contain the actual automatic result, not the temporary correction used by the check. Keep private evaluations in the Git-ignored `.runtime/evaluations/` folder. Transcript text is not printed to the console unless `--print-transcript` is explicitly supplied for the basic ASR diagnostic. Timing reports do not measure transcription accuracy; that needs a reference transcript or human review.

For 0.1.2, this complete workflow passed with networking denied on both the six-minute fixture and the 46-second two-voice fixture. With Phonon prepared, ASR took 7.71 seconds and 0.99 seconds respectively, excluding startup preparation and speaker analysis. The long file retained end coverage at 362.53 seconds; the two-voice file produced two speakers and four turns. These synthetic diagnostics do not replace interactive testing of the playback/editor UI or tests with real conversations.

For 0.1.4, the expanded workflow also passed on the same six-minute synthetic fixture with both Phonon and Parakeet, including creation of a separate new version. Phonon preparation/ASR took 23.61/9.49 seconds; Parakeet took 11.89/15.38 seconds. Speaker analysis was measured separately. A core stress check assigned 30,000 timed words across 10,000 turns in 0.15 seconds in a debug build; this measures timestamp matching, not neural speaker detection. Real-recording quality and interactive UI checks remain outstanding.

For 0.1.5, the two-voice workflow passed both with automatic speaker analysis on (two speakers, four reading paragraphs) and off (no speaker analysis, two reading paragraphs). The ASR worker remained loaded during speaker analysis. Waveform tests caught and fixed stale filesystem metadata when regenerating peaks after a source change. UI click/drag behavior still needs an interactive macOS check.

A separate synthetic six-minute file (362.59 seconds) produced 12 ASR chunks and 750 timed words in 27.79 seconds including loading. Its last word reached 362.53 seconds and the backend reported no truncation. This exercises chunking and end-of-file coverage, not realistic conversational accuracy.

With startup preparation enabled, a three-second synthetic speech clip took 0.20 seconds on the first ASR request after readiness and 0.11 seconds on the second. Preparation itself took 24.95 seconds before recording; this cost has been moved to startup, not eliminated. Both requests reported zero model reload time. Reproduce with `swift run LautDiagnostics /path/to/test.wav --preload`. The warmup uses a generated non-silent signal because Phonon's silence gate otherwise skips inference.

Before a stable release: real-world long-meeting tests, overlapping speakers, permission recovery, cancellation/device interruptions, cross-app caret handling, UI verification, English localization, a bundled runtime, signed/notarized releases, and an opt-in live transcript view.

## Architecture

- `LautCore`: recording/library formats, editing rules, vocabulary and exports.
- `LautAudio`: streaming AVFoundation conversion, process isolation, model adapters, meeting capture and speaker analysis.
- `Laut`: native SwiftUI library/editor/settings and global shortcuts.
- `Resources/mlx_worker.py`: serial JSON-lines inference worker. One model is retained at a time; model loading uses local directories only. Fermion's internal adapter is pinned to its tested version.
- `Resources/Branding`: selected speech-bubble logo, app icon and monochrome menu-bar mark. `scripts/build-icons.sh` packages the PNG artwork as a macOS `.icns` file using the system tools; the app build includes it automatically. Imagegen prompts are retained in `generation.json`.

Please use synthetic/redacted fixtures in issues and pull requests. Do not upload private recordings to GitHub.

## License

Laut's own code is MIT-licensed. Dependencies and model weights retain their own licenses; see [THIRD_PARTY.md](THIRD_PARTY.md). Model weights are not distributed with this repository.

For 0.1.6, core search checks cover FTS matching, filters, model-specific vector caches, hybrid ranking, reopen, stale-result rejection, deletion, Unicode chunking and safe model removal. Six Python checks cover the existing worker plus pinned download revisions, format filtering, progress and checksum rejection. An actual E5 Small test with networking denied matched all three synthetic German paraphrases. The Swift/Python/index integration retained the 42-second audio target, then successfully cancelled and restarted embedding inference. Two passages took about 6.9 seconds including a cold worker/model load; a warm query took about 0.025 seconds on this machine. These are tiny synthetic fixtures, not a large-library benchmark. E5 Base and generated library answers still need model-level validation; the new SwiftUI controls still need interactive testing.

```sh
swift run LautDiagnostics --search-checks --model /absolute/path/to/multilingual-e5-small
.runtime/venv/bin/python -m unittest discover -s Tests/Python -p 'test_*.py'
.runtime/venv/bin/python Tests/Python/check_embeddings.py /absolute/path/to/multilingual-e5-small
```

### 0.1.7 stability pass

Search requests now invalidate answer eligibility immediately, including the typing debounce interval. Results and answers cannot cross query/filter/model/library revisions. SQLite read failures are surfaced instead of becoming silently partial result sets, a failed index open closes its handle, and the derived index can be explicitly reset. Long passages retain segment identity and use available word timestamps for audio navigation. Old cached passage JSON remains readable and is refreshed on synchronization.

Three newly generated German fixtures completed the offline import → ASR → optional speakers → correction → save/reopen → undo/redo → export checks. Phonon ASR took 0.20 seconds for a 10-second clip, 0.93 seconds for a 55.93-second two-voice clip, and 4.08 seconds for a 256.20-second repeated conversation. Cold preparation was 11.5–12.9 seconds; speaker analysis took 2.2–2.5 seconds on the two longer fixtures and identified two speakers. These timings exclude one another and are not real-meeting measurements.

**A technical success did not imply a complete transcript.** Against the known 106-word reference for the two-voice fixture, Phonon returned only 58 words and omitted several sentences from the second voice. Its raw model text already lacked those sentences; Laut's word-timestamp rendering did not cause the loss. Case/punctuation-normalized word error rate on this single synthetic fixture was 47.2% for Phonon and 1.9% for Parakeet (107 output words, one incorrectly rendered term). Parakeet ASR took 1.69 seconds, with 3.96 seconds of preparation. This narrow result motivates the German model recommendation; it is not a representative accuracy benchmark. No personal recordings were used. Real conversation evaluation and interactive UI checks remain outstanding.

A local reference comparison is available without printing transcript contents:

```sh
.runtime/venv/bin/python Tests/Python/check_transcript_quality.py --reference /path/to/reference.txt --transcript /path/to/transcript.txt --max-wer 0.1
```

The threshold is optional and must be chosen for the particular evaluation. A saved transcript and successful pipeline checks alone are insufficient evidence of recognition quality.

### 0.1.8 Markdown archive

Core archive checks cover deterministic dates, filename sanitization and UTF-8 length, metadata and content, idempotent synchronization, transcript updates, title changes, duplicate titles, externally edited files, symlinks, retained deleted entries and damaged manifests. These filesystem checks and a native app build passed; the new Settings controls still need interactive verification.

### 0.1.9 Audio links and saved jobs

Core checks now cover supported/rejected URL forms, canonical YouTube identities, durable job IDs/stages, pause-on-restart, damaged-ledger preservation and backward-compatible source metadata. The 19 Python checks include audio/video validation, private-address rejection, incomplete checkpoints, truncated audio, live-source rejection, timeout cleanup and terminating a helper together with its downloader child. Existing core/search/archive and nine native audio interface checks passed.

An actual public YouTube audio-only import of Blender's *Big Buck Bunny* completed with 596.52 seconds of M4A audio (9,648,640 bytes), title, publisher and publication date. Download and validation took 75.92 seconds on this connection; this is network time, not speech recognition. A direct MP3 fixture from Mozilla also passed (2.07 seconds of audio). Native Swift/Python integration checks exercised checkpoint reuse, replacement of an interrupted staging copy, stable recording IDs, duplicate protection and source metadata after library reopen. An unrelated test server returned HTML and was rejected without importing it.

The existing synthetic German two-speaker workflow also passed through import, Parakeet, speaker analysis, manual corrections, save/reopen, undo/redo and export. For 55.93 seconds of audio, model preparation took 8.33 seconds and ASR 1.99 seconds; the speaker stage found two speakers. These are narrow synthetic/integration checks. Interactive SwiftUI job controls, a forced app restart during an actual download, long recordings near the new limits, and broad YouTube compatibility still need validation. The YouTube fixture was not used to evaluate ASR quality.

To run a link integration check against a public source (this explicitly accesses the network and keeps downloaded audio in the output directory):

```sh
swift run LautDiagnostics --link-import 'https://www.youtube.com/watch?v=YE7VzlLtp-4' --output .runtime/evaluations/link-import-new
```
