import SwiftUI
import AppKit
import AVFoundation
import UniformTypeIdentifiers
import LautCore
import LautAudio

@MainActor
final class AppStore: ObservableObject {
    @Published var recordings: [Recording] = [] { didSet { searchRevision += 1; scheduleSearch() } }
    @Published var selected: UUID? { didSet { if selected != oldValue { stopPlayback() } } }
    @Published var section = "library"
    @Published var search = "" { didSet { scheduleSearch(show: true) } }
    @Published var settings = AppSettings()
    @Published var vocabulary: [VocabularyEntry] = []
    @Published var status = "Bereit · lokal auf deinem Mac"
    @Published var busy = false { didSet { scheduleSearch() } }
    @Published var error: String?
    @Published var isRecording = false { didSet { scheduleSearch() } }
    @Published var playerTime: Double = 0
    @Published var playing = false
    @Published var playbackDuration: Double = 0
    @Published var playbackRate: Float = 1
    @Published var importing = false
    @Published var importStatus = ""
    @Published var modelStatus = "Modell nicht vorbereitet"
    private var preparedModelKey: String?
    private var observedModelKey: String?
    private var automaticPreparation = true
    let library: Library
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
    private var importTask: Task<Void, Never>?
    private var importQueue: [URL] = []
    private let importer = FileImporter()
    private var timer: Timer?
    var recorder: AVAudioRecorder?
    var captureURL: URL?
    var capturedKind: DocumentKind = .meeting
    var captureTarget: NSRunningApplication?
    var captureAnchor: TextInsertion.Anchor?
    var meeting: MeetingCapture?
    var meetingDirectory: URL?
    var shortcuts: GlobalShortcuts?
    @Published private(set) var pending: [UUID] = []

