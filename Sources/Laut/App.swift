import SwiftUI
import AppKit

@main
struct LautApp: App {
    @StateObject private var store = AppStore()
    var body: some Scene {
        WindowGroup {
            ContentView().environmentObject(store)
                .frame(minWidth: 1000, minHeight: 680)
        }
        .commands {
            CommandGroup(after: .newItem) {
                Button("Dateien importieren …") { store.chooseFiles() }.keyboardShortcut("o")
                Button("Schnellnotiz") { store.newNote() }.keyboardShortcut("n")
            }
        }
        MenuBarExtra("Laut", systemImage: store.isRecording ? "record.circle.fill" : "waveform") {
            Button("Öffnen") { NSApp.activate(ignoringOtherApps: true); NSApp.windows.first?.makeKeyAndOrderFront(nil) }
            Button("Dateien importieren …") { store.chooseFiles() }
            Button("Schnellnotiz") { store.newNote(); NSApp.activate(ignoringOtherApps: true) }
            Divider()
            Button("Beenden") { NSApp.terminate(nil) }.keyboardShortcut("q")
        }
    }
}
