import Foundation
import LautCore
import LautAudio

enum LinkWorkflow {
    static func run(project: URL, input: String, output: URL) async throws {
        let fm = FileManager.default
        guard !fm.fileExists(atPath: output.path) else { throw LocalEngineError("Use a new output directory") }
        let library = try Library(root: output.appendingPathComponent("library"))
        var settings = AppSettings(); settings.runtimeDirectory = project.appendingPathComponent(".runtime").path
        let link = try SourceLink.parse(input), downloads = output.appendingPathComponent("download")
        let script = project.appendingPathComponent("Resources/media_download.py")
        let downloader = MediaDownloader()
        let start = Date()
        let result = try await downloader.download(link, folder: downloads, script: script, settings: settings) { _ in }
        let elapsed = Date().timeIntervalSince(start)
        guard result.source.url == link.url.absoluteString, result.duration > 0 else { throw LocalEngineError("Missing source metadata") }
        let audio = downloads.appendingPathComponent(result.filename)
        let before = try audio.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
        let resumed = try await downloader.download(link, folder: downloads, script: script, settings: settings) { _ in }
        guard resumed.source == result.source, try audio.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate == before else { throw LocalEngineError("Completed download was not reused") }
        let importer = FileImporter(), id = UUID()
        // A crash during copying leaves an unpublished staging directory. Retry must replace only that copy.
        let partial = library.root.appendingPathComponent(".import-staging/" + id.uuidString)
        try fm.createDirectory(at: partial, withIntermediateDirectories: true)
        try Data("partial".utf8).write(to: partial.appendingPathComponent("partial"))
        let record = try await importer.importFile(audio, root: library.root, title: result.title, recordingID: id, metadata: result.source) { _ in }
        guard record.id == id, record.source == result.source, abs(record.duration - result.duration) < 0.2,
              let imported = library.audioURL(record), fm.contentsEqual(atPath: audio.path, andPath: imported.path),
              try library.load().first?.source == result.source, !(record.audioFilename ?? "").hasPrefix("source.") else { throw LocalEngineError("Library import/reopen or metadata failed") }
        var duplicateRejected = false
        do { _ = try await importer.importFile(audio, root: library.root, recordingID: id) { _ in } }
        catch { duplicateRejected = true }
        guard duplicateRejected, try library.load().count == 1, fm.contentsEqual(atPath: audio.path, andPath: imported.path) else { throw LocalEngineError("Duplicate import modified published audio") }
        _ = try await MarkdownArchive().synchronize([record], directory: output.appendingPathComponent("archive"))
        print("Link workflow passed: \(result.source.provider), \(result.duration) seconds audio, \(String(format: "%.2f", elapsed)) seconds download/check; checkpoint reuse, staging recovery, stable ID, duplicate protection, metadata/reopen/archive")
    }
}
