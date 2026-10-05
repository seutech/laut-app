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
        print("7 core checks passed")
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
    }
}

func XCTAssertEqual<T: Equatable>(_ a: T, _ b: T, file: StaticString = #filePath, line: UInt = #line) { precondition(a == b, "Expected \(a) == \(b)", file: file, line: line) }
func XCTAssertTrue(_ value: Bool, file: StaticString = #filePath, line: UInt = #line) { precondition(value, "Expected true", file: file, line: line) }
func XCTAssertNotNil<T>(_ value: T?, file: StaticString = #filePath, line: UInt = #line) { precondition(value != nil, "Expected value", file: file, line: line) }
func XCTUnwrap<T>(_ value: T?) throws -> T { guard let value else { throw NSError(domain: "Check", code: 1) }; return value }
