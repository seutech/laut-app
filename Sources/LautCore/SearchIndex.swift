import Foundation
import CryptoKit
import CSQLite

public enum SearchMode: String, Codable, CaseIterable, Identifiable, Sendable {
    case fullText, semantic, hybrid
    public var id: String { rawValue }
    public var label: String { switch self { case .fullText: return "Volltext"; case .semantic: return "Bedeutung"; case .hybrid: return "Hybrid" } }
}
public enum EmbeddingModel: String, Codable, CaseIterable, Identifiable, Sendable {
    case small, base
    public var id: String { rawValue }
    public var modelID: String { "intfloat/multilingual-e5-" + rawValue }
    public var label: String { "Multilingual E5 " + rawValue.capitalized }
    public var detail: String { self == .small ? "Deutsch + weitere Sprachen · ca. 495 MB Download · sparsamer Einstieg" : "Deutsch + weitere Sprachen · ca. 1,1 GB Download · höherer Speicherbedarf" }
}
public struct SearchPassage: Codable, Identifiable, Sendable {
    public var id: String
    public var recordingID: UUID
    public var title: String
    public var text: String
    public var source: String
    public var speaker: String
    public var start: Double?
    public var date: Double
    public var fingerprint: String
    public var embeddingText: String { title + "\n" + speaker + "\n" + text }
    public static func passages(_ recordings: [Recording]) -> [SearchPassage] {
        var result: [SearchPassage] = []
        for r in recordings {
            func append(_ text: String, source: String, key: String, speaker: String = "", start: Double? = nil) {
                // Bound index units without cutting a Unicode scalar or losing trailing text.
                let words = text.split(whereSeparator: \.isWhitespace)
                var chunks: [String] = [], chunk = ""
                for word in words {
                    if chunk.count + word.count > 900 && !chunk.isEmpty { chunks.append(chunk); chunk = "" }
                    chunk += (chunk.isEmpty ? "" : " ") + word
                }
                if !chunk.isEmpty { chunks.append(chunk) }
                for (n, body) in chunks.enumerated() {
                    let id = "\(r.id.uuidString)/\(key)/\(n)"
                    let value = [r.title, body, speaker, source, start.map(String.init(describing:)) ?? "", String(r.createdAt.timeIntervalSince1970)].joined(separator: "\u{0}")
                    let hash = SHA256.hash(data: Data(value.utf8)).map { String(format: "%02x", $0) }.joined()
                    result.append(Self(id: id, recordingID: r.id, title: r.title, text: body, source: source, speaker: speaker, start: start, date: r.createdAt.timeIntervalSince1970, fingerprint: hash))
                }
            }
            append(r.title, source: "Titel", key: "title")
            append(r.notes, source: "Notizen", key: "notes")
            append(r.refinedText, source: "Text-KI", key: "refined")
            for segment in r.segments {
                append(segment.text, source: "Transkript", key: segment.id.uuidString, speaker: r.speakerName(segment.speakerID), start: segment.start)
            }
        }
        return result
    }
}
public struct SearchHit: Identifiable, Sendable {
    public var passage: SearchPassage
    public var keyword: Bool
    public var semantic: Bool
    public var id: String { passage.id }
}
public struct SearchFilter: Sendable {
    public var recordingID: UUID?
    public var speaker = ""
    public var since: Date?
    public init(recordingID: UUID? = nil, speaker: String = "", since: Date? = nil) { self.recordingID = recordingID; self.speaker = speaker; self.since = since }
    func allows(_ p: SearchPassage) -> Bool {
        (recordingID == nil || recordingID == p.recordingID) && (speaker.isEmpty || p.speaker.localizedCaseInsensitiveContains(speaker)) && (since == nil || p.date >= since!.timeIntervalSince1970)
    }
}
private struct IndexError: LocalizedError { var message: String; var errorDescription: String? { message } }

