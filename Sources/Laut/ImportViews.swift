import SwiftUI
import AppKit
import LautCore
import LautAudio

struct ImportJobsView: View {
    @EnvironmentObject var store: AppStore
    @State private var link = ""
    @State private var autoTranscribe = true
    @State private var removal: UUID?
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Quellen & Aufträge").font(.largeTitle.bold())
            Text("YouTube oder einen direkten Audio-Link einfügen. Audio, Transkript und Quellenangaben werden lokal gespeichert.").foregroundStyle(.secondary)
            HStack {
                TextField("https://www.youtube.com/watch?v=… oder https://…/audio.mp3", text: $link)
                    .textFieldStyle(.roundedBorder).onSubmit { submit() }
                Button("Laden") { submit() }.buttonStyle(.borderedProminent).disabled(link.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || store.queueError != nil)
            }
            Toggle("Nach dem Laden automatisch transkribieren", isOn: $autoTranscribe)
            Text("Nur Audio · bis 6 Stunden / 2 GiB je Link · keine laufenden Livestreams oder Playlists. Die Quelle sieht den Abruf und deine IP-Adresse. Kein Login, keine Browser-Cookies. Transkription bleibt offline.").font(.caption).foregroundStyle(.secondary)
            let missing = DownloadTools(settings: store.settings).missing
            if !missing.isEmpty {
                HStack { Text("Download-Werkzeuge fehlen: " + missing.joined(separator: ", ")).font(.caption).foregroundStyle(.orange); Button("Einrichten") { store.section = "settings" } }
            }
            if let error = store.queueError { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Text("\(store.waitingJobs) wartend").font(.subheadline)
                Spacer()
                Button("Dateien hinzufügen …") { store.chooseFiles() }
                Button("Alle pausieren") { store.pauseImportQueue() }.disabled(!store.importJobs.contains { $0.stage.unfinished })
            }
            ScrollView {
                LazyVStack(spacing: 10) {
                    ForEach(store.importJobs.reversed()) { job in
                        VStack(alignment: .leading, spacing: 7) {
                            HStack {
                                if store.activeImportID == job.id { ProgressView().controlSize(.small) }
                                Text(job.title).font(.headline).lineLimit(2)
                                Spacer()
                                Text(job.stage.label).font(.caption).foregroundStyle(job.stage == .failed ? .orange : .secondary)
                            }
                            if let message = job.message { Text(message).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                            if store.activeImportID == job.id {
                                Text(store.importStatus.isEmpty ? store.status : store.importStatus).font(.caption).foregroundStyle(.secondary)
                            }
                            HStack {
                                Text(job.createdAt.formatted(date: .abbreviated, time: .shortened)).font(.caption).foregroundStyle(.secondary)
                                Spacer()
                                if store.recordings.contains(where: { $0.id == job.recordingID }) {
                                    Button("Öffnen") { store.selected = job.recordingID; store.section = "library" }
                                }
                                if job.stage == .paused || job.stage == .failed { Button("Fortsetzen") { store.resumeImport(job.id) }.disabled(store.queueError != nil) }
                                Button("Entfernen …", role: .destructive) { removal = job.id }.disabled(store.activeImportID == job.id || store.queueError != nil)
                            }
                        }.padding(14).background(Color.secondary.opacity(0.07), in: RoundedRectangle(cornerRadius: 12))
                    }
                }
            }
            Text("Nach einem Neustart bleiben offene Aufträge pausiert. Fortsetzen nutzt bereits übernommenes Audio. Unvollständige Downloads beginnen erneut; Transkription verwendet das aktuell gewählte Modell.").font(.caption).foregroundStyle(.secondary)
        }.padding(28)
        .confirmationDialog("Auftrag und verbliebene Download-Zwischendateien entfernen? Bereits übernommene Bibliothekseinträge bleiben erhalten.", isPresented: Binding(get: { removal != nil }, set: { if !$0 { removal = nil } })) {
            Button("Auftrag entfernen", role: .destructive) { if let removal { store.removeImport(removal) }; removal = nil }
        }
    }
    func submit() {
        guard !link.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        let previousCount = store.importJobs.count
        store.addLink(link, autoTranscribe: autoTranscribe)
        if store.importJobs.count > previousCount { link = "" }
    }
}

struct LinkImportSettings: View {
    @EnvironmentObject var store: AppStore
    var body: some View {
        GroupBox("Linkimport") {
            VStack(alignment: .leading, spacing: 10) {
                Text("YouTube verwendet yt-dlp, passende EJS-Komponenten und Deno (ab 2.3). FFmpeg/FFprobe prüfen und konvertieren Audio. Werkzeuge werden nur für ausdrücklich gestartete Linkimporte verwendet.").font(.caption).foregroundStyle(.secondary)
                ForEach(["yt-dlp", "deno", "ffmpeg", "ffprobe"], id: \.self) { name in
                    HStack {
                        Text(name).frame(width: 75, alignment: .leading)
                        Text(DownloadTools(settings: store.settings).path(name) ?? "Nicht gefunden").font(.caption.monospaced()).lineLimit(2).textSelection(.enabled)
                        Spacer()
                        Button("Wählen …") {
                            let panel = NSOpenPanel(); panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
                            if panel.runModal() == .OK, let url = panel.url {
                                var paths = store.settings.downloadToolPaths ?? [:]; paths[name] = url.path
                                store.settings.downloadToolPaths = paths; store.saveSettings()
                            }
                        }
                        Button("Automatisch") { store.settings.downloadToolPaths?.removeValue(forKey: name); store.saveSettings() }
                    }
                }
                Text("Entwicklerversion: scripts/setup-downloads.sh installiert yt-dlp samt EJS separat. Deno und FFmpeg müssen zusätzlich vorhanden sein. Es werden keine Werkzeuge beim Einfügen eines Links nachgeladen.").font(.caption).foregroundStyle(.secondary)
            }.padding(12)
        }
    }
}

struct RecordingSourceView: View {
    @EnvironmentObject var store: AppStore
    let record: Recording
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            if let source = record.source {
                VStack(alignment: .leading, spacing: 3) {
                    Text([source.provider, source.publisher, source.publishedOn.map { "Veröffentlicht: " + $0 }, source.language.map { "Sprache: " + $0 }].compactMap { $0 }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                    if let url = URL(string: source.url), url.scheme == "https" { Link("Originalquelle öffnen", destination: url).font(.caption) }
                }
            }
            Spacer()
            if let audio = store.library.audioURL(record) {
                Button("Audio im Finder") { NSWorkspace.shared.activateFileViewerSelecting([audio]) }.font(.caption)
                Button("Audio extern öffnen") { NSWorkspace.shared.open(audio) }.font(.caption)
            }
        }
    }
}
