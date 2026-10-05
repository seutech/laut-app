import Foundation

public struct Word: Codable, Equatable, Sendable {
    public var text: String
    public var start: Double
    public var end: Double
    public init(text: String, start: Double, end: Double) { self.text = text; self.start = start; self.end = end }
}

public struct Segment: Identifiable, Codable, Equatable, Sendable {
    public var id: UUID = UUID()
    public var start: Double
    public var end: Double
    public var text: String
    public var originalText: String
    public var speakerID: String?
    public var speakerLocked: Bool = false
    public var words: [Word]
    public init(start: Double, end: Double, text: String, speakerID: String? = nil, words: [Word] = []) {
        self.start = start; self.end = end; self.text = text; self.originalText = text
        self.speakerID = speakerID; self.words = words
    }
}

public struct Speaker: Identifiable, Codable, Equatable, Sendable {
    public var id: String
    public var name: String
    public init(id: String = UUID().uuidString, name: String) { self.id = id; self.name = name }
}

public struct SpeakerTurn: Sendable {
    public var speakerID: String
    public var start: Double
    public var end: Double
    public init(speakerID: String, start: Double, end: Double) { self.speakerID = speakerID; self.start = start; self.end = end }
}

public enum DocumentKind: String, Codable, Sendable { case file, meeting, dictation, note }
public enum JobState: String, Codable, Sendable { case ready, processing, complete, failed }

public struct Recording: Identifiable, Codable, Sendable {
    public var id: UUID = UUID()
    public var title: String
    public var createdAt: Date = Date()
    public var kind: DocumentKind
    public var audioFilename: String?
    public var secondaryAudioFilename: String?
    public var duration: Double = 0
    public var segments: [Segment] = []
    public var originalSegments: [Segment]?
    public var speakers: [Speaker] = []
    public var notes: String = ""
    public var refinedText: String = ""
    public var state: JobState = .ready
    public var error: String?
    public var engine: String = ""
    public var processingSeconds: Double?
    public var modelLoadSeconds: Double?
    public var transcriptionSeconds: Double?
    public var audioPreparationSeconds: Double?
    public init(title: String, kind: DocumentKind = .file, audioFilename: String? = nil) {
        self.title = title; self.kind = kind; self.audioFilename = audioFilename
    }
    public func speakerName(_ id: String?) -> String { speakers.first { $0.id == id }?.name ?? "Unzugeordnet" }
    public var plainText: String { segments.map(\.text).joined(separator: " ") }
}

public struct VocabularyEntry: Identifiable, Codable, Sendable {
    public var id: UUID = UUID()
    public var term: String
    public var aliases: String
    public var active: Bool = true
    public init(term: String, aliases: String = "") { self.term = term; self.aliases = aliases }
}

public enum EngineKind: String, Codable, CaseIterable, Identifiable, Sendable {
    case phonon, parakeet, qwen
    public var id: String { rawValue }
    public var label: String {
        switch self { case .phonon: return "Phonon-2"; case .parakeet: return "Parakeet v3"; case .qwen: return "Qwen3-ASR 0.6B" }
    }
    public var modelID: String {
        switch self {
        case .phonon: return "phonon-2"
        case .parakeet: return "mlx-community/parakeet-tdt-0.6b-v3"
        case .qwen: return "mlx-community/Qwen3-ASR-0.6B-8bit"
        }
    }
    public var detail: String {
        switch self {
        case .phonon: return "164 MB Download · Fokus Englisch · Deutsch mit Parakeet vergleichen"
        case .parakeet: return "25 europäische Sprachen, auch Deutsch · ca. 1–3 GB"
        case .qwen: return "Experimentell · mehrsprachig · ca. 1 GB · nur Abschnittszeitmarken"
        }
    }
}

public struct AppSettings: Codable, Sendable {
    public var engine: EngineKind = .phonon
    public var modelPaths: [String: String] = [:]
    public var runtimeDirectory: String = ""
    public var language: String = "de"
    public var diarizationInstalled: Bool = false
    public var customInstructions: String = "Korrigiere Rechtschreibung und Zeichensetzung. Behalte Sprache, Bedeutung, Namen und Zahlen bei. Erfinde keine Inhalte. Gib nur den bearbeiteten Text zurück."
    public var llmModelPath: String = ""
    public var llmModelID: String = "mlx-community/Qwen3-1.7B-4bit"
    public var keepDictationAudio: Bool = false
    public init() {}
}

