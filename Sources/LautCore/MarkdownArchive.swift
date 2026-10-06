import Foundation
import Darwin
import CryptoKit

public struct MarkdownArchiveResult: Sendable {
    public var written = 0
    public var conflicts = 0
}

/// A one-way, editable Markdown archive. Only unchanged files previously written by Laut are replaced.
public actor MarkdownArchive {
    private struct Entry: Codable { var filename: String; var digest: String; var baseFilename: String? }
    private struct Manifest: Codable { var entries: [String: Entry] = [:] }
    public init() {}
    private func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
    public static func filename(_ record: Recording, timeZone: TimeZone = .current) -> String {
        let formatter = DateFormatter(); formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX"); formatter.timeZone = timeZone; formatter.dateFormat = "yyyy-MM-dd"
        let invalid = CharacterSet.controlCharacters.union(CharacterSet(charactersIn: "/:\\"))
        let cleaned = record.title.components(separatedBy: invalid).joined(separator: " ").split(whereSeparator: \.isWhitespace).joined(separator: " ")
        var title = String((cleaned.isEmpty ? "Transkript" : cleaned).prefix(100)).trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        while title.utf8.count > 180 { title.removeLast() }
        return formatter.string(from: record.createdAt) + " " + (title.isEmpty ? "Transkript" : title) + ".md"
    }
    public static func markdown(_ record: Recording) throws -> String {
        func quoted(_ value: String) throws -> String { String(decoding: try JSONEncoder().encode(value), as: UTF8.self) }
        var output = "---\nlaut_id: \(try quoted(record.id.uuidString))\ntitle: \(try quoted(record.title))\ncreated: \(try quoted(ISO8601DateFormatter().string(from: record.createdAt)))\nduration_seconds: \(record.duration)\nengine: \(try quoted(record.engine))\n---\n\n"
        output += "# " + record.title.replacingOccurrences(of: "\n", with: " ") + "\n\n"
        if !record.refinedText.isEmpty { output += "## Auswertung\n\n" + record.refinedText + "\n\n" }
        if !record.notes.isEmpty { output += "## Notizen\n\n" + record.notes + "\n\n" }
        if !record.segments.isEmpty {
            output += "## Transkript\n\n"
            for paragraph in TranscriptLayout.paragraphs(record.segments) {
                let speaker = record.speakers.isEmpty ? "" : record.speakerName(paragraph.speakerID) + " · "
                output += "**\(speaker)\(Exporter.timestamp(paragraph.start))**\n\n\(paragraph.text)\n\n"
            }
        }
        return output
    }
    public func synchronize(_ recordings: [Recording], directory: URL) throws -> MarkdownArchiveResult {
        try Task.checkCancellation()
        let fm = FileManager.default
        try fm.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let manifestURL = directory.appendingPathComponent(".laut-archive.json")
        var manifest = Manifest()
        if fm.fileExists(atPath: manifestURL.path) {
            // Refuse damaged bookkeeping instead of claiming ownership of unrelated files.
            manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: manifestURL))
        }
        var report = MarkdownArchiveResult()
        for record in recordings where !record.segments.isEmpty || !record.notes.isEmpty || !record.refinedText.isEmpty {
            try Task.checkCancellation()
            let body = Data(try Self.markdown(record).utf8), hash = digest(body), key = record.id.uuidString
            let wanted = Self.filename(record)
            let entry = manifest.entries[key]
            let safeEntry = entry.flatMap { e -> Entry? in
                guard e.filename == (e.filename as NSString).lastPathComponent, e.filename.hasSuffix(".md"), !e.filename.contains("\\") else { return nil }; return e
            }
            let previous = safeEntry.map { directory.appendingPathComponent($0.filename) }
            let exists = previous.map { fm.fileExists(atPath: $0.path) } ?? false
            let owned = try previous.map { file -> Bool in
                let values = try? file.resourceValues(forKeys: [.isSymbolicLinkKey, .isRegularFileKey])
                guard values?.isSymbolicLink != true, values?.isRegularFile == true, let safeEntry else { return false }
                return digest(try Data(contentsOf: file)) == safeEntry.digest
            } ?? false
            let stem = (wanted as NSString).deletingPathExtension
            let sameTitle = safeEntry.map { ($0.baseFilename ?? $0.filename) == wanted } ?? false
            if owned, sameTitle, safeEntry?.digest == hash { continue }
            var target = directory.appendingPathComponent(wanted)
            if owned && sameTitle, let previous { target = previous }
            else {
                var suffix = 2
                while fm.fileExists(atPath: target.path) || (try? target.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) == true {
                    target = directory.appendingPathComponent(stem + " (\(suffix)).md"); suffix += 1
                }
            }
            if exists && !owned { report.conflicts += 1 }
            if target == previous && owned {
                try body.write(to: target, options: .atomic)
            } else {
                // O_EXCL protects an unrelated file created after the filename check.
                let descriptor = Darwin.open(target.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
                guard descriptor >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
                let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
                defer { try? handle.close() }
                try handle.write(contentsOf: body); try handle.synchronize()
            }
            if owned, let previous, previous != target { try fm.removeItem(at: previous) }
            manifest.entries[key] = Entry(filename: target.lastPathComponent, digest: hash, baseFilename: wanted)
            try JSONEncoder().encode(manifest).write(to: manifestURL, options: .atomic)
            report.written += 1
        }
        // Archive copies deliberately survive deletion from the app library.
        return report
    }
}
