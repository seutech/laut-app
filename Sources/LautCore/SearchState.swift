import Foundation

/// Identifies the exact library/query combination that produced a set of results.
public struct SearchContext: Equatable, Sendable {
    public var query: String
    public var mode: SearchMode
    public var recordingID: UUID?
    public var speaker: String
    public var days: Int
    public var revision: Int
    public var model: String
    public init(query: String, mode: SearchMode, recordingID: UUID?, speaker: String, days: Int, revision: Int, model: String) {
        self.query = query; self.mode = mode; self.recordingID = recordingID; self.speaker = speaker
        self.days = days; self.revision = revision; self.model = model
    }
}
public struct SearchState: Sendable {
    public private(set) var context: SearchContext?
    private var request: UUID?
    private var ready = false
    public init() {}
    public mutating func begin(_ context: SearchContext) -> UUID {
        let token = UUID(); self.context = context; request = token; ready = false; return token
    }
    @discardableResult public mutating func finish(_ token: UUID) -> Bool {
        guard request == token else { return false }; ready = true; return true
    }
    public func canAnswer(_ context: SearchContext) -> Bool {
        ready && self.context == context && !context.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}
