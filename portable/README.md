# Laut Rust desktop

The current Laut desktop app and shared command engine for macOS, Windows and Linux. The application is written in Rust with egui and keeps its own library and preferences. It can import compatible projects from the earlier Swift app, but does not automatically adopt that app's live library.

This is a public alpha. macOS is the primary exercised desktop; Windows CPU support is the first portability target and Linux follows. [ROADMAP.md](ROADMAP.md) tracks the complete feature inventory, remaining gaps and acceptance criteria.

## Implemented

- Open an existing `recording.json`, preserving unknown JSON fields, precise numbers, Swift dates, original text, word timestamps, notes, source metadata, conference documents and edit history.
- Reading paragraphs follow the Swift grouping rules. Confirmed exclusions affect the reading/export projection only. Edited boundary segments are retained when precise alignment is unavailable.
- Select/copy text across paragraphs; copy the entire reading version regardless of search. Double-click a word to seek/play. Edited text falls back to the segment start.
- A persistent sidebar with Library, local event/topic collections shown as folders, Sources & jobs, Search & AI, Models, Settings and searchable recording history follows the original Mac app’s navigation. Selecting a collection filters the recording history and makes it the active research scope. Import, model management and research occupy the main workspace. **Settings / Einstellungen** remembers German/English, system/light/dark appearance and transcript text size (14–20 points). The local macOS/Windows UI font is used when available, with bundled fallback fonts; no system font is redistributed. **File / Ablage** contains open, project copy, export and draft recovery. Less frequent PDF, speaker and AI actions are grouped under **Tools / Werkzeuge**.
- The transcript workspace keeps its local waveform, player, view tabs, chapters, search and full-text copy together above the reading text. The waveform supports paused click/drag seeking and 1×–16× zoom/panning; bounded peaks are cached in the local state folder and regenerated when the source audio changes. Speaker/time markers and chapter headings sit beside the copyable reading text. Search replaces the reader with direct local results and returns to the passage after selection.
- The inline player provides five-second skips, previous/next included passage and direct `mm:ss` / `hh:mm:ss` navigation. Once audio has been started, a compact transport remains available while navigating Sources, Search, Models or Settings; it keeps play/pause, five-second skips, seeking, original time and a direct return to the transcript. Pausing does not hide it. The reading view highlights the current passage and optionally scrolls when the passage changes. Navigation preserves paused playback. Space toggles playback and arrow keys skip outside text inputs and dialogs; typing keeps its normal shortcuts. The saved **Tempo / Speed** menu offers 0.5×–2× with preserved pitch and original-media time marks.
- Search text/speakers, navigate existing chapters, correct individual segments, confirm names and assign speakers. Text changes require **Apply text**. Naming a voice protects its attribution without forcing extra paragraph boundaries.
- Swift-compatible delta undo/redo survives project copies (up to 30 revisions and a 64 MiB history budget, always retaining the newest operation). Existing histories can be used if their expected content still matches.
- Play local audio through rodio/Symphonia. Shared conference media references are resolved within their archive. Audio is decoded as a stream, not loaded completely into memory. Device/file loading and seeking run on an audio thread so delayed output cannot freeze the editor.
- Save a **new independent project folder**, copying referenced original audio/PDFs and checking hashes. Filenames incompatible with Windows receive short safe names; original names remain in `portableMediaOriginalNames`, and known media references (including conference history) are updated. Existing destinations are refused. Missing media prevents a project copy; plain-text export remains available. `COPY-INCOMPLETE` identifies a publish interrupted between file moves; such copies are refused on open.
- Local audio/video import, multi-file selection/drop, compact audio copies, a persistent sequential queue, cancellation/resume and automatic publication into the selected portable library.
- Public media-link import through yt-dlp, including individual YouTube/Vimeo videos, compatible podcast or media-library pages, direct media files, completed live archives, multiple links, an optional opening excerpt and source metadata/chapters. Failed sources do not stop later jobs.
- Whisper.cpp transcription on CPU with word timings when compatible token evidence is available; optional vocabulary/names prompt. A bundled CPU worker can be built independently of the UI.
- Explicit model installation/resume/verification/removal with pinned SHA-256, safe archive extraction and a local manifest. External worker/model paths remain supported. Downloads are separate from inference.
- Local CPU speaker analysis through sherpa-onnx, reviewable voice/name mappings, listening examples, protected manual assignments and undoable application without rerunning ASR.
- Remembered German/English interface, library folder, last project and local worker/model settings.
- Edit title/notes, split at a Unicode-safe text boundary and explicit time, merge segments, add/merge speakers, and exact find/replace. Original evidence survives structural edits independently of the bounded undo history. **Apply all inputs** commits pending segment/name/title/note corrections as one undoable operation.
- Export TXT, Markdown, SRT or complete document JSON. JSON exports do not include media; use a project copy or library backup for that.
- Local draft recovery, library-wide literal search and verified directory backups/restoration.
- An independent portable library with project-copy import, a recording list, automatic saving of applied document corrections, a previous valid version, explicit recovery warnings and conflict detection between writers. External projects still use in-memory edits and explicit project copies.

