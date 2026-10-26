//! Local preferences and recoverable editor drafts, separate from recording libraries.
use crate::{Session, read_json_bytes};
use anyhow::{Context, Result, ensure};
use laut_core::Document;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const PLAYBACK_SPEEDS: [f64; 7] = [0.5, 0.75, 1., 1.25, 1.5, 1.75, 2.];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub english: bool,
    pub theme: String,
    pub recording_microphone: String,
    pub recording_output: String,
    pub recording_transcribe: bool,
    pub library: Option<PathBuf>,
    pub last_project: Option<PathBuf>,
    pub worker_paths: [String; 4],
    pub transcription_language: String,
    pub use_gpu: bool,
    pub diarizer_paths: [String; 3],
    pub speaker_count: u32,
    pub compact_audio: bool,
    pub vocabulary: String,
    pub download_paths: [String; 2],
    pub pdf: crate::documents::PdfConfig,
    pub research: crate::research::ResearchConfig,
    pub reading_font_size: f32,
    pub follow_playback: bool,
    pub playback_speed: f64,
    pub show_navigation: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            english: false,
            theme: "system".into(),
            recording_microphone: String::new(),
            recording_output: String::new(),
            recording_transcribe: true,
            library: None,
            last_project: None,
            worker_paths: Default::default(),
            transcription_language: "auto".into(),
            use_gpu: false,
            diarizer_paths: Default::default(),
            speaker_count: 0,
            compact_audio: true,
            vocabulary: String::new(),
            download_paths: Default::default(),
            pdf: Default::default(),
            research: Default::default(),
            reading_font_size: 16.,
            follow_playback: true,
            playback_speed: 1.,
            show_navigation: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Draft {
    pub version: u32,
    pub source: PathBuf,
    pub document: Value,
    pub dirty: bool,
    pub selected: Option<String>,
    pub texts: std::collections::BTreeMap<String, String>,
    pub title: String,
    pub notes: String,
    pub speaker_names: Vec<(String, String)>,
}
impl Draft {
    pub fn session(&self) -> Result<Session> {
        ensure!(
            self.version == 1 && self.source.is_absolute(),
            "Unsupported recovery draft"
        );
        Ok(Session {
            document: Document::from_value(self.document.clone())?,
            source: self.source.clone(),
            dirty: self.dirty,
        })
    }
}
pub type DraftCatalog = (Vec<(String, Draft)>, Vec<String>);

#[derive(Clone)]
pub struct StateStore {
    pub root: PathBuf,
}
impl StateStore {
    pub fn draft_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
    pub fn default_path() -> Result<PathBuf> {
        if let Some(path) = std::env::var_os("LAUT_PORTABLE_STATE_DIR") {
            let path = PathBuf::from(path);
            ensure!(path.is_absolute(), "State folder must be absolute");
            return Ok(path);
        }
        let base = crate::library::default_root()?;
        Ok(base.with_file_name(if cfg!(target_os = "linux") {
            "laut-portable-state"
        } else {
            "LautPortableState"
        }))
    }
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn preferences(&self) -> Result<Preferences> {
        let path = self.root.join("preferences.json");
        if !path.exists() {
            return Ok(Preferences::default());
        }
        let mut settings: Preferences = serde_json::from_slice(&read_json_bytes(&path)?)?;
        ensure!(
            settings.version == 1,
            "Unsupported preferences version; file retained"
        );
        if !PLAYBACK_SPEEDS.contains(&settings.playback_speed) {
            settings.playback_speed = 1.;
        }
        Ok(settings)
    }
    pub fn save_preferences(&self, settings: &Preferences) -> Result<()> {
        ensure!(settings.version == 1, "Unsupported preferences version");
        settings.research.validate_persistence()?;
        self.prepare()?;
        atomic(
            &self.root.join("preferences.json"),
            &serde_json::to_vec_pretty(settings)?,
        )
    }
    fn prepare(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        ensure!(
            !fs::symlink_metadata(&self.root)?.file_type().is_symlink(),
            "State folder is a symlink"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    fn draft_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self
            .root
            .join(format!("draft-{}.json", uuid::Uuid::parse_str(id)?)))
    }
    pub fn save_draft(&self, id: &str, draft: &Draft) -> Result<()> {
        draft.session()?;
        self.prepare()?;
        atomic(&self.draft_path(id)?, &serde_json::to_vec(draft)?)
    }
    pub fn remove_draft(&self, id: &str) -> Result<()> {
        match fs::remove_file(self.draft_path(id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    /// A recovered draft is retained in an archive; the new editor uses its own draft ID.
    pub fn archive_draft(&self, id: &str, expected: &Draft) -> Result<()> {
        let source = self.draft_path(id)?;
        let actual: Draft = serde_json::from_slice(&read_json_bytes(&source)?)?;
        ensure!(
            &actual == expected,
            "Draft changed in another window; original retained"
        );
        let archive = self.root.join("recovered");
        fs::create_dir_all(&archive)?;
        ensure!(
            !fs::symlink_metadata(&archive)?.file_type().is_symlink(),
            "Invalid draft archive"
        );
        fs::rename(
            source,
            archive.join(format!("draft-{}.json", Self::draft_id())),
        )?;
        Ok(())
    }
    pub fn drafts(&self) -> Result<DraftCatalog> {
        let mut drafts = vec![];
        let mut warnings = vec![];
        if !self.root.exists() {
            return Ok((drafts, warnings));
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name
                .strip_prefix("draft-")
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            let result = (|| -> Result<Draft> {
                uuid::Uuid::parse_str(id)?;
                ensure!(!entry.file_type()?.is_symlink(), "Draft is a symlink");
                let draft: Draft = serde_json::from_slice(&read_json_bytes(&entry.path())?)?;
                draft.session()?;
                Ok(draft)
            })();
            match result {
                Ok(draft) => drafts.push((id.to_owned(), draft)),
                Err(e) => warnings.push(format!("{name}: {e}")),
            }
        }
        drafts.sort_by(|a, b| a.0.cmp(&b.0));
        Ok((drafts, warnings))
    }
}
pub(crate) fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() as u64 <= crate::MAX_JSON_BYTES,
        "Local recovery exceeds the document size limit; previous draft retained"
    );
    if let Ok(meta) = fs::symlink_metadata(path) {
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Invalid state file"
        );
    }
    let parent = path.parent().context("Missing parent folder")?;
    ensure!(
        fs2::available_space(parent)? > bytes.len() as u64 + 16 * 1024 * 1024,
        "Insufficient space for local recovery"
    );
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
