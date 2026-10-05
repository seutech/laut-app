import SwiftUI
import LautCore

struct TranscriptReadingView: View {
    let paragraph: TranscriptParagraph
    let speaker: String
    let active: Bool
    let canSeek: Bool
    let seek: (Double) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                if !speaker.isEmpty { Text(speaker).font(.callout.weight(.semibold)).foregroundStyle(.teal) }
                Button(Exporter.timestamp(paragraph.start)) { seek(paragraph.start) }
                    .buttonStyle(.plain).font(.caption.monospacedDigit()).foregroundStyle(.secondary).disabled(!canSeek)
                Spacer()
            }
            Text(linkedText).font(.system(size: 15)).lineSpacing(7).textSelection(.enabled)
                .tint(.primary)
                .environment(\.openURL, OpenURLAction { url in
                    guard canSeek, url.scheme == "laut", url.host == "seek",
                          let raw = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first(where: { $0.name == "time" })?.value,
                          let time = Double(raw), time.isFinite else { return .discarded }
                    seek(time); return .handled
                })
                .help("Auf ein Wort klicken, um dorthin zu spulen. Bei bearbeitetem Text oder fehlenden Wortzeitmarken wird der Abschnittsanfang verwendet.")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 10).padding(.horizontal, 12)
        .background(active ? Color.teal.opacity(0.07) : .clear, in: RoundedRectangle(cornerRadius: 8))
    }

    private func link(_ time: Double) -> URL? { URL(string: "laut://seek?time=\(max(0, time))") }
    private var linkedText: AttributedString {
        var result = AttributedString()
        for (index, segment) in paragraph.segments.enumerated() {
            if index > 0 { result.append(AttributedString(" ")) }
            var part = AttributedString(segment.text)
            if canSeek {
                part.link = link(segment.start)
                for timed in TranscriptLayout.wordRanges(segment) {
                    guard let range = Range(timed.range, in: segment.text),
                          let lower = AttributedString.Index(range.lowerBound, within: part),
                          let upper = AttributedString.Index(range.upperBound, within: part) else { continue }
                    part[lower..<upper].link = link(timed.time)
                }
            }
            part.foregroundColor = .primary
            result.append(part)
        }
        return result
    }
}
