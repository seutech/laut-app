import Foundation
import LautCore
import LautAudio

@main struct Diagnostics {
    static func main() async throws {
        let args = CommandLine.arguments
        let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        let analyzer = SpeakerAnalyzer()
        let models = root.appendingPathComponent(".runtime/models/diarization")
        if args.contains("--download-speakers") { try await analyzer.download(to: models); print("Speaker models ready"); return }
        guard args.count >= 2 else { print("Usage: swift run LautDiagnostics <audio> [--diarize] OR --download-speakers"); return }
        if args.contains("--workflow") {
            try await FileWorkflow.check(source: URL(fileURLWithPath: args[1]), project: root)
            return
        }
        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("laut-diagnostic-\(UUID().uuidString).wav")
        defer { try? FileManager.default.removeItem(at: temporary) }
        let duration = try await AudioFiles.convert(URL(fileURLWithPath: args[1]), to: temporary)
        print("Decoded \(duration) seconds")
        if args.contains("--diarize") {
            let turns = try await analyzer.analyze(temporary, directory: models, count: nil) { current, total in print("Speaker windows: \(current)/\(total)") }
            print("Speaker IDs: \(Set(turns.map(\.speakerID)).count), segments: \(turns.count)")
        } else {
            var settings = AppSettings()
            settings.runtimeDirectory = root.appendingPathComponent(".runtime").path
            settings.modelPaths["phonon"] = settings.runtimeDirectory + "/models/speech/FermionResearch__Phonon-2/model_phonon2_c4c_int6"
            let engine = TranscriptionEngine(worker: root.appendingPathComponent("Resources/mlx_worker.py"))
            if args.contains("--preload") {
                let began = Date()
                try await engine.preload(settings: settings)
                print("Startup preparation: \(Date().timeIntervalSince(began)) seconds")
                try await engine.preload(settings: settings)
            }
            for run in 1...2 {
                let began = Date()
                let result = try await engine.transcribe(audio: temporary, settings: settings, vocabulary: [])
                print(result.text)
                print("Run \(run), end-to-end ASR seconds: \(Date().timeIntervalSince(began)); editor segments: \(result.editorSegments().count)")
                print("Load: \(result.load_seconds ?? -1); transcription: \(result.decode_seconds ?? -1)")
                if args.contains("--preload") { guard result.load_seconds == 0 else { throw LocalEngineError("Prepared model was reloaded during transcription") } }
                guard !result.text.isEmpty else { throw LocalEngineError("Empty transcription") }
            }
            engine.warmWorker.stop()
        }
    }
}
