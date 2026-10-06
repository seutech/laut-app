import SwiftUI
import AppKit
import LautCore

struct ModelsView: View {
    @EnvironmentObject var store: AppStore
    @State private var category = "Sprache"
    @State private var removal: ModelRemoval?
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Dein Mac. Deine Modelle.").font(.largeTitle.bold())
                Text("Du wählst, was lokal installiert wird. Nur Downloads verbinden sich mit Modellanbietern; Audio und Texte bleiben auf deinem Mac.").foregroundStyle(.secondary)
                Picker("Modelltyp", selection: $category) {
                    ForEach(["Sprache", "Sprecher", "Text-KI", "Suche"], id: \.self) { Text($0).tag($0) }
                }.pickerStyle(.segmented)
                if !store.downloadProgress.isEmpty {
                    Label(store.downloadProgress, systemImage: "arrow.down.circle").font(.callout.monospacedDigit())
                }
                if store.busy {
                    HStack { ProgressView().controlSize(.small); Text(store.status).font(.callout); Spacer(); Button("Abbrechen") { store.cancel() } }
                }
                if category == "Sprache" { speech }
                if category == "Sprecher" { speakers }
                if category == "Text-KI" { textModel }
                if category == "Suche" { searchModels }
                Text("Abgebrochene Hugging-Face-Downloads werden beim nächsten Download fortgesetzt. Die Prozentanzeige kann bei bereits gespeicherten Dateien springen. Modellordner müssen zum jeweiligen Backend passen.").font(.caption).foregroundStyle(.secondary)
            }.padding(32).frame(maxWidth: 950)
        }
        .confirmationDialog(removal?.question ?? "Modell entfernen?", isPresented: Binding(get: { removal != nil }, set: { if !$0 { removal = nil } }), titleVisibility: .visible) {
            if let item = removal { Button(item.target == nil ? "Verknüpfung entfernen" : "Modelldateien löschen", role: .destructive) { store.removeModel(item); removal = nil } }
        }
    }
    var unavailable: Bool { store.busy || store.isRecording }
    func installed(_ path: String?) -> Bool { path.map { FileManager.default.fileExists(atPath: $0) } ?? false }
    func location(_ path: String?) -> some View {
        Group {
            if let path, !path.isEmpty {
                HStack {
                    Label(installed(path) ? "Lokal vorhanden" : "Ordner fehlt", systemImage: installed(path) ? "checkmark.circle" : "exclamationmark.triangle").font(.caption).foregroundStyle(installed(path) ? .teal : .orange)
                    Spacer()
                    Button("Im Finder") { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: path) }.controlSize(.small)
                }
                Text(path).font(.caption2).foregroundStyle(.secondary).textSelection(.enabled)
            } else { Text("Nicht installiert").font(.caption).foregroundStyle(.secondary) }
        }
    }
    var speech: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Button("Auswahl vorladen") { store.prepareSelectedModel(force: true) }
                Button("Arbeitsspeicher freigeben") { store.unloadModel() }
            }.disabled(unavailable)
            ForEach(EngineKind.allCases) { engine in
                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack { Text(engine.label).font(.title3.bold()); Spacer(); if store.settings.engine == engine { Text("Ausgewählt").foregroundStyle(.teal) } }
                        Text(engine.detail).foregroundStyle(.secondary)
                        Text(engine == .qwen ? "Abschnittszeitmarken · experimenteller Adapter" : "Wortzeitmarken · lokal auf Apple Silicon").font(.caption)
                        Text(engine.modelID).font(.caption.monospaced()).textSelection(.enabled)
                        location(store.settings.modelPaths[engine.rawValue])
                        HStack {
                            Button(installed(store.settings.modelPaths[engine.rawValue]) ? "Download prüfen" : "Herunterladen / fortsetzen") { store.download(engine) }
                            Button("Lokaler Ordner …") { store.chooseDirectory { store.settings.modelPaths[engine.rawValue] = $0 } }
                            Spacer()
                            Button("Verwenden") { store.settings.engine = engine; store.saveSettings() }.disabled(!installed(store.settings.modelPaths[engine.rawValue]))
                            if let path = store.settings.modelPaths[engine.rawValue] { Button(role: .destructive) { removal = ModelRemoval(slot: .speech(engine), path: path, runtime: store.settings.runtimeDirectory, modelID: engine.modelID) } label: { Image(systemName: "trash") } }
                        }.disabled(unavailable)
                    }.padding(10)
                }
            }
            Text("RAM-Bedarf und Geschwindigkeit hängen von Aufnahme, Modell und Quantisierung ab. Phonon und Parakeet wurden lokal geprüft; eine vergleichbare RAM-Messung steht noch aus.").font(.caption).foregroundStyle(.secondary)
        }
    }
    var speakers: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                Text("FluidAudio · Sprechererkennung").font(.title3.bold())
                Text("Lokale Core-ML-Modelle · ca. 21 MB installierte Modelldateien. Erkennung nach der Transkription; manuelle Korrekturen bleiben möglich.").foregroundStyle(.secondary)
                location(store.settings.diarizationInstalled ? store.modelDirectory.path : nil)
                HStack {
                    Button("Herunterladen / vorbereiten") { store.downloadSpeakers() }
                    Spacer()
                    if store.settings.diarizationInstalled { Button("Entfernen", role: .destructive) { removal = ModelRemoval(slot: .speakers, path: store.modelDirectory.path, runtime: store.settings.runtimeDirectory, modelID: "speakers") } }
                }.disabled(unavailable)
                Text("Modellvorbereitung zeigt keinen verlässlichen Prozentwert.").font(.caption).foregroundStyle(.secondary)
            }.padding(10)
        }
    }
    var textModel: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                Text("Lokale Text-KI · MLX-LM").font(.title3.bold())
                Text("Für eigene Anweisungen, Zusammenfassungen und Umformulierungen. Modellgröße und RAM-Bedarf hängen vom gewählten Repository ab.").foregroundStyle(.secondary)
                TextField("Hugging-Face-Modell-ID", text: $store.settings.llmModelID).textFieldStyle(.roundedBorder).disabled(unavailable)
                Text("Vorschlag: mlx-community/Qwen3-1.7B-4bit · auch Deutsch. Eigene kompatible MLX-LM-Modelle sind möglich.").font(.caption).foregroundStyle(.secondary)
                location(store.settings.llmModelPath.isEmpty ? nil : store.settings.llmModelPath)
                Text("Die Modell-ID gilt für den nächsten Download. Verarbeitet wird mit dem angezeigten lokalen Ordner.").font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Herunterladen / fortsetzen") { store.downloadLLM() }
                    Button("Lokaler Ordner …") { store.chooseDirectory { store.settings.llmModelPath = $0 } }
                    Spacer()
                    if !store.settings.llmModelPath.isEmpty { Button("Entfernen", role: .destructive) { removal = ModelRemoval(slot: .text, path: store.settings.llmModelPath, runtime: store.settings.runtimeDirectory, modelID: store.settings.llmModelID) } }
                }.disabled(unavailable)
            }.padding(10)
        }
    }
    var searchModels: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Volltext funktioniert ohne Suchmodell. Bedeutung und Hybrid verwenden ein lokales Embedding-Modell. Die Bibliothek wird nach Änderungen im Hintergrund indexiert; während Transkriptionen pausiert die Bedeutungssuche.").foregroundStyle(.secondary)
            ForEach(EmbeddingModel.allCases) { model in
                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack { Text(model.label).font(.title3.bold()); Spacer(); if store.selectedEmbedding == model { Text("Ausgewählt").foregroundStyle(.teal) } }
                        Text(model.detail).foregroundStyle(.secondary)
                        Text(model.modelID).font(.caption.monospaced())
                        location(store.settings.embeddingPaths?[model.rawValue])
                        HStack {
                            Button("Herunterladen / fortsetzen") { store.downloadEmbedding(model) }
                            Spacer()
                            Button("Verwenden") { store.useEmbedding(model) }.disabled(!installed(store.settings.embeddingPaths?[model.rawValue]))
                            if let path = store.settings.embeddingPaths?[model.rawValue] { Button("Entfernen", role: .destructive) { removal = ModelRemoval(slot: .embedding(model), path: path, runtime: store.settings.runtimeDirectory, modelID: model.modelID) } }
                        }.disabled(unavailable)
                    }.padding(10)
                }
            }
            Text(store.searchStatus).font(.caption).foregroundStyle(.secondary)
        }
    }
}

