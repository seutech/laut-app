import AVFoundation
import Foundation

public struct WaveformData: Codable, Sendable {
    public var duration: Double
    public var peaks: [Float]
}

/// Decodes small buffers on an actor executor; neither the audio nor its waveform leaves the Mac.
public actor WaveformReader {
    public init() {}
    private struct Cache: Codable {
        var version = 1
        var filename: String
        var bytes: Int
        var modified: Date
        var waveform: WaveformData
    }
    public func load(_ source: URL, cache: URL) async throws -> WaveformData {
        try Task.checkCancellation()
        var freshSource = source
        freshSource.removeAllCachedResourceValues()
        let attributes = try freshSource.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])
        let bytes = attributes.fileSize ?? 0, modified = attributes.contentModificationDate ?? .distantPast
        if let data = try? Data(contentsOf: cache), let saved = try? JSONDecoder().decode(Cache.self, from: data),
           saved.version == 1, saved.filename == source.lastPathComponent, saved.bytes == bytes, saved.modified == modified,
           saved.waveform.duration.isFinite, saved.waveform.duration > 0, !saved.waveform.peaks.isEmpty,
           saved.waveform.peaks.count <= 60000, saved.waveform.peaks.allSatisfy({ $0.isFinite && $0 >= 0 && $0 <= 1 }) { return saved.waveform }
        let asset = AVURLAsset(url: source)
        let duration = try await asset.load(.duration).seconds
        guard duration.isFinite, duration > 0 else { throw LocalEngineError("Keine gültige Audiodauer für die Wellenform.") }
        let tracks = try await asset.loadTracks(withMediaType: .audio)
        guard !tracks.isEmpty else { throw LocalEngineError("Keine Audiospur für die Wellenform.") }
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderAudioMixOutput(audioTracks: tracks, audioSettings: [AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: 16000, AVNumberOfChannelsKey: 1, AVLinearPCMBitDepthKey: 32, AVLinearPCMIsFloatKey: true, AVLinearPCMIsBigEndianKey: false, AVLinearPCMIsNonInterleaved: false])
        if tracks.count > 1 {
            let mix = AVMutableAudioMix()
            mix.inputParameters = tracks.map { track in
                let parameters = AVMutableAudioMixInputParameters(track: track)
                parameters.setVolume(1 / Float(tracks.count), at: .zero)
                return parameters
            }
            output.audioMix = mix
        }
        reader.add(output)
        guard reader.startReading() else { throw reader.error ?? LocalEngineError("Wellenform konnte nicht gelesen werden.") }
        let count = Int(min(60000, max(1024, ceil(duration * 30))))
        var peaks = [Float](repeating: 0, count: count)
        do {
            while let sample = output.copyNextSampleBuffer() {
                try Task.checkCancellation()
                guard let block = CMSampleBufferGetDataBuffer(sample) else { continue }
                let length = CMBlockBufferGetDataLength(block)
                var values = [Float](repeating: 0, count: length / MemoryLayout<Float>.size)
                guard !values.isEmpty else { continue }
                let byteCount = values.count * MemoryLayout<Float>.size
                let result = values.withUnsafeMutableBytes { CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: byteCount, destination: $0.baseAddress!) }
                guard result == kCMBlockBufferNoErr else { throw LocalEngineError("Ungültige Audiodaten für die Wellenform.") }
                let start = CMSampleBufferGetPresentationTimeStamp(sample).seconds
                guard start.isFinite else { continue }
                for (i, value) in values.enumerated() where value.isFinite {
                    let time = start + Double(i) / 16000
                    let index = min(count - 1, max(0, Int(time / duration * Double(count))))
                    peaks[index] = max(peaks[index], min(1, abs(value)))
                }
            }
            try Task.checkCancellation()
            guard reader.status == .completed else { throw reader.error ?? LocalEngineError("Wellenform unvollständig.") }
        } catch { reader.cancelReading(); throw error }
        let waveform = WaveformData(duration: duration, peaks: peaks)
        let saved = Cache(filename: source.lastPathComponent, bytes: bytes, modified: modified, waveform: waveform)
        // A cache write failure must not prevent playback or waveform display.
        if !Task.isCancelled, FileManager.default.fileExists(atPath: source.path) {
            try? JSONEncoder().encode(saved).write(to: cache, options: .atomic)
        }
        return waveform
    }
}