## Playback speed

Use **Tempo / Speed** in the inline player: **0.5×, 0.75×, 1×, 1.25×, 1.5×, 1.75× or 2×**. The choice is remembered across projects and restarts. Pitch is preserved; the time display, transcript highlighting, chapters, waveform, five-second skips and direct jumps always use the **original recording time**. Changing speed retains the current position and play/pause state, with a brief rebuffer while playing. Paused navigation does not open an audio device.

At 1×, playback uses the existing native decoder. Other speeds stream through the selected local **FFmpeg** `atempo` filter on CPU, into a bounded stereo PCM queue. No model, API key, GPU, network request or full converted audio copy is needed. The audio callback never waits on FFmpeg; an underrun shows the spinner and freezes the source clock until audio arrives. Original recordings, transcripts and exports are unchanged. On Windows, select `ffmpeg.exe` under **Models → Local programs**, as for imports. A missing executable produces an explicit error; there is no pitch-shifting fallback.

The optional integration test uses generated stereo tones and the actual local FFmpeg, including nonzero seeks, pitch, timing, cancellation and invalid input; it does not open an audio device:

```sh
LAUT_TEST_FFMPEG=/path/to/ffmpeg cargo test -p laut-portable --locked real_tempo -- --ignored --nocapture
```

In PowerShell, set `$env:LAUT_TEST_FFMPEG = 'C:\path\to\ffmpeg.exe'` first, then run the same `cargo test` command without the environment prefix. Native Windows playback and subjective speech listening remain to be checked; automated pitch tests do not establish identical voice quality for every recording, especially at extreme speeds.

## Microphone and meeting recordings

Validation status: microphone capture, local transcription and playback were exercised on this Mac with a BRIO microphone. The meeting implementation is available, but its macOS hardware test remains blocked by system-audio permission; Windows hardware testing is also pending. Run a short two-track test before using meeting recording for an interview. See `VALIDATION.md` for the observed limits.

Choose **Mikrofon / Microphone** or **Meeting** in the toolbar, enter a title and select the input device (or system default). **Aufnahme starten / Start recording** shows elapsed time and live input levels; **Stoppen & sichern / Stop & save** saves the audio in the selected library. With **Transcribe locally after stopping** enabled, the existing local Whisper queue then adds the transcript to that same entry. A missing model or failed ASR leaves the saved audio available; **Transcribe locally** can start processing later. Optional speaker analysis uses the existing review workflow; voices are not automatically identified as people.

After five seconds without any microphone signal, the recording bar asks you to check the microphone and mute settings. A completely silent track is marked in the saved recording's warnings. If every track is proven silent, Laut retains the recording but skips transcription. This detects digital silence, not intelligibility, background noise or intermittent dropouts.

Microphone capture uses CPAL/CoreAudio on macOS and CPAL/WASAPI on Windows. Meetings additionally record computer audio through ScreenCaptureKit on macOS 13+ or WASAPI loopback on Windows; Windows offers the output-device selector. No special GPU, virtual audio driver, account or API key is required. macOS requires microphone and screen/system-audio permissions, although Laut saves no screen images. Permission names vary with macOS version and the OS may request a restart. Linux microphone support uses CPAL; Linux system-audio recording remains disabled until its adapter is implemented and tested. Windows implementation is type-checked; actual Windows hardware is still unverified.

Use headphones to avoid recording remote voices twice through speakers and microphone. This version does not perform acoustic echo cancellation or suppress notifications from other apps. macOS excludes Laut's own playback; Windows records the selected output device, and Laut pauses/disables its player while recording. A disconnected input stops the capture with an error instead of silently switching microphones. Keep the computer awake: automatic sleep prevention and seamless device switching are not implemented.

Audio is compressed continuously to disk, with bounded callback queues. Each retained track is **24 kHz mono, 16-bit FLAC**. Meetings keep `microphone.flac` and `system.flac`, plus a 96 kbit/s AAC listening/transcription mix; a microphone recording uses its FLAC directly. These are speech-oriented recordings, not archival stereo music masters. Shared timing, gap/overlap counts, device names and SHA-256 hashes are retained under `portableCapture` in `recording.json`. Project copies and library backups include all tracks. FLAC size depends on the signal; the meeting mix adds about 43 MB per hour.

While recording, files and a checkpoint manifest live in `LautPortableState/captures` (`laut-portable-state/captures` on Linux). After a successful verified library copy, redundant working audio is removed. **Sources & jobs → Unfinished recordings → Save partial recording** can salvage decodable tracks after interruption. The result is clearly marked partial and must be checked for completeness; recovery retains the original working files. Empty or undecodable files cannot be recovered. Capture has a 24-hour limit, checks free space and refuses simultaneous publication of an active recording. Closing the window during capture offers Stop & save first; forced quits and system shutdown can leave a partial recording.

