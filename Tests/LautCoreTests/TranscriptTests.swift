import Foundation
import LautCore

@main
struct TranscriptTests {
    static func main() throws {
        let suite = TranscriptTests()
        suite.testDiarizationRespectsManualSpeakerAndTextEdits()
        suite.testWordTimestampsSplitSpeakersWithoutLosingWords()
        suite.testSplitUnicodeAndInvalidBoundaries()
        suite.testVocabularyUsesWholeWordsAndEscapesReplacement()
        try suite.testLibraryRoundTripAndInterruptedJobRecovery()
        suite.testSubtitlesRoundAcrossMinuteBoundary()
        try suite.testOldRecordingWithoutTimingBreakdownStillLoads()
        try suite.testEditHistorySurvivesRestartAndLeavesJobMetadataAlone()
        suite.testTypingCoalescesAndNewBranchClearsRedo()
        suite.testStructuralUndoRestoresSpeakerLocksAndWords()
        suite.testHistoryBoundAndStaleEditProtection()
        try suite.testDamagedAndMissingPrimaryRecoverFromBackup()
        try suite.testFailedSavePreservesPreviousRecording()
        suite.testTranscriptNavigation()
        print("14 core checks passed")
    }
    func testDiarizationRespectsManualSpeakerAndTextEdits() {
        var locked = Segment(start: 0, end: 2, text: "Mein Name", speakerID: "person")
        locked.speakerLocked = true
        var edited = Segment(start: 2, end: 4, text: "alter Text", words: [Word(text: "alt", start: 2, end: 4)])
        edited.text = "Korrigierter Fachbegriff"
        let result = TranscriptEditor.assign([SpeakerTurn(speakerID: "auto", start: 0, end: 4)], to: [locked, edited])
        XCTAssertEqual(result[0], locked)
        XCTAssertEqual(result[1].text, "Korrigierter Fachbegriff")
        XCTAssertEqual(result[1].originalText, "alter Text")
        XCTAssertEqual(result[1].speakerID, "auto")
    }
    func testWordTimestampsSplitSpeakersWithoutLosingWords() {
        let words = [Word(text: "Hallo", start: 0, end: 1), Word(text: "Anna.", start: 1, end: 2), Word(text: "Guten", start: 3, end: 4), Word(text: "Tag!", start: 4, end: 5)]
        let input = Segment(start: 0, end: 5, text: "Hallo Anna. Guten Tag!", words: words)
        let result = TranscriptEditor.assign([SpeakerTurn(speakerID: "a", start: 0, end: 2), SpeakerTurn(speakerID: "b", start: 3, end: 5)], to: [input])
        XCTAssertEqual(result.map(\.text), ["Hallo Anna.", "Guten Tag!"])
        XCTAssertEqual(result.map(\.speakerID), ["a", "b"])
        XCTAssertEqual(result.flatMap(\.words), words)
    }
    func testSplitUnicodeAndInvalidBoundaries() {
        let input = Segment(start: 0, end: 10, text: "Grüße 👋 Welt")
        let result = TranscriptEditor.split(input, at: 5, characterOffset: 7)
        XCTAssertEqual(result.map(\.text), ["Grüße 👋", "Welt"])
        XCTAssertTrue(result.allSatisfy(\.speakerLocked))
        XCTAssertEqual(TranscriptEditor.split(input, at: 0, characterOffset: 7), [input])
    }
    func testVocabularyUsesWholeWordsAndEscapesReplacement() {
        let input = "laut Lauter LAUT. C++ klappt."
        let output = TranscriptEditor.applyVocabulary(input, entries: [.init(term: "$Laut\\", aliases: "laut"), .init(term: "C plus plus", aliases: "C++")])
        XCTAssertEqual(output, "$Laut\\ Lauter $Laut\\. C plus plus klappt.")
    }
    func testLibraryRoundTripAndInterruptedJobRecovery() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try Library(root: root)
        var record = Recording(title: "Interview")
        record.segments = [Segment(start: 0, end: 2, text: "Test")]
        record.originalSegments = record.segments; record.segments[0].text = "Korrigiert"
        record.state = .processing; try library.save(record)
        let loaded = try XCTUnwrap(library.load().first)
        XCTAssertEqual(loaded.state, .failed)
        XCTAssertEqual(loaded.segments[0].text, "Korrigiert")
        XCTAssertEqual(loaded.originalSegments?[0].text, "Test")
        XCTAssertNotNil(loaded.error)
    }
    func testSubtitlesRoundAcrossMinuteBoundary() {
        var record = Recording(title: "Test")
        record.segments = [Segment(start: 59.9997, end: 61, text: "Hallo")]
        XCTAssertTrue(Exporter.srt(record).contains("00:01:00,000 --> 00:01:01,000"))
    }
    func testOldRecordingWithoutTimingBreakdownStillLoads() throws {
        var record = Recording(title: "Existing recording")
        record.processingSeconds = 25.1
        record.modelLoadSeconds = 20; record.transcriptionSeconds = 4.9; record.audioPreparationSeconds = 0.2
        let encoder = JSONEncoder(), decoder = JSONDecoder()
        let updated = try decoder.decode(Recording.self, from: encoder.encode(record))
        XCTAssertEqual(updated.modelLoadSeconds, 20)
        var old = try JSONSerialization.jsonObject(with: encoder.encode(record)) as! [String: Any]
        for key in ["modelLoadSeconds", "transcriptionSeconds", "audioPreparationSeconds"] { old.removeValue(forKey: key) }
        let loaded = try decoder.decode(Recording.self, from: JSONSerialization.data(withJSONObject: old))
        XCTAssertEqual(loaded.processingSeconds, 25.1)
        XCTAssertTrue(loaded.modelLoadSeconds == nil && loaded.transcriptionSeconds == nil && loaded.audioPreparationSeconds == nil)
        XCTAssertTrue(loaded.editHistory == nil)
    }

    func testEditHistorySurvivesRestartAndLeavesJobMetadataAlone() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try Library(root: root)
        var original = Recording(title: "Interview", audioFilename: "source.wav")
        original.segments = [Segment(start: 0, end: 2, text: "Original")]
        original.originalSegments = original.segments
        var edited = RecordingEditor.edit(original, label: "Korrektur") { $0.segments[0].text = "Korrigiert" }
        edited.processingSeconds = 9; edited.audioFilename = nil; edited.state = .complete
        try library.save(edited)
        let loaded = try XCTUnwrap(Library(root: root).load().first)
        let undone = try XCTUnwrap(RecordingEditor.undo(loaded))
        XCTAssertEqual(undone.segments, original.segments)
        XCTAssertEqual(undone.originalSegments, original.originalSegments)
        XCTAssertEqual(undone.processingSeconds, 9); XCTAssertEqual(undone.state, .complete)
        XCTAssertTrue(undone.audioFilename == nil)
        try library.save(undone)
        let redone = try XCTUnwrap(RecordingEditor.redo(Library(root: root).load()[0]))
        XCTAssertEqual(redone.segments, edited.segments)
    }

    func testTypingCoalescesAndNewBranchClearsRedo() {
        let original = Recording(title: "Original")
        let date = Date(timeIntervalSince1970: 1000)
        var edited = RecordingEditor.edit(original, label: "Titel", key: "title", date: date) { $0.title = "A" }
        edited = RecordingEditor.edit(edited, label: "Titel", key: "title", date: date.addingTimeInterval(0.5)) { $0.title = "AB" }
        XCTAssertEqual(edited.editHistory?.undo.count, 1)
        XCTAssertEqual(RecordingEditor.undo(edited)?.title, original.title)
        edited = RecordingEditor.edit(edited, label: "Titel", key: "title", date: date.addingTimeInterval(4)) { $0.title = "ABC" }
        XCTAssertEqual(edited.editHistory?.undo.count, 2)
        let undone = RecordingEditor.undo(edited)!
        XCTAssertEqual(undone.title, "AB")
        let branched = RecordingEditor.edit(undone, label: "Notiz") { $0.notes = "Neuer Zweig" }
        XCTAssertTrue(RecordingEditor.redo(branched) == nil)
        let restored = RecordingEditor.edit(RecordingEditor.edit(original, label: "Titel", key: "title", date: date) { $0.title = "X" }, label: "Titel", key: "title", date: date.addingTimeInterval(0.1)) { $0.title = original.title }
        XCTAssertEqual(restored.editHistory?.undo.count, 0)
    }

    func testStructuralUndoRestoresSpeakerLocksAndWords() {
        var original = Recording(title: "Gespräch")
        original.speakers = [Speaker(id: "a", name: "A"), Speaker(id: "b", name: "B")]
        original.segments = [Segment(start: 0, end: 4, text: "Hallo Welt", speakerID: "a", words: [Word(text: "Hallo", start: 0, end: 2), Word(text: "Welt", start: 2, end: 4)])]
        let split = RecordingEditor.edit(original, label: "Teilen") { $0.segments = TranscriptEditor.split($0.segments[0], at: 2, characterOffset: 5) }
        let assigned = RecordingEditor.edit(split, label: "Zuordnen") { $0.segments[1].speakerID = "b"; $0.speakers[1].name = "Berta" }
        XCTAssertEqual(assigned.segments.count, 2)
        XCTAssertEqual(TranscriptEditor.assign([SpeakerTurn(speakerID: "auto", start: 0, end: 4)], to: assigned.segments), assigned.segments)
        let undoneAssignment = RecordingEditor.undo(assigned)!
        XCTAssertEqual(undoneAssignment.speakers, original.speakers)
        let undoneSplit = RecordingEditor.undo(undoneAssignment)!
        XCTAssertEqual(undoneSplit.segments, original.segments)
        let redone = RecordingEditor.redo(RecordingEditor.redo(undoneSplit)!)!
        XCTAssertEqual(redone.segments, assigned.segments); XCTAssertEqual(redone.speakers, assigned.speakers)
    }

    func testHistoryBoundAndStaleEditProtection() {
        var record = Recording(title: "0")
        record.segments = (0..<100).map { Segment(start: Double($0), end: Double($0 + 1), text: "Text") }
        for i in 1...35 { record = RecordingEditor.edit(record, label: "Titel") { $0.title = String(i) } }
        XCTAssertEqual(record.editHistory?.undo.count, 30)
        record = RecordingEditor.edit(record, label: "Text") { $0.segments[50].text = "Geändert" }
        XCTAssertEqual(record.editHistory?.undo.last?.segments?.before.count, 1)
        XCTAssertEqual(record.editHistory?.undo.last?.segments?.index, 50)
        record.segments[50].text = "Anderweitig geändert"
        XCTAssertTrue(RecordingEditor.undo(record) == nil)
    }

    func testDamagedAndMissingPrimaryRecoverFromBackup() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try Library(root: root)
        var record = Recording(title: "Erster Stand")
        try library.save(record); record.title = "Zweiter Stand"; try library.save(record)
        let primary = library.folder(record.id).appendingPathComponent("recording.json")
        let damaged = Data("broken JSON".utf8)
        try damaged.write(to: primary)
        let recovered = try XCTUnwrap(library.load().first)
        XCTAssertEqual(recovered.title, "Erster Stand"); XCTAssertEqual(library.loadWarnings.count, 1)
        XCTAssertEqual(try Data(contentsOf: primary), damaged)
        try library.save(recovered)
        let preserved = try FileManager.default.contentsOfDirectory(at: library.folder(record.id), includingPropertiesForKeys: nil).filter { $0.lastPathComponent.hasPrefix("recording.damaged-") }
        XCTAssertEqual(preserved.count, 1); XCTAssertEqual(try Data(contentsOf: preserved[0]), damaged)
        try FileManager.default.removeItem(at: primary)
        XCTAssertEqual(try library.load().first?.title, "Erster Stand")
    }

    func testFailedSavePreservesPreviousRecording() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try Library(root: root)
        var record = Recording(title: "Gespeichert")
        try library.save(record)
        let primary = library.folder(record.id).appendingPathComponent("recording.json")
        let before = try Data(contentsOf: primary)
        // A directory at the backup path simulates an unwritable backup without relying on permissions.
        try FileManager.default.createDirectory(at: library.folder(record.id).appendingPathComponent("recording.previous.json"), withIntermediateDirectories: true)
        record.title = "Ungespeichert"
        var failed = false
        do { try library.save(record) } catch { failed = true }
        XCTAssertTrue(failed); XCTAssertEqual(try Data(contentsOf: primary), before)
    }

    func testTranscriptNavigation() {
        XCTAssertEqual(TranscriptNavigation.parseTime(" 1:02:03,5 "), 3723.5)
        XCTAssertEqual(TranscriptNavigation.parseTime("90"), 90)
        for invalid in ["", "-1", "1:60", "nan", "inf", "1::2", "1.5:20", "1:2:3:4"] { XCTAssertTrue(TranscriptNavigation.parseTime(invalid) == nil) }
        let segments = [Segment(start: 0, end: 2, text: "A"), Segment(start: 3, end: 5, text: "B")]
        XCTAssertEqual(TranscriptNavigation.activeSegment(at: 0, in: segments), segments[0].id)
        XCTAssertTrue(TranscriptNavigation.activeSegment(at: 2.5, in: segments) == nil)
        XCTAssertEqual(TranscriptNavigation.activeSegment(at: 3, in: segments), segments[1].id)
        XCTAssertEqual(TranscriptNavigation.adjacentStart(from: 3.5, forward: false, in: segments), 0)
        XCTAssertEqual(TranscriptNavigation.adjacentStart(from: 4.5, forward: false, in: segments), 3)
        XCTAssertEqual(TranscriptNavigation.adjacentStart(from: 0, forward: true, in: segments), 3)
        XCTAssertTrue(TranscriptNavigation.adjacentStart(from: 5, forward: true, in: segments) == nil)
    }
}

func XCTAssertEqual<T: Equatable>(_ a: T, _ b: T, file: StaticString = #filePath, line: UInt = #line) { precondition(a == b, "Expected \(a) == \(b)", file: file, line: line) }
func XCTAssertTrue(_ value: Bool, file: StaticString = #filePath, line: UInt = #line) { precondition(value, "Expected true", file: file, line: line) }
func XCTAssertNotNil<T>(_ value: T?, file: StaticString = #filePath, line: UInt = #line) { precondition(value != nil, "Expected value", file: file, line: line) }
func XCTUnwrap<T>(_ value: T?) throws -> T { guard let value else { throw NSError(domain: "Check", code: 1) }; return value }
