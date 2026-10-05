import Foundation
import LautCore

public struct TranscriptResult: Decodable {
    public var text: String
    public var duration_seconds: Double?
    public var wall_seconds: Double?
    public var load_seconds: Double?
    public var decode_seconds: Double?
    public var truncated: Bool?
    public var segments: [RawSegment]?
    public var words: [Word]?
    public struct RawSegment: Decodable { public var start: Double; public var end: Double; public var text: String }
    public func editorSegments() -> [Segment] {
        // Split into readable groups at real timestamps, so speaker changes can be corrected precisely.
        if let words, !words.isEmpty {
            var result: [Segment] = [], group: [Word] = []
            for word in words where word.start.isFinite && word.end.isFinite && word.end >= word.start {
                if let first = group.first, let last = group.last,
                   word.start - last.end > 1.2 || word.end - first.start > 18 || (group.count > 8 && ".!?".contains(last.text.last ?? " ")) {
                    result.append(Segment(start: first.start, end: last.end, text: TranscriptEditor.joinedWords(group), words: group)); group = []
                }
                group.append(word)
            }
            if let first = group.first, let last = group.last { result.append(Segment(start: first.start, end: last.end, text: TranscriptEditor.joinedWords(group), words: group)) }
            return result
        }
        let rows = (segments ?? []).map { Segment(start: $0.start, end: $0.end, text: $0.text) }
        return rows.isEmpty && !text.isEmpty ? [Segment(start: 0, end: duration_seconds ?? 0, text: text)] : rows
    }
}

public final class TranscriptionEngine: @unchecked Sendable {
    public let runner = ProcessRunner()
    public let warmWorker = WarmWorker()
    private let worker: URL
    public init(worker: URL) { self.worker = worker }
    private func runtime(_ settings: AppSettings, executable: String) throws -> URL {
        let url = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("venv/bin/" + executable)
        guard FileManager.default.isExecutableFile(atPath: url.path) else { throw LocalEngineError("Lokale Laufzeit fehlt. Führe scripts/setup-runtime.sh im Projekt aus und wähle unter Einstellungen den .runtime-Ordner.") }
        return url
    }
    private func request(_ body: [String: Any], settings: AppSettings, online: Bool = false) async throws -> Data {
        if !online { return try await warmWorker.run(python: runtime(settings, executable: "python"), worker: worker, request: body) }
        let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
        try JSONSerialization.data(withJSONObject: body).write(to: file, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        defer { try? FileManager.default.removeItem(at: file) }
        return try await runner.run(executable: runtime(settings, executable: "python"), arguments: [worker.path, file.path], networkAllowed: online)
    }
    public func transcribe(audio: URL, settings: AppSettings, vocabulary: [VocabularyEntry]) async throws -> TranscriptResult {
        guard let modelPath = settings.modelPaths[settings.engine.rawValue], FileManager.default.fileExists(atPath: modelPath) else { throw LocalEngineError("Bitte \(settings.engine.label) zuerst im Bereich Modelle installieren oder einen lokalen Modellordner auswählen.") }
        let terms = vocabulary.filter { $0.active && !$0.term.isEmpty }.prefix(25).map(\.term)
        let data = try await request(["operation": "transcribe", "engine": settings.engine.rawValue, "audio": audio.path, "modelPath": modelPath, "language": settings.language, "hotwords": terms], settings: settings)
        let result = try JSONDecoder().decode(TranscriptResult.self, from: data)
        guard result.truncated != true else { throw LocalEngineError("Das Modell meldet eine abgeschnittene Transkription. Das Ergebnis wurde nicht als vollständig gespeichert.") }
        return result
    }
    public func preload(settings: AppSettings) async throws {
        guard let path = settings.modelPaths[settings.engine.rawValue] else { throw LocalEngineError("Bitte zuerst ein Modell installieren.") }
        _ = try await request(["operation": "preload", "engine": settings.engine.rawValue, "modelPath": path], settings: settings)
    }
    public func download(_ engine: EngineKind, settings: AppSettings) async throws -> String {
        let cache = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("models")
        try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
        if engine == .phonon {
            let output = try await runner.run(executable: runtime(settings, executable: "fermion"), arguments: ["transcribe", "phonon-2", "/dev/null", "--download-only"], networkAllowed: true,
                environment: ["FERMION_CACHE_DIR": cache.path, "HF_HOME": cache.appendingPathComponent("huggingface").path])
            let path = String(data: output, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            guard FileManager.default.fileExists(atPath: path) else { throw LocalEngineError("Download lieferte keinen gültigen Modellordner.") }
            return path
        }
        return try await downloadModel(engine.modelID, settings: settings)
    }
    public func downloadModel(_ modelID: String, settings: AppSettings) async throws -> String {
        let data = try await request(["operation": "download", "modelID": modelID, "cache": settings.runtimeDirectory + "/models/huggingface"], settings: settings, online: true)
        let result = try JSONSerialization.jsonObject(with: data) as? [String: String]
        guard let path = result?["path"] else { throw LocalEngineError("Modell-Download fehlgeschlagen.") }; return path
    }
    public func refine(_ text: String, settings: AppSettings) async throws -> String {
        guard !settings.llmModelPath.isEmpty else { throw LocalEngineError("Bitte zuerst ein lokales Textmodell unter Modelle installieren.") }
        guard text.count < 24000 else { throw LocalEngineError("Bitte für die Text-KI einen kürzeren Abschnitt auswählen (max. 24.000 Zeichen). Das Original bleibt erhalten.") }
        let data = try await request(["operation": "refine", "modelPath": settings.llmModelPath, "text": text, "instructions": settings.customInstructions], settings: settings)
        let value = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let result = value?["text"] as? String else { throw LocalEngineError("Kein Text vom Modell erhalten.") }; return result
    }
}
