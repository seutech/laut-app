import Foundation

public struct TranscriptParagraph: Identifiable, Sendable {
    public var segments: [Segment]
    public var id: UUID { segments[0].id }
    public var speakerID: String? { segments[0].speakerID }
    public var start: Double { segments[0].start }
    public var end: Double { segments.last!.end }
    public var text: String { segments.map(\.text).joined(separator: " ") }
}

public struct TimedTextRange: Equatable, Sendable {
    public var range: NSRange
    public var time: Double
}

public enum TranscriptLayout {
    /// A presentation-only grouping: never rewrites existing edits, IDs, or undo history.
    public static func paragraphs(_ segments: [Segment]) -> [TranscriptParagraph] {
        var result: [TranscriptParagraph] = []
        var length = 0
        for segment in segments {
            if let last = result.last, let previous = last.segments.last,
               previous.speakerID == segment.speakerID,
               !previous.speakerLocked, !segment.speakerLocked,
               segment.start >= previous.start, segment.start - previous.end <= 3,
               segment.end - last.start <= 90, length + segment.text.count < 1400 {
                result[result.count - 1].segments.append(segment); length += segment.text.count + 1
            } else { result.append(TranscriptParagraph(segments: [segment])); length = segment.text.count }
        }
        return result
    }

    /// UTF-16 ranges match native attributed text, including emoji and accented characters.
    /// Edited text uses the segment start rather than inventing word timestamps.
    public static func wordRanges(_ segment: Segment) -> [TimedTextRange] {
        guard segment.text == segment.originalText else { return [] }
        let text = segment.text as NSString
        var offset = 0, result: [TimedTextRange] = []
        for word in segment.words {
            guard !word.text.isEmpty, word.start.isFinite, word.end.isFinite, word.end >= word.start,
                  word.start >= segment.start, word.start <= segment.end else { continue }
            let range = text.range(of: word.text, range: NSRange(location: offset, length: text.length - offset))
            guard range.location != NSNotFound else { continue }
            result.append(TimedTextRange(range: range, time: word.start)); offset = NSMaxRange(range)
        }
        return result
    }

    public static func applySpeakers(_ turns: [SpeakerTurn], to recording: inout Recording) {
        // IDs belong to one analysis; a new cluster must never steal a manually named speaker.
        let rawIDs = Array(Set(turns.map(\.speakerID))).sorted()
        let speakers = rawIDs.enumerated().map { Speaker(name: "Sprecher \($0.offset + 1)") }
        let mapping = Dictionary(uniqueKeysWithValues: zip(rawIDs, speakers.map(\.id)))
        let mapped = turns.map { SpeakerTurn(speakerID: mapping[$0.speakerID]!, start: $0.start, end: $0.end) }
        recording.segments = TranscriptEditor.assign(mapped, to: recording.segments)
        let retained = Set(recording.segments.compactMap(\.speakerID))
        recording.speakers = (recording.speakers + speakers).filter { retained.contains($0.id) }
    }

    /// On a new transcript, identify turns before dictionary replacements change its raw text.
    /// Subsequent/manual analyses still use applySpeakers to preserve the user's corrections.
    public static func applyInitialSpeakers(_ turns: [SpeakerTurn], source: [Segment], vocabulary: [VocabularyEntry], to recording: inout Recording) {
        recording.segments = source
        applySpeakers(turns, to: &recording)
        for i in recording.segments.indices {
            recording.segments[i].text = TranscriptEditor.applyVocabulary(recording.segments[i].text, entries: vocabulary)
        }
    }
}

public enum AudioTimeline {
    public static func time(at fraction: Double, start: Double, span: Double, duration: Double) -> Double {
        guard fraction.isFinite, start.isFinite, span.isFinite, duration.isFinite, duration > 0 else { return 0 }
        return min(duration, max(0, start + min(1, max(0, fraction)) * max(0, span)))
    }
}
