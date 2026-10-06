import SwiftUI
import AppKit
import LautCore

extension AppStore {
    var markdownDirectory: URL {
        if let path = settings.markdownDirectory, !path.isEmpty { return URL(fileURLWithPath: path) }
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Laut/Transkripte")
    }
    func scheduleMarkdownArchive() {
        markdownTask?.cancel()
        guard settings.markdownExportEnabled ?? true else { markdownStatus = "Automatisches Markdown-Archiv ausgeschaltet"; return }
        let snapshot = recordings, folder = markdownDirectory
        markdownTask = Task {
            do {
                try await Task.sleep(for: .seconds(1))
                let report = try await markdownArchive.synchronize(snapshot, directory: folder)
                try Task.checkCancellation()
                markdownStatus = report.conflicts > 0 ? "\(report.conflicts) extern bearbeitete Datei(en) erhalten; neue Kopien angelegt" : "Markdown-Archiv aktuell"
            } catch {
                if !Task.isCancelled { markdownStatus = "Markdown-Archiv nicht aktualisiert: \(error.localizedDescription)" }
            }
        }
    }
}

struct MarkdownArchiveSettings: View {
    @EnvironmentObject var store: AppStore
    var body: some View {
        GroupBox("Markdown-Archiv") {
            VStack(alignment: .leading, spacing: 12) {
                Toggle("Transkripte und Notizen automatisch als Markdown speichern", isOn: Binding(get: { store.settings.markdownExportEnabled ?? true }, set: { store.settings.markdownExportEnabled = $0; store.saveSettings() }))
                Text("YYYY-MM-DD TITEL.md · Datum der Aufnahme bzw. des Imports in Laut. Enthält Auswertung, Notizen, Sprecher und Zeitmarken. Korrekturen in Laut aktualisieren die Dateien automatisch.").font(.caption).foregroundStyle(.secondary)
                Text(store.markdownDirectory.path).font(.caption.monospaced()).textSelection(.enabled)
                HStack {
                    Button("Ordner wählen …") { store.chooseDirectory { store.settings.markdownDirectory = $0 } }
                    Button("Im Finder öffnen") { NSWorkspace.shared.open(store.markdownDirectory) }
                    Button("Jetzt aktualisieren") { store.scheduleMarkdownArchive() }
                }
                Text(store.markdownStatus).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                Text("Externe Änderungen werden erhalten; bei Konflikten entsteht eine zusätzliche Datei. Änderungen an Markdown fließen nicht automatisch zurück nach Laut. Archivkopien bleiben auch beim Löschen eines Eintrags in Laut erhalten. Bei einem Ordnerwechsel bleiben vorhandene Kopien im bisherigen Ordner.").font(.caption).foregroundStyle(.secondary)
            }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
