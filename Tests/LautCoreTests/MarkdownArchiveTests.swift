import Foundation
import LautCore

struct MarkdownArchiveTests {
    static func run() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("laut-markdown-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        let archive = MarkdownArchive()
        var record = Recording(title: "Projekt / Alpha", kind: .note)
        record.createdAt = Date(timeIntervalSince1970: 0); record.notes = "Eigene Notizen"; record.refinedText = "Lokale Auswertung"
        let filename = MarkdownArchive.filename(record, timeZone: TimeZone(secondsFromGMT: 0)!)
        XCTAssertEqual(filename, "1970-01-01 Projekt Alpha.md")
        var report = try await archive.synchronize([record], directory: folder)
        XCTAssertEqual(report.written, 1)
        let original = folder.appendingPathComponent(MarkdownArchive.filename(record))
        let content = try String(contentsOf: original, encoding: .utf8)
        XCTAssertTrue(content.contains("laut_id:")); XCTAssertTrue(content.contains("## Notizen")); XCTAssertTrue(content.contains("## Auswertung"))
        report = try await archive.synchronize([record], directory: folder); XCTAssertEqual(report.written, 0)
        record.notes = "In Laut korrigiert"
        report = try await archive.synchronize([record], directory: folder); XCTAssertEqual(report.written, 1)
        XCTAssertTrue(try String(contentsOf: original, encoding: .utf8).contains("In Laut korrigiert"))
        try Data("Extern bearbeitet".utf8).write(to: original)
        record.notes = "Weitere Änderung"
        report = try await archive.synchronize([record], directory: folder); XCTAssertEqual(report.conflicts, 1)
        XCTAssertEqual(try String(contentsOf: original, encoding: .utf8), "Extern bearbeitet")
        let duplicate = folder.appendingPathComponent((original.lastPathComponent as NSString).deletingPathExtension + " (2).md")
        XCTAssertTrue(try String(contentsOf: duplicate, encoding: .utf8).contains("Weitere Änderung"))
        report = try await archive.synchronize([record], directory: folder); XCTAssertEqual(report.written, 0)
        record.title = "Neuer Titel"
        _ = try await archive.synchronize([record], directory: folder)
        XCTAssertTrue(!FileManager.default.fileExists(atPath: duplicate.path)); XCTAssertTrue(FileManager.default.fileExists(atPath: original.path))
        var second = record; second.id = UUID(); second.notes = "Anderer Eintrag"
        _ = try await archive.synchronize([record, second], directory: folder)
        let files = try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil).filter { $0.pathExtension == "md" }
        XCTAssertEqual(files.count, 3)
        _ = try await archive.synchronize([], directory: folder)
        XCTAssertTrue(files.allSatisfy { FileManager.default.fileExists(atPath: $0.path) })
        second.title = String(repeating: "👋", count: 200)
        XCTAssertTrue(MarkdownArchive.filename(second).utf8.count < 255)
        let outside = folder.appendingPathComponent("preserve.txt"); try Data("unverändert".utf8).write(to: outside)
        let symlink = folder.appendingPathComponent(MarkdownArchive.filename(second))
        try FileManager.default.createSymbolicLink(at: symlink, withDestinationURL: outside)
        _ = try await archive.synchronize([second], directory: folder)
        XCTAssertEqual(try String(contentsOf: outside, encoding: .utf8), "unverändert")
        try Data("broken manifest".utf8).write(to: folder.appendingPathComponent(".laut-archive.json"))
        var refused = false
        do { _ = try await archive.synchronize([record], directory: folder) } catch { refused = true }
        XCTAssertTrue(refused)
        print("Markdown archive checks passed: naming, idempotency, updates, conflicts, renames, collisions, retention, Unicode and symlink protection")
    }
}
