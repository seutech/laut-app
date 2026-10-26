import Foundation

public enum StorageSafety {
    public static let reserve: Int64 = 512 * 1024 * 1024
    public static func available(at url: URL) throws -> Int64? {
        var directory = url
        while !FileManager.default.fileExists(atPath: directory.path), directory.path != "/" {
            directory.deleteLastPathComponent()
        }
        // The purgeable-capacity service can return zero in restricted processes.
        // Actual filesystem free bytes are a conservative fallback, without assuming
        // that macOS will reclaim purgeable storage in time.
        let attributes = try FileManager.default.attributesOfFileSystem(forPath: directory.path)
        if let bytes = attributes[.systemFreeSize] as? NSNumber { return bytes.int64Value }
        return try directory.resourceValues(forKeys: [.volumeAvailableCapacityKey]).volumeAvailableCapacity.map(Int64.init)
    }
    public static func validate(available: Int64?, required: Int64) throws {
        guard required >= 0, required <= Int64.max - reserve else { throw ImportJobError(L("Ungültiger Speicherbedarf.")) }
        if let available, available < required + reserve {
            throw ImportJobError(L("Nicht genügend freier Speicher. Benötigt: ") + format(required + reserve) + L(" einschließlich 512 MB Reserve; verfügbar: ") + format(available) + L(". Bitte Platz freigeben und erneut starten. Gespeicherte Originale und Zwischenstände bleiben erhalten."))
        }
    }
    public static func require(_ bytes: Int64, at url: URL) throws {
        try validate(available: available(at: url), required: bytes)
    }
    public static func format(_ bytes: Int64) -> String { ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file) }
}

public struct LibraryStorageReport: Sendable {
    public var audio: Int64 = 0
    public var documents: Int64 = 0
    public var other: Int64 = 0
    public var available: Int64?
    public var issues: [String] = []
    public var total: Int64 { audio + documents + other }
    public static func inspect(_ root: URL) throws -> Self {
        var report = Self()
        report.available = try StorageSafety.available(at: root)
        for file in try LibraryBackup.files(in: root) {
            let size = Int64(try file.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0)
            switch file.pathExtension.lowercased() {
            case "m4a", "wav", "mp3", "flac", "aac", "opus", "mp4", "mov": report.audio += size
            case "pdf": report.documents += size
            default: report.other += size
            }
        }
        let library = try Library(root: root)
        let records = try library.load()
        report.issues = library.loadWarnings
        for record in records {
            let names = [record.audioFilename, record.secondaryAudioFilename].compactMap { $0 } + (record.conference?.documents.map(\.filename) ?? [])
            for name in names {
                do { _ = try ConferenceMedia.url(name, in: library.folder(record.id)) }
                catch { report.issues.append(record.title + ": " + error.localizedDescription) }
            }
        }
        return report
    }
}
