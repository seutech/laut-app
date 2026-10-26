import SwiftUI
import AppKit
import LautCore

struct StorageSettings: View {
    @EnvironmentObject var store: AppStore
    var body: some View {
        GroupBox(L("Speicher & Sicherung")) {
            VStack(alignment: .leading, spacing: 12) {
                if let report = store.storageReport {
                    LabeledContent(L("Bibliothek"), value: StorageSafety.format(report.total))
                    LabeledContent(L("Audio / Video"), value: StorageSafety.format(report.audio))
                    LabeledContent("PDFs", value: StorageSafety.format(report.documents))
                    LabeledContent(L("Text, Metadaten und weitere Dateien"), value: StorageSafety.format(report.other))
                    LabeledContent(L("Auf dem Datenträger verfügbar"), value: report.available.map(StorageSafety.format) ?? L("Nicht ermittelbar"))
                    if !report.issues.isEmpty {
                        DisclosureGroup(L("\(report.issues.count) Hinweise zur Ablage")) {
                            Text(report.issues.joined(separator: "\n\n")).font(.caption).textSelection(.enabled)
                        }.foregroundStyle(.orange)
                    }
                }
                Text(L("Die Größen beziehen sich auf diesen Bibliotheksordner. Modelle und externe Konferenzarchive liegen separat. Die Prüfung meldet fehlende Medien; sie ersetzt keine inhaltliche Transkriptprüfung.")).font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button(L("Speicher & Dateien prüfen")) { store.inspectStorage() }
                    Button(L("Sicherung erstellen …")) { store.createLibraryBackup() }
                    Button(L("Sicherung wiederherstellen …")) { store.restoreLibraryBackup() }
                }.disabled(store.busy || store.isRecording || store.importJobTask != nil)
                Text(L("Eine Sicherung enthält Originale, Texte, Bearbeitungsstände, Wörterbuch und Einstellungen mit Prüfsummen. Modelle, Suchindizes und externe Quelldateien noch wartender Aufträge sind nicht enthalten. Wiederherstellen legt einen neuen Ordner an; die aktuelle Bibliothek bleibt erhalten.")).font(.caption).foregroundStyle(.secondary)
                if !store.storageStatus.isEmpty { Text(store.storageStatus).font(.callout).textSelection(.enabled) }
                if let restored = store.restoredLibrary {
                    Button(L("Wiederhergestellte Bibliothek in Laut öffnen")) {
                        let configuration = NSWorkspace.OpenConfiguration()
                        configuration.createsNewApplicationInstance = true
                        configuration.arguments = ["--library", restored.path]
                        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, error in
                            if let error { Task { @MainActor in store.error = error.localizedDescription } }
                        }
                    }
                }
            }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

extension AppStore {
    func inspectStorage() {
        guard !busy, !isRecording, storageTask == nil else { return }
        let root = library.root
        runStorageOperation(L("Bibliothek prüfen …")) {
            let report = try LibraryStorageReport.inspect(root)
            return (report, nil, report.issues.isEmpty ? L("Dateiprüfung abgeschlossen. Keine fehlenden Medien gefunden.") : L("Dateiprüfung abgeschlossen. Bitte die Hinweise zur Ablage prüfen."))
        }
    }
    func createLibraryBackup() {
        guard !busy, !isRecording, storageTask == nil, importJobTask == nil else { return }
        let panel = NSSavePanel()
        panel.title = L("Neuen Sicherungsordner wählen")
        panel.nameFieldStringValue = L("Laut-Sicherung-") + Date().formatted(.iso8601.year().month().day())
        guard panel.runModal() == .OK, let destination = panel.url else { return }
        do { try library.saveSettings(settings); try library.saveVocabulary(vocabulary) }
        catch { self.error = L("Aktuelle Einstellungen konnten vor der Sicherung nicht gespeichert werden: ") + error.localizedDescription; return }
        let root = library.root
        runStorageOperation(L("Sicherung erstellen und Prüfsummen vergleichen …")) {
            let manifest = try LibraryBackup.create(from: root, to: destination)
            return (nil, nil, L("Sicherung geprüft: \(manifest.files.count) Dateien unter \(destination.path)"))
        }
    }
    func restoreLibraryBackup() {
        guard !busy, !isRecording, storageTask == nil, importJobTask == nil else { return }
        let sourcePanel = NSOpenPanel()
        sourcePanel.title = L("Laut-Sicherungsordner auswählen")
        sourcePanel.canChooseFiles = false; sourcePanel.canChooseDirectories = true
        guard sourcePanel.runModal() == .OK, let source = sourcePanel.url else { return }
        let targetPanel = NSSavePanel()
        targetPanel.title = L("Neuen Ordner für die wiederhergestellte Bibliothek wählen")
        targetPanel.nameFieldStringValue = L("Laut-Wiederhergestellt")
        guard targetPanel.runModal() == .OK, let destination = targetPanel.url else { return }
        runStorageOperation(L("Sicherung prüfen und in neuen Ordner wiederherstellen …")) {
            try LibraryBackup.restore(source, to: destination)
            return (nil, destination, L("Wiederhergestellt und geprüft: \(destination.path). Die bisherige Bibliothek ist unverändert. Ältere Transkript-Zwischenstände können eine neue Version erfordern."))
        }
    }
    private func runStorageOperation(_ message: String, operation: @escaping @Sendable () throws -> (LibraryStorageReport?, URL?, String)) {
        stopPlayback()
        if queueError == nil { pauseImportQueue() }
        maintenanceRunning = true; busy = true; storageStatus = message; status = message
        storageTask = Task {
            defer { maintenanceRunning = false; busy = false; storageTask = nil }
            let worker = Task.detached(priority: .utility) { try operation() }
            do {
                let (report, restored, message) = try await withTaskCancellationHandler { try await worker.value } onCancel: { worker.cancel() }
                if let report { storageReport = report }
                if let restored { restoredLibrary = restored }
                storageStatus = message; status = L("Speicherprüfung abgeschlossen")
            } catch {
                storageStatus = Task.isCancelled ? L("Abgebrochen. Bestehende Bibliothek und Sicherungen bleiben erhalten.") : error.localizedDescription
                if !Task.isCancelled { self.error = storageStatus }
                status = L("Speichervorgang nicht abgeschlossen")
            }
        }
    }
    func updateProcessingActivity() {
        if busy || isRecording {
            if processingActivity == nil { processingActivity = ProcessInfo.processInfo.beginActivity(options: .userInitiated, reason: L("Laut verarbeitet oder sichert eine Aufnahme")) }
        } else if let token = processingActivity {
            ProcessInfo.processInfo.endActivity(token); processingActivity = nil
        }
    }
    func installSleepHandling() {
        let center = NSWorkspace.shared.notificationCenter
        workspaceObservers.append(center.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.systemSleeping = true
                if self.isRecording { self.stopRecording() }
                if self.maintenanceRunning { self.storageTask?.cancel() }
                else {
                    self.pauseImportQueue()
                    self.task?.cancel(); self.engine.runner.cancel(); self.engine.warmWorker.stop()
                }
                self.stopPlayback()
            }
        })
        workspaceObservers.append(center.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in
                self?.systemSleeping = false
                self?.status = L("Mac wieder aktiv. Pausierte Aufträge bitte ausdrücklich fortsetzen.")
            }
        })
    }
}
