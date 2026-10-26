use crate::{Document, markdown, plain_text, search, srt};
use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

/// Shared by the desktop and CLI; no widget can directly mutate the authoritative JSON.
#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Inspect,
    AppendAnalysis {
        text: String,
    },
    EditConference {
        value: Value,
    },
    Search {
        query: String,
    },
    ExportText,
    ExportMarkdown,
    ExportSrt,
    ApplyDraft {
        title: String,
        notes: String,
        texts: std::collections::BTreeMap<String, String>,
        names: std::collections::BTreeMap<String, String>,
    },
    UpdateDetails {
        title: String,
        notes: String,
    },
    SplitSegment {
        id: String,
        #[serde(deserialize_with = "deserialize_time")]
        at: f64,
        offset: usize,
    },
    MergeNext {
        id: String,
    },
    ReplaceText {
        find: String,
        replacement: String,
    },
    AddSpeaker {
        name: String,
    },
    ApplySpeakerAnalysis {
        analysis: Value,
        mapping: std::collections::BTreeMap<String, String>,
    },
    MergeSpeakers {
        from: String,
        into: String,
    },
    EditSegment {
        id: String,
        text: String,
    },
    RenameSpeaker {
        id: String,
        name: String,
    },
    AssignSpeaker {
        segment_id: String,
        speaker_id: Option<String>,
    },
    Undo,
    Redo,
}
fn deserialize_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<f64, D::Error> {
    // Internally tagged enums buffer numbers; Value handles serde_json's precise-number map.
    let value = Value::deserialize(deserializer)?;
    value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| serde::de::Error::custom("Expected a finite time"))
}
pub fn execute(doc: &mut Document, cmd: Command) -> Result<Value> {
    let r = doc.view();
    match cmd {
        Command::Inspect => {
            return Ok(
                json!({"id":r.id,"title":r.title,"segments":r.segments.len(),"duration":r.duration,"speakers":r.speakers.len()}),
            );
        }
        Command::Search { query } => return Ok(serde_json::to_value(search(&r, &query))?),
        Command::ExportText => return Ok(json!(plain_text(&r))),
        Command::ExportMarkdown => return Ok(json!(markdown(&r))),
        Command::ExportSrt => return Ok(json!(srt(&r))),
        Command::Undo => return move_history(doc, false),
        Command::Redo => return move_history(doc, true),
        _ => {}
    }
    let mut next = doc.raw.clone();
    let label = match cmd {
        Command::AppendAnalysis { text } => {
            ensure!(
                !text.trim().is_empty() && text.len() <= 8_000_000,
                "Invalid analysis text"
            );
            let old = next["refinedText"].as_str().unwrap_or("");
            next["refinedText"] = json!(if old.is_empty() {
                text
            } else {
                format!("{old}\n\n---\n\n{text}")
            });
            "Save local analysis"
        }
        Command::EditConference { value } => {
            crate::conference::validate(&value, r.duration)?;
            next["conference"] = value;
            "Edit conference"
        }
        Command::ApplyDraft {
            title,
            notes,
            texts,
            names,
        } => {
            ensure!(!title.trim().is_empty(), "Title is empty");
            next["title"] = json!(title.trim());
            next["notes"] = json!(notes);
            for (id, text) in texts {
                let i = r
                    .segments
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or_else(|| anyhow!("Draft references a missing segment"))?;
                next["segments"][i]["text"] = json!(text);
            }
            for (id, name) in names {
                ensure!(!name.trim().is_empty(), "Speaker name is empty");
                let i = r
                    .speakers
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or_else(|| anyhow!("Draft references a missing speaker"))?;
                if r.speakers[i].name == name.trim() {
                    continue;
                }
                next["speakers"][i]["name"] = json!(name.trim());
                for (i, s) in r
                    .segments
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.speaker_id.as_deref() == Some(id.as_str()))
                {
                    if !s.speaker_locked {
                        next["segments"][i]["allowsParagraphGrouping"] = json!(true);
                    }
                    next["segments"][i]["speakerLocked"] = json!(true);
                }
            }
            "Apply editor draft"
        }
        Command::UpdateDetails { title, notes } => {
            ensure!(!title.trim().is_empty(), "Title is empty");
            next["title"] = json!(title.trim());
            next["notes"] = json!(notes);
            "Edit title and notes"
        }
        Command::ReplaceText { find, replacement } => {
            ensure!(!find.is_empty(), "Search text is empty");
            for (i, s) in r.segments.iter().enumerate() {
                next["segments"][i]["text"] = json!(s.text.replace(&find, &replacement));
            }
            "Replace transcript text"
        }
        Command::SplitSegment { id, at, offset } => {
            let i = r
                .segments
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| anyhow!("Unknown segment"))?;
            let s = &r.segments[i];
            ensure!(
                at.is_finite() && at > s.start && at < s.end,
                "Split time must be inside the segment"
            );
            let boundary = s
                .text
                .grapheme_indices(true)
                .nth(offset)
                .map(|(i, _)| i)
                .filter(|i| *i > 0)
                .ok_or_else(|| anyhow!("Choose a text boundary inside the segment"))?;
            let (left, right) = s.text.split_at(boundary);
            ensure!(
                !left.trim().is_empty() && !right.trim().is_empty(),
                "Both parts need text"
            );
            let original = next["segments"][i].clone();
            let pieces: Vec<_> = [(s.start, at, left), (at, s.end, right)]
                .into_iter()
                .map(|(start, end, text)| {
                    let mut part = original.clone();
                    part["id"] = json!(uuid::Uuid::new_v4().to_string().to_uppercase());
                    part["start"] = json!(start);
                    part["end"] = json!(end);
                    part["text"] = json!(text.trim());
                    part["originalText"] = json!(text.trim());
                    // A manual text/time boundary is not forced alignment. Preserve raw evidence
                    // independently, including unknown fields, after the bounded undo history expires.
                    part["words"] = json!([]);
                    part["portableEditSources"] = json!(evidence(&original));
                    part["speakerLocked"] = json!(true);
                    part.as_object_mut()
                        .unwrap()
                        .remove("allowsParagraphGrouping");
                    part
                })
                .collect();
            next["segments"]
                .as_array_mut()
                .unwrap()
                .splice(i..=i, pieces);
            "Split segment"
        }
        Command::MergeNext { id } => {
            let i = r
                .segments
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| anyhow!("Unknown segment"))?;
            ensure!(i + 1 < r.segments.len(), "No following segment");
            let (a, b) = (&r.segments[i], &r.segments[i + 1]);
            ensure!(
                b.start >= a.start,
                "Segments are not in chronological order"
            );
            let mut originals = evidence(&next["segments"][i]);
            for source in evidence(&next["segments"][i + 1]) {
                if !originals.contains(&source) {
                    originals.push(source);
                }
            }
            let mut words = next["segments"][i]["words"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            words.extend(
                next["segments"][i + 1]["words"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
            let merged = &mut next["segments"][i];
            merged["end"] = json!(a.end.max(b.end));
            merged["text"] = json!(format!("{} {}", a.text, b.text));
            merged["originalText"] = json!(format!("{} {}", a.original_text, b.original_text));
            merged["words"] = json!(words);
            merged["speakerLocked"] = json!(true);
            merged["portableEditSources"] = json!(originals);
            merged
                .as_object_mut()
                .unwrap()
                .remove("allowsParagraphGrouping");
            next["segments"].as_array_mut().unwrap().remove(i + 1);
            "Merge segments"
        }
        Command::AddSpeaker { name } => {
            ensure!(!name.trim().is_empty(), "Speaker name is empty");
            next["speakers"].as_array_mut().unwrap().push(
                json!({"id":uuid::Uuid::new_v4().to_string().to_uppercase(),"name":name.trim()}),
            );
            "Add speaker"
        }
        Command::ApplySpeakerAnalysis { analysis, mapping } => {
            crate::speakers::apply(doc, &mut next, &analysis, &mapping)?;
            "Apply speaker analysis"
        }
        Command::MergeSpeakers { from, into } => {
            ensure!(from != into, "Choose two different speakers");
            let a = r
                .speakers
                .iter()
                .position(|s| s.id == from)
                .ok_or_else(|| anyhow!("Unknown source speaker"))?;
            let b = r
                .speakers
                .iter()
                .position(|s| s.id == into)
                .ok_or_else(|| anyhow!("Unknown destination speaker"))?;
            let source = next["speakers"][a].clone();
            let target = next["speakers"][b]
                .as_object_mut()
                .unwrap()
                .entry("portableMergedSpeakers")
                .or_insert_with(|| json!([]));
            target
                .as_array_mut()
                .ok_or_else(|| anyhow!("Invalid speaker provenance"))?
                .push(source);
            next["speakers"].as_array_mut().unwrap().remove(a);
            for (i, s) in r
                .segments
                .iter()
                .enumerate()
                .filter(|(_, s)| s.speaker_id.as_deref() == Some(from.as_str()))
            {
                next["segments"][i]["speakerID"] = json!(into);
                if !s.speaker_locked {
                    next["segments"][i]["allowsParagraphGrouping"] = json!(true);
                }
                next["segments"][i]["speakerLocked"] = json!(true);
            }
            "Merge speakers"
        }
        Command::EditSegment { id, text } => {
            let i = r
                .segments
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| anyhow!("Unknown segment"))?;
            next["segments"][i]["text"] = json!(text);
            "Edit transcript"
        }
        Command::RenameSpeaker { id, name } => {
            ensure!(!name.trim().is_empty(), "Speaker name is empty");
            let i = r
                .speakers
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| anyhow!("Unknown speaker"))?;
            next["speakers"][i]["name"] = json!(name.trim());
            for (i, s) in r
                .segments
                .iter()
                .enumerate()
                .filter(|(_, s)| s.speaker_id.as_deref() == Some(&id))
            {
                if !s.speaker_locked {
                    next["segments"][i]["allowsParagraphGrouping"] = json!(true);
                }
                next["segments"][i]["speakerLocked"] = json!(true);
            }
            "Name speaker"
        }
        Command::AssignSpeaker {
            segment_id,
            speaker_id,
        } => {
            ensure!(
                speaker_id
                    .as_ref()
                    .is_none_or(|id| r.speakers.iter().any(|s| &s.id == id)),
                "Unknown speaker"
            );
            let i = r
                .segments
                .iter()
                .position(|s| s.id == segment_id)
                .ok_or_else(|| anyhow!("Unknown segment"))?;
            next["segments"][i]["speakerID"] = json!(speaker_id);
            next["segments"][i]["speakerLocked"] = json!(true);
            next["segments"][i]
                .as_object_mut()
                .unwrap()
                .remove("allowsParagraphGrouping");
            "Assign speaker"
        }
        _ => unreachable!(),
    };
    if next == doc.raw {
        return Ok(json!({"changed":false}));
    }
    // Swift-compatible delta history: only changed segment span / speaker list is stored.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs_f64()
        - 978307200.;
    let mut revision =
        json!({"id":uuid::Uuid::new_v4().to_string().to_uppercase(),"label":label,"date":now});
    for field in ["title", "notes", "refinedText", "speakers", "conference"] {
        if next[field] != doc.raw[field] {
            revision[field] = json!({"before":doc.raw.get(field).cloned().unwrap_or_else(||if field=="conference" {Value::Null}else{json!("")}),"after":next[field]});
        }
    }
    let a = doc.raw["segments"].as_array().unwrap();
    let b = next["segments"].as_array().unwrap();
    if a != b {
        let prefix = a.iter().zip(b).take_while(|(a, b)| a == b).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        revision["segments"] = json!({"index":prefix,"before":a[prefix..a.len()-suffix],"after":b[prefix..b.len()-suffix]});
    }
    let mut history = history(&doc.raw)?;
    let undo = history["undo"].as_array_mut().unwrap();
    undo.push(revision);
    if undo.len() > 30 {
        undo.remove(0);
    }
    // Full speaker passes can touch an entire seven-hour transcript. Bound history by
    // size as well as count, while always retaining the newest undo operation.
    let sizes = undo
        .iter()
        .map(|v| serde_json::to_vec_pretty(v).map(|b| b.len()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut total: usize = sizes.iter().sum();
    let mut drop = 0;
    while total > 64 * 1024 * 1024 && drop + 1 < sizes.len() {
        total -= sizes[drop];
        drop += 1;
    }
    undo.drain(..drop);
    history["redo"] = json!([]);
    next["editHistory"] = history;
    *doc = Document::from_value(next)?;
    Ok(json!({"changed":true}))
}
fn history(raw: &Value) -> Result<Value> {
    let history = match raw.get("editHistory") {
        Some(v) if !v.is_null() => v.clone(),
        _ => json!({"undo":[],"redo":[]}),
    };
    ensure!(
        history["undo"].is_array() && history["redo"].is_array(),
        "Invalid edit history; original retained"
    );
    Ok(history)
}
fn move_history(doc: &mut Document, forward: bool) -> Result<Value> {
    let mut h = history(&doc.raw)?;
    let (from, to, expected, replacement) = if forward {
        ("redo", "undo", "before", "after")
    } else {
        ("undo", "redo", "after", "before")
    };
    let revision = h[from]
        .as_array_mut()
        .unwrap()
        .pop()
        .ok_or_else(|| anyhow!("No history in this direction"))?;
    let mut next = doc.raw.clone();
    for field in ["title", "notes", "refinedText", "speakers", "conference"] {
        if let Some(change) = revision.get(field).filter(|v| !v.is_null()) {
            ensure!(
                change.get(expected).is_some() || field == "conference",
                "Malformed history"
            );
            ensure!(
                next.get(field).cloned().unwrap_or_else(|| {
                    if ["notes", "refinedText"].contains(&field) {
                        json!("")
                    } else {
                        Value::Null
                    }
                }) == change[expected],
                "History conflicts with current document"
            );
            next[field] = change[replacement].clone();
        }
    }
    if let Some(c) = revision.get("segments").filter(|v| !v.is_null()) {
        let i = c["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Invalid history index"))? as usize;
        let a = c[expected]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid history segments"))?;
        let b = c[replacement]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid history segments"))?;
        let segments = next["segments"].as_array_mut().unwrap();
        if i > segments.len() || a.len() > segments.len() - i || segments[i..i + a.len()] != *a {
            bail!("History conflicts with current segments");
        }
        segments.splice(i..i + a.len(), b.iter().cloned());
    }
    h[to].as_array_mut().unwrap().push(revision);
    next["editHistory"] = h;
    *doc = Document::from_value(next)?;
    Ok(json!({"changed":true}))
}

fn evidence(segment: &Value) -> Vec<Value> {
    segment
        .get("portableEditSources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![segment.clone()])
}
