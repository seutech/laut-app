import SwiftUI
import AppKit
import AVFoundation
import UniformTypeIdentifiers
import LautCore
import LautAudio

@MainActor
final class AppStore: ObservableObject {
    @Published var recordings: [Recording] = [] { didSet { searchRevision += 1; scheduleSearch(); scheduleMarkdownArchive() } }
    @Published var selected: UUID? { didSet { if selected != oldValue { stopPlayback() } } }
    @Published var section = "library"
    @Published var search = "" { didSet { scheduleSearch(show: true) } }
    @Published var settings = AppSettings()
    @Published var vocabulary: [VocabularyEntry] = []
    @Published var status = L("Bereit · lokal auf deinem Mac")
    @Published var busy = false { didSet { updateProcessingActivity(); scheduleSearch(); if !busy { Task { @MainActor [weak self] in self?.runImportQueue() } } } }
    @Published var error: String?
    @Published var isRecording = false { didSet { updateProcessingActivity(); scheduleSearch(); if !isRecording { Task { @MainActor [weak self] in self?.runImportQueue() } } } }
    @Published var maintenanceRunning = false
    @Published var storageReport: LibraryStorageReport?
    @Published var storageStatus = ""
    @Published var restoredLibrary: URL?
    var storageTask: Task<Void, Never>?
    var processingActivity: NSObjectProtocol?
    var workspaceObservers: [NSObjectProtocol] = []
    var systemSleeping = false
    @Published var playerTime: Double = 0
    @Published var playing = false
    @Published var skipExcludedRanges = false
    @Published var playbackDuration: Double = 0
    @Published var playbackRate: Float = 1
    @Published var importing = false
    @Published var importStatus = ""
    @Published var modelStatus = L("Modell nicht vorbereitet")
    private var preparedModelKey: String?
    private var observedModelKey: String?
    private var automaticPreparation = true
    let library: Library
    let markdownArchive = MarkdownArchive()
    var markdownTask: Task<Void, Never>?
    @Published var markdownStatus = ""
    let searchIndex: SearchIndex
    let embeddings: EmbeddingEngine
    @Published var searchHits: [SearchHit] = []
    @Published var searchStatus = ""
    @Published var searching = false
    @Published var searchSpeaker = "" { didSet { scheduleSearch() } }
    @Published var searchDays = 0 { didSet { scheduleSearch() } }
    @Published var searchRecording: UUID? { didSet { scheduleSearch() } }
    @Published var searchDestination: SearchHit?
    @Published var downloadProgress = ""
    var downloadGeneration = UUID()
    var searchState = SearchState()
    var searchRevision = 0
    var indexedSearchRevision: Int?
    @Published var libraryAnswer = ""
    @Published var answerSources: [SearchHit] = []
    @Published var answering = false
    var searchTask: Task<Void, Never>?
    var embeddingIdleTask: Task<Void, Never>?
    let engine: TranscriptionEngine
    let analyzer = SpeakerAnalyzer()
    var task: Task<Void, Never>?
    var player: AVPlayer?
    private var playbackID: UUID?
    let importer = FileImporter()
    let mediaDownloader = MediaDownloader()
    @Published var importJobs: [ImportJob] = []
    @Published var queueError: String?
    @Published var activeImportID: UUID?
    var importJobTask: Task<Void, Never>?
    private var timer: Timer?
    var recorder: AVAudioRecorder?
    var captureURL: URL?
    var capturedKind: DocumentKind = .meeting
    var captureTarget: NSRunningApplication?
    var captureAnchor: TextInsertion.Anchor?
    var meeting: MeetingCapture?
    var meetingDirectory: URL?
    var shortcuts: GlobalShortcuts?

