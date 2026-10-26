import Foundation

public struct MediaClip: Codable, Equatable, Sendable {
    public var start: Double
    public var end: Double
    public init(start: Double, end: Double) throws {
        guard start.isFinite, end.isFinite, start >= 0, end > start, end <= 86400 else {
            throw ImportJobError(L("Bitte einen Ausschnitt mit Ende nach Start innerhalb von 24 Stunden wählen."))
        }
        self.start = start; self.end = end
    }
    public var key: String { "@\(start)-\(end)" }
}

public struct TranscriptChapter: Codable, Identifiable, Equatable, Sendable {
    public var id = UUID()
    public var start: Double
    public var title: String
    public var origin: String
    public init(start: Double, title: String, origin: String = "Vorschlag") {
        self.start = start; self.title = title; self.origin = origin
    }
    public var originLabel: String {
        switch origin {
        case "Vorschlag": return L("Vorschlag")
        case "Manuell": return L("Manuell")
        case "KI-Vorschlag": return L("KI-Vorschlag")
        default: return origin
        }
    }
}

public struct DocumentPage: Codable, Identifiable, Equatable, Sendable {
    public var number: Int
    public var text: String
    public var extraction: String?
    public var ocrText: String?
    public var id: Int { number }
    public init(number: Int, text: String) { self.number = number; self.text = text }
}

public struct ConferenceDocument: Codable, Identifiable, Equatable, Sendable {
    public var id: UUID
    public var filename: String
    public var title: String
    public var pages: [DocumentPage]
    public var nameCandidates: [String]
    public init(id: UUID, filename: String, title: String, pages: [DocumentPage], nameCandidates: [String]) {
        self.id = id; self.filename = filename; self.title = title; self.pages = pages; self.nameCandidates = nameCandidates
    }
}

