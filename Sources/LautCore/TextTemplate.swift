import Foundation

public enum TextTemplate: String, CaseIterable, Identifiable, Sendable {
    case custom, summary, minutes, tasks, email
    public var id: String { rawValue }
    public var label: String {
        switch self { case .custom: return "Eigene Anweisungen"; case .summary: return "Zusammenfassung"; case .minutes: return "Protokoll"; case .tasks: return "Aufgaben"; case .email: return "E-Mail-Entwurf" }
    }
    public var instructions: String? {
        let rule = "Verwende nur die bereitgestellten Inhalte. Erfinde keine Fakten, Namen, Termine oder Zusagen. Trenne Vorschläge von getroffenen Entscheidungen. Behalte die Sprache des Ausgangstextes bei. "
        switch self {
        case .custom: return nil
        case .summary: return rule + "Fasse die wichtigsten Aussagen kurz und verständlich zusammen."
        case .minutes: return rule + "Erstelle ein Protokoll mit Thema, Kernaussagen, Entscheidungen und offenen Fragen. Lasse nicht belegte Punkte weg."
        case .tasks: return rule + "Extrahiere Aufgaben. Nenne Verantwortliche und Termine nur, wenn sie ausdrücklich genannt werden; sonst 'nicht genannt'."
        case .email: return rule + "Formuliere einen sachlichen E-Mail-Entwurf aus diesen Notizen, mit Betreff und Nachricht."
        }
    }
}
