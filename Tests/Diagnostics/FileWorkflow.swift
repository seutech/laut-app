import Foundation
import LautCore
import LautAudio

/// Uses a caller-supplied fixture in an isolated temporary library, never the user's library.
enum FileWorkflow {
    static func check(source: URL, project: URL, settings: AppSettings, output: URL?) async throws {
        if let output, FileManager.default.fileExists(atPath: output.path) { throw LocalEngineError("Output directory already exists; choose a new directory to preserve earlier results.") }
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
        let engine = TranscriptionEngine(worker: project.appendingPathComponent("Resources/mlx_worker.py"))
        defer { engine.warmWorker.stop() }
        let preparationBegan = Date()
        try await engine.preload(settings: settings)
        let preparationSeconds = Date().timeIntervalSince(preparationBegan)
        let began = Date()
        let result = try await engine.transcribe(audio: wav, settings: settings, vocabulary: [])
        let transcriptionSeconds = Date().timeIntervalSince(began)
        record.segments = result.editorSegments(); record.originalSegments = record.segments
        record.duration = duration; record.state = .complete; record.engine = settings.engine.label
        guard !record.segments.isEmpty else { throw LocalEngineError("Empty transcript") }
        print("\(settings.engine.label): preparation \(preparationSeconds) seconds; ASR \(transcriptionSeconds) seconds; \(record.segments.count) segments; last end \(record.segments.last!.end)")
        try library.save(record)

        let analyzer = SpeakerAnalyzer()
        let diarizationBegan = Date()
        let detected = try await analyzer.analyzeIfEnabled(wav, directory: project.appendingPathComponent(".runtime/models/diarization"), enabled: settings.speakerDetectionEnabled) { _, _ in }
        let diarizationSeconds = Date().timeIntervalSince(diarizationBegan)
        let turns = detected ?? []
        if detected != nil {
            guard !turns.isEmpty else { throw LocalEngineError("No speaker turns") }
            record = RecordingEditor.edit(record, label: "Sprecheranalyse") { TranscriptLayout.applyInitialSpeakers(turns, source: $0.originalSegments ?? $0.segments, vocabulary: [], to: &$0) }
        }
        print("Diarization enabled: \(settings.speakerDetectionEnabled); \(record.speakers.count) speakers; \(turns.count) turns; reading paragraphs: \(TranscriptLayout.paragraphs(record.segments).count)")
        let automaticRecording = record
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
        let copy = try await importer.importFile(copied, root: library.root, title: record.title + " · neue Transkription") { _ in }
        guard copy.id != record.id, copy.segments.isEmpty, copy.editHistory == nil, copy.state == .ready,
              copy.title == record.title + " · neue Transkription", abs(copy.duration - record.duration) < 0.01,
              FileManager.default.contentsEqual(atPath: copied.path, andPath: library.audioURL(copy)!.path),
              try library.load().first(where: { $0.id == record.id })?.segments == record.segments else { throw LocalEngineError("New version did not preserve the existing transcript") }
        if let output {
            try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            var exported = automaticRecording; exported.audioFilename = nil; exported.editHistory = nil
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(exported).write(to: output.appendingPathComponent("transcript.json"), options: .atomic)
            try Data(exported.plainText.utf8).write(to: output.appendingPathComponent("transcript.txt"), options: .atomic)
            try Data(Exporter.srt(exported).utf8).write(to: output.appendingPathComponent("transcript.srt"), options: .atomic)
            let report: [String: Any] = ["engine": settings.engine.rawValue, "audio_seconds": duration,
                "preparation_seconds": preparationSeconds, "transcription_seconds": transcriptionSeconds,
                "diarization_seconds": diarizationSeconds, "asr_seconds_per_audio_second": transcriptionSeconds / duration,
                "timed_word_count": result.words?.count ?? 0, "last_segment_end": automatic.last?.end ?? 0,
                "speaker_count": exported.speakers.count, "speakers_enabled": settings.speakerDetectionEnabled, "reading_paragraphs": TranscriptLayout.paragraphs(exported.segments).count, "workflow_checks_passed": true]
            try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: output.appendingPathComponent("report.json"), options: .atomic)
            print("Local transcripts and timing report saved. No transcript text printed to the log.")
        }
        print("Workflow passed: import → ASR → speakers → manual correction → save/reopen → undo/redo → export")
    }
}
