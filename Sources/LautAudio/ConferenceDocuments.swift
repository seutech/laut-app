import Foundation
import PDFKit
import Vision
import NaturalLanguage
import LautCore

public struct NameCorrectionProposal: Identifiable, Sendable {
    public var variant: String
    public var canonical: String
    public var excerpt: String
    public var time: Double
    public var id: String { variant + "|" + canonical }
}

public enum ConferenceDocuments {
    public static func corrections(segments: [Segment], terms: [VocabularyEntry]) async -> [NameCorrectionProposal] {
        let work = Task.detached(priority: .utility) {
            func normalized(_ value: String) -> String { value.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: Locale(identifier: "en_US_POSIX")) }
            func distance(_ a: String, _ b: String) -> Int {
                let a = Array(a), b = Array(b)
                var previous = Array(0...b.count)
                for (i, char) in a.enumerated() {
                    var row = [i + 1]
                    for (j, other) in b.enumerated() { row.append(min(row[j] + 1, previous[j + 1] + 1, previous[j] + (char == other ? 0 : 1))) }
                    previous = row
                }
                return previous.last ?? 0
            }
            var proposals: [NameCorrectionProposal] = [], seen = Set<String>()
            let canonical = Array(Set(terms.filter(\.active).map(\.term))).filter { $0.count >= 5 && $0.count <= 80 }
            for segment in segments {
                if Task.isCancelled { break }
                let words = segment.text.split { !$0.isLetter && $0 != "-" && $0 != "'" }.map(String.init)
                for term in canonical {
                    let count = term.split(separator: " ").count
                    guard count > 0, count <= 5, words.count >= count else { continue }
                    for i in 0...(words.count - count) where words[i].first?.isUppercase == true {
                        let variant = words[i..<(i + count)].joined(separator: " ")
                        guard variant != term, !seen.contains(variant + "|" + term), abs(variant.count - term.count) <= 2 else { continue }
                        let score = distance(normalized(variant), normalized(term))
                        if score <= (term.count >= 10 ? 2 : 1) {
                            seen.insert(variant + "|" + term)
                            proposals.append(NameCorrectionProposal(variant: variant, canonical: term, excerpt: String(segment.text.prefix(200)), time: segment.start))
                        }
                    }
                }
            }
            return proposals
        }
        return await withTaskCancellationHandler { await work.value } onCancel: { work.cancel() }
    }
    /// PDF text and OCR stay on device. Own copies keep references valid after originals move.
    public static func read(_ urls: [URL], into folder: URL) async throws -> [ConferenceDocument] {
        let work = Task.detached(priority: .utility) {
            let destination = folder.appendingPathComponent("documents")
            try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
            var result: [ConferenceDocument] = []
            var committed = false
            defer {
                if !committed {
                    for document in result { try? FileManager.default.removeItem(at: folder.appendingPathComponent(document.filename)) }
                }
            }
            for url in urls {
                try Task.checkCancellation()
                guard let pdf = PDFDocument(url: url), !pdf.isLocked, pdf.pageCount > 0 else { throw LocalEngineError(L("PDF nicht lesbar: \(url.lastPathComponent)")) }
                let id = UUID(), filename = "documents/" + UUID().uuidString + ".pdf"
                var pages: [DocumentPage] = [], names = Set<String>()
                for index in 0..<pdf.pageCount {
                    try Task.checkCancellation()
                    guard let page = pdf.page(at: index) else { continue }
                    let embedded = page.string ?? ""
                    var text = embedded
                    var extraction = L("PDF-Text")
                    if text.trimmingCharacters(in: .whitespacesAndNewlines).count < 20 {
                        extraction = L("Vision OCR (ungeprüft)")
                        let thumbnail = page.thumbnail(of: CGSize(width: 1600, height: 1600), for: .mediaBox)
                        if let image = thumbnail.cgImage(forProposedRect: nil, context: nil, hints: nil) {
                            let request = VNRecognizeTextRequest(); request.recognitionLevel = .accurate
                            request.automaticallyDetectsLanguage = true
                            try VNImageRequestHandler(cgImage: image).perform([request])
                            text = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
                        }
                    }
                    var extracted = DocumentPage(number: index + 1, text: embedded.isEmpty ? text : embedded); extracted.extraction = extraction
                    if extraction.hasPrefix("Vision"), !embedded.isEmpty { extracted.ocrText = text; extracted.extraction = L("PDF-Text + Vision OCR (ungeprüft)") }
                    pages.append(extracted)
                    let tagger = NLTagger(tagSchemes: [.nameType]); tagger.string = text
                    tagger.enumerateTags(in: text.startIndex..<text.endIndex, unit: .word, scheme: .nameType, options: [.joinNames, .omitWhitespace, .omitPunctuation]) { tag, range in
                        if tag == .personalName {
                            let name = String(text[range]).trimmingCharacters(in: .whitespacesAndNewlines)
                            if name.count > 4 && name.count < 80 && name.contains(" ") { names.insert(name) }
                        }
                        return true
                    }
                }
                try FileManager.default.copyItem(at: url, to: folder.appendingPathComponent(filename))
                result.append(ConferenceDocument(id: id, filename: filename, title: url.lastPathComponent, pages: pages, nameCandidates: names.sorted()))
            }
            try Task.checkCancellation()
            committed = true
            return result
        }
        return try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
    }

    public static func recognizePage(_ url: URL, number: Int) async throws -> String {
        let work = Task.detached(priority: .utility) {
            try Task.checkCancellation()
            guard let page = PDFDocument(url: url)?.page(at: number - 1),
                  let image = page.thumbnail(of: CGSize(width: 2200, height: 2200), for: .mediaBox).cgImage(forProposedRect: nil, context: nil, hints: nil) else { throw LocalEngineError(L("PDF-Seite nicht lesbar.")) }
            let request = VNRecognizeTextRequest(); request.recognitionLevel = .accurate; request.automaticallyDetectsLanguage = true
            try VNImageRequestHandler(cgImage: image).perform([request])
            try Task.checkCancellation()
            return (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
        }
        return try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
    }

    public static func pageURL(_ document: ConferenceDocument, folder: URL) -> URL? {
        guard document.filename.hasPrefix("documents/"), document.filename.hasSuffix(".pdf") else { return nil }
        return try? ConferenceMedia.url(document.filename, in: folder)
    }
}
