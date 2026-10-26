//! Durable local queue. Startup only lists jobs; execution always requires an explicit action.
use crate::{
    Session,
    diarization::DiarizationConfig,
    downloads::{self, UrlImport},
    jobs::{JobConfig, atomic_json, transcribe},
    library::Library,
    read_json_bytes,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueItem {
    pub version: u32,
    #[serde(default)]
    pub created_at_ms: u64,
    pub id: String,
    pub config: JobConfig,
    #[serde(default)]
    pub url_import: Option<UrlImport>,
    #[serde(default)]
    pub recording_id: Option<String>,
    pub library: PathBuf,
    pub diarization: Option<DiarizationConfig>,
    pub state: String,
    pub error: Option<String>,
    pub result: Option<PathBuf>,
}
#[derive(Default, Debug, Serialize)]
pub struct BatchResult {
    pub completed: usize,
    pub failed: Vec<String>,
    pub last: Option<PathBuf>,
    pub paused: bool,
}
#[derive(Clone)]
pub struct Queue {
    pub root: PathBuf,
}
impl Queue {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: if root.is_absolute() {
                root
            } else {
                std::env::current_dir().unwrap_or_default().join(root)
            },
        }
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        Ok(self
            .root
            .join(format!("{}.json", uuid::Uuid::parse_str(id)?)))
    }
    fn lock(&self, id: &str) -> Result<fs::File> {
        uuid::Uuid::parse_str(id)?;
        fs::create_dir_all(&self.root)?;
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(format!("{id}.lock")))?;
        for attempt in 0..=4 {
            match fs2::FileExt::try_lock_exclusive(&f) {
                Ok(()) => return Ok(f),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && attempt < 4 => {
                    // A just-finished run can release its OS lock a few milliseconds
                    // after its result is visible. Keep real contention fast.
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(error) => return Err(error).context("Job is already running"),
            }
        }
        unreachable!("the lock loop always returns")
    }
    pub fn list(&self) -> Result<Vec<QueueItem>> {
        if !self.root.exists() {
            return Ok(vec![]);
        }
        let mut jobs = vec![];
        for e in fs::read_dir(&self.root)? {
            let e = e?;
            if e.path().extension().is_some_and(|s| s == "json") {
                let job: QueueItem = serde_json::from_slice(&read_json_bytes(&e.path())?)
                    .with_context(|| format!("Invalid queue file: {}", e.path().display()))?;
                ensure!(
                    job.version == 1 && self.path(&job.id)? == e.path(),
                    "Invalid queue identity"
                );
                jobs.push(job);
            }
        }
        jobs.sort_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms).then(a.id.cmp(&b.id)));
        Ok(jobs)
    }
    pub fn enqueue(
        &self,
        config: JobConfig,
        library: PathBuf,
        diarization: Option<DiarizationConfig>,
    ) -> Result<QueueItem> {
        self.enqueue_source(config, library, diarization, None, None)
    }
    pub fn enqueue_url(
        &self,
        config: JobConfig,
        library: PathBuf,
        diarization: Option<DiarizationConfig>,
        mut source: UrlImport,
    ) -> Result<QueueItem> {
        source.normalize()?;
        self.enqueue_source(config, library, diarization, Some(source), None)
    }
    /// Transcribe an already saved recording without creating a second library project.
    pub fn enqueue_recording(
        &self,
        mut config: JobConfig,
        library: PathBuf,
        id: &str,
        diarization: Option<DiarizationConfig>,
    ) -> Result<QueueItem> {
        let loaded = Library::open(&library)?.load(id)?;
        ensure!(
            loaded.session.document.view().segments.is_empty(),
            "This recording already has a transcript"
        );
        ensure!(
            loaded.session.document.raw()["portableCapture"].is_object(),
            "Not a local capture"
        );
        config.input = loaded.session.audio_path()?;
        self.enqueue_source(config, library, diarization, None, Some(id.to_owned()))
    }
    fn enqueue_source(
        &self,
        mut config: JobConfig,
        library: PathBuf,
        mut diarization: Option<DiarizationConfig>,
        url_import: Option<UrlImport>,
        recording_id: Option<String>,
    ) -> Result<QueueItem> {
        let library = Library::open(&library)?.root;
        if url_import.is_none() {
            config.input = fs::canonicalize(&config.input)?;
            ensure!(config.input.is_file(), "Source is not a file");
        } else {
            config.input = PathBuf::new();
        }
        let cwd = std::env::current_dir()?;
        for path in [
            &mut config.whisper,
            &mut config.model,
            &mut config.ffmpeg,
            &mut config.ffprobe,
        ] {
            if !path.is_absolute() {
                *path = cwd.join(&*path);
            }
        }
        if let Some(config) = &mut diarization {
            for path in [
                &mut config.executable,
                &mut config.segmentation,
                &mut config.embedding,
                &mut config.ffmpeg,
            ] {
                if !path.is_absolute() {
                    *path = cwd.join(&*path);
                }
            }
        }
        // A global enqueue lock prevents duplicate submissions from two windows.
        let _lock = self.lock("00000000-0000-4000-8000-000000000000")?;
        ensure!(
            !self.list()?.iter().any(|j| j.result.is_none()
                && j.library == library
                && match (&j.url_import, &url_import) {
                    (Some(a), Some(b)) => a.url == b.url && a.first_seconds == b.first_seconds,
                    (None, None) => j.config.input == config.input,
                    _ => false,
                }
                && j.config.model == config.model
                && j.config.language == config.language
                && j.config.compact_audio == config.compact_audio
                && j.config.use_gpu == config.use_gpu
                && j.config.prompt == config.prompt
                && j.diarization == diarization),
            "This source is already in the queue"
        );
        let id = uuid::Uuid::new_v4().to_string();
        config.output = self.root.join(format!("job-{id}"));
        let job = QueueItem {
            version: 1,
            created_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64,
            id,
            config,
            url_import,
            recording_id,
            library,
            diarization,
            state: "queued".into(),
            error: None,
            result: None,
        };
        atomic_json(&self.path(&job.id)?, &job)?;
        Ok(job)
    }
    pub fn run(
        &self,
        id: &str,
        cancel: &AtomicBool,
        progress: impl Fn(f32, &str),
    ) -> Result<PathBuf> {
        let _lock = self.lock(id)?;
        let path = self.path(id)?;
        let mut job: QueueItem = serde_json::from_slice(&read_json_bytes(&path)?)?;
        ensure!(
            job.version == 1
                && job.id == id
                && job.config.output == self.root.join(format!("job-{id}")),
            "Invalid queue job"
        );
        if let Some(result) = &job.result {
            ensure!(result.is_file(), "Completed project is missing");
            return Ok(result.clone());
        }
        job.state = "running".into();
        job.error = None;
        atomic_json(&path, &job)?;
        let result = (|| -> Result<PathBuf> {
            let mut captured = if let Some(id) = &job.recording_id {
                let library = Library::open(&job.library)?;
                let loaded = library.load(id)?;
                if loaded.session.document.raw()["portableTranscription"]["job"] == job.id {
                    return Ok(loaded.session.source);
                }
                ensure!(
                    loaded.session.document.view().segments.is_empty(),
                    "Recording was transcribed elsewhere; no text was overwritten"
                );
                ensure!(
                    loaded.session.audio_path()? == job.config.input
                        && loaded.session.document.raw()["portableCapture"].is_object(),
                    "Capture source changed"
                );
                Some((library, loaded))
            } else {
                None
            };
            let downloaded = if let Some(source) = &job.url_import {
                let folder = self.root.join(format!("download-{id}"));
                let data = downloads::download(
                    source,
                    &folder,
                    &job.config.ffmpeg,
                    &job.config.ffprobe,
                    cancel,
                    |p, s| progress(p * 0.15, s),
                )?;
                job.config.input = folder.join(&data.filename);
                atomic_json(&path, &job)?;
                Some(data)
            } else {
                None
            };
            let start = if downloaded.is_some() { 0.15 } else { 0. };
            let end = if job.diarization.is_some() { 0.8 } else { 1. };
            let recording = transcribe(&job.config, cancel, |p, s| {
                progress(start + p * (end - start), s)
            })?;
            let mut session = Session::open(&recording)?;
            if let Some((library, loaded)) = captured.as_mut() {
                let result = session.document.raw();
                let mut raw = loaded.session.document.raw().clone();
                for key in [
                    "segments",
                    "speakers",
                    "engine",
                    "state",
                    "transcriptionProgress",
                ] {
                    if let Some(value) = result.get(key) {
                        raw[key] = value.clone();
                    }
                }
                raw["portableTranscription"] = serde_json::json!({"job":job.id,"sourceSha256":result["portableImport"]["sourceSha256"]});
                loaded.session.document = laut_core::Document::from_value(raw)?;
                library.save(&mut loaded.session, &mut loaded.revision)?;
                session = loaded.session.clone();
            }
            if let Some(data) = downloaded {
                let mut raw = session.document.raw().clone();
                raw["title"] = data.source["originalTitle"].clone();
                raw["source"] = data.source;
                if raw["source"]["chapters"]
                    .as_array()
                    .is_some_and(|c| !c.is_empty())
                {
                    raw["conference"] = serde_json::json!({"documents":[],"chapters":raw["source"]["chapters"],"slides":[],"ranges":[],"language":job.config.language,"vocabulary":job.config.prompt});
                }
                raw["portableImport"]["url"] = raw["source"]["url"].clone();
                raw["portableImport"]["originalKeptAtSource"] = serde_json::json!(false);
                raw["portableImport"]["downloadSha256"] = serde_json::json!(data.sha256);
                session.document = laut_core::Document::from_value(raw)?;
            }
            // Save suggestions separately; the user reviews voice/name mapping before applying.
            if let Some(config) = &job.diarization
                && let Err(error) = crate::diarization::analyze(
                    &session,
                    config,
                    &self.root.join("speaker-analyses"),
                    cancel,
                    |p, s| progress(0.8 + p * 0.2, s),
                )
            {
                if cancel.load(Ordering::Relaxed) {
                    return Err(error);
                }
                // The optional voice pass must not hide a successfully transcribed talk.
                job.error = Some(format!(
                    "Transkript fertig; Sprecheranalyse bitte erneut prüfen / Transcript ready; retry speaker analysis: {error:#}"
                ));
            }
            let library = Library::open(&job.library)?;
            if captured.is_some() {
                return Ok(session.source);
            }
            let id = session.document.view().id;
            let dest = library.root.join(id.to_uppercase()).join("recording.json");
            if dest.exists() {
                let loaded = library.load(&id)?;
                ensure!(
                    loaded.session.document.raw()["portableImport"]
                        == session.document.raw()["portableImport"],
                    "Existing library project has different source provenance"
                );
                return Ok(loaded.session.source);
            }
            Ok(library.import(&session)?.session.source)
        })();
        match &result {
            Ok(p) => {
                job.state = "complete".into();
                job.result = Some(p.clone());
            }
            Err(e) => {
                job.state = if cancel.load(Ordering::Relaxed) {
                    "paused"
                } else {
                    "failed"
                }
                .into();
                job.error = Some(format!("{e:#}"));
            }
        }
        atomic_json(&path, &job)?;
        result
    }
    /// Retry uses the currently selected downloader/runtime; ASR settings and checkpoints stay fixed.
    pub fn download_programs(
        &self,
        id: &str,
        yt_dlp: &std::path::Path,
        deno: &std::path::Path,
    ) -> Result<()> {
        let _lock = self.lock(id)?;
        let path = self.path(id)?;
        let mut job: QueueItem = serde_json::from_slice(&read_json_bytes(&path)?)?;
        ensure!(job.version == 1 && job.id == id, "Invalid queue identity");
        if job.result.is_none()
            && let Some(source) = &mut job.url_import
        {
            ensure!(
                yt_dlp.is_file() && deno.is_file(),
                "Configure yt-dlp and Deno under Models / programs"
            );
            source.yt_dlp = fs::canonicalize(yt_dlp)?;
            source.deno = fs::canonicalize(deno)?;
            atomic_json(&path, &job)?;
        }
        Ok(())
    }
    /// Failures belong to individual jobs. A bad link must not block later imports.
    pub fn run_batch(
        &self,
        ids: &[String],
        cancel: &AtomicBool,
        progress: impl Fn(f32, &str),
    ) -> BatchResult {
        let mut result = BatchResult::default();
        for (index, id) in ids.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                result.paused = true;
                break;
            }
            match self.run(id, cancel, |p, s| {
                progress(p, &format!("{}/{} · {s}", index + 1, ids.len()))
            }) {
                Ok(path) => {
                    result.completed += 1;
                    result.last = Some(path);
                }
                Err(error) => {
                    if cancel.load(Ordering::Relaxed) {
                        result.paused = true;
                        break;
                    }
                    result.failed.push(format!("{id}: {error:#}"));
                }
            }
        }
        result
    }
    /// Explicit cleanup after a durable library copy; retain job history and the external original.
    pub fn cleanup(&self, id: &str) -> Result<()> {
        let _lock = self.lock(id)?;
        let job: QueueItem = serde_json::from_slice(&read_json_bytes(&self.path(id)?)?)?;
        ensure!(
            job.config.output == self.root.join(format!("job-{id}")),
            "Invalid job path"
        );
        let result = job
            .result
            .context("Only completed imports can be cleaned")?;
        let session = Session::open(&result)?;
        let audio = session.audio_path()?;
        let provenance = if job.recording_id.is_some() {
            "portableCapture"
        } else {
            "portableImport"
        };
        let expected = session.document.raw()[provenance]["audioSha256"]
            .as_str()
            .context("Missing audio hash")?;
        ensure!(
            crate::media::digest(&audio)? == expected,
            "Library audio changed or is missing"
        );
        if job.url_import.is_some() {
            let folder = self.root.join(format!("download-{id}"));
            if folder.exists() {
                ensure!(
                    !fs::symlink_metadata(&folder)?.file_type().is_symlink(),
                    "Download folder is a symlink"
                );
                fs::remove_dir_all(folder)?;
            }
        }
        if job.config.output.exists() {
            ensure!(
                !fs::symlink_metadata(&job.config.output)?
                    .file_type()
                    .is_symlink(),
                "Job folder is a symlink"
            );
            fs::remove_dir_all(job.config.output)?;
        }
        Ok(())
    }
}
