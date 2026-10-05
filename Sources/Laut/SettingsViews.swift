import SwiftUI
import AppKit
import LautCore

struct ModelsView: View {
    @EnvironmentObject var store: AppStore
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("Dein Mac. Deine Modelle.").font(.largeTitle.bold())
                Text("Downloads stellen eine Verbindung zum jeweiligen Modellanbieter her. Die Verarbeitung danach läuft lokal. Audio und Texte werden nicht übertragen.").foregroundStyle(.secondary)
                HStack {
                    Button("Gewähltes Modell vorladen") { store.prepareSelectedModel(force: true) }
                    Button("Modell aus Speicher entladen") { store.unloadModel() }
                }
                ForEach(EngineKind.allCases) { engine in
                    GroupBox {
                        VStack(alignment: .leading, spacing: 12) {
                            HStack { Text(engine.label).font(.title3.bold()); Spacer(); if installed(engine) { Label("Lokal vorhanden", systemImage: "checkmark.circle.fill").foregroundStyle(.teal).font(.caption) } }
                            Text(engine.detail).foregroundStyle(.secondary)
                            Text(engine.modelID).font(.caption.monospaced()).textSelection(.enabled)
                            HStack {
                                Button(installed(engine) ? "Download prüfen" : "Modell herunterladen") { store.download(engine) }
                                Button("Lokalen Ordner wählen …") { store.chooseDirectory { store.settings.modelPaths[engine.rawValue] = $0 } }
                                Spacer()
                                Button(store.settings.engine == engine ? "Ausgewählt" : "Verwenden") { store.settings.engine = engine; store.saveSettings() }.disabled(!installed(engine))
                            }
                            if let path = store.settings.modelPaths[engine.rawValue] { Text(path).font(.caption2).foregroundStyle(.secondary).textSelection(.enabled) }
                        }.padding(10)
                    }
                }
                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Sprechererkennung").font(.title3.bold())
                        Text("FluidAudio · lokale Core ML-Modelle. Standardmäßig direkt nach der Transkription; auch nachträglich möglich. Der erste Download und die Modellvorbereitung können einige Minuten dauern.").foregroundStyle(.secondary)
                        Button(store.settings.diarizationInstalled ? "Modelle erneut vorbereiten" : "Sprechermodelle herunterladen") { store.downloadSpeakers() }
                    }.padding(10).frame(maxWidth: .infinity, alignment: .leading)
                }
                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Lokale Text-KI").font(.title3.bold())
                        Text("Zum Korrigieren, Umformulieren und für eigene Anweisungen. Unterstützt kompatible MLX-LM-Modelle; große Modelle können den Arbeitsspeicher überlasten.").foregroundStyle(.secondary)
                        TextField("Hugging Face Modell-ID", text: $store.settings.llmModelID).textFieldStyle(.roundedBorder)
                        HStack { Button("Textmodell herunterladen") { store.downloadLLM() }; Button("Lokalen Ordner wählen …") { store.chooseDirectory { store.settings.llmModelPath = $0 } } }
                        if !store.settings.llmModelPath.isEmpty { Text(store.settings.llmModelPath).font(.caption2).foregroundStyle(.secondary).textSelection(.enabled) }
                    }.padding(10)
                }
                Text("Eigene ASR-Modelle müssen zum gewählten Backend passen. Eine beliebige ONNX-, GGUF- oder BIN-Datei ist kein austauschbares Sprachmodell.").font(.caption).foregroundStyle(.secondary)
            }.padding(32).frame(maxWidth: 900)
        }.disabled(store.busy || store.isRecording)
    }
    func installed(_ engine: EngineKind) -> Bool { store.settings.modelPaths[engine.rawValue].map { FileManager.default.fileExists(atPath: $0) } ?? false }
}

struct VocabularyView: View {
    @EnvironmentObject var store: AppStore
    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack { Text("Dein Wörterbuch").font(.largeTitle.bold()); Spacer(); Button { store.vocabulary.append(VocabularyEntry(term: "")); store.saveVocabulary() } label: { Label("Begriff", systemImage: "plus") } }
            Text("Namen, Fachbegriffe und eigene Schreibweisen. Phonon bekommt bis zu 25 aktive Begriffe als Erkennungshilfe. Häufige Falschschreibweisen werden nach der Transkription durch den richtigen Begriff ersetzt.").foregroundStyle(.secondary)
            HStack { Text("Aktiv").frame(width: 45); Text("Richtige Schreibweise").frame(maxWidth: .infinity, alignment: .leading); Text("Falschschreibweisen, mit Komma getrennt").frame(maxWidth: .infinity, alignment: .leading) }.font(.caption).foregroundStyle(.secondary)
            ScrollView {
                LazyVStack(spacing: 12) {
                    ForEach($store.vocabulary) { $entry in
                        HStack {
                            Toggle("Aktiv", isOn: $entry.active).labelsHidden().frame(width: 45)
                            TextField("z. B. Laut", text: $entry.term).textFieldStyle(.roundedBorder)
                            TextField("z. B. Loud, Lauth", text: $entry.aliases).textFieldStyle(.roundedBorder)
                            Button { store.vocabulary.removeAll { $0.id == entry.id }; store.saveVocabulary() } label: { Image(systemName: "minus.circle") }.buttonStyle(.plain)
                        }
                    }
                }
            }
            Text("Änderungen werden lokal gespeichert. Ersetzungen ändern weder alte Transkripte noch Originaltexte automatisch.").font(.caption).foregroundStyle(.secondary)
        }.padding(32).disabled(store.busy)
        .onChange(of: vocabularyFingerprint) { store.saveVocabulary() }
    }
    var vocabularyFingerprint: String { store.vocabulary.map { "\($0.id)\($0.term)\($0.aliases)\($0.active)" }.joined() }
}

