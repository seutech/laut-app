import Foundation
import LautCore

enum ImportJobTests {
    static func run() throws {
        let link = try SourceLink.parse("https://www.youtube.com/watch?v=YE7VzlLtp-4")
        for input in [" https://youtu.be/YE7VzlLtp-4?t=10 ", "https://m.youtube.com/watch?v=YE7VzlLtp-4&list=ignored", "https://www.youtube.com/shorts/YE7VzlLtp-4", "https://youtube.com/live/YE7VzlLtp-4"] {
            XCTAssertEqual(try SourceLink.parse(input), link)
        }
        for input in ["http://youtu.be/YE7VzlLtp-4", "https://user:secret@example.com/a.mp3", "https://youtube.com/playlist?list=abc", "https://youtube.com/watch?v=bad", "https://youtube.com.evil.example/watch?v=YE7VzlLtp-4", "https://127.0.0.1/a.mp3", "https://localhost/a.mp3", "https://server.local/a.mp3", "https://example.com:8443/a.mp3", "https://example.com/a.mp4"] {
            var rejected = false
            do { _ = try SourceLink.parse(input) } catch { rejected = true }
            XCTAssertTrue(rejected)
        }
        XCTAssertEqual(try SourceLink.parse("https://example.com/audio.MP3?key=a#t=5").url.absoluteString, "https://example.com/audio.MP3?key=a")

        let root = FileManager.default.temporaryDirectory.appendingPathComponent("laut-import-check-" + UUID().uuidString)
        let library = try Library(root: root)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ImportJobStore(root: root)
        var jobs = [ImportStage.queued, .downloading, .importing, .transcribing, .complete, .failed, .paused].map { stage -> ImportJob in
            var job = ImportJob(kind: .link, input: link.url.absoluteString, sourceKey: link.key, title: "Quelle")
            job.stage = stage; return job
        }
        try store.save(jobs)
        let recovered = try store.load()
        XCTAssertEqual(recovered.map(\.stage), [.paused, .paused, .paused, .paused, .complete, .failed, .paused])
        XCTAssertEqual(recovered.map(\.recordingID), jobs.map(\.recordingID))
        XCTAssertEqual(recovered.map(\.id), jobs.map(\.id))
        try store.save(recovered)
        XCTAssertEqual(try store.load().map(\.stage), recovered.map(\.stage))
        jobs.append(jobs[0]); try store.save(jobs)
        var rejected = false
        do { _ = try store.load() } catch { rejected = true }
        XCTAssertTrue(rejected)
        let damaged = Data("not json".utf8), ledger = root.appendingPathComponent("import-jobs.json")
        try damaged.write(to: ledger)
        rejected = false
        do { _ = try store.load() } catch { rejected = true }
        XCTAssertTrue(rejected); XCTAssertEqual(try Data(contentsOf: ledger), damaged)

        var record = Recording(title: "Quartalszahlen")
        var metadata = SourceMetadata(url: link.url.absoluteString, provider: link.provider, importedAt: Date(timeIntervalSince1970: 1_800_000_000))
        metadata.publishedOn = "2025-02-21"; metadata.publisher = "Beispiel AG"; metadata.language = "de"
        record.source = metadata; try library.save(record)
        XCTAssertEqual(try library.load().first?.source, metadata)
        let markdown = try MarkdownArchive.markdown(record)
        XCTAssertTrue(markdown.contains("2025-02-21") && markdown.contains("Beispiel AG") && markdown.contains(link.url.absoluteString))
        XCTAssertTrue(Exporter.markdown(record).contains("2025-02-21"))
        var legacy = try JSONSerialization.jsonObject(with: JSONEncoder().encode(record)) as! [String: Any]
        legacy.removeValue(forKey: "source")
        XCTAssertTrue(try JSONDecoder().decode(Recording.self, from: JSONSerialization.data(withJSONObject: legacy)).source == nil)
        var oldSettings = try JSONSerialization.jsonObject(with: JSONEncoder().encode(AppSettings())) as! [String: Any]
        oldSettings.removeValue(forKey: "downloadToolPaths")
        XCTAssertTrue(try JSONDecoder().decode(AppSettings.self, from: JSONSerialization.data(withJSONObject: oldSettings)).downloadToolPaths == nil)
        print("Import checks passed: URL normalization/rejection, durable stages/IDs, restart pause, corrupt ledger preservation, source/archive metadata and migration")
    }
}
