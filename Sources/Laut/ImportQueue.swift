import Foundation
import LautCore
import LautAudio

extension AppStore {
    var jobStore: ImportJobStore { ImportJobStore(root: library.root) }
    var waitingJobs: Int { importJobs.filter { $0.stage == .queued }.count }
    func saveJobs(_ next: [ImportJob]) throws {
        guard queueError == nil else { throw ImportJobError(queueError!) }
        do { try jobStore.save(next); importJobs = next }
        catch { queueError = "Aufträge konnten nicht gespeichert werden: " + error.localizedDescription; throw error }
    }
    func changeJob(_ id: UUID, _ apply: (inout ImportJob) -> Void) throws {
        var next = importJobs
        guard let i = next.firstIndex(where: { $0.id == id }) else { throw ImportJobError("Auftrag nicht mehr vorhanden.") }
        apply(&next[i]); try saveJobs(next)
    }
    func addLink(_ input: String, autoTranscribe: Bool) {
        do {
            let link = try SourceLink.parse(input)
            if let existing = importJobs.first(where: { $0.sourceKey == link.key }) {
                if let record = recordings.first(where: { $0.id == existing.recordingID }) { selected = record.id }
                section = "jobs"; error = "Dieser Link ist bereits in der Auftragsliste. Dort kannst du ihn öffnen oder fortsetzen."; return
            }
            let job = ImportJob(kind: .link, input: link.url.absoluteString, sourceKey: link.key,
                                title: link.provider + " · " + (link.url.host ?? "Audio"), autoTranscribe: autoTranscribe)
            try saveJobs(importJobs + [job]); section = "jobs"; runImportQueue()
        } catch { self.error = error.localizedDescription }
    }
    func addFileJobs(_ urls: [URL]) {
        do {
            var next = importJobs
            for url in urls where url.isFileURL {
                let key = url.standardizedFileURL.path
                guard !next.contains(where: { $0.sourceKey == key && $0.stage != .complete && $0.stage != .failed }) else { continue }
                next.append(ImportJob(kind: .file, input: url.path, sourceKey: key, title: url.deletingPathExtension().lastPathComponent, autoTranscribe: false))
            }
            try saveJobs(next); section = "jobs"; runImportQueue()
        } catch { self.error = error.localizedDescription }
    }
    func addTranscriptionJobs(_ ids: [UUID]) {
        do {
            var next = importJobs
            for id in ids {
                guard let record = recordings.first(where: { $0.id == id }), record.segments.isEmpty else { continue }
                if let i = next.firstIndex(where: { $0.recordingID == id }) {
                    guard next[i].stage != .transcribing, next[i].id != activeImportID else { continue }
                    next[i].autoTranscribe = true; next[i].stage = .queued; next[i].message = nil
                } else {
                    var job = ImportJob(kind: .recording, input: "", sourceKey: "recording:" + id.uuidString, title: record.title)
                    job.recordingID = id; next.append(job)
                }
            }
            try saveJobs(next); section = "jobs"; runImportQueue()
        } catch { self.error = error.localizedDescription }
    }
    func resumeImport(_ id: UUID) {
        guard activeImportID != id else { return }
        do { try changeJob(id) { $0.stage = .queued; $0.message = nil }; runImportQueue() }
        catch { self.error = error.localizedDescription }
    }
    func pauseImportQueue() {
        do {
            var next = importJobs
            for i in next.indices where next[i].stage.unfinished { next[i].stage = .paused; next[i].message = "Pausiert. Fertiges Audio bleibt erhalten." }
            try saveJobs(next)
        } catch { self.error = error.localizedDescription }
        if activeImportID != nil { importJobTask?.cancel(); task?.cancel() }
    }
    func removeImport(_ id: UUID) {
        guard activeImportID != id else { return }
        do {
            let recordID = importJobs.first(where: { $0.id == id })?.recordingID
            try saveJobs(importJobs.filter { $0.id != id })
            let folder = jobStore.folder(id)
            if FileManager.default.fileExists(atPath: folder.path) { try FileManager.default.removeItem(at: folder) }
            if let recordID {
                let staging = library.root.appendingPathComponent(".import-staging/" + recordID.uuidString)
                if FileManager.default.fileExists(atPath: staging.path) { try FileManager.default.removeItem(at: staging) }
            }
        } catch { self.error = error.localizedDescription }
    }
    func runImportQueue() {
        guard importJobTask == nil, queueError == nil, !busy, !isRecording,
              let job = importJobs.first(where: { $0.stage == .queued }) else { return }
        activeImportID = job.id; busy = true
        importJobTask = Task {
            defer {
                importing = false; importStatus = ""; activeImportID = nil; importJobTask = nil; busy = false
                runImportQueue()
            }
            do {
                var item = recordings.first { $0.id == job.recordingID }
                if item == nil {
                    guard job.kind != .recording else { throw ImportJobError("Der Bibliothekseintrag wurde gelöscht.") }
                    let audio: URL, title: String, metadata: SourceMetadata?
                    importing = true
                    if job.kind == .link {
                        try changeJob(job.id) { $0.stage = .downloading; $0.message = nil }
                        let link = try SourceLink.parse(job.input)
                        let resource = Bundle.main.resourceURL ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("Resources")
                        let downloaded = try await mediaDownloader.download(link, folder: jobStore.folder(job.id),
                            script: resource.appendingPathComponent("media_download.py"), settings: settings) { [weak self] value in
                                Task { @MainActor in if self?.activeImportID == job.id { self?.importStatus = value; self?.status = value } }
                            }
                        audio = jobStore.folder(job.id).appendingPathComponent(downloaded.filename)
                        title = downloaded.title; metadata = downloaded.source
                    } else { audio = URL(fileURLWithPath: job.input); title = job.title; metadata = nil }
                    try Task.checkCancellation()
                    try changeJob(job.id) { $0.stage = .importing; $0.title = title }
                    let imported = try await importer.importFile(audio, root: library.root, title: title, recordingID: job.recordingID, metadata: metadata) { [weak self] fraction in
                        Task { @MainActor in if self?.activeImportID == job.id { self?.importStatus = "Audio übernehmen · \(Int(fraction * 100)) %" } }
                    }
                    // Publish even if cancellation arrived just after the atomic import commit.
                    recordings.insert(imported, at: 0); selected = imported.id; item = imported
                }
                importing = false; importStatus = ""
                try Task.checkCancellation()
                guard let item else { throw ImportJobError("Import nicht abgeschlossen.") }
                if job.autoTranscribe && item.segments.isEmpty {
                    try changeJob(job.id) { $0.stage = .transcribing }
                    busy = false
                    transcribe(item.id)
                    guard let running = task else { throw ImportJobError("Transkription konnte nicht gestartet werden.") }
                    await running.value
                    try Task.checkCancellation()
                    guard let finished = recordings.first(where: { $0.id == item.id }), finished.state == .complete else {
                        throw ImportJobError(recordings.first(where: { $0.id == item.id })?.error ?? "Transkription nicht abgeschlossen.")
                    }
                }
                let warning = recordings.first(where: { $0.id == item.id })?.error
                try changeJob(job.id) { $0.stage = .complete; $0.message = warning; $0.title = item.title }
                // The library copy is durable now; remove only redundant download data.
                let folder = jobStore.folder(job.id)
                if FileManager.default.fileExists(atPath: folder.path) { try? FileManager.default.removeItem(at: folder) }
                status = warning == nil ? "Auftrag abgeschlossen · lokal gespeichert" : "Transkript gespeichert · Hinweis im Auftrag"
            } catch {
                let paused = Task.isCancelled
                do { try changeJob(job.id) { $0.stage = paused ? .paused : .failed; $0.message = paused ? "Pausiert. Bereits gespeichertes Audio und Text bleiben erhalten." : error.localizedDescription } }
                catch { self.error = "Auftragsstatus nicht gespeichert: " + error.localizedDescription }
                status = paused ? "Auftrag pausiert" : "Auftrag fehlgeschlagen · siehe Aufträge"
            }
        }
    }
}
