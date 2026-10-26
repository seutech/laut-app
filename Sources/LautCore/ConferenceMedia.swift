import Foundation
import CryptoKit

/// Conference originals are immutable, content-addressed files. Recordings keep their
/// native logical filenames; a small sidecar resolves them within the portable archive.
public enum ConferenceMedia {
    public struct References: Codable {
        public var version = 1
        public var root: String
        public var files: [String: String] = [:]
    }
    public static let marker = "conference-storage.json"
    public static let sidecar = "media-references.json"
    private static func safe(_ path: String) -> Bool {
        !path.isEmpty && !path.hasPrefix("/") && !path.contains("\\") && path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." }
    }
    public static func relative(_ target: URL, from folder: URL) -> String {
        let a = folder.standardizedFileURL.pathComponents, b = target.standardizedFileURL.pathComponents
        var common = 0
        while common < min(a.count, b.count) && a[common] == b[common] { common += 1 }
        return (Array(repeating: "..", count: a.count - common) + Array(b.dropFirst(common))).joined(separator: "/")
    }
    public static func url(_ logical: String, in folder: URL) throws -> URL {
        guard safe(logical) else { throw ImportJobError(L("Ungültiger Medienpfad.")) }
        let file = folder.appendingPathComponent(sidecar)
        if FileManager.default.fileExists(atPath: file.path) {
            let refs = try JSONDecoder().decode(References.self, from: Data(contentsOf: file))
            guard refs.version == 1 else { throw ImportJobError(L("Unbekannte Medienablage-Version.")) }
            if let asset = refs.files[logical] {
                let ancestors = refs.root.split(separator: "/", omittingEmptySubsequences: false)
                guard (refs.root == "." || (ancestors.count <= 8 && ancestors.allSatisfy { $0 == ".." })),
                      asset.range(of: "^assets/[a-f0-9]{64}\\.[a-z0-9]+$", options: .regularExpression) != nil else { throw ImportJobError(L("Ungültiger gemeinsamer Medienverweis.")) }
                let root = folder.appendingPathComponent(refs.root).standardizedFileURL
                let header = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent(marker))) as? [String: Any]
                guard header?["version"] as? Int == 1 else { throw ImportJobError(L("Gemeinsame Medienablage fehlt.")) }
                let resolved = root.appendingPathComponent(asset).resolvingSymlinksInPath()
                guard resolved.path.hasPrefix(root.resolvingSymlinksInPath().appendingPathComponent("assets").path + "/"),
                      FileManager.default.fileExists(atPath: resolved.path) else { throw ImportJobError(L("Gemeinsame Mediendatei fehlt: ") + asset) }
                return resolved
            }
        }
        let local = folder.appendingPathComponent(logical).standardizedFileURL
        guard local.resolvingSymlinksInPath().path.hasPrefix(folder.resolvingSymlinksInPath().path + "/"),
              FileManager.default.fileExists(atPath: local.path) else { throw ImportJobError(L("Mediendatei fehlt: ") + logical) }
        return local
    }
    public static func initialize(_ root: URL) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: root.appendingPathComponent("assets"), withIntermediateDirectories: true)
        let file = root.appendingPathComponent(marker)
        if fm.fileExists(atPath: file.path) {
            let header = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any]
            guard header?["version"] as? Int == 1 else { throw ImportJobError(L("Unbekannte Medienablage-Version.")) }
        } else {
            try Data("{\"version\":1,\"layout\":\"content-addressed-media\"}\n".utf8).write(to: file, options: .atomic)
        }
    }
    public static func digest(_ file: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: file); defer { try? handle.close() }
        var hash = SHA256()
        while let bytes = try handle.read(upToCount: 1024 * 1024), !bytes.isEmpty { hash.update(data: bytes) }
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }
    /// Commit the verified asset and reference before removing a redundant local copy.
    @discardableResult public static func attach(_ source: URL, logical: String, folder: URL, root: URL, removeLocal: Bool = false) throws -> URL {
        guard safe(logical), folder.standardizedFileURL.path.hasPrefix(root.standardizedFileURL.path + "/") else { throw ImportJobError(L("Medienziel liegt außerhalb des Archivs.")) }
        let ext = source.pathExtension.lowercased()
        guard ext.range(of: "^[a-z0-9]+$", options: .regularExpression) != nil else { throw ImportJobError(L("Ungültiges Medienformat.")) }
        try initialize(root)
        let hash = try digest(source), key = "assets/" + hash + "." + ext, target = root.appendingPathComponent(key), fm = FileManager.default
        if source.resolvingSymlinksInPath().deletingLastPathComponent().path == root.appendingPathComponent("assets").resolvingSymlinksInPath().path {
            guard source.deletingPathExtension().lastPathComponent == hash else { throw ImportJobError(L("Prüfsumme des Originalmediums stimmt nicht; Verweis bleibt erhalten.")) }
        }
        if fm.fileExists(atPath: target.path) {
            guard try digest(target) == hash else { throw ImportJobError(L("Prüfsumme der Medienablage stimmt nicht.")) }
        } else {
            let temp = root.appendingPathComponent("assets/." + UUID().uuidString)
            defer { try? fm.removeItem(at: temp) }
            try fm.copyItem(at: source, to: temp)
            guard try digest(temp) == hash else { throw ImportJobError(L("Medienkopie konnte nicht verifiziert werden.")) }
            try fm.moveItem(at: temp, to: target)
        }
        try fm.createDirectory(at: folder, withIntermediateDirectories: true)
        let file = folder.appendingPathComponent(sidecar)
        var refs = fm.fileExists(atPath: file.path) ? try JSONDecoder().decode(References.self, from: Data(contentsOf: file)) : References(root: relative(root, from: folder))
        guard refs.version == 1, refs.root == relative(root, from: folder) else { throw ImportJobError(L("Medienablage-Konflikt.")) }
        refs.files[logical] = key
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(refs).write(to: file, options: .atomic)
        let local = folder.appendingPathComponent(logical)
        if removeLocal && fm.fileExists(atPath: local.path) {
            guard local.standardizedFileURL != target.standardizedFileURL, try digest(local) == hash else { throw ImportJobError(L("Lokale Datei wurde verändert; bleibt erhalten.")) }
            try fm.removeItem(at: local)
        }
        return target
    }
    public static func compact(_ record: Recording, folder: URL, root: URL) throws {
        let names = [record.audioFilename].compactMap { $0 } + (record.conference?.documents.map(\.filename) ?? [])
        for name in names { try attach(url(name, in: folder), logical: name, folder: folder, root: root, removeLocal: true) }
    }
}
