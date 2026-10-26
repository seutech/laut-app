import Foundation

public struct TranscriptionCheckpoint: Codable, Sendable {
    public var signature: String
    public var completed: Double = 0
    public var duration: Double
    public var segments: [Segment] = []
    public var preparationSeconds: Double = 0
    public var decodeSeconds: Double = 0
    public var loadSeconds: Double = 0
    public init(signature: String, duration: Double) { self.signature = signature; self.duration = duration }
    public func validate() throws {
        guard duration.isFinite, duration > 0, completed.isFinite, completed >= 0, completed <= duration,
              [preparationSeconds, decodeSeconds, loadSeconds].allSatisfy({ $0.isFinite && $0 >= 0 }),
              Set(segments.map(\.id)).count == segments.count,
              segments.allSatisfy({ segment in
                  segment.start.isFinite && segment.end.isFinite && segment.start >= 0 &&
                  segment.end >= segment.start && segment.end <= completed + 0.01 &&
                  segment.words.allSatisfy { $0.start.isFinite && $0.end.isFinite && $0.start >= segment.start - 0.01 && $0.end >= $0.start && $0.end <= segment.end + 0.01 }
              }) else { throw ImportJobError(L("Der Transkript-Zwischenstand ist beschädigt. Er bleibt unverändert erhalten. Bitte eine Sicherung verwenden oder eine neue Version starten.")) }
    }
}
