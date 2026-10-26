//! Source-preserving conference review. Suggestions are lexical evidence, never visual proof.
use crate::{Document, Recording, included_segments};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}
pub fn context(doc: &Document) -> Value {
    let mut c = doc
        .raw()
        .get("conference")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    for key in ["documents", "chapters", "slides", "ranges"] {
        if !c[key].is_array() {
            c[key] = json!([]);
        }
    }
    if !c["language"].is_string() {
        c["language"] = json!("en");
    }
    if !c["vocabulary"].is_string() {
        c["vocabulary"] = json!("");
    }
    for key in ["chapters", "slides", "ranges"] {
        for row in c[key].as_array_mut().unwrap() {
            if !row["id"].is_string() {
                row["id"] = json!(id());
            }
            if key == "chapters" && !row["origin"].is_string() {
                row["origin"] = json!("Vorschlag");
            }
            if key == "ranges" {
                if !row["kind"].is_string() {
                    row["kind"] = json!("Markierung");
                }
                if !row["evidence"].is_string() {
                    row["evidence"] = json!("");
                }
            }
        }
    }
    for d in c["documents"].as_array_mut().unwrap() {
        if !d["nameCandidates"].is_array() {
            d["nameCandidates"] = json!([]);
        }
    }
    c
}
pub fn validate(c: &Value, duration: f64) -> Result<()> {
    ensure!(c.is_object(), "Invalid conference");
    ensure!(
        c["vocabulary"].as_str().is_some_and(|v| v.len() <= 100_000),
        "Invalid vocabulary"
    );
    ensure!(
        c["language"].as_str().is_some(),
        "Missing conference language"
    );
    let mut ids = BTreeSet::new();
    let mut pages = BTreeSet::new();
    for key in ["documents", "chapters", "slides", "ranges"] {
        let rows = c[key]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing {key}"))?;
        ensure!(rows.len() <= 10_000, "Too many conference entries");
        for row in rows {
            let id = row["id"].as_str().unwrap_or("");
            ensure!(
                uuid::Uuid::parse_str(id).is_ok() && ids.insert(id),
                "Invalid or duplicate conference ID"
            );
            if key == "documents" {
                let filename = row["filename"].as_str().unwrap_or("");
                ensure!(
                    filename.to_ascii_lowercase().ends_with(".pdf")
                        && !filename.contains(['\\', ':'])
                        && filename
                            .split('/')
                            .all(|p| !p.is_empty() && p != "." && p != ".."),
                    "Invalid PDF path"
                );
                ensure!(
                    row["title"].as_str().is_some_and(|s| !s.trim().is_empty()),
                    "Missing document title"
                );
                let pp = row["pages"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Missing pages"))?;
                ensure!(!pp.is_empty() && pp.len() <= 2000, "Invalid PDF page count");
                for p in pp {
                    let n = p["number"].as_u64().unwrap_or(0);
                    ensure!(
                        n > 0 && n <= 2000 && pages.insert((id, n)),
                        "Invalid or duplicate PDF page"
                    );
                    ensure!(
                        p["text"].as_str().is_some_and(|s| s.len() <= 2_000_000),
                        "Invalid PDF text"
                    );
                }
            } else {
                let start = row["start"].as_f64().unwrap_or(-1.);
                ensure!(
                    start.is_finite() && start >= 0. && start <= duration,
                    "Time outside recording"
                );
                if key == "chapters" {
                    ensure!(
                        row["title"].as_str().is_some_and(|s| !s.trim().is_empty()),
                        "Missing chapter title"
                    );
                }
                if key == "ranges" {
                    let end = row["end"].as_f64().unwrap_or(-1.);
                    ensure!(
                        end.is_finite() && end > start && end <= duration,
                        "Invalid range"
                    );
                    ensure!(
                        row["confirmed"].is_boolean() && row["excluded"].is_boolean(),
                        "Missing review status"
                    );
                }
                if key == "slides" {
                    ensure!(row["confirmed"].is_boolean(), "Missing slide review status");
                    ensure!(
                        pages.contains(&(
                            row["documentID"].as_str().unwrap_or(""),
                            row["page"].as_u64().unwrap_or(0)
                        )),
                        "Unknown PDF page"
                    );
                }
            }
        }
    }
    Ok(())
}
fn tokens(text: &str) -> BTreeSet<String> {
    let stop = "this that with have from they were will your about into their there which would should could these those durch diese dieser einer einem einen nicht werden wurde haben sowie telekom deutsche investor presentation slide company und der die das the and for are was our als ist von mit den des eine auf has you its";
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.chars().count() >= 4 && !stop.split_whitespace().any(|w| w == *s))
        .map(str::to_owned)
        .collect()
}
pub fn page_text(p: &crate::Page) -> String {
    match &p.ocr_text {
        Some(ocr) if !ocr.is_empty() => format!("{}\n{}", p.text, ocr),
        _ => p.text.clone(),
    }
}
pub fn chapter_suggestions(r: &Recording) -> Vec<Value> {
    let segments = included_segments(r);
    let mut result = vec![];
    let mut last = 0.;
    for (i, s) in segments.iter().enumerate() {
        let gap = i > 0 && s.start - segments[i - 1].end >= 2.;
        let change = i > 0 && s.speaker_id != segments[i - 1].speaker_id;
        if i == 0
            || (s.start - last >= 240.
                && s.text.split_whitespace().count() >= 6
                && (gap || change || s.start - last >= 480.))
        {
            last = s.start;
            result.push(json!({"id":id(),"start":s.start,"title":s.text.chars().take(72).collect::<String>(),"origin":"Vorschlag","portableEvidence":"Reading boundary from time, pause or speaker change; not an inferred topic"}));
        }
    }
    result
}
pub fn slide_suggestions(r: &Recording) -> Vec<Value> {
    let Some(c) = &r.conference else {
        return vec![];
    };
    let pages: Vec<_> = c
        .documents
        .iter()
        .flat_map(|d| {
            d.pages.iter().filter_map(move |p| {
                let text = page_text(p);
                (text.split_whitespace().count() >= 15).then(|| (&d.id, p.number, tokens(&text)))
            })
        })
        .collect();
    let mut freq = BTreeMap::new();
    for (_, _, words) in &pages {
        for w in words {
            *freq.entry(w.clone()).or_insert(0usize) += 1;
        }
    }
    let weight = |w: &String| {
        (1. + pages.len() as f64 / (1 + freq.get(w).copied().unwrap_or(0)) as f64).ln() + 1.
    };
    let mut boundaries: Vec<f64> = c
        .chapters
        .iter()
        .map(|c| c.start)
        .filter(|t| t.is_finite())
        .collect();
    if boundaries.is_empty() {
        boundaries = chapter_suggestions(r)
            .iter()
            .filter_map(|c| c["start"].as_f64())
            .collect();
    }
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    if boundaries.first().is_some_and(|t| *t > 0.) {
        boundaries.insert(0, 0.);
    }
    let segments = included_segments(r);
    let mut result = vec![];
    // Retain confirmed choices and explicitly rejected pages when refreshing suggestions.
    let mut used: BTreeSet<_> = c
        .slides
        .iter()
        .filter(|s| s.confirmed || s.useful == Some(false))
        .map(|s| (s.document_id.clone(), s.page))
        .collect();
    for (i, start) in boundaries.iter().enumerate() {
        let end = boundaries.get(i + 1).copied().unwrap_or(r.duration);
        let text = segments
            .iter()
            .filter(|s| s.start >= *start && s.start < end)
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let words = tokens(&text);
        let mut ranked = vec![];
        for (id, n, terms) in &pages {
            if used.contains(&((*id).clone(), *n)) {
                continue;
            }
            let shared: Vec<_> = words.intersection(terms).collect();
            let specific = shared
                .iter()
                .filter(|w| {
                    freq.get(**w).copied().unwrap_or(0) <= std::cmp::max(1, pages.len() / 5)
                })
                .count();
            if shared.len() < 5 || specific < 2 {
                continue;
            }
            let score = shared.iter().map(|w| weight(w)).sum::<f64>()
                / (terms.iter().map(&weight).sum::<f64>() * words.iter().map(&weight).sum::<f64>())
                    .max(1.)
                    .sqrt();
            ranked.push((
                score,
                (*id).clone(),
                *n,
                shared.into_iter().cloned().collect::<Vec<_>>(),
            ));
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        if let Some((score, doc, page, shared)) = ranked.first()
            && *score > 0.18
            && ranked.get(1).is_none_or(|next| *score - next.0 >= 0.025)
        {
            used.insert((doc.clone(), *page));
            result.push(json!({"id":id(),"documentID":doc,"page":page,"start":start,"confirmed":false,"matchEvidence":format!("Shared terms / Gemeinsame Begriffe: {}",shared.iter().take(10).cloned().collect::<Vec<_>>().join(", ")),"portableMatch":{"method":"lexical-idf-v1","score":score,"visualMatch":false}}));
        }
    }
    result
}
pub fn range_suggestions(r: &Recording) -> Vec<Value> {
    let starts = [
        "watch a video",
        "watch the video",
        "show you a film",
        "play the video",
        "einspieler",
        "kurzen film",
    ];
    let returns = [
        "this movie",
        "that video",
        "back on stage",
        "back to the stage",
        "dieser film",
        "zurück auf",
    ];
    let mut result = vec![];
    for (i, s) in r.segments.iter().enumerate() {
        if !starts.iter().any(|v| s.text.to_lowercase().contains(v)) {
            continue;
        }
        if let Some(end) = r
            .segments
            .iter()
            .skip(i + 1)
            .take_while(|v| v.start - s.end < 600.)
            .find(|v| {
                v.start > s.end + 15.
                    && returns
                        .iter()
                        .any(|cue| v.text.to_lowercase().contains(cue))
            })
        {
            let start = r.segments[i + 1].start;
            if end.start <= start {
                continue;
            }
            result.push(json!({"id":id(),"start":start,"end":end.start,"kind":"Möglicher Einspieler / Possible video insert","evidence":format!("{} → {}",s.text,end.text),"confirmed":false,"excluded":false}));
        }
    }
    result
}

/// A name mention alone is never an identity hint. Use confirmed vocabulary plus explicit cues.
pub fn name_suggestions(r: &Recording) -> Vec<Value> {
    let names: BTreeSet<_> = r
        .conference
        .iter()
        .flat_map(|c| c.vocabulary.lines())
        .filter_map(|s| s.split('|').next())
        .map(str::trim)
        .filter(|s| (2..=5).contains(&s.split_whitespace().count()))
        .collect();
    let mut result = vec![];
    let mut seen = BTreeSet::new();
    let segments = included_segments(r);
    for (i, s) in segments.iter().enumerate() {
        let Some(speaker) = &s.speaker_id else {
            continue;
        };
        let lower = s.text.to_lowercase();
        let intro = ["my name is ", "ich heiße ", "mein name ist "]
            .iter()
            .find_map(|cue| lower.split_once(cue).map(|(_, tail)| tail));
        let prev = i.checked_sub(1).map(|n| &segments[n]);
        let handover = prev.is_some_and(|p| {
            p.speaker_id != s.speaker_id
                && s.start - p.end < 45.
                && ["hand over", "come on stage", "übergebe", "begrüßen"]
                    .iter()
                    .any(|cue| p.text.to_lowercase().contains(cue))
        });
        let matches: Vec<_> = names
            .iter()
            .filter(|name| {
                let name = name.to_lowercase();
                intro.is_some_and(|tail| {
                    tail.strip_prefix(&name).is_some_and(|rest| {
                        rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
                    })
                }) || (handover && prev.is_some_and(|p| p.text.to_lowercase().contains(&name)))
            })
            .collect();
        if matches.len() == 1 && seen.insert((speaker.clone(), *matches[0])) {
            result.push(json!({"speakerID":speaker,"name":matches[0],"time":s.start,"evidence":if intro.is_some(){s.text.as_str()}else{prev.unwrap().text.as_str()},"method":"Explicit introduction/handover + reviewed vocabulary"}));
        }
    }
    result
}
