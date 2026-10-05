import Foundation
import LautCore
import LautAudio

/// Uses a caller-supplied fixture in an isolated temporary library, never the user's library.
enum FileWorkflow {
    static func check(source: URL, project: URL) async throws {
        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("laut-workflow-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: temporary) }
        let library = try Library(root: temporary.appendingPathComponent("library"))
        let importer = FileImporter()
        var record = try await importer.importFile(source, root: library.root) { _ in }
        guard let copied = library.audioURL(record), record.duration > 0,
              FileManager.default.contentsEqual(atPath: source.path, andPath: copied.path) else { throw LocalEngineError("Imported audio differs from source") }
        print("Import: \(record.duration) seconds, source preserved")

        let invalid = temporary.appendingPathComponent("invalid.wav")
        try Data("Not an audio file".utf8).write(to: invalid)
        var rejected = false
        do { _ = try await importer.importFile(invalid, root: library.root) { _ in } } catch { rejected = true }
        guard rejected else { throw LocalEngineError("Invalid audio accepted") }

        let cancelled = Task {
            try await importer.importFile(source, root: library.root) { _ in
                withUnsafeCurrentTask { $0?.cancel() }
            }
        }
        var didCancel = false
        do { _ = try await cancelled.value } catch is CancellationError { didCancel = true }
        let folders = try FileManager.default.contentsOfDirectory(at: library.root, includingPropertiesForKeys: nil)
        guard didCancel, folders.count == 1, FileManager.default.contentsEqual(atPath: source.path, andPath: copied.path) else {
            throw LocalEngineError("Cancelled import left partial files or changed its source")
        }
        print("Invalid file rejected; cancelled import cleaned up")

        let wav = temporary.appendingPathComponent("converted.wav")
        let duration = try await AudioFiles.convert(copied, to: wav)
        var settings = AppSettings()
        settings.runtimeDirectory = project.appendingPathComponent(".runtime").path
        settings.modelPaths["phonon"] = settings.runtimeDirectory + "/models/speech/FermionResearch__Phonon-2/model_phonon2_c4c_int6"
        let engine = TranscriptionEngine(worker: project.appendingPathComponent("Resources/mlx_worker.py"))
        defer { engine.warmWorker.stop() }
        try await engine.preload(settings: settings)
        let began = Date()
        let result = try await engine.transcribe(audio: wav, settings: settings, vocabulary: [])
        record.segments = result.editorSegments(); record.originalSegments = record.segments
        record.duration = duration; record.state = .complete
        guard !record.segments.isEmpty else { throw LocalEngineError("Empty transcript") }
        print("ASR: \(Date().timeIntervalSince(began)) seconds; \(record.segments.count) segments; last end \(record.segments.last!.end)")
        try library.save(record)

        let analyzer = SpeakerAnalyzer()
        let turns = try await analyzer.analyze(wav, directory: project.appendingPathComponent(".runtime/models/diarization"), count: nil) { _, _ in }
        guard !turns.isEmpty else { throw LocalEngineError("No speaker turns") }
        record = RecordingEditor.edit(record, label: "Sprecheranalyse") {
            $0.segments = TranscriptEditor.assign(turns, to: $0.segments)
            $0.speakers = Set(turns.map(\.speakerID)).sorted().map { Speaker(id: $0, name: $0) }
        }
        print("Diarization: \(record.speakers.count) speakers; \(turns.count) turns")
        let automatic = record.segments
        record = RecordingEditor.edit(record, label: "Manuelle Korrektur") {
            $0.speakers.append(Speaker(id: "manual", name: "Testperson"))
            $0.segments[0].speakerID = "manual"; $0.segments[0].speakerLocked = true
            $0.segments[0].text = "Manuell korrigierter Fachbegriff"
        }
        guard TranscriptEditor.assign(turns, to: record.segments).first == record.segments.first else {
            throw LocalEngineError("Reanalysis overwrote a manual correction")
        }
        try library.save(record)
        let reopened = try Library(root: library.root).load()[0]
        guard reopened.segments == record.segments, reopened.speakers == record.speakers,
              let undone = RecordingEditor.undo(reopened), undone.segments == automatic,
              let redone = RecordingEditor.redo(undone), redone.segments == record.segments,
              !Exporter.srt(redone).isEmpty else { throw LocalEngineError("Save/reopen/undo/redo/export failed") }
        print("Workflow passed: import → ASR → speakers → manual correction → save/reopen → undo/redo → export")
    }
}
