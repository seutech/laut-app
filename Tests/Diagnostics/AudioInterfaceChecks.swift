import AVFoundation
import Foundation
import LautAudio
import LautCore

enum AudioInterfaceChecks {
    static func run() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("laut-audio-interface-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("tone.wav"), cache = root.appendingPathComponent("waveform-v1.json")
        try writeTone(source, amplitude: 0.7)
        let reader = WaveformReader()
        let wave = try await reader.load(source, cache: cache)
        try require(abs(wave.duration - 3) < 0.001, "Waveform duration")
        try require(wave.peaks.first == 0 && wave.peaks.last == 0, "Silent regions")
        try require((wave.peaks.max() ?? 0) > 0.69 && wave.peaks[wave.peaks.count / 2] > 0.69, "Tone location and amplitude")
        let saved = try Data(contentsOf: cache)
        let timestamp = try cache.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
        let repeated = try await reader.load(source, cache: cache)
        try require(repeated.peaks == wave.peaks && Data(contentsOf: cache) == saved, "Cached waveform")
        try require(try cache.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate == timestamp, "Cache should not be rewritten on a hit")
        try writeTone(source, amplitude: 0.25)
        try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSinceNow: 60)], ofItemAtPath: source.path)
        let changed = try await reader.load(source, cache: cache)
        try require((changed.peaks.max() ?? 0) < 0.26 && (changed.peaks.max() ?? 0) > 0.24, "Changed source invalidates cache: peak \(changed.peaks.max() ?? -1)")
        try Data("damaged cache".utf8).write(to: cache)
        let repaired = try await reader.load(source, cache: cache)
        try require(repaired.peaks == changed.peaks, "Damaged cache regenerates")
        let cancelled = Task { () throws -> WaveformData in
            withUnsafeCurrentTask { $0?.cancel() }
            return try await reader.load(source, cache: root.appendingPathComponent("cancelled.json"))
        }
        var cancelledCorrectly = false
        do { _ = try await cancelled.value } catch is CancellationError { cancelledCorrectly = true }
        try require(cancelledCorrectly && !FileManager.default.fileExists(atPath: root.appendingPathComponent("cancelled.json").path), "Cancelled waveform leaves no cache")

        let analyzer = SpeakerAnalyzer()
        let skipped = try await analyzer.analyzeIfEnabled(root.appendingPathComponent("missing.wav"), directory: root, enabled: false) { _, _ in }
        try require(skipped == nil, "Disabled speakers must skip models and audio")
        var missingReported = false
        do { _ = try await analyzer.analyzeIfEnabled(source, directory: root, enabled: true) { _, _ in } }
        catch { missingReported = error.localizedDescription.contains("fehlen") }
        try require(missingReported, "Missing speaker models must produce an actionable local error")
        let words = (0..<30).map { i in ["text": i % 10 == 9 ? "Ende." : "Wort", "start": Double(i), "end": Double(i) + 0.5] as [String: Any] }
        let result = try JSONDecoder().decode(TranscriptResult.self, from: JSONSerialization.data(withJSONObject: ["text": "Test", "words": words, "duration_seconds": 30]))
        try require(result.editorSegments().count == 1 && result.editorSegments()[0].words.count == 30, "Readable ASR paragraphs retain word timing")
        print("9 audio interface checks passed: waveform signal/timing, cache reuse/invalidation/recovery, cancellation, speakers off/missing, paragraph granularity")
    }

    private static func require(_ condition: Bool, _ message: String) throws {
        guard condition else { throw LocalEngineError("Check failed: " + message) }
    }
    private static func writeTone(_ url: URL, amplitude: Float) throws {
        let format = AVAudioFormat(standardFormatWithSampleRate: 16000, channels: 1)!
        let file = try AVAudioFile(forWriting: url, settings: format.settings)
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 48000)!
        buffer.frameLength = 48000
        for i in 0..<48000 { buffer.floatChannelData![0][i] = (16000..<32000).contains(i) ? amplitude * Float(sin(Double(i) * 2 * .pi * 440 / 16000)) : 0 }
        try file.write(from: buffer)
    }
}
