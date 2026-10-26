import LautCore
import AppKit

@MainActor
final class ApplicationLifecycle: NSObject, NSApplicationDelegate {
    weak var store: AppStore?
    private var terminating = false

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let store else { return .terminateNow }
        if terminating { return .terminateLater }
        if store.isRecording {
            let alert = NSAlert()
            alert.messageText = L("Die Aufnahme läuft noch.")
            alert.informativeText = L("Beende und speichere die Aufnahme, bevor du Laut schließt.")
            alert.addButton(withTitle: L("Aufnahme beenden"))
            alert.addButton(withTitle: L("Weiter aufnehmen"))
            if alert.runModal() == .alertFirstButtonReturn { store.stopRecording() }
            return .terminateCancel
        }
        guard store.busy || store.importJobTask != nil || store.storageTask != nil else { return .terminateNow }
        let operation = store.task, queue = store.importJobTask, maintenance = store.storageTask
        guard operation != nil || queue != nil || maintenance != nil else {
            let alert = NSAlert()
            alert.messageText = L("Laut speichert oder verarbeitet noch.")
            alert.informativeText = L("Bitte diesen Vorgang abschließen lassen und Laut danach beenden.")
            alert.runModal()
            return .terminateCancel
        }
        let alert = NSAlert()
        alert.messageText = L("Verarbeitung anhalten und Laut beenden?")
        alert.informativeText = L("Gespeicherte Dateien und Transkript-Zwischenstände bleiben erhalten. Beim Fortsetzen kann der letzte noch nicht gespeicherte Abschnitt erneut verarbeitet werden.")
        alert.addButton(withTitle: L("Anhalten & beenden"))
        alert.addButton(withTitle: L("Weiterarbeiten"))
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        terminating = true
        store.systemSleeping = true // Also prevents queued work from starting during shutdown.
        if !store.maintenanceRunning { store.pauseImportQueue() }
        maintenance?.cancel(); operation?.cancel(); queue?.cancel()
        store.engine.runner.cancel(); store.engine.warmWorker.stop()
        Task {
            await maintenance?.value
            await queue?.value
            await operation?.value
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}