/// Disposable local index. Source JSON remains authoritative; updates and vector writes are transactional.
public actor SearchIndex {
    private let url: URL
    private var db: OpaquePointer?
    private var passages: [String: SearchPassage] = [:]
    public init(url: URL) { self.url = url }
    deinit { if let db { sqlite3_close(db) } }
    private func open() throws {
        guard db == nil else { return }
        guard sqlite3_open(url.path, &db) == SQLITE_OK else { throw failure() }
        try execute("PRAGMA journal_mode=WAL")
        try execute("CREATE TABLE IF NOT EXISTS passages(id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, payload BLOB NOT NULL)")
        try execute("CREATE VIRTUAL TABLE IF NOT EXISTS fulltext USING fts5(id UNINDEXED, title, body, speaker, tokenize='unicode61 remove_diacritics 2')")
        try execute("CREATE TABLE IF NOT EXISTS vectors(id TEXT, model TEXT, fingerprint TEXT, vector BLOB, PRIMARY KEY(id,model))")
        let statement = try prepare("SELECT payload FROM passages"); defer { sqlite3_finalize(statement) }
        while sqlite3_step(statement) == SQLITE_ROW {
            let p = try JSONDecoder().decode(SearchPassage.self, from: data(statement, 0)); passages[p.id] = p
        }
    }
    private func failure() -> IndexError { IndexError(message: "Suchindex: " + (db.map { String(cString: sqlite3_errmsg($0)) } ?? "nicht erreichbar")) }
    private func prepare(_ sql: String) throws -> OpaquePointer {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else { throw failure() }; return statement
    }
    private func bind(_ values: [String], to statement: OpaquePointer) {
        for (i, value) in values.enumerated() { _ = value.withCString { sqlite3_bind_text(statement, Int32(i + 1), $0, -1, unsafeBitCast(-1, to: sqlite3_destructor_type.self)) } }
    }
    private func execute(_ sql: String, _ values: [String] = []) throws {
        let statement = try prepare(sql); defer { sqlite3_finalize(statement) }; bind(values, to: statement)
        let code = sqlite3_step(statement); guard code == SQLITE_DONE || code == SQLITE_ROW else { throw failure() }
    }
    private func data(_ statement: OpaquePointer, _ column: Int32) -> Data {
        guard let bytes = sqlite3_column_blob(statement, column) else { return Data() }
        return Data(bytes: bytes, count: Int(sqlite3_column_bytes(statement, column)))
    }
    private func string(_ s: OpaquePointer, _ c: Int32) -> String { sqlite3_column_text(s, c).map { String(cString: $0) } ?? "" }
    public func synchronize(_ recordings: [Recording]) throws {
        try open(); try Task.checkCancellation()
        let next = Dictionary(uniqueKeysWithValues: SearchPassage.passages(recordings).map { ($0.id, $0) })
        try execute("BEGIN IMMEDIATE")
        do {
            for id in passages.keys where next[id] == nil {
                try execute("DELETE FROM fulltext WHERE id=?", [id]); try execute("DELETE FROM passages WHERE id=?", [id]); try execute("DELETE FROM vectors WHERE id=?", [id])
            }
            for p in next.values where passages[p.id]?.fingerprint != p.fingerprint {
                try Task.checkCancellation()
                let json = String(decoding: try JSONEncoder().encode(p), as: UTF8.self)
                try execute("INSERT OR REPLACE INTO passages VALUES(?,?,?)", [p.id, p.fingerprint, json])
                try execute("DELETE FROM fulltext WHERE id=?", [p.id])
                try execute("INSERT INTO fulltext VALUES(?,?,?,?)", [p.id, p.title, p.text, p.speaker])
                try execute("DELETE FROM vectors WHERE id=?", [p.id])
            }
            try execute("COMMIT"); passages = next
        } catch { try? execute("ROLLBACK"); throw error }
    }
    public func missingVectors(model: String) throws -> [SearchPassage] {
        try open()
        let s = try prepare("SELECT id,fingerprint FROM vectors WHERE model=?"); defer { sqlite3_finalize(s) }; bind([model], to: s)
        var ready: [String: String] = [:]
        while sqlite3_step(s) == SQLITE_ROW { ready[string(s, 0)] = string(s, 1) }
        return passages.values.filter { $0.source != "Titel" && ready[$0.id] != $0.fingerprint }.sorted { $0.id < $1.id }
    }
    public func storeVectors(_ vectors: [[Float]], for items: [SearchPassage], model: String) throws {
        guard vectors.count == items.count, let dim = vectors.first?.count, dim > 0,
              vectors.allSatisfy({ $0.count == dim && $0.allSatisfy(\.isFinite) && $0.contains(where: { $0 != 0 }) }) else { throw IndexError(message: "Ungültige Suchmodell-Ausgabe.") }
        try open(); try execute("BEGIN IMMEDIATE")
        do {
            for (p, vector) in zip(items, vectors) where passages[p.id]?.fingerprint == p.fingerprint {
                let json = String(decoding: try JSONEncoder().encode(vector), as: UTF8.self)
                try execute("INSERT OR REPLACE INTO vectors VALUES(?,?,?,?)", [p.id, model, p.fingerprint, json])
            }
            try execute("COMMIT")
        } catch { try? execute("ROLLBACK"); throw error }
    }
    /// Quote every user token: punctuation and SQL/FTS syntax never become executable query syntax.
    public static func fullTextQuery(_ input: String) -> String {
        input.split { !$0.isLetter && !$0.isNumber }.prefix(32).map { "\"\($0)\"*" }.joined(separator: " AND ")
    }
    public func search(_ query: String, mode: SearchMode, model: String = "", vector: [Float]? = nil, filter: SearchFilter = SearchFilter()) throws -> [SearchHit] {
        try open(); try Task.checkCancellation()
        var keyword: [String] = [], semantic: [String] = []
        let expression = Self.fullTextQuery(query)
        if mode != .semantic && !expression.isEmpty {
            let s = try prepare("SELECT id FROM fulltext WHERE fulltext MATCH ? ORDER BY bm25(fulltext,0,3,1,2),id")
            defer { sqlite3_finalize(s) }; bind([expression], to: s)
            while sqlite3_step(s) == SQLITE_ROW {
                let id = string(s, 0)
                if let p = passages[id], filter.allows(p) { keyword.append(id); if keyword.count == 100 { break } }
            }
        }
        if mode != .fullText, let vector, !vector.isEmpty, vector.allSatisfy(\.isFinite) {
            let s = try prepare("SELECT id,fingerprint,vector FROM vectors WHERE model=?"); defer { sqlite3_finalize(s) }; bind([model], to: s)
            var scores: [(String, Float)] = []
            while sqlite3_step(s) == SQLITE_ROW {
                try Task.checkCancellation()
                let id = string(s, 0)
                guard let p = passages[id], filter.allows(p), p.fingerprint == string(s, 1) else { continue }
                let values = try JSONDecoder().decode([Float].self, from: data(s, 2))
                guard values.count == vector.count else { continue }
                let score = zip(values, vector).reduce(Float(0)) { $0 + $1.0 * $1.1 }
                scores.append((id, score))
            }
            semantic = scores.sorted { $0.1 == $1.1 ? $0.0 < $1.0 : $0.1 > $1.1 }.prefix(100).map(\.0)
        }
        let keys = Set(keyword), meanings = Set(semantic)
        var ranks: [String: Double] = [:]
        for list in [keyword, semantic] { for (rank, id) in list.enumerated() { ranks[id, default: 0] += 1 / Double(60 + rank + 1) } }
        return ranks.sorted { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }.prefix(60).compactMap { pair in
            passages[pair.key].map { SearchHit(passage: $0, keyword: keys.contains(pair.key), semantic: meanings.contains(pair.key)) }
        }
    }
}