struct ModelRemoval: Identifiable {
    enum Slot { case speech(EngineKind), speakers, text, embedding(EmbeddingModel) }
    let id = UUID()
    let slot: Slot
    let path: String
    let target: URL?
    init(slot: Slot, path: String, runtime: String, modelID: String) {
        self.slot = slot; self.path = path
        target = ModelFiles.removalTarget(path: path, runtime: runtime, modelID: modelID)
    }
    var question: String { target == nil ? "Lokalen Modellordner aus Laut entfernen? Deine externen Dateien bleiben erhalten." : "Die von Laut verwalteten Modelldateien löschen? Transkripte und Audio bleiben erhalten; das Modell lässt sich erneut herunterladen." }
}
extension AppStore {
    func removeModel(_ item: ModelRemoval) {
        guard !busy, !isRecording else { return }
        searchTask?.cancel(); embeddings.worker.stop(); unloadModel()
        do {
            if let target = item.target, FileManager.default.fileExists(atPath: target.path) { try FileManager.default.removeItem(at: target) }
            switch item.slot {
            case .speech(let engine): settings.modelPaths.removeValue(forKey: engine.rawValue)
            case .speakers: settings.diarizationInstalled = false
            case .text: settings.llmModelPath = ""
            case .embedding(let model): settings.embeddingPaths?.removeValue(forKey: model.rawValue)
            }
            saveSettings(); scheduleSearch()
        } catch { self.error = error.localizedDescription }
    }
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
                MarkdownArchiveSettings()
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
