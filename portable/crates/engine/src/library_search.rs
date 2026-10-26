use crate::library::Library;
use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct LibraryHit {
    pub recording_id: String,
    pub title: String,
    pub kind: String,
    pub text: String,
    pub segment_id: Option<String>,
    pub time: Option<f64>,
    pub document_id: Option<String>,
    pub page: Option<u32>,
}
#[derive(Default, Serialize)]
pub struct LibrarySearch {
    pub hits: Vec<LibraryHit>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}
fn excerpt(text: &str, q: &str) -> String {
    // Search case folding can change byte length, so never use its offsets in the original.
    let chars: Vec<_> = text.chars().collect();
    let folded = text.to_lowercase();
    let position = folded
        .find(q)
        .map(|i| folded[..i].chars().count())
        .unwrap_or(0)
        .min(chars.len());
    let start = position.saturating_sub(50);
    let end = (start + 220).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}
impl Library {
    /// Read-only, rebuild-free baseline search. All records are scanned; UI work stays off-thread.
    pub fn search(&self, query: &str) -> Result<LibrarySearch> {
        let q = query.trim().to_lowercase();
        let mut result = LibrarySearch::default();
        if q.is_empty() {
            return Ok(result);
        }
        let catalog = self.catalog()?;
        result.warnings = catalog.warnings;
        for entry in catalog.entries {
            let loaded = match self.load(&entry.id) {
                Ok(v) => v,
                Err(e) => {
                    result.warnings.push(format!("{}: {e}", entry.title));
                    continue;
                }
            };
            let r = loaded.session.document.view();
            let mut add = |kind: &str,
                           text: &str,
                           segment_id: Option<String>,
                           time: Option<f64>,
                           reference: Option<(String, u32)>| {
                if result.hits.len() >= 200 {
                    result.truncated = true;
                    return;
                }
                result.hits.push(LibraryHit {
                    recording_id: r.id.clone(),
                    title: r.title.clone(),
                    kind: kind.into(),
                    text: excerpt(text, &q),
                    segment_id,
                    time,
                    document_id: reference.as_ref().map(|r| r.0.clone()),
                    page: reference.map(|r| r.1),
                });
            };
            for (kind, text) in [
                ("title", &r.title),
                ("notes", &r.notes),
                ("analysis", &r.refined_text),
            ] {
                if text.to_lowercase().contains(&q) {
                    add(kind, text, None, None, None);
                }
            }
            for hit in laut_core::search(&r, &q) {
                add(
                    "transcript",
                    &hit.text,
                    Some(hit.segment_id),
                    Some(hit.time),
                    None,
                );
            }
            if let Some(c) = &r.conference {
                for doc in &c.documents {
                    for page in &doc.pages {
                        let text = laut_core::conference::page_text(page);
                        if text.to_lowercase().contains(&q) {
                            add(
                                "document",
                                &format!("{} · {}: {}", doc.title, page.number, text),
                                None,
                                None,
                                Some((doc.id.clone(), page.number)),
                            );
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}
