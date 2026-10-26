import Foundation
import LautCore
import LautAudio

private actor ConferenceEvaluationStore {
    let library: Library
    var recording: Recording
    init(library: Library, recording: Recording) { self.library = library; self.recording = recording }
    func save(_ checkpoint: TranscriptionCheckpoint) throws {
        recording.originalSegments = checkpoint.segments; recording.segments = checkpoint.segments
        recording.transcriptionProgress = checkpoint.completed / checkpoint.duration
        recording.duration = checkpoint.duration
        try library.save(recording)
        print("Saved \(Int(checkpoint.completed))/\(Int(checkpoint.duration)) seconds, \(checkpoint.segments.count) segments")
    }
}

enum ConferenceWorkflow {
    static func run(project: URL, arguments: [String]) async throws {
        guard let input = try DiagnosticConfiguration.value("--conference", in: arguments),
              let output = try DiagnosticConfiguration.value("--output", in: arguments) else { throw LocalEngineError("Provide --conference audio and --output new-directory") }
        let folder = URL(fileURLWithPath: output)
        guard !FileManager.default.fileExists(atPath: folder.path) else { throw LocalEngineError("Output already exists") }
        let library = try Library(root: folder.appendingPathComponent("library"))
        var config = try DiagnosticConfiguration.settings(project: project, arguments: arguments); config.language = "en"
        let engine = TranscriptionEngine(worker: project.appendingPathComponent("Resources/mlx_worker.py"))
        defer { engine.warmWorker.stop() }
        var metadata: SourceMetadata?
        if let path = try DiagnosticConfiguration.value("--download-result", in: arguments) {
            let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .iso8601
            metadata = try decoder.decode(DownloadedAudio.self, from: Data(contentsOf: URL(fileURLWithPath: path))).source
        }
        var record = try await FileImporter().importFile(URL(fileURLWithPath: input), root: library.root, title: "Telekom AI Investor Day · erste 30 Minuten", metadata: metadata) { _ in }
        var context = ConferenceContext()
        if let docs = try DiagnosticConfiguration.value("--pdf-folder", in: arguments) {
            let files = try FileManager.default.contentsOfDirectory(at: URL(fileURLWithPath: docs), includingPropertiesForKeys: nil).filter { $0.pathExtension.lowercased() == "pdf" }.sorted { $0.path < $1.path }
            context.documents = try await ConferenceDocuments.read(files, into: library.folder(record.id))
            print("Read \(context.documents.count) PDFs, \(context.documents.flatMap(\.pages).count) pages, \(context.documents.flatMap(\.nameCandidates).count) name candidates")
        }
        record.conference = context; try library.save(record)
        let store = ConferenceEvaluationStore(library: library, recording: record)
        let source = library.audioURL(record)!, began = Date()
        // Deliberately interrupt after one checkpoint, then verify actual inference resumes without duplicate words.
        var interrupted = false
        do {
            _ = try await ConferenceTranscriber.run(source: source, folder: library.folder(record.id), engine: engine, settings: config, vocabulary: []) { checkpoint in
                try await store.save(checkpoint)
                if checkpoint.completed >= 300 && checkpoint.completed < checkpoint.duration { throw CancellationError() }
            }
        } catch is CancellationError { interrupted = true; print("Checkpoint interruption verified") }
        let result = try await ConferenceTranscriber.run(source: source, folder: library.folder(record.id), engine: engine, settings: config, vocabulary: []) { try await store.save($0) }
        guard result.completed == record.duration, result.segments.allSatisfy({ $0.start >= 0 && $0.end <= result.duration }),
              Set(result.segments.map(\.id)).count == result.segments.count else { throw LocalEngineError("Coverage/identity check failed") }
        record.segments = result.segments; record.originalSegments = result.segments
        record.transcriptionProgress = nil; record.state = .complete; record.engine = config.engine.label
        record.transcriptionSeconds = result.decodeSeconds; record.modelLoadSeconds = result.loadSeconds; record.audioPreparationSeconds = result.preparationSeconds
        if config.speakerDetectionEnabled && !record.segments.isEmpty {
            let wav = folder.appendingPathComponent("speakers.wav")
            defer { try? FileManager.default.removeItem(at: wav) }
            _ = try await AudioFiles.convert(source, to: wav)
            let start = Date()
            let turns = try await SpeakerAnalyzer().analyze(wav, directory: project.appendingPathComponent(".runtime/models/diarization"), count: nil) { current, total in print("Speakers \(current)/\(total)") }
            TranscriptLayout.applySpeakers(turns, to: &record); record.diarizationSeconds = Date().timeIntervalSince(start)
        }
        record.conference?.chapters = ConferenceLayout.chapters(segments: record.segments, source: metadata?.chapters ?? [])
        let slides = ConferenceLayout.suggestedSlides(record)
        record.conference?.slides = slides
        record.processingSeconds = Date().timeIntervalSince(began)
        try library.save(record)
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(record).write(to: folder.appendingPathComponent("transcript.json"), options: .atomic)
        try Data(Exporter.markdown(record).utf8).write(to: folder.appendingPathComponent("transcript.md"), options: .atomic)
        try Data(Exporter.srt(record).utf8).write(to: folder.appendingPathComponent("transcript.srt"), options: .atomic)
        let report: [String: Any] = ["duration": record.duration, "checkpointResumeTested": interrupted, "asrSeconds": result.decodeSeconds, "loadSeconds": result.loadSeconds, "preparationSeconds": result.preparationSeconds, "totalSeconds": record.processingSeconds!, "speakers": record.speakers.count, "segments": record.segments.count, "words": record.segments.flatMap(\.words).count, "lastWordEnd": record.segments.last?.end ?? 0, "pdfs": context.documents.count]
        try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: folder.appendingPathComponent("report.json"))
        print(String(decoding: try JSONSerialization.data(withJSONObject: report), as: UTF8.self))
    }
}
