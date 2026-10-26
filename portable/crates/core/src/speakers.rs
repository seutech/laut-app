//! Conservative, reversible alignment. A cluster is a voice label, never a person's identity.
use crate::Document;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub start: f64,
    pub end: f64,
    pub speaker: String,
}

pub fn validated_turns(value: &Value, duration: f64) -> Result<Vec<Turn>> {
    let mut turns: Vec<Turn> = serde_json::from_value(value.clone())?;
    ensure!(
        !turns.is_empty() && turns.len() <= 100_000,
        "No usable speaker turns"
    );
    ensure!(
        turns.iter().all(|t| t.start.is_finite()
            && t.end.is_finite()
            && t.start >= 0.
            && t.end > t.start
            && t.end <= duration + 0.1
            && !t.speaker.is_empty()
            && t.speaker.len() <= 100),
        "Invalid speaker turns"
    );
    ensure!(
        turns
            .iter()
            .map(|t| &t.speaker)
            .collect::<BTreeSet<_>>()
            .len()
            <= 100,
        "Too many speakers"
    );
    turns.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
    Ok(turns)
}

fn bridged(turns: &[Turn]) -> Vec<Turn> {
    let mut result = turns.to_vec();
    let mut latest: f64 = 0.;
    for pair in turns.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        latest = latest.max(a.end);
        if a.speaker == b.speaker && a.end >= latest && b.start > a.end && b.start - a.end <= 3.5 {
            result.push(Turn {
                start: a.end,
                end: b.start,
                speaker: a.speaker.clone(),
            });
        }
    }
    result.sort_by(|a, b| a.start.total_cmp(&b.start));
    result
}

struct TurnIndex {
    turns: Vec<Turn>,
    max_end: Vec<f64>,
}
impl TurnIndex {
    fn new(turns: Vec<Turn>) -> Self {
        let mut max: f64 = 0.;
        let max_end = turns
            .iter()
            .map(|t| {
                max = max.max(t.end);
                max
            })
            .collect();
        Self { turns, max_end }
    }
    fn intersecting(&self, start: f64, end: f64) -> impl Iterator<Item = &Turn> {
        let lo = self.max_end.partition_point(|v| *v <= start);
        let hi = self.turns.partition_point(|t| t.start < end);
        self.turns[lo.min(hi)..hi]
            .iter()
            .filter(move |t| t.end > start)
    }
}

// Sum the union of same-speaker spans: overlapping detector windows must not inflate votes.
fn scores(turns: &TurnIndex, start: f64, end: f64) -> BTreeMap<String, f64> {
    let end = end.max(start + 0.001);
    let mut ends: BTreeMap<String, f64> = BTreeMap::new();
    let mut scores = BTreeMap::new();
    for t in turns.intersecting(start, end) {
        let previous = ends.entry(t.speaker.clone()).or_insert(start);
        let a = start.max(t.start).max(*previous);
        let b = end.min(t.end);
        *scores.entry(t.speaker.clone()).or_insert(0.) += (b - a).max(0.);
        *previous = previous.max(b);
    }
    scores
}
fn choose(turns: &TurnIndex, start: f64, end: f64) -> Option<String> {
    let votes = scores(turns, start, end);
    let mut sorted: Vec<_> = votes.into_iter().filter(|(_, n)| *n > 0.).collect();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
    if let Some((id, n)) = sorted.first() {
        let runner = sorted.get(1).map_or(0., |(_, n)| *n);
        return (*n >= (end - start).max(0.001) * 0.55 && runner < *n * 0.45).then(|| id.clone());
    }
    // Only repair a short, wholly uncovered interval next to one unambiguous voice.
    if end - start > 1.5 {
        return None;
    }
    let near: BTreeSet<_> = turns
        .intersecting(start - 0.751, end + 0.751)
        .filter(|t| {
            (t.end <= start && start - t.end <= 0.75) || (t.start >= end && t.start - end <= 0.75)
        })
        .map(|t| t.speaker.clone())
        .collect();
    (near.len() == 1).then(|| near.into_iter().next().unwrap())
}

/// Suggestions use manually locked evidence only. Every proposed name remains reviewable.
pub fn suggested_mapping(doc: &Document, raw: &Value) -> Result<BTreeMap<String, String>> {
    let r = doc.view();
    let turns = TurnIndex::new(validated_turns(raw, r.duration)?);
    let mut totals: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for s in r.segments.iter().filter(|s| s.speaker_locked) {
        if let Some(id) = s
            .speaker_id
            .as_ref()
            .filter(|id| r.speakers.iter().any(|p| &p.id == *id))
        {
            for (cluster, n) in scores(&turns, s.start, s.end) {
                *totals
                    .entry(cluster)
                    .or_default()
                    .entry(id.clone())
                    .or_default() += n;
            }
        }
    }
    let mut map = BTreeMap::new();
    for (cluster, votes) in totals {
        let sum: f64 = votes.values().sum();
        if let Some((id, n)) = votes.iter().max_by(|a, b| a.1.total_cmp(b.1))
            && *n >= 2.
            && *n >= sum * 0.8
        {
            map.insert(cluster, id.clone());
        }
    }
    // Multiple clusters might be a split voice, but do not silently merge them by a guess.
    let values: Vec<_> = map.values().cloned().collect();
    map.retain(|_, id| values.iter().filter(|v| *v == id).count() == 1);
    Ok(map)
}

