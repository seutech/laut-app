import Foundation

/// Verified directory snapshots. Never merge into or overwrite a live library.
public enum LibraryBackup {
    public struct Entry: Codable, Sendable {
        public var path: String
        public var bytes: Int64
        public var sha256: String
    }
    public struct Manifest: Codable, Sendable {
        public var version = 1
        public var created = Date()
        public var sourceRoot: String
        public var files: [Entry]
    }
    static let manifestName = "laut-backup.json"
    static func files(in root: URL) throws -> [URL] {
        let fm = FileManager.default
        guard try root.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink != true else { throw ImportJobError(L("Verknüpfter Bibliotheksordner in der Sicherung ist nicht erlaubt.")) }
        var failure: Error?
        guard let walker = fm.enumerator(at: root, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey], errorHandler: { _, error in failure = error; return false }) else {
            throw ImportJobError(L("Ordner kann nicht gelesen werden: ") + root.path)
        }
        var result: [URL] = []
        for case let file as URL in walker {
            let values = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard values.isSymbolicLink != true else { throw ImportJobError(L("Verknüpfung bitte separat prüfen: ") + file.path) }
            if values.isRegularFile == true { result.append(file) }
        }
        if let failure { throw failure }
        return result.sorted { $0.path < $1.path }
    }
    private static func relative(_ file: URL, root: URL) -> String { ConferenceMedia.relative(file, from: root) }
    private static func safePath(_ path: String) -> Bool {
        !path.isEmpty && !path.hasPrefix("/") && !path.contains("\\") &&
        path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." }
    }
    private static func retained(_ path: String) -> Bool {
        let name = (path as NSString).lastPathComponent
        return !path.hasPrefix(".import-staging/") && name != ".DS_Store" &&
            !["transcription-chunk.wav", "speakers-temporary.wav", "processing.wav", "waveform-v1.json"].contains(name) &&
            !((path.hasPrefix("search-") || path.hasPrefix("search.sqlite")) && path.contains(".sqlite") && !path.contains("/")) &&
            name != ConferenceMedia.sidecar
    }
    private static func checkDestination(_ source: URL, _ destination: URL) throws {
        let a = source.resolvingSymlinksInPath().standardizedFileURL.path
        let b = destination.resolvingSymlinksInPath().standardizedFileURL.path
        guard a != b, !b.hasPrefix(a + "/"), !a.hasPrefix(b + "/"),
              !FileManager.default.fileExists(atPath: destination.path) else {
            throw ImportJobError(L("Bitte einen neuen Zielordner außerhalb der Quellbibliothek wählen. Bestehende Ordner werden nicht überschrieben."))
        }
    }
    /// Copy all owned files and materialize shared conference references. Search indexes
    /// and temporary decoder files are rebuildable and intentionally excluded.
    public static func create(from source: URL, to destination: URL) throws -> Manifest {
        try checkDestination(source, destination)
        let source = source.resolvingSymlinksInPath().standardizedFileURL
        var inputs: [String: URL] = [:]
        var referenceHashes: [URL: String] = [:]
        let originalFiles = try files(in: source)
        for file in originalFiles {
            let path = relative(file, root: source)
            if retained(path) { inputs[path] = file }
            if file.lastPathComponent == ConferenceMedia.sidecar {
                referenceHashes[file] = try ConferenceMedia.digest(file)
                let refs = try JSONDecoder().decode(ConferenceMedia.References.self, from: Data(contentsOf: file))
                for logical in refs.files.keys {
                    let actual = try ConferenceMedia.url(logical, in: file.deletingLastPathComponent())
                    let directory = (path as NSString).deletingLastPathComponent
                    inputs[directory.isEmpty ? logical : directory + "/" + logical] = actual
                }
            }
        }
        let library = try Library(root: source)
        for record in try library.load() {
            for name in [record.audioFilename, record.secondaryAudioFilename].compactMap({ $0 }) + (record.conference?.documents.map(\.filename) ?? []) {
                _ = try ConferenceMedia.url(name, in: library.folder(record.id))
            }
        }
        let bytes = try inputs.values.reduce(Int64(0)) { try $0 + Int64($1.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0) }
        try StorageSafety.require(bytes, at: destination.deletingLastPathComponent())
        let staging = destination.deletingLastPathComponent().appendingPathComponent(".laut-backup-" + UUID().uuidString)
        let fm = FileManager.default
        try fm.createDirectory(at: staging, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try fm.createDirectory(at: staging.appendingPathComponent("library"), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        defer { try? fm.removeItem(at: staging) }
        var entries: [Entry] = []
        for path in inputs.keys.sorted() {
            try Task.checkCancellation()
            guard safePath(path), let input = inputs[path] else { throw ImportJobError(L("Ungültiger Sicherungspfad: ") + path) }
            let before = try ConferenceMedia.digest(input)
            if input.deletingLastPathComponent().lastPathComponent == "assets",
               input.deletingPathExtension().lastPathComponent.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil,
               input.deletingPathExtension().lastPathComponent != before {
                throw ImportJobError(L("Originalmedium mit abweichender Prüfsumme: ") + path)
            }
            let target = staging.appendingPathComponent("library/" + path)
            try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            try fm.copyItem(at: input, to: target)
            guard try ConferenceMedia.digest(target) == before else { throw ImportJobError(L("Datei während der Sicherung verändert: ") + path) }
            entries.append(Entry(path: path, bytes: Int64(try target.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0), sha256: before))
        }
        // Detect edits made while copying, including newly created or deleted files.
        func sourceInventory(_ urls: [URL]) -> [String] {
            urls.filter { retained(relative($0, root: source)) || $0.lastPathComponent == ConferenceMedia.sidecar }.map(\.path)
        }
        guard try sourceInventory(files(in: source)) == sourceInventory(originalFiles) else { throw ImportJobError(L("Bibliothek wurde während der Sicherung verändert. Bitte erneut versuchen.")) }
        for entry in entries {
            guard try ConferenceMedia.digest(inputs[entry.path]!) == entry.sha256 else { throw ImportJobError(L("Bibliothek wurde während der Sicherung verändert: ") + entry.path) }
        }
        for (file, hash) in referenceHashes {
            guard try ConferenceMedia.digest(file) == hash else { throw ImportJobError(L("Medienverweise wurden während der Sicherung verändert.")) }
        }
        let manifest = Manifest(sourceRoot: source.path, files: entries)
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(manifest).write(to: staging.appendingPathComponent(manifestName), options: .atomic)
        _ = try verify(staging)
        try Task.checkCancellation()
        try fm.moveItem(at: staging, to: destination)
        return manifest
    }
    @discardableResult public static func verify(_ backup: URL) throws -> Manifest {
        let backup = backup.resolvingSymlinksInPath().standardizedFileURL
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: backup.appendingPathComponent(manifestName)))
        guard manifest.version == 1, Set(manifest.files.map { $0.path.precomposedStringWithCanonicalMapping }).count == manifest.files.count else { throw ImportJobError(L("Unbekanntes oder beschädigtes Sicherungsformat.")) }
        let root = backup.appendingPathComponent("library")
        let present = try files(in: root)
        guard Set(present.map { relative($0, root: root).precomposedStringWithCanonicalMapping }) == Set(manifest.files.map { $0.path.precomposedStringWithCanonicalMapping }) else {
            throw ImportJobError(L("Sicherung enthält fehlende oder zusätzliche Dateien."))
        }
        for entry in manifest.files {
            try Task.checkCancellation()
            guard safePath(entry.path), entry.bytes >= 0 else { throw ImportJobError(L("Ungültiger Sicherungspfad.")) }
            let file = root.appendingPathComponent(entry.path)
            guard try Int64(file.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? -1) == entry.bytes,
                  try ConferenceMedia.digest(file) == entry.sha256 else { throw ImportJobError(L("Sicherungsdatei beschädigt: ") + entry.path) }
        }
        return manifest
    }
    /// Restore as a separate, verified library; the current library remains untouched.
    public static func restore(_ backup: URL, to destination: URL) throws {
        try checkDestination(backup, destination)
        let manifest = try verify(backup)
        try StorageSafety.require(manifest.files.reduce(0) { $0 + $1.bytes }, at: destination.deletingLastPathComponent())
        let fm = FileManager.default
        let staging = destination.deletingLastPathComponent().appendingPathComponent(".laut-restore-" + UUID().uuidString)
        defer { try? fm.removeItem(at: staging) }
        try fm.copyItem(at: backup.appendingPathComponent("library"), to: staging)
        guard Set(try files(in: staging).map { relative($0, root: staging).precomposedStringWithCanonicalMapping }) == Set(manifest.files.map { $0.path.precomposedStringWithCanonicalMapping }) else {
            throw ImportJobError(L("Die Sicherung wurde während der Wiederherstellung verändert."))
        }
        for entry in manifest.files {
            guard try ConferenceMedia.digest(staging.appendingPathComponent(entry.path)) == entry.sha256 else { throw ImportJobError(L("Wiederhergestellte Datei konnte nicht verifiziert werden: ") + entry.path) }
        }
        // Owned attachments must follow the restored library. External waiting inputs
        // remain external and are deliberately not represented as backed-up originals.
        let jobFile = staging.appendingPathComponent("import-jobs.json")
        if fm.fileExists(atPath: jobFile.path), var jobs = try? ImportJobStore(root: staging).load() {
            func relocate(_ path: String) -> String {
                let canonical = URL(fileURLWithPath: path).resolvingSymlinksInPath().path
                return canonical.hasPrefix(manifest.sourceRoot + "/") ? destination.path + String(canonical.dropFirst(manifest.sourceRoot.count)) : path
            }
            for i in jobs.indices {
                jobs[i].documentPaths = jobs[i].documentPaths?.map(relocate)
                if jobs[i].kind == .file { jobs[i].input = relocate(jobs[i].input); jobs[i].sourceKey = jobs[i].input }
            }
            try ImportJobStore(root: staging).save(jobs)
        }
        let restored = try Library(root: staging)
        if let settings = try? restored.loadSettings() {
            var isolated = settings
            isolated.markdownDirectory = destination.appendingPathComponent("markdown-archive").path
            try restored.saveSettings(isolated)
        }
        for record in try restored.load() {
            try ConferenceArchive.writeDocuments(record, folder: restored.folder(record.id))
        }
        try Task.checkCancellation()
        try fm.moveItem(at: staging, to: destination)
    }
}
