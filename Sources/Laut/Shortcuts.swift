import AppKit
import Carbon
import ApplicationServices
import AVFoundation
import LautCore
import LautAudio

final class GlobalShortcuts {
    private var refs: [EventHotKeyRef?] = []
    private var handler: EventHandlerRef?
    let action: (UInt32) -> Void
    init(action: @escaping (UInt32) -> Void) {
        self.action = action
        var type = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, event, context -> OSStatus in
            guard let event, let context else { return OSStatus(eventNotHandledErr) }
            var id = EventHotKeyID()
            GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil, MemoryLayout<EventHotKeyID>.size, nil, &id)
            Unmanaged<GlobalShortcuts>.fromOpaque(context).takeUnretainedValue().action(id.id)
            return noErr
        }, 1, &type, Unmanaged.passUnretained(self).toOpaque(), &handler)
        for (key, id) in [(UInt32(kVK_Space), UInt32(1)), (UInt32(kVK_ANSI_R), UInt32(2))] {
            var ref: EventHotKeyRef?
            RegisterEventHotKey(key, UInt32(controlKey | optionKey), EventHotKeyID(signature: 0x53505243, id: id), GetApplicationEventTarget(), 0, &ref)
            refs.append(ref)
        }
    }
    deinit { for ref in refs { if let ref { UnregisterEventHotKey(ref) } }; if let handler { RemoveEventHandler(handler) } }
}

enum TextInsertion {
    struct Anchor {
        let app: NSRunningApplication
        let element: AXUIElement
        let selection: CFRange
        let text: String
        func unchanged() -> Bool {
            guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
                  let current = TextInsertion.focused(app), CFEqual(current, element),
                  let range = TextInsertion.range(element), range.location == selection.location, range.length == selection.length else { return false }
            return TextInsertion.selected(element) == text
        }
    }
    static func range(_ element: AXUIElement) -> CFRange? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, kAXSelectedTextRangeAttribute as CFString, &value) == .success,
              let value, CFGetTypeID(value) == AXValueGetTypeID() else { return nil }
        var range = CFRange()
        guard AXValueGetValue(value as! AXValue, .cfRange, &range) else { return nil }
        return range
    }
    static func anchor(_ app: NSRunningApplication) -> Anchor? {
        guard let element = focused(app), let range = range(element), let text = selected(element) else { return nil }
        return Anchor(app: app, element: element, selection: range, text: text)
    }
    static func focused(_ app: NSRunningApplication) -> AXUIElement? {
        let root = AXUIElementCreateApplication(app.processIdentifier)
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(root, kAXFocusedUIElementAttribute as CFString, &value) == .success,
              let value, CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
        return (value as! AXUIElement)
    }
    static func selected(_ element: AXUIElement) -> String? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, kAXSelectedTextAttribute as CFString, &value) == .success else { return nil }
        return value as? String
    }
    static func permitted() -> Bool {
        AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary)
    }
    /// Writes to the accessibility element directly: no clipboard or Universal Clipboard.
    @discardableResult static func insert(_ text: String, into app: NSRunningApplication, anchor: Anchor?) -> Bool {
        guard let anchor, anchor.unchanged() else { return false }
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
              let focused = focused(app) else { return false }
        return AXUIElementSetAttributeValue(focused, kAXSelectedTextAttribute as CFString, text as CFString) == .success
    }
}

