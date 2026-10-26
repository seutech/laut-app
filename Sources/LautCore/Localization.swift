import Foundation

public enum InterfaceLanguage: String, Codable, CaseIterable, Sendable {
    case system, de, en
    public var resolved: InterfaceLanguage {
        self == .system ? (Locale.preferredLanguages.first?.hasPrefix("de") == true ? .de : .en) : self
    }
}

/// UI messages keep interpolation values separate from their translation keys.
/// User content, filenames and model identifiers are never translated implicitly.
public struct LocalizedMessage: ExpressibleByStringLiteral, ExpressibleByStringInterpolation {
    public var key: String
    public var arguments: [String]
    public init(stringLiteral value: String) { key = value; arguments = [] }
    public init(stringInterpolation value: StringInterpolation) { key = value.key; arguments = value.arguments }
    public struct StringInterpolation: StringInterpolationProtocol {
        var key = ""
        var arguments: [String] = []
        public init(literalCapacity: Int, interpolationCount: Int) {
            key.reserveCapacity(literalCapacity); arguments.reserveCapacity(interpolationCount)
        }
        public mutating func appendLiteral(_ literal: String) { key += literal }
        public mutating func appendInterpolation<T>(_ value: T) {
            key += "{\(arguments.count)}"; arguments.append(String(describing: value))
        }
    }
}

public enum LautLocalization {
    private static let lock = NSLock()
    private static var selected: InterfaceLanguage = .de
    public static var language: InterfaceLanguage {
        get { lock.lock(); defer { lock.unlock() }; return selected }
        set { lock.lock(); defer { lock.unlock() }; selected = newValue }
    }
    public static var locale: Locale { Locale(identifier: language.resolved.rawValue) }
    public static let english: [String: String] = {
        guard let url = Bundle.module.url(forResource: "en", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let catalog = try? JSONDecoder().decode([String: String].self, from: data) else { return [:] }
        return catalog
    }()
    private static let placeholders = try! NSRegularExpression(pattern: #"\{([0-9]+)\}"#)

    /// Only for our workers' diagnostic/progress messages, never transcript content.
    /// Unknown third-party diagnostics remain intact, including filenames and traces.
    public static func workerMessage(_ message: String) -> String {
        guard language.resolved == .en else { return message }
        func translate(_ line: String) -> String {
            if let translated = english[line] { return translated }
            for key in english.keys.filter({ $0.hasSuffix(": ") }).sorted(by: { $0.count > $1.count }) where line.hasPrefix(key) {
                return english[key]! + line.dropFirst(key.count)
            }
            return line
        }
        return message.components(separatedBy: "\n").map { line in
            for prefix in ["ValueError: ", "RuntimeError: ", "InterruptedError: "] where line.hasPrefix(prefix) {
                return prefix + translate(String(line.dropFirst(prefix.count)))
            }
            return translate(line)
        }.joined(separator: "\n")
    }

    public static func render(_ message: LocalizedMessage, language: InterfaceLanguage? = nil) -> String {
        let template = (language ?? self.language).resolved == .en ? english[message.key] ?? message.key : message.key
        let source = template as NSString
        var output = "", offset = 0
        for match in placeholders.matches(in: template, range: NSRange(location: 0, length: source.length)) {
            output += source.substring(with: NSRange(location: offset, length: match.range.location - offset))
            let index = Int(source.substring(with: match.range(at: 1)))!
            output += message.arguments.indices.contains(index) ? message.arguments[index] : source.substring(with: match.range)
            offset = NSMaxRange(match.range)
        }
        output += source.substring(from: offset)
        return output
    }
}

public func L(_ message: LocalizedMessage) -> String { LautLocalization.render(message) }
