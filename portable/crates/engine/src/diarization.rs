//! A separate local CPU pass. No ASR rerun, cloud request, or automatic naming.
use crate::{
    Session,
    jobs::{atomic_json, run, run_capture},
    media::digest,
    read_json_bytes,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DiarizationConfig {
    pub executable: PathBuf,
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
    pub ffmpeg: PathBuf,
    pub speakers: u32,
    pub threshold: f32,
    pub threads: u32,
}
impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            segmentation: PathBuf::new(),
            embedding: PathBuf::new(),
            ffmpeg: PathBuf::new(),
            speakers: 0,
            threshold: 0.6,
            threads: 4,
        }
    }
}
pub fn parse_output(text: &str, duration: f64) -> Result<Vec<laut_core::speakers::Turn>> {
    let mut turns = vec![];
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() >= 2 && fields[1] == "--" {
            ensure!(
                fields.len() >= 4 && fields[3].starts_with("speaker_"),
                "Unsupported speaker output"
            );
            turns.push(laut_core::speakers::Turn {
                start: fields[0].parse()?,
                end: fields[2].parse()?,
                speaker: fields[3].to_owned(),
            });
        }
    }
    laut_core::speakers::validated_turns(&json!(turns), duration)
}
pub fn analyze(
    session: &Session,
    config: &DiarizationConfig,
    cache: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<Value> {
    ensure!(
        config.speakers <= 100
            && config.threads > 0
            && config.threads <= 64
            && config.threshold.is_finite()
            && config.threshold > 0.
            && config.threshold <= 2.,
        "Invalid speaker settings"
    );
    for p in [
        &config.executable,
        &config.segmentation,
        &config.embedding,
        &config.ffmpeg,
    ] {
        ensure!(
            p.is_file(),
            "Speaker analysis file is missing: {}",
            p.display()
        );
    }
    let audio = session.audio_path()?;
    let r = session.document.view();
    ensure!(
        r.duration > 0. && r.duration <= 86400.,
        "Invalid recording duration"
    );
    let signature = json!({"audio":digest(&audio)?,"worker":digest(&config.executable)?,"segmentation":digest(&config.segmentation)?,"embedding":digest(&config.embedding)?,"ffmpeg":digest(&config.ffmpeg)?,"speakers":config.speakers,"threshold":config.threshold,"threads":config.threads,"duration":r.duration,"adapter":1});
    fs::create_dir_all(cache)?;
    let key = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(serde_json::to_vec(&signature)?))
    };
    let saved = cache.join(format!("{key}.json"));
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cache.join(format!("{key}.lock")))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Speaker analysis is already running")?;
    if saved.exists() {
        let mut result: Value = serde_json::from_slice(&read_json_bytes(&saved)?)?;
        ensure!(result["provenance"] == signature, "Speaker cache changed");
        laut_core::speakers::validated_turns(&result["turns"], r.duration)?;
        result["recordingID"] = json!(r.id);
        return Ok(result);
    }
    ensure!(
        fs2::available_space(cache)? > (r.duration * 32000.) as u64 + 256 * 1024 * 1024,
        "Insufficient space for speaker analysis"
    );
    let scratch = tempfile::Builder::new()
        .prefix(".speaker-")
        .tempdir_in(cache)?;
    let wav = scratch.path().join("audio.wav");
    progress(0., "Preparing speaker audio / Sprecheranalyse vorbereiten");
    let mut decode = Command::new(&config.ffmpeg);
    decode
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&audio)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&wav);
    run(decode, &scratch.path().join("decode.log"), cancel)?;
    let output = scratch.path().join("turns.txt");
    let mut worker = Command::new(&config.executable);
    #[cfg(windows)]
    {
        // The official shared package keeps its C/C++ API DLLs in ../lib.
        // Scope that search path to this child; do not change the machine PATH.
        let bin = config
            .executable
            .parent()
            .context("Missing runtime folder")?;
        let mut paths = vec![bin.to_path_buf(), bin.join("../lib")];
        if let Some(path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&path));
        }
        worker.env("PATH", std::env::join_paths(paths)?);
    }
    worker
        .arg(format!(
            "--segmentation.pyannote-model={}",
            config.segmentation.display()
        ))
        .arg(format!("--embedding.model={}", config.embedding.display()))
        .args([
            "--segmentation.provider=cpu",
            "--embedding.provider=cpu",
            "--min-duration-on=0.2",
            "--min-duration-off=0.5",
        ])
        .arg(format!("--segmentation.num-threads={}", config.threads))
        .arg(format!("--embedding.num-threads={}", config.threads))
        .arg(format!(
            "--clustering.cluster-threshold={}",
            config.threshold
        ));
    if config.speakers > 0 {
        worker.arg(format!("--clustering.num-clusters={}", config.speakers));
    }
    worker.arg(&wav);
    run_capture(
        worker,
        &scratch.path().join("speaker.log"),
        Some(&output),
        cancel,
        |p| progress(p, "Analysing voices locally / Stimmen lokal analysieren"),
    )?;
    let text = String::from_utf8(read_json_bytes(&output)?)?;
    let turns = parse_output(&text, r.duration)?;
    ensure!(
        digest(&audio)? == signature["audio"].as_str().unwrap(),
        "Audio changed during speaker analysis"
    );
    let result = json!({"version":1,"id":uuid::Uuid::new_v4().to_string(),"recordingID":r.id,"turns":turns,"provenance":signature,"rawOutput":text});
    atomic_json(&saved, &result)?;
    progress(1., "Speaker suggestions ready / Sprechervorschläge bereit");
    Ok(result)
}