struct PreferencesView: View {
    @EnvironmentObject var store: AppStore
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("Laut nach deinen Regeln.").font(.largeTitle.bold())
                GroupBox("Transkription") {
                    VStack(alignment: .leading, spacing: 10) {
                        Toggle("Sprecher automatisch erkennen", isOn: Binding(get: { store.settings.speakerDetectionEnabled }, set: { store.settings.speakerDetectionEnabled = $0; store.saveSettings() }))
                        Text("Standardmäßig werden Sprecher direkt nach der Transkription erkannt. Ausschalten spart die zusätzliche Analyse. Das Transkript wird vorher gespeichert; fehlende Sprechermodelle werden nicht automatisch heruntergeladen.").font(.caption).foregroundStyle(.secondary)
                    }.padding(12)
                }
                GroupBox("Sprache & Anweisungen") {
                    VStack(alignment: .leading, spacing: 14) {
                        Picker("Bevorzugte Sprache", selection: $store.settings.language) { Text("Deutsch").tag("de"); Text("English").tag("en"); Text("Automatisch").tag("auto") }.frame(width: 320)
                        Text("Phonon und Parakeet erkennen die Sprache selbst. Die Auswahl wird an Qwen3-ASR übergeben.").font(.caption).foregroundStyle(.secondary)
                        Text("Eigene Anweisungen für die Text-KI").font(.headline)
                        TextEditor(text: $store.settings.customInstructions).font(.body).frame(height: 140).padding(8).border(.quaternary)
                        Text("Diese Anweisungen gelten für die Textbearbeitung nach der Transkription und für ⌃⌥R.").font(.caption).foregroundStyle(.secondary)
                    }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
                GroupBox("Tastenkürzel") {
                    VStack(alignment: .leading, spacing: 12) {
                        LabeledContent("Diktat starten / stoppen", value: "⌃⌥Leertaste")
                        LabeledContent("Markierten Text mit lokaler KI bearbeiten", value: "⌃⌥R")
                        Text("Das Einfügen braucht die macOS-Berechtigung „Bedienungshilfen“. Wenn die Ziel-App kein Einfügen unterstützt oder du die App wechselst, bleibt das Ergebnis in Laut. Die Zwischenablage wird dafür nicht verwendet.").font(.caption).foregroundStyle(.secondary)
                        Button("Bedienungshilfen erlauben") { _ = TextInsertion.permitted() }
                    }.padding(12)
                }
                GroupBox("Lokale Speicherung") {
                    VStack(alignment: .leading, spacing: 12) {
                        Toggle("Audio von Diktaten behalten", isOn: $store.settings.keepDictationAudio)
                        Text("Importierte Dateien und Meetingaufnahmen bleiben erhalten, bis du sie löschst. Die Bibliothek ist nicht zusätzlich verschlüsselt; macOS und deine Datenträgerverschlüsselung schützen sie.").font(.caption).foregroundStyle(.secondary)
                        Text(store.library.root.path).font(.caption.monospaced()).textSelection(.enabled)
                        Button("Bibliothek im Finder öffnen") { NSWorkspace.shared.open(store.library.root) }
                    }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
                GroupBox("Entwicklerversion · Laufzeit") {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Diese erste Version verwendet eine isolierte Python-/MLX-Laufzeit. Richte sie einmal mit scripts/setup-runtime.sh ein.").foregroundStyle(.secondary)
                        HStack { TextField(".runtime-Ordner", text: $store.settings.runtimeDirectory).textFieldStyle(.roundedBorder); Button("Wählen …") { store.chooseDirectory { store.settings.runtimeDirectory = $0 } } }
                        Text("Keine Telemetrie, keine Anmeldung, keine Cloud-Transkription. Downloads werden ausschließlich über die Modell-Schaltflächen gestartet.").font(.caption).foregroundStyle(.secondary)
                    }.padding(12)
                }
            }.padding(32).frame(maxWidth: 900)
        }.disabled(store.busy || store.isRecording)
        .onDisappear { store.saveSettings() }
    }
}
