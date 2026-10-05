import Foundation

public struct LocalEngineError: LocalizedError {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

/// No shell interpolation. Both streams go to files to avoid pipe-buffer deadlocks.
public final class ProcessRunner: @unchecked Sendable {
    private let lock = NSLock()
    private var current: Process?
    private var cancelled = false
    public init() {}
    public func cancel() { lock.lock(); cancelled = true; let process = current; if process?.isRunning == true { process?.terminate() }; lock.unlock() }
    public func run(executable: URL, arguments: [String], networkAllowed: Bool = false, environment: [String: String] = [:]) async throws -> Data {
        try Task.checkCancellation()
        lock.withLock { cancelled = false }
        return try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.global(qos: .userInitiated).async {
                    do {
                        let temp = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
                        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                        defer { try? FileManager.default.removeItem(at: temp) }
                        let out = temp.appendingPathComponent("out"); let err = temp.appendingPathComponent("err")
                        FileManager.default.createFile(atPath: out.path, contents: nil)
                        FileManager.default.createFile(atPath: err.path, contents: nil)
                        let stdout = try FileHandle(forWritingTo: out), stderr = try FileHandle(forWritingTo: err)
                        defer { try? stdout.close(); try? stderr.close() }
                        let process = Process()
                        if networkAllowed {
                            process.executableURL = executable; process.arguments = arguments
                        } else {
                            // Fail closed if sandbox-exec is unavailable; never silently fall back to networking.
                            process.executableURL = URL(fileURLWithPath: "/usr/bin/sandbox-exec")
                            process.arguments = ["-p", "(version 1) (allow default) (deny network*)", executable.path] + arguments
                        }
                        var env = ProcessInfo.processInfo.environment
                        env.merge(environment) { _, new in new }
                        env["HF_HUB_DISABLE_TELEMETRY"] = "1"; env["DO_NOT_TRACK"] = "1"; env["PYTHONUNBUFFERED"] = "1"
                        env["HF_HUB_DISABLE_IMPLICIT_TOKEN"] = "1"
                        if !networkAllowed { env["HF_HUB_OFFLINE"] = "1"; env["TRANSFORMERS_OFFLINE"] = "1" }
                        process.environment = env; process.standardOutput = stdout; process.standardError = stderr
                        self.lock.lock()
                        if self.cancelled { self.lock.unlock(); throw CancellationError() }
                        self.current = process
                        do { try process.run(); self.lock.unlock() } catch { self.current = nil; self.lock.unlock(); throw error }
                        defer { self.lock.lock(); self.current = nil; self.lock.unlock() }
                        process.waitUntilExit()
                        guard process.terminationStatus == 0 else {
                            let details = String(data: try Data(contentsOf: err), encoding: .utf8) ?? ""
                            throw LocalEngineError(details.isEmpty ? "Verarbeitung abgebrochen (\(process.terminationStatus))." : String(details.suffix(4000)))
                        }
                        continuation.resume(returning: try Data(contentsOf: out))
                    } catch { continuation.resume(throwing: error) }
                }
            }
        }, onCancel: { self.cancel() })
    }
}
