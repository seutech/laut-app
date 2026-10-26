import Foundation
import LautCore

enum RecoveryTests {
    static func rejects(_ action: () throws -> Void) -> Bool {
        do { try action(); return false } catch { return true }
    }
    static func run() async throws {
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent("laut-recovery-" + UUID().uuidString)
        defer { try? fm.removeItem(at: root) }
        let source = root.appendingPathComponent("original")
        let library = try Library(root: source)
        var record = Recording(title: "Höttges – Prüfung", audioFilename: "Original.m4a")
        record.segments = [Segment(start: 0, end: 2, text: "Unveränderter Text.")]
        record.originalSegments = record.segments
        record.notes = "Eigene Anmerkungen"
        record.state = .complete
        try library.save(record)
        let audio = library.folder(record.id).appendingPathComponent(record.audioFilename!)
        try Data("original audio bytes".utf8).write(to: audio)
        let snapshot = try Data(contentsOf: library.folder(record.id).appendingPathComponent("recording.json"))
        try library.saveSettings(AppSettings())
        try library.saveVocabulary([VocabularyEntry(term: "Höttges")])
        var job = ImportJob(kind: .recording, input: "", sourceKey: "test", title: "Pause")
        job.stage = .transcribing
        job.documentPaths = [source.appendingPathComponent("import-jobs/attachment.pdf").path]
        try ImportJobStore(root: source).save([job])
        let backup = root.appendingPathComponent("Sicherung")
        let manifest = try LibraryBackup.create(from: source, to: backup)
        XCTAssertTrue(manifest.files.count >= 5)
        let restored = root.appendingPathComponent("Wiederhergestellt")
        try LibraryBackup.restore(backup, to: restored)
        let restoredLibrary = try Library(root: restored)
        XCTAssertEqual(try restoredLibrary.loadSettings().markdownDirectory, restored.appendingPathComponent("markdown-archive").path)
        XCTAssertEqual(try Data(contentsOf: restoredLibrary.folder(record.id).appendingPathComponent("recording.json")), snapshot)
        XCTAssertEqual(try Data(contentsOf: XCTUnwrap(restoredLibrary.audioURL(record))), Data("original audio bytes".utf8))
        let resumedJobs = try ImportJobStore(root: restored).load()
        XCTAssertEqual(resumedJobs[0].stage, .paused)
        XCTAssertTrue(resumedJobs[0].documentPaths![0].hasPrefix(restored.path + "/"))
        XCTAssertTrue(rejects { try LibraryBackup.restore(backup, to: source) })
        XCTAssertTrue(rejects { _ = try LibraryBackup.create(from: source, to: source.appendingPathComponent("nested")) })
        XCTAssertEqual(try Data(contentsOf: library.folder(record.id).appendingPathComponent("recording.json")), snapshot)
        let cancelledTarget = root.appendingPathComponent("cancelled-backup")
        let cancelled = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            _ = try LibraryBackup.create(from: restored, to: cancelledTarget)
        }
        var cancellationReported = false
        do { try await cancelled.value } catch is CancellationError { cancellationReported = true }
        XCTAssertTrue(cancellationReported)
        XCTAssertTrue(!fm.fileExists(atPath: cancelledTarget.path))
        XCTAssertTrue(try fm.contentsOfDirectory(atPath: root.path).allSatisfy { !$0.hasPrefix(".laut-backup-") })
        // Missing source media leaves text accessible and never publishes an incomplete backup.
        try fm.removeItem(at: audio)
        XCTAssertTrue(rejects { _ = try library.requireAudio(record) })
        XCTAssertTrue(try LibraryStorageReport.inspect(source).issues.count == 1)
        let incomplete = root.appendingPathComponent("incomplete")
        XCTAssertTrue(rejects { _ = try LibraryBackup.create(from: source, to: incomplete) })
        XCTAssertTrue(!fm.fileExists(atPath: incomplete.path))
        XCTAssertEqual(try library.load()[0].segments, record.segments)
        // Tampered backups are refused before any destination is published.
        let backupAudio = backup.appendingPathComponent("library/\(record.id.uuidString)/Original.m4a")
        try Data("damaged".utf8).write(to: backupAudio)
        let badTarget = root.appendingPathComponent("bad-restore")
        XCTAssertTrue(rejects { try LibraryBackup.restore(backup, to: badTarget) })
        XCTAssertTrue(!fm.fileExists(atPath: badTarget.path))
        // Backups must not follow symlinks into unrelated directories.
        let linked = root.appendingPathComponent("linked")
        try fm.createDirectory(at: linked, withIntermediateDirectories: true)
        try fm.createSymbolicLink(at: linked.appendingPathComponent("outside"), withDestinationURL: source)
        XCTAssertTrue(rejects { _ = try LibraryBackup.create(from: linked, to: root.appendingPathComponent("linked-backup")) })
        XCTAssertTrue(rejects { try StorageSafety.validate(available: StorageSafety.reserve + 99, required: 100) })
        try StorageSafety.validate(available: StorageSafety.reserve + 100, required: 100)
        try StorageSafety.validate(available: nil, required: 100)
        // Persist invalid settings/vocabulary verbatim before an explicit replacement.
        for name in ["settings", "vocabulary"] {
            try Data("damaged original".utf8).write(to: source.appendingPathComponent(name + ".json"))
        }
        try library.saveSettings(AppSettings()); try library.saveVocabulary([])
        let damaged = try fm.contentsOfDirectory(at: source, includingPropertiesForKeys: nil).filter { $0.lastPathComponent.contains(".damaged-") }
        XCTAssertEqual(damaged.count, 2)
        for file in damaged { XCTAssertEqual(try Data(contentsOf: file), Data("damaged original".utf8)) }
        // Shared conference media must become an independent backup.
        let conference = root.appendingPathComponent("conference")
        let sharedLibrary = try Library(root: conference.appendingPathComponent("library"))
        try sharedLibrary.save(record)
        try Data("shared source".utf8).write(to: sharedLibrary.folder(record.id).appendingPathComponent("Original.m4a"))
        try ConferenceMedia.compact(record, folder: sharedLibrary.folder(record.id), root: conference)
        let sharedBackup = root.appendingPathComponent("shared-backup")
        _ = try LibraryBackup.create(from: sharedLibrary.root, to: sharedBackup)
        try fm.removeItem(at: conference)
        let sharedRestore = root.appendingPathComponent("shared-restored")
        try LibraryBackup.restore(sharedBackup, to: sharedRestore)
        XCTAssertEqual(try Data(contentsOf: XCTUnwrap(Library(root: sharedRestore).audioURL(record))), Data("shared source".utf8))
        // Structurally valid JSON can still contain unsafe checkpoints.
        var checkpoint = TranscriptionCheckpoint(signature: "test", duration: 600)
        checkpoint.completed = 300; checkpoint.segments = record.segments
        try checkpoint.validate()
        checkpoint.segments.append(checkpoint.segments[0])
        XCTAssertTrue(rejects { try checkpoint.validate() })
        checkpoint.segments = [Segment(start: 299, end: 301, text: "outside saved time")]
        XCTAssertTrue(rejects { try checkpoint.validate() })
        checkpoint.segments = [Segment(start: 0, end: 2, text: "bad word", words: [Word(text: "word", start: 0, end: 5)])]
        XCTAssertTrue(rejects { try checkpoint.validate() })
        checkpoint.segments = []; checkpoint.completed = .nan
        XCTAssertTrue(rejects { try checkpoint.validate() })
        print("Recovery checks passed: backup/restore, no overwrite, shared media, corruption, missing audio, low space, paused jobs, configuration preservation and checkpoint validation")
    }
}
