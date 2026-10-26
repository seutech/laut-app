import LautCore
import SwiftUI
import AppKit

@main
struct LautApp: App {
    @NSApplicationDelegateAdaptor(ApplicationLifecycle.self) private var lifecycle
    @StateObject private var store = AppStore()
    var body: some Scene {
        WindowGroup {
            ContentView().environmentObject(store)
                .environment(\.locale, Locale(identifier: LautLocalization.language.resolved.rawValue))
                .frame(minWidth: 1000, minHeight: 760)
                .onAppear { lifecycle.store = store }
        }
        .commands {
            CommandGroup(after: .undoRedo) {
                Button(L("Dokumentänderung zurücknehmen")) { store.undoEdit() }.keyboardShortcut("z", modifiers: [.command, .option]).disabled(store.busy || store.undoLabel == nil)
                Button(L("Dokumentänderung wiederherstellen")) { store.redoEdit() }.keyboardShortcut("z", modifiers: [.command, .option, .shift]).disabled(store.busy || store.redoLabel == nil)
            }
            CommandGroup(after: .newItem) {
                Button(L("Dateien importieren …")) { store.chooseFiles() }.keyboardShortcut("o")
                Button(L("Schnellnotiz")) { store.newNote() }.keyboardShortcut("n")
            }
        }
        MenuBarExtra {
            Button(L("Öffnen")) { NSApp.activate(ignoringOtherApps: true); NSApp.windows.first?.makeKeyAndOrderFront(nil) }
            Button(L("Dateien importieren …")) { store.chooseFiles() }
            Button(L("Schnellnotiz")) { store.newNote(); NSApp.activate(ignoringOtherApps: true) }
            Divider()
            Button(L("Beenden")) { NSApp.terminate(nil) }.keyboardShortcut("q")
        } label: {
            if store.isRecording { Image(systemName: "record.circle.fill").accessibilityLabel(L("Laut nimmt auf")) }
            else { Image(nsImage: BrandAssets.menuBarMark).renderingMode(.template).accessibilityLabel("Laut") }
        }
    }
}
