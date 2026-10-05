import Foundation

public final class Library: @unchecked Sendable {
    public let root: URL
    private let encoder: JSONEncoder
    private let decoder = JSONDecoder()
    public private(set) var loadWarnings: [String] = []
    public init(root: URL) throws {
        self.root = root
        encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    }
    public func folder(_ id: UUID) -> URL { root.appendingPathComponent(id.uuidString, isDirectory: true) }
    public func save(_ recording: Recording) throws {
        let dir = folder(recording.id)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let target = dir.appendingPathComponent("recording.json")
        let encoded = try encoder.encode(recording)
        if FileManager.default.fileExists(atPath: target.path) {
            let previous = try Data(contentsOf: target)
            let backup = (try? decoder.decode(Recording.self, from: previous)) != nil ? "recording.previous.json" : "recording.damaged-\(UUID().uuidString).json"
            try previous.write(to: dir.appendingPathComponent(backup), options: .atomic)
        }
        try encoded.write(to: target, options: .atomic)
    }
    public func load() throws -> [Recording] {
        loadWarnings = []
        let urls = try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)
        return urls.filter { UUID(uuidString: $0.lastPathComponent) != nil }.compactMap { dir -> Recording? in
            let file = dir.appendingPathComponent("recording.json")
            let backup = dir.appendingPathComponent("recording.previous.json")
            guard FileManager.default.fileExists(atPath: file.path) || FileManager.default.fileExists(atPath: backup.path) else { return nil }
            var decoded = try? decoder.decode(Recording.self, from: Data(contentsOf: file))
            if decoded?.id.uuidString != dir.lastPathComponent { decoded = nil }
            if decoded == nil {
                decoded = try? decoder.decode(Recording.self, from: Data(contentsOf: backup))
                if decoded?.id.uuidString != dir.lastPathComponent { decoded = nil }
                loadWarnings.append((decoded == nil ? "Nicht lesbar, unverändert erhalten: " : "Aus Sicherung geladen; Originaldatei bleibt unverändert: ") + file.path)
            }
            guard var item = decoded, item.id.uuidString == dir.lastPathComponent else { return nil }
            if item.state == .processing { item.state = .failed; item.error = "Verarbeitung wurde unterbrochen. Die Originaldatei ist erhalten; du kannst erneut starten." }
            return item
        }.sorted { $0.createdAt > $1.createdAt }
    }
    public func importFile(_ source: URL) throws -> Recording {
        var recording = Recording(title: source.deletingPathExtension().lastPathComponent)
        let filename = "source." + source.pathExtension.lowercased()
        recording.audioFilename = filename
        let dir = folder(recording.id)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        do {
            try FileManager.default.copyItem(at: source, to: dir.appendingPathComponent(filename))
            try save(recording)
        } catch { try? FileManager.default.removeItem(at: dir); throw error }
        return recording
    }
    public func audioURL(_ recording: Recording) -> URL? { recording.audioFilename.map { folder(recording.id).appendingPathComponent($0) } }
    public func delete(_ recording: Recording) throws { try FileManager.default.removeItem(at: folder(recording.id)) }
    public func deleteAudio(_ recording: inout Recording) throws {
        for filename in [recording.audioFilename, recording.secondaryAudioFilename, "waveform-v1.json"].compactMap({ $0 }) {
            let url = folder(recording.id).appendingPathComponent(filename)
            if FileManager.default.fileExists(atPath: url.path) { try FileManager.default.removeItem(at: url) }
        }
        recording.audioFilename = nil; recording.secondaryAudioFilename = nil; try save(recording)
    }
    public func loadSettings() throws -> AppSettings {
        let path = root.appendingPathComponent("settings.json")
        guard FileManager.default.fileExists(atPath: path.path) else { return AppSettings() }
        return try decoder.decode(AppSettings.self, from: Data(contentsOf: path))
    }
    public func saveSettings(_ settings: AppSettings) throws { try encoder.encode(settings).write(to: root.appendingPathComponent("settings.json"), options: .atomic) }
    public func loadVocabulary() throws -> [VocabularyEntry] {
        let path = root.appendingPathComponent("vocabulary.json")
        guard FileManager.default.fileExists(atPath: path.path) else { return [] }
        return try decoder.decode([VocabularyEntry].self, from: Data(contentsOf: path))
    }
    public func saveVocabulary(_ entries: [VocabularyEntry]) throws { try encoder.encode(entries).write(to: root.appendingPathComponent("vocabulary.json"), options: .atomic) }
}
