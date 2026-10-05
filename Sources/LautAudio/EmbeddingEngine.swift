import Foundation
import LautCore

public final class EmbeddingEngine: @unchecked Sendable {
    public let worker = WarmWorker()
    private let script: URL
    public init(script: URL) { self.script = script }
    public func embed(_ texts: [String], query: Bool = false, path: String, settings: AppSettings) async throws -> [[Float]] {
        let python = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("venv/bin/python")
        guard FileManager.default.isExecutableFile(atPath: python.path) else { throw LocalEngineError("Lokale Laufzeit fehlt.") }
        let response = try await worker.run(python: python, worker: script, request: ["modelPath": path, "texts": texts, "query": query])
        struct Response: Decodable { var vectors: [[Float]] }
        return try JSONDecoder().decode(Response.self, from: response).vectors
    }
}
