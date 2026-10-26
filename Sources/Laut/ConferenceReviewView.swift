import SwiftUI
import LautCore

struct ConferenceReviewView: View {
    @EnvironmentObject var store: AppStore
    let record: Recording
    @State private var start = "00:00"
    @State private var end = "00:00"
    @State private var kind = L("Einspieler")
    @State private var proposals: [SpeakerNameProposal] = []
    @State private var checkedNames = false
    func edit(_ label: String, _ body: (inout ConferenceContext) -> Void) {
        store.edit(record.id, label: label) { item in
            var context = item.conference ?? ConferenceContext(); body(&context); item.conference = context
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            GroupBox(L("Einspieler, Pausen und Vorlauf")) {
                VStack(alignment: .leading, spacing: 10) {
                    Text(L("Ausschlüsse wirken auf Lesefassung, Suche und Textexport. Originaltext, Audio und Zeitachse bleiben erhalten. Bearbeitete Textabschnitte an einer Schnittgrenze bleiben vorsichtshalber sichtbar; Grenzen dort an Abschnittsanfänge legen.")).font(.caption).foregroundStyle(.secondary)
                    Button(L("Hinweise auf Einspieler suchen")) {
                        let found = ConferenceReview.rangeProposals(record)
                        edit(L("Einspieler vorschlagen")) { context in
                            var ranges = context.ranges ?? []
                            ranges += found.filter { item in !ranges.contains { abs($0.start - item.start) < 5 } }
                            context.ranges = ranges
                        }
                        store.status = found.isEmpty ? L("Keine ausdrückliche Einspieler-Ankündigung gefunden. Das schließt Einspieler nicht aus.") : L("Einspieler-Hinweise gefunden – Grenzen bitte nachhören.")
                    }
                    Text(L("Audiohinweise liefern nur Verdachtsstellen, keine visuelle Erkennung. KI-Stimmen können wie der echte Sprecher klingen.")).font(.caption).foregroundStyle(.secondary)
                    ForEach(record.conference?.ranges ?? []) { range in
                        VStack(alignment: .leading, spacing: 5) {
                            HStack {
                                Button(Exporter.timestamp(range.start) + "–" + Exporter.timestamp(range.end)) { store.play(record, at: range.start) }
                                Text(range.kind)
                                Spacer()
                                if !range.confirmed {
                                    Button(L("Grenzen bestätigt · ausblenden")) { edit(L("Bereich ausschließen")) { context in if let i = context.ranges?.firstIndex(where: { $0.id == range.id }) { context.ranges?[i].confirmed = true; context.ranges?[i].excluded = true } } }
                                } else {
                                    Button(range.excluded ? L("Wieder einblenden") : L("Ausblenden")) { edit(L("Bereich umschalten")) { context in if let i = context.ranges?.firstIndex(where: { $0.id == range.id }) { context.ranges?[i].excluded.toggle() } } }
                                }
                                Button(L("Grenzen übernehmen")) { start = Exporter.timestamp(range.start); end = Exporter.timestamp(range.end); kind = L("Einspieler") }
                                Button(L("Entfernen")) { edit(L("Zeitmarkierung entfernen")) { $0.ranges?.removeAll { $0.id == range.id } } }
                            }
                            Text(range.evidence).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                        }
                    }
                    HStack {
                        TextField("Start", text: $start).frame(width: 90)
                        Button(L("← Position")) { start = Exporter.timestamp(store.playerTime) }
                        TextField(L("Ende"), text: $end).frame(width: 90)
                        Button(L("← Position")) { end = Exporter.timestamp(store.playerTime) }
                        Picker(L("Art"), selection: $kind) { Text(L("Einspieler")).tag(L("Einspieler")); Text(L("Pause/Vorlauf")).tag(L("Pause/Vorlauf")) }.frame(width: 190)
                    }.textFieldStyle(.roundedBorder)
                    Button(L("Geprüften Bereich ausschließen")) {
                        guard let a = TranscriptNavigation.parseTime(start), let b = TranscriptNavigation.parseTime(end), a < b, b <= record.duration else { store.error = L("Gültigen Bereich innerhalb der Aufnahme angeben."); return }
                        var range = ConferenceRange(start: a, end: b, kind: kind, evidence: L("Manuell geprüfte Zeitgrenzen"))
                        range.confirmed = true; range.excluded = true
                        edit(L("Bereich ausschließen")) { $0.ranges = ($0.ranges ?? []) + [range] }
                    }
                }.padding(8)
            }
            GroupBox(L("Sprechernamen aus dem Kontext")) {
                VStack(alignment: .leading, spacing: 10) {
                    Text(L("Vorschläge aus Selbstvorstellungen und eindeutigen Übergaben an Namen im Konferenzwörterbuch. Kein Stimmenabgleich mit Personen außerhalb dieser Aufnahme. Bei mehreren Stimmen derselben Person jede Zuordnung einzeln prüfen.")).font(.caption).foregroundStyle(.secondary)
                    Button(L("Namenszuordnungen vorschlagen")) { proposals = ConferenceReview.speakerProposals(record); checkedNames = true }
                    if checkedNames && proposals.isEmpty { Text(L("Keine eindeutige Selbstvorstellung oder Übergabe erkannt. Die manuelle Zuordnung bleibt unter Namen und Fachbegriffe verfügbar.")).font(.caption) }
                    ForEach(proposals) { proposal in
                        VStack(alignment: .leading) {
                            HStack {
                                Text(record.speakerName(proposal.speakerID) + " → " + proposal.name)
                                Button(L("Nachhören")) { store.play(record, at: proposal.time) }
                                Button(L("Zuordnung bestätigen")) {
                                    store.edit(record.id, label: L("Sprecheridentität bestätigen")) { item in
                                        TranscriptEditor.nameSpeaker(proposal.speakerID, name: proposal.name, in: &item)
                                        item.notes += L("\nSprecherzuordnung bestätigt: ") + proposal.name + " · " + Exporter.timestamp(proposal.time) + " · " + String(proposal.evidence.prefix(400))
                                    }
                                    proposals.removeAll { $0.id == proposal.id }
                                }
                            }
                            Text(proposal.evidence).font(.caption).textSelection(.enabled)
                        }
                    }
                }.padding(8)
            }
        }.disabled(store.busy || record.transcriptionProgress != nil)
    }
}
