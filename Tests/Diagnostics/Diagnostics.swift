import Foundation
import LautCore
import LautAudio

@main struct Diagnostics {
    static func main() async throws {
        let args = CommandLine.arguments
        let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        if args.contains("--search-checks") {
            guard let path = try DiagnosticConfiguration.value("--model", in: args) else { throw LocalEngineError("Provide --model with a local E5 small directory") }
            try await SearchWorkflow.run(project: root, model: path); return
        }
        if args.contains("--audio-interface-checks") { try await AudioInterfaceChecks.run(); return }
        let analyzer = SpeakerAnalyzer()
        let models = root.appendingPathComponent(".runtime/models/diarization")
        if args.contains("--download-speakers") { try await analyzer.download(to: models); print("Speaker models ready"); return }
        guard args.count >= 2 else { print("Usage: swift run LautDiagnostics <audio> [--workflow] [--engine phonon|parakeet|qwen] [--output directory] [--preload] [--print-transcript] OR --download-speakers"); return }
        if args.contains("--workflow") {
            let settings = try DiagnosticConfiguration.settings(project: root, arguments: args)
            let output = try DiagnosticConfiguration.value("--output", in: args).map { URL(fileURLWithPath: $0) }
            try await FileWorkflow.check(source: URL(fileURLWithPath: args[1]), project: root, settings: settings, output: output)
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
            let settings = try DiagnosticConfiguration.settings(project: root, arguments: args)
            let engine = TranscriptionEngine(worker: root.appendingPathComponent("Resources/mlx_worker.py"))
            defer { engine.warmWorker.stop() }
            if args.contains("--preload") {
                let began = Date()
                try await engine.preload(settings: settings)
                print("Startup preparation: \(Date().timeIntervalSince(began)) seconds")
                try await engine.preload(settings: settings)
            }
            for run in 1...2 {
                let began = Date()
                let result = try await engine.transcribe(audio: temporary, settings: settings, vocabulary: [])
                if args.contains("--print-transcript") { print(result.text) }
                print("Run \(run), end-to-end ASR seconds: \(Date().timeIntervalSince(began)); editor segments: \(result.editorSegments().count)")
                print("Load: \(result.load_seconds ?? -1); transcription: \(result.decode_seconds ?? -1)")
                if args.contains("--preload") { guard result.load_seconds == 0 else { throw LocalEngineError("Prepared model was reloaded during transcription") } }
                guard !result.text.isEmpty else { throw LocalEngineError("Empty transcription") }
            }
            engine.warmWorker.stop()
        }
    }
}
