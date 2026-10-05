import SwiftUI
import AppKit
import AVFoundation
import UniformTypeIdentifiers
import LautCore
import LautAudio

@MainActor
final class AppStore: ObservableObject {
    @Published var recordings: [Recording] = []
    @Published var selected: UUID?
    @Published var section = "library"
    @Published var search = ""
    @Published var settings = AppSettings()
    @Published var vocabulary: [VocabularyEntry] = []
    @Published var status = "Bereit · lokal auf deinem Mac"
    @Published var busy = false
    @Published var error: String?
    @Published var isRecording = false
    @Published var playerTime: Double = 0
    @Published var playing = false
    @Published var modelStatus = "Modell nicht vorbereitet"
    private var preparedModelKey: String?
    private var observedModelKey: String?
    private var automaticPreparation = true
    let library: Library
    let engine: TranscriptionEngine
    let analyzer = SpeakerAnalyzer()
    var task: Task<Void, Never>?
    var player: AVAudioPlayer?
    private var timer: Timer?
    var recorder: AVAudioRecorder?
    var captureURL: URL?
    var capturedKind: DocumentKind = .meeting
    var captureTarget: NSRunningApplication?
    var captureAnchor: TextInsertion.Anchor?
    var meeting: MeetingCapture?
    var meetingDirectory: URL?
    var shortcuts: GlobalShortcuts?
    private var pending: [UUID] = []