public enum TranscriptEditor {
    /// Manual edits always win over a fresh automatic analysis.
    public static func assign(_ turns: [SpeakerTurn], to segments: [Segment]) -> [Segment] {
        segments.flatMap { segment -> [Segment] in
            if segment.speakerLocked { return [segment] }
            let best: (Double, Double) -> String? = { start, end in
                let candidates = turns.map { ($0.speakerID, max(0, min(end, $0.end) - max(start, $0.start))) }
                let scores = Dictionary(grouping: candidates, by: { $0.0 }).mapValues { $0.reduce(0) { $0 + $1.1 } }
                return scores.filter { $0.value > 0 }.sorted { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }.first?.key
            }
            // Never overwrite edited text by reconstructing it from raw word timestamps.
            guard !segment.words.isEmpty, segment.text == segment.originalText else {
                var copy = segment; copy.speakerID = best(segment.start, segment.end); return [copy]
            }
            var groups: [Segment] = []
            for word in segment.words {
                let speaker = best(word.start, word.end)
                if let last = groups.indices.last, groups[last].speakerID == speaker {
                    groups[last].words.append(word); groups[last].end = word.end
                    groups[last].text = joinedWords(groups[last].words); groups[last].originalText = groups[last].text
                } else {
                    groups.append(Segment(start: word.start, end: word.end, text: word.text, speakerID: speaker, words: [word]))
                }
            }
            return groups
        }
    }
    public static func joinedWords(_ words: [Word]) -> String {
        words.map(\.text).joined(separator: " ")
            .replacingOccurrences(of: #"\s+([,.!?;:])"#, with: "$1", options: .regularExpression)
    }
    /// Explicit text split point avoids pretending that edited words still have exact timestamps.
    public static func split(_ segment: Segment, at time: Double, characterOffset: Int) -> [Segment] {
        guard time > segment.start, time < segment.end, characterOffset > 0, characterOffset < segment.text.count else { return [segment] }
        let index = segment.text.index(segment.text.startIndex, offsetBy: characterOffset)
        var left = Segment(start: segment.start, end: time, text: String(segment.text[..<index]).trimmingCharacters(in: .whitespaces), speakerID: segment.speakerID)
        var right = Segment(start: time, end: segment.end, text: String(segment.text[index...]).trimmingCharacters(in: .whitespaces), speakerID: segment.speakerID)
        left.speakerLocked = true; right.speakerLocked = true
        return [left, right]
    }
    public static func applyVocabulary(_ text: String, entries: [VocabularyEntry]) -> String {
        var result = text
        for entry in entries where entry.active && !entry.term.isEmpty {
            for alias in entry.aliases.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespaces) }).filter({ !$0.isEmpty }) {
                let pattern = "(?<![\\p{L}\\p{N}_])" + NSRegularExpression.escapedPattern(for: alias) + "(?![\\p{L}\\p{N}_])"
                guard let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive) else { continue }
                result = regex.stringByReplacingMatches(in: result, range: NSRange(result.startIndex..., in: result), withTemplate: NSRegularExpression.escapedTemplate(for: entry.term))
            }
        }
        return result
    }
}

public enum Exporter {
    public static func timestamp(_ seconds: Double) -> String {
        let s = max(0, Int(seconds)); return String(format: "%02d:%02d:%02d", s / 3600, (s % 3600) / 60, s % 60)
    }
    public static func markdown(_ recording: Recording) -> String {
        var output = "# \(recording.title)\n\n"
        for segment in recording.segments {
            output += "**\(recording.speakerName(segment.speakerID))** · \(timestamp(segment.start))\n\n\(segment.text)\n\n"
        }
        if !recording.notes.isEmpty { output += "## Notizen\n\n\(recording.notes)\n" }
        if !recording.refinedText.isEmpty { output += "\n## KI-Bearbeitung\n\n\(recording.refinedText)\n" }
        return output
    }
    public static func srt(_ recording: Recording) -> String {
        func time(_ value: Double) -> String { let ms = max(0, Int((value * 1000).rounded())); return timestamp(Double(ms / 1000)) + String(format: ",%03d", ms % 1000) }
        return recording.segments.enumerated().map { i, s in "\(i + 1)\n\(time(s.start)) --> \(time(s.end))\n\(recording.speakerName(s.speakerID)): \(s.text)\n" }.joined(separator: "\n")
    }
}
