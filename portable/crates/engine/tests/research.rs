use anyhow::Result;
use laut_engine::{
    Session,
    research::{self, Corpus, Finding, Inference},
};
use serde_json::json;
use std::{
    cell::Cell,
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};
fn session() -> Session {
    Session {
        document: laut_core::Document::parse(include_bytes!("../../../fixtures/conversation.json"))
            .unwrap(),
        source: "/fixture/recording.json".into(),
        dirty: false,
    }
}
struct Fake {
    key: String,
    embeds: Cell<usize>,
    generations: Cell<usize>,
    cancel_after: Option<usize>,
    invalid: bool,
}
impl Fake {
    fn new() -> Self {
        Self {
            key: "model-1".into(),
            embeds: Cell::new(0),
            generations: Cell::new(0),
            cancel_after: None,
            invalid: false,
        }
    }
}
impl Inference for Fake {
    fn embedding_key(&self) -> &str {
        &self.key
    }
    fn generation_key(&self) -> &str {
        &self.key
    }
    fn embed(&self, texts: &[String], _: bool, _: &AtomicBool) -> Result<Vec<Vec<f32>>> {
        self.embeds.set(self.embeds.get() + texts.len());
        Ok(texts
            .iter()
            .map(|s| {
                let mut v = vec![0.1; 16];
                v[0] = s.len() as f32;
                v
            })
            .collect())
    }
    fn generate(
        &self,
        _: &str,
        _: &str,
        allowed: &[String],
        _: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<Finding>> {
        let n = self.generations.get() + 1;
        self.generations.set(n);
        if self.cancel_after == Some(n) {
            cancel.store(true, Ordering::Relaxed);
        }
        Ok(vec![Finding {
            quote: None,
            text: "Supported draft".into(),
            sources: vec![if self.invalid {
                "invented".into()
            } else {
                allowed[0].clone()
            }],
        }])
    }
}
#[test]
fn unicode_splitting_keeps_every_word_including_tail() {
    let text = (0..500)
        .map(|i| format!("größer東京🙂{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let pieces = research::split_text(&text, 1400, 160);
    assert!(pieces.iter().all(|p| p.len() <= 1400));
    for word in text.split_whitespace() {
        assert!(pieces.iter().any(|p| p.contains(word)), "{word}");
    }
}
#[test]
fn primary_corpus_preserves_pdf_page_and_omits_exclusions_and_generated_text() {
    let mut s = session();
    let mut raw = s.document.raw().clone();
    raw["refinedText"] = json!("NEVER INDEX GENERATED");
    raw["conference"] = json!({"ranges":[{"start":0.,"end":99999.,"confirmed":true,"excluded":true}],"documents":[{"id":"doc","title":"Chart","filename":"chart.pdf","pages":[{"number":7,"text":"","ocrText":"OCR primary fact"},{"number":8,"text":""}]}]});
    s.document = laut_core::Document::from_value(raw).unwrap();
    let c = Corpus::from_session(&s, true);
    assert_eq!(c.sources.len(), 1);
    assert_eq!(c.sources[0].page, Some(7));
    assert_eq!(c.sources[0].text, "OCR primary fact");
    assert_eq!(c.warnings.len(), 1);
}
#[test]
fn pdf_layout_whitespace_does_not_split_related_facts() {
    let mut s = session();
    let mut raw = s.document.raw().clone();
    let text = format!(
        "AI Guidelines{}2018\nLeading Digital Telco{}2021",
        " ".repeat(1800),
        " ".repeat(1800)
    );
    raw["conference"] = json!({"documents":[{"id":"doc","title":"Strategy","filename":"strategy.pdf","pages":[{"number":3,"text":text}]}]});
    s.document = laut_core::Document::from_value(raw.clone()).unwrap();
    let c = Corpus::from_session(&s, true);
    let pdf: Vec<_> = c.sources.iter().filter(|x| x.kind == "pdf").collect();
    assert_eq!(pdf.len(), 1);
    assert_eq!(pdf[0].text, "AI Guidelines 2018 Leading Digital Telco 2021");
    assert_eq!(pdf[0].page, Some(3));
    assert_eq!(s.document.raw(), &raw);
}
#[test]
fn quoted_spelling_is_restored_without_changing_words_or_numbers() {
    let c = Corpus::from_session(&session(), false);
    let mut source = c.sources[0].clone();
    source.text = "We expect efficiency benefits for 2027.".into();
    let sources = std::collections::BTreeMap::from([("S1".into(), source)]);
    let mut findings = vec![Finding {
        text: "Draft".into(),
        sources: vec!["S1".into()],
        quote: Some("we expect  efficiency benefits for 2027".into()),
    }];
    research::restore_quote_spelling(&mut findings, &sources);
    assert_eq!(
        findings[0].quote.as_deref(),
        Some("We expect efficiency benefits for 2027")
    );
    research::validate_quotes(&findings, &sources).unwrap();
    findings[0].quote = Some("We expect efficiency benefits for 2028".into());
    research::restore_quote_spelling(&mut findings, &sources);
    assert!(research::validate_quotes(&findings, &sources).is_err());
    findings[0].sources = vec!["S2".into()];
    findings[0].quote = Some("we expect efficiency benefits for 2027".into());
    research::restore_quote_spelling(&mut findings, &sources);
    assert!(research::validate_quotes(&findings, &sources).is_err());
}
#[test]
fn vectors_reuse_unchanged_text_and_invalidate_model_and_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let c = Corpus::from_session(&session(), false);
    let f = Fake::new();
    let cancel = AtomicBool::new(false);
    let first = research::search(&f, &c, tmp.path(), "question", &cancel, |_, _| {}).unwrap();
    let n = f.embeds.get();
    research::search(&f, &c, tmp.path(), "question", &cancel, |_, _| {}).unwrap();
    assert_eq!(f.embeds.get(), n);
    let mut changed = c.clone();
    changed.sources[0].text.push_str(" corrected");
    research::search(&f, &changed, tmp.path(), "question", &cancel, |_, _| {}).unwrap();
    assert_eq!(f.embeds.get(), n + 1);
    assert!(research::answer(&f, &changed, &first, "question", "de", &cancel).is_err());
    let mut other = Fake::new();
    other.key = "other-model".into();
    research::search(&other, &c, tmp.path(), "question", &cancel, |_, _| {}).unwrap();
    assert_eq!(other.embeds.get(), n);
}
#[test]
fn interrupted_full_analysis_resumes_and_keeps_all_source_passages() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = Corpus::from_session(&session(), false);
    let base = c.sources[0].clone();
    c.sources = (0..17)
        .map(|i| {
            let mut s = base.clone();
            s.id = format!("id{i}");
            s.text = format!("unique section {i}");
            s
        })
        .collect();
    let mut first = Fake::new();
    first.cancel_after = Some(1);
    let cancel = AtomicBool::new(false);
    assert!(research::analyse(&first, &c, tmp.path(), "task", "de", &cancel, |_, _| {}).is_err());
    let second = Fake::new();
    let result = research::analyse(
        &second,
        &c,
        tmp.path(),
        "task",
        "de",
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(result.processed_passages, 17);
    assert_eq!(result.sections.len(), 6);
    assert!(second.generations.get() < 7);
    assert!(result.markdown().contains("unique section 16"));
    let third = Fake::new();
    research::analyse(
        &third,
        &c,
        tmp.path(),
        "task",
        "de",
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(third.generations.get(), 0);
}
#[test]
fn unknown_citations_cannot_be_published() {
    let tmp = tempfile::tempdir().unwrap();
    let c = Corpus::from_session(&session(), false);
    let mut f = Fake::new();
    f.invalid = true;
    assert!(
        research::analyse(
            &f,
            &c,
            tmp.path(),
            "task",
            "de",
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
    assert!(
        research::validate_findings(
            &[Finding {
                quote: None,
                text: "claim".into(),
                sources: vec![]
            }],
            &BTreeSet::new()
        )
        .is_err()
    );
}
#[test]
fn reports_are_separate_and_analysis_append_is_undoable() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = session();
    let original = s.document.raw().clone();
    let c = Corpus::from_session(&s, false);
    let report = research::analyse(
        &Fake::new(),
        &c,
        &tmp.path().join("steps"),
        "task",
        "en",
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    research::save_report(&tmp.path().join("reports"), &report).unwrap();
    assert_eq!(
        research::reports(&tmp.path().join("reports"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s.document.raw(), &original);
    s.execute(laut_core::Command::AppendAnalysis {
        text: report.markdown(),
    })
    .unwrap();
    assert_eq!(s.document.raw()["segments"], original["segments"]);
    s.execute(laut_core::Command::Undo).unwrap();
    assert_eq!(
        s.document.view().refined_text,
        original["refinedText"].as_str().unwrap_or("")
    );
    s.execute(laut_core::Command::Redo).unwrap();
    assert!(s.document.view().refined_text.contains("Supported draft"));
}
#[test]
fn cancelled_search_never_launches_inference() {
    let tmp = tempfile::tempdir().unwrap();
    let f = Fake::new();
    let c = Corpus::from_session(&session(), false);
    assert!(
        research::search(
            &f,
            &c,
            tmp.path(),
            "question",
            &AtomicBool::new(true),
            |_, _| {}
        )
        .is_err()
    );
    assert_eq!(f.embeds.get(), 0);
}

#[test]
fn seven_hour_source_pass_has_no_24000_character_cutoff() {
    let mut s = session();
    let mut raw = s.document.raw().clone();
    let base = raw["segments"][0].clone();
    raw["duration"] = json!(25200.);
    raw["segments"] = json!(
        (0..8400)
            .map(|i| {
                let mut v = base.clone();
                v["id"] = json!(uuid::Uuid::new_v4().to_string());
                v["start"] = json!(i as f64 * 3.);
                v["end"] = json!(i as f64 * 3. + 2.);
                v["words"] = json!([]);
                v["text"] = json!(format!("Unique evidence number {i} remains available."));
                v["originalText"] = v["text"].clone();
                v
            })
            .collect::<Vec<_>>()
    );
    s.document = laut_core::Document::from_value(raw).unwrap();
    let c = Corpus::from_session(&s, false);
    assert!(c.sources.iter().any(|s| s.text.contains("number 8399")));
    assert!(c.sources.iter().map(|s| s.text.len()).sum::<usize>() > 300_000);
}

#[test]
fn quote_must_occur_in_the_actual_cited_source() {
    let c = Corpus::from_session(&session(), false);
    let sources = std::collections::BTreeMap::from([("S1".to_string(), c.sources[0].clone())]);
    let exact = c.sources[0].text.clone();
    let mut finding = Finding {
        text: "draft".into(),
        sources: vec!["S1".into()],
        quote: Some(exact),
    };
    research::validate_quotes(&[finding.clone()], &sources).unwrap();
    finding.quote = Some("This quotation was never spoken.".into());
    assert!(research::validate_quotes(&[finding.clone()], &sources).is_err());
    finding.quote = Some(c.sources[0].text.clone());
    finding.sources = vec!["S2".into()];
    assert!(research::validate_quotes(&[finding], &sources).is_err());
}

#[test]
fn exact_years_outweigh_a_generic_semantic_match() {
    struct Ranker;
    impl Inference for Ranker {
        fn embedding_key(&self) -> &str {
            "ranking"
        }
        fn generation_key(&self) -> &str {
            "unused"
        }
        fn embed(&self, texts: &[String], query: bool, _: &AtomicBool) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| {
                    let mut v = vec![0.; 16];
                    let similarity: f32 = if query {
                        1.
                    } else if text.starts_with("Strategy") {
                        0.34
                    } else {
                        0.44
                    };
                    v[0] = similarity;
                    v[1] = (1. - similarity * similarity).sqrt();
                    v
                })
                .collect())
        }
        fn generate(
            &self,
            _: &str,
            _: &str,
            _: &[String],
            _: &str,
            _: &AtomicBool,
        ) -> Result<Vec<Finding>> {
            unreachable!()
        }
    }
    let mut c = Corpus::from_session(&session(), false);
    let base = c.sources[0].clone();
    c.sources = [
        "General benefits in 2024",
        "Strategy phases: 2015, 2021, 2024",
        "A misleading substring: 201500 202100 202400",
    ]
    .iter()
    .enumerate()
    .map(|(i, text)| {
        let mut source = base.clone();
        source.id = i.to_string();
        source.text = (*text).into();
        source
    })
    .collect();
    let cache = tempfile::tempdir().unwrap();
    let found = research::search(
        &Ranker,
        &c,
        cache.path(),
        "Welche Strategiephasen werden für 2015, 2021 und 2024 genannt?",
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(found.hits[0].source.id, "1");
    assert_eq!(found.hits[2].source.id, "2");
}
