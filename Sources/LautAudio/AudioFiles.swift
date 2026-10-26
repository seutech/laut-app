import AVFoundation
import Foundation
import LautCore

public enum AudioFiles {
    /// Decode in small buffers; long meetings never need to fit in memory.
    public static func convert(_ source: URL, to target: URL) async throws -> Double {
        let asset = AVURLAsset(url: source)
        return try await convert(asset, to: target)
    }
    public static func convert(_ source: URL, to target: URL, start: Double, duration: Double) async throws -> Double {
        guard start.isFinite, duration.isFinite, start >= 0, duration > 0 else { throw LocalEngineError(L("Ungültiger Audioabschnitt.")) }
        return try await convert(AVURLAsset(url: source), to: target, range: CMTimeRange(start: CMTime(seconds: start, preferredTimescale: 16000), duration: CMTime(seconds: duration, preferredTimescale: 16000)))
    }
    public static func mix(_ sources: [(URL, Double)], to target: URL) async throws -> Double {
        let composition = AVMutableComposition()
        for (url, delay) in sources {
            let asset = AVURLAsset(url: url)
            let tracks = try await asset.loadTracks(withMediaType: .audio)
            let duration = try await asset.load(.duration)
            for track in tracks {
                guard let output = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) else { throw LocalEngineError(L("Audiospuren konnten nicht verbunden werden.")) }
                try output.insertTimeRange(CMTimeRange(start: .zero, duration: duration), of: track, at: CMTime(seconds: max(0, delay), preferredTimescale: 48000))
            }
        }
        return try await convert(composition, to: target)
    }
    private static func convert(_ asset: AVAsset, to target: URL, range: CMTimeRange? = nil) async throws -> Double {
        let seconds: Double
        if let range { seconds = range.duration.seconds }
        else { seconds = try await asset.load(.duration).seconds }
        guard seconds.isFinite, seconds > 0, seconds < Double(Int64.max / 32000) else { throw LocalEngineError(L("Ungültige Audiodauer.")) }
        try StorageSafety.require(Int64(ceil(seconds * 32000)) + 4096, at: target.deletingLastPathComponent())
        let tracks = try await asset.loadTracks(withMediaType: .audio)
        guard !tracks.isEmpty else { throw LocalEngineError(L("Diese Datei enthält keine lesbare Audiospur.")) }
        let reader = try AVAssetReader(asset: asset)
        if let range { reader.timeRange = range }
        let settings: [String: Any] = [AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: 16000, AVNumberOfChannelsKey: 1, AVLinearPCMBitDepthKey: 16, AVLinearPCMIsFloatKey: false, AVLinearPCMIsBigEndianKey: false, AVLinearPCMIsNonInterleaved: false]
        let output = AVAssetReaderAudioMixOutput(audioTracks: tracks, audioSettings: settings)
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
        guard reader.startReading() else { throw reader.error ?? LocalEngineError(L("Audio konnte nicht geöffnet werden.")) }
        let format = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16000, channels: 1, interleaved: true)!
        let file = try AVAudioFile(forWriting: target, settings: format.settings, commonFormat: .pcmFormatInt16, interleaved: true)
        var total: Int64 = 0
        do {
            while let sample = output.copyNextSampleBuffer() {
                try Task.checkCancellation()
                guard let block = CMSampleBufferGetDataBuffer(sample) else { continue }
                let length = CMBlockBufferGetDataLength(block)
                guard length > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(length / 2)), let dest = buffer.int16ChannelData?[0] else { continue }
                let status = CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: dest)
                guard status == kCMBlockBufferNoErr else { throw LocalEngineError(L("Audio-Decoder lieferte einen ungültigen Puffer.")) }
                buffer.frameLength = AVAudioFrameCount(length / 2)
                try file.write(from: buffer); total += Int64(buffer.frameLength)
            }
            guard reader.status == .completed else { throw reader.error ?? LocalEngineError(L("Audioimport unvollständig.")) }
            guard total > 0 else { throw LocalEngineError(L("Die Audiospur ist leer.")) }
        } catch { reader.cancelReading(); throw error }
        return Double(total) / 16000
    }
}
