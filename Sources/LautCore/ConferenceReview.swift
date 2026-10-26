import Foundation

public struct ConferenceRange: Codable, Equatable, Identifiable, Sendable {
    public var id = UUID()
    public var start: Double
    public var end: Double
    public var kind: String
    public var evidence: String
    public var confirmed = false
    public var excluded = false
    public init(start: Double, end: Double, kind: String, evidence: String) {
        self.start = start; self.end = end; self.kind = kind; self.evidence = evidence
    }
    public var valid: Bool { start.isFinite && end.isFinite && start >= 0 && end > start }
}
public struct SpeakerNameProposal: Identifiable, Sendable {
    public var speakerID: String
    public var name: String
    public var time: Double
    public var evidence: String
    public var id: String { speakerID + "|" + name }
}
public enum ConferenceReview {
    public static func excludedRanges(_ record: Recording) -> [ConferenceRange] {
        (record.conference?.ranges ?? []).filter { $0.valid && $0.confirmed && $0.excluded }
    }
    /// A reading/export projection. Original segments, audio, and source times never change.
    /// Edited or untimed boundary segments stay visible rather than losing unaligned words.
    public static func includedSegments(_ record: Recording) -> [Segment] {
        let ranges = excludedRanges(record)
        guard !ranges.isEmpty else { return record.segments }
        return record.segments.compactMap { segment in
            if ranges.contains(where: { $0.start <= segment.start && $0.end >= segment.end }) { return nil }
            guard segment.text == segment.originalText, !segment.words.isEmpty else { return segment }
            let words = segment.words.filter { word in !ranges.contains { $0.start <= word.start && $0.end >= word.end } }
            guard words.count != segment.words.count else { return segment }
            guard let first = words.first, let last = words.last else { return nil }
            var copy = segment; copy.words = words; copy.start = first.start; copy.end = last.end
            copy.text = TranscriptEditor.joinedWords(words); copy.originalText = copy.text
            return copy
        }
    }
    /// Only explicit verbal cues; audio alone cannot prove that a video was shown.
    public static func rangeProposals(_ record: Recording) -> [ConferenceRange] {
        let segments = record.segments
        var result: [ConferenceRange] = []
        let starts = ["virtual version", "watch a video", "watch the video", "show you a film", "play the video", "einspieler", "schauen wir", "kurzen film"]
        let returns = ["this movie", "that video", "back on stage", "back to the stage", "dieser film", "zurück auf"]
        for (i, segment) in segments.enumerated() where starts.contains(where: { segment.text.lowercased().contains($0) }) {
            guard i + 1 < segments.count else { continue }
            let following = segments[(i + 1)...].prefix { $0.start - segment.end < 600 }
            guard let closing = following.first(where: { candidate in candidate.start > segment.end + 15 && returns.contains(where: { candidate.text.lowercased().contains($0) }) }) else { continue }
            let start = following.first?.start ?? segment.end
            guard closing.start > start else { continue }
            result.append(ConferenceRange(start: start, end: closing.start, kind: L("Möglicher Einspieler"), evidence: L("Ankündigung: ") + String(segment.text.prefix(240)) + L(" · Rückkehr: ") + String(closing.text.prefix(160))))
        }
        return result
    }
    public static func speakerProposals(_ record: Recording) -> [SpeakerNameProposal] {
        let segments = includedSegments(record)
        let names = Array(Set(record.conference?.terms.map(\.term) ?? [])).filter { $0.split(separator: " ").count >= 2 }
        var result: [SpeakerNameProposal] = [], seen = Set<String>()
        for (i, segment) in segments.enumerated() {
            guard let speaker = segment.speakerID else { continue }
            let lower = segment.text.lowercased()
            let selfIntro = ["my name is ", "ich heiße ", "mein name ist "].compactMap { cue -> String? in
                guard let range = lower.range(of: cue) else { return nil }; return String(lower[range.upperBound...].prefix(100))
            }.first
            let previous = i > 0 ? segments[i - 1] : nil
            let handover = previous.map { $0.speakerID != speaker && segment.start - $0.end < 45 && ["hand over", "come on stage", "übergebe", "begrüßen"].contains(where: $0.text.lowercased().contains) } ?? false
            var candidates: [String] = []
            for name in names {
                let first = name.split(separator: " ").first.map(String.init) ?? name
                let firstUnique = names.filter { $0.split(separator: " ").first?.lowercased() == first.lowercased() }.count == 1
                if let selfIntro, selfIntro.hasPrefix(name.lowercased()) || (firstUnique && selfIntro.hasPrefix(first.lowercased() + ",")) { candidates.append(name) }
                else if handover, let previous, previous.text.localizedCaseInsensitiveContains(name) { candidates.append(name) }
            }
            // Ambiguous mentions never become identity suggestions.
            guard candidates.count == 1, let name = candidates.first, seen.insert(speaker + "|" + name).inserted else { continue }
            result.append(SpeakerNameProposal(speakerID: speaker, name: name, time: segment.start, evidence: (selfIntro != nil ? L("Selbstvorstellung: ") + segment.text : L("Übergabe: ") + (previous?.text ?? ""))))
        }
        return result
    }
}
