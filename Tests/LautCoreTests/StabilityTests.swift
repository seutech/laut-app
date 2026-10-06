import Foundation
import LautCore

struct StabilityTests {
    static func run() async throws {
        testSearchRequestIsolation()
        try testLongPassageAudioTargets()
        try await testIndexRecovery()
        print("Stability checks passed: search request isolation, timed long passages, damaged-index recovery and rebuild")
    }
    static func testSearchRequestIsolation() {
        var state = SearchState()
        var context = SearchContext(query: "Budget", mode: .hybrid, recordingID: nil, speaker: "", days: 0, revision: 1, model: "small")
        let first = state.begin(context)
        XCTAssertTrue(!state.canAnswer(context)) // Includes the UI's debounce interval.
        XCTAssertTrue(state.finish(first)); XCTAssertTrue(state.canAnswer(context))
        let old = context
        context.query = "Termin"
        let second = state.begin(context)
        XCTAssertTrue(!state.canAnswer(old)); XCTAssertTrue(!state.canAnswer(context))
        XCTAssertTrue(!state.finish(first)); XCTAssertTrue(!state.canAnswer(context))
        XCTAssertTrue(state.finish(second)); XCTAssertTrue(state.canAnswer(context))
        for field in 0..<5 {
            var changed = context
            switch field {
            case 0: changed.speaker = "Anna"
            case 1: changed.recordingID = UUID()
            case 2: changed.days = 7
            case 3: changed.revision += 1
            default: changed.model = "base"
            }
            XCTAssertTrue(!state.canAnswer(changed))
        }
        context.query = " "
        state.finish(state.begin(context)); XCTAssertTrue(!state.canAnswer(context))
    }
    static func testLongPassageAudioTargets() throws {
        let words = (0..<240).map { Word(text: "Begriff\($0)👋", start: Double($0) + 10, end: Double($0) + 10.8) }
        let segment = Segment(start: 10, end: 250, text: TranscriptEditor.joinedWords(words), words: words)
        var record = Recording(title: "Zeitmarken"); record.segments = [segment]
        let chunks = SearchPassage.passages([record]).filter { $0.source == "Transkript" }
        XCTAssertTrue(chunks.count > 2)
        for chunk in chunks {
            let first = String(chunk.text.split(separator: " ").first!)
            XCTAssertEqual(chunk.start, words.first { $0.text == first }?.start)
            XCTAssertEqual(chunk.segmentID, segment.id)
        }
        var legacy = try JSONSerialization.jsonObject(with: JSONEncoder().encode(chunks[0])) as! [String: Any]
        legacy.removeValue(forKey: "segmentID")
        let migrated = try JSONDecoder().decode(SearchPassage.self, from: JSONSerialization.data(withJSONObject: legacy))
        XCTAssertTrue(migrated.segmentID == nil)
        XCTAssertTrue(chunks[1].start! > 10)
        XCTAssertEqual(chunks.map(\.text).joined(separator: " "), segment.text)
        record.segments[0].text = "Korrektur " + segment.text
        let edited = SearchPassage.passages([record]).filter { $0.source == "Transkript" }
        XCTAssertTrue(edited.allSatisfy { $0.start == 10 })
        record.segments[0].text = segment.text; record.segments[0].words = []
        XCTAssertTrue(SearchPassage.passages([record]).filter { $0.source == "Transkript" }.allSatisfy { $0.start == 10 })
    }
    static func testIndexRecovery() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("laut-recovery-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let library = try Library(root: folder.appendingPathComponent("library"))
        var record = Recording(title: "Unverändertes Original", kind: .note); record.notes = "Die Budgetfreigabe ist offen."
        try library.save(record)
        let original = try Data(contentsOf: library.folder(record.id).appendingPathComponent("recording.json"))
        let url = library.root.appendingPathComponent("search-v1.sqlite")
        try Data("damaged database".utf8).write(to: url)
        let index = SearchIndex(url: url)
        for _ in 0..<2 {
            var failed = false
            do { try await index.synchronize([record]) } catch { failed = true }
            XCTAssertTrue(failed) // A failed open must not leave a partially initialized handle.
        }
        try await index.reset(); try await index.synchronize(library.load())
        XCTAssertEqual(try await index.search("Budgetfreigabe", mode: .fullText).count, 1)
        let missing = try await index.missingVectors(model: "small")
        try await index.storeVectors(missing.map { _ in [Float(1), 0] }, for: missing, model: "small")
        try await index.reset()
        XCTAssertTrue(try await index.search("Budgetfreigabe", mode: .fullText).isEmpty)
        try await index.synchronize(library.load())
        XCTAssertEqual(try await index.missingVectors(model: "small").count, missing.count)
        XCTAssertEqual(try Data(contentsOf: library.folder(record.id).appendingPathComponent("recording.json")), original)
        let reopened = SearchIndex(url: url)
        XCTAssertEqual(try await reopened.search("Budgetfreigabe", mode: .fullText).count, 1)
    }
}
