import SwiftUI
import AppKit
import UniformTypeIdentifiers
import LautCore

struct ContentView: View {
    @EnvironmentObject var store: AppStore
    @State private var dropping = false
    var body: some View {
        NavigationSplitView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 10) {
                    Image(systemName: "waveform.circle.fill").font(.system(size: 32)).foregroundStyle(.teal)
                    VStack(alignment: .leading) { Text("Laut").font(.title3.bold()); Text("Deine Stimme. Dein Mac.").font(.caption).foregroundStyle(.secondary) }
                }.padding(.top, 16)
                VStack(spacing: 6) {
                    nav("Bibliothek", symbol: "rectangle.stack", key: "library")
                    nav("Wörterbuch", symbol: "text.book.closed", key: "vocabulary")
                    nav("Modelle", symbol: "cpu", key: "models")
                    nav("Einstellungen", symbol: "slider.horizontal.3", key: "settings")
                }
                Divider()
                HStack { Text("VERLAUF").font(.caption.weight(.semibold)).foregroundStyle(.secondary); Spacer(); Button { store.newNote() } label: { Image(systemName: "square.and.pencil") }.buttonStyle(.plain).help("Schnellnotiz") }
                TextField("Suchen …", text: $store.search).textFieldStyle(.roundedBorder)
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(store.filtered) { record in
                            Button {
                                store.stopPlayback(); store.selected = record.id; store.section = "library"
                            } label: {
                                HStack(alignment: .top, spacing: 9) {
                                    Image(systemName: record.kind == .note ? "note.text" : "waveform").foregroundStyle(.teal).padding(.top, 3)
                                    VStack(alignment: .leading, spacing: 5) {
                                        Text(record.title).font(.callout.weight(.medium)).lineLimit(2)
                                        Text(record.createdAt.formatted(date: .abbreviated, time: .shortened)).font(.caption2).foregroundStyle(.secondary)
                                    }; Spacer(minLength: 0)
                                    if record.state == .failed { Image(systemName: "exclamationmark.circle").foregroundStyle(.orange) }
                                }.padding(10).frame(maxWidth: .infinity, alignment: .leading)
                                    .background(store.selected == record.id && store.section == "library" ? Color.teal.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 9))
                            }.buttonStyle(.plain)
                        }
                    }
                }
                Label("Kein Konto. Keine Cloud.", systemImage: "lock.shield").font(.caption).foregroundStyle(.secondary)
            }.padding(.horizontal, 15).padding(.bottom, 16)
            .navigationSplitViewColumnWidth(min: 230, ideal: 260, max: 340)
        } detail: {
            VStack(spacing: 0) {
                Group {
                    switch store.section {
                    case "vocabulary": VocabularyView()
                    case "models": ModelsView()
                    case "settings": PreferencesView()
                    default:
                        if let record = store.current { RecordingView(record: record).id(record.id) }
                        else { welcome }
                    }
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
                Divider()
                HStack(spacing: 9) {
                    if store.busy { ProgressView().controlSize(.small) }
                    else { Image(systemName: store.isRecording ? "record.circle.fill" : "lock.fill").foregroundStyle(store.isRecording ? .red : .teal) }
                    Text(store.status).font(.caption).lineLimit(2)
                    Spacer()
                    if store.busy { Button("Abbrechen") { store.cancel() }.controlSize(.small) }
                    if store.isRecording { Button("Aufnahme beenden") { store.stopRecording() }.tint(.red) }
                }.padding(12).background(.bar)
            }
            .overlay { if dropping { RoundedRectangle(cornerRadius: 16).stroke(.teal, style: StrokeStyle(lineWidth: 3, dash: [8])).padding(12).allowsHitTesting(false) } }
            .onDrop(of: [.fileURL], isTargeted: $dropping) { providers in
                for provider in providers {
                    _ = provider.loadObject(ofClass: URL.self) { url, _ in
                        if let url { Task { @MainActor in store.importFiles([url]) } }
                    }
                }; return true
            }
        }
        .tint(.teal)
        .onChange(of: store.settings.engine) { store.saveSettings() }
        .toolbar {
            ToolbarItemGroup {
                Button { store.chooseFiles() } label: { Label("Importieren", systemImage: "plus") }
                Button { store.toggleRecording(kind: .meeting) } label: { Label(store.isRecording ? "Stoppen" : "Mikrofon", systemImage: store.isRecording ? "stop.circle.fill" : "mic") }.disabled(store.busy)
                Button { store.startMeeting() } label: { Label("Meeting", systemImage: "person.2.wave.2") }.disabled(store.busy || store.isRecording).help("Mikrofon und Systemaudio aufnehmen")
            }
        }
        .alert("Hinweis", isPresented: Binding(get: { store.error != nil }, set: { if !$0 { store.error = nil } })) { Button("OK") { store.error = nil } } message: { Text(store.error ?? "") }
    }
    func nav(_ title: String, symbol: String, key: String) -> some View {
        Button { store.section = key } label: {
            Label(title, systemImage: symbol).frame(maxWidth: .infinity, alignment: .leading).padding(9)
                .background(store.section == key ? Color.teal.opacity(0.1) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
        }.buttonStyle(.plain)
    }
    var welcome: some View {
        VStack(alignment: .leading, spacing: 25) {
            Label("LOKAL AUF DEINEM MAC", systemImage: "lock.shield").font(.caption.weight(.semibold)).foregroundStyle(.teal)
            Text("Aus Gesprochenem\nwird dein Text.").font(.system(size: 42, weight: .semibold, design: .rounded))
            Text("Audio oder Video hier ablegen. Transkribieren, Sprecher zuordnen und Gedanken festhalten — alles auf deinem Gerät.").font(.title3).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Button { store.chooseFiles() } label: { Label("Dateien auswählen", systemImage: "arrow.down.doc").padding(7) }.buttonStyle(.borderedProminent)
            HStack(alignment: .top, spacing: 30) {
                feature("01", "Datei importieren", "Die Quelldatei bleibt erhalten.")
                feature("02", "Lokal transkribieren", "Du entscheidest über das Modell.")
                feature("03", "Sprecher korrigieren", "Namen und Zuordnung bearbeiten.")
            }.padding(.top, 26)
            Text("Diktat: ⌃⌥Leertaste    ·    Text-KI: ⌃⌥R").font(.caption).foregroundStyle(.secondary)
        }.padding(50).frame(maxWidth: 860, alignment: .leading)
    }
    func feature(_ number: String, _ title: String, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: 8) { Text(number).font(.caption.monospaced()).foregroundStyle(.teal); Text(title).font(.headline); Text(detail).font(.caption).foregroundStyle(.secondary) }.frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct RecordingView: View {
    @EnvironmentObject var store: AppStore
    let record: Recording
    @State private var tab = 0
    @State private var speakerCount = 0
    @State private var splitSegment: Segment?
    @State private var confirmDelete = false
    @State private var confirmAudioDelete = false
    func binding<T>(_ key: WritableKeyPath<Recording, T>) -> Binding<T> {
        Binding(get: { store.recordings.first { $0.id == record.id }?[keyPath: key] ?? record[keyPath: key] }, set: { value in store.update(record.id) { $0[keyPath: key] = value } })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 8) {
                    TextField("Titel", text: binding(\.title)).textFieldStyle(.plain).font(.title.bold())
                    Text(record.createdAt.formatted(date: .long, time: .shortened) + (record.duration > 0 ? "  ·  " + Exporter.timestamp(record.duration) : "") + (record.engine.isEmpty ? "" : "  ·  " + record.engine)).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Menu {
                    ForEach(["txt", "md", "srt", "json"], id: \.self) { format in Button(format.uppercased()) { store.export(record, format: format) } }
                    Divider()
                    Button("Nur Audiodateien löschen …", role: .destructive) { confirmAudioDelete = true }.disabled(record.audioFilename == nil || store.busy)
                    Button("Eintrag löschen …", role: .destructive) { confirmDelete = true }.disabled(store.busy)
                } label: { Label("Export & mehr", systemImage: "square.and.arrow.up") }
            }
            if record.kind != .note {
                HStack {
                    Picker("Modell", selection: $store.settings.engine) { ForEach(EngineKind.allCases) { Text($0.label).tag($0) } }.frame(maxWidth: 280)
                    Button(record.segments.isEmpty ? "Transkribieren" : "Neue Version") { store.transcribe(record.id) }.buttonStyle(.borderedProminent).disabled(store.busy || store.isRecording || record.audioFilename == nil)
                    if record.segments.isEmpty { Button("Alle offenen Dateien") { store.transcribeAll() }.disabled(store.busy || store.isRecording) }
                    Spacer()
                }
                if record.audioFilename != nil {
                    HStack(spacing: 12) {
                        Button { store.play(record) } label: { Image(systemName: store.playing ? "pause.fill" : "play.fill") }.buttonStyle(.bordered)
                        Slider(value: Binding(get: { min(store.playerTime, max(record.duration, 1)) }, set: { store.player?.currentTime = $0; store.playerTime = $0 }), in: 0...max(record.duration, 1))
                        Text(Exporter.timestamp(store.playerTime)).font(.caption.monospacedDigit())
                    }.padding(12).background(Color.teal.opacity(0.06), in: RoundedRectangle(cornerRadius: 10))
                }
            }
            Picker("Ansicht", selection: $tab) {
                if record.kind != .note { Text("Transkript").tag(0); Text("Sprecher").tag(1) }
                Text("Notizen").tag(2); Text("Text-KI").tag(3)
            }.pickerStyle(.segmented)
            if let error = record.error { Label(error, systemImage: "exclamationmark.triangle").font(.callout).foregroundStyle(.orange).textSelection(.enabled) }
            Group {
                switch tab {
                case 1: speakers
                case 2: TextEditor(text: binding(\.notes)).font(.body).padding(8).overlay(alignment: .topLeading) { if record.notes.isEmpty { Text("Gedanken, Stichpunkte, Aufgaben …").foregroundStyle(.tertiary).padding(13).allowsHitTesting(false) } }
                case 3:
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Deine Anweisungen aus den Einstellungen werden auf den Text angewendet. Das Ergebnis bleibt getrennt vom Original.").font(.callout).foregroundStyle(.secondary)
                        Button("Lokal bearbeiten") { store.refine(record.id) }.disabled(store.busy)
                        TextEditor(text: binding(\.refinedText)).font(.body)
                    }
                default: transcript
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
        }.padding(28)
        .onAppear { if record.kind == .note { tab = 2 } }
        .sheet(item: $splitSegment) { segment in SplitView(segment: segment) { pieces in
            store.update(record.id) { item in if let index = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments.replaceSubrange(index...index, with: pieces) } }
        } }
        .confirmationDialog("Eintrag und zugehörige lokale Dateien endgültig löschen?", isPresented: $confirmDelete) { Button("Löschen", role: .destructive) { store.delete(record) } }
        .confirmationDialog("Audiodateien löschen? Text und Notizen bleiben erhalten. Wiedergabe und neue Sprecheranalyse sind danach nicht mehr möglich.", isPresented: $confirmAudioDelete) { Button("Audio löschen", role: .destructive) { store.removeAudio(record.id) } }
    }
    var transcript: some View {
        ScrollView {
            if record.segments.isEmpty {
                ContentUnavailableView("Bereit für dein Transkript", systemImage: "text.alignleft", description: Text("Wähle ein installiertes Modell und starte die lokale Transkription."))
            }
            LazyVStack(alignment: .leading, spacing: 16) {
                ForEach(record.segments) { segment in
                    VStack(alignment: .leading, spacing: 9) {
                        HStack {
                            Button(Exporter.timestamp(segment.start)) { store.play(record, at: segment.start) }.font(.caption.monospacedDigit()).buttonStyle(.plain).foregroundStyle(.teal)
                            Picker("Sprecher", selection: Binding(get: { segment.speakerID ?? "" }, set: { value in store.update(record.id) { item in if let i = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments[i].speakerID = value.isEmpty ? nil : value; item.segments[i].speakerLocked = true } } })) {
                                Text("Unzugeordnet").tag("")
                                ForEach(record.speakers) { Text($0.name).tag($0.id) }
                            }.labelsHidden().fixedSize().disabled(store.busy)
                            if segment.speakerLocked { Image(systemName: "lock.fill").font(.caption2).foregroundStyle(.secondary).help("Manuelle Zuordnung bleibt bei neuer Analyse erhalten") }
                            Spacer()
                            Menu {
                                Button("Abschnitt teilen …") { splitSegment = segment }.disabled(segment.text.count < 2)
                                Button("Mit nächstem Abschnitt verbinden") { mergeNext(segment) }
                                Button("Automatische Sprecherzuordnung wieder erlauben") { store.update(record.id) { item in if let i = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments[i].speakerLocked = false } } }
                            } label: { Image(systemName: "ellipsis") }.menuStyle(.borderlessButton).fixedSize().disabled(store.busy)
                        }
                        TextField("Text", text: Binding(get: { segment.text }, set: { text in store.update(record.id) { item in if let i = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments[i].text = text } } }), axis: .vertical)
                            .textFieldStyle(.plain).font(.system(size: 15)).lineSpacing(5).disabled(store.busy)
                        if segment.text != segment.originalText { Text("Bearbeitet").font(.caption2).foregroundStyle(.secondary).help("Original: " + segment.originalText) }
                    }.padding(16).background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 12))
                }
            }
        }
    }
    var speakers: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text("Wer hat was gesagt?").font(.title2.weight(.semibold))
                Text("Erkennung liefert zunächst neutrale Sprecher. Benenne sie um und korrigiere einzelne Abschnitte im Transkript. Manuelle Zuordnungen sind vor einer erneuten Analyse geschützt.").foregroundStyle(.secondary)
                HStack {
                    Picker("Anzahl", selection: $speakerCount) { Text("Automatisch").tag(0); ForEach(1...12, id: \.self) { Text("\($0)").tag($0) } }.frame(width: 200)
                    Button("Sprecher erkennen") { store.diarize(record.id, count: speakerCount == 0 ? nil : speakerCount) }.disabled(store.busy || record.segments.isEmpty || record.audioFilename == nil)
                    Button("Sprecher hinzufügen") { store.update(record.id) { $0.speakers.append(Speaker(name: "Neue Person")) } }.disabled(store.busy)
                }
                ForEach(record.speakers) { speaker in
                    HStack {
                        Image(systemName: "person.crop.circle.fill").foregroundStyle(.teal).font(.title2)
                        TextField("Name", text: Binding(get: { speaker.name }, set: { name in store.update(record.id) { item in
                            if let i = item.speakers.firstIndex(where: { $0.id == speaker.id }) { item.speakers[i].name = name }
                            for i in item.segments.indices where item.segments[i].speakerID == speaker.id { item.segments[i].speakerLocked = true }
                        } })).textFieldStyle(.roundedBorder)
                        Text("\(record.segments.filter { $0.speakerID == speaker.id }.count) Abschnitte").font(.caption).foregroundStyle(.secondary)
                        Menu("Zusammenführen mit …") {
                            ForEach(record.speakers.filter { $0.id != speaker.id }) { other in
                                Button(other.name) { store.update(record.id) { item in
                                    for i in item.segments.indices where item.segments[i].speakerID == speaker.id { item.segments[i].speakerID = other.id; item.segments[i].speakerLocked = true }
                                    item.speakers.removeAll { $0.id == speaker.id }
                                } }
                            }
                        }.fixedSize()
                    }.disabled(store.busy)
                }
            }.padding(.vertical, 8)
        }
    }
    func mergeNext(_ segment: Segment) {
        store.update(record.id) { item in
            guard let i = item.segments.firstIndex(where: { $0.id == segment.id }), i + 1 < item.segments.count else { return }
            let next = item.segments[i + 1]
            item.segments[i].end = next.end; item.segments[i].text += " " + next.text
            item.segments[i].originalText += " " + next.originalText; item.segments[i].words += next.words; item.segments[i].speakerLocked = true
            item.segments.remove(at: i + 1)
        }
    }
}

