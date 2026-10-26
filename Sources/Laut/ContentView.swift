import SwiftUI
import AppKit
import UniformTypeIdentifiers
import LautCore
import LautAudio

struct ContentView: View {
    @EnvironmentObject var store: AppStore
    @State private var dropping = false
    var body: some View {
        NavigationSplitView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 10) {
                    Image(nsImage: BrandAssets.appIcon).resizable().scaledToFit().frame(width: 38, height: 38).accessibilityHidden(true)
                    VStack(alignment: .leading) { Text("Laut").font(.title3.bold()); Text(L("Deine Stimme. Dein Mac.")).font(.caption).foregroundStyle(.secondary) }
                }.padding(.top, 16)
                VStack(spacing: 6) {
                    nav(L("Bibliothek"), symbol: "rectangle.stack", key: "library")
                    nav(L("Quellen & Aufträge"), symbol: "tray.and.arrow.down", key: "jobs")
                    nav(L("Suche"), symbol: "magnifyingglass", key: "search")
                    nav(L("Wörterbuch"), symbol: "text.book.closed", key: "vocabulary")
                    nav(L("Modelle"), symbol: "cpu", key: "models")
                    nav(L("Einstellungen"), symbol: "slider.horizontal.3", key: "settings")
                }
                Divider()
                HStack { Text(L("VERLAUF")).font(.caption.weight(.semibold)).foregroundStyle(.secondary); Spacer(); Button { store.newNote() } label: { Image(systemName: "square.and.pencil") }.buttonStyle(.plain).help(L("Schnellnotiz")) }
                TextField(L("Suchen …"), text: $store.search).textFieldStyle(.roundedBorder)
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
                                        Text(record.createdAt.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened).locale(LautLocalization.locale))).font(.caption2).foregroundStyle(.secondary)
                                    }; Spacer(minLength: 0)
                                    if record.state == .failed { Image(systemName: "exclamationmark.circle").foregroundStyle(.orange) }
                                }.padding(10).frame(maxWidth: .infinity, alignment: .leading)
                                    .background(store.selected == record.id && store.section == "library" ? Color.teal.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 9))
                            }.buttonStyle(.plain)
                        }
                    }
                }
                Label(L("Kein Konto. Keine Cloud."), systemImage: "lock.shield").font(.caption).foregroundStyle(.secondary)
            }.padding(.horizontal, 15).padding(.bottom, 16)
            .navigationSplitViewColumnWidth(min: 230, ideal: 260, max: 340)
        } detail: {
            VStack(spacing: 0) {
                Group {
                    switch store.section {
                    case "jobs": ImportJobsView()
                    case "search": SearchView()
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
                    VStack(alignment: .leading, spacing: 3) {
                        Text(store.downloadProgress.isEmpty ? store.status : store.status + " · " + store.downloadProgress).font(.caption).lineLimit(2)
                        Text(store.modelStatus).font(.caption2).foregroundStyle(store.modelReady ? .teal : .secondary)
                    }
                    Spacer()
                    if store.waitingJobs > 0 { Text(L("\(store.waitingJobs) Aufträge warten")).font(.caption).foregroundStyle(.secondary) }
                    if store.busy { Button(L("Abbrechen")) { store.cancel() }.controlSize(.small) }
                    if store.isRecording { Button(L("Aufnahme beenden")) { store.stopRecording() }.tint(.red) }
                }.padding(12).background(.bar)
                if store.importing {
                    HStack { ProgressView().controlSize(.small); Text(store.importStatus).font(.caption).lineLimit(1); Spacer(); Button(L("Import abbrechen")) { store.cancelImport() }.controlSize(.small) }.padding(10)
                }
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
        .disabled(store.maintenanceRunning)
        .overlay {
            if store.maintenanceRunning {
                VStack(spacing: 12) {
                    ProgressView()
                    Text(store.storageStatus).multilineTextAlignment(.center)
                    Button(L("Abbrechen")) { store.cancel() }
                }.padding(28).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
            }
        }
        .onChange(of: store.settings.engine) { store.saveSettings() }
        .toolbar {
            ToolbarItemGroup {
                Button { store.chooseFiles() } label: { Label(L("Importieren"), systemImage: "plus") }
                Button { store.section = "jobs" } label: { Label("Link", systemImage: "link") }
                Button { store.toggleRecording(kind: .meeting) } label: { Label(store.isRecording ? L("Stoppen") : L("Mikrofon"), systemImage: store.isRecording ? "stop.circle.fill" : "mic") }.disabled(store.busy)
                Button { store.startMeeting() } label: { Label("Meeting", systemImage: "person.2.wave.2") }.disabled(store.busy || store.isRecording).help(L("Mikrofon und Systemaudio aufnehmen"))
            }
        }
        .alert(L("Hinweis"), isPresented: Binding(get: { store.error != nil }, set: { if !$0 { store.error = nil } })) { Button("OK") { store.error = nil } } message: { Text(store.error ?? "") }
    }
    func nav(_ title: String, symbol: String, key: String) -> some View {
        Button { store.section = key } label: {
            Label(title, systemImage: symbol).frame(maxWidth: .infinity, alignment: .leading).padding(9)
                .background(store.section == key ? Color.teal.opacity(0.1) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
        }.buttonStyle(.plain)
    }
    var welcome: some View {
        VStack(alignment: .leading, spacing: 25) {
            Label(L("LOKAL AUF DEINEM MAC"), systemImage: "lock.shield").font(.caption.weight(.semibold)).foregroundStyle(.teal)
            Text(L("Aus Gesprochenem\nwird dein Text.")).font(.system(size: 42, weight: .semibold, design: .rounded))
            Text(L("Audio oder Video hier ablegen. Transkribieren, Sprecher zuordnen und Gedanken festhalten — alles auf deinem Gerät.")).font(.title3).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Button { store.chooseFiles() } label: { Label(L("Dateien auswählen"), systemImage: "arrow.down.doc").padding(7) }.buttonStyle(.borderedProminent)
            HStack(alignment: .top, spacing: 30) {
                feature("01", L("Datei importieren"), L("Die Quelldatei bleibt erhalten."))
                feature("02", L("Lokal transkribieren"), L("Du entscheidest über das Modell."))
                feature("03", L("Sprecher korrigieren"), L("Namen und Zuordnung bearbeiten."))
            }.padding(.top, 26)
            Text(L("Diktat: ⌃⌥Leertaste    ·    Text-KI: ⌃⌥R")).font(.caption).foregroundStyle(.secondary)
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
    @State private var textTemplate: TextTemplate = .custom
    @State private var speakerCount = 0
    @State private var splitSegment: Segment?
    @State private var confirmDelete = false
    @State private var confirmAudioDelete = false
    @State private var transcriptSearch = ""
    @State private var followPlayback = true
    @State private var jumpTime = ""
    @State private var editingTranscript = false
    @State private var transcriptCopyID: UUID?
    @FocusState private var focusedSegment: UUID?
    var activeSegmentID: UUID? { store.playbackIsLoaded(record.id) ? TranscriptNavigation.activeSegment(at: store.playerTime, in: record.segments) : nil }
    var visibleSegments: [Segment] { transcriptSearch.isEmpty ? record.segments : record.segments.filter { ($0.text + " " + record.speakerName($0.speakerID)).localizedCaseInsensitiveContains(transcriptSearch) } }
    var visibleParagraphs: [TranscriptParagraph] {
        TranscriptLayout.paragraphs(ConferenceReview.includedSegments(record), boundaries: record.conference?.chapters.map(\.start) ?? []).filter { transcriptSearch.isEmpty || ($0.text + " " + record.speakerName($0.speakerID)).localizedCaseInsensitiveContains(transcriptSearch) }
    }
    private var timingSummary: String {
        let values: [(String, Double?)] = [(L("Gesamt"), record.processingSeconds), (L("Transkription"), record.transcriptionSeconds), (L("Modellladen"), record.modelLoadSeconds), (L("Sprecher"), record.diarizationSeconds), ("Audio", record.audioPreparationSeconds)]
        return values.compactMap { label, seconds in seconds.map { "\(label): \(String(format: "%.2f", $0)) s" } }.joined(separator: " · ")
    }
    func binding<T>(_ key: WritableKeyPath<Recording, T>) -> Binding<T> {
        Binding(get: { store.recordings.first { $0.id == record.id }?[keyPath: key] ?? record[keyPath: key] }, set: { value in store.edit(record.id, label: L("Text bearbeiten"), key: String(describing: key)) { $0[keyPath: key] = value } })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 8) {
                    TextField(L("Titel"), text: binding(\.title)).textFieldStyle(.plain).font(.title.bold())
                    Text(record.createdAt.formatted(Date.FormatStyle(date: .long, time: .shortened).locale(LautLocalization.locale)) + (record.duration > 0 ? "  ·  " + Exporter.timestamp(record.duration) : "") + (record.engine.isEmpty ? "" : "  ·  " + record.engine)).font(.caption).foregroundStyle(.secondary)
                    if record.processingSeconds != nil {
                        Text(timingSummary)
                            .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
                Spacer()
                Button { store.undoEdit() } label: { Image(systemName: "arrow.uturn.backward") }
                    .disabled(store.busy || store.undoLabel == nil).help(L("Rückgängig: ") + (store.undoLabel ?? L("Keine Änderung"))).accessibilityLabel(L("Rückgängig"))
                Button { store.redoEdit() } label: { Image(systemName: "arrow.uturn.forward") }
                    .disabled(store.busy || store.redoLabel == nil).help(L("Wiederherstellen: ") + (store.redoLabel ?? L("Keine Änderung"))).accessibilityLabel(L("Wiederherstellen"))
                Menu {
                    ForEach(["txt", "md", "srt", "json"], id: \.self) { format in Button(format.uppercased()) { store.export(record, format: format) } }
                    Divider()
                    Button(L("Nur Audiodateien löschen …"), role: .destructive) { confirmAudioDelete = true }.disabled(record.audioFilename == nil || store.busy)
                    Button(L("Eintrag löschen …"), role: .destructive) { confirmDelete = true }.disabled(store.busy)
                } label: { Label(L("Export & mehr"), systemImage: "square.and.arrow.up") }
            }
            RecordingSourceView(record: record)
            if record.kind != .note {
                HStack {
                    Picker(L("Modell"), selection: $store.settings.engine) { ForEach(EngineKind.allCases) { Text($0.label).tag($0) } }.frame(maxWidth: 280).disabled(store.busy || store.isRecording)
                    Button(record.transcriptionProgress != nil ? L("Fortsetzen") : record.segments.isEmpty ? L("Transkribieren") : L("Neue Version")) {
                        if record.segments.isEmpty || record.transcriptionProgress != nil { store.addTranscriptionJobs([record.id]) }
                        else { store.transcribe(record.id) }
                    }.buttonStyle(.borderedProminent).disabled(store.busy || store.isRecording || record.audioFilename == nil)
                    if record.transcriptionProgress != nil { Button(L("Neue Version")) { store.transcribe(record.id, forceNew: true) }.disabled(store.busy || store.isRecording) }
                    if record.segments.isEmpty { Button(L("Alle offenen Dateien")) { store.transcribeAll() }.disabled(store.busy || store.isRecording) }
                    Spacer()
                }
                if store.settings.engine == .phonon && store.settings.language != "en" {
                    HStack(alignment: .top) {
                        Text(L("Phonon ließ im deutschen Gesprächstest ganze Sätze aus. Für deutsche Gespräche empfehlen wir Parakeet v3.")).font(.caption).foregroundStyle(.secondary)
                        Button(L("Modelle ansehen")) { store.section = "models" }.controlSize(.small)
                    }
                }
                HStack {
                    Toggle(L("Sprecher automatisch erkennen"), isOn: Binding(get: { store.settings.speakerDetectionEnabled }, set: { store.settings.speakerDetectionEnabled = $0; store.saveSettings() }))
                        .toggleStyle(.switch).controlSize(.small).disabled(store.busy || store.isRecording)
                    Text(store.settings.speakerDetectionEnabled ? L("Direkt nach der Transkription") : L("Schneller ohne Sprecheranalyse")).font(.caption).foregroundStyle(.secondary)
                    Spacer()
                }
                if record.audioFilename != nil {
                    VStack(spacing: 12) {
                        if let audio = store.library.audioURL(record) {
                            WaveformView(source: audio, cache: store.library.folder(record.id).appendingPathComponent("waveform-v1.json"), position: store.playerTime) { store.seek(record, to: $0) }
                        }
                        HStack(spacing: 12) {
                            Button { store.jumpSegment(record, forward: false) } label: { Image(systemName: "backward.end") }.help(L("Vorheriger Abschnitt"))
                            Button { store.skip(record, by: -5) } label: { Image(systemName: "gobackward.5") }.help(L("5 Sekunden zurück"))
                            Button { store.play(record) } label: { Image(systemName: store.playing ? "pause.fill" : "play.fill") }.buttonStyle(.bordered)
                            Button { store.skip(record, by: 5) } label: { Image(systemName: "goforward.5") }.help(L("5 Sekunden vor"))
                            Button { store.jumpSegment(record, forward: true) } label: { Image(systemName: "forward.end") }.help(L("Nächster Abschnitt"))
                            Slider(value: Binding(get: { min(store.playerTime, max(record.duration, store.playbackDuration, 1)) }, set: { store.seek(record, to: $0) }), in: 0...max(record.duration, store.playbackDuration, 1))
                            Text(Exporter.timestamp(store.playerTime)).font(.caption.monospacedDigit())
                        }
                        HStack {
                            Picker(L("Tempo"), selection: Binding(get: { store.playbackRate }, set: { store.setPlaybackRate($0) })) { ForEach([Float(0.75), 1, 1.25, 1.5, 2], id: \.self) { Text(String(format: "%g×", $0)).tag($0) } }.frame(width: 155)
                            Toggle(L("Text folgt Wiedergabe"), isOn: $followPlayback).font(.caption)
                            Spacer()
                            TextField("mm:ss", text: $jumpTime).textFieldStyle(.roundedBorder).frame(width: 75).onSubmit(jumpToTime)
                            Button(L("Springen"), action: jumpToTime).disabled(TranscriptNavigation.parseTime(jumpTime) == nil)
                        }
                    }.padding(12).background(Color.teal.opacity(0.06), in: RoundedRectangle(cornerRadius: 10))
                }
            }
            if let progress = record.transcriptionProgress {
                ProgressView(L("Zwischenstand · \(Int(progress * 100)) % der Aufnahme verarbeitet"), value: progress)
            }
            if let chapters = record.conference?.chapters, !chapters.isEmpty {
                Menu(L("Kapitel · \(chapters.count)")) {
                    ForEach(chapters) { chapter in
                        Button(Exporter.timestamp(chapter.start) + " · " + chapter.title) { store.play(record, at: chapter.start) }
                    }
                }.fixedSize()
            }
            Picker(L("Ansicht"), selection: $tab) {
                if record.kind != .note { Text(L("Transkript")).tag(0); Text(L("Sprecher")).tag(1) }
                Text(L("Notizen")).tag(2); Text(L("Text-KI")).tag(3); Text(L("Konferenz")).tag(4)
            }.pickerStyle(.segmented)
            if let error = record.error { Label(error, systemImage: "exclamationmark.triangle").font(.callout).foregroundStyle(.orange).textSelection(.enabled) }
            Group {
                switch tab {
                case 4: ConferenceView(record: record)
                case 1: speakers
                case 2: TextEditor(text: binding(\.notes)).font(.body).padding(8).overlay(alignment: .topLeading) { if record.notes.isEmpty { Text(L("Gedanken, Stichpunkte, Aufgaben …")).foregroundStyle(.tertiary).padding(13).allowsHitTesting(false) } }
                case 3:
                    VStack(alignment: .leading, spacing: 12) {
                        Text(L("Wähle eine Vorlage oder deine eigenen Anweisungen. Transkript und Notizen dienen als Grundlage; das Ergebnis bleibt getrennt vom Original.")).font(.callout).foregroundStyle(.secondary)
                        Picker(L("Vorlage"), selection: $textTemplate) { ForEach(TextTemplate.allCases) { Text($0.label).tag($0) } }.frame(maxWidth: 400)
                        Button(L("Lokal bearbeiten")) { store.refine(record.id, template: textTemplate) }.disabled(store.busy)
                        TextEditor(text: binding(\.refinedText)).font(.body)
                    }
                default: transcript
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
        }.padding(24)
        .onAppear { applySearchDestination(); if record.kind == .note { tab = 2 } }
        .onChange(of: store.searchDestination?.id) { applySearchDestination() }
        .sheet(item: $splitSegment) { segment in SplitView(segment: segment) { pieces in
            store.edit(record.id, label: L("Abschnitt teilen")) { item in if let index = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments.replaceSubrange(index...index, with: pieces) } }
        } }
        .confirmationDialog(L("Eintrag und zugehörige lokale Dateien endgültig löschen?"), isPresented: $confirmDelete) { Button(L("Löschen"), role: .destructive) { store.delete(record) } }
        .confirmationDialog(L("Audiodateien löschen? Text und Notizen bleiben erhalten. Wiedergabe und neue Sprecheranalyse sind danach nicht mehr möglich."), isPresented: $confirmAudioDelete) { Button(L("Audio löschen"), role: .destructive) { store.removeAudio(record.id) } }
    }
    func applySearchDestination() {
        guard let hit = store.searchDestination, hit.passage.recordingID == record.id else { return }
        tab = hit.passage.documentID != nil ? 4 : hit.passage.source == "Notizen" ? 2 : hit.passage.source == "Text-KI" ? 3 : 0
    }
    var transcript: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Button(action: copyTranscript) {
                    Label(L("Gesamten Text kopieren"), systemImage: "doc.on.doc")
                }
                .disabled(record.segments.isEmpty)
                .help(L("Kopiert die gesamte Lesefassung mit Absätzen und Korrekturen, ohne Zeitmarken. Die Suche schränkt die Kopie nicht ein; bestätigte Ausschlüsse bleiben ausgelassen."))
                if transcriptCopyID != nil {
                    Label(L("Kopiert"), systemImage: "checkmark").font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .task(id: transcriptCopyID) {
                guard transcriptCopyID != nil else { return }
                do { try await Task.sleep(for: .seconds(2)) } catch { return }
                transcriptCopyID = nil
            }
            .onChange(of: record.id) { transcriptCopyID = nil }
            HStack {
                TextField(L("Im Transkript oder nach Sprechern suchen …"), text: $transcriptSearch).textFieldStyle(.roundedBorder)
                if !transcriptSearch.isEmpty { Text(L("\(editingTranscript ? visibleSegments.count : visibleParagraphs.count) Treffer")).font(.caption); Button(L("Leeren")) { transcriptSearch = "" } }
                Toggle(L("Ausschlüsse überspringen"), isOn: $store.skipExcludedRanges).toggleStyle(.checkbox)
                Picker(L("Darstellung"), selection: $editingTranscript) { Text(L("Lesen")).tag(false); Text(L("Bearbeiten")).tag(true) }.pickerStyle(.segmented).frame(width: 190).disabled(record.transcriptionProgress != nil)
            }
            ScrollViewReader { proxy in
                ScrollView {
                    if record.segments.isEmpty {
                        ContentUnavailableView(L("Bereit für dein Transkript"), systemImage: "text.alignleft", description: Text(L("Wähle ein installiertes Modell und starte die lokale Transkription.")))
                    }
                    LazyVStack(alignment: .leading, spacing: 16) {
                        if !editingTranscript || record.transcriptionProgress != nil {
                            ForEach(visibleParagraphs) { paragraph in
                                if let chapter = record.conference?.chapters.last(where: { $0.start <= paragraph.start }),
                                   visibleParagraphs.first(where: { $0.start >= chapter.start })?.id == paragraph.id {
                                    Text(chapter.title).font(.title3.bold()).padding(.top, 8)
                                }
                                TranscriptReadingView(paragraph: paragraph, speaker: record.speakers.isEmpty ? "" : record.speakerName(paragraph.speakerID), active: paragraph.segments.contains(where: { $0.id == activeSegmentID }), canSeek: record.audioFilename != nil) { store.seek(record, to: $0) }.id(paragraph.id)
                                ForEach((record.conference?.slides ?? []).filter { $0.confirmed && $0.useful == true && $0.start >= paragraph.start && $0.start < paragraph.end }) { slide in
                                    if let document = record.conference?.documents.first(where: { $0.id == slide.documentID }), let url = ConferenceDocuments.pageURL(document, folder: store.library.folder(record.id)) {
                                        VStack(alignment: .leading) {
                                            Text(document.title + L(" · Seite \(slide.page)")).font(.caption)
                                            PDFPagePreview(url: url, page: slide.page).frame(height: 230)
                                        }.padding(10).background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 10))
                                    }
                                }
                            }
                        } else {
                            ForEach(visibleSegments) { segment in
                                VStack(alignment: .leading, spacing: 9) {
                                    HStack {
                                        Button(Exporter.timestamp(segment.start)) { store.play(record, at: segment.start) }.font(.caption.monospacedDigit()).buttonStyle(.plain).foregroundStyle(.teal)
                                        Picker(L("Sprecher"), selection: Binding(get: { segment.speakerID ?? "" }, set: { value in store.edit(record.id, label: L("Sprecher zuordnen")) { item in if let i = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments[i].speakerID = value.isEmpty ? nil : value; item.segments[i].speakerLocked = true; item.segments[i].allowsParagraphGrouping = nil } } })) {
                                            Text(L("Unzugeordnet")).tag("")
                                            ForEach(record.speakers) { Text($0.name).tag($0.id) }
                                        }.labelsHidden().fixedSize().disabled(store.busy)
                                        if segment.speakerLocked { Image(systemName: "lock.fill").font(.caption2).foregroundStyle(.secondary).help(L("Manuelle Zuordnung bleibt bei neuer Analyse erhalten")) }
                                        Spacer()
                                        Menu {
                                            Button(L("Abschnitt teilen …")) { splitSegment = segment }.disabled(segment.text.count < 2)
                                            Button(L("Mit nächstem Abschnitt verbinden")) { mergeNext(segment) }
                                            Button(L("Automatische Sprecherzuordnung wieder erlauben")) { store.edit(record.id, label: L("Sprecherzuordnung entsperren")) { item in if let i = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments[i].speakerLocked = false } } }
                                        } label: { Image(systemName: "ellipsis") }.menuStyle(.borderlessButton).fixedSize().disabled(store.busy)
                                    }
                                    TextField("Text", text: Binding(get: { segment.text }, set: { text in store.edit(record.id, label: L("Text korrigieren"), key: "segment-" + segment.id.uuidString) { item in if let i = item.segments.firstIndex(where: { $0.id == segment.id }) { item.segments[i].text = text } } }), axis: .vertical)
                                        .textFieldStyle(.plain).font(.system(size: 15)).lineSpacing(5).disabled(store.busy)
                                        .focused($focusedSegment, equals: segment.id)
                                    if segment.text != segment.originalText { Text(L("Bearbeitet")).font(.caption2).foregroundStyle(.secondary).help(L("Original: ") + segment.originalText) }
                                }.padding(16).background(activeSegmentID == segment.id ? Color.teal.opacity(0.12) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 12)).id(segment.id)
                            }
                        }
                    }
                }
                .task(id: store.searchDestination?.id) {
                    guard let hit = store.searchDestination, hit.passage.recordingID == record.id, let time = hit.passage.start else { return }
                    let segment = record.segments.first { $0.id == hit.passage.segmentID } ?? record.segments.first { $0.start == time }
                    let target = editingTranscript ? segment?.id : visibleParagraphs.first { $0.segments.contains { $0.id == segment?.id } }?.id
                    if let target { proxy.scrollTo(target, anchor: .center) }
                }
                .onChange(of: activeSegmentID) { _, id in
                    if followPlayback, store.playing, focusedSegment == nil, let id {
                        let target = editingTranscript ? visibleSegments.first(where: { $0.id == id })?.id : visibleParagraphs.first(where: { $0.segments.contains(where: { $0.id == id }) })?.id
                        if let target { withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(target, anchor: .center) } }
                    }
                }
            }
        }
    }
    private func copyTranscript() {
        let current = store.recordings.first { $0.id == record.id } ?? record
        let paragraphs = TranscriptLayout.paragraphs(ConferenceReview.includedSegments(current), boundaries: current.conference?.chapters.map(\.start) ?? [])
        let text = paragraphs.map(\.text).joined(separator: "\n\n")
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            store.error = L("Die Lesefassung enthält noch keinen Text zum Kopieren.")
            return
        }
        NSPasteboard.general.clearContents()
        guard NSPasteboard.general.setString(text, forType: .string) else {
            transcriptCopyID = nil
            store.error = L("Der Text konnte nicht in die Zwischenablage kopiert werden. Bitte erneut versuchen.")
            return
        }
        transcriptCopyID = UUID()
    }
    func jumpToTime() {
        if let time = TranscriptNavigation.parseTime(jumpTime) { store.seek(record, to: time) }
    }
    var speakers: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text(L("Wer hat was gesagt?")).font(.title2.weight(.semibold))
                Text(L("Die Erkennung unterscheidet Stimmen, erkennt aber keine Namen. Höre eine längere Passage an und benenne die Stimme. Kurze Erkennungslücken werden überbrückt; unsichere Stellen bleiben unzugeordnet. Manuelle Zuordnungen sind vor einer erneuten Analyse geschützt.")).foregroundStyle(.secondary)
                HStack {
                    Picker(L("Anzahl"), selection: $speakerCount) { Text(L("Automatisch")).tag(0); ForEach(1...12, id: \.self) { Text("\($0)").tag($0) } }.frame(width: 200)
                    Button(L("Sprecher erkennen")) { store.diarize(record.id, count: speakerCount == 0 ? nil : speakerCount) }.disabled(store.busy || record.transcriptionProgress != nil || record.segments.isEmpty || record.audioFilename == nil)
                    Button(L("Sprecher hinzufügen")) { store.edit(record.id, label: L("Sprecher hinzufügen")) { $0.speakers.append(Speaker(name: L("Neue Person"))) } }.disabled(store.busy)
                }
                ForEach(record.speakers) { speaker in
                    HStack {
                        Image(systemName: "person.crop.circle.fill").foregroundStyle(.teal).font(.title2)
                        if let sample = record.segments.filter({ $0.speakerID == speaker.id }).max(by: { $0.end - $0.start < $1.end - $1.start }) {
                            Button(L("Anhören"), systemImage: "play.circle") { store.play(record, at: sample.start) }
                                .help(L("Längere Passage dieser Stimme ab \(Exporter.timestamp(sample.start)) abspielen"))
                                .disabled(record.audioFilename == nil)
                        }
                        TextField("Name", text: Binding(get: { speaker.name }, set: { name in store.edit(record.id, label: L("Sprecher benennen"), key: "speaker-" + speaker.id) { item in
                            TranscriptEditor.nameSpeaker(speaker.id, name: name, in: &item)
                        } })).textFieldStyle(.roundedBorder)
                        Text(L("\(record.segments.filter { $0.speakerID == speaker.id }.count) Abschnitte")).font(.caption).foregroundStyle(.secondary)
                        Menu(L("Zusammenführen mit …")) {
                            ForEach(record.speakers.filter { $0.id != speaker.id }) { other in
                                Button(other.name) { store.edit(record.id, label: L("Sprecher zusammenführen")) { item in
                                    for i in item.segments.indices where item.segments[i].speakerID == speaker.id { item.segments[i].speakerID = other.id; item.segments[i].speakerLocked = true; item.segments[i].allowsParagraphGrouping = nil }
                                    item.speakers.removeAll { $0.id == speaker.id }
                                } }
                            }
                        }.fixedSize()
                    }.disabled(store.busy || record.transcriptionProgress != nil)
                }
            }.padding(.vertical, 8)
        }
    }
    func mergeNext(_ segment: Segment) {
        store.edit(record.id, label: L("Abschnitte verbinden")) { item in
            guard let i = item.segments.firstIndex(where: { $0.id == segment.id }), i + 1 < item.segments.count else { return }
            let next = item.segments[i + 1]
            item.segments[i].end = next.end; item.segments[i].text += " " + next.text
            item.segments[i].originalText += " " + next.originalText; item.segments[i].words += next.words; item.segments[i].speakerLocked = true; item.segments[i].allowsParagraphGrouping = nil
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
            Text(L("Sprecherwechsel nachtragen")).font(.title2.bold())
            Text(L("Wähle die Textgrenze und den Zeitpunkt des Wechsels. Danach kannst du jedem Abschnitt eine Person zuordnen.")).foregroundStyle(.secondary)
            Slider(value: $offset, in: 1...Double(max(2, segment.text.count - 1)), step: 1)
            Text(String(segment.text.prefix(Int(offset))) + "  |  " + String(segment.text.dropFirst(Int(offset)))).padding().background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
            HStack { Text(L("Zeitpunkt in Sekunden")); TextField(L("Sekunden"), value: $time, format: .number).textFieldStyle(.roundedBorder).frame(width: 120) }
            Text(L("Abschnitt: \(String(format: "%.2f", segment.start))–\(String(format: "%.2f", segment.end)) s")).font(.caption)
            HStack { Spacer(); Button(L("Abbrechen")) { dismiss() }; Button(L("Teilen")) { apply(TranscriptEditor.split(segment, at: time, characterOffset: Int(offset))); dismiss() }.buttonStyle(.borderedProminent).disabled(time <= segment.start || time >= segment.end || Int(offset) >= segment.text.count) }
        }.padding(28).frame(width: 560)
        .onAppear { offset = Double(max(1, segment.text.count / 2)); time = (segment.start + segment.end) / 2 }
    }
}
