import Foundation
import LautCore

/// Reads existing files only. Compares the actual Swift projection with the Rust TXT export,
/// and checks that histories produced by Rust remain usable by the current macOS app.
@main
struct PortableInterop {
    static func main() throws {
        guard CommandLine.arguments.count == 3 else {
            throw NSError(domain: "PortableInterop", code: 1, userInfo: [NSLocalizedDescriptionKey: "Usage: LautPortableInterop recording.json transcript.txt"])
        }
        let recording = try JSONDecoder().decode(Recording.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        let paragraphs = TranscriptLayout.paragraphs(ConferenceReview.includedSegments(recording), boundaries: recording.conference?.chapters.map(\.start) ?? [])
        let expected = paragraphs.map(\.text).joined(separator: "\n\n")
        let actual = try String(contentsOfFile: CommandLine.arguments[2], encoding: .utf8)
        guard actual == expected else {
            throw NSError(domain: "PortableInterop", code: 2, userInfo: [NSLocalizedDescriptionKey: "Swift/Rust reading projections differ"])
        }
        if let revision = recording.editHistory?.undo.last, ["Edit transcript", "Name speaker", "Assign speaker", "Edit title and notes", "Split segment", "Merge segments", "Replace transcript text", "Add speaker", "Merge speakers", "Apply editor draft", "Apply speaker analysis", "Edit conference", "Save local analysis"].contains(revision.label) {
            guard let undone = RecordingEditor.undo(recording), let redone = RecordingEditor.redo(undone),
                  redone.segments == recording.segments, redone.speakers == recording.speakers,
                  redone.title == recording.title, redone.notes == recording.notes, redone.conference == recording.conference,
                  redone.refinedText == recording.refinedText else {
                throw NSError(domain: "PortableInterop", code: 3, userInfo: [NSLocalizedDescriptionKey: "Rust history did not round-trip through Swift"])
            }
        }
        print("Swift/Rust compatible: \(recording.segments.count) segments, \(paragraphs.count) paragraphs, \(expected.count) characters")
    }
}
