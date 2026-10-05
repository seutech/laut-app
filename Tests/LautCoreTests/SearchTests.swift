import Foundation
import LautCore

struct SearchTests {
    static func run() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("laut-search-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("search.sqlite"), index = SearchIndex(url: root.appendingPathComponent("search.sqlite"))
        var a = Recording(title: "Planung")
        a.speakers = [Speaker(id: "anna", name: "Anna Müller")]
        a.segments = [Segment(start: 42.5, end: 61, text: "Wir verschieben die Veröffentlichung auf den nächsten Monat.", speakerID: "anna")]
        a.notes = "Budgetfreigabe fehlt"; a.refinedText = "Entscheidung: Termin ändern"
        var b = Recording(title: "Küche", kind: .note); b.notes = "Kartoffeln kochen"; b.createdAt = Date(timeIntervalSince1970: 0)
        try await index.synchronize([a, b])
        let full = try await index.search("Veröffentlichung", mode: .fullText)
        XCTAssertEqual(full.count, 1); XCTAssertEqual(full[0].passage.start, 42.5)
        XCTAssertEqual(try await index.search("muller", mode: .fullText).first?.passage.speaker, "Anna Müller")
        XCTAssertEqual(try await index.search("Budgetfreigabe", mode: .fullText).first?.passage.source, "Notizen")
        XCTAssertEqual(try await index.search("Entscheidung", mode: .fullText).first?.passage.source, "Text-KI")
        XCTAssertTrue(try await index.search("Kartoffeln", mode: .fullText, filter: SearchFilter(recordingID: a.id)).isEmpty)
        XCTAssertTrue(try await index.search("Kartoffeln", mode: .fullText, filter: SearchFilter(since: Date(timeIntervalSince1970: 1))).isEmpty)
        XCTAssertTrue(try await index.search("Veröffentlichung", mode: .fullText, filter: SearchFilter(speaker: "Bob")).isEmpty)
        XCTAssertTrue(try await index.search("\"* OR DROP TABLE passages; --", mode: .fullText).isEmpty)
        XCTAssertTrue(try await index.search("!!!", mode: .fullText).isEmpty)
        let pending = try await index.missingVectors(model: "one")
        let vectors: [[Float]] = pending.map { $0.source == "Transkript" ? [1, 0] : [0, 1] }
        try await index.storeVectors(vectors, for: pending, model: "one")
        XCTAssertTrue(try await index.missingVectors(model: "one").isEmpty)
        let semantic = try await index.search("später starten", mode: .semantic, model: "one", vector: [1, 0])
        XCTAssertEqual(semantic.first?.passage.start, 42.5)
        let hybrid = try await index.search("Veröffentlichung", mode: .hybrid, model: "one", vector: [1, 0])
        XCTAssertTrue(hybrid[0].keyword && hybrid[0].semantic)
        XCTAssertEqual(try await index.missingVectors(model: "two").count, pending.count)
        XCTAssertTrue(try await index.search("später", mode: .semantic, model: "two", vector: [1, 0]).isEmpty)
        let reopened = SearchIndex(url: url)
        XCTAssertEqual(try await reopened.search("Veröffentlichung", mode: .fullText).first?.passage.id, full.first?.passage.id)
        XCTAssertEqual(try await reopened.search("später", mode: .semantic, model: "one", vector: [1, 0]).first?.passage.id, full.first?.passage.id)
        a.segments[0].text = "Wir starten jetzt."
        try await index.synchronize([a])
        XCTAssertTrue(try await index.search("Veröffentlichung", mode: .fullText).isEmpty)
        XCTAssertTrue(try await index.search("Kartoffeln", mode: .fullText).isEmpty)
        // An in-flight embedding for old text must not resurrect a stale result.
        try await index.storeVectors(vectors, for: pending, model: "one")
        XCTAssertEqual(try await index.missingVectors(model: "one").count, 1)
        let stale = try await index.search("später", mode: .semantic, model: "one", vector: [1, 0])
        XCTAssertTrue(!stale.contains { $0.passage.source == "Transkript" })
        do { try await index.storeVectors([[Float.nan]], for: Array(pending.prefix(1)), model: "one"); preconditionFailure("NaN accepted") } catch {}
        try await index.synchronize([])
        XCTAssertTrue(try await index.search("Planung", mode: .fullText).isEmpty)
        let runtime = root.appendingPathComponent("runtime").path
        XCTAssertTrue(ModelFiles.removalTarget(path: "/outside/model", runtime: runtime, modelID: "intfloat/multilingual-e5-small") == nil)
        XCTAssertTrue(ModelFiles.removalTarget(path: runtime + "/models/huggingface/models--evil/snapshots/abc", runtime: runtime, modelID: "../evil") == nil)
        let safe = runtime + "/models/huggingface/models--intfloat--multilingual-e5-small/snapshots/abc"
        XCTAssertEqual(ModelFiles.removalTarget(path: safe, runtime: runtime, modelID: "intfloat/multilingual-e5-small")?.lastPathComponent, "models--intfloat--multilingual-e5-small")
        let parent = root.appendingPathComponent("runtime/models/huggingface")
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: parent.appendingPathComponent("models--intfloat--multilingual-e5-small"), withDestinationURL: root)
        XCTAssertTrue(ModelFiles.removalTarget(path: safe, runtime: runtime, modelID: "intfloat/multilingual-e5-small") == nil)
        var long = Recording(title: "Lang", kind: .note); long.notes = Array(repeating: "Grüße 👋", count: 300).joined(separator: " ") + " Endmarker"
        let chunks = SearchPassage.passages([long]).filter { $0.source == "Notizen" }
        XCTAssertTrue(chunks.count > 1); XCTAssertTrue(chunks.last!.text.hasSuffix("Endmarker"))
        XCTAssertEqual(chunks.map(\.text).joined(separator: " "), long.notes)
        print("Search checks passed: FTS, filters, semantic/hybrid, persistence, invalidation, deletion, Unicode and safe model removal")
    }
}
