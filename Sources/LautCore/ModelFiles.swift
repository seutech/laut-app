import Foundation

public enum ModelFiles {
    /// Only app-managed caches can be removed. Imported directories are detached, never deleted.
    public static func removalTarget(path: String, runtime: String, modelID: String) -> URL? {
        guard !path.isEmpty, !runtime.isEmpty else { return nil }
        let base = URL(fileURLWithPath: runtime).appendingPathComponent("models").standardizedFileURL
        let supplied = URL(fileURLWithPath: path).standardizedFileURL
        let target: URL
        if modelID == "phonon-2" {
            target = base.appendingPathComponent("speech/FermionResearch__Phonon-2/model_phonon2_c4c_int6")
            guard supplied.path == target.path else { return nil }
        } else if modelID == "speakers" {
            target = base.appendingPathComponent("diarization")
            guard supplied.path == target.path else { return nil }
        } else {
            let pieces = modelID.split(separator: "/", omittingEmptySubsequences: false)
            let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_."))
            guard pieces.count == 2, pieces.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." && $0.unicodeScalars.allSatisfy(allowed.contains) }) else { return nil }
            target = base.appendingPathComponent("huggingface/models--" + modelID.replacingOccurrences(of: "/", with: "--"))
            guard supplied.path.hasPrefix(target.appendingPathComponent("snapshots").path + "/") else { return nil }
        }
        // Reject linked cache roots/parents that would escape the runtime's models directory.
        guard target.resolvingSymlinksInPath().path.hasPrefix(base.resolvingSymlinksInPath().path + "/"),
              supplied.resolvingSymlinksInPath().path.hasPrefix(target.resolvingSymlinksInPath().path + "/") || supplied.resolvingSymlinksInPath() == target.resolvingSymlinksInPath() else { return nil }
        return target
    }
}
