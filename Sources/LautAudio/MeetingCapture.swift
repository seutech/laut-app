import AVFoundation
import ScreenCaptureKit
import Foundation

/// Captures system audio and microphone independently, then mixes their real start offsets.
public final class MeetingCapture: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    private let queue = DispatchQueue(label: "laut.meeting.audio")
    private var stream: SCStream?
    private var writer: AVAssetWriter?
    private var input: AVAssetWriterInput?
    private var microphone: AVAudioRecorder?
    private var firstSystemPTS: CMTime?
    private var microphoneStart = 0.0
    private var failure: Error?
    private let directory: URL
    public init(directory: URL) { self.directory = directory }
    public func start() async throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else { throw LocalEngineError("Kein Bildschirm für Systemaudio gefunden.") }
        let config = SCStreamConfiguration()
        config.capturesAudio = true; config.excludesCurrentProcessAudio = true
        config.sampleRate = 48000; config.channelCount = 2
        config.width = 2; config.height = 2; config.minimumFrameInterval = CMTime(seconds: 1, preferredTimescale: 1)
        let writer = try AVAssetWriter(outputURL: directory.appendingPathComponent("system.m4a"), fileType: .m4a)
        let input = AVAssetWriterInput(mediaType: .audio, outputSettings: [AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 48000, AVNumberOfChannelsKey: 2, AVEncoderBitRateKey: 192000])
        input.expectsMediaDataInRealTime = true; writer.add(input)
        self.writer = writer; self.input = input
        let recorder = try AVAudioRecorder(url: directory.appendingPathComponent("microphone.m4a"), settings: [AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 48000, AVNumberOfChannelsKey: 1, AVEncoderBitRateKey: 128000])
        microphone = recorder
        let stream = SCStream(filter: SCContentFilter(display: display, excludingWindows: []), configuration: config, delegate: self)
        self.stream = stream
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: queue)
        try await stream.startCapture()
        microphoneStart = CMClockGetTime(CMClockGetHostTimeClock()).seconds
        guard recorder.record() else { try? await stream.stopCapture(); throw LocalEngineError("Meeting-Mikrofon konnte nicht gestartet werden.") }
    }
    public func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid, CMSampleBufferDataIsReady(sampleBuffer), CMSampleBufferGetNumSamples(sampleBuffer) > 0, let writer, let input, failure == nil else { return }
        if firstSystemPTS == nil {
            let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
            guard writer.startWriting() else { failure = writer.error ?? LocalEngineError("Systemaudio-Datei konnte nicht geöffnet werden."); return }
            writer.startSession(atSourceTime: pts); firstSystemPTS = pts
        }
        guard input.isReadyForMoreMediaData else { failure = LocalEngineError("Systemaudio konnte nicht schnell genug gespeichert werden. Die Teilaufnahme bleibt erhalten."); return }
        if !input.append(sampleBuffer) { failure = writer.error ?? LocalEngineError("Systemaudio-Aufnahme abgebrochen.") }
    }
    public func stream(_ stream: SCStream, didStopWithError error: Error) { queue.async { self.failure = error } }
    public func stop() async throws -> URL {
        microphone?.stop(); microphone = nil
        do { try await stream?.stopCapture() } catch { queue.async { self.failure = error } }
        stream = nil
        let snapshot: (AVAssetWriter?, CMTime?, Error?) = await withCheckedContinuation { continuation in
            queue.async { if self.writer?.status == .writing { self.input?.markAsFinished() }; continuation.resume(returning: (self.writer, self.firstSystemPTS, self.failure)) }
        }
        if let writer = snapshot.0, snapshot.1 != nil { await writer.finishWriting() }
        if let failure = snapshot.2 { throw failure }
        let output = directory.appendingPathComponent("meeting.wav")
        let mic = directory.appendingPathComponent("microphone.m4a")
        if let systemStart = snapshot.1?.seconds {
            guard snapshot.0?.status == .completed else { throw snapshot.0?.error ?? LocalEngineError("Systemaudio wurde nicht vollständig gespeichert.") }
            let origin = min(systemStart, microphoneStart)
            _ = try await AudioFiles.mix([(directory.appendingPathComponent("system.m4a"), systemStart - origin), (mic, microphoneStart - origin)], to: output)
        } else {
            throw LocalEngineError("Keine Systemaudio-Daten empfangen. Die Mikrofonaufnahme liegt unter \(mic.path).")
        }
        return output
    }
}
