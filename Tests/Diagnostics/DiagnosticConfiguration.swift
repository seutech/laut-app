import Foundation
import LautCore
import LautAudio

enum DiagnosticConfiguration {
    static func value(_ flag: String, in arguments: [String]) throws -> String? {
        guard let index = arguments.firstIndex(of: flag) else { return nil }
        guard index + 1 < arguments.count, !arguments[index + 1].hasPrefix("--") else { throw LocalEngineError("Missing value for \(flag)") }
        return arguments[index + 1]
    }
    static func settings(project: URL, arguments: [String]) throws -> AppSettings {
        let raw = try value("--engine", in: arguments) ?? "phonon"
        guard let kind = EngineKind(rawValue: raw) else { throw LocalEngineError("Unknown engine: \(raw). Choose phonon, parakeet or qwen.") }
        var settings = AppSettings(); settings.engine = kind
        settings.runtimeDirectory = project.appendingPathComponent(".runtime").path
        let path: String
        if let supplied = try value("--model", in: arguments) { path = supplied }
        else if kind == .phonon { path = settings.runtimeDirectory + "/models/speech/FermionResearch__Phonon-2/model_phonon2_c4c_int6" }
        else {
            let cache = URL(fileURLWithPath: settings.runtimeDirectory).appendingPathComponent("models/huggingface/models--" + kind.modelID.replacingOccurrences(of: "/", with: "--"))
            let revision = try String(contentsOf: cache.appendingPathComponent("refs/main"), encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
            guard revision.range(of: "^[a-f0-9]{40}$", options: .regularExpression) != nil else { throw LocalEngineError("Invalid local model revision") }
            path = cache.appendingPathComponent("snapshots/" + revision).path
        }
        guard FileManager.default.fileExists(atPath: path) else { throw LocalEngineError("Model not installed: \(kind.label)") }
        settings.modelPaths[kind.rawValue] = path
        return settings
    }
}
