import Foundation

public struct SourceMetadata: Codable, Equatable, Sendable {
    public var url: String
    public var provider: String
    public var externalID: String?
    public var originalTitle: String?
    public var publisher: String?
    /// Calendar date supplied by the publisher; deliberately not the import date.
    public var publishedOn: String?
    public var language: String?
    public var importedAt: Date
    public var eventDate: String?
    public var reportingPeriod: String?
    public init(url: String, provider: String, importedAt: Date = Date()) {
        self.url = url; self.provider = provider; self.importedAt = importedAt
    }
}

public struct SourceLink: Equatable, Sendable {
    public let url: URL
    public let key: String
    public let provider: String
    public static func parse(_ input: String) throws -> SourceLink {
        guard var parts = URLComponents(string: input.trimmingCharacters(in: .whitespacesAndNewlines)),
              parts.scheme?.lowercased() == "https", let host = parts.host?.lowercased(),
              !host.isEmpty, parts.user == nil, parts.password == nil,
              parts.port == nil || parts.port == 443 else { throw ImportJobError("Bitte einen HTTPS-Link ohne Zugangsdaten eingeben.") }
        let youtube = ["youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com", "youtu.be", "www.youtu.be"]
        if youtube.contains(host) {
            let path = parts.path.split(separator: "/").map(String.init)
            let id: String?
            if host.hasSuffix("youtu.be") { id = path.first }
            else if path.first == "watch" { id = parts.queryItems?.first { $0.name == "v" }?.value }
            else if let first = path.first, ["shorts", "embed", "live"].contains(first), path.count == 2 { id = path[1] }
            else { id = nil }
            guard let id, id.range(of: "^[A-Za-z0-9_-]{11}$", options: .regularExpression) != nil else {
                throw ImportJobError("Bitte den Link eines einzelnen YouTube-Videos verwenden. Playlists und Kanäle werden noch nicht importiert.")
            }
            return SourceLink(url: URL(string: "https://www.youtube.com/watch?v=" + id)!, key: "youtube:" + id, provider: "YouTube")
        }
        guard host != "localhost", !host.hasSuffix(".localhost"), !host.hasSuffix(".local"),
              !host.contains(":"), !host.allSatisfy({ $0.isNumber || $0 == "." }),
              ["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", "aif"].contains((parts.path as NSString).pathExtension.lowercased()) else {
            throw ImportJobError("Unterstützt werden einzelne YouTube-Videos und direkte Audio-Dateilinks (z. B. MP3 oder M4A).")
        }
        parts.fragment = nil
        guard let url = parts.url else { throw ImportJobError("Ungültiger Link.") }
        return SourceLink(url: url, key: url.absoluteString, provider: "Audio-Link")
    }
}

public struct ImportJobError: LocalizedError {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

public enum ImportStage: String, Codable, Sendable {
    case queued, downloading, importing, transcribing, paused, complete, failed
    public var label: String {
        switch self {
        case .queued: return "Wartet"
        case .downloading: return "Audio laden"
        case .importing: return "Datei übernehmen"
        case .transcribing: return "Lokal transkribieren"
        case .paused: return "Pausiert"
        case .complete: return "Fertig"
        case .failed: return "Fehlgeschlagen"
        }
    }
    public var unfinished: Bool { self != .complete && self != .failed && self != .paused }
}

public struct ImportJob: Codable, Identifiable, Sendable {
    public enum Kind: String, Codable, Sendable { case link, file, recording }
    public var id: UUID = UUID()
    /// Chosen before import, so recovery cannot create a second library entry.
    public var recordingID: UUID = UUID()
    public var kind: Kind
    public var input: String
    public var sourceKey: String
    public var title: String
    public var createdAt = Date()
    public var stage: ImportStage = .queued
    public var autoTranscribe: Bool
    public var message: String?
    public init(kind: Kind, input: String, sourceKey: String, title: String, autoTranscribe: Bool = true) {
        self.kind = kind; self.input = input; self.sourceKey = sourceKey; self.title = title; self.autoTranscribe = autoTranscribe
    }
    public mutating func recover() {
        if stage.unfinished { stage = .paused; message = "Nach Neustart pausiert. Mit Fortsetzen geht es beim letzten gespeicherten Schritt weiter." }
    }
}

/// Main-actor caller serializes mutations; a damaged ledger is never silently replaced.
public struct ImportJobStore {
    public let root: URL
    public init(root: URL) { self.root = root }
    public func folder(_ id: UUID) -> URL { root.appendingPathComponent("import-jobs/" + id.uuidString) }
    public func load() throws -> [ImportJob] {
        let path = root.appendingPathComponent("import-jobs.json")
        guard FileManager.default.fileExists(atPath: path.path) else { return [] }
        var jobs = try JSONDecoder().decode([ImportJob].self, from: Data(contentsOf: path))
        guard Set(jobs.map(\.id)).count == jobs.count else { throw ImportJobError("Auftragsdatei enthält doppelte Kennungen.") }
        for i in jobs.indices { jobs[i].recover() }
        return jobs
    }
    public func save(_ jobs: [ImportJob]) throws {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(jobs).write(to: root.appendingPathComponent("import-jobs.json"), options: .atomic)
    }
}
