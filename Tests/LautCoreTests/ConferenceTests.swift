import Foundation
import LautCore

enum ConferenceTests {
    static func run() throws {
        for bounds in [(Double.nan, 1800.0), (-1, 1800), (1800, 1800), (0, 86401)] {
            var rejected = false
            do { _ = try MediaClip(start: bounds.0, end: bounds.1) } catch { rejected = true }
            XCTAssertTrue(rejected)
        }
        let clip = try MediaClip(start: 600, end: 2400)
        XCTAssertEqual(clip.end - clip.start, 1800)
        XCTAssertTrue(clip.key != (try MediaClip(start: 0, end: 1800)).key)
        var recording = Recording(title: "Konferenz")
        recording.source = SourceMetadata(url: "https://www.youtube.com/watch?v=upGaBbzTHIY", provider: "YouTube")
        recording.source?.clipStart = 600; recording.source?.clipEnd = 2400
        recording.duration = 1800
        recording.segments = [Segment(start: 0, end: 40, text: "Tim Hötges beschreibt networks infrastructure automation efficiency customers."), Segment(start: 500, end: 560, text: "New section about enterprise software engineering productivity.")]
        var context = ConferenceContext(); context.vocabulary = "Tim Höttges | Tim Hötges"
        context.chapters = ConferenceLayout.chapters(segments: recording.segments)
        XCTAssertEqual(context.chapters.count, 2)
        XCTAssertEqual(context.chapters[1].start, 500)
        XCTAssertEqual(ConferenceLayout.chapters(segments: recording.segments, source: [TranscriptChapter(start: 0, title: "Original", origin: "YouTube")]).count, 1)
        let doc = ConferenceDocument(id: UUID(), filename: "documents/test.pdf", title: "Präsentation", pages: [DocumentPage(number: 1, text: "networks infrastructure automation efficiency customers use software engineering to improve productivity and lower costs across operations")], nameCandidates: ["Tim Höttges"])
        context.documents = [doc]; recording.conference = context
        let suggestions = ConferenceLayout.suggestedSlides(recording)
        XCTAssertEqual(suggestions.count, 1); XCTAssertTrue(!suggestions[0].confirmed)
        XCTAssertEqual(ConferenceLayout.sourceTime(30, recording: recording), 630)
        XCTAssertTrue(ConferenceLayout.sourceURL(at: 30, recording: recording)!.absoluteString.contains("t=630"))
        let corrected = RecordingEditor.edit(recording, label: "Name prüfen") { record in
            record.segments[0].text = TranscriptEditor.applyVocabulary(record.segments[0].text, entries: context.terms)
            record.conference?.chapters[0].title = "KI-Netze"
        }
        XCTAssertTrue(corrected.segments[0].text.contains("Höttges"))
        XCTAssertEqual(corrected.segments[0].originalText, recording.segments[0].text)
        let reopened = try JSONDecoder().decode(Recording.self, from: JSONEncoder().encode(corrected))
        let undone = try XCTUnwrap(RecordingEditor.undo(reopened))
        XCTAssertEqual(undone.conference, recording.conference); XCTAssertEqual(undone.segments, recording.segments)
        XCTAssertTrue(Exporter.markdown(corrected).contains("00:10:00"))
        XCTAssertTrue(Exporter.markdown(corrected).contains("### KI-Netze"))
        XCTAssertTrue(try MarkdownArchive.markdown(corrected).contains("KI-Netze"))
        recording.transcriptionProgress = 0.5
        XCTAssertTrue(Exporter.markdown(recording).contains("Unvollständiger Zwischenstand"))
        let adjacent = [Segment(start: 0, end: 10, text: "A"), Segment(start: 10, end: 20, text: "B")]
        XCTAssertEqual(TranscriptLayout.paragraphs(adjacent, boundaries: [10]).count, 2)
        var legacy = try JSONSerialization.jsonObject(with: JSONEncoder().encode(recording)) as! [String: Any]
        legacy.removeValue(forKey: "conference"); legacy.removeValue(forKey: "transcriptionProgress")
        XCTAssertTrue(try JSONDecoder().decode(Recording.self, from: JSONSerialization.data(withJSONObject: legacy)).conference == nil)
        var review = Recording(title: "Review")
        review.duration = 300; review.conference = ConferenceContext()
        review.segments = [Segment(start: 0, end: 10, text: "Opening", speakerID: "host"),
                           Segment(start: 10, end: 20, text: "Watch the video", speakerID: "host"),
                           Segment(start: 21, end: 60, text: "Promotional film", speakerID: "film"),
                           Segment(start: 65, end: 80, text: "This movie shows the future", speakerID: "person")]
        let proposals = ConferenceReview.rangeProposals(review)
        XCTAssertEqual(proposals.count, 1)
        review.conference?.ranges = proposals
        XCTAssertEqual(ConferenceReview.includedSegments(review).count, 4)
        review.conference?.ranges?[0].confirmed = true; review.conference?.ranges?[0].excluded = true
        XCTAssertEqual(ConferenceReview.includedSegments(review).count, 3)
        XCTAssertEqual(review.segments.count, 4)
        XCTAssertTrue(!Exporter.srt(review).contains("Promotional film"))
        XCTAssertTrue(!SearchPassage.passages([review]).contains { $0.text.contains("Promotional film") })
        var boundary = Segment(start: 50, end: 70, text: "kept hidden", words: [Word(text: "kept", start: 50, end: 55), Word(text: "hidden", start: 60, end: 70)])
        var range = ConferenceRange(start: 60, end: 80, kind: "Test", evidence: "Test"); range.confirmed = true; range.excluded = true
        review.conference?.ranges = [range]; review.segments = [boundary]
        XCTAssertEqual(ConferenceReview.includedSegments(review)[0].text, "kept")
        boundary.text = "Edited words without safe alignment"; review.segments = [boundary]
        XCTAssertEqual(ConferenceReview.includedSegments(review)[0].text, boundary.text)
        review.conference?.ranges = nil; review.conference?.vocabulary = "Jane Doe\nJohn Smith"
        review.segments = [Segment(start: 0, end: 30, text: "My name is Jane Doe and welcome.", speakerID: "one")]
        XCTAssertEqual(ConferenceReview.speakerProposals(review).first?.name, "Jane Doe")
        review.segments[0].text = "We thank Jane Doe for her presentation."
        XCTAssertTrue(ConferenceReview.speakerProposals(review).isEmpty)
        review.conference?.documents = [doc]
        let pdfPassage = SearchPassage.passages([review]).first { $0.documentID == doc.id }
        XCTAssertEqual(pdfPassage?.pageNumber, 1)
        XCTAssertTrue(pdfPassage?.start == nil)
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        try ConferenceArchive.writeDocuments(review, folder: folder)
        let sidecar = folder.appendingPathComponent("derived/documents/" + doc.id.uuidString + "/text.txt")
        XCTAssertTrue(try String(contentsOf: sidecar, encoding: .utf8).contains(doc.pages[0].text))
        try FileManager.default.createDirectory(at: folder.appendingPathComponent("documents"), withIntermediateDirectories: true)
        let pdfBytes = Data("Original PDF test bytes".utf8)
        try pdfBytes.write(to: folder.appendingPathComponent(doc.filename))
        let package = folder.appendingPathComponent("export")
        try ConferenceArchive.export(review, sourceFolder: folder, to: package)
        XCTAssertEqual(try Data(contentsOf: package.appendingPathComponent(doc.filename)), pdfBytes)
        XCTAssertTrue(FileManager.default.fileExists(atPath: package.appendingPathComponent("transcript-original.txt").path))
        XCTAssertEqual(try JSONDecoder().decode(Recording.self, from: Data(contentsOf: package.appendingPathComponent("recording.json"))).segments, review.segments)
        var migration = try JSONSerialization.jsonObject(with: JSONEncoder().encode(review)) as! [String: Any]
        var oldContext = migration["conference"] as! [String: Any]; oldContext.removeValue(forKey: "ranges"); migration["conference"] = oldContext
        XCTAssertTrue(try JSONDecoder().decode(Recording.self, from: JSONSerialization.data(withJSONObject: migration)).conference?.ranges == nil)
        // One immutable original across editable library and versioned exports; relative
        // references still work after relocation, and standalone export materializes files.
        let shared = folder.appendingPathComponent("shared")
        let lib = try Library(root: shared.appendingPathComponent("library"))
        var sharedRecord = review; sharedRecord.audioFilename = "audio.m4a"
        let local = lib.folder(sharedRecord.id)
        try FileManager.default.createDirectory(at: local.appendingPathComponent("documents"), withIntermediateDirectories: true)
        try pdfBytes.write(to: local.appendingPathComponent(doc.filename))
        let audioBytes = Data("original audio".utf8)
        try audioBytes.write(to: local.appendingPathComponent("audio.m4a"))
        try lib.save(sharedRecord)
        try ConferenceMedia.compact(sharedRecord, folder: local, root: shared)
        XCTAssertTrue(!FileManager.default.fileExists(atPath: local.appendingPathComponent("audio.m4a").path))
        for name in ["vortraege/one", "automatische-fassungen/one"] {
            try ConferenceArchive.export(sharedRecord, sourceFolder: local, to: shared.appendingPathComponent(name), sharedRoot: shared)
        }
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: shared.appendingPathComponent("assets").path).count, 2)
        XCTAssertTrue(try String(contentsOf: shared.appendingPathComponent("vortraege/one/transcript.md"), encoding: .utf8).contains("../../assets/"))
        let moved = folder.appendingPathComponent("moved")
        try FileManager.default.moveItem(at: shared, to: moved)
        let movedLibrary = try Library(root: moved.appendingPathComponent("library"))
        XCTAssertEqual(try Data(contentsOf: XCTUnwrap(movedLibrary.audioURL(sharedRecord))), audioBytes)
        let portable = folder.appendingPathComponent("standalone")
        try ConferenceArchive.export(sharedRecord, sourceFolder: moved.appendingPathComponent("vortraege/one"), to: portable)
        XCTAssertEqual(try Data(contentsOf: portable.appendingPathComponent(doc.filename)), pdfBytes)
        XCTAssertEqual(try Data(contentsOf: portable.appendingPathComponent("audio.m4a")), audioBytes)
        let resolvedAudio = try XCTUnwrap(movedLibrary.audioURL(sharedRecord))
        try Data("damaged".utf8).write(to: resolvedAudio)
        var corruptSourceRejected = false
        do { try ConferenceMedia.compact(sharedRecord, folder: movedLibrary.folder(sharedRecord.id), root: moved) } catch { corruptSourceRejected = true }
        XCTAssertTrue(corruptSourceRejected)
        try audioBytes.write(to: movedLibrary.folder(sharedRecord.id).appendingPathComponent("audio.m4a"))
        var corruptRejected = false
        do { try ConferenceMedia.attach(movedLibrary.folder(sharedRecord.id).appendingPathComponent("audio.m4a"), logical: "audio.m4a", folder: movedLibrary.folder(sharedRecord.id), root: moved, removeLocal: true) } catch { corruptRejected = true }
        XCTAssertTrue(corruptRejected)
        XCTAssertTrue(FileManager.default.fileExists(atPath: movedLibrary.folder(sharedRecord.id).appendingPathComponent("audio.m4a").path))
        print("Conference checks passed: clip bounds, source offsets, chapter boundaries, PDF suggestions, reversible names/context, partial export and migration")
    }
}