    init() {
        let args = CommandLine.arguments
        let override = args.firstIndex(of: "--library").flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil }
        let base = override.map { URL(fileURLWithPath: $0) } ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Laut")
        // Failure to open the real library must never silently create a different one.
        do { library = try Library(root: base) } catch { fatalError(L("Bibliothek nicht erreichbar: \(error)")) }
        let resource = Bundle.main.resourceURL ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("Resources")
        searchIndex = SearchIndex(url: base.appendingPathComponent("search-v1.sqlite"))
        embeddings = EmbeddingEngine(script: resource.appendingPathComponent("embedding_worker.py"))
        engine = TranscriptionEngine(worker: resource.appendingPathComponent("mlx_worker.py"))
        do { settings = try library.loadSettings() } catch { self.error = L("Einstellungen sind nicht lesbar; die Originaldatei bleibt erhalten. ") + error.localizedDescription }
        let languageOverride = args.firstIndex(of: "--ui-language").flatMap { $0 + 1 < args.count ? InterfaceLanguage(rawValue: args[$0 + 1]) : nil }
        LautLocalization.language = languageOverride ?? settings.interfaceLanguage ?? .de
        if !FileManager.default.fileExists(atPath: base.appendingPathComponent("settings.json").path) {
            settings.customInstructions = AppSettings().customInstructions
        }
        status = L("Bereit · lokal auf deinem Mac")
        modelStatus = L("Modell nicht vorbereitet")
        do {
            recordings = try library.load()
            if !library.loadWarnings.isEmpty { self.error = library.loadWarnings.joined(separator: "\n") }
        } catch { self.error = L("Lokale Daten konnten nicht vollständig gelesen werden: \(error.localizedDescription)") }
        do { vocabulary = try library.loadVocabulary() } catch { self.error = L("Wörterbuch ist nicht lesbar; die Originaldatei bleibt erhalten. ") + error.localizedDescription }
        if override != nil, settings.markdownDirectory == nil {
            settings.markdownDirectory = base.appendingPathComponent("markdown-archive").path
        }
        if settings.runtimeDirectory.isEmpty,
           let data = try? Data(contentsOf: resource.appendingPathComponent("development.json")),
           let config = try? JSONSerialization.jsonObject(with: data) as? [String: String], let path = config["runtime"] {
            settings.runtimeDirectory = path
            let phonon = path + "/models/speech/FermionResearch__Phonon-2/model_phonon2_c4c_int6"
            if FileManager.default.fileExists(atPath: phonon) { settings.modelPaths["phonon"] = phonon }
            let speakers = URL(fileURLWithPath: path).appendingPathComponent("models/diarization/speaker-diarization")
            settings.diarizationInstalled = ["Segmentation.mlmodelc", "Embedding.mlmodelc", "PldaRho.mlmodelc", "FBank.mlmodelc", "plda-parameters.json"].allSatisfy { FileManager.default.fileExists(atPath: speakers.appendingPathComponent($0).path) }
        }
        selected = recordings.first?.id
        do { importJobs = try jobStore.load() }
        catch { queueError = L("Auftragsliste konnte nicht gelesen werden; Originaldatei bleibt erhalten: ") + error.localizedDescription }
        if !settings.runtimeDirectory.isEmpty {
            for model in [EngineKind.parakeet, .qwen] where settings.modelPaths[model.rawValue] == nil {
                let repo = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("models/huggingface/models--" + model.modelID.replacingOccurrences(of: "/", with: "--"))
                if let revision = try? String(contentsOf: repo.appendingPathComponent("refs/main"), encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines),
                   revision.range(of: "^[a-f0-9]{40}$", options: .regularExpression) != nil {
                    let snapshot = repo.appendingPathComponent("snapshots/" + revision)
                    if FileManager.default.fileExists(atPath: snapshot.appendingPathComponent("config.json").path) { settings.modelPaths[model.rawValue] = snapshot.path }
                }
            }
        }
        for model in EmbeddingModel.allCases where settings.embeddingPaths?[model.rawValue] == nil {
            let folder = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("models/huggingface/models--" + model.modelID.replacingOccurrences(of: "/", with: "--") + "/snapshots")
            let candidates = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
            for candidate in candidates.sorted(by: { $0.path < $1.path }) {
                if let data = try? Data(contentsOf: candidate.appendingPathComponent(".laut-complete.json")),
                   let marker = try? JSONSerialization.jsonObject(with: data) as? [String: String], marker["modelID"] == model.modelID,
                   FileManager.default.fileExists(atPath: candidate.appendingPathComponent("model.safetensors").path) {
                    var paths = settings.embeddingPaths ?? [:]; paths[model.rawValue] = candidate.path; settings.embeddingPaths = paths
                }
            }
        }
        timer = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                if let failure = self.player?.currentItem?.error {
                    self.stopPlayback(); self.error = L("Audio konnte nicht abgespielt werden: \(failure.localizedDescription)"); return
                }
                let position = self.player?.currentTime().seconds ?? 0
                if position.isFinite, abs(self.playerTime - position) > 0.04 { self.playerTime = position }
                let isPlaying = (self.player?.rate ?? 0) != 0
                if isPlaying, self.skipExcludedRanges, let id = self.playbackID, let record = self.recordings.first(where: { $0.id == id }),
                   let range = ConferenceReview.excludedRanges(record).first(where: { $0.start <= position && position < $0.end }) {
                    self.seek(record, to: range.end)
                }
                if self.playing != isPlaying { self.playing = isPlaying }
                if let duration = self.player?.currentItem?.duration.seconds, duration.isFinite, duration > 0, self.playbackDuration != duration { self.playbackDuration = duration }
            }
        }
        shortcuts = GlobalShortcuts { [weak self] id in
            Task { @MainActor in
                if id == 1 { self?.toggleRecording(kind: .dictation) }
                else { self?.refineSelection() }
            }
        }
        observedModelKey = selectedModelKey
        prepareSelectedModel()
        scheduleSearch()
        scheduleMarkdownArchive()
        installSleepHandling()
    }
    private var selectedModelKey: String {
        [settings.runtimeDirectory, settings.engine.rawValue, settings.modelPaths[settings.engine.rawValue] ?? ""].joined(separator: "\n")
    }
    var modelReady: Bool { preparedModelKey == selectedModelKey && engine.warmWorker.isRunning }
    func prepareSelectedModel(force: Bool = false) {
        if force { automaticPreparation = true }
        guard automaticPreparation, !busy, !isRecording, !systemSleeping else { return }
        if modelReady { return }
        guard let path = settings.modelPaths[settings.engine.rawValue], FileManager.default.fileExists(atPath: path) else {
            modelStatus = L("Bitte ein Sprachmodell installieren"); return
        }
        let config = settings, key = selectedModelKey
        busy = true; preparedModelKey = nil
        modelStatus = L("\(config.engine.label) wird geladen und aufgewärmt …")
        status = L("Modell vorbereiten · einmalig vor der ersten Aufnahme")
        task = Task {
            defer { busy = false; task = nil }
            do {
                let began = Date()
                try await engine.preload(settings: config)
                try Task.checkCancellation()
                guard selectedModelKey == key else { modelStatus = L("Modellwahl geändert · bitte vorladen"); return }
                preparedModelKey = key
                modelStatus = L("\(config.engine.label) bereit")
                status = L("Modell in \(String(format: "%.1f", Date().timeIntervalSince(began))) s vorbereitet · bereit für Aufnahmen")
            } catch {
                automaticPreparation = false
                modelStatus = L("Modell nicht vorbereitet")
                status = Task.isCancelled ? L("Vorbereitung abgebrochen") : L("Modellvorbereitung fehlgeschlagen")
                if !Task.isCancelled { self.error = error.localizedDescription }
            }
        }
    }
    func unloadModel() {
        guard !busy, !isRecording else { return }
        automaticPreparation = false; preparedModelKey = nil
        engine.warmWorker.stop(); modelStatus = L("Modell entladen"); status = L("Arbeitsspeicher freigegeben")
    }
    var current: Recording? { recordings.first { $0.id == selected } }
    var modelDirectory: URL { URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("models/diarization") }
    var filtered: [Recording] { recordings }
    func saveSettings() {
        guard !maintenanceRunning else { return }
        scheduleMarkdownArchive()
        do { try library.saveSettings(settings) } catch { self.error = error.localizedDescription }
        if observedModelKey != selectedModelKey {
            observedModelKey = selectedModelKey; preparedModelKey = nil; automaticPreparation = true
            modelStatus = L("Modell nicht vorbereitet"); prepareSelectedModel()
        }
    }
    func saveVocabulary() { guard !maintenanceRunning else { return }; do { try library.saveVocabulary(vocabulary) } catch { self.error = error.localizedDescription } }
    @discardableResult func update(_ id: UUID, _ edit: (inout Recording) -> Void) -> Bool {
        guard let index = recordings.firstIndex(where: { $0.id == id }) else { return false }
        var candidate = recordings[index]; edit(&candidate)
        return commit(candidate, at: index)
    }
    @discardableResult private func commit(_ candidate: Recording, at index: Int) -> Bool {
        guard !maintenanceRunning else { return false }
        do { try library.save(candidate); recordings[index] = candidate; return true }
        catch { self.error = L("Änderung konnte nicht gespeichert werden. Der letzte gespeicherte Stand bleibt erhalten. \(error.localizedDescription)"); return false }
    }
    @discardableResult func edit(_ id: UUID, label: String, key: String? = nil, change: (inout Recording) -> Void) -> Bool {
        guard let index = recordings.firstIndex(where: { $0.id == id }) else { return false }
        let candidate = RecordingEditor.edit(recordings[index], label: label, key: key, change: change)
        return commit(candidate, at: index)
    }
    var undoLabel: String? { current?.editHistory?.undo.last?.label }
    var redoLabel: String? { current?.editHistory?.redo.last?.label }
    func undoEdit() { moveEdit(forward: false) }
    func redoEdit() { moveEdit(forward: true) }
    private func moveEdit(forward: Bool) {
        guard !busy, let index = recordings.firstIndex(where: { $0.id == selected }) else { return }
        let result = forward ? RecordingEditor.redo(recordings[index]) : RecordingEditor.undo(recordings[index])
        guard let result else { error = L("Diese Änderung passt nicht mehr zum aktuellen Dokumentstand."); return }
        commit(result, at: index)
    }
    func chooseFiles() {
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = true
        panel.allowedContentTypes = [.audio, .movie]; panel.canChooseDirectories = false
        guard panel.runModal() == .OK else { return }; importFiles(panel.urls)
    }
    func importFiles(_ urls: [URL]) { addFileJobs(urls) }
    func cancelImport() { pauseImportQueue() }
    func newNote() {
        guard !maintenanceRunning else { return }
        let item = Recording(title: L("Schnellnotiz"), kind: .note)
        do { try library.save(item); recordings.insert(item, at: 0); selected = item.id; section = "library" }
        catch { self.error = error.localizedDescription }
    }
    func transcribeAll() {
        addTranscriptionJobs(recordings.filter { $0.audioFilename != nil && $0.segments.isEmpty }.map(\.id).reversed())
    }
    func transcribe(_ id: UUID, pasteTo target: NSRunningApplication? = nil, anchor: TextInsertion.Anchor? = nil, forceNew: Bool = false) {
        guard !busy, !isRecording, !systemSleeping, let item = recordings.first(where: { $0.id == id }) else { return }
        let source: URL
        do { source = try library.requireAudio(item) } catch { self.error = error.localizedDescription; return }
        // Re-running creates a separate version so no correction or source result is overwritten.
        if forceNew || (!item.segments.isEmpty && item.transcriptionProgress == nil) {
            busy = true; status = L("Neue Version vorbereiten …")
            task = Task { [self] in
                var copy: Recording?
                do {
                    copy = try await importer.importFile(source, root: library.root, title: item.title + L(" · neue Transkription"), metadata: item.source) { [weak self] fraction in
                        Task { @MainActor in self?.status = L("Neue Version kopieren · \(Int(fraction * 100)) %") }
                    }
                    try Task.checkCancellation()
                    guard var copy else { throw LocalEngineError(L("Neue Version konnte nicht angelegt werden.")) }
                    copy.conference = item.conference
                    for document in item.conference?.documents ?? [] {
                        let sourcePDF = try ConferenceMedia.url(document.filename, in: library.folder(item.id))
                        let destination = library.folder(copy.id).appendingPathComponent(document.filename)
                        try FileManager.default.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
                        try await Task.detached { try FileManager.default.copyItem(at: sourcePDF, to: destination) }.value
                    }
                    try Task.checkCancellation()
                    try library.save(copy)
                    recordings.insert(copy, at: 0); selected = copy.id
                    busy = false; task = nil; transcribe(copy.id)
                } catch {
                    if let copy { try? library.delete(copy) }
                    busy = false; task = nil
                    status = Task.isCancelled ? L("Abgebrochen. Das vorhandene Transkript bleibt erhalten.") : L("Neue Version konnte nicht angelegt werden.")
                    if !Task.isCancelled { self.error = error.localizedDescription }
                }
            }
            return
        }
        var config = settings
        if let context = item.conference { config.language = context.language }
        let terms = (item.conference?.terms ?? []) + vocabulary
        busy = true; status = L("Audio vorbereiten …")
        guard update(id, { $0.state = .processing; $0.error = nil }) else { busy = activeImportID != nil; return }
        task = Task { [self] in
            let temp = library.folder(id).appendingPathComponent("processing.wav")
            defer { try? FileManager.default.removeItem(at: temp); busy = activeImportID != nil; task = nil }
            do {
                let start = Date()
                let checkpoint = try await ConferenceTranscriber.run(source: source, folder: library.folder(id), engine: engine, settings: config, vocabulary: terms) { [weak self] checkpoint in
                    guard let store = self else { throw CancellationError() }
                    try await MainActor.run {
                        let saved = store.update(id) { record in
                            let edits = Dictionary(uniqueKeysWithValues: record.segments.filter { $0.text != $0.originalText || $0.speakerLocked }.map { ($0.id, $0) })
                            record.originalSegments = checkpoint.segments
                            record.segments = checkpoint.segments.map { segment in
                                if let edited = edits[segment.id] { return edited }
                                var copy = segment; copy.text = TranscriptEditor.applyVocabulary(copy.text, entries: terms); return copy
                            }
                            record.duration = checkpoint.duration; record.transcriptionProgress = checkpoint.completed / checkpoint.duration
                        }
                        guard saved else { throw LocalEngineError(L("Transkript-Zwischenstand konnte nicht gespeichert werden.")) }
                        store.status = L("Lokal transkribieren · \(Exporter.timestamp(checkpoint.completed)) / \(Exporter.timestamp(checkpoint.duration)) gespeichert")
                    }
                }
                let duration = checkpoint.duration
                try Task.checkCancellation()
                preparedModelKey = [config.runtimeDirectory, config.engine.rawValue, config.modelPaths[config.engine.rawValue] ?? ""].joined(separator: "\n")
                modelStatus = "\(config.engine.label) bereit"
                let originalSegments = checkpoint.segments
                let segments = recordings.first(where: { $0.id == id })?.segments ?? originalSegments
                guard update(id, {
                    $0.engine = config.engine.label; $0.audioPreparationSeconds = checkpoint.preparationSeconds
                    $0.modelLoadSeconds = checkpoint.loadSeconds; $0.transcriptionSeconds = checkpoint.decodeSeconds
                    $0.processingSeconds = Date().timeIntervalSince(start); $0.state = .complete; $0.transcriptionProgress = nil
                    if $0.conference == nil { $0.conference = ConferenceContext(); $0.conference?.language = config.language }
                    if $0.conference?.chapters.isEmpty == true {
                        $0.conference?.chapters = ConferenceLayout.chapters(segments: $0.segments, source: $0.source?.chapters ?? [])
                    }
                }) else { throw LocalEngineError(L("Transkript konnte nicht gespeichert werden.")) }
                var speakerWarning: String?
                if config.speakerDetectionEnabled && !segments.isEmpty && item.editHistory?.undo.isEmpty != false {
                    status = L("Transkript gespeichert · Sprecher werden erkannt …")
                    do {
                        let speakerStart = Date()
                        _ = try await AudioFiles.convert(source, to: temp)
                        let directory = URL(fileURLWithPath: config.runtimeDirectory).appendingPathComponent("models/diarization")
                        if let turns = try await analyzer.analyzeIfEnabled(temp, directory: directory, enabled: config.speakerDetectionEnabled, progress: { [weak self] current, total in
                            Task { @MainActor in self?.status = L("Sprecher erkennen · \(current)/\(total)") }
                        }) {
                            try Task.checkCancellation()
                            guard edit(id, label: L("Automatische Sprechererkennung"), change: { TranscriptLayout.applyInitialSpeakers(turns, source: originalSegments, vocabulary: terms, to: &$0) }) else { throw LocalEngineError(L("Sprecherzuordnung konnte nicht gespeichert werden.")) }
                            update(id) { $0.diarizationSeconds = Date().timeIntervalSince(speakerStart) }
                        }
                    } catch {
                        speakerWarning = Task.isCancelled ? L("Sprechererkennung abgebrochen. Das Transkript bleibt erhalten.") : L("Transkript gespeichert, Sprechererkennung nicht abgeschlossen: \(error.localizedDescription)")
                        update(id) { $0.error = speakerWarning; $0.state = .complete }
                    }
                }
                update(id) {
                    $0.processingSeconds = Date().timeIntervalSince(start)
                    if item.conference?.chapters.isEmpty != false {
                        $0.conference?.chapters = ConferenceLayout.chapters(segments: $0.segments, source: $0.source?.chapters ?? [])
                    }
                }
                status = speakerWarning ?? L("Transkription fertig · \(String(format: "%.1f", Date().timeIntervalSince(start))) s für \(Exporter.timestamp(duration)) Audio")
                if Task.isCancelled { return }
                if let target, let finished = recordings.first(where: { $0.id == id }) {
                    if !TextInsertion.insert(finished.plainText, into: target, anchor: anchor) { self.error = L("Automatisches Einfügen war nicht möglich oder die Textposition hat sich geändert. Dein Diktat ist in Laut gespeichert.") }
                }
                if item.kind == .dictation && !config.keepDictationAudio { removeAudio(id) }
            } catch {
                let message = Task.isCancelled ? L("Abgebrochen. Die Quelldatei bleibt erhalten.") : error.localizedDescription
                if !engine.warmWorker.isRunning { preparedModelKey = nil; modelStatus = L("Modell nicht vorbereitet") }
                update(id) { $0.state = .failed; $0.error = message }; status = message
            }
        }
    }
    func diarize(_ id: UUID, count: Int?) {
        guard !busy, let item = recordings.first(where: { $0.id == id }) else { return }
        let source: URL
        do { source = try library.requireAudio(item) } catch { self.error = error.localizedDescription; return }
        guard settings.diarizationInstalled else { section = "models"; error = L("Bitte das Modell für Sprechererkennung zuerst herunterladen."); return }
        busy = true; status = L("Sprecheranalyse vorbereiten …")
        task = Task { [self] in
            let temp = library.folder(id).appendingPathComponent("speakers.wav")
            defer { try? FileManager.default.removeItem(at: temp); busy = false; task = nil }
            do {
                _ = try await AudioFiles.convert(source, to: temp)
                let began = Date()
                let turns = try await analyzer.analyze(temp, directory: modelDirectory, count: count) { [weak self] current, total in
                    Task { @MainActor in self?.status = L("Sprecher analysieren · \(current)/\(total)") }
                }
                try Task.checkCancellation()
                let saved = edit(id, label: L("Sprecheranalyse")) { record in
                    TranscriptLayout.applySpeakers(turns, to: &record)
                }
                guard saved else { throw LocalEngineError(L("Sprecherkorrekturen konnten nicht gespeichert werden.")) }
                update(id) { $0.diarizationSeconds = Date().timeIntervalSince(began); $0.error = nil }
                status = L("Sprecheranalyse fertig. Namen und Zuordnungen kannst du jetzt korrigieren.")
            } catch { self.error = error.localizedDescription; status = L("Sprecheranalyse nicht abgeschlossen") }
        }
    }
    func download(_ model: EngineKind) {
        perform(L("\(model.label) wird heruntergeladen …")) {
            let path = try await self.engine.download(model, settings: self.settings, progress: self.downloadReporter)
            self.settings.modelPaths[model.rawValue] = path; self.saveSettings()
        }
    }
    func downloadSpeakers() {
        perform(L("Sprechermodelle herunterladen und vorbereiten …")) {
            try await self.analyzer.download(to: self.modelDirectory)
            self.settings.diarizationInstalled = true; self.saveSettings()
        }
    }
    func downloadLLM() {
        perform(L("Textmodell herunterladen …")) {
            self.settings.llmModelPath = try await self.engine.downloadModel(self.settings.llmModelID, settings: self.settings, progress: self.downloadReporter); self.saveSettings()
        }
    }
    func perform(_ message: String, operation: @escaping @MainActor () async throws -> Void) {
        guard !busy, !isRecording else { return }; busy = true; status = message; downloadProgress = ""; downloadGeneration = UUID()
        preparedModelKey = nil; modelStatus = L("Modell durch anderen Vorgang belegt")
        task = Task {
            defer { busy = false; task = nil; downloadGeneration = UUID(); downloadProgress = ""; prepareSelectedModel() }
            do { try await operation(); try Task.checkCancellation(); status = L("Fertig · lokal gespeichert") }
            catch { self.error = error.localizedDescription; status = L("Vorgang nicht abgeschlossen") }
        }
    }
    func cancel() {
        if maintenanceRunning { storageTask?.cancel(); return }
        if activeImportID != nil { pauseImportQueue(); return }
        automaticPreparation = false; preparedModelKey = nil; modelStatus = L("Modell nicht vorbereitet")
        task?.cancel(); engine.runner.cancel(); engine.warmWorker.stop(); status = L("Abbruch angefordert …")
    }
    func chooseDirectory(_ apply: (String) -> Void) {
        let panel = NSOpenPanel(); panel.canChooseDirectories = true; panel.canChooseFiles = false
        if panel.runModal() == .OK, let url = panel.url { apply(url.path); saveSettings() }
    }
    func refine(_ id: UUID, template: TextTemplate = .custom) {
        guard let record = recordings.first(where: { $0.id == id }) else { return }
        var config = settings
        if let instructions = template.instructions { config.customInstructions = instructions }
        perform(L("Text mit lokalem Modell bearbeiten …")) {
            let source = record.kind == .note ? record.notes : record.plainText + (record.notes.isEmpty ? "" : L("\n\nEigene Notizen:\n") + record.notes)
            let text = try await self.engine.refine(source, settings: config)
            try Task.checkCancellation()
            guard self.edit(id, label: L("KI-Bearbeitung"), change: { $0.refinedText = text }) else { throw LocalEngineError(L("Textbearbeitung konnte nicht gespeichert werden.")) }
        }
    }
    func play(_ recording: Recording, at time: Double? = nil) {
        guard preparePlayback(recording) else { return }
        if let time { seek(recording, to: time); player?.playImmediately(atRate: playbackRate) }
        else if (player?.rate ?? 0) != 0 { player?.pause() }
        else {
            if playerTime >= max(playbackDuration, recording.duration) - 0.1 { seek(recording, to: 0) }
            player?.playImmediately(atRate: playbackRate)
        }
    }
    private func preparePlayback(_ recording: Recording) -> Bool {
        let url: URL
        do { url = try library.requireAudio(recording) } catch { self.error = error.localizedDescription; return false }
        if playbackID != recording.id {
            stopPlayback(); player = AVPlayer(url: url); playbackID = recording.id; playbackDuration = recording.duration
        }
        if let failure = player?.currentItem?.error { error = failure.localizedDescription; return false }
        return true
    }
    func playbackIsLoaded(_ id: UUID) -> Bool { playbackID == id }
    func seek(_ recording: Recording, to time: Double) {
        guard time.isFinite, preparePlayback(recording) else { return }
        let bounded = min(max(0, time), max(playbackDuration, recording.duration, 0))
        playerTime = bounded
        player?.seek(to: CMTime(seconds: bounded, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero)
    }
    func skip(_ recording: Recording, by seconds: Double) { seek(recording, to: playerTime + seconds) }
    func jumpSegment(_ recording: Recording, forward: Bool) {
        if let start = TranscriptNavigation.adjacentStart(from: playerTime, forward: forward, in: recording.segments) { play(recording, at: start) }
    }
    func setPlaybackRate(_ rate: Float) { playbackRate = rate; if playing { player?.rate = rate } }
    func stopPlayback() { player?.pause(); player = nil; playbackID = nil; playerTime = 0; playbackDuration = 0; playing = false }
    func export(_ record: Recording, format: String) {
        let panel = NSSavePanel(); panel.nameFieldStringValue = record.title + "." + format
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let data: Data
            if format == "json" { let encoder = JSONEncoder(); encoder.outputFormatting = .prettyPrinted; var exported = record; exported.editHistory = nil; data = try encoder.encode(exported) }
            else { data = Data((format == "srt" ? Exporter.srt(record) : format == "md" ? Exporter.markdown(record) : (record.kind == .note ? record.notes : record.plainText)).utf8) }
            try data.write(to: url, options: .atomic)
        } catch { self.error = error.localizedDescription }
    }
    func delete(_ record: Recording) {
        guard !busy else { return }
        do { stopPlayback(); try library.delete(record); recordings.removeAll { $0.id == record.id }; selected = recordings.first?.id }
        catch { self.error = error.localizedDescription }
    }
    func removeAudio(_ id: UUID) {
        guard let i = recordings.firstIndex(where: { $0.id == id }) else { return }
        do { stopPlayback(); try library.deleteAudio(&recordings[i]) } catch { self.error = error.localizedDescription }
    }
}
