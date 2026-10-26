import Foundation
import AVFoundation
import CryptoKit
import LautCore

public enum ConferenceTranscriber {
    /// Bounded ASR memory, two seconds of context, durable checkpoints after each five-minute window.
    /// The callback is awaited so the library commits a partial result before inference continues.
    public static func run(source: URL, folder: URL, engine: TranscriptionEngine, settings: AppSettings,
                           vocabulary: [VocabularyEntry], progress: @escaping @Sendable (TranscriptionCheckpoint) async throws -> Void) async throws -> TranscriptionCheckpoint {
        let duration = try await AVURLAsset(url: source).load(.duration).seconds
        guard duration.isFinite, duration > 0 else { throw LocalEngineError(L("Ungültige Aufnahmedauer.")) }
        let attrs = try FileManager.default.attributesOfItem(atPath: source.path)
        var identity: [String: Any] = [
            "version": 1, "source": source.path, "bytes": (attrs[.size] as? NSNumber)?.int64Value ?? 0,
            "modified": (attrs[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0,
            "engine": settings.engine.rawValue, "model": settings.modelPaths[settings.engine.rawValue] ?? "",
            "language": settings.language, "terms": vocabulary.filter(\.active).map { $0.term }
        ]
        func signature(_ value: [String: Any]) throws -> String {
            SHA256.hash(data: try JSONSerialization.data(withJSONObject: value, options: .sortedKeys)).map { String(format: "%02x", $0) }.joined()
        }
        let legacySignature = try signature(identity)
        // New checkpoints follow byte-identical originals across backups and moves.
        identity["version"] = 2
        identity.removeValue(forKey: "source"); identity.removeValue(forKey: "modified")
        identity["sha256"] = try ConferenceMedia.digest(source)
        let signature = try signature(identity)
        let checkpointFile = folder.appendingPathComponent("transcription-checkpoint.json")
        let storedRecording = (try? Data(contentsOf: folder.appendingPathComponent("recording.json"))).flatMap { try? JSONDecoder().decode(Recording.self, from: $0) }
        let published = (storedRecording?.transcriptionProgress ?? 0) * duration
        var checkpoint = TranscriptionCheckpoint(signature: signature, duration: duration)
        if FileManager.default.fileExists(atPath: checkpointFile.path) {
            let saved: TranscriptionCheckpoint
            do { saved = try JSONDecoder().decode(TranscriptionCheckpoint.self, from: Data(contentsOf: checkpointFile)); try saved.validate() }
            catch { throw LocalEngineError(L("Der Transkript-Zwischenstand ist nicht lesbar oder beschädigt. Die Datei bleibt erhalten. Bitte eine Sicherung verwenden oder eine neue Version starten.")) }
            guard (saved.signature == signature || saved.signature == legacySignature), abs(saved.duration - duration) < 0.01 else {
                throw LocalEngineError(L("Der gespeicherte Abschnitt gehört zu anderen Modell-/Spracheinstellungen. Bitte die ursprünglichen Einstellungen wiederherstellen oder eine neue Version starten."))
            }
            guard saved.completed + 0.01 >= published else { throw LocalEngineError(L("Der Zwischenstand ist älter als der bereits gespeicherte Text. Bitte eine Sicherung prüfen oder eine neue Version starten. Vorhandener Text bleibt erhalten.")) }
            checkpoint = saved
            checkpoint.signature = signature
        } else if published > 0 {
            throw LocalEngineError(L("Der Transkript-Zwischenstand fehlt. Vorhandener Text bleibt erhalten. Bitte die Sicherung wiederherstellen oder eine neue Version starten."))
        }
        try await progress(checkpoint)
        let chunkFile = folder.appendingPathComponent("transcription-chunk.wav")
        defer { try? FileManager.default.removeItem(at: chunkFile) }
        while checkpoint.completed < duration {
            try Task.checkCancellation()
            let start = checkpoint.completed, end = min(duration, start + 300)
            let decodeStart = max(0, start - 2), decodeEnd = min(duration, end + 2)
            let prepared = Date()
            _ = try await AudioFiles.convert(source, to: chunkFile, start: decodeStart, duration: decodeEnd - decodeStart)
            checkpoint.preparationSeconds += Date().timeIntervalSince(prepared)
            let result = try await engine.transcribe(audio: chunkFile, settings: settings, vocabulary: vocabulary)
            try Task.checkCancellation()
            for segment in result.editorSegments() {
                if !segment.words.isEmpty {
                    let words = segment.words.compactMap { word -> Word? in
                        let middle = decodeStart + (word.start + word.end) / 2
                        guard middle >= start, middle < end else { return nil }
                        return Word(text: word.text, start: max(start, decodeStart + word.start), end: min(end, decodeStart + word.end))
                    }
                    if let first = words.first, let last = words.last {
                        checkpoint.segments.append(Segment(start: first.start, end: last.end, text: TranscriptEditor.joinedWords(words), words: words))
                    }
                } else {
                    let middle = decodeStart + (segment.start + segment.end) / 2
                    if middle >= start && middle < end {
                        checkpoint.segments.append(Segment(start: max(start, decodeStart + segment.start), end: min(end, decodeStart + segment.end), text: segment.text))
                    }
                }
            }
            checkpoint.decodeSeconds += result.decode_seconds ?? 0
            checkpoint.loadSeconds += result.load_seconds ?? 0
            checkpoint.completed = end
            try checkpoint.validate()
            try StorageSafety.require(0, at: folder)
            try JSONEncoder().encode(checkpoint).write(to: checkpointFile, options: .atomic)
            try await progress(checkpoint)
        }
        return checkpoint
    }
}
