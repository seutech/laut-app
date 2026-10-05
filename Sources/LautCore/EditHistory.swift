import Foundation

public struct TextChange: Codable, Sendable {
    public var before: String
    public var after: String
}

public struct SegmentChange: Codable, Sendable {
    public var index: Int
    public var before: [Segment]
    public var after: [Segment]
}

public struct SpeakerChange: Codable, Sendable {
    public var before: [Speaker]
    public var after: [Speaker]
}

/// Only changed ranges are retained, rather than a full transcript per keystroke.
public struct EditRevision: Codable, Identifiable, Sendable {
    public var id = UUID()
    public var label: String
    public var date: Date
    public var coalescingKey: String?
    public var title: TextChange?
    public var notes: TextChange?
    public var refinedText: TextChange?
    public var segments: SegmentChange?
    public var speakers: SpeakerChange?

    fileprivate init?(before: Recording, after: Recording, label: String, key: String?, date: Date) {
        self.label = label; self.coalescingKey = key; self.date = date
        func change(_ a: String, _ b: String) -> TextChange? { a == b ? nil : TextChange(before: a, after: b) }
        title = change(before.title, after.title); notes = change(before.notes, after.notes)
        refinedText = change(before.refinedText, after.refinedText)
        if before.speakers != after.speakers { speakers = SpeakerChange(before: before.speakers, after: after.speakers) }
        let a = before.segments, b = after.segments
        if a != b {
            var prefix = 0, suffix = 0
            while prefix < min(a.count, b.count), a[prefix] == b[prefix] { prefix += 1 }
            while suffix < min(a.count, b.count) - prefix, a[a.count - suffix - 1] == b[b.count - suffix - 1] { suffix += 1 }
            segments = SegmentChange(index: prefix, before: Array(a[prefix..<(a.count - suffix)]), after: Array(b[prefix..<(b.count - suffix)]))
        }
        if title == nil && notes == nil && refinedText == nil && segments == nil && speakers == nil { return nil }
    }

    /// Validate before applying; never restore stale file paths, job state or timing metadata.
    fileprivate func apply(to recording: inout Recording, forward: Bool) -> Bool {
        func expected(_ change: TextChange?) -> String? { forward ? change?.before : change?.after }
        if let expected = expected(title), recording.title != expected { return false }
        if let expected = expected(notes), recording.notes != expected { return false }
        if let expected = expected(refinedText), recording.refinedText != expected { return false }
        if let speakers, recording.speakers != (forward ? speakers.before : speakers.after) { return false }
        if let segments {
            let expected = forward ? segments.before : segments.after
            guard segments.index >= 0, segments.index <= recording.segments.count, expected.count <= recording.segments.count - segments.index,
                  Array(recording.segments[segments.index..<(segments.index + expected.count)]) == expected else { return false }
        }
        if let title { recording.title = forward ? title.after : title.before }
        if let notes { recording.notes = forward ? notes.after : notes.before }
        if let refinedText { recording.refinedText = forward ? refinedText.after : refinedText.before }
        if let speakers { recording.speakers = forward ? speakers.after : speakers.before }
        if let segments {
            let count = forward ? segments.before.count : segments.after.count
            recording.segments.replaceSubrange(segments.index..<(segments.index + count), with: forward ? segments.after : segments.before)
        }
        return true
    }
}

public struct EditHistory: Codable, Sendable {
    public var undo: [EditRevision] = []
    public var redo: [EditRevision] = []
    public init() {}
}

public enum RecordingEditor {
    public static func edit(_ before: Recording, label: String, key: String? = nil, date: Date = Date(), change: (inout Recording) -> Void) -> Recording {
        var after = before; change(&after)
        guard var revision = EditRevision(before: before, after: after, label: label, key: key, date: date) else { return before }
        var history = before.editHistory ?? EditHistory()
        if let key, let previous = history.undo.last, previous.coalescingKey == key,
           date.timeIntervalSince(previous.date) >= 0, date.timeIntervalSince(previous.date) < 1.5, history.redo.isEmpty {
            var baseline = before
            if previous.apply(to: &baseline, forward: false) {
                history.undo.removeLast()
                if let combined = EditRevision(before: baseline, after: after, label: label, key: key, date: date) { revision = combined }
                else { after.editHistory = history; return after }
            }
        }
        history.undo.append(revision); history.redo = []
        if history.undo.count > 30 { history.undo.removeFirst(history.undo.count - 30) }
        after.editHistory = history
        return after
    }

    public static func undo(_ recording: Recording) -> Recording? { move(recording, forward: false) }
    public static func redo(_ recording: Recording) -> Recording? { move(recording, forward: true) }
    private static func move(_ recording: Recording, forward: Bool) -> Recording? {
        guard var history = recording.editHistory, let revision = forward ? history.redo.last : history.undo.last else { return nil }
        var result = recording
        guard revision.apply(to: &result, forward: forward) else { return nil }
        if forward { history.redo.removeLast(); history.undo.append(revision) }
        else { history.undo.removeLast(); history.redo.append(revision) }
        result.editHistory = history
        return result
    }
}

public enum TranscriptNavigation {
    public static func parseTime(_ text: String) -> Double? {
        let parts = text.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: ",", with: ".").split(separator: ":", omittingEmptySubsequences: false)
        guard (1...3).contains(parts.count) else { return nil }
        var seconds: Double = 0
        for (index, part) in parts.enumerated() {
            guard !part.isEmpty, let value = Double(part), value.isFinite, value >= 0,
                  (index == 0 || value < 60), (index == parts.count - 1 || value.rounded(.down) == value) else { return nil }
            seconds = seconds * 60 + value
        }
        return seconds.isFinite ? seconds : nil
    }
    public static func activeSegment(at time: Double, in segments: [Segment]) -> UUID? {
        guard time.isFinite else { return nil }
        return segments.first { $0.start <= time && time < $0.end }?.id
    }
    public static func adjacentStart(from time: Double, forward: Bool, in segments: [Segment]) -> Double? {
        let starts = segments.map(\.start).filter(\.isFinite).sorted()
        // One-second tolerance lets Previous return to the current segment once it has begun.
        return forward ? starts.first { $0 > time + 0.1 } : starts.last { $0 < time - 1 }
    }
}
