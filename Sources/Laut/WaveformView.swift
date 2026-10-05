import SwiftUI
import LautCore
import LautAudio

struct WaveformView: View {
    let source: URL
    let cache: URL
    let position: Double
    let seek: (Double) -> Void
    @State private var waveform: WaveformData?
    @State private var message: String?
    @State private var zoom: Double = 1
    @State private var start: Double = 0
    private static let reader = WaveformReader()
    private var span: Double { (waveform?.duration ?? 1) / zoom }

    var body: some View {
        VStack(spacing: 5) {
            if let waveform {
                HStack {
                    Text("Audio").font(.caption.weight(.medium))
                    Text("Klicken oder ziehen zum Spulen").font(.caption2).foregroundStyle(.secondary)
                    Spacer()
                    Button { changeZoom(max(1, zoom / 2)) } label: { Image(systemName: "minus.magnifyingglass") }.disabled(zoom == 1).help("Wellenform verkleinern")
                    Text("\(Int(zoom))×").font(.caption.monospacedDigit()).frame(width: 28)
                    Button { changeZoom(min(16, zoom * 2)) } label: { Image(systemName: "plus.magnifyingglass") }.disabled(zoom == 16).help("Wellenform vergrößern")
                }.buttonStyle(.plain)
                GeometryReader { geometry in
                    Canvas { context, size in
                        let bars = max(1, Int(size.width / 3)), total = waveform.peaks.count
                        let scale = max(Float(0.02), waveform.peaks.max() ?? 1)
                        for bar in 0..<bars {
                            let from = max(0, min(total - 1, Int((start + Double(bar) / Double(bars) * span) / waveform.duration * Double(total))))
                            let to = max(from + 1, min(total, Int((start + Double(bar + 1) / Double(bars) * span) / waveform.duration * Double(total))))
                            let peak = waveform.peaks[from..<to].max() ?? 0
                            let height = max(1.5, CGFloat(peak / scale) * (size.height - 6))
                            let time = start + Double(bar) / Double(bars) * span
                            let rect = CGRect(x: CGFloat(bar) * size.width / CGFloat(bars), y: (size.height - height) / 2, width: 2, height: height)
                            context.fill(Path(roundedRect: rect, cornerRadius: 1), with: .color(time <= position ? .teal : .secondary.opacity(0.45)))
                        }
                        if position >= start, position <= start + span {
                            let x = CGFloat((position - start) / span) * size.width
                            context.fill(Path(CGRect(x: x, y: 0, width: 2, height: size.height)), with: .color(.primary))
                        }
                    }
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                        seek(AudioTimeline.time(at: value.location.x / max(1, geometry.size.width), start: start, span: span, duration: waveform.duration))
                    })
                    .accessibilityElement().accessibilityLabel("Audiowellenform")
                    .accessibilityValue(Exporter.timestamp(position))
                    .accessibilityAdjustableAction { direction in seek(min(waveform.duration, max(0, position + (direction == .increment ? 5 : -5)))) }
                }.frame(height: 58)
                HStack {
                    Text(Exporter.timestamp(start))
                    if zoom > 1 {
                        Slider(value: $start, in: 0...max(0.001, waveform.duration - span)).labelsHidden().accessibilityLabel("Sichtbarer Audioausschnitt")
                    } else { Spacer() }
                    Text(Exporter.timestamp(min(waveform.duration, start + span)))
                }.font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
            } else if let message {
                Label(message, systemImage: "waveform").font(.caption).foregroundStyle(.secondary)
            } else {
                HStack { ProgressView().controlSize(.small); Text("Wellenform wird lokal berechnet …").font(.caption); Spacer() }.frame(height: 58)
            }
        }
        .task(id: source) {
            waveform = nil; message = nil; zoom = 1; start = 0
            do { let loaded = try await Self.reader.load(source, cache: cache); try Task.checkCancellation(); waveform = loaded }
            catch { if !Task.isCancelled { message = "Wellenform nicht verfügbar. Die Zeitleiste bleibt nutzbar." } }
        }
        .onChange(of: position) { _, time in
            guard let waveform, zoom > 1, time < start || time > start + span else { return }
            start = max(0, min(waveform.duration - span, time - span / 2))
        }
    }
    private func changeZoom(_ value: Double) {
        zoom = value
        start = max(0, min((waveform?.duration ?? 1) - span, position - span / 2))
    }
}
