import Foundation
import LautCore

enum SpeakerAlignmentReview {
    /// Read-only comparison of saved ASR against saved acoustic turns. Writes a fresh
    /// preview directory; never touches the user's library or runs ASR again.
    static func run(recording input: String, turns: String, output: String) throws {
        let decoder = JSONDecoder()
        let original = try decoder.decode(Recording.self, from: Data(contentsOf: URL(fileURLWithPath: input)))
        let timeline = try decoder.decode([SpeakerTurn].self, from: Data(contentsOf: URL(fileURLWithPath: turns)))
        let source = original.originalSegments ?? original.segments
        let boundaries = original.conference?.chapters.map(\.start) ?? []
        let strict = TranscriptEditor.assign(timeline, to: source)
        let repaired = TranscriptEditor.assign(timeline, to: source, repairGaps: true)
        guard strict.flatMap(\.words) == repaired.flatMap(\.words) else {
            throw NSError(domain: "SpeakerAlignmentReview", code: 1, userInfo: [NSLocalizedDescriptionKey: "Alignment changed words"])
        }
        func metrics(_ segments: [Segment]) -> [String: Int] {
            ["segments": segments.count,
             "paragraphs": TranscriptLayout.paragraphs(segments, boundaries: boundaries).count,
             "unassignedSegments": segments.filter { $0.speakerID == nil }.count,
             "unassignedWords": segments.filter { $0.speakerID == nil }.flatMap(\.words).count,
             "words": segments.flatMap(\.words).count]
        }
        // Also test the exact path used by the app when reanalysing an existing recording.
        let updated = RecordingEditor.edit(original, label: "Sprecherlücken überbrücken") {
            TranscriptLayout.applySpeakers(timeline, to: &$0)
        }
        guard updated.segments.flatMap(\.words) == original.segments.flatMap(\.words),
              RecordingEditor.undo(updated)?.segments == original.segments else {
            throw NSError(domain: "SpeakerAlignmentReview", code: 2, userInfo: [NSLocalizedDescriptionKey: "Reanalysis or undo changed original words"])
        }
        let directory = URL(fileURLWithPath: output, isDirectory: true)
        guard !FileManager.default.fileExists(atPath: directory.path) else {
            throw NSError(domain: "SpeakerAlignmentReview", code: 3, userInfo: [NSLocalizedDescriptionKey: "Preview destination already exists"])
        }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let report: [String: Any] = ["stored": metrics(original.segments), "strict": metrics(strict),
                                   "repaired": metrics(repaired), "appReanalysis": metrics(updated.segments),
                                   "wordsPreserved": true, "undoVerified": true,
                                   "note": "Alignment heuristics only; speaker identities and acoustic accuracy are not verified."]
        try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: directory.appendingPathComponent("report.json"), options: .withoutOverwriting)
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(repaired).write(to: directory.appendingPathComponent("aligned-segments.json"), options: .withoutOverwriting)
        let paragraphs = TranscriptLayout.paragraphs(repaired, boundaries: boundaries)
        let text = paragraphs.map(\.text).joined(separator: "\n\n")
        try text.write(to: directory.appendingPathComponent("Transkript.txt"), atomically: true, encoding: .utf8)
        let markdown = "# \(original.title)\n\nVorschau: Sprecherlücken geglättet. Namen und inhaltliche Richtigkeit noch nicht geprüft.\n\n" +
            paragraphs.map { "**\($0.speakerID ?? "Unzugeordnet") · \(Exporter.timestamp($0.start))**\n\n\($0.text)" }.joined(separator: "\n\n")
        try markdown.write(to: directory.appendingPathComponent("Transkript-mit-Sprechern.md"), atomically: true, encoding: .utf8)
        print(String(data: try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]), encoding: .utf8)!)
    }
}
