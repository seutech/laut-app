import Foundation
import LautCore
import LautAudio

enum RecoveryWorkflow {
    static func run(project: URL, arguments: [String]) async throws {
        guard let input = try DiagnosticConfiguration.value("--recovery-workflow", in: arguments),
              let output = try DiagnosticConfiguration.value("--output", in: arguments) else { throw LocalEngineError("Provide a synthetic audio fixture longer than 5 minutes and a new --output folder") }
        let root = URL(fileURLWithPath: output)
        let fm = FileManager.default
        guard !fm.fileExists(atPath: root.path) else { throw LocalEngineError("Output already exists") }
        let library = try Library(root: root.appendingPathComponent("original"))
        var record = try await FileImporter().importFile(URL(fileURLWithPath: input), root: library.root, title: "Recovery fixture") { _ in }
        guard record.duration > 300 else { throw LocalEngineError("Fixture must exceed five minutes") }
        let source = try library.requireAudio(record), folder = library.folder(record.id)
        let config = try DiagnosticConfiguration.settings(project: project, arguments: arguments)
        let engine = TranscriptionEngine(worker: project.appendingPathComponent("Resources/mlx_worker.py"))
        defer { engine.warmWorker.stop() }
        let checkpointFile = folder.appendingPathComponent("transcription-checkpoint.json")
        var interrupted = false
        do {
            _ = try await ConferenceTranscriber.run(source: source, folder: folder, engine: engine, settings: config, vocabulary: []) {
                if $0.completed >= 300 { throw CancellationError() }
            }
        } catch is CancellationError { interrupted = true }
        guard interrupted else { throw LocalEngineError("Expected interruption") }
        engine.warmWorker.stop() // Resume must start a fresh inference process.
        let saved = try JSONDecoder().decode(TranscriptionCheckpoint.self, from: Data(contentsOf: checkpointFile))
        guard saved.completed == 300, !saved.segments.isEmpty else { throw LocalEngineError("Expected persisted first chunk") }
        record.segments = saved.segments; record.originalSegments = saved.segments
        record.transcriptionProgress = saved.completed / saved.duration; record.state = .failed
        try library.save(record)
        _ = try LibraryBackup.create(from: library.root, to: root.appendingPathComponent("backup"))
        let restored = root.appendingPathComponent("restored")
        try LibraryBackup.restore(root.appendingPathComponent("backup"), to: restored)
        // Remove only this diagnostic's original; the supplied fixture is untouched.
        try fm.removeItem(at: library.root)
        let recovered = try Library(root: restored)
        let result = try await ConferenceTranscriber.run(source: recovered.requireAudio(record), folder: recovered.folder(record.id), engine: engine, settings: config, vocabulary: []) { _ in }
        guard result.completed == record.duration, Array(result.segments.prefix(saved.segments.count)) == saved.segments,
              Set(result.segments.map(\.id)).count == result.segments.count else { throw LocalEngineError("Resume changed or duplicated committed text") }
        // A corrupt checkpoint must fail before replacing the file or invoking ASR.
        let recoveredCheckpoint = recovered.folder(record.id).appendingPathComponent("transcription-checkpoint.json")
        let nativeBefore = try Data(contentsOf: recovered.folder(record.id).appendingPathComponent("recording.json"))
        try fm.removeItem(at: recoveredCheckpoint)
        var missingRejected = false
        do { _ = try await ConferenceTranscriber.run(source: recovered.requireAudio(record), folder: recovered.folder(record.id), engine: engine, settings: config, vocabulary: []) { _ in } }
        catch { missingRejected = error.localizedDescription.contains("fehlt") }
        guard missingRejected, try Data(contentsOf: recovered.folder(record.id).appendingPathComponent("recording.json")) == nativeBefore else { throw LocalEngineError("Missing checkpoint damaged existing transcript") }
        var older = saved
        older.completed = 0; older.segments = []
        try JSONEncoder().encode(older).write(to: recoveredCheckpoint)
        var olderRejected = false
        do { _ = try await ConferenceTranscriber.run(source: recovered.requireAudio(record), folder: recovered.folder(record.id), engine: engine, settings: config, vocabulary: []) { _ in } }
        catch { olderRejected = error.localizedDescription.contains("älter") }
        guard olderRejected else { throw LocalEngineError("Stale checkpoint was accepted") }
        let damaged = Data("{broken checkpoint".utf8)
        try damaged.write(to: recoveredCheckpoint)
        var rejected = false
        do { _ = try await ConferenceTranscriber.run(source: recovered.requireAudio(record), folder: recovered.folder(record.id), engine: engine, settings: config, vocabulary: []) { _ in } }
        catch { rejected = error.localizedDescription.contains("beschädigt") }
        guard rejected, try Data(contentsOf: recoveredCheckpoint) == damaged else { throw LocalEngineError("Corrupt checkpoint was not preserved") }
        try JSONEncoder().encode(result).write(to: recoveredCheckpoint, options: .atomic)
        let report: [String: Any] = ["passed": true, "duration": result.duration, "savedAtInterruption": saved.completed,
                                   "originalPrefixPreserved": true, "backupRestoreResume": true, "corruptCheckpointPreserved": true,
                                   "missingCheckpointRejected": true, "olderCheckpointRejected": true,
                                   "words": result.segments.flatMap(\.words).count]
        try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("report.json"))
        print("Recovery workflow passed: interrupted ASR, fresh worker, verified backup, separate restore, removed original, resumed remaining audio and preserved corrupt checkpoint")
    }
}
