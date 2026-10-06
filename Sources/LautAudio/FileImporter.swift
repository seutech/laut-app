import Foundation
import AVFoundation
import LautCore

public actor FileImporter {
    public init() {}
    public func importFile(_ source: URL, root: URL, title: String? = nil, recordingID: UUID? = nil, metadata: SourceMetadata? = nil, progress: @escaping @Sendable (Double) -> Void) async throws -> Recording {
        guard source.isFileURL else { throw LocalEngineError("Bitte eine lokale Audio- oder Videodatei auswählen.") }
        let access = source.startAccessingSecurityScopedResource()
        defer { if access { source.stopAccessingSecurityScopedResource() } }
        let values = try source.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard values.isRegularFile == true, let size = values.fileSize, size > 0 else { throw LocalEngineError("Die Datei ist leer oder keine reguläre Datei.") }
        let asset = AVURLAsset(url: source)
        guard try await !asset.loadTracks(withMediaType: .audio).isEmpty else { throw LocalEngineError("Die Datei enthält keine unterstützte Audiospur.") }
        let duration = try await asset.load(.duration).seconds
        try Task.checkCancellation()
        let library = try Library(root: root.appendingPathComponent(".import-staging"))
        var recording = Recording(title: title ?? source.deletingPathExtension().lastPathComponent)
        if let recordingID { recording.id = recordingID }
        recording.source = metadata
        recording.audioFilename = (MarkdownArchive.filename(recording) as NSString).deletingPathExtension + "." + source.pathExtension.lowercased()
        recording.duration = duration.isFinite ? max(0, duration) : 0
        let folder = library.folder(recording.id)
        let destination = root.appendingPathComponent(recording.id.uuidString)
        guard !FileManager.default.fileExists(atPath: destination.path) else { throw LocalEngineError("Importordner existiert bereits. Bitte den gespeicherten Auftrag prüfen.") }
        // Only this job's incomplete staging copy is disposable. Published library entries are never replaced.
        if FileManager.default.fileExists(atPath: folder.path) { try FileManager.default.removeItem(at: folder) }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        do {
            let target = folder.appendingPathComponent(recording.audioFilename!)
            guard FileManager.default.createFile(atPath: target.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw LocalEngineError("Zieldatei konnte nicht angelegt werden.") }
            let input = try FileHandle(forReadingFrom: source), output = try FileHandle(forWritingTo: target)
            defer { try? input.close(); try? output.close() }
            var copied = 0, lastProgress = Date.distantPast
            while true {
                try Task.checkCancellation()
                guard let data = try input.read(upToCount: 1024 * 1024), !data.isEmpty else { break }
                try output.write(contentsOf: data); copied += data.count
                if Date().timeIntervalSince(lastProgress) > 0.15 { progress(min(1, Double(copied) / Double(size))); lastProgress = Date() }
            }
            guard copied == size else { throw LocalEngineError("Die Datei hat sich während des Kopierens verändert. Bitte erneut importieren.") }
            try output.synchronize(); try Task.checkCancellation()
            try library.save(recording)
            try FileManager.default.moveItem(at: folder, to: destination); progress(1)
            return recording
        } catch { try? FileManager.default.removeItem(at: folder); throw error }
    }
}
