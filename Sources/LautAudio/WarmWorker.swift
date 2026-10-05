import Foundation

/// A single serial JSON-lines worker with no listening port and no network access.
public final class WarmWorker: @unchecked Sendable {
    private let queue = DispatchQueue(label: "laut.model.worker", qos: .userInitiated)
    private let lock = NSLock()
    private var process: Process?
    private var input: FileHandle?
    private var output: FileHandle?
    private var log: FileHandle?
    private var directory: URL?
    private var executablePath = ""
    private var cancelled = false
    private var mustRestart = false
    public init() {}
    public var isRunning: Bool { lock.withLock { process?.isRunning == true && !mustRestart } }
    deinit { lock.withLock { reset() } }
    public func stop() { lock.withLock { cancelled = true; mustRestart = true; if process?.isRunning == true { process?.terminate() } } }
    private func reset() {
        if process?.isRunning == true { process?.terminate() }
        try? input?.close(); try? output?.close(); try? log?.close()
        if let directory { try? FileManager.default.removeItem(at: directory) }
        process = nil; input = nil; output = nil; log = nil; directory = nil
    }
    public func run(python: URL, worker: URL, request: [String: Any]) async throws -> Data {
        let data = try JSONSerialization.data(withJSONObject: request) + Data([10])
        try Task.checkCancellation()
        lock.withLock { cancelled = false }
        return try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { continuation in
                queue.async {
                    do {
                        try self.lock.withLock {
                            guard !self.cancelled else { throw CancellationError() }
                            if self.mustRestart || self.process?.isRunning != true || self.executablePath != python.path {
                                self.reset()
                                self.mustRestart = false
                                let folder = FileManager.default.temporaryDirectory.appendingPathComponent("laut-worker-" + UUID().uuidString)
                                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                                self.directory = folder
                                let logURL = folder.appendingPathComponent("stderr")
                                FileManager.default.createFile(atPath: logURL.path, contents: nil)
                                let log = try FileHandle(forWritingTo: logURL)
                                let stdin = Pipe(), stdout = Pipe(), process = Process()
                                process.executableURL = URL(fileURLWithPath: "/usr/bin/sandbox-exec")
                                process.arguments = ["-p", "(version 1) (allow default) (deny network*)", python.path, worker.path, "--stream"]
                                var env = ProcessInfo.processInfo.environment
                                for key in ["HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "HF_HUB_DISABLE_TELEMETRY", "HF_HUB_DISABLE_IMPLICIT_TOKEN", "DO_NOT_TRACK", "PYTHONUNBUFFERED"] { env[key] = "1" }
                                process.environment = env; process.standardInput = stdin; process.standardOutput = stdout; process.standardError = log
                                try process.run()
                                self.process = process; self.input = stdin.fileHandleForWriting; self.output = stdout.fileHandleForReading; self.log = log; self.executablePath = python.path
                            }
                        }
                        guard let input = self.input, let output = self.output else { throw LocalEngineError("Modellprozess konnte nicht gestartet werden.") }
                        try input.write(contentsOf: data)
                        var response = Data()
                        while !response.contains(10) {
                            let chunk = output.availableData
                            guard !chunk.isEmpty else {
                                let detail = self.directory.flatMap { try? String(contentsOf: $0.appendingPathComponent("stderr"), encoding: .utf8) } ?? ""
                                throw LocalEngineError("Lokaler Modellprozess beendet. " + String(detail.suffix(3000)))
                            }
                            response.append(chunk)
                            guard response.count < 128 * 1024 * 1024 else { throw LocalEngineError("Modellausgabe überschreitet 128 MB.") }
                        }
                        if let error = (try JSONSerialization.jsonObject(with: response) as? [String: Any])?["error"] as? String { throw LocalEngineError(error) }
                        continuation.resume(returning: response)
                    } catch {
                        self.lock.withLock { self.reset() }
                        continuation.resume(throwing: error)
                    }
                }
            }
        }, onCancel: { self.stop() })
    }
}
