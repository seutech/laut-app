import FluidAudio
import Foundation
import LautCore

public actor SpeakerAnalyzer {
    public init() { ModelHub.offlineMode = true }
    public func download(to directory: URL) async throws {
        ModelHub.offlineMode = false
        defer { ModelHub.offlineMode = true }
        _ = try await OfflineDiarizerModels.load(from: directory)
    }
    public func analyze(_ file: URL, directory: URL, count: Int?, progress: @escaping @Sendable (Int, Int) -> Void) async throws -> [SpeakerTurn] {
        ModelHub.offlineMode = true
        let models = try await OfflineDiarizerModels.load(from: directory)
        var config = OfflineDiarizerConfig()
        config.clustering.numSpeakers = count
        config.postProcessing.exclusiveSegments = true
        let manager = OfflineDiarizerManager(config: config)
        manager.initialize(models: models)
        let result = try await manager.process(file, progressCallback: progress)
        try Task.checkCancellation()
        return result.segments.map { SpeakerTurn(speakerID: $0.speakerId, start: Double($0.startTimeSeconds), end: Double($0.endTimeSeconds)) }
    }
    public func analyzeIfEnabled(_ file: URL, directory: URL, enabled: Bool, progress: @escaping @Sendable (Int, Int) -> Void) async throws -> [SpeakerTurn]? {
        guard enabled else { return nil }
        let folder = directory.appendingPathComponent("speaker-diarization")
        let files = ["Segmentation.mlmodelc", "Embedding.mlmodelc", "PldaRho.mlmodelc", "FBank.mlmodelc", "plda-parameters.json"]
        guard files.allSatisfy({ FileManager.default.fileExists(atPath: folder.appendingPathComponent($0).path) }) else {
            throw LocalEngineError("Sprechermodelle fehlen. Bitte im Bereich Modelle herunterladen oder die Sprechererkennung ausschalten.")
        }
        return try await analyze(file, directory: directory, count: nil, progress: progress)
    }
}
