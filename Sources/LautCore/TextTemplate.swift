import Foundation

public enum TextTemplate: String, CaseIterable, Identifiable, Sendable {
    case custom, summary, minutes, tasks, email
    public var id: String { rawValue }
    public var label: String {
        switch self { case .custom: return L("Eigene Anweisungen"); case .summary: return L("Zusammenfassung"); case .minutes: return L("Protokoll"); case .tasks: return L("Aufgaben"); case .email: return L("E-Mail-Entwurf") }
    }
    public var instructions: String? {
        let rule = L("Verwende nur die bereitgestellten Inhalte. Erfinde keine Fakten, Namen, Termine oder Zusagen. Trenne Vorschläge von getroffenen Entscheidungen. Behalte die Sprache des Ausgangstextes bei. ")
        switch self {
        case .custom: return nil
        case .summary: return rule + L("Fasse die wichtigsten Aussagen kurz und verständlich zusammen.")
        case .minutes: return rule + L("Erstelle ein Protokoll mit Thema, Kernaussagen, Entscheidungen und offenen Fragen. Lasse nicht belegte Punkte weg.")
        case .tasks: return rule + L("Extrahiere Aufgaben. Nenne Verantwortliche und Termine nur, wenn sie ausdrücklich genannt werden; sonst 'nicht genannt'.")
        case .email: return rule + L("Formuliere einen sachlichen E-Mail-Entwurf aus diesen Notizen, mit Betreff und Nachricht.")
        }
    }
}