    init() {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Laut")
        // Failure to open the real library must never silently create a different one.
        do { library = try Library(root: base) } catch { fatalError("Bibliothek nicht erreichbar: \(error)") }
        let resource = Bundle.main.resourceURL ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("Resources")
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
        timer = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.playerTime = self.player?.currentTime ?? 0; self.playing = self.player?.isPlaying ?? false
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
    var filtered: [Recording] { recordings.filter { search.isEmpty || ($0.title + " " + $0.plainText + " " + $0.notes).localizedCaseInsensitiveContains(search) } }
    func saveSettings() {
        do { try library.saveSettings(settings) } catch { self.error = error.localizedDescription }
        if observedModelKey != selectedModelKey {
            observedModelKey = selectedModelKey; preparedModelKey = nil; automaticPreparation = true
            modelStatus = "Modell nicht vorbereitet"; prepareSelectedModel()
        }
    }
    func saveVocabulary() { do { try library.saveVocabulary(vocabulary) } catch { self.error = error.localizedDescription } }
    func update(_ id: UUID, _ edit: (inout Recording) -> Void) {
        guard let index = recordings.firstIndex(where: { $0.id == id }) else { return }
        edit(&recordings[index])
        do { try library.save(recordings[index]) } catch { self.error = error.localizedDescription }
    }
    func chooseFiles() {
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = true
        panel.allowedContentTypes = [.audio, .movie]; panel.canChooseDirectories = false
        guard panel.runModal() == .OK else { return }; importFiles(panel.urls)
    }
    func importFiles(_ urls: [URL]) {
        for url in urls {
            let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
            do { let item = try library.importFile(url); recordings.insert(item, at: 0); selected = item.id }
            catch { self.error = error.localizedDescription }
        }
        section = "library"
    }
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
            do {
                var copy = try library.importFile(source); copy.title = item.title + " · neue Transkription"
                try library.save(copy); recordings.insert(copy, at: 0); selected = copy.id; transcribe(copy.id)
            } catch { self.error = error.localizedDescription }; return
        }
        let config = settings, terms = vocabulary
        busy = true; status = "Audio vorbereiten …"
        update(id) { $0.state = .processing; $0.error = nil }
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
                var segments = result.editorSegments()
                for i in segments.indices { segments[i].text = TranscriptEditor.applyVocabulary(segments[i].text, entries: terms) }
                update(id) {
                    $0.segments = segments; $0.originalSegments = result.editorSegments(); $0.duration = duration; $0.engine = config.engine.label
                    $0.processingSeconds = Date().timeIntervalSince(start); $0.audioPreparationSeconds = preparationSeconds
                    $0.modelLoadSeconds = result.load_seconds; $0.transcriptionSeconds = result.decode_seconds; $0.state = .complete
                }
                status = "Transkription fertig · \(String(format: "%.1f", Date().timeIntervalSince(start))) s für \(Exporter.timestamp(duration)) Audio"
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
        preparedModelKey = nil; modelStatus = "Sprachmodell pausiert während Sprecheranalyse"
        engine.warmWorker.stop()
        task = Task {
            let temp = library.folder(id).appendingPathComponent("speakers.wav")
            defer { try? FileManager.default.removeItem(at: temp); busy = false; task = nil; prepareSelectedModel() }
            do {
                _ = try await AudioFiles.convert(source, to: temp)
                let turns = try await analyzer.analyze(temp, directory: modelDirectory, count: count) { [weak self] current, total in
                    Task { @MainActor in self?.status = "Sprecher analysieren · \(current)/\(total)" }
                }
                try Task.checkCancellation()
                // Cluster IDs are arbitrary per run. Fresh IDs cannot steal manually named speakers.
                let rawIDs = Array(Set(turns.map(\.speakerID))).sorted()
                let speakers = rawIDs.enumerated().map { Speaker(name: "Sprecher \($0.offset + 1)") }
                let mapping = Dictionary(uniqueKeysWithValues: zip(rawIDs, speakers.map(\.id)))
                let mapped = turns.map { SpeakerTurn(speakerID: mapping[$0.speakerID]!, start: $0.start, end: $0.end) }
                update(id) { record in
                    record.segments = TranscriptEditor.assign(mapped, to: record.segments)
                    let retained = Set(record.segments.compactMap(\.speakerID))
                    record.speakers = (record.speakers + speakers).filter { retained.contains($0.id) }
                }
                status = "Sprecheranalyse fertig. Namen und Zuordnungen kannst du jetzt korrigieren."
            } catch { self.error = error.localizedDescription; status = "Sprecheranalyse nicht abgeschlossen" }
        }
    }
    func download(_ model: EngineKind) {
        perform("\(model.label) wird heruntergeladen …") {
            let path = try await self.engine.download(model, settings: self.settings)
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
            self.settings.llmModelPath = try await self.engine.downloadModel(self.settings.llmModelID, settings: self.settings); self.saveSettings()
        }
    }
    func perform(_ message: String, operation: @escaping @MainActor () async throws -> Void) {
        guard !busy, !isRecording else { return }; busy = true; status = message
        preparedModelKey = nil; modelStatus = "Modell durch anderen Vorgang belegt"
        task = Task {
            defer { busy = false; task = nil; prepareSelectedModel() }
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
    func refine(_ id: UUID) {
        guard let record = recordings.first(where: { $0.id == id }) else { return }
        perform("Text mit lokalem Modell bearbeiten …") {
            let text = try await self.engine.refine(record.kind == .note ? record.notes : record.plainText, settings: self.settings)
            try Task.checkCancellation(); self.update(id) { $0.refinedText = text }
        }
    }
    func play(_ recording: Recording, at time: Double? = nil) {
        guard let url = library.audioURL(recording) else { return }
        do {
            if player?.url != url { player = try AVAudioPlayer(contentsOf: url) }
            if let time { player?.currentTime = time; player?.play() }
            else if player?.isPlaying == true { player?.pause() } else { player?.play() }
        } catch { self.error = error.localizedDescription }
    }
    func stopPlayback() { player?.stop(); player = nil }
    func export(_ record: Recording, format: String) {
        let panel = NSSavePanel(); panel.nameFieldStringValue = record.title + "." + format
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let data: Data
            if format == "json" { let encoder = JSONEncoder(); encoder.outputFormatting = .prettyPrinted; data = try encoder.encode(record) }
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
