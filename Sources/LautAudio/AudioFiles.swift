import AVFoundation
import Foundation

public enum AudioFiles {
    /// Decode in small buffers; long meetings never need to fit in memory.
    public static func convert(_ source: URL, to target: URL) async throws -> Double {
        let asset = AVURLAsset(url: source)
        return try await convert(asset, to: target)
    }
    public static func mix(_ sources: [(URL, Double)], to target: URL) async throws -> Double {
        let composition = AVMutableComposition()
        for (url, delay) in sources {
            let asset = AVURLAsset(url: url)
            let tracks = try await asset.loadTracks(withMediaType: .audio)
            let duration = try await asset.load(.duration)
            for track in tracks {
                guard let output = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) else { throw LocalEngineError("Audiospuren konnten nicht verbunden werden.") }
                try output.insertTimeRange(CMTimeRange(start: .zero, duration: duration), of: track, at: CMTime(seconds: max(0, delay), preferredTimescale: 48000))
            }
        }
        return try await convert(composition, to: target)
    }
    private static func convert(_ asset: AVAsset, to target: URL) async throws -> Double {
        let tracks = try await asset.loadTracks(withMediaType: .audio)
        guard !tracks.isEmpty else { throw LocalEngineError("Diese Datei enthält keine lesbare Audiospur.") }
        let reader = try AVAssetReader(asset: asset)
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
        guard reader.startReading() else { throw reader.error ?? LocalEngineError("Audio konnte nicht geöffnet werden.") }
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
                guard status == kCMBlockBufferNoErr else { throw LocalEngineError("Audio-Decoder lieferte einen ungültigen Puffer.") }
                buffer.frameLength = AVAudioFrameCount(length / 2)
                try file.write(from: buffer); total += Int64(buffer.frameLength)
            }
            guard reader.status == .completed else { throw reader.error ?? LocalEngineError("Audioimport unvollständig.") }
            guard total > 0 else { throw LocalEngineError("Die Audiospur ist leer.") }
        } catch { reader.cancelReading(); throw error }
        return Double(total) / 16000
    }
}