public struct ConferenceContext: Codable, Equatable, Sendable {
    public var documents: [ConferenceDocument] = []
    /// One canonical spelling per line, optionally followed by | and confirmed aliases.
    public var vocabulary: String = ""
    public var language: String = "en"
    public var chapters: [TranscriptChapter] = []
    public var slides: [ConferenceSlide] = []
    public var ranges: [ConferenceRange]?
    public init() {}
    public var terms: [VocabularyEntry] {
        vocabulary.split(separator: "\n").compactMap { line in
            let parts = line.split(separator: "|", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            guard let name = parts.first, !name.isEmpty else { return nil }
            return VocabularyEntry(term: name, aliases: parts.count > 1 ? parts[1] : "")
        }
    }
}

public struct ConferenceSlide: Codable, Identifiable, Equatable, Sendable {
    public var id = UUID()
    public var documentID: UUID
    public var page: Int
    public var start: Double
    public var confirmed: Bool
    public var useful: Bool?
    public var matchEvidence: String?
    public init(documentID: UUID, page: Int, start: Double, confirmed: Bool = false) {
        self.documentID = documentID; self.page = page; self.start = start; self.confirmed = confirmed
    }
}

public enum ConferenceLayout {
    /// Cheap text overlap proposes one PDF reference per chapter, never claims an on-screen match.
    public static func suggestedSlides(_ recording: Recording) -> [ConferenceSlide] {
        guard let context = recording.conference else { return [] }
        let stop = Set("this that with have from they were will your about into their there which would should could these those durch diese dieser einer einem einen nicht werden wurde haben sowie telekom deutsche investor presentation slide company".split(separator: " ").map(String.init))
        func tokens(_ text: String) -> Set<String> {
            Set(text.lowercased().split { !$0.isLetter && !$0.isNumber }.map(String.init).filter { $0.count >= 4 && !stop.contains($0) })
        }
        let pages = context.documents.flatMap { doc in doc.pages.filter { $0.text.split(whereSeparator: \.isWhitespace).count >= 15 }.map { (doc.id, $0.number, tokens($0.text)) } }
        var frequencies: [String: Int] = [:]
        for (_, _, terms) in pages { for term in terms { frequencies[term, default: 0] += 1 } }
        func weight(_ term: String) -> Double { log(1 + Double(pages.count) / Double(1 + (frequencies[term] ?? 0))) + 1 }
        var result: [ConferenceSlide] = [], used = Set<String>()
        for (i, chapter) in context.chapters.enumerated() {
            let end = i + 1 < context.chapters.count ? context.chapters[i + 1].start : recording.duration
            let words = tokens(ConferenceReview.includedSegments(recording).filter { $0.start >= chapter.start && $0.start < end }.map(\.text).joined(separator: " "))
            let ranked = pages.compactMap { id, number, terms -> (UUID, Int, Double)? in
                let shared = words.intersection(terms)
                let specific = shared.filter { (frequencies[$0] ?? 0) <= max(1, pages.count / 5) }
                guard shared.count >= 5, specific.count >= 2, !used.contains(id.uuidString + ":\(number)") else { return nil }
                let a = terms.reduce(0.0) { $0 + weight($1) }, b = words.reduce(0.0) { $0 + weight($1) }
                return (id, number, shared.reduce(0.0) { $0 + weight($1) } / sqrt(max(1, a * b)))
            }.sorted { $0.2 > $1.2 }
            if let best = ranked.first, best.2 > 0.18, ranked.count < 2 || best.2 - ranked[1].2 >= 0.025 {
                used.insert(best.0.uuidString + ":\(best.1)")
                var slide = ConferenceSlide(documentID: best.0, page: best.1, start: chapter.start)
                if let page = pages.first(where: { $0.0 == best.0 && $0.1 == best.1 }) {
                    slide.matchEvidence = L("Gemeinsame Begriffe: ") + words.intersection(page.2).sorted { weight($0) > weight($1) }.prefix(8).joined(separator: ", ")
                }
                result.append(slide)
            }
        }
        return result
    }
    /// Preserve published chapter times; fallback boundaries are explicitly only reading suggestions.
    public static func chapters(segments: [Segment], source: [TranscriptChapter] = []) -> [TranscriptChapter] {
        guard let first = segments.first, let last = segments.last else { return [] }
        let published = source.filter { $0.start.isFinite && $0.start >= 0 && $0.start <= last.end }.sorted { $0.start < $1.start }
        if !published.isEmpty { return published }
        var result = [TranscriptChapter(start: first.start, title: L("Beginn"))]
        var previous = first
        for segment in segments.dropFirst() {
            let elapsed = segment.start - result.last!.start
            if elapsed >= 240 && segment.text.split(whereSeparator: \.isWhitespace).count >= 6 &&
                (segment.start - previous.end >= 2 || segment.speakerID != previous.speakerID || elapsed >= 480) {
                let excerpt = segment.text.trimmingCharacters(in: .whitespacesAndNewlines)
                result.append(TranscriptChapter(start: segment.start, title: String(excerpt.prefix(72)) + (excerpt.count > 72 ? " …" : "")))
            }
            previous = segment
        }
        return result
    }
    public static func sourceTime(_ local: Double, recording: Recording) -> Double { local + (recording.source?.clipStart ?? 0) }
    public static func sourceURL(at local: Double, recording: Recording) -> URL? {
        guard let source = recording.source, var url = URLComponents(string: source.url), url.scheme == "https" else { return nil }
        if source.provider == "YouTube" {
            var query = (url.queryItems ?? []).filter { $0.name != "t" }
            query.append(URLQueryItem(name: "t", value: String(Int(sourceTime(local, recording: recording)))))
            url.queryItems = query
        }
        return url.url
    }
    public static func markdown(_ recording: Recording) -> String {
        guard let context = recording.conference else { return "" }
        var text = ""
        if let start = recording.source?.clipStart, let end = recording.source?.clipEnd {
            text += L("\nAusschnitt: \(Exporter.timestamp(start))–\(Exporter.timestamp(end)) im Original. Zeitmarken dieses Dokuments beziehen sich auf die Originalquelle; SRT-Zeiten auf die lokale Audiodatei.\n")
        }
        if let ranges = context.ranges, !ranges.isEmpty {
            text += L("\n## Zeitmarkierungen\n\n")
            for range in ranges where range.valid {
                text += "- \(Exporter.timestamp(sourceTime(range.start, recording: recording)))–\(Exporter.timestamp(sourceTime(range.end, recording: recording))) · \(range.kind) · \(range.confirmed && range.excluded ? "In Lesefassung ausgelassen; Original erhalten" : "Nicht ausgeschlossen")\n"
            }
        }
        if !context.chapters.isEmpty {
            text += L("\n## Kapitel\n\n")
            for chapter in context.chapters {
                let stamp = Exporter.timestamp(sourceTime(chapter.start, recording: recording))
                let reference = sourceURL(at: chapter.start, recording: recording).map { "[\(stamp)](\($0.absoluteString))" } ?? stamp
                text += "- \(reference) · \(chapter.title) (\(chapter.origin))\n"
            }
        }
        if !context.documents.isEmpty {
            text += L("\n## Unterlagen\n\n") + context.documents.map { L("- \($0.title) · \($0.pages.count) Seiten") }.joined(separator: "\n") + "\n"
        }
        if !context.slides.isEmpty {
            text += L("\n## Ausgewählte PDF-Seiten\n\n")
            for slide in context.slides.filter({ $0.confirmed && $0.useful == true }).sorted(by: { $0.start < $1.start }) {
                guard let document = context.documents.first(where: { $0.id == slide.documentID }) else { continue }
                text += L("- \(Exporter.timestamp(sourceTime(slide.start, recording: recording))) · \(document.title), Seite \(slide.page) · \(slide.confirmed ? "Manuell zugeordnet" : "Thematischer Vorschlag, ungeprüft")\n")
            }
        }
        return text
    }
    public static func transcriptMarkdown(_ recording: Recording) -> String {
        let chapters = (recording.conference?.chapters ?? []).sorted { $0.start < $1.start }
        var output = L("## Transkript\n\n"), nextChapter = 0
        for paragraph in TranscriptLayout.paragraphs(ConferenceReview.includedSegments(recording), boundaries: chapters.map(\.start)) {
            while nextChapter < chapters.count && chapters[nextChapter].start <= paragraph.start {
                output += "### " + chapters[nextChapter].title + "\n\n"
                nextChapter += 1
            }
            let speaker = recording.speakers.isEmpty ? "" : recording.speakerName(paragraph.speakerID) + " · "
            let stamp = Exporter.timestamp(sourceTime(paragraph.start, recording: recording))
            let reference = sourceURL(at: paragraph.start, recording: recording).map { "[\(stamp)](\($0.absoluteString))" } ?? stamp
            output += "**\(speaker)\(reference)**\n\n\(paragraph.text)\n\n"
        }
        return output
    }
}
