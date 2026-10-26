import Foundation
import LautCore
import LautAudio

/// Resumable local batch, using the same import, ASR, speaker and export components as Laut.
enum ConferenceBatch {
    struct Manifest: Decodable {
        var eventDate: String
        var sourcePage: String
        var vocabularyPath: String
        var documentCacheRecordingPath: String?
        var items: [Item]
    }
    struct Item: Decodable {
        var slug: String
        var url: String
        var title: String
        var expectedDuration: Double
        var pdfPaths: [String]
        var pdfTitles: [String]?
    }
    static func run(project: URL, arguments: [String]) async throws {
        guard let input = try DiagnosticConfiguration.value("--conference-batch", in: arguments) else { throw LocalEngineError("Missing manifest") }
        let file = URL(fileURLWithPath: input), root = file.deletingLastPathComponent()
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: file))
        let fm = FileManager.default, library = try Library(root: root.appendingPathComponent("library"))
        try ConferenceMedia.initialize(root)
        func inputURL(_ path: String) -> URL { path.hasPrefix("/") ? URL(fileURLWithPath: path) : root.appendingPathComponent(path) }
        var settings = try DiagnosticConfiguration.settings(project: project, arguments: arguments)
        settings.language = "en"
        let engine = TranscriptionEngine(worker: project.appendingPathComponent("Resources/mlx_worker.py"))
        defer { engine.warmWorker.stop() }
        let vocabulary = try String(contentsOf: inputURL(manifest.vocabularyPath), encoding: .utf8)
        var failures: [String] = []
        for item in manifest.items {
            do {
                guard item.slug.range(of: "^[a-z0-9-]+$", options: .regularExpression) != nil else { throw LocalEngineError("Invalid slug") }
                let download = root.appendingPathComponent("downloads/" + item.slug)
                let metadataDecoder = JSONDecoder(); metadataDecoder.dateDecodingStrategy = .iso8601
                let downloaded = try metadataDecoder.decode(DownloadedAudio.self, from: Data(contentsOf: download.appendingPathComponent("result.json")))
                var metadata = downloaded.source; metadata.eventDate = manifest.eventDate
                guard abs(downloaded.duration - item.expectedDuration) < 3, metadata.url == item.url else { throw LocalEngineError("Source/duration mismatch") }
                let existing = try library.load().first { $0.source?.url == item.url }
                var record: Recording
                if let existing { record = existing }
                else {
                    record = try await FileImporter().importFile(ConferenceMedia.url(downloaded.filename, in: download), root: library.root, title: item.title, metadata: metadata) { _ in }
                }
                let folder = library.folder(record.id)
                print("BEGIN \(item.slug) · \(Int(record.duration)) seconds"); fflush(stdout)
                if record.conference == nil {
                    var context = ConferenceContext(); context.vocabulary = vocabulary
                    var remaining: [URL] = []
                    // Reuse extracted text only when the original PDF bytes are identical.
                    let cacheFile = manifest.documentCacheRecordingPath.map(inputURL)
                    let cached = try cacheFile.map { try JSONDecoder().decode(Recording.self, from: Data(contentsOf: $0)) }
                    for path in item.pdfPaths {
                        let pdf = inputURL(path), bytes = try Data(contentsOf: pdf)
                        if let cacheFile, let doc = cached?.conference?.documents.first(where: {
                            (try? Data(contentsOf: ConferenceMedia.url($0.filename, in: cacheFile.deletingLastPathComponent()))) == bytes
                        }) {
                            let target = folder.appendingPathComponent(doc.filename)
                            try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
                            try bytes.write(to: target, options: .atomic)
                            context.documents.append(doc)
                        } else { remaining.append(pdf) }
                    }
                    context.documents += try await ConferenceDocuments.read(remaining, into: folder)
                    for i in context.documents.indices {
                        let bytes = try Data(contentsOf: folder.appendingPathComponent(context.documents[i].filename))
                        if let index = item.pdfPaths.firstIndex(where: { (try? Data(contentsOf: inputURL($0))) == bytes }),
                           let titles = item.pdfTitles, index < titles.count { context.documents[i].title = titles[index] }
                    }
                    record.conference = context; try library.save(record)
                }
                // Final media locations are stable before the ASR checkpoint signature is created.
                try ConferenceMedia.compact(record, folder: folder, root: root)
                if record.state != .complete {
                    let began = Date(), source = library.audioURL(record)!
                    record.state = .processing; record.error = nil
                    let store = BatchCheckpointStore(library: library, record: record, slug: item.slug)
                    let result = try await ConferenceTranscriber.run(source: source, folder: folder, engine: engine, settings: settings, vocabulary: record.conference!.terms) { checkpoint in
                        try await store.save(checkpoint)
                    }
                    guard abs(result.completed - record.duration) < 0.1, !result.segments.isEmpty,
                          result.segments.allSatisfy({ $0.start >= 0 && $0.end <= result.duration && $0.end >= $0.start }),
                          Set(result.segments.map(\.id)).count == result.segments.count else { throw LocalEngineError("Transcript coverage/identity check failed") }
                    record.originalSegments = result.segments; record.segments = result.segments
                    record.transcriptionProgress = 1; record.engine = settings.engine.label
                    record.transcriptionSeconds = result.decodeSeconds; record.modelLoadSeconds = result.loadSeconds
                    record.audioPreparationSeconds = result.preparationSeconds
                    try library.save(record)
                    if settings.speakerDetectionEnabled {
                        let wav = folder.appendingPathComponent("speakers-temporary.wav")
                        defer { try? fm.removeItem(at: wav) }
                        _ = try await AudioFiles.convert(source, to: wav)
                        let start = Date()
                        let turns = try await SpeakerAnalyzer().analyze(wav, directory: project.appendingPathComponent(".runtime/models/diarization"), count: nil) { current, total in
                            if current % 20 == 0 || current == total { print("SPEAKERS \(item.slug) \(current)/\(total)"); fflush(stdout) }
                        }
                        TranscriptLayout.applyInitialSpeakers(turns, source: result.segments, vocabulary: record.conference!.terms, to: &record)
                        record.diarizationSeconds = Date().timeIntervalSince(start)
                    } else {
                        for i in record.segments.indices { record.segments[i].text = TranscriptEditor.applyVocabulary(record.segments[i].text, entries: record.conference!.terms) }
                    }
                    record.conference?.chapters = ConferenceLayout.chapters(segments: record.segments, source: metadata.chapters ?? [])
                    let ranges = ConferenceReview.rangeProposals(record)
                    let slides = ConferenceLayout.suggestedSlides(record)
                    record.conference?.ranges = ranges
                    record.conference?.slides = slides
                    record.notes = "Lokale automatische Transkription der offiziellen Einzelveröffentlichung. Quelle: \(manifest.sourcePage). Sprechergruppen sind keine bestätigten Identitäten. Kapitel, Einspieler und PDF-Zuordnungen sind Vorschläge; keine visuelle Verifikation. Bestätigte Wörterbuch-Aliasse wurden angewandt; Originaltext bleibt erhalten. Zeitangaben beziehen sich auf dieses Einzelvideo."
                    record.processingSeconds = Date().timeIntervalSince(began)
                    record.transcriptionProgress = nil; record.state = .complete
                    try library.save(record)
                }
                let destination = root.appendingPathComponent("vortraege/" + item.slug)
                if !fm.fileExists(atPath: destination.path) {
                    let staging = root.appendingPathComponent("vortraege/.staging-" + item.slug)
                    if fm.fileExists(atPath: staging.path) { try fm.removeItem(at: staging) }
                    try ConferenceArchive.export(record, sourceFolder: folder, to: staging, sharedRoot: root)
                    let names = await ConferenceDocuments.corrections(segments: record.segments, terms: record.conference!.terms)
                    let speakers = ConferenceReview.speakerProposals(record)
                    let review: [String: Any] = [
                        "status": "Ungeprüfte Vorschläge; nichts automatisch ausgeblendet",
                        "names": names.map { ["variant": $0.variant, "canonical": $0.canonical, "time": $0.time, "excerpt": $0.excerpt] as [String: Any] },
                        "speakers": speakers.map { ["speakerID": $0.speakerID, "name": $0.name, "time": $0.time, "evidence": $0.evidence] as [String: Any] }
                    ]
                    try JSONSerialization.data(withJSONObject: review, options: [.prettyPrinted, .sortedKeys]).write(to: staging.appendingPathComponent("pruefvorschlaege.json"), options: .atomic)
                    try fm.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
                    try fm.moveItem(at: staging, to: destination)
                }
                print("COMPLETE \(item.slug) · \(record.segments.flatMap(\.words).count) words · \(record.speakers.count) speaker groups"); fflush(stdout)
            } catch {
                failures.append(item.slug + ": " + error.localizedDescription)
                print("FAILED \(item.slug): \(error.localizedDescription)"); fflush(stdout)
            }
        }
        let records = try library.load()
        let index = SearchIndex(url: root.appendingPathComponent("search.sqlite"))
        try await index.synchronize(records)
        let report: [String: Any] = ["records": records.count, "complete": records.filter { $0.state == .complete }.count, "failures": failures,
                                   "seconds": records.reduce(0.0) { $0 + $1.duration }, "words": records.flatMap(\.segments).flatMap(\.words).count]
        try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("transcription-report.json"), options: .atomic)
        if !failures.isEmpty { throw LocalEngineError(failures.joined(separator: "\n")) }
        print("BATCH COMPLETE \(records.count) records"); fflush(stdout)
    }
}

private actor BatchCheckpointStore {
    let library: Library
    var record: Recording
    let slug: String
    init(library: Library, record: Recording, slug: String) { self.library = library; self.record = record; self.slug = slug }
    func save(_ checkpoint: TranscriptionCheckpoint) throws {
        record.originalSegments = checkpoint.segments; record.segments = checkpoint.segments
        record.transcriptionProgress = checkpoint.completed / checkpoint.duration
        try library.save(record)
        print("ASR \(slug) \(Int(checkpoint.completed))/\(Int(checkpoint.duration)) seconds"); fflush(stdout)
    }
}