extension AppStore {
    func toggleRecording(kind: DocumentKind) {
        if isRecording { stopRecording(); return }
        guard !busy else { error = "Bitte den laufenden Vorgang zuerst beenden."; return }
        if kind == .dictation, !TextInsertion.permitted() {
            error = "Aktiviere Laut unter Systemeinstellungen → Datenschutz & Sicherheit → Bedienungshilfen. Danach kannst du Text direkt einfügen."; return
        }
        let target = NSWorkspace.shared.frontmostApplication
        let anchor = target.flatMap { TextInsertion.anchor($0) }
        busy = true
        Task {
            defer { busy = false }
            let allowed = await AVCaptureDevice.requestAccess(for: .audio)
            guard allowed else { error = "Mikrofonzugriff fehlt. Er lässt sich in den Systemeinstellungen erlauben."; return }
            do {
                let temp = library.root.appendingPathComponent("capture-\(UUID().uuidString).m4a")
                let recorder = try AVAudioRecorder(url: temp, settings: [AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 48000, AVNumberOfChannelsKey: 1, AVEncoderBitRateKey: 128000])
                guard recorder.record() else { throw LocalEngineError("Mikrofonaufnahme konnte nicht gestartet werden.") }
                self.recorder = recorder; captureURL = temp; capturedKind = kind; captureTarget = target; captureAnchor = anchor
                isRecording = true; status = kind == .dictation ? "Diktat läuft · ⌃⌥Leertaste beendet die Aufnahme" : "Mikrofonaufnahme läuft"
            } catch { self.error = error.localizedDescription }
        }
    }
    func stopRecording() {
        if meeting != nil { stopMeeting(); return }
        recorder?.stop(); recorder = nil; isRecording = false
        guard let url = captureURL else { return }; captureURL = nil
        do {
            var item = try library.importFile(url)
            item.kind = capturedKind; item.title = (capturedKind == .dictation ? "Diktat · " : "Aufnahme · ") + Date().formatted(date: .abbreviated, time: .shortened)
            try library.save(item); recordings.insert(item, at: 0); selected = item.id; section = "library"
            try FileManager.default.removeItem(at: url)
            transcribe(item.id, pasteTo: capturedKind == .dictation ? captureTarget : nil, anchor: captureAnchor)
        } catch { self.error = "Aufnahme bleibt unter \(url.path) erhalten. \(error.localizedDescription)" }
    }
    func startMeeting() {
        guard !busy, !isRecording else { return }
        busy = true; status = "Meetingaufnahme vorbereiten …"
        Task {
            defer { busy = false }
            guard await AVCaptureDevice.requestAccess(for: .audio) else { error = "Bitte Mikrofonzugriff in den Systemeinstellungen erlauben."; return }
            let folder = library.root.appendingPathComponent("meeting-\(UUID().uuidString)")
            do {
                let capture = MeetingCapture(directory: folder)
                try await capture.start()
                meeting = capture; meetingDirectory = folder; isRecording = true
                status = "Meetingaufnahme läuft · Mikrofon + Systemaudio"
            } catch { self.error = "Meetingaufnahme nicht gestartet: \(error.localizedDescription)"; status = "Aufnahme nicht gestartet" }
        }
    }
    func stopMeeting() {
        guard let capture = meeting else { return }
        meeting = nil; isRecording = false; busy = true; status = "Meetingspuren lokal verbinden …"
        Task {
            do {
                let audio = try await capture.stop()
                var item = try library.importFile(audio)
                item.kind = .meeting; item.title = "Meeting · " + Date().formatted(date: .abbreviated, time: .shortened)
                try library.save(item); recordings.insert(item, at: 0); selected = item.id; section = "library"
                if let folder = meetingDirectory { try? FileManager.default.removeItem(at: folder) }; meetingDirectory = nil
                busy = false; transcribe(item.id)
            } catch {
                busy = false; self.error = "Aufnahme nicht abgeschlossen. Vorhandene Audiospuren bleiben unter \(meetingDirectory?.path ?? "Bibliothek") erhalten. \(error.localizedDescription)"
                status = "Meetingaufnahme prüfen"
            }
        }
    }
    func refineSelection() {
        guard !busy, !isRecording else { return }
        guard TextInsertion.permitted(), let app = NSWorkspace.shared.frontmostApplication,
              let anchor = TextInsertion.anchor(app), !anchor.text.isEmpty else {
            error = "Markiere zuerst Text in einer App, die Bedienungshilfen unterstützt. Für diese Funktion ist die entsprechende macOS-Berechtigung erforderlich."; return
        }
        let text = anchor.text
        perform("Auswahl lokal mit deinen Anweisungen bearbeiten …") {
            let result = try await self.engine.refine(text, settings: self.settings)
            try Task.checkCancellation()
            var note = Recording(title: "KI-Text · " + Date().formatted(), kind: .note); note.notes = text; note.refinedText = result
            try self.library.save(note); self.recordings.insert(note, at: 0)
            if anchor.unchanged(),
               AXUIElementSetAttributeValue(anchor.element, kAXSelectedTextAttribute as CFString, result as CFString) == .success { return }
            self.selected = note.id; self.section = "library"
            self.error = "Die Textauswahl hat sich geändert oder die App erlaubt kein Einfügen. Das Ergebnis ist als Notiz gespeichert."
        }
    }
}
