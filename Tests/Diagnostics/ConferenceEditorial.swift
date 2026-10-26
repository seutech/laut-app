import Foundation
import LautCore

/// Applies an explicit, evidence-backed review plan without changing the automatic source version.
enum ConferenceEditorial {
    struct Review: Decodable {
        var slug: String
        var url: String
        var names: [String: String]
        var notes: String
        var chapters: [Chapter]
        var corrections: [String: String]?
        var ranges: [ConferenceRange]?
        var slides: [ConferenceSlide]?
        var speakerOverrides: [String: String]?
    }
    struct Chapter: Decodable { var start: Double; var title: String }
    static func run(plan: URL) async throws {
        let root = plan.deletingLastPathComponent(), fm = FileManager.default
        let library = try Library(root: root.appendingPathComponent("library"))
        let reviews = try JSONDecoder().decode([Review].self, from: Data(contentsOf: plan))
        for review in reviews {
            guard review.slug.range(of: "^[a-z0-9-]+$", options: .regularExpression) != nil,
                  let original = try library.load().first(where: { $0.source?.url == review.url }), original.state == .complete else { throw ImportJobError("Ungültiger Review-Auftrag") }
            let destination = root.appendingPathComponent("vortraege/" + review.slug)
            let backup = root.appendingPathComponent("automatische-fassungen/" + review.slug)
            guard !fm.fileExists(atPath: backup.path) else { throw ImportJobError("Bereits redaktionell bearbeitet: " + review.slug) }
            let validIDs = Set(original.speakers.map(\.id))
            guard Set(review.names.keys).isSubset(of: validIDs), review.chapters.allSatisfy({ $0.start >= 0 && $0.start < original.duration }) else { throw ImportJobError("Review verweist auf unbekannte Sprecher/Zeit") }
            guard Set((review.speakerOverrides ?? [:]).values).isSubset(of: validIDs),
                  Set((review.speakerOverrides ?? [:]).keys).isSubset(of: Set(original.segments.map { $0.id.uuidString })) else { throw ImportJobError("Ungültige Sprecherkorrektur") }
            let record = RecordingEditor.edit(original, label: "Kapitel und belegte Sprecherzuordnungen prüfen") { record in
                for i in record.speakers.indices { if let name = review.names[record.speakers[i].id] { record.speakers[i].name = name } }
                for i in record.segments.indices {
                    if let speaker = review.speakerOverrides?[record.segments[i].id.uuidString] {
                        record.segments[i].speakerID = speaker; record.segments[i].speakerLocked = true
                    }
                }
                let terms = (review.corrections ?? [:]).map { VocabularyEntry(term: $0.value, aliases: $0.key) }
                for i in record.segments.indices { record.segments[i].text = TranscriptEditor.applyVocabulary(record.segments[i].text, entries: terms) }
                if !terms.isEmpty { record.conference?.vocabulary += "\n" + terms.map { $0.term + " | " + $0.aliases }.joined(separator: "\n") }
                if let ranges = review.ranges {
                    let combined = (record.conference?.ranges ?? []) + ranges
                    record.conference?.ranges = combined
                }
                if !review.chapters.isEmpty {
                    record.conference?.chapters = review.chapters.sorted { $0.start < $1.start }.map { TranscriptChapter(start: $0.start, title: $0.title, origin: "Inhaltlich gegliedert; Zeitgrenzen näherungsweise") }
                    let slides = ConferenceLayout.suggestedSlides(record)
                    record.conference?.slides = slides
                }
                if let slides = review.slides { record.conference?.slides = slides }
                record.notes += "\n\nRedaktionelle Strukturprüfung: " + review.notes
            }
            let staging = root.appendingPathComponent("vortraege/.editorial-staging-" + review.slug)
            try ConferenceArchive.export(record, sourceFolder: library.folder(record.id), to: staging, sharedRoot: root)
            let proposalData = try Data(contentsOf: destination.appendingPathComponent("pruefvorschlaege.json"))
            var proposals = try JSONSerialization.jsonObject(with: proposalData) as! [String: Any]
            proposals["status"] = "Automatische Vorschläge; recording.json enthält den aktuellen redaktionellen Prüfstand."
            proposals["editorialNotes"] = review.notes
            try JSONSerialization.data(withJSONObject: proposals, options: [.prettyPrinted, .sortedKeys]).write(to: staging.appendingPathComponent("pruefvorschlaege.json"))
            try fm.createDirectory(at: backup.deletingLastPathComponent(), withIntermediateDirectories: true)
            try fm.moveItem(at: destination, to: backup)
            try fm.moveItem(at: staging, to: destination)
            try library.save(record)
            print("REVIEWED " + review.slug)
        }
        let index = SearchIndex(url: root.appendingPathComponent("search.sqlite"))
        try await index.synchronize(library.load())
    }
}
