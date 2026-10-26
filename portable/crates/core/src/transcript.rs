use crate::{Recording, Segment};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;
pub fn text_boundaries(text: &str) -> Vec<usize> {
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect()
}

#[derive(Clone, Debug)]
pub struct Paragraph {
    pub segments: Vec<Segment>,
    pub text: String,
}
impl Paragraph {
    pub fn start(&self) -> f64 {
        self.segments[0].start
    }
    pub fn speaker_id(&self) -> Option<&str> {
        self.segments[0].speaker_id.as_deref()
    }
}

pub fn included_segments(r: &Recording) -> Vec<Segment> {
    let ranges: Vec<_> = r
        .conference
        .as_ref()
        .and_then(|c| c.ranges.as_ref())
        .into_iter()
        .flatten()
        .filter(|x| x.confirmed && x.excluded && x.start >= 0. && x.end > x.start)
        .collect();
    r.segments
        .iter()
        .filter_map(|s| {
            if ranges.iter().any(|x| x.start <= s.start && x.end >= s.end) {
                return None;
            }
            let mut copy = s.clone();
            if s.text == s.original_text && !s.words.is_empty() {
                copy.words
                    .retain(|w| !ranges.iter().any(|x| x.start <= w.start && x.end >= w.end));
                if copy.words.is_empty() {
                    return None;
                }
                if copy.words.len() != s.words.len() {
                    copy.start = copy.words[0].start;
                    copy.end = copy.words.last().unwrap().end;
                    let mut text = String::new();
                    for word in &copy.words {
                        if !text.is_empty()
                            && !word.text.starts_with([',', '.', '!', '?', ';', ':'])
                        {
                            text.push(' ');
                        }
                        text.push_str(&word.text);
                    }
                    copy.original_text = text.clone();
                    copy.text = text;
                }
            }
            Some(copy)
        })
        .collect()
}

pub fn paragraphs(r: &Recording) -> Vec<Paragraph> {
    let boundaries: Vec<_> = r
        .conference
        .iter()
        .flat_map(|c| c.chapters.iter().map(|c| c.start))
        .collect();
    let mut out: Vec<Paragraph> = vec![];
    let mut length = 0;
    for s in included_segments(r) {
        let size = s.text.graphemes(true).count();
        let join = out.last().is_some_and(|p| {
            let prev = p.segments.last().unwrap();
            prev.speaker_id == s.speaker_id
                && !boundaries.iter().any(|b| *b > prev.start && *b <= s.start)
                && (!prev.speaker_locked || prev.allows_paragraph_grouping == Some(true))
                && (!s.speaker_locked || s.allows_paragraph_grouping == Some(true))
                && s.start >= prev.start
                && s.start - prev.end <= 3.
                && s.end - p.start() <= 90.
                && length + size < 1400
        });
        if join {
            let p = out.last_mut().unwrap();
            p.text.push(' ');
            p.text.push_str(&s.text);
            p.segments.push(s);
            length += size + 1;
        } else {
            out.push(Paragraph {
                text: s.text.clone(),
                segments: vec![s],
            });
            length = size;
        }
    }
    out
}
pub fn plain_text(r: &Recording) -> String {
    paragraphs(r)
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}
pub fn timestamp(t: f64) -> String {
    let t = if t.is_finite() { t.max(0.) as u64 } else { 0 };
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}
/// Local media time, matching Swift's SRT exporter (source offsets belong in Markdown).
pub fn srt(r: &Recording) -> String {
    fn time(t: f64) -> String {
        let ms = (t.max(0.) * 1000.).round() as u64;
        format!("{},{:03}", timestamp((ms / 1000) as f64), ms % 1000)
    }
    included_segments(r)
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{}\n{} --> {}\n{}: {}\n",
                i + 1,
                time(s.start),
                time(s.end),
                r.speaker_name(s.speaker_id.as_deref()),
                s.text
                    .replace("\r\n", "\n")
                    .split('\n')
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn markdown(r: &Recording) -> String {
    let mut out = format!("# {}\n\n", r.title);
    if let Some(source) = &r.source {
        out += &format!("Source: {}\n\n", source.url);
    }
    let mut chapters: Vec<_> = r
        .conference
        .iter()
        .flat_map(|c| c.chapters.iter())
        .collect();
    chapters.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut chapter = 0;
    for p in paragraphs(r) {
        while chapter < chapters.len() && chapters[chapter].start <= p.start() {
            out += &format!("## {}\n\n", chapters[chapter].title);
            chapter += 1;
        }
        out += &format!(
            "**{} · {}**\n\n{}\n\n",
            r.speaker_name(p.speaker_id()),
            timestamp(r.source_time(p.start())),
            p.text
        );
    }
    if !r.notes.is_empty() {
        out += &format!("## Notes\n\n{}\n\n", r.notes);
    }
    if !r.refined_text.is_empty() {
        out += &format!("## Text AI\n\n{}\n", r.refined_text);
    }
    out
}

/// Byte offsets, for Rust/egui strings. Never fabricate word timing for edited text.
#[derive(Debug, PartialEq)]
pub struct WordRange {
    pub start: usize,
    pub end: usize,
    pub time: f64,
}
pub fn word_ranges(s: &Segment) -> Vec<WordRange> {
    if s.text != s.original_text {
        return vec![];
    }
    let mut offset = 0;
    let mut out = vec![];
    for w in &s.words {
        if w.text.is_empty() || w.start < s.start || w.start > s.end {
            continue;
        }
        if let Some(found) = s.text[offset..].find(&w.text) {
            let start = offset + found;
            offset = start + w.text.len();
            out.push(WordRange {
                start,
                end: offset,
                time: w.start,
            });
        }
    }
    out
}
#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub segment_id: String,
    pub time: f64,
    pub text: String,
}
pub fn search(r: &Recording, query: &str) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    included_segments(r)
        .into_iter()
        .filter(|s| {
            s.text.to_lowercase().contains(&q)
                || r.speaker_name(s.speaker_id.as_deref())
                    .to_lowercase()
                    .contains(&q)
        })
        .map(|s| SearchHit {
            segment_id: s.id,
            time: s.start,
            text: s.text,
        })
        .collect()
}
