import Foundation

public enum SpeakerAlignment {
    /// Input is valid and ordered by start. Bridge only an empty gap bounded by the
    /// same voice; never erase a short detected interjection or a competing overlap.
    /// This is an alignment heuristic, not new acoustic evidence or identity recognition.
    static func bridgingShortGaps(_ turns: [SpeakerTurn]) -> [SpeakerTurn] {
        guard turns.count > 1 else { return turns }
        var result = turns
        var latestEnd = turns[0].end
        for i in 1..<turns.count {
            let previous = turns[i - 1], next = turns[i]
            let gap = next.start - previous.end
            if previous.speakerID == next.speakerID,
               previous.end >= latestEnd, gap > 0, gap <= 3.5 {
                result[i - 1].end = next.start
            }
            latestEnd = max(latestEnd, next.end)
        }
        return result
    }
}
