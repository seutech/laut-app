import Foundation
import LautCore

public struct DownloadedAudio: Codable, Sendable {
    public var filename: String
    public var fileSize: Int64
    public var title: String
    public var duration: Double
    public var source: SourceMetadata
}

public struct DownloadTools: Sendable {
    public let settings: AppSettings
    public init(settings: AppSettings) { self.settings = settings }
    public func path(_ name: String) -> String? {
        let fm = FileManager.default
        if let chosen = settings.downloadToolPaths?[name], !chosen.isEmpty {
            return fm.isExecutableFile(atPath: chosen) ? chosen : nil
        }
        let candidates = [settings.runtimeDirectory + "/tools/" + name,
                          settings.runtimeDirectory + "/download-venv/bin/" + name,
                          "/opt/homebrew/bin/" + name, "/usr/local/bin/" + name]
        return candidates.first { fm.isExecutableFile(atPath: $0) }
    }
    public var missing: [String] { ["yt-dlp", "deno", "ffmpeg", "ffprobe"].filter { path($0) == nil } }
}

public final class MediaDownloader: @unchecked Sendable {
    private let runner = ProcessRunner()
    public init() {}
    public func download(_ link: SourceLink, folder: URL, script: URL, settings: AppSettings,
                         progress: @escaping @Sendable (String) -> Void) async throws -> DownloadedAudio {
        let tools = DownloadTools(settings: settings)
        guard tools.missing.isEmpty else { throw LocalEngineError("Download-Werkzeuge fehlen: " + tools.missing.joined(separator: ", ") + ". Bitte unter Einstellungen → Linkimport einrichten.") }
        let python = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("venv/bin/python")
        guard FileManager.default.isExecutableFile(atPath: python.path) else { throw LocalEngineError("Die lokale Python-Laufzeit fehlt.") }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let progressFile = folder.appendingPathComponent("progress.json")
        let monitor = Task {
            while !Task.isCancelled {
                if let data = try? Data(contentsOf: progressFile),
                   let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let message = value["message"] as? String {
                    var text = message
                    if let bytes = value["bytes"] as? Int64 { text += " · " + ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file) }
                    if let total = value["total"] as? Int64, total > 0 { text += " / " + ByteCountFormatter.string(fromByteCount: total, countStyle: .file) }
                    progress(text)
                }
                try? await Task.sleep(for: .milliseconds(300))
            }
        }
        defer { monitor.cancel() }
        let data = try await runner.run(executable: python,
            arguments: [script.path, "--url", link.url.absoluteString, "--provider", link.provider, "--folder", folder.path,
                        "--ytdlp", tools.path("yt-dlp")!, "--deno", tools.path("deno")!,
                        "--ffmpeg", tools.path("ffmpeg")!, "--ffprobe", tools.path("ffprobe")!], networkAllowed: true)
        try Task.checkCancellation()
        let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .iso8601
        let result = try decoder.decode(DownloadedAudio.self, from: data)
        guard result.filename == (result.filename as NSString).lastPathComponent,
              result.filename == "audio.m4a", result.duration.isFinite, result.duration > 0 else { throw LocalEngineError("Ungültige Antwort des Downloadwerkzeugs.") }
        return result
    }
}