The codec/recovery integration test uses real local FFmpeg/FFprobe and no microphone:

```sh
LAUT_TEST_FFMPEG=/path/to/ffmpeg LAUT_TEST_FFPROBE=/path/to/ffprobe \
  cargo test -p laut-engine --test recording --locked -- --ignored
```

## Public media links

Choose **Link** or **Quellen & Aufträge / Sources & jobs**, paste one public HTTPS link per line and choose **Zur Warteschlange hinzufügen / Add to queue**. Examples are an individual YouTube/Vimeo video, a compatible podcast episode or media-library page, or a direct MP3/M4A/MP4 address. Then start that job or all pending jobs under **Aufträge / Jobs**. File and link imports share the selected local Whisper model, language, vocabulary prompt and optional reviewed speaker analysis. The optional **Nur die ersten … Minuten / Only the first … minutes** applies to every link; otherwise the whole recording is imported. YouTube time markers are removed and a video link containing a playlist parameter imports only that video.

Under **Modelle / Models** in the sidebar, install **yt-dlp** and **Deno**, or choose existing executables under **Lokale Programme und eigene Modelle**. Official standalone yt-dlp includes the YouTube challenge scripts; Deno runs them locally. Packages are pinned by SHA-256 and size for macOS ARM/Intel, Windows x64 and Linux x64/ARM64. The catalog currently uses yt-dlp **2026.10.21** and Deno **2.9.8**. Paths persist, and explicitly retrying queued URLs uses the currently selected downloader/runtime. The program catalog is updated with Laut; there is no automatic yt-dlp self-update. External current versions remain usable. See the [official yt-dlp instructions](https://github.com/yt-dlp/yt-dlp) and [YouTube runtime setup](https://github.com/yt-dlp/yt-dlp/wiki/EJS).

On **Windows without a dedicated GPU**, install/select the CPU Whisper model, then yt-dlp and Deno in the same program manager. **FFmpeg and FFprobe remain required local executables**: select `ffmpeg.exe` and `ffprobe.exe` from the same `bin` folder, or place them in `tools/` next to Laut. No Python, shell command or API key is required for URL imports. Arguments use native process APIs, including paths with spaces/Unicode; child processes belong to a Windows Job Object so cancellation stops their descendants. The Windows implementation is type-checked and has cross-platform subprocess tests in CI; an actual Windows device run remains outstanding.

Only downloading the source and explicitly installing tools/models need network access. ASR and speaker analysis remain local. Active/upcoming streams, archives not yet fully published, private/login-only pages, playlists and feeds are rejected. No browser cookies are read. Use a public link without credentials or personal access tokens; the normalized source link is retained in the project. Completed recordings up to 24 hours and audio downloads up to 2 GiB are supported. Actual provider support and availability follow the installed yt-dlp version and can change independently of Laut.

Permanent projects live in the selected portable library (`LautPortable` by default), with `recording.json` and a compact `source.m4a`. The JSON retains transcript, title, normalized source URL, provider, publisher, publication date, original duration, optional excerpt bounds and source chapters. The header links back to the source. Full-text copying and TXT/Markdown/SRT exports work as for file imports. Source video is never downloaded. AAC is copied without another lossy encoding pass; other audio is converted to 96 kbit/s when compact audio is selected.

Queue history, logs, download files and ASR checkpoints live separately in `LautPortableState/jobs` (`laut-portable-state/jobs` on Linux). Interrupted jobs are paused and need an explicit restart. yt-dlp can resume supported partial downloads; FFmpeg excerpt downloads may restart. Completed audio is reused only after hash verification. ASR resumes committed five-minute chunks. **Temporäre Dateien entfernen / Remove temporary files** deletes both download and processing caches only after verifying the permanent library audio; queue history and the library remain. Until that explicit cleanup, temporary audio copies consume additional disk space.

CLI equivalent, using a normal `JobConfig` JSON for the local ASR programs/model (its `input` and `output` are ignored here):

```sh
laut-cli queue-add-url /path/to/jobs /path/to/library job.json \
  'https://vimeo.com/VIDEO_ID' \
  --yt-dlp /path/to/yt-dlp --deno /path/to/deno --first-seconds 1800
laut-cli queue-run /path/to/jobs JOB_ID
laut-cli queue-cleanup /path/to/jobs JOB_ID
```

Omit `--first-seconds` to import the full video. The same commands are available in `laut-cli.exe` using Windows paths and quoting.

## Conference documents and research

Open a library project and choose **Conference / Konferenz**. **PDFs → Add PDFs** copies the originals into that project's `documents/` folder, records their SHA-256, extracts native text page by page, and optionally OCRs pages with little text. Native text, supplemental OCR, source page, extraction method and warnings remain separate. Scanned German/English documents are supported by the verified `ocr-deu` / `ocr-eng` model packages. No inference request leaves the computer.

The local programs **Poppler** (`pdfinfo`, `pdftotext`, `pdftoppm`) and **Tesseract** must be installed; their paths and OCR language are configurable under **Storage / programs**. The development Mac has these programs and both language packages installed. They are not yet bundled into Windows/Linux installers. Automatic OCR uses a text-density heuristic; **Add OCR** can also process an individual page containing text inside an image. A readable heading does not imply all chart labels were extracted. Text and OCR are search aids; the original PDF remains authoritative for layout, tables, charts and footnotes.

- **PDFs** searches native text and OCR, opens the actual page image, and links a page to the current audio time. Library search also opens the matching PDF page directly.
- **Slides** proposes a small selection based on distinctive shared words in the transcript and PDF text, with evidence. These are thematic matches, not video-frame recognition. Review a match and mark it useful before it appears alongside the transcript or as an exported image. Rejected pages are not automatically proposed again.
- **Chapters** offers editable reading boundaries and excerpt titles. These are not generated semantic summaries. Existing editorial chapters remain intact.
- **Video inserts** suggests intervals only from explicit spoken introductions and returns. Review the times before enabling exclusion. Exclusions affect the reading/copy/export projection; original segments and audio remain intact, and changes can be undone.
- **Vocabulary** saves reviewed names/terms. Explicit self-introductions or short handovers can suggest names for listening and confirmation; a mere mention is insufficient. The vocabulary can be copied into the next transcription's prompt.
- **Storage / programs → Export research folder** creates an independent folder with original PDFs/audio, editable project/history, TXT/Markdown/SRT, page-separated text/JSON and `index.html` with selected reviewed slide images. No web service is needed to read it. Existing destinations are refused.

Conference changes use the same automatic library saving, conflict protection and undo history as transcript edits. A project copy retains PDFs referenced by undo/redo as well as currently visible attachments. Modal reference edits and vocabulary input require their explicit **Apply/Save** action; those unapplied form inputs are not part of periodic transcript-draft recovery.

The CLI exposes `pdf-import`, `pdf-preview`, `pdf-ocr`, `conference-suggest` and `conference-export`; run it without arguments for signatures. A JSON PDF configuration may override program paths, `ocr_language`, `auto_ocr`, and a `traineddata` map of language codes to installed files. The defaults use programs on PATH and system Tesseract language data.

## Semantic search and local text analysis

Open a library project and choose **Suche & KI / Search & AI** in the sidebar or **KI-Auswertung / AI analysis** above the transcript. This is separate from the existing exact-text search.

- **Search / ask** searches transcript passages and, optionally, native PDF text plus OCR. Search can cover the current project, one collection or the whole portable library. German questions can retrieve English sources. Semantic ranking is supplemented by exact word matches weighted by rarity; passages matching more query anchors are ordered first, so names, acronyms and years are not treated as merely similar concepts. For a named entity, use a short query with two to five anchors, for example `SpaceX Musk Starlink`, instead of adding broad related words. Literal matches are named and highlighted in every result. Semantic-only passages must clear a conservative relevance floor; weaker similarities are hidden behind a separate opt-in control and are not presented as evidence. Duplicate passages and conference-wide copies of the same PDF text are collapsed. A result summary groups counts by recording. Results open the original transcript location or PDF page, and **Back to results** restores the unchanged list. Similarity is a ranking signal, not evidence that a claim is true.
- **Answer locally** creates a short draft from selected search hits (up to five passages), with source IDs and exact supporting quotations. Its displayed coverage is deliberately limited. It is not a complete conference summary.
- **Full analysis** processes all selected source text from the current project, one event/topic collection or the whole library in small batches, retains section notes, and builds an overview. PDF page text is opt-in here. Completed steps are saved; start again with the same task, model and unchanged sources to resume after cancellation. There is no fixed 24,000-character truncation. Reading exclusions apply; raw segments remain intact.
- **Saved results** retains the question, model identity, coverage and source snapshots. Copy/export the complete report or explicitly **Append to project analysis**. Appending preserves previous results, is undoable, and makes the report part of project copies/library backups. Changed sources are detected before navigation; an old report retains the text it actually used.

The model manager includes **Qwen3 Embedding 0.6B Q8** (639 MB) and **Qwen3 4B Instruct 2507 Q4_K_M** (2.50 GB), approximately **3.14 GB** combined. The embedding file is published by Qwen; the 4B quantization is published by Unsloth. Both are Apache-2.0 model packages pinned by revision, size and SHA-256 in the catalog. The smaller 1.7B model remains an experimental option; an initial test incorrectly attached years to events. Neither model is suitable for unchecked factual publication. A 16 GB RAM machine is a sensible starting point for development/testing, not a measured minimum for every OS.

Local `llama-embedding` and `llama-completion` workers use **llama.cpp 0.7.0**, explicit model files, offline mode and CPU execution. No server, API key or cloud fallback is used. Models are loaded for each small batch; CPU throughput and startup cost vary by machine. Text generation uses a 4,096-token context and disables weight repacking, and loads model weights into ordinary RAM to avoid repeated file-page eviction under desktop memory pressure. This does not lock RAM against normal OS paging. Heavy system memory pressure can make even short answers very slow. Four threads are the default. A dedicated GPU and Apple-specific inference framework are not required. The Mac bundle includes both workers. `python portable/scripts/build-llama.py` and the CI matrix prepare equivalent CPU workers for Windows/Linux; those platforms still need native real-device execution tests. The Windows build disables AVX/FMA-specific requirements rather than assuming a recent GPU or CPU extension.

**Setup** remembers worker/model paths, CPU threads and output language. Supported text profiles are Qwen3 Instruct and Qwen3 with thinking disabled; an arbitrary GGUF model may need a different prompt adapter. Search uses Qwen3 embeddings with last-token pooling. Downloads require a network connection; installed-model processing does not. The app does not apply an OS-level network firewall to user-selected external executables.

Disposable vectors and resumable analysis steps are stored in `LautPortableState/research/vectors` and `steps`; independent reports in `research/results`. Cache identities include text, model and worker hashes; editing one passage only requires recomputing its vector. These state folders are separate from library backups. Export a report or append it to the project to include it in the research archive. No audio or PDF is duplicated for indexing. This initial implementation scans cached vectors locally; very large libraries and ANN/SQLite indexing remain future work.

Exact quotation and source-ID checks reject broken references. Unambiguous ASCII capitalization and whitespace differences in a quote are restored from the original source; changed words or numbers are not repaired. These checks do **not** verify the meaning of a generated sentence. Always check names, numbers, dates and interpretations against the source. ASR/OCR mistakes can propagate. Text-only models do not interpret the visual layout or diagrams inside PDFs; the original page remains necessary. Source documents are treated as quoted data, never as instructions to run tools or upload content.

CLI equivalent:

```sh
laut-cli research /local/library research.json /local/research-cache --recording RECORDING-UUID --mode search --query "Which efficiency targets are mentioned?"
laut-cli research /local/library research.json /local/research-cache --recording RECORDING-UUID --mode answer --query "Which efficiency targets are mentioned?"
laut-cli research /local/library research.json /local/research-cache --recording RECORDING-UUID --mode analyse --no-pdfs --query "Extract key claims, numbers, deadlines and unresolved questions."
```

`research.json` contains absolute `embedding_worker`, `generation_worker`, `embedding_model`, `text_model` paths, `threads` (4), `language` (`de` or `en`) and `text_profile` (`qwen3-instruct` by default). On Windows use `.exe` workers and escaped backslashes or forward slashes. Omitting `--recording` searches the whole library; `--no-pdfs` restricts sources to transcripts. CLI reports are written to the supplied cache's `results` folder; use the app's research folder if they should appear in its saved-results list.

## Library

Open **Settings / Einstellungen → Manage library / Bibliothek verwalten**, select a new empty folder and choose **Create new library**, or choose **Open / refresh** for an existing portable library. **Import this project** copies the currently open project and its media into that library and opens the copy. The existing Swift library is never automatically adopted. An existing recording ID is refused rather than overwritten.

Within a library project, **Apply text**, speaker corrections and document undo/redo are saved in the background; a successful save retains `recording.previous.json`. A failed or conflicting save reports an error. Pending editor inputs remain available and are periodically saved to a separate local recovery folder. **Apply all inputs** commits the pending title, notes, segment corrections and speaker names together. Structural edits and document undo/redo require pending inputs to be applied first. Loading a damaged current document falls back to its previous valid version with a warning; a later save preserves the damaged bytes under a separate filename. This previous-version recovery is separate from an independent library backup.

Under **Collections / Sammlungen → Manage**, create an event or topic folder and select any number of library recordings. A collection stores only names and recording IDs in `laut-library.json`: projects and media are not moved or duplicated. Collections are included in verified library backups. Selecting one filters the sidebar; **Search & AI** uses the same collection for hybrid search, local answers and full analysis. Removing a collection removes only that grouping, never its recordings, transcripts, PDFs or saved research.

Suggested roots are `~/Library/Application Support/LautPortable` on macOS, `%LOCALAPPDATA%/LautPortable` on Windows, and `$XDG_DATA_HOME/laut-portable` (or `~/.local/share/laut-portable`) on Linux. No library is created merely by opening the chooser. The custom library path, interface language, last project and worker/model paths are remembered between launches. The app reopens the last available project.

**Drafts / Entwürfe** lists recoverable inputs after reopening. Drafts are written approximately once per second and before switching projects/normal exit. A sudden crash can lose the most recent fraction of a second. Closing with a known failed recovery write is blocked by the normal window-close path. A recovered draft rejoins its library only when its document still matches the saved base; otherwise it opens as an independent recovery that requires a new project copy. Recovery creates a separate draft ID and archives the recovered input under `recovered/`. The archive is retained locally. Drafts do not duplicate media.

Preferences and drafts live in `~/Library/Application Support/LautPortableState` on macOS, `%LOCALAPPDATA%/LautPortableState` on Windows, and `$XDG_DATA_HOME/laut-portable-state` (or `~/.local/share/laut-portable-state`) on Linux. Development/tests may override this with an absolute `LAUT_PORTABLE_STATE_DIR`. These files are separate from the Swift app and from library backups.

In **Library**, type a query and choose **Search contents**. The background scan searches titles, included transcript text/speaker names, notes, stored analysis and saved PDF page text. Transcript results open the relevant editable segment. Results stop displaying at 200 with an explicit message to refine the query; every recording is still checked for warnings. This is literal search, not yet a SQLite or semantic index.

**Settings → Storage & trash** (also available in **Manage library**) shows the logical file size of each project, largest first, plus active recordings, local trash, other library files and free space on the drive. Filter by title to find an item. Counts include audio, PDFs and retained project versions; models and processing caches outside the library are not included. Symbolic links are not followed; unreadable entries produce visible warnings rather than silently complete totals.

**Move to trash** asks for confirmation and moves the entire project folder into this library's `.trash` directory. Audio, PDFs, unknown attachments and edit history stay together. The item disappears from the active catalog/search; **Trash → Restore** returns it and opens the transcript. Pending inputs must be applied and saved before moving the open project. A changed project or an existing project with the same ID blocks the operation without overwriting either copy. The engine coordinates directory moves with imports, saves and backups through a library lock. There is no permanent-delete or automatic-emptying action: trash still occupies disk space and is included in library backups. It is separate from the operating system's Trash/Recycle Bin.

**Back up library** writes a new directory containing saved projects, media, previous versions and a SHA-256 manifest. Project locks prevent concurrent Laut saves during copying; files are checked before/after copying. Missing media, symlinks and damaged/recovered projects stop the backup with an error. **Restore backup** verifies the complete manifest and creates a separate library, never overwriting an existing destination. Drafts/preferences are kept separately on the current computer; apply inputs before backing up if they should be included. Keep an independent copy on another drive for protection against disk failure.

The CLI exposes the same ownership and save rules:

```sh
laut-cli library-init /new/portable-library
laut-cli library-import /new/portable-library /source/recording.json
laut-cli library-list /new/portable-library
laut-cli library-collections /new/portable-library
laut-cli library-collection-set /new/portable-library "Event name" --recording RECORDING-UUID --recording ANOTHER-UUID
laut-cli library-search /new/portable-library "investment"
laut-cli library-storage /new/portable-library
laut-cli library-trash /new/portable-library RECORDING-UUID
laut-cli library-untrash /new/portable-library TRASH-TOKEN
laut-cli library-backup /new/portable-library /new/backup-folder
laut-cli library-restore /new/backup-folder /new/restored-library
laut-cli library-commands /new/portable-library RECORDING-UUID
```

`library-commands` reads the same JSON commands documented below. Each successful document change is saved before success is reported. A concurrent file change is refused; the command does not force an overwrite.

## Build

Rust is pinned in `rust-toolchain.toml`. On the development Mac, `../scripts/portable-cargo.sh` uses the isolated toolchain under `.runtime/rust`; it does not modify the user's shell configuration. Elsewhere, use a normal rustup installation:

```sh
cd portable
cargo test --workspace --locked
cargo run --release -p laut-portable -- /path/to/recording.json
```

To build the bundled CPU speech worker, run `python3 portable/scripts/build-whisper.py` from the repository root (CMake and a C++ toolchain required on the build host). It verifies the pinned whisper.cpp source and builds without CUDA or Metal. The Windows build disables AVX/FMA-specific defaults for a broader CPU baseline. End users do not need Python or CMake for this worker.

For the macOS app bundle, run `bash scripts/build-portable-app.sh` from the repository root. It writes `dist/Laut.app` and applies an ad-hoc local signature. Public alpha archives are not notarized.

On Windows, use the MSVC Rust toolchain and Visual Studio C++ build tools. Linux needs the audio/window development libraries listed in `.github/workflows/portable.yml`. The workflow is configured to build/test on macOS, Windows and Linux and package the app, CLI and pinned CPU Whisper/llama.cpp workers under `tools/`. FFmpeg/FFprobe must currently be provided locally; the app detects them in PATH, beside the app, or under `tools/`. Download the speaker runtime/models explicitly from the model manager. The official Windows runtime archive and its DLL paths were checked; execution on a real Windows machine remains outstanding. A checked-in workflow is **not evidence of a successful remote run**. These binaries are not signed/notarized public installers; Linux binaries also need compatible system libraries. Windows ARM, Intel Mac, mobile and browser releases are not validated.

## One engine, multiple clients

```text
desktop (egui + audio output)      CLI (JSON lines / batch)
                 \                 /
                 engine (files, workers, jobs)
                              |
                 core (document, commands, projections)
```

`core` and `engine` have no UI/audio-device dependencies. `check-layers.py` verifies this from Cargo's dependency graph. Changes enter through `Command`; the UI never directly edits authoritative JSON. Data is projected into typed structs for reading, while the original JSON retains fields the Rust app does not understand.

[Photocraft](https://github.com/storytold/photocraft) inspired the UI-independent engine, shared commands, enforced dependency direction and platform test matrix. No Photocraft source was copied. The egui choice is provisional: selection, editing, accessibility and large-text responsiveness need hands-on comparison before replacing SwiftUI.

## CLI

```sh
cargo run -p laut-cli -- inspect fixtures/conversation.json
cargo run -p laut-cli -- search fixtures/conversation.json 'Grüße'
cargo run -p laut-cli -- export fixtures/conversation.json --out /new/transcript.txt
cargo run -p laut-cli -- copy /path/to/recording.json --out /new/project-folder
cargo run -p laut-cli -- commands /path/to/recording.json --out /new/edited-project
```

The last command reads one JSON command per stdin line and prints one result per line. EOF saves the new project only when `--out` is supplied. No socket or external access is opened. Examples:

```json
{"command":"inspect"}
{"command":"search","query":"investment"}
{"command":"edit_segment","id":"EXISTING-SEGMENT-UUID","text":"Corrected text."}
{"command":"rename_speaker","id":"EXISTING-SPEAKER-ID","name":"Confirmed name"}
{"command":"update_details","title":"Interview","notes":"Check the quoted numbers."}
{"command":"split_segment","id":"EXISTING-SEGMENT-UUID","at":12.5,"offset":20}
{"command":"merge_next","id":"EXISTING-SEGMENT-UUID"}
{"command":"replace_text","find":"Jansen","replacement":"Jensen"}
{"command":"add_speaker","name":"Confirmed name"}
{"command":"merge_speakers","from":"SOURCE-SPEAKER-ID","into":"DESTINATION-SPEAKER-ID"}
{"command":"undo"}
{"command":"redo"}
{"command":"export_text"}
```

## Import, models and speaker analysis

Open **Quellen & Aufträge / Sources & jobs**, or use **+ Importieren / + Import** in the toolbar. On a prepared installation, select audio/video files, choose the language, optionally enter names/terms and add the files to the queue. **Jobs / Aufträge → Start pending jobs** processes them sequentially. Each successful job creates an independent project in the selected portable library; existing projects are never overwritten. A repeated unfinished source/model/settings configuration is refused as a duplicate. Importing a completed source again creates an independent new project. Restarting Laut only lists unfinished jobs; it does not resume them automatically. Click **Start / resume** to continue. The unfinished five-minute ASR chunk repeats; completed chunks remain exact.

**Compact audio** selects the first audio stream and removes video from the library copy. Existing AAC is remuxed without another lossy encoding. Other formats become 96 kbit/s AAC, up to stereo/48 kHz. Disable this option to keep a byte-identical source copy. The original file is never deleted or modified. Video and additional audio tracks remain at the original path, recorded with its SHA-256 in `portableImport`; a library backup only contains the library's copy. Temp WAV data is deleted when the worker finishes or is cancelled. Completed job folders can be removed explicitly after Laut verifies the library audio hash; job history remains. During import, the job copy and library copy temporarily coexist.

**Models / programs** starts with task cards for transcription, link import, speaker separation, semantic search and local analysis. Each card explains its purpose, reports **Ready** only when its complete local chain is available and installs all missing catalog packages sequentially with one explicit action. **Whisper Small Q5** is the recommended CPU speech model. The complete catalog remains under **All packages and alternatives** for Base/Tiny models, OCR languages, verification and individual removal. Partial downloads survive interruption; bad hashes are quarantined. Archive paths are checked, internal links become regular files, and installed files receive a hash manifest. **Verify** rechecks all installed files. **Remove** only deletes installer-owned packages, never external model files. Program/model paths persist. The app discovers its bundled workers and locally installed FFmpeg/FFprobe; other paths can be chosen under **Local programs and external models**.

The manager lives under `LautPortableState/models`; queue requests, checkpoints and cached speaker results under `LautPortableState/jobs`. Use the platform-specific state roots above. These are separate from library backups. Models can be installed again; keep the original sources until the independent library copy is verified. No audio/transcript is sent to the model hosts. Inference invokes local executables without any API key or cloud fallback; this does not impose a system-level network firewall on an arbitrary external executable.

In **Speakers**, specify a known speaker count if available (0 = automatic), then choose **Analyse voices locally**. This pass uses the existing audio, not another ASR transcription. Compare listening examples and map voices to existing people, or leave new voices unnamed and name them later in the editor. Name suggestions use manually confirmed transcript spans only; there is no identity recognition from a voice. Applying a selected name mapping protects those assignments. Existing locked segments and text corrections are retained. Unambiguous short gaps within the same voice are bridged; competing turns and ambiguous overlap remain unassigned. Word-level splitting requires matching original text and valid word evidence. Large original token evidence is stored once under `portableDiarizationSources` with per-segment references. **Undo** reverses the applied speaker/segment changes; the original evidence archive remains available.

Optional analysis during queue processing computes and caches suggestions. If that optional pass fails, the finished transcript is still published with an explicit warning; it can be opened and speaker analysis retried separately. It does not silently assign names. After opening the imported project, **Analyse voices locally** with the same settings reuses this result for review. The current native diarizer processes the recording as one pass, so cancelling it restarts that pass; ASR checkpoints remain intact. Whole-recording clustering helps avoid changing voice labels at arbitrary five-minute boundaries, but seven-hour runtime/RAM has not yet been measured.

### CLI examples

```sh
laut-cli models /local/model-folder
laut-cli model-install /local/model-folder ggml-small-q5_1
laut-cli model-verify /local/model-folder ggml-small-q5_1
laut-cli queue-add /local/jobs /local/library /local/job.json
laut-cli queue-list /local/jobs
laut-cli queue-run /local/jobs JOB-UUID
laut-cli queue-cleanup /local/jobs JOB-UUID
laut-cli diarize /project/recording.json /local/speakers.json /local/speaker-cache /new/analysis.json
```

`queue-add --diarization speakers.json` prepares the optional analysis as well. Direct transcription remains available through `laut-cli transcribe --config job.json`:

```json
{
  "input": "/path/to/video.mp4",
  "output": "/path/to/new-job-folder",
  "whisper": "/path/to/whisper-cli",
  "model": "/path/to/ggml-small-q5_1.bin",
  "ffmpeg": "/path/to/ffmpeg",
  "ffprobe": "/path/to/ffprobe",
  "language": "en",
  "use_gpu": false,
  "compact_audio": true,
  "prompt": "Friedrich Merz, Jensen Huang, NVIDIA"
}
```

On Windows use absolute paths, escaped backslashes or forward slashes, and `.exe` filenames. `speakers.json` contains `executable`, `segmentation`, `embedding`, `ffmpeg` paths and optionally `speakers` (0), `threshold` (0.6), `threads` (4). The CPU provider is explicit on all systems. `apply_speaker_analysis` is the shared JSON command with `analysis` (the complete analysis object) and `mapping` (cluster label → existing speaker ID; omit unmatched clusters). Applying without a mapping creates neutral voice labels.

Source/model/program/settings hashes must match when resuming. Changes to published partial text or notes are refused, never overwritten. A persisted request permits retrying a cancelled initial audio extraction. Old version-1 checkpoints remain readable with their original signature/output format. Ctrl+C requests CLI cancellation. Downloads may take up to their network timeout to stop; stalled downloads can be resumed explicitly.

The adapter is tested against whisper.cpp 1.9.5. Its full JSON token evidence is preserved; words are joined only when their text matches the transcript and their timestamps are valid. Otherwise the segment remains usable without invented word timing. Final padded segment endpoints are bounded to the audio duration, with original offsets retained under `workerTiming`. ASR words and speaker boundaries are estimates, particularly during laughter, applause, overlaps and short interjections. Five-minute ASR boundaries still lack overlapping context. Vocabulary is a hint, not a guarantee that all names are correct. This is not yet a quality-equivalent replacement for Swift's MLX/Parakeet workflow.

Primary runtime/model sources and license links are pinned in `crates/engine/src/model-catalog.json`: [whisper.cpp](https://github.com/ggml-org/whisper.cpp), [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.8), [Pyannote segmentation (MIT)](https://huggingface.co/pyannote/segmentation-3.0), [TitaNet Small (NeMo Toolkit license)](https://catalog.ngc.nvidia.com/orgs/nvidia/nemo/models/titanet_small). Bundled archives retain their notices; the compiled speech worker includes its MIT license/source reference. FFmpeg redistribution and a full offline installer remain packaging work.

## Checks and remaining work

See [VALIDATION.md](VALIDATION.md) for actual local results and the distinction between checked code and tested hardware. Development builds can trigger a fresh macOS folder-access prompt when their ad-hoc signature changes.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python check-layers.py
```

`Tests/PortableInterop` exercises the **existing Swift implementation**, checking that a Rust TXT projection is identical and Rust-produced edit history can undo/redo in Swift. Run `LautPortableInterop recording.json exported.txt` after building that Swift target.

Generated semantic chapters and API-key support remain open. Microphone and meeting capture are implemented as described above. PDF text/OCR, conference review, semantic retrieval and local source-linked text analysis are implemented. The Swift MLX path remains available in the original app; it is not yet wired into the Rust engine. Large-library indexed search, scheduled backups, permanent deletion/separate audio removal and editing stored AI results remain open. Audio, native dialogs, keyboard/selection behavior and accessibility still need real Windows/Linux hardware testing. Long inference jobs must currently run while the computer stays awake.
