//! Portable document and editing semantics. No UI, files, processes or inference libraries.
mod commands;
pub mod conference;
pub mod speakers;
mod transcript;
pub use commands::{Command, execute};
pub use transcript::*;

use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

/// The original JSON remains authoritative. Typed projections never rewrite unknown fields,
/// Swift reference-date numbers, original words, conference attachments or existing history.
#[derive(Clone, Debug)]
pub struct Document {
    raw: Value,
}
impl Document {
    pub fn from_value(raw: Value) -> Result<Self> {
        let doc = Self { raw };
        doc.validate()?;
        Ok(doc)
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::from_value(serde_json::from_slice(bytes)?)
    }
    pub fn raw(&self) -> &Value {
        &self.raw
    }
    pub fn view(&self) -> Recording {
        // Only constructed or mutated after validation.
        serde_json::from_value(self.raw.clone()).expect("validated recording")
    }
    pub fn validate(&self) -> Result<()> {
        let r: Recording = serde_json::from_value(self.raw.clone())?;
        ensure!(uuid::Uuid::parse_str(&r.id).is_ok(), "Invalid recording ID");
        ensure!(
            r.duration.is_finite() && r.duration >= 0.,
            "Invalid duration"
        );
        let mut ids = HashSet::new();
        for s in &r.segments {
            ensure!(
                uuid::Uuid::parse_str(&s.id).is_ok() && ids.insert(&s.id),
                "Invalid or duplicate segment ID"
            );
            ensure!(
                s.start.is_finite() && s.end.is_finite() && s.start >= 0. && s.end >= s.start,
                "Invalid segment time"
            );
            for w in &s.words {
                ensure!(
                    w.start.is_finite() && w.end.is_finite() && w.start >= 0. && w.end >= w.start,
                    "Invalid word time"
                );
            }
        }
        let mut speakers = HashSet::new();
        ensure!(
            r.speakers
                .iter()
                .all(|s| !s.id.is_empty() && speakers.insert(&s.id)),
            "Invalid speaker IDs"
        );
        // A non-null unknown speaker is retained and displayed as unassigned, just as in Swift.
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub duration: f64,
    pub segments: Vec<Segment>,
    pub speakers: Vec<Speaker>,
    #[serde(default)]
    pub audio_filename: Option<String>,
    #[serde(default)]
    pub secondary_audio_filename: Option<String>,
    #[serde(default)]
    pub portable_capture: Option<CaptureMedia>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub refined_text: String,
    #[serde(default)]
    pub conference: Option<Conference>,
    #[serde(default)]
    pub source: Option<Source>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub original_text: String,
    #[serde(rename = "speakerID")]
    pub speaker_id: Option<String>,
    #[serde(default)]
    pub speaker_locked: bool,
    pub allows_paragraph_grouping: Option<bool>,
    #[serde(default)]
    pub words: Vec<Word>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Speaker {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Conference {
    #[serde(default)]
    pub vocabulary: String,
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    #[serde(default)]
    pub documents: Vec<Attachment>,
    #[serde(default)]
    pub slides: Vec<Slide>,
    pub ranges: Option<Vec<Exclusion>>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Chapter {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub origin: String,
    pub start: f64,
    pub title: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub title: String,
    #[serde(default)]
    pub pages: Vec<Page>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Page {
    #[serde(default, rename = "ocrText")]
    pub ocr_text: Option<String>,
    #[serde(default)]
    pub extraction: Option<String>,
    pub number: u32,
    pub text: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Slide {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "matchEvidence")]
    pub match_evidence: Option<String>,
    #[serde(rename = "documentID")]
    pub document_id: String,
    pub page: u32,
    pub start: f64,
    pub confirmed: bool,
    pub useful: Option<bool>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Exclusion {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub evidence: String,
    pub start: f64,
    pub end: f64,
    pub confirmed: bool,
    pub excluded: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub url: String,
    pub clip_start: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CaptureMedia {
    pub tracks: Vec<CaptureTrack>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct CaptureTrack {
    pub filename: String,
}

impl Recording {
    pub fn speaker_name(&self, id: Option<&str>) -> &str {
        self.speakers
            .iter()
            .find(|s| Some(s.id.as_str()) == id)
            .map(|s| s.name.as_str())
            .unwrap_or("—")
    }
    pub fn source_time(&self, local: f64) -> f64 {
        local
            + self
                .source
                .as_ref()
                .and_then(|s| s.clip_start)
                .unwrap_or(0.)
    }
    pub fn media_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self
            .audio_filename
            .iter()
            .chain(self.secondary_audio_filename.iter())
            .map(String::as_str)
            .collect();
        if let Some(c) = &self.conference {
            names.extend(c.documents.iter().map(|d| d.filename.as_str()));
        }
        if let Some(capture) = &self.portable_capture {
            names.extend(capture.tracks.iter().map(|t| t.filename.as_str()));
        }
        names.sort();
        names.dedup();
        names
    }
}
