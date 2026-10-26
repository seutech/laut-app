//! CPU-only, source-linked research. Primary text stays immutable; caches are disposable.
mod inference;
use crate::{Session, jobs::atomic_json, library::Library, media, read_json_bytes};
use anyhow::{Context, Result, ensure};
pub use inference::{LocalInference, ResearchConfig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

const VERSION: u32 = 1;
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Cancelled; completed work is retained locally"
    );
    Ok(())
}
fn directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "Research folder is a symlink"
    );
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub recording_id: String,
    pub title: String,
    pub kind: String,
    pub text: String,
    pub segment_id: Option<String>,
    pub time: Option<f64>,
    pub end: Option<f64>,
    pub document_id: Option<String>,
    pub page: Option<u32>,
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Corpus {
    pub sources: Vec<Source>,
    pub warnings: Vec<String>,
}
impl Corpus {
    pub fn fingerprint(&self) -> String {
        digest(&serde_json::to_vec(&self.sources).expect("sources"))
    }
    pub fn from_library(library: &Library, recording: Option<&str>, pdfs: bool) -> Result<Self> {
        let recordings = recording.map(|id| BTreeSet::from([id.to_owned()]));
        Self::from_library_ids(library, recordings.as_ref(), pdfs)
    }
    pub fn from_library_ids(
        library: &Library,
        recordings: Option<&BTreeSet<String>>,
        pdfs: bool,
    ) -> Result<Self> {
        let catalog = library.catalog()?;
        let mut out = Self {
            sources: vec![],
            warnings: catalog.warnings,
        };
        let mut entries = catalog.entries;
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        let mut projects = vec![];
        for entry in entries {
            if recordings.is_some_and(|ids| !ids.contains(&entry.id)) {
                continue;
            }
            let loaded = library.load(&entry.id)?;
            let documents = loaded
                .session
                .document
                .view()
                .conference
                .as_ref()
                .map_or(0, |conference| conference.documents.len());
            projects.push((documents, entry.id, loaded.session));
        }
        // Prefer the most specific project when the same PDF is attached to both its
        // talk and a conference-wide Q&A/archive project.
        projects.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let mut seen = BTreeSet::new();
        for (_, _, session) in projects {
            let pdf_identities: BTreeMap<_, _> = session
                .document
                .raw()
                .get("conference")
                .and_then(|value| value.get("documents"))
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|document| {
                    let id = document.get("id")?.as_str()?.to_owned();
                    let identity = document
                        .get("portableSource")
                        .and_then(|value| value.get("sha256"))
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| document.get("sha256").and_then(serde_json::Value::as_str))
                        .unwrap_or(&id)
                        .to_owned();
                    Some((id, identity))
                })
                .collect();
            let current = Self::from_session(&session, pdfs);
            for source in current.sources {
                let normal = normalised(&source.text);
                let keys = if source.kind == "pdf" {
                    let mut keys = vec![];
                    if let Some(identity) = source
                        .document_id
                        .as_ref()
                        .and_then(|id| pdf_identities.get(id))
                    {
                        // Copies of one PDF can produce slightly different reading
                        // order after separate extraction/OCR runs. The verified file
                        // hash and page number identify the authoritative source.
                        keys.push(format!(
                            "pdf-file|{identity}|{}",
                            source.page.unwrap_or_default()
                        ));
                    }
                    // Older imported projects may lack a retained file hash and may
                    // have different document IDs. A sorted word projection collapses
                    // the same page even when Poppler/OCR returned a different reading
                    // order, while keeping genuinely different page content distinct.
                    let mut words: Vec<_> = normal
                        .split(|c: char| !c.is_alphanumeric())
                        .filter(|word| !word.is_empty())
                        .map(str::to_lowercase)
                        .collect();
                    words.sort();
                    keys.push(format!(
                        "pdf-text|{}|{}",
                        source.label.to_lowercase(),
                        words.join(" ")
                    ));
                    keys
                } else if normal.len() >= 96 {
                    // Exact long overlaps occur when a preview and the final talk are
                    // both in the library. Keep short repeated phrases in context.
                    vec![format!("transcript|{normal}")]
                } else {
                    vec![source.id.clone()]
                };
                let keys: Vec<_> = keys.into_iter().map(|key| digest(key.as_bytes())).collect();
                if keys.iter().all(|key| !seen.contains(key)) {
                    seen.extend(keys);
                    out.sources.push(source);
                }
            }
            out.warnings.extend(current.warnings);
        }
        out.warnings.sort();
        out.warnings.dedup();
        ensure!(
            !out.sources.is_empty(),
            "No searchable source text in this scope"
        );
        ensure!(
            out.sources.len() <= 100_000,
            "Choose a smaller research scope (over 100,000 passages)"
        );
        Ok(out)
    }
    pub fn from_session(session: &Session, pdfs: bool) -> Self {
        let r = session.document.view();
        let mut sources = vec![];
        let mut warnings = vec![];
        let mut add = |text: &str,
                       kind: &str,
                       segment_id: Option<String>,
                       time: Option<f64>,
                       end: Option<f64>,
                       doc: Option<String>,
                       page: Option<u32>,
                       label: String| {
            // Layout spaces in PDF extraction must not consume the passage budget or
            // leave isolated footer fragments ranked as relevant evidence. Originals
            // retain their layout; this searchable projection preserves every word.
            for part in split_text(&normalised(text), 1400, 160) {
                let mut s = Source {
                    id: String::new(),
                    recording_id: r.id.clone(),
                    title: r.title.clone(),
                    kind: kind.into(),
                    text: part,
                    segment_id: segment_id.clone(),
                    time,
                    end,
                    document_id: doc.clone(),
                    page,
                    label: label.clone(),
                };
                s.id = digest(&serde_json::to_vec(&s).expect("source"));
                sources.push(s);
            }
        };
        for p in laut_core::paragraphs(&r) {
            add(
                &p.text,
                "transcript",
                Some(p.segments[0].id.clone()),
                Some(p.start()),
                p.segments.last().map(|s| s.end),
                None,
                None,
                format!(
                    "{} · {}",
                    laut_core::timestamp(r.source_time(p.start())),
                    r.speaker_name(p.speaker_id())
                ),
            );
        }
        if pdfs {
            for d in r.conference.iter().flat_map(|c| &c.documents) {
                for p in &d.pages {
                    if laut_core::conference::page_text(p).trim().is_empty() {
                        warnings.push(format!(
                            "{} · page {}: no extracted text; inspect the original PDF",
                            d.title, p.number
                        ));
                    }
                    add(
                        &laut_core::conference::page_text(p),
                        "pdf",
                        None,
                        None,
                        None,
                        Some(d.id.clone()),
                        Some(p.number),
                        format!("{} · page {}", d.title, p.number),
                    );
                }
            }
        }
        Self { sources, warnings }
    }
}
/// UTF-8 byte budget, preferring word boundaries. Overlap retains cross-boundary phrases.
pub fn split_text(text: &str, max: usize, overlap: usize) -> Vec<String> {
    assert!(max > overlap + 8);
    let mut result = vec![];
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end < text.len()
            && let Some((i, _)) = text[start..end]
                .char_indices()
                .rev()
                .find(|(i, c)| *i > max / 2 && c.is_whitespace())
        {
            end = start + i;
        }
        let value = text[start..end].trim();
        if !value.is_empty() {
            result.push(value.to_owned());
        }
        if end == text.len() {
            break;
        }
        let mut next = end.saturating_sub(overlap).max(start + 1);
        while !text.is_char_boundary(next) {
            next += 1;
        }
        start = next;
    }
    result
}
pub trait Inference {
    fn embedding_key(&self) -> &str;
    fn generation_key(&self) -> &str;
    fn embed(&self, texts: &[String], query: bool, cancel: &AtomicBool) -> Result<Vec<Vec<f32>>>;
    fn generate(
        &self,
        task: &str,
        evidence: &str,
        allowed: &[String],
        language: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Finding>>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Finding {
    pub text: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub quote: Option<String>,
}
pub fn validate_findings(findings: &[Finding], allowed: &BTreeSet<String>) -> Result<()> {
    ensure!(findings.len() <= 12, "Too many findings");
    for f in findings {
        ensure!(
            !f.text.trim().is_empty()
                && f.text.len() <= 2400
                && !f.sources.is_empty()
                && f.sources.len() <= 12
                && f.sources.iter().all(|s| allowed.contains(s)),
            "Invalid or unsupported source reference in model output"
        );
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub source: Source,
    pub semantic_score: f32,
    pub score: f32,
    #[serde(default)]
    pub matched_terms: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    #[serde(default)]
    pub weak_hits: Vec<SearchHit>,
    #[serde(default)]
    pub omitted_duplicates: usize,
    pub passages: usize,
    pub warnings: Vec<String>,
    pub fingerprint: String,
}
#[derive(Serialize, Deserialize)]
struct VectorCache {
    version: u32,
    key: String,
    vector: Vec<f32>,
}
fn normalise(v: &mut [f32]) -> Result<()> {
    ensure!(
        (16..=65536).contains(&v.len()) && v.iter().all(|x| x.is_finite()),
        "Invalid embedding vector"
    );
    let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    ensure!(norm > 1e-9, "Empty embedding vector");
    for x in v {
        *x = (*x as f64 / norm) as f32;
    }
    Ok(())
}
fn vector_key(engine: &impl Inference, text: &str, query: bool) -> String {
    digest(format!("{VERSION}|{}|{query}|{text}", engine.embedding_key()).as_bytes())
}
fn vectors(
    engine: &impl Inference,
    cache: &Path,
    texts: &[String],
    query: bool,
    cancel: &AtomicBool,
    progress: &impl Fn(f32, &str),
) -> Result<Vec<Vec<f32>>> {
    directory(cache)?;
    let mut result = vec![None; texts.len()];
    let mut missing = vec![];
    for (i, text) in texts.iter().enumerate() {
        check(cancel)?;
        let key = vector_key(engine, text, query);
        let path = cache.join(format!("{key}.json"));
        if path.exists() {
            let cached = read_json_bytes(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<VectorCache>(&b).ok());
            if let Some(mut c) = cached
                && c.version == VERSION
                && c.key == key
                && normalise(&mut c.vector).is_ok()
            {
                result[i] = Some(c.vector);
                continue;
            }
        }
        missing.push(i);
    }
    for batch in missing.chunks(4) {
        check(cancel)?;
        let input: Vec<_> = batch.iter().map(|i| texts[*i].clone()).collect();
        let output = engine.embed(&input, query, cancel)?;
        ensure!(
            output.len() == batch.len(),
            "Worker omitted embedding passages"
        );
        for (&i, mut vector) in batch.iter().zip(output) {
            normalise(&mut vector)?;
            let key = vector_key(engine, &texts[i], query);
            atomic_json(
                &cache.join(format!("{key}.json")),
                &VectorCache {
                    version: VERSION,
                    key,
                    vector: vector.clone(),
                },
            )?;
            result[i] = Some(vector);
        }
        let done = result.iter().filter(|v| v.is_some()).count();
        progress(
            done as f32 / texts.len().max(1) as f32,
            &format!("Index {done}/{}", texts.len()),
        );
    }
    let result: Vec<Vec<f32>> = result.into_iter().map(Option::unwrap).collect();
    ensure!(
        result.iter().all(|v| v.len() == result[0].len()),
        "Embedding dimensions changed; rebuild index with one model"
    );
    Ok(result)
}
pub fn search(
    engine: &impl Inference,
    corpus: &Corpus,
    cache: &Path,
    query: &str,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<SearchResult> {
    let query = query.trim();
    ensure!(
        !query.is_empty() && query.len() <= 1000,
        "Enter a query of at most 1000 bytes"
    );
    ensure!(!corpus.sources.is_empty(), "No source text");
    let texts: Vec<_> = corpus.sources.iter().map(|s| s.text.clone()).collect();
    let docs = vectors(engine, cache, &texts, false, cancel, &progress)?;
    let q = vectors(engine, cache, &[query.into()], true, cancel, &|_, _| {})?.remove(0);
    let stopwords: BTreeSet<&str> = [
        "a",
        "an",
        "and",
        "about",
        "are",
        "as",
        "at",
        "by",
        "for",
        "from",
        "how",
        "in",
        "include",
        "including",
        "is",
        "like",
        "mention",
        "mentioned",
        "mentions",
        "of",
        "on",
        "or",
        "related",
        "the",
        "these",
        "this",
        "to",
        "what",
        "where",
        "which",
        "who",
        "with",
        "auch",
        "das",
        "der",
        "die",
        "ein",
        "eine",
        "einschließlich",
        "erwähnt",
        "erwähnungen",
        "im",
        "in",
        "mit",
        "oder",
        "sowie",
        "und",
        "von",
        "was",
        "welche",
        "wer",
        "wie",
        "wo",
        "wurde",
        "werden",
        "zu",
    ]
    .into_iter()
    .collect();
    let terms: BTreeSet<_> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|x| x.chars().count() >= 2)
        .map(str::to_lowercase)
        .filter(|x| !stopwords.contains(x.as_str()))
        .collect();
    // Exact names, short acronyms and years matter in journalistic questions.
    // Weight anchors by rarity; semantic similarity alone often misses dates.
    let terms: Vec<_> = terms.into_iter().collect();
    let mut frequency = vec![0usize; terms.len()];
    let matches: Vec<Vec<usize>> = corpus
        .sources
        .iter()
        .map(|source| {
            let words: BTreeSet<_> = source
                .text
                .split(|c: char| !c.is_alphanumeric())
                .map(str::to_lowercase)
                .collect();
            terms
                .iter()
                .enumerate()
                .filter_map(|(i, term)| {
                    if words.contains(term) {
                        frequency[i] += 1;
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect();
    let weights: Vec<f32> = frequency
        .iter()
        .map(|&n| {
            if n == 0 {
                0.
            } else {
                (1. + corpus.sources.len() as f32 / n as f32).ln()
            }
        })
        .collect();
    let total_weight = weights.iter().sum::<f32>().max(f32::EPSILON);
    let mut hits = vec![];
    for ((s, v), matched) in corpus.sources.iter().zip(docs).zip(matches) {
        ensure!(
            q.len() == v.len(),
            "Embedding model/query dimension mismatch"
        );
        let semantic_score = q.iter().zip(v).map(|(a, b)| a * b).sum::<f32>();
        let lexical = matched.iter().map(|&i| weights[i]).sum::<f32>() / total_weight;
        hits.push(SearchHit {
            source: s.clone(),
            semantic_score,
            score: semantic_score + 0.25 * lexical,
            matched_terms: matched.iter().map(|&i| terms[i].clone()).collect(),
        });
    }
    // Literal evidence comes first. Semantic-only passages need a conservative
    // absolute floor; returning a fixed top 30 made unrelated material look like
    // evidence even when the best vector match was weak.
    hits.sort_by(|a, b| {
        (!b.matched_terms.is_empty())
            .cmp(&(!a.matched_terms.is_empty()))
            .then_with(|| b.matched_terms.len().cmp(&a.matched_terms.len()))
            .then_with(|| b.score.total_cmp(&a.score))
    });
    let mut unique = vec![];
    let mut seen = BTreeSet::new();
    let mut omitted_duplicates = 0;
    for hit in hits {
        let text = normalised(&hit.source.text).to_lowercase();
        let key = if hit.source.kind == "pdf" || text.len() >= 96 {
            format!("{}|{text}", hit.source.kind)
        } else {
            hit.source.id.clone()
        };
        if seen.insert(digest(key.as_bytes())) {
            unique.push(hit);
        } else {
            omitted_duplicates += 1;
        }
    }
    const STRONG_SEMANTIC_SCORE: f32 = 0.40;
    let (mut hits, mut weak_hits): (Vec<_>, Vec<_>) = unique.into_iter().partition(|hit| {
        !hit.matched_terms.is_empty() || hit.semantic_score >= STRONG_SEMANTIC_SCORE
    });
    hits.truncate(30);
    weak_hits.truncate(12);
    Ok(SearchResult {
        hits,
        weak_hits,
        omitted_duplicates,
        passages: corpus.sources.len(),
        warnings: corpus.warnings.clone(),
        fingerprint: corpus.fingerprint(),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub id: String,
    pub created_at: u64,
    pub question: String,
    pub mode: String,
    pub corpus_fingerprint: String,
    pub model: String,
    pub language: String,
    pub processed_passages: usize,
    pub total_passages: usize,
    pub findings: Vec<Finding>,
    pub sections: Vec<Vec<Finding>>,
    pub sources: BTreeMap<String, Source>,
    pub warnings: Vec<String>,
}
impl Report {
    pub fn markdown(&self) -> String {
        let de = self.language == "de";
        let mode = if de {
            if self.mode == "Search passages only" {
                "Antwort aus ausgewählten Suchtreffern"
            } else {
                "Gesamter ausgewählter Quellentext"
            }
        } else {
            &self.mode
        };
        let mut s = format!(
            "# {}\n\n{} · {} · {} / {} {}\n\n{}\n\n",
            self.question,
            if de { "KI-Entwurf" } else { "AI draft" },
            mode,
            self.processed_passages,
            self.total_passages,
            if de { "Passagen" } else { "passages" },
            if de {
                "Fundstellen und Belegzitate geprüft; Aussagen redaktionell prüfen."
            } else {
                "Source references and supporting quotations checked; factual claims still need editorial review."
            }
        );
        if self.findings.is_empty() {
            s += if de {
                "Keine ausreichend gestützten Aussagen erzeugt. Quellen und Abschnittsnotizen prüfen.\n"
            } else {
                "No supported findings generated. Inspect sources and section notes.\n"
            };
        }
        for f in &self.findings {
            s += &format!(
                "- {} {}\n",
                f.text,
                f.sources
                    .iter()
                    .map(|x| format!("[{x}]"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            if let Some(q) = &f.quote {
                s += &format!("  {}: {q}\n", if de { "Beleg" } else { "Evidence" });
            }
        }
        if !self.sections.is_empty() {
            s += if de {
                "\n## Abschnittsnotizen\n\n"
            } else {
                "\n## Section notes\n\n"
            };
            for (i, section) in self.sections.iter().enumerate() {
                s += &format!("### {}\n\n", i + 1);
                if section.is_empty() {
                    s += if de {
                        "Keine Aussagen für diesen Abschnitt erzeugt; Quellentext prüfen.\n"
                    } else {
                        "No findings returned for this section; inspect source text.\n"
                    };
                }
                for f in section {
                    s += &format!(
                        "- {} {}\n",
                        f.text,
                        f.sources
                            .iter()
                            .map(|x| format!("[{x}]"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    if let Some(q) = &f.quote {
                        s += &format!("  {}: {q}\n", if de { "Beleg" } else { "Evidence" });
                    }
                }
            }
        }
        s += if de {
            "\n## Quellen\n\n"
        } else {
            "\n## Sources\n\n"
        };
        for (id, source) in &self.sources {
            s += &format!(
                "### [{id}] {} · {}\n\n{}\n\n",
                source.title, source.label, source.text
            );
        }
        for warning in &self.warnings {
            s += &format!("\nNote: {warning}\n");
        }
        s
    }
}
fn evidence(sources: &BTreeMap<String, Source>) -> String {
    sources
        .iter()
        .map(|(id, s)| format!("[{id}] {}\n{}", s.label, s.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}
fn generation(
    engine: &impl Inference,
    task: &str,
    text: &str,
    allowed: &[String],
    language: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Finding>> {
    check(cancel)?;
    let f = engine.generate(task, text, allowed, language, cancel)?;
    validate_findings(&f, &allowed.iter().cloned().collect())?;
    Ok(f)
}
pub fn answer(
    engine: &impl Inference,
    corpus: &Corpus,
    search: &SearchResult,
    question: &str,
    language: &str,
    cancel: &AtomicBool,
) -> Result<Report> {
    ensure!(
        search.fingerprint == corpus.fingerprint(),
        "Sources changed; search again"
    );
    let mut sources = BTreeMap::new();
    let mut size = 0;
    for hit in &search.hits {
        if size + hit.source.text.len() > 4500 {
            continue;
        }
        size += hit.source.text.len();
        sources.insert(format!("S{}", sources.len() + 1), hit.source.clone());
        if sources.len() >= 5 {
            break;
        }
    }
    ensure!(!sources.is_empty(), "No supporting search passages");
    let allowed = sources.keys().cloned().collect::<Vec<_>>();
    let findings = grounded_generation(
        engine,
        question,
        &evidence(&sources),
        (&allowed, &sources),
        language,
        cancel,
    )?;
    Ok(report(
        engine,
        corpus,
        question,
        ("Search passages only", language),
        findings,
        vec![],
        sources,
    ))
}
fn report(
    engine: &impl Inference,
    corpus: &Corpus,
    task: &str,
    description: (&str, &str),
    findings: Vec<Finding>,
    sections: Vec<Vec<Finding>>,
    sources: BTreeMap<String, Source>,
) -> Report {
    Report {
        version: VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        question: task.into(),
        mode: description.0.into(),
        corpus_fingerprint: corpus.fingerprint(),
        model: engine.generation_key().into(),
        language: description.1.into(),
        processed_passages: sources.len(),
        total_passages: corpus.sources.len(),
        findings,
        sections,
        sources,
        warnings: corpus.warnings.clone(),
    }
}
#[derive(Serialize, Deserialize)]
struct Step {
    version: u32,
    key: String,
    findings: Vec<Finding>,
}
fn cached_generation(
    engine: &impl Inference,
    folder: &Path,
    task: &str,
    text: &str,
    references: (&[String], &BTreeMap<String, Source>),
    language: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Finding>> {
    let (allowed, sources) = references;
    let key = digest(&serde_json::to_vec(&(
        VERSION,
        engine.generation_key(),
        task,
        text,
        allowed,
        language,
    ))?);
    let path = folder.join(format!("{key}.json"));
    if path.exists() {
        let saved: Step = serde_json::from_slice(&read_json_bytes(&path)?)?;
        ensure!(
            saved.version == VERSION && saved.key == key,
            "Analysis checkpoint changed"
        );
        validate_findings(&saved.findings, &allowed.iter().cloned().collect())?;
        validate_quotes(&saved.findings, sources)?;
        return Ok(saved.findings);
    }
    let findings = grounded_generation(engine, task, text, (allowed, sources), language, cancel)?;
    atomic_json(
        &path,
        &Step {
            version: VERSION,
            key,
            findings: findings.clone(),
        },
    )?;
    Ok(findings)
}
pub fn analyse(
    engine: &impl Inference,
    corpus: &Corpus,
    cache: &Path,
    task: &str,
    language: &str,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<Report> {
    ensure!(
        !corpus.sources.is_empty() && !task.trim().is_empty() && task.len() <= 1000,
        "No source text or invalid task"
    );
    directory(cache)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cache.join(".analysis.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Another analysis is running in this cache")?;
    let sources: BTreeMap<String, Source> = corpus
        .sources
        .iter()
        .enumerate()
        .map(|(i, s)| (format!("S{}", i + 1), s.clone()))
        .collect();
    let mut batches: Vec<BTreeMap<String, Source>> = vec![];
    let mut batch = BTreeMap::new();
    for i in 0..corpus.sources.len() {
        let id = format!("S{}", i + 1);
        let mut candidate = batch.clone();
        candidate.insert(id.clone(), sources[&id].clone());
        if !batch.is_empty()
            && (candidate.len() > 3 || serde_json::to_string(&evidence(&candidate))?.len() > 4600)
        {
            batches.push(std::mem::take(&mut batch));
        }
        batch.insert(id.clone(), sources[&id].clone());
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    let mut sections = vec![];
    let mut done = 0;
    for selected in batches {
        check(cancel)?;
        progress(
            done as f32 / corpus.sources.len() as f32 * 0.8,
            &format!("Local AI / Lokale KI · {done}/{}", corpus.sources.len()),
        );
        let allowed: Vec<_> = selected.keys().cloned().collect();
        let note_task = format!(
            "Extract up to four relevant findings from this section. Preserve numbers, dates and uncertainty. Task: {task}"
        );
        let findings = cached_generation(
            engine,
            cache,
            &note_task,
            &evidence(&selected),
            (&allowed, &selected),
            language,
            cancel,
        )?;
        validate_quotes(&findings, &selected)?;
        sections.push(findings);
        done += selected.len();
        progress(
            done as f32 / corpus.sources.len() as f32 * 0.8,
            &format!("Analyse {done}/{}", corpus.sources.len()),
        );
    }
    // Every section note remains in the report, even when the overview must be shorter.
    let mut level: Vec<Finding> = sections.iter().flatten().cloned().collect();
    let all_allowed: BTreeSet<_> = sources.keys().cloned().collect();
    for round in 0..12 {
        // A short set of already grounded notes is its own overview. Avoid
        // loading a multi-GB model again just to paraphrase one to three facts.
        if level.len() <= 3 {
            break;
        }
        let mut batches: Vec<Vec<Finding>> = vec![];
        let mut current = vec![];
        let mut bytes = 0;
        for finding in level {
            let n = serde_json::to_vec(&finding)?.len();
            if bytes + n > 4200 && !current.is_empty() {
                batches.push(std::mem::take(&mut current));
                bytes = 0;
            }
            bytes += n;
            current.push(finding);
        }
        if !current.is_empty() {
            batches.push(current);
        }
        let single = batches.len() == 1;
        let mut reduced = vec![];
        for batch in batches {
            check(cancel)?;
            let allowed: Vec<_> = batch
                .iter()
                .flat_map(|f| f.sources.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let text = serde_json::to_string(&batch)?;
            let task = format!(
                "Combine these source-linked notes into at most three concise key findings. Keep only supported facts and their original source IDs. Task: {task}"
            );
            reduced.extend(cached_generation(
                engine,
                cache,
                &task,
                &text,
                (&allowed, &sources),
                language,
                cancel,
            )?);
        }
        validate_findings_unbounded(&reduced, &all_allowed)?;
        level = reduced;
        progress(0.9, &format!("Overview / Überblick {}", round + 1));
        if single {
            break;
        }
        ensure!(
            round < 11,
            "Overview reduction did not converge; section checkpoints retained"
        );
    }
    check(cancel)?;
    validate_quotes(&level, &sources)?;
    for section in &sections {
        validate_quotes(section, &sources)?;
    }
    Ok(report(
        engine,
        corpus,
        task,
        ("Full selected source text", language),
        level,
        sections,
        sources,
    ))
}
fn validate_findings_unbounded(findings: &[Finding], allowed: &BTreeSet<String>) -> Result<()> {
    for f in findings {
        validate_findings(std::slice::from_ref(f), allowed)?;
    }
    Ok(())
}
pub fn save_report(root: &Path, result: &Report) -> Result<PathBuf> {
    directory(root)?;
    uuid::Uuid::parse_str(&result.id)?;
    let path = root.join(format!("{}.json", result.id));
    ensure!(!path.exists(), "Report already exists");
    atomic_json(&path, result)?;
    Ok(path)
}
pub fn reports(root: &Path) -> Result<Vec<Report>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut all = vec![];
    for e in fs::read_dir(root)? {
        let p = e?.path();
        if p.extension().is_some_and(|s| s == "json") {
            let r: Report = serde_json::from_slice(&read_json_bytes(&p)?)?;
            ensure!(r.version == VERSION, "Unsupported research report");
            all.push(r);
        }
    }
    all.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    Ok(all)
}
pub fn verify_source(library: &Library, source: &Source) -> Result<()> {
    let loaded = library.load(&source.recording_id)?;
    let current = Corpus::from_session(&loaded.session, true);
    ensure!(
        current.sources.iter().any(|s| s.id == source.id),
        "Source text has changed. The report retains its original evidence; search again for current references."
    );
    Ok(())
}
pub fn model_key(path: &Path) -> Result<String> {
    ensure!(path.is_file(), "Select an installed local model");
    media::digest(path)
}

fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn validate_quotes(findings: &[Finding], sources: &BTreeMap<String, Source>) -> Result<()> {
    for f in findings {
        if let Some(quote) = &f.quote {
            let quote = normalised(quote);
            ensure!(
                quote.len() >= 12
                    && f.sources.iter().any(|id| sources
                        .get(id)
                        .is_some_and(|s| normalised(&s.text).contains(&quote))),
                "Supporting quote does not occur in the cited source; no report was published"
            );
        }
    }
    Ok(())
}

fn grounded_generation(
    engine: &impl Inference,
    task: &str,
    text: &str,
    references: (&[String], &BTreeMap<String, Source>),
    language: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Finding>> {
    let (allowed, sources) = references;
    let mut findings = generation(engine, task, text, allowed, language, cancel)?;
    restore_quote_spelling(&mut findings, sources);
    if validate_quotes(&findings, sources).is_ok() {
        return Ok(findings);
    }
    let repair = format!(
        "{task} Important: Every quote must be a contiguous excerpt copied exactly from a cited source. Do not translate quotes or replace words. Return fewer findings if necessary."
    );
    let mut findings = generation(engine, &repair, text, allowed, language, cancel)?;
    restore_quote_spelling(&mut findings, sources);
    validate_quotes(&findings, sources)?;
    Ok(findings)
}

/// Restore only unambiguous ASCII case/whitespace differences from the actual
/// evidence. Words, punctuation and numbers must match; never repair meaning.
pub fn restore_quote_spelling(findings: &mut [Finding], sources: &BTreeMap<String, Source>) {
    for finding in findings {
        let Some(quote) = &finding.quote else {
            continue;
        };
        let quote = normalised(quote);
        if quote.len() < 12 {
            continue;
        }
        let mut matches = BTreeSet::new();
        for id in &finding.sources {
            if let Some(source) = sources.get(id) {
                let source = normalised(&source.text);
                for (start, _) in source.char_indices() {
                    if let Some(candidate) = source.get(start..start.saturating_add(quote.len()))
                        && candidate.eq_ignore_ascii_case(&quote)
                    {
                        matches.insert(candidate.to_owned());
                    }
                }
            }
        }
        if matches.len() == 1 {
            finding.quote = matches.into_iter().next();
        }
    }
}