pub(crate) fn apply(
    doc: &Document,
    next: &mut Value,
    analysis: &Value,
    mapping: &BTreeMap<String, String>,
) -> Result<()> {
    let r = doc.view();
    let raw = analysis.get("turns").context("Missing speaker turns")?;
    let original = validated_turns(raw, r.duration)?;
    let turns = TurnIndex::new(bridged(&original));
    let clusters: BTreeSet<_> = original.iter().map(|t| t.speaker.clone()).collect();
    let names: BTreeMap<String, String> = analysis
        .get("names")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();
    ensure!(
        names.iter().all(|(id, name)| clusters.contains(id)
            && !name.trim().is_empty()
            && name.len() <= 400),
        "Invalid confirmed speaker names"
    );
    ensure!(
        analysis
            .get("recordingID")
            .is_none_or(|id| id == &json!(r.id)),
        "Speaker analysis belongs to another recording"
    );
    ensure!(
        mapping.keys().all(|s| clusters.contains(s))
            && mapping
                .values()
                .all(|id| r.speakers.iter().any(|s| &s.id == id)),
        "Invalid speaker mapping"
    );
    let mut ids = mapping.clone();
    for (i, cluster) in clusters.iter().enumerate() {
        let id = ids
            .entry(cluster.clone())
            .or_insert_with(|| uuid::Uuid::new_v4().to_string().to_uppercase())
            .clone();
        if !r.speakers.iter().any(|s| s.id == id) {
            next["speakers"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":id,"name":names.get(cluster).map(|s|s.trim().to_owned()).unwrap_or_else(||format!("Speaker {}",i+1))}));
        }
        let person = next["speakers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s["id"] == id)
            .unwrap();
        person["portableDiarization"] = json!({"analysisID":analysis["id"],"cluster":cluster,"turns":original.iter().filter(|t|&t.speaker==cluster).collect::<Vec<_>>(),"provenance":analysis["provenance"]});
    }
    let mut segments = vec![];
    for (i, s) in r.segments.iter().enumerate() {
        let raw = &doc.raw()["segments"][i];
        if s.speaker_locked {
            segments.push(raw.clone());
            continue;
        }
        let mut boundaries = vec![];
        let mut cursor = 0;
        if s.text == s.original_text
            && !s.words.is_empty()
            && s.words.iter().all(|w| w.start >= s.start && w.end <= s.end)
            && s.words.windows(2).all(|w| w[0].start <= w[1].start)
        {
            for w in &s.words {
                let Some(at) = s.text[cursor..].find(&w.text) else {
                    boundaries.clear();
                    break;
                };
                let pos = cursor + at;
                // Unexpected non-whitespace text indicates incompatible word evidence.
                if !s.text[cursor..pos].trim().is_empty() || w.text.is_empty() {
                    boundaries.clear();
                    break;
                }
                boundaries.push((pos, w.start, w.end, choose(&turns, w.start, w.end)));
                cursor = pos + w.text.len();
            }
            if !s.text[cursor..].trim().is_empty() {
                boundaries.clear();
            }
        }
        let mut groups: Vec<(usize, usize, Option<String>)> = vec![];
        for (j, (_, _, _, speaker)) in boundaries.iter().enumerate() {
            if let Some(g) = groups.last_mut().filter(|g| &g.2 == speaker) {
                g.1 = j + 1;
            } else {
                groups.push((j, j + 1, speaker.clone()));
            }
        }
        if groups.len() <= 1 {
            let mut part = raw.clone();
            let cluster = if groups.len() == 1 {
                groups[0].2.clone()
            } else {
                choose(&turns, s.start, s.end)
            };
            part["speakerLocked"] = json!(
                cluster
                    .as_ref()
                    .is_some_and(|c| mapping.contains_key(c) || names.contains_key(c))
            );
            part["speakerID"] = json!(cluster.and_then(|c| ids.get(&c).cloned()));
            part["allowsParagraphGrouping"] = json!(true);
            part["portableSpeakerAnalysisID"] = analysis["id"].clone();
            segments.push(part);
            continue;
        }
        for (a, b, cluster) in groups {
            let lo = if a == 0 { 0 } else { boundaries[a].0 };
            let hi = if b == boundaries.len() {
                s.text.len()
            } else {
                boundaries[b].0
            };
            let mut part = raw.clone();
            // Keep large token evidence once per original segment. Repeating it in every
            // split part and in each history revision makes long talks needlessly huge.
            let evidence_id = raw["portableDiarizationSourceID"].as_str().unwrap_or(&s.id);
            if doc.raw()["portableDiarizationSources"]
                .get(evidence_id)
                .is_none()
            {
                next.as_object_mut()
                    .unwrap()
                    .entry("portableDiarizationSources")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .context("Invalid diarization evidence archive")?
                    .entry(evidence_id.to_owned())
                    .or_insert_with(|| raw.clone());
            }
            part["portableDiarizationSourceID"] = json!(evidence_id);
            part.as_object_mut().unwrap().remove("workerTokens");
            part["id"] = json!(uuid::Uuid::new_v4().to_string().to_uppercase());
            part["start"] = json!(if a == 0 { s.start } else { boundaries[a].1 });
            part["end"] = json!(if b == boundaries.len() {
                s.end
            } else {
                boundaries[b - 1].2
            });
            part["text"] = json!(s.text[lo..hi].trim());
            part["originalText"] = part["text"].clone();
            part["words"] = json!(&raw["words"].as_array().unwrap()[a..b]);
            part["speakerLocked"] = json!(
                cluster
                    .as_ref()
                    .is_some_and(|c| mapping.contains_key(c) || names.contains_key(c))
            );
            part["speakerID"] = json!(cluster.and_then(|c| ids.get(&c).cloned()));
            part["allowsParagraphGrouping"] = json!(true);
            part["portableSpeakerAnalysisID"] = analysis["id"].clone();
            segments.push(part);
        }
    }
    next["segments"] = json!(segments);
    Ok(())
}
