import Foundation

public enum ConferenceArchive {
    public static func export(_ record: Recording, sourceFolder: URL, to destination: URL, sharedRoot: URL? = nil) throws {
        let fm = FileManager.default
        guard !fm.fileExists(atPath: destination.path) else { throw ImportJobError(L("Zielordner existiert bereits.")) }
        try fm.createDirectory(at: destination, withIntermediateDirectories: true)
        try fm.createDirectory(at: destination.appendingPathComponent("documents"), withIntermediateDirectories: true)
        for doc in record.conference?.documents ?? [] {
            let relative = doc.filename
            guard relative.hasPrefix("documents/"), relative.split(separator: "/").count == 2, !relative.contains("..") else { throw ImportJobError(L("Ungültiger PDF-Pfad.")) }
            let source = try ConferenceMedia.url(relative, in: sourceFolder)
            if let sharedRoot { try ConferenceMedia.attach(source, logical: relative, folder: destination, root: sharedRoot) }
            else { try fm.copyItem(at: source, to: destination.appendingPathComponent(relative)) }
        }
        if let audio = record.audioFilename, audio == (audio as NSString).lastPathComponent {
            let source = try ConferenceMedia.url(audio, in: sourceFolder)
            if let sharedRoot { try ConferenceMedia.attach(source, logical: audio, folder: destination, root: sharedRoot) }
            else { try fm.copyItem(at: source, to: destination.appendingPathComponent(audio)) }
        }
        try writeDocuments(record, folder: destination)
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(record).write(to: destination.appendingPathComponent("recording.json"), options: .atomic)
        var text = Exporter.markdown(record)
        text += L("\n## Original-PDFs\n\n")
        func link(_ filename: String) throws -> String {
            let path = ConferenceMedia.relative(try ConferenceMedia.url(filename, in: destination), from: destination)
            return path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? path
        }
        for doc in record.conference?.documents ?? [] { text += "- [" + doc.title + "](" + (try link(doc.filename)) + ")\n" }
        for slide in record.conference?.slides ?? [] where slide.confirmed && slide.useful == true {
            if let doc = record.conference?.documents.first(where: { $0.id == slide.documentID }) { text += L("- [Geprüfte Referenz · Seite \(slide.page)](\(try link(doc.filename))#page=\(slide.page)) · \(Exporter.timestamp(slide.start))\n") }
        }
        try Data(text.utf8).write(to: destination.appendingPathComponent("transcript.md"), options: .atomic)
        try Data(Exporter.srt(record).utf8).write(to: destination.appendingPathComponent("transcript.srt"), options: .atomic)
        try Data(ConferenceReview.includedSegments(record).map(\.text).joined(separator: "\n\n").utf8).write(to: destination.appendingPathComponent("transcript.txt"), options: .atomic)
        let raw = (record.originalSegments ?? record.segments).map { "[" + Exporter.timestamp($0.start) + "] " + $0.originalText }.joined(separator: "\n\n")
        try Data(raw.utf8).write(to: destination.appendingPathComponent("transcript-original.txt"), options: .atomic)
        let storage = sharedRoot == nil ? L("Originale: Audio + documents/*.pdf.") : L("Originale: einmalig im gemeinsamen assets-Ordner des Konferenzarchivs. media-references.json ordnet die logischen Dateinamen zu. Den gesamten Konferenzordner zusammen aufbewahren.")
        try Data((storage + L(" Suchtexte: derived/documents. recording.json enthält Originalsegmente, Korrekturen und Markierungen. transcript.* ist die Lesefassung mit bestätigten Ausschlüssen. PDF-Text/OCR kann Grafiken, Tabellenbeziehungen und Layout nicht verlustfrei ersetzen. Es wurde keine Videodatei archiviert.\n")).utf8).write(to: destination.appendingPathComponent("ABLAGE.txt"), options: .atomic)
    }
    private static func writeChanged(_ data: Data, to url: URL) throws {
        if (try? Data(contentsOf: url)) != data { try data.write(to: url, options: .atomic) }
    }
    /// Disposable sidecars next to unchanged source PDFs. Never replace the PDF with extracted text.
    public static func writeDocuments(_ record: Recording, folder: URL) throws {
        guard let context = record.conference else { return }
        let root = folder.appendingPathComponent("derived/documents")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        func reference(_ doc: ConferenceDocument) -> String {
            (try? ConferenceMedia.url(doc.filename, in: folder)).map { ConferenceMedia.relative($0, from: folder) } ?? doc.filename
        }
        for document in context.documents {
            let directory = root.appendingPathComponent(document.id.uuidString)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            var text = document.title + L("\nOriginal: ") + reference(document) + L("\nText ist eine Suchhilfe; Grafiken, Layout und OCR immer am PDF prüfen.\n")
            for page in document.pages {
                text += L("\n--- Seite \(page.number) · \(page.extraction ?? "Bestandsextraktion; Methode unbekannt") ---\n") + page.text + "\n"
                if let ocr = page.ocrText { text += L("\nBildtext (OCR, ungeprüft):\n") + ocr + "\n" }
            }
            try writeChanged(Data(text.utf8), to: directory.appendingPathComponent("text.txt"))
            try writeChanged(encoder.encode(document), to: directory.appendingPathComponent("pages.json"))
        }
        let manifest = context.documents.map { ["id": $0.id.uuidString, "title": $0.title, "original": reference($0), "text": "derived/documents/" + $0.id.uuidString + "/text.txt", "pages": String($0.pages.count)] }
        try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("manifest.json"), options: .atomic)
    }
}