    init() {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Laut")
        // Failure to open the real library must never silently create a different one.
        do { library = try Library(root: base) } catch { fatalError("Bibliothek nicht erreichbar: \(error)") }
        let resource = Bundle.main.resourceURL ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("Resources")
        searchIndex = SearchIndex(url: base.appendingPathComponent("search-v1.sqlite"))
        embeddings = EmbeddingEngine(script: resource.appendingPathComponent("embedding_worker.py"))
        engine = TranscriptionEngine(worker: resource.appendingPathComponent("mlx_worker.py"))
        do {
            recordings = try library.load()
            if !library.loadWarnings.isEmpty { self.error = library.loadWarnings.joined(separator: "\n") }
            settings = try library.loadSettings(); vocabulary = try library.loadVocabulary()
        } catch { self.error = "Lokale Daten konnten nicht vollständig gelesen werden: \(error.localizedDescription)" }
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
                    self.stopPlayback(); self.error = "Audio konnte nicht abgespielt werden: \(failure.localizedDescription)"; return
                }
                let position = self.player?.currentTime().seconds ?? 0
                if position.isFinite, abs(self.playerTime - position) > 0.04 { self.playerTime = position }
                let isPlaying = (self.player?.rate ?? 0) != 0
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
    }
    private var selectedModelKey: String {
        [settings.runtimeDirectory, settings.engine.rawValue, settings.modelPaths[settings.engine.rawValue] ?? ""].joined(separator: "\n")
    }
    var modelReady: Bool { preparedModelKey == selectedModelKey && engine.warmWorker.isRunning }
    func prepareSelectedModel(force: Bool = false) {
        if force { automaticPreparation = true }
        guard automaticPreparation, !busy, !isRecording else { return }
        if modelReady { return }
        guard let path = settings.modelPaths[settings.engine.rawValue], FileManager.default.fileExists(atPath: path) else {
            modelStatus = "Bitte ein Sprachmodell installieren"; return
        }
        let config = settings, key = selectedModelKey
        busy = true; preparedModelKey = nil
        modelStatus = "\(config.engine.label) wird geladen und aufgewärmt …"
        status = "Modell vorbereiten · einmalig vor der ersten Aufnahme"
        task = Task {
            defer { busy = false; task = nil }
            do {
                let began = Date()
                try await engine.preload(settings: config)
                try Task.checkCancellation()
                guard selectedModelKey == key else { modelStatus = "Modellwahl geändert · bitte vorladen"; return }
                preparedModelKey = key
                modelStatus = "\(config.engine.label) bereit"
                status = "Modell in \(String(format: "%.1f", Date().timeIntervalSince(began))) s vorbereitet · bereit für Aufnahmen"
            } catch {
                automaticPreparation = false
                modelStatus = "Modell nicht vorbereitet"
                status = Task.isCancelled ? "Vorbereitung abgebrochen" : "Modellvorbereitung fehlgeschlagen"
                if !Task.isCancelled { self.error = error.localizedDescription }
            }
        }
    }
    func unloadModel() {
        guard !busy, !isRecording else { return }
        automaticPreparation = false; preparedModelKey = nil
        engine.warmWorker.stop(); modelStatus = "Modell entladen"; status = "Arbeitsspeicher freigegeben"
    }
    var current: Recording? { recordings.first { $0.id == selected } }
    var modelDirectory: URL { URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("models/diarization") }
    var filtered: [Recording] { recordings }
    func saveSettings() {
        do { try library.saveSettings(settings) } catch { self.error = error.localizedDescription }
        if observedModelKey != selectedModelKey {
            observedModelKey = selectedModelKey; preparedModelKey = nil; automaticPreparation = true
            modelStatus = "Modell nicht vorbereitet"; prepareSelectedModel()
        }
    }
    func saveVocabulary() { do { try library.saveVocabulary(vocabulary) } catch { self.error = error.localizedDescription } }
    @discardableResult func update(_ id: UUID, _ edit: (inout Recording) -> Void) -> Bool {
        guard let index = recordings.firstIndex(where: { $0.id == id }) else { return false }
        var candidate = recordings[index]; edit(&candidate)
        return commit(candidate, at: index)
    }
    @discardableResult private func commit(_ candidate: Recording, at index: Int) -> Bool {
        do { try library.save(candidate); recordings[index] = candidate; return true }
        catch { self.error = "Änderung konnte nicht gespeichert werden. Der letzte gespeicherte Stand bleibt erhalten. \(error.localizedDescription)"; return false }
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
        guard let result else { error = "Diese Änderung passt nicht mehr zum aktuellen Dokumentstand."; return }
        commit(result, at: index)
    }
    func chooseFiles() {
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = true
        panel.allowedContentTypes = [.audio, .movie]; panel.canChooseDirectories = false
        guard panel.runModal() == .OK else { return }; importFiles(panel.urls)
    }
    func importFiles(_ urls: [URL]) {
        importQueue += urls
        section = "library"
        guard importTask == nil else { return }
        importing = true
        importTask = Task {
            defer {
                importing = false; importTask = nil; importStatus = ""
                if !importQueue.isEmpty { importFiles([]) }
            }
            var failures: [String] = []
            while !importQueue.isEmpty && !Task.isCancelled {
                let url = importQueue.removeFirst()
                importStatus = "\(url.lastPathComponent) prüfen …"
                do {
                    let item = try await importer.importFile(url, root: library.root) { [weak self] fraction in
                        Task { @MainActor in self?.importStatus = "\(url.lastPathComponent) kopieren · \(Int(fraction * 100)) %" }
                    }
                    recordings.insert(item, at: 0); selected = item.id
                } catch {
                    if Task.isCancelled { break }
                    failures.append(url.lastPathComponent + ": " + error.localizedDescription)
                }
            }
            if !failures.isEmpty { error = failures.joined(separator: "\n") }
        }
    }
    func cancelImport() { importQueue = []; importTask?.cancel() }
    func newNote() {
        let item = Recording(title: "Schnellnotiz", kind: .note)
        do { try library.save(item); recordings.insert(item, at: 0); selected = item.id; section = "library" }
        catch { self.error = error.localizedDescription }
    }
    func transcribeAll() {
        guard !busy else { return }
        pending = recordings.filter { $0.audioFilename != nil && $0.segments.isEmpty }.map(\.id).reversed()
        nextJob()
    }
    private func nextJob() { guard !pending.isEmpty else { return }; let id = pending.removeFirst(); transcribe(id) }
    func transcribe(_ id: UUID, pasteTo target: NSRunningApplication? = nil, anchor: TextInsertion.Anchor? = nil) {
        guard !busy, !isRecording, let item = recordings.first(where: { $0.id == id }), let source = library.audioURL(item) else { return }
        // Re-running creates a separate version so no correction or source result is overwritten.
        if !item.segments.isEmpty {
            busy = true; status = "Neue Version vorbereiten …"
            task = Task {
                var copy: Recording?
                do {
                    copy = try await importer.importFile(source, root: library.root, title: item.title + " · neue Transkription") { [weak self] fraction in
                        Task { @MainActor in self?.status = "Neue Version kopieren · \(Int(fraction * 100)) %" }
                    }
                    try Task.checkCancellation()
                    guard let copy else { throw LocalEngineError("Neue Version konnte nicht angelegt werden.") }
                    recordings.insert(copy, at: 0); selected = copy.id
                    busy = false; task = nil; transcribe(copy.id)
                } catch {
                    if let copy { try? library.delete(copy) }
                    busy = false; task = nil
                    status = Task.isCancelled ? "Abgebrochen. Das vorhandene Transkript bleibt erhalten." : "Neue Version konnte nicht angelegt werden."
                    if !Task.isCancelled { self.error = error.localizedDescription }
                }
            }
            return
        }
        let config = settings, terms = vocabulary
        busy = true; status = "Audio vorbereiten …"
        guard update(id, { $0.state = .processing; $0.error = nil }) else { busy = false; pending = []; return }
        task = Task {
            let temp = library.folder(id).appendingPathComponent("processing.wav")
            defer { try? FileManager.default.removeItem(at: temp); busy = false; task = nil; nextJob() }
            do {
                let start = Date()
                let duration = try await AudioFiles.convert(source, to: temp)
                let preparationSeconds = Date().timeIntervalSince(start)
                try Task.checkCancellation(); status = "\(config.engine.label) transkribiert lokal …"
                let result = try await engine.transcribe(audio: temp, settings: config, vocabulary: terms)
                try Task.checkCancellation()
                preparedModelKey = [config.runtimeDirectory, config.engine.rawValue, config.modelPaths[config.engine.rawValue] ?? ""].joined(separator: "\n")
                modelStatus = "\(config.engine.label) bereit"
                let originalSegments = result.editorSegments()
                var segments = originalSegments
                for i in segments.indices { segments[i].text = TranscriptEditor.applyVocabulary(segments[i].text, entries: terms) }
                let saved = update(id) {
                    $0.segments = segments; $0.originalSegments = originalSegments; $0.duration = duration; $0.engine = config.engine.label
                    $0.processingSeconds = Date().timeIntervalSince(start); $0.audioPreparationSeconds = preparationSeconds
                    $0.modelLoadSeconds = result.load_seconds; $0.transcriptionSeconds = result.decode_seconds; $0.state = .complete
                }
                guard saved else { throw LocalEngineError("Transkript konnte nicht gespeichert werden. Die Audiodatei bleibt erhalten.") }
                var speakerWarning: String?
                if config.speakerDetectionEnabled && !segments.isEmpty {
                    status = "Transkript gespeichert · Sprecher werden erkannt …"
                    do {
                        let speakerStart = Date()
                        let directory = URL(fileURLWithPath: config.runtimeDirectory).appendingPathComponent("models/diarization")
                        if let turns = try await analyzer.analyzeIfEnabled(temp, directory: directory, enabled: config.speakerDetectionEnabled, progress: { [weak self] current, total in
                            Task { @MainActor in self?.status = "Sprecher erkennen · \(current)/\(total)" }
                        }) {
                            try Task.checkCancellation()
                            guard edit(id, label: "Automatische Sprechererkennung", change: { TranscriptLayout.applyInitialSpeakers(turns, source: originalSegments, vocabulary: terms, to: &$0) }) else { throw LocalEngineError("Sprecherzuordnung konnte nicht gespeichert werden.") }
                            update(id) { $0.diarizationSeconds = Date().timeIntervalSince(speakerStart) }
                        }
                    } catch {
                        speakerWarning = Task.isCancelled ? "Sprechererkennung abgebrochen. Das Transkript bleibt erhalten." : "Transkript gespeichert, Sprechererkennung nicht abgeschlossen: \(error.localizedDescription)"
                        update(id) { $0.error = speakerWarning; $0.state = .complete }
                    }
                }
                update(id) { $0.processingSeconds = Date().timeIntervalSince(start) }
                status = speakerWarning ?? "Transkription fertig · \(String(format: "%.1f", Date().timeIntervalSince(start))) s für \(Exporter.timestamp(duration)) Audio"
                if Task.isCancelled { return }
                if let target, let finished = recordings.first(where: { $0.id == id }) {
                    if !TextInsertion.insert(finished.plainText, into: target, anchor: anchor) { self.error = "Automatisches Einfügen war nicht möglich oder die Textposition hat sich geändert. Dein Diktat ist in Laut gespeichert." }
                }
                if item.kind == .dictation && !config.keepDictationAudio { removeAudio(id) }
            } catch {
                let message = Task.isCancelled ? "Abgebrochen. Die Quelldatei bleibt erhalten." : error.localizedDescription
                if !engine.warmWorker.isRunning { preparedModelKey = nil; modelStatus = "Modell nicht vorbereitet" }
                update(id) { $0.state = .failed; $0.error = message }; status = message
            }
        }
    }
    func diarize(_ id: UUID, count: Int?) {
        guard !busy, let item = recordings.first(where: { $0.id == id }), let source = library.audioURL(item) else { return }
        guard settings.diarizationInstalled else { section = "models"; error = "Bitte das Modell für Sprechererkennung zuerst herunterladen."; return }
        busy = true; status = "Sprecheranalyse vorbereiten …"
        task = Task {
            let temp = library.folder(id).appendingPathComponent("speakers.wav")
            defer { try? FileManager.default.removeItem(at: temp); busy = false; task = nil }
            do {
                _ = try await AudioFiles.convert(source, to: temp)
                let began = Date()
                let turns = try await analyzer.analyze(temp, directory: modelDirectory, count: count) { [weak self] current, total in
                    Task { @MainActor in self?.status = "Sprecher analysieren · \(current)/\(total)" }
                }
                try Task.checkCancellation()
                let saved = edit(id, label: "Sprecheranalyse") { record in
                    TranscriptLayout.applySpeakers(turns, to: &record)
                }
                guard saved else { throw LocalEngineError("Sprecherkorrekturen konnten nicht gespeichert werden.") }
                update(id) { $0.diarizationSeconds = Date().timeIntervalSince(began); $0.error = nil }
                status = "Sprecheranalyse fertig. Namen und Zuordnungen kannst du jetzt korrigieren."
            } catch { self.error = error.localizedDescription; status = "Sprecheranalyse nicht abgeschlossen" }
        }
    }
    func download(_ model: EngineKind) {
        perform("\(model.label) wird heruntergeladen …") {
            let path = try await self.engine.download(model, settings: self.settings, progress: self.downloadReporter)
            self.settings.modelPaths[model.rawValue] = path; self.saveSettings()
        }
    }
    func downloadSpeakers() {
        perform("Sprechermodelle herunterladen und vorbereiten …") {
            try await self.analyzer.download(to: self.modelDirectory)
            self.settings.diarizationInstalled = true; self.saveSettings()
        }
    }
    func downloadLLM() {
        perform("Textmodell herunterladen …") {
            self.settings.llmModelPath = try await self.engine.downloadModel(self.settings.llmModelID, settings: self.settings, progress: self.downloadReporter); self.saveSettings()
        }
    }
    func perform(_ message: String, operation: @escaping @MainActor () async throws -> Void) {
        guard !busy, !isRecording else { return }; busy = true; status = message; downloadProgress = ""; downloadGeneration = UUID()
        preparedModelKey = nil; modelStatus = "Modell durch anderen Vorgang belegt"
        task = Task {
            defer { busy = false; task = nil; downloadGeneration = UUID(); downloadProgress = ""; prepareSelectedModel() }
            do { try await operation(); try Task.checkCancellation(); status = "Fertig · lokal gespeichert" }
            catch { self.error = error.localizedDescription; status = "Vorgang nicht abgeschlossen" }
        }
    }
    func cancel() {
        automaticPreparation = false; preparedModelKey = nil; modelStatus = "Modell nicht vorbereitet"
        pending = []; task?.cancel(); engine.runner.cancel(); engine.warmWorker.stop(); status = "Abbruch angefordert …"
    }
    func chooseDirectory(_ apply: (String) -> Void) {
        let panel = NSOpenPanel(); panel.canChooseDirectories = true; panel.canChooseFiles = false
        if panel.runModal() == .OK, let url = panel.url { apply(url.path); saveSettings() }
    }
    func refine(_ id: UUID, template: TextTemplate = .custom) {
        guard let record = recordings.first(where: { $0.id == id }) else { return }
        var config = settings
        if let instructions = template.instructions { config.customInstructions = instructions }
        perform("Text mit lokalem Modell bearbeiten …") {
            let source = record.kind == .note ? record.notes : record.plainText + (record.notes.isEmpty ? "" : "\n\nEigene Notizen:\n" + record.notes)
            let text = try await self.engine.refine(source, settings: config)
            try Task.checkCancellation()
            guard self.edit(id, label: "KI-Bearbeitung", change: { $0.refinedText = text }) else { throw LocalEngineError("Textbearbeitung konnte nicht gespeichert werden.") }
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
        guard let url = library.audioURL(recording) else { return false }
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
