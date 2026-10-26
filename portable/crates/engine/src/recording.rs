//! Durable local capture files. Device callbacks live in laut-capture, not in this engine.
use crate::{
    Session,
    jobs::{atomic_json, run},
    library::Library,
    media::digest,
    read_json_bytes,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub kind: String,
    pub device: String,
    pub filename: String,
    pub rate: u32,
    pub frames: u64,
    pub received_frames: u64,
    pub gap_frames: u64,
    pub overlap_frames: u64,
    #[serde(default)]
    pub peak: Option<f32>,
}
impl Track {
    pub fn new(kind: &str, device: String, rate: u32) -> Self {
        Self {
            kind: kind.into(),
            device,
            filename: format!("{kind}.flac"),
            rate,
            frames: 0,
            received_frames: 0,
            gap_frames: 0,
            overlap_frames: 0,
            peak: Some(0.),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub meeting: bool,
    pub library: PathBuf,
    pub created_at: f64,
    pub state: String,
    pub tracks: Vec<Track>,
    pub error: Option<String>,
    pub result: Option<PathBuf>,
}
#[derive(Clone, Default)]
pub struct EncoderControl(Arc<Mutex<Vec<Arc<Mutex<Child>>>>>);
impl EncoderControl {
    pub fn abort(&self) {
        if let Ok(children) = self.0.lock() {
            for child in children.iter() {
                if let Ok(mut c) = child.lock() {
                    let _ = c.kill();
                }
            }
        }
    }
}
struct Encoder {
    child: Arc<Mutex<Child>>,
    input: Option<ChildStdin>,
}
impl Drop for Encoder {
    fn drop(&mut self) {
        self.input.take();
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Encoder {
    fn start(ffmpeg: &Path, path: &Path, rate: u32, control: &EncoderControl) -> Result<Self> {
        let mut command = Command::new(ffmpeg);
        command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-n",
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
                "-f",
                "f32le",
                "-ar",
                &rate.to_string(),
                "-ac",
                "1",
                "-i",
                "pipe:0",
                "-vn",
                "-ar",
                "24000",
                "-c:a",
                "flac",
                "-sample_fmt",
                "s16",
                "-compression_level",
                "5",
                "-flush_packets",
                "1",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(fs::File::create(path.with_extension("log"))?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .context("Cannot start local audio encoder")?;
        let input = child.stdin.take().context("Audio encoder input missing")?;
        let child = Arc::new(Mutex::new(child));
        control.0.lock().unwrap().push(child.clone());
        Ok(Self {
            child,
            input: Some(input),
        })
    }
    fn samples(&mut self, samples: &[f32]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for sample in samples {
            bytes.extend_from_slice(&sample.clamp(-1., 1.).to_le_bytes());
        }
        self.input
            .as_mut()
            .context("Encoder already stopped")?
            .write_all(&bytes)
            .context("Audio cannot be saved; captured data is retained")
    }
    fn silence(&mut self, mut count: u64) -> Result<()> {
        let zeros = [0f32; 4096];
        while count > 0 {
            let n = count.min(zeros.len() as u64) as usize;
            self.samples(&zeros[..n])?;
            count -= n as u64;
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        self.input.take();
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.lock().unwrap().try_wait()? {
                ensure!(
                    status.success(),
                    "Audio encoder stopped with {status}; partial files are retained"
                );
                return Ok(());
            }
            ensure!(
                started.elapsed() < Duration::from_secs(10),
                "Audio encoder did not finish; partial files are retained"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
pub struct Writer {
    pub folder: PathBuf,
    pub manifest: Manifest,
    encoders: Vec<Encoder>,
    _lock: fs::File,
    checkpoint: Instant,
}
fn lock(folder: &Path) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(folder.join("capture.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file).context("This recording is still active")?;
    Ok(file)
}
impl Writer {
    pub fn create(
        root: &Path,
        library: &Path,
        title: String,
        meeting: bool,
        tracks: Vec<Track>,
        ffmpeg: &Path,
        control: EncoderControl,
    ) -> Result<Self> {
        ensure!(!title.trim().is_empty(), "Enter a recording title");
        ensure!(
            tracks.len() == if meeting { 2 } else { 1 },
            "Invalid track configuration"
        );
        for (index, t) in tracks.iter().enumerate() {
            ensure!(
                t.kind == if index == 0 { "microphone" } else { "system" }
                    && t.filename == format!("{}.flac", t.kind)
                    && (8000..=192000).contains(&t.rate),
                "Unsupported capture format"
            );
        }
        let library = Library::open(library)?.root;
        fs::create_dir_all(root)?;
        ensure!(
            fs2::available_space(root)? > 512 * 1024 * 1024,
            "Not enough free space to record"
        );
        let id = uuid::Uuid::new_v4().to_string();
        let folder = root.join(&id);
        fs::create_dir(&folder)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700))?;
        }
        let capture_lock = lock(&folder)?;
        let manifest = Manifest {
            version: 1,
            id,
            title,
            meeting,
            library,
            created_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64() - 978307200.,
            state: "recording".into(),
            tracks,
            error: None,
            result: None,
        };
        atomic_json(&folder.join("capture.json"), &manifest)?;
        let encoders = manifest
            .tracks
            .iter()
            .map(|t| Encoder::start(ffmpeg, &folder.join(&t.filename), t.rate, &control))
            .collect::<Result<_>>()?;
        Ok(Self {
            folder,
            manifest,
            encoders,
            _lock: capture_lock,
            checkpoint: Instant::now(),
        })
    }
    pub fn write(&mut self, index: usize, at: f64, samples: &[f32]) -> Result<()> {
        ensure!(
            at.is_finite() && (0.0..=86400.).contains(&at),
            "Capture timestamp is out of range"
        );
        let t = self
            .manifest
            .tracks
            .get_mut(index)
            .context("Unknown capture track")?;
        ensure!(
            samples.len() <= t.rate as usize * 2 && samples.iter().all(|s| s.is_finite()),
            "Invalid audio packet"
        );
        let encoder = &mut self.encoders[index];
        let requested = (at * t.rate as f64).round() as u64;
        let (gap, skip) = alignment(t.frames, requested, t.rate, samples.len());
        if gap > 0 {
            encoder.silence(gap)?;
            t.frames += gap;
            t.gap_frames += gap;
        }
        encoder.samples(&samples[skip..])?;
        t.peak = Some(
            samples[skip..]
                .iter()
                .fold(t.peak.unwrap_or(0.), |peak, sample| peak.max(sample.abs())),
        );
        t.frames += (samples.len() - skip) as u64;
        t.received_frames += samples.len() as u64;
        t.overlap_frames += skip as u64;
        if self.checkpoint.elapsed() > Duration::from_secs(1) {
            self.checkpoint()?;
        }
        Ok(())
    }
    pub fn checkpoint(&mut self) -> Result<()> {
        ensure!(
            fs2::available_space(&self.folder)? > 128 * 1024 * 1024,
            "Recording stopped: disk space is low. Partial files are retained"
        );
        atomic_json(&self.folder.join("capture.json"), &self.manifest)?;
        self.checkpoint = Instant::now();
        Ok(())
    }
    pub fn finish(mut self, duration: f64, error: Option<String>) -> Result<PathBuf> {
        let mut errors = error.into_iter().collect::<Vec<_>>();
        for (track, encoder) in self.manifest.tracks.iter_mut().zip(&mut self.encoders) {
            if track.received_frames == 0 {
                errors.push(format!("No audio samples received from {}", track.kind));
            } else {
                let end = (duration.min(86400.) * track.rate as f64) as u64;
                if end > track.frames {
                    if let Err(e) = encoder.silence(end - track.frames) {
                        errors.push(e.to_string());
                    } else {
                        track.frames = end;
                    }
                }
            }
            if let Err(e) = encoder.finish() {
                errors.push(e.to_string());
            }
        }
        self.manifest.state = if errors.is_empty() {
            "stopped"
        } else {
            "interrupted"
        }
        .into();
        self.manifest.error = (!errors.is_empty()).then(|| errors.join("; "));
        atomic_json(&self.folder.join("capture.json"), &self.manifest)?;
        if let Some(error) = &self.manifest.error {
            bail!("{error}\n{}", self.folder.display());
        }
        Ok(self.folder.clone())
    }
}
/// Ignore small callback jitter; retain actual gaps and trim duplicate/overlapping buffers.
pub fn alignment(written: u64, requested: u64, rate: u32, count: usize) -> (u64, usize) {
    let tolerance = rate as u64 / 50;
    if requested > written + tolerance {
        (requested - written, 0)
    } else if written > requested + tolerance {
        (0, (written - requested).min(count as u64) as usize)
    } else {
        (0, 0)
    }
}
pub fn list(root: &Path) -> Result<Vec<(PathBuf, Manifest)>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut captures = vec![];
    for e in fs::read_dir(root)? {
        let e = e?;
        if e.file_type()?.is_dir()
            && uuid::Uuid::parse_str(&e.file_name().to_string_lossy()).is_ok()
        {
            let path = e.path();
            let m: Manifest =
                serde_json::from_slice(&read_json_bytes(&path.join("capture.json"))?)?;
            if m.result.is_none() {
                captures.push((path, m));
            }
        }
    }
    captures.sort_by(|a, b| b.1.created_at.total_cmp(&a.1.created_at));
    Ok(captures)
}
fn probe(ffprobe: &Path, path: &Path) -> Result<f64> {
    let mut cmd = Command::new(ffprobe);
    cmd.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
    ])
    .arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output()?;
    ensure!(out.status.success(), "Cannot inspect captured audio");
    let duration = String::from_utf8(out.stdout)?.trim().parse::<f64>()?;
    ensure!(
        duration.is_finite() && duration > 0. && duration <= 86400.,
        "Invalid capture duration"
    );
    Ok(duration)
}
pub fn publish(
    folder: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    recover: bool,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    ensure!(
        !fs::symlink_metadata(folder)?.file_type().is_symlink(),
        "Capture folder is a symlink"
    );
    let _lock = lock(folder)?;
    let mut m: Manifest = serde_json::from_slice(&read_json_bytes(&folder.join("capture.json"))?)?;
    ensure!(
        m.version == 1
            && folder.file_name().is_some_and(|n| n == m.id.as_str())
            && uuid::Uuid::parse_str(&m.id).is_ok(),
        "Invalid capture identity"
    );
    let library = Library::open(&m.library)?;
    let target = library
        .root
        .join(m.id.to_uppercase())
        .join("recording.json");
    if target.exists() {
        let s = Session::open(&target)?;
        ensure!(
            s.document.raw()["portableCapture"]["id"] == m.id,
            "Capture identity conflict"
        );
        m.result = Some(target.clone());
        m.state = "saved".into();
        atomic_json(&folder.join("capture.json"), &m)?;
        return Ok(target);
    }
    ensure!(
        recover || m.state == "stopped",
        "Recording was interrupted. Review and recover its partial audio explicitly"
    );
    let staging = tempfile::Builder::new()
        .prefix(".prepare-")
        .tempdir_in(folder)?;
    let mut tracks = vec![];
    let mut warnings = vec![];
    if let Some(error) = &m.error {
        warnings.push(error.clone());
    }
    if recover && m.state != "stopped" {
        warnings.push("Recovered partial recording; verify completeness before quoting".into());
    }
    for track in &m.tracks {
        if track.peak == Some(0.) && track.received_frames > 0 {
            warnings.push(format!(
                "{}: captured samples contain only silence; check mute/input settings",
                track.kind
            ));
        }
        ensure!(
            matches!(track.kind.as_str(), "microphone" | "system")
                && track.filename == format!("{}.flac", track.kind),
            "Invalid capture track path"
        );
        let source = match crate::media::resolve(folder, &track.filename) {
            Ok(source) => source,
            Err(error) if recover => {
                warnings.push(format!("{} unavailable: {error:#}", track.kind));
                continue;
            }
            Err(error) => return Err(error),
        };
        let dest = staging.path().join(&track.filename);
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-v", "error", "-nostdin", "-n", "-i"])
            .arg(&source)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-ar",
                "24000",
                "-ac",
                "1",
                "-c:a",
                "flac",
                "-sample_fmt",
                "s16",
            ])
            .arg(&dest);
        let result = run(
            cmd,
            &folder.join(format!("{}-finalize.log", track.kind)),
            cancel,
        )
        .and_then(|_| probe(ffprobe, &dest));
        match result {
            Ok(d) => {
                tracks.push(json!({"kind":track.kind,"device":track.device,"filename":track.filename,"sha256":digest(&dest)?,"duration":d,"gapFrames":track.gap_frames,"overlapFrames":track.overlap_frames,"inputSampleRate":track.rate,"peak":track.peak}));
            }
            Err(e) if recover && !cancel.load(Ordering::Relaxed) => {
                warnings.push(format!("{} unavailable: {e:#}", track.kind));
            }
            Err(e) => return Err(e),
        }
    }
    ensure!(
        !tracks.is_empty(),
        "No recoverable audio; original files are retained"
    );
    ensure!(
        recover || tracks.len() == if m.meeting { 2 } else { 1 },
        "Missing capture track"
    );
    let filename = if tracks.len() == 1 {
        tracks[0]["filename"].as_str().unwrap().to_owned()
    } else {
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-v", "error", "-nostdin", "-n"]);
        for t in &tracks {
            cmd.arg("-i")
                .arg(staging.path().join(t["filename"].as_str().unwrap()));
        }
        cmd.args(["-filter_complex","amix=inputs=2:duration=longest:dropout_transition=0:normalize=0,alimiter=limit=0.95:level=0:latency=1","-ar","24000","-ac","1","-c:a","aac","-b:a","96k","-movflags","+faststart"]).arg(staging.path().join("audio.m4a"));
        run(cmd, &folder.join("mix.log"), cancel)?;
        "audio.m4a".into()
    };
    let audio_hash = digest(&staging.path().join(&filename))?;
    let duration = probe(ffprobe, &staging.path().join(&filename))?;
    let document = json!({"id":m.id.to_uppercase(),"title":m.title,"createdAt":m.created_at,"kind":"meeting","audioFilename":filename,"duration":duration,"segments":[],"speakers":[],"notes":"","refinedText":"","state":"ready","portableCapture":{"version":1,"id":m.id,"mode":if m.meeting{"meeting"}else{"microphone"},"tracks":tracks,"audioSha256":audio_hash,"recovered":recover,"warnings":warnings}});
    let path = staging.path().join("recording.json");
    crate::write_new(&path, &serde_json::to_vec_pretty(&document)?)?;
    let saved = library.import(&Session::open(&path)?)?;
    m.result = Some(saved.session.source.clone());
    m.state = "saved".into();
    atomic_json(&folder.join("capture.json"), &m)?;
    // Only remove our encoded working copies after the library has verified every permanent asset.
    for track in &m.tracks {
        if !recover
            && saved
                .session
                .document
                .view()
                .media_names()
                .contains(&track.filename.as_str())
        {
            let original = folder.join(&track.filename);
            if original.is_file() {
                fs::remove_file(original)?;
            }
        }
    }
    Ok(saved.session.source)
}
