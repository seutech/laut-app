import Foundation
import LautCore
import LautAudio

enum SearchWorkflow {
    static func run(project: URL, model: String) async throws {
        var settings = AppSettings(); settings.runtimeDirectory = project.appendingPathComponent(".runtime").path
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("laut-search-workflow-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let engine = EmbeddingEngine(script: project.appendingPathComponent("Resources/embedding_worker.py"))
        defer { engine.worker.stop() }
        let index = SearchIndex(url: folder.appendingPathComponent("index.sqlite"))
        var meeting = Recording(title: "Terminplanung")
        meeting.segments = [Segment(start: 42, end: 50, text: "Wir verschieben die Veröffentlichung auf den nächsten Monat.")]
        var meal = Recording(title: "Küche", kind: .note); meal.notes = "Zum Abendessen gibt es Kartoffeln und Gemüse."
        try await index.synchronize([meeting, meal])
        let missing = try await index.missingVectors(model: model)
        let began = Date()
        let vectors = try await engine.embed(missing.map(\.embeddingText), path: model, settings: settings)
        try await index.storeVectors(vectors, for: missing, model: model)
        let warm = Date()
        let query = try await engine.embed(["Was startet später als geplant?"], query: true, path: model, settings: settings)
        let results = try await index.search("Was startet später als geplant?", mode: .hybrid, model: model, vector: query.first)
        print("Result sources: \(results.map { $0.passage.source + ":" + $0.passage.title })")
        guard results.first?.passage.start == 42, results.first?.passage.recordingID == meeting.id else { throw LocalEngineError("Semantic search did not retain audio target") }
        print("Offline Swift/Python/index workflow: \(vectors.count) passages, cold index \(Date().timeIntervalSince(began)) s, warm query \(Date().timeIntervalSince(warm)) s; audio target 42 s")
        let running = Task { try await engine.embed(Array(repeating: String(repeating: "Künstliche Testdaten. ", count: 300), count: 16), path: model, settings: settings) }
        try await Task.sleep(for: .milliseconds(50)); running.cancel()
        do { _ = try await running.value; throw LocalEngineError("Cancelled embedding unexpectedly completed") }
        catch let error as LocalEngineError where error.message == "Cancelled embedding unexpectedly completed" { throw error }
        catch {}
        let restarted = try await engine.embed(["Bezahlung"], query: true, path: model, settings: settings)
        guard restarted.first?.count == 384 else { throw LocalEngineError("Embedding restart failed") }
        print("Embedding cancellation and worker restart passed")
    }
}
