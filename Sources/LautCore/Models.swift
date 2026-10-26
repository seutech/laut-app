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
    /// Naming a voice protects attribution without imposing a manual paragraph boundary.
    /// Missing in older recordings: retain their existing boundary behavior.
    public var allowsParagraphGrouping: Bool?
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

public struct SpeakerTurn: Codable, Sendable {
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
    public var source: SourceMetadata?
    public var conference: ConferenceContext?
    public var transcriptionProgress: Double?
    public var processingSeconds: Double?
    public var modelLoadSeconds: Double?
    public var transcriptionSeconds: Double?
    public var audioPreparationSeconds: Double?
    public var diarizationSeconds: Double?
    public var editHistory: EditHistory?
    public init(title: String, kind: DocumentKind = .file, audioFilename: String? = nil) {
        self.title = title; self.kind = kind; self.audioFilename = audioFilename
    }
    public func speakerName(_ id: String?) -> String { speakers.first { $0.id == id }?.name ?? L("Unzugeordnet") }
    public var plainText: String { ConferenceReview.includedSegments(self).map(\.text).joined(separator: " ") }
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
        case .phonon: return L("164 MB Download · Fokus Englisch · im deutschen Zweistimmentest wurden ganze Sätze ausgelassen")
        case .parakeet: return L("Empfohlen für deutsche Gespräche · 25 europäische Sprachen · ca. 1–3 GB")
        case .qwen: return L("Experimentell · mehrsprachig · ca. 1 GB · nur Abschnittszeitmarken")
        }
    }
}

public struct AppSettings: Codable, Sendable {
    // Missing in existing settings means German, preserving the previous interface.
    public var interfaceLanguage: InterfaceLanguage? = .system
    public var engine: EngineKind = .parakeet
    public var modelPaths: [String: String] = [:]
    public var runtimeDirectory: String = ""
    public var language: String = "de"
    public var diarizationInstalled: Bool = false
    // Optional storage keeps settings written by older versions decodable; missing means enabled.
    public var automaticSpeakerDetection: Bool?
    public var markdownDirectory: String?
    public var markdownExportEnabled: Bool?
    public var downloadToolPaths: [String: String]?
    public var searchMode: SearchMode?
    public var embeddingModel: EmbeddingModel?
    public var embeddingPaths: [String: String]?
    public var speakerDetectionEnabled: Bool {
        get { automaticSpeakerDetection ?? true }
        set { automaticSpeakerDetection = newValue }
    }
    public var customInstructions: String = L("Korrigiere Rechtschreibung und Zeichensetzung. Behalte Sprache, Bedeutung, Namen und Zahlen bei. Erfinde keine Inhalte. Gib nur den bearbeiteten Text zurück.")
    public var llmModelPath: String = ""
    public var llmModelID: String = "mlx-community/Qwen3-1.7B-4bit"
    public var keepDictationAudio: Bool = false
    public init() {}
}

public enum TranscriptEditor {
    public static func nameSpeaker(_ id: String, name: String, in recording: inout Recording) {
        guard let index = recording.speakers.firstIndex(where: { $0.id == id }) else { return }
        recording.speakers[index].name = name
        for i in recording.segments.indices where recording.segments[i].speakerID == id {
            if !recording.segments[i].speakerLocked { recording.segments[i].allowsParagraphGrouping = true }
            recording.segments[i].speakerLocked = true
        }
    }
    /// Manual edits always win over a fresh automatic analysis.
    public static func assign(_ turns: [SpeakerTurn], to segments: [Segment], repairGaps: Bool = false) -> [Segment] {
        var ordered = turns.enumerated().filter { $0.element.start.isFinite && $0.element.end.isFinite && $0.element.end > $0.element.start }
            .sorted { $0.element.start == $1.element.start ? $0.offset < $1.offset : $0.element.start < $1.element.start }.map(\.element)
        if repairGaps { ordered = SpeakerAlignment.bridgingShortGaps(ordered) }
        var prefixEnds: [Double] = [], latest = -Double.infinity
        for turn in ordered { latest = max(latest, turn.end); prefixEnds.append(latest) }
        // Search the overlapping time range once per word, rather than every turn in a long meeting.
        // Prefix maxima retain long/nested overlaps even when later turns have already ended.
        func firstIndex(_ predicate: (Int) -> Bool) -> Int {
            var low = 0, high = ordered.count
            while low < high {
                let middle = low + (high - low) / 2
                if predicate(middle) { high = middle } else { low = middle + 1 }
            }
            return low
        }
        func best(_ start: Double, _ end: Double) -> String? {
            guard start.isFinite, end.isFinite, end >= start, repairGaps || end > start else { return nil }
            // Some ASR tokens have a point timestamp. Look at that point, without stretching
            // ordinary words across a real speaker boundary.
            let queryEnd = end > start ? end : end.nextUp
            let lower = firstIndex { prefixEnds[$0] > start }, upper = firstIndex { ordered[$0].start >= queryEnd }
            var scores: [String: Double] = [:]
            if lower < upper {
                for i in lower..<upper {
                    let turn = ordered[i], overlap = min(queryEnd, turn.end) - max(start, turn.start)
                    if overlap > 0 { scores[turn.speakerID, default: 0] += overlap }
                }
            }
            if let match = scores.sorted(by: { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }).first?.key { return match }
            guard repairGaps else { return nil }
            // Only an entirely uncovered, short word/range can use a nearby turn edge.
            // Two different plausible voices remain ambiguous, even if one is closer.
            let tolerance = 0.75
            let from = firstIndex { prefixEnds[$0] >= end - tolerance }
            let to = firstIndex { ordered[$0].start > start + tolerance }
            var candidates = Set<String>()
            if from < to {
                for i in from..<to {
                    let turn = ordered[i]
                    if (turn.end <= start && end - turn.end <= tolerance) ||
                       (turn.start >= end && turn.start - start <= tolerance) {
                        candidates.insert(turn.speakerID)
                    }
                }
            }
            return candidates.count == 1 ? candidates.first : nil
        }
        return segments.flatMap { segment -> [Segment] in
            if segment.speakerLocked { return [segment] }
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
        if let source = recording.source {
            output += L("Quelle: ") + source.url + "\n\n"
            if let date = source.publishedOn { output += L("Veröffentlicht: ") + date + "\n\n" }
            if let publisher = source.publisher { output += L("Herausgeber: ") + publisher + "\n\n" }
        }
        output += ConferenceLayout.markdown(recording)
        if recording.transcriptionProgress != nil { output += L("\n**Unvollständiger Zwischenstand**\n\n") }
        if recording.conference != nil {
            output += ConferenceLayout.transcriptMarkdown(recording)
        } else {
            for segment in recording.segments {
                output += "**\(recording.speakerName(segment.speakerID))** · \(timestamp(ConferenceLayout.sourceTime(segment.start, recording: recording)))\n\n\(segment.text)\n\n"
            }
        }
        if !recording.notes.isEmpty { output += L("## Notizen\n\n\(recording.notes)\n") }
        if !recording.refinedText.isEmpty { output += L("\n## KI-Bearbeitung\n\n\(recording.refinedText)\n") }
        return output
    }
    public static func srt(_ recording: Recording) -> String {
        func time(_ value: Double) -> String { let ms = max(0, Int((value * 1000).rounded())); return timestamp(Double(ms / 1000)) + String(format: ",%03d", ms % 1000) }
        return ConferenceReview.includedSegments(recording).enumerated().map { i, s in "\(i + 1)\n\(time(s.start)) --> \(time(s.end))\n\(recording.speakerName(s.speakerID)): \(s.text)\n" }.joined(separator: "\n")
    }
}