struct SplitView: View {
    @Environment(\.dismiss) var dismiss
    let segment: Segment
    let apply: ([Segment]) -> Void
    @State private var offset = 1.0
    @State private var time = 0.0
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Sprecherwechsel nachtragen").font(.title2.bold())
            Text("Wähle die Textgrenze und den Zeitpunkt des Wechsels. Danach kannst du jedem Abschnitt eine Person zuordnen.").foregroundStyle(.secondary)
            Slider(value: $offset, in: 1...Double(max(2, segment.text.count - 1)), step: 1)
            Text(String(segment.text.prefix(Int(offset))) + "  |  " + String(segment.text.dropFirst(Int(offset)))).padding().background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
            HStack { Text("Zeitpunkt in Sekunden"); TextField("Sekunden", value: $time, format: .number).textFieldStyle(.roundedBorder).frame(width: 120) }
            Text("Abschnitt: \(String(format: "%.2f", segment.start))–\(String(format: "%.2f", segment.end)) s").font(.caption)
            HStack { Spacer(); Button("Abbrechen") { dismiss() }; Button("Teilen") { apply(TranscriptEditor.split(segment, at: time, characterOffset: Int(offset))); dismiss() }.buttonStyle(.borderedProminent).disabled(time <= segment.start || time >= segment.end || Int(offset) >= segment.text.count) }
        }.padding(28).frame(width: 560)
        .onAppear { offset = Double(max(1, segment.text.count / 2)); time = (segment.start + segment.end) / 2 }
    }
}
