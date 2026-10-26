//! Local whisper.cpp adapter. Network access is separate, in the explicit package installer and URL downloader.
//! Five-minute checkpoints keep committed segments when a worker is stopped or restarted.
use crate::{media::digest, read_json_bytes};
use anyhow::{Context, Result, bail, ensure};
use command_group::CommandGroup;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct JobConfig {
    pub input: PathBuf,
    pub output: PathBuf,
    pub whisper: PathBuf,
    pub model: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub language: String,
    pub use_gpu: bool,
    #[serde(default = "yes")]
    pub compact_audio: bool,
    #[serde(default)]
    pub prompt: String,
}
fn yes() -> bool {
    true
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    version: u32,
    signature: Value,
    next: f64,
    duration: f64,
    recording: Value,
}

pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.flush()?;
    ensure!(
        temp.as_file().metadata()?.len() <= crate::MAX_JSON_BYTES,
        "Job document exceeds the local size limit; previous checkpoint retained"
    );
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub(crate) fn run(command: Command, log: &Path, cancel: &AtomicBool) -> Result<()> {
    run_capture(command, log, None, cancel, |_| {})
}
pub(crate) fn run_capture(
    mut command: Command,
    log: &Path,
    stdout: Option<&Path>,
    cancel: &AtomicBool,
    progress: impl Fn(f32),
) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Cancelled; completed chunks are retained"
    );
    let stderr = fs::File::create(log)?;
    command
        .stdin(Stdio::null())
        .stdout(match stdout {
            Some(p) => Stdio::from(fs::File::create(p)?),
            None => Stdio::null(),
        })
        .stderr(stderr);
    let mut group = command.group();
    #[cfg(windows)]
    group.creation_flags(0x08000000).kill_on_drop(true);
    let mut child = group.spawn().context("Could not start local worker")?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Cancelled; completed chunks are retained");
        }
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                use std::io::{Read, Seek, SeekFrom};
                let mut file = fs::File::open(log)?;
                let length = file.metadata()?.len();
                file.seek(SeekFrom::Start(length.saturating_sub(8192)))?;
                let mut bytes = vec![];
                file.take(8192).read_to_end(&mut bytes)?;
                bail!(
                    "Local worker failed ({status}): {}",
                    String::from_utf8_lossy(&bytes)
                );
            }
            return Ok(());
        }
        // Read just the tail: sherpa emits progress on stderr even for very long recordings.
        if let Ok(mut f) = fs::File::open(log) {
            use std::io::{Read, Seek, SeekFrom};
            let len = f.metadata()?.len();
            f.seek(SeekFrom::Start(len.saturating_sub(512)))?;
            let mut tail = String::new();
            let _ = f.read_to_string(&mut tail);
            if let Some(p) = tail.lines().rev().find_map(|s| {
                s.strip_prefix("progress ")?
                    .trim()
                    .strip_suffix('%')?
                    .parse::<f32>()
                    .ok()
            }) {
                progress((p / 100.).clamp(0., 1.));
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

pub fn transcribe(
    config: &JobConfig,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<PathBuf> {
    ensure!(
        !config.language.is_empty()
            && config
                .language
                .bytes()
                .all(|c| c.is_ascii_alphabetic() || c == b'-'),
        "Invalid language code"
    );
    for path in [
        &config.input,
        &config.whisper,
        &config.model,
        &config.ffmpeg,
        &config.ffprobe,
    ] {
        ensure!(
            path.is_file(),
            "Required local file is missing: {}",
            path.display()
        );
    }
    ensure!(
        config.prompt.len() <= 4000,
        "Vocabulary prompt exceeds 4000 bytes"
    );
    progress(0., "Checking local files");
    let mut signature = json!({"audio":digest(&config.input)?,"model":digest(&config.model)?,"whisper":digest(&config.whisper)?,"ffmpeg":digest(&config.ffmpeg)?,"ffprobe":digest(&config.ffprobe)?,"language":config.language,"gpu":config.use_gpu,"chunk":300});
    let existing = config.output.exists();
    let checkpoint_path = config.output.join("checkpoint.json");
    let request_path = config.output.join("request.json");
    // Old checkpoints retain their original adapter signature and output format.
    let legacy = existing && checkpoint_path.is_file() && !request_path.exists();
    if !legacy {
        signature["adapter"] = json!(2);
        signature["compactAudio"] = json!(config.compact_audio);
    }
    if !config.prompt.trim().is_empty() {
        signature["prompt"] = json!(config.prompt.trim());
    }
    if existing {
        ensure!(
            checkpoint_path.is_file() || request_path.is_file(),
            "Existing folder is not a resumable Laut job; choose a new folder"
        );
    }
    if !existing {
        fs::create_dir(&config.output)?;
    }
    let lockfile = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(config.output.join(".job.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lockfile).context("This job is already running")?;
    let mut checkpoint: Checkpoint;
    if request_path.exists() {
        let request: Value = serde_json::from_slice(&read_json_bytes(&request_path)?)?;
        ensure!(
            request == signature,
            "Import settings changed; choose a new job"
        );
    } else if !legacy {
        atomic_json(&request_path, &signature)?;
    }
    if checkpoint_path.exists() {
        checkpoint = serde_json::from_slice(&read_json_bytes(&checkpoint_path)?)?;
        ensure!(
            checkpoint.version == 1 && checkpoint.signature == signature,
            "Job source/model/settings changed; choose a new folder"
        );
        let doc = laut_core::Document::from_value(checkpoint.recording.clone())?;
        ensure!(
            checkpoint.duration.is_finite()
                && checkpoint.duration > 0.
                && checkpoint.next.is_finite()
                && checkpoint.next >= 0.
                && checkpoint.next <= checkpoint.duration
                && (checkpoint.next == checkpoint.duration || checkpoint.next % 300. == 0.),
            "Invalid checkpoint boundary"
        );
        ensure!(
            doc.view()
                .segments
                .iter()
                .all(|s| s.end <= checkpoint.next + 0.01),
            "Checkpoint contains future segments"
        );
        let audio = crate::media::resolve(
            &config.output,
            &doc.view()
                .audio_filename
                .context("Checkpoint audio is missing")?,
        )?;
        ensure!(
            digest(&audio)?
                == checkpoint.recording["portableImport"]["audioSha256"]
                    .as_str()
                    .unwrap_or(signature["audio"].as_str().unwrap()),
            "Stored audio changed"
        );
        if config.output.join("recording.json").exists() {
            let published: Value =
                serde_json::from_slice(&read_json_bytes(&config.output.join("recording.json"))?)?;
            validate_published(&published, &checkpoint.recording)?;
        }
    } else {
        let mut probe = Command::new(&config.ffprobe);
        probe
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration:stream=codec_type,codec_name",
                "-of",
                "json",
            ])
            .arg(&config.input);
        let probe_path = config.output.join("probe.json");
        run_capture(
            probe,
            &config.output.join("probe.log"),
            Some(&probe_path),
            cancel,
            |_| {},
        )?;
        let info: Value = serde_json::from_slice(&read_json_bytes(&probe_path)?)?;
        let streams = info["streams"].as_array().context("No audio streams")?;
        let stream = streams
            .iter()
            .find(|s| s["codec_type"] == "audio")
            .context("File contains no audio")?;
        let duration: f64 = info["format"]["duration"]
            .as_str()
            .context("Missing duration")?
            .parse()?;
        ensure!(
            duration.is_finite() && duration > 0. && duration <= 86400.,
            "Audio must be between 0 and 24 hours"
        );
        let ext = config
            .input
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("audio")
            .to_lowercase();
        ensure!(
            ext.bytes().all(|b| b.is_ascii_alphanumeric()),
            "Invalid audio extension"
        );
        let expected = if config.compact_audio {
            (duration * 40_000.) as u64
        } else {
            fs::metadata(&config.input)?.len()
        };
        ensure!(
            fs2::available_space(&config.output)? > expected.saturating_add(600 * 1024 * 1024),
            "Insufficient free space"
        );
        let name = if config.compact_audio {
            "source.m4a".to_owned()
        } else {
            format!("source.{ext}")
        };
        progress(0., "Preparing audio / Audio vorbereiten");
        if config.compact_audio {
            let mut decode = Command::new(&config.ffmpeg);
            decode
                .args(["-nostdin", "-v", "error", "-y", "-i"])
                .arg(&config.input)
                .args(["-map", "0:a:0", "-vn", "-map_metadata", "-1"]);
            // AAC can be extracted without another lossy encoding pass.
            if stream["codec_name"] == "aac" {
                decode.args(["-c:a", "copy"]);
            } else {
                decode.args(["-c:a", "aac", "-b:a", "96k", "-ac", "2", "-ar", "48000"]);
            }
            decode
                .args(["-movflags", "+faststart"])
                .arg(config.output.join(&name));
            run(decode, &config.output.join("import.log"), cancel)?;
        } else {
            fs::copy(&config.input, config.output.join(&name))?;
        }
        ensure!(
            digest(&config.input)? == signature["audio"].as_str().unwrap(),
            "Audio changed during import"
        );
        let mut recording = json!({"id":uuid::Uuid::new_v4().to_string().to_uppercase(),"title":config.input.file_stem().unwrap_or_default().to_string_lossy(),"createdAt":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs_f64()-978307200.,"kind":"file","audioFilename":name,"duration":duration,"segments":[],"speakers":[],"notes":"","refinedText":"","state":"processing","engine":"whisper.cpp","transcriptionProgress":0.});
        recording["portableImport"] = json!({"sourcePath":config.input,"sourceSha256":signature["audio"],"audioSha256":digest(&config.output.join(&name))?,"compactAudio":config.compact_audio,"originalKeptAtSource":true,"audioStream":0});
        checkpoint = Checkpoint {
            version: 1,
            signature,
            next: 0.,
            duration,
            recording,
        };
        atomic_json(&checkpoint_path, &checkpoint)?;
    }
    let scratch = tempfile::Builder::new()
        .prefix(".chunk-")
        .tempdir_in(&config.output)?;
    while checkpoint.next < checkpoint.duration {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "Cancelled; completed chunks are retained"
        );
        ensure!(
            fs2::available_space(&config.output)? > 550 * 1024 * 1024,
            "Insufficient free space for next chunk"
        );
        let from = checkpoint.next;
        let span = (checkpoint.duration - from).min(300.);
        progress((from / checkpoint.duration) as f32, "Transcribing locally");
        let wav = scratch.path().join("chunk.wav");
        let result = scratch.path().join("result");
        let audio = config.output.join(
            checkpoint.recording["audioFilename"]
                .as_str()
                .context("Missing audio")?,
        );
        let mut decode = Command::new(&config.ffmpeg);
        decode
            .args(["-nostdin", "-v", "error", "-y", "-ss"])
            .arg(from.to_string())
            .arg("-i")
            .arg(audio)
            .arg("-t")
            .arg(span.to_string())
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
            .arg(&wav);
        run(decode, &scratch.path().join("decode.log"), cancel)?;
        let json_path = result.with_extension("json");
        if json_path.exists() {
            fs::remove_file(&json_path)?;
        }
        let mut whisper = Command::new(&config.whisper);
        whisper
            .arg("-m")
            .arg(&config.model)
            .arg("-f")
            .arg(&wav)
            .arg("-l")
            .arg(&config.language)
            .arg(if legacy { "-oj" } else { "-ojf" })
            .arg("-of")
            .arg(&result);
        if !config.use_gpu {
            whisper.arg("-ng");
        }
        if !config.prompt.trim().is_empty() {
            whisper.arg("--prompt").arg(config.prompt.trim());
        }
        run(whisper, &scratch.path().join("whisper.log"), cancel)?;
        let output: Value = serde_json::from_slice(&read_json_bytes(&json_path)?)?;
        let new_segments = parse_whisper(&output, from, span)?;
        checkpoint.recording["segments"]
            .as_array_mut()
            .context("Invalid checkpoint")?
            .extend(new_segments);
        checkpoint.next = (from + span).min(checkpoint.duration);
        checkpoint.recording["transcriptionProgress"] =
            json!(checkpoint.next / checkpoint.duration);
        // The complete checkpoint is authoritative; published partial text follows it.
        atomic_json(&checkpoint_path, &checkpoint)?;
        atomic_json(&config.output.join("recording.json"), &checkpoint.recording)?;
    }
    checkpoint.recording["state"] = json!("complete");
    checkpoint
        .recording
        .as_object_mut()
        .unwrap()
        .remove("transcriptionProgress");
    atomic_json(&checkpoint_path, &checkpoint)?;
    atomic_json(&config.output.join("recording.json"), &checkpoint.recording)?;
    progress(1., "Transcription complete");
    Ok(config.output.join("recording.json"))
}
pub fn parse_whisper(output: &Value, offset: f64, duration: f64) -> Result<Vec<Value>> {
    ensure!(
        offset.is_finite() && offset >= 0. && duration.is_finite() && duration > 0.,
        "Invalid chunk bounds"
    );
    let entries = output["transcription"]
        .as_array()
        .context("Unsupported whisper.cpp JSON output")?;
    let mut segments = vec![];
    for item in entries {
        let text = item["text"].as_str().context("Missing text")?.trim();
        let start = item["offsets"]["from"]
            .as_f64()
            .context("Missing start time")?
            / 1000.;
        let end = item["offsets"]["to"].as_f64().context("Missing end time")? / 1000.;
        ensure!(
            start.is_finite()
                && end.is_finite()
                && start >= 0.
                && end >= start
                && start <= duration
                // Whisper decodes padded 30-second windows. A final segment can extend
                // beyond the real audio; keep its text and original offsets for review.
                && end <= duration + 30.,
            "Invalid worker timestamps: {start}–{end}s for a {duration}s chunk"
        );
        if text.is_empty() {
            continue;
        }
        let mut segment = json!({"id":uuid::Uuid::new_v4().to_string().to_uppercase(),"start":offset+start,"end":offset+end.min(duration),"text":text,"originalText":text,"speakerLocked":false,"words":[]});
        if let Some(tokens) = item.get("tokens") {
            segment["workerTokens"] = tokens.clone();
            segment["words"] = json!(whisper_words(
                tokens,
                text,
                offset,
                start,
                end.min(duration)
            ));
        }
        if end > duration {
            segment["workerTiming"] =
                json!({"chunkOffset":offset,"from":start,"to":end,"clampedToAudio":true});
        }
        segments.push(segment);
    }
    Ok(segments)
}

fn whisper_words(tokens: &Value, text: &str, offset: f64, start: f64, end: f64) -> Vec<Value> {
    let Some(tokens) = tokens.as_array() else {
        return vec![];
    };
    let mut words: Vec<Value> = vec![];
    let mut joined = String::new();
    for token in tokens {
        let Some(t) = token["text"].as_str() else {
            return vec![];
        };
        if t.starts_with("[_") || t.starts_with("<|") {
            continue;
        }
        if t.is_empty() {
            continue;
        }
        joined.push_str(t);
        let (Some(a), Some(b)) = (
            token["offsets"]["from"].as_f64(),
            token["offsets"]["to"].as_f64(),
        ) else {
            return vec![];
        };
        let (a, b) = (a / 1000., b / 1000.);
        if a < start - 0.1 || b < a || a > end || !a.is_finite() || !b.is_finite() {
            return vec![];
        }
        if t.trim().is_empty() {
            continue;
        }
        if words.is_empty() || t.starts_with(char::is_whitespace) {
            words
                .push(json!({"text":t.trim(),"start":offset+a.max(start),"end":offset+b.min(end)}));
        } else {
            let last = words.last_mut().unwrap();
            last["text"] = json!(format!("{}{}", last["text"].as_str().unwrap(), t));
            last["end"] = json!(offset + b.min(end));
        }
    }
    if joined.trim() != text
        || words
            .windows(2)
            .any(|w| w[0]["start"].as_f64() > w[1]["start"].as_f64())
    {
        return vec![];
    }
    words
}

/// A crash may occur after checkpoint commit but before partial-text publication. Accept an
/// unchanged prefix, never a divergent edit, changed notes or replaced source metadata.
fn validate_published(published: &Value, checkpoint: &Value) -> Result<()> {
    let old = published["segments"]
        .as_array()
        .context("Invalid published segments")?;
    let current = checkpoint["segments"]
        .as_array()
        .context("Invalid checkpoint segments")?;
    ensure!(
        old.len() <= current.len() && current[..old.len()] == *old,
        "Published transcript changed; refusing to overwrite edits"
    );
    let mut a = published.clone();
    let mut b = checkpoint.clone();
    for key in ["segments", "state", "transcriptionProgress"] {
        a.as_object_mut()
            .context("Invalid published document")?
            .remove(key);
        b.as_object_mut()
            .context("Invalid checkpoint document")?
            .remove(key);
    }
    ensure!(
        a == b,
        "Published metadata changed; refusing to overwrite edits"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_can_be_ahead_of_publication_but_cannot_overwrite_edits() {
        let old = json!({"notes":"keep","segments":[{"text":"first"}],"state":"processing","transcriptionProgress":0.5});
        let mut new = old.clone();
        new["segments"]
            .as_array_mut()
            .unwrap()
            .push(json!({"text":"next"}));
        new["transcriptionProgress"] = json!(1.);
        assert!(validate_published(&old, &new).is_ok());
        let mut edited = old.clone();
        edited["notes"] = json!("new notes");
        assert!(validate_published(&edited, &new).is_err());
        edited = old;
        edited["segments"][0]["text"] = json!("manual correction");
        assert!(validate_published(&edited, &new).is_err());
    }
}
