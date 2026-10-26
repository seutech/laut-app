import Foundation
import LautCore

enum SpeakerAlignmentTests {
    static func run() {
        func turn(_ id: String, _ start: Double, _ end: Double) -> SpeakerTurn {
            SpeakerTurn(speakerID: id, start: start, end: end)
        }
        func word(_ text: String, _ start: Double, _ end: Double) -> Word {
            Word(text: text, start: start, end: end)
        }
        func assign(_ turns: [SpeakerTurn], _ words: [Word]) -> [Segment] {
            TranscriptEditor.assign(turns, to: [Segment(start: words.first!.start, end: words.last!.end,
                text: TranscriptEditor.joinedWords(words), words: words)], repairGaps: true)
        }
        // A missing piece of VAD inside one voice should not split the sentence.
        let words = [word("Can", 1, 1.3), word("you", 2.3, 2.6), word("hear?", 4.2, 4.7)]
        let joined = assign([turn("a", 4, 5), turn("a", 1, 2)], words)
        XCTAssertEqual(joined.count, 1)
        XCTAssertEqual(joined[0].speakerID, "a")
        XCTAssertEqual(joined.flatMap(\.words), words)
        // A brief actual interjection stays, including its point-timestamp word.
        let interjection = assign([turn("a", 0, 2), turn("b", 2.2, 2.4), turn("a", 2.5, 4)],
                                  [word("Yes", 1, 1.4), word("No!", 2.3, 2.3), word("Okay", 3, 3.5)])
        XCTAssertEqual(interjection.map(\.speakerID), ["a", "b", "a"])
        // A word near two different turn edges is uncertain, not a nearest-voice guess.
        XCTAssertEqual(assign([turn("a", 0, 1), turn("b", 1.6, 3)], [word("Hm", 1.2, 1.4)])[0].speakerID, nil)
        // Single nearby edge can repair a truncated first or last word.
        XCTAssertEqual(assign([turn("a", 3.056, 7.2)], [word("I", 2.64, 2.96)])[0].speakerID, "a")
        XCTAssertEqual(assign([turn("a", 0, 1)], [word("Yes", 1.1, 1.3)])[0].speakerID, "a")
        // Long gaps and long uncovered utterances must not be assigned wholesale.
        XCTAssertEqual(assign([turn("a", 0, 1), turn("a", 7, 9)], [word("Unknown", 3, 4)])[0].speakerID, nil)
        XCTAssertEqual(assign([turn("a", 0, 1)], [word("Several words", 1.1, 3)])[0].speakerID, nil)
        XCTAssertEqual(assign([], [word("Unknown", 0, 1)])[0].speakerID, nil)
        // Nested competing evidence must not be swallowed by extending the short a-turn.
        XCTAssertEqual(assign([turn("b", 0, 5), turn("a", 1, 2), turn("a", 4, 6)],
                              [word("Actually", 2.5, 3.5)])[0].speakerID, "b")
        var locked = Segment(start: 2.3, end: 2.6, text: "Manuell", speakerID: "person")
        locked.speakerLocked = true
        var edited = Segment(start: 2.3, end: 2.6, text: "Rohtext", words: [word("Rohtext", 2.3, 2.6)])
        edited.text = "Korrigierter Text"
        let protected = TranscriptEditor.assign([turn("a", 1, 2), turn("a", 4, 5)], to: [locked, edited], repairGaps: true)
        XCTAssertEqual(protected[0], locked)
        XCTAssertEqual(protected[1].text, edited.text)
        XCTAssertEqual(protected[1].words, edited.words)
        // Reanalysis of an already fragmented transcript must produce the same readable text.
        let fragments = words.map { Segment(start: $0.start, end: $0.end, text: $0.text, words: [$0]) }
        let repaired = TranscriptEditor.assign([turn("a", 1, 2), turn("a", 4, 5)], to: fragments, repairGaps: true)
        XCTAssertEqual(TranscriptLayout.paragraphs(repaired).map(\.text), ["Can you hear?"])
        var record = Recording(title: "Namen")
        record.segments = repaired; record.speakers = [Speaker(id: "a", name: "Sprecher 1")]
        let named = RecordingEditor.edit(record, label: "Benennen") { TranscriptEditor.nameSpeaker("a", name: "Anna", in: &$0) }
        XCTAssertTrue(named.segments.allSatisfy(\.speakerLocked))
        XCTAssertEqual(TranscriptLayout.paragraphs(named.segments).map(\.text), ["Can you hear?"])
        XCTAssertEqual(RecordingEditor.undo(named)?.segments, record.segments)
        var reanalysed = named
        TranscriptLayout.applySpeakers([turn("b", 0, 10)], to: &reanalysed)
        XCTAssertEqual(reanalysed.segments, named.segments)
        XCTAssertEqual(reanalysed.speakerName("a"), "Anna")
        // An explicitly edited/legacy boundary still wins over naming the voice.
        record.segments[1].speakerLocked = true
        TranscriptEditor.nameSpeaker("a", name: "Anna", in: &record)
        XCTAssertEqual(TranscriptLayout.paragraphs(record.segments).count, 3)
        print("Speaker alignment checks passed (gaps, interjections, ambiguity, point timestamps, edits)")
    }
}
