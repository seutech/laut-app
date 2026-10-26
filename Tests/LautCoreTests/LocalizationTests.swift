import Foundation
import LautCore

enum LocalizationTests {
    static func run() throws {
        let saved = LautLocalization.language
        defer { LautLocalization.language = saved }
        XCTAssertTrue(LautLocalization.english.count >= 730)
        let pattern = try NSRegularExpression(pattern: #"\{[0-9]+\}"#)
        func tokens(_ text: String) -> [String] {
            let value = text as NSString
            return pattern.matches(in: text, range: NSRange(location: 0, length: value.length))
                .map { value.substring(with: $0.range) }.sorted()
        }
        for (key, value) in LautLocalization.english {
            XCTAssertEqual(tokens(key), tokens(value))
            XCTAssertTrue(!value.isEmpty)
        }
        LautLocalization.language = .en
        XCTAssertEqual(L("Gesamten Text kopieren"), "Copy all text")
        XCTAssertEqual(L("Sprecher \(3)"), "Speaker 3")
        // Interpolation values must never be reinterpreted as placeholders or UI keys.
        let path = "/A/{1}/Grüße 🎤"
        XCTAssertEqual(L("Sicherung geprüft: \(7) Dateien unter \(path)"), "Backup verified: 7 files at " + path)
        XCTAssertEqual(L("Unbekannte Meldung"), "Unbekannte Meldung")
        XCTAssertEqual(LautLocalization.workerMessage("ValueError: Prüfsumme stimmt nicht: /A/Notizen.pdf"),
                       "ValueError: Checksum mismatch: /A/Notizen.pdf")
        XCTAssertEqual(LautLocalization.workerMessage("Some external error: {0}"), "Some external error: {0}")
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        var settings = AppSettings()
        settings.interfaceLanguage = .en; settings.language = "de"
        settings.customInstructions = "Meine eigenen deutschen Anweisungen"
        let roundTrip = try JSONDecoder().decode(AppSettings.self, from: encoder.encode(settings))
        XCTAssertEqual(roundTrip.interfaceLanguage, .en)
        XCTAssertEqual(roundTrip.language, "de")
        XCTAssertEqual(roundTrip.customInstructions, settings.customInstructions)
        var legacy = try JSONSerialization.jsonObject(with: encoder.encode(settings)) as! [String: Any]
        legacy.removeValue(forKey: "interfaceLanguage")
        let old = try JSONDecoder().decode(AppSettings.self, from: JSONSerialization.data(withJSONObject: legacy))
        XCTAssertEqual(old.interfaceLanguage ?? .de, .de)
        XCTAssertEqual(old.customInstructions, settings.customInstructions)
        XCTAssertEqual(try SourceLink.parse("https://example.com/talk.mp3").provider, "Audio-Link")
        XCTAssertEqual(SourceMetadata(url: "https://example.com/talk.mp3", provider: "Audio-Link").providerLabel, "Audio link")
        var record = Recording(title: "Unveränderter Titel")
        record.notes = "Meine Notizen"; record.refinedText = "Auswertung"
        record.segments = [Segment(start: 0, end: 1, text: "Der Originaltext", speakerID: "person")]
        record.speakers = [Speaker(id: "person", name: "Sprechername")]
        let before = try encoder.encode(record)
        let hits = SearchPassage.passages([record])
        XCTAssertTrue(hits.contains { $0.source == "Notizen" && $0.sourceLabel == "Notes" })
        XCTAssertEqual(record.plainText, "Der Originaltext")
        XCTAssertEqual(record.speakerName("person"), "Sprechername")
        let chapter = TranscriptChapter(start: 0, title: "Kapitel")
        XCTAssertEqual(chapter.origin, "Vorschlag")
        XCTAssertEqual(chapter.originLabel, "Suggestion")
        XCTAssertEqual(try encoder.encode(record), before)
        LautLocalization.language = .de
        XCTAssertEqual(L("Sprecher \(3)"), "Sprecher 3")
        XCTAssertEqual(L("Gesamten Text kopieren"), "Gesamten Text kopieren")
        XCTAssertEqual(SearchPassage.passages([record]).map(\.id), hits.map(\.id))
        print("Localization checks passed: catalog, interpolation, worker errors, legacy settings and content preservation")
    }
}
