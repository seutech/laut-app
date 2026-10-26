//! Explicit YouTube audio download. No shell, browser cookies, playlists or implicit updates.
use crate::{
    jobs::{atomic_json, run_capture},
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
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_SECONDS: f64 = 86400.;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UrlImport {
    pub url: String,
    pub yt_dlp: PathBuf,
    pub deno: PathBuf,
    /// Optional preview starting at zero; the library retains this source offset.
    #[serde(default)]
    pub first_seconds: Option<f64>,
}
impl UrlImport {
    pub fn normalize(&mut self) -> Result<()> {
        self.url = youtube_url(&self.url)?;
        ensure!(
            self.first_seconds
                .is_none_or(|n| n.is_finite() && n > 0. && n <= MAX_SECONDS),
            "Invalid preview duration / Ungültige Ausschnittdauer"
        );
        let cwd = std::env::current_dir()?;
        for p in [&mut self.yt_dlp, &mut self.deno] {
            if !p.is_absolute() {
                *p = cwd.join(&*p);
            }
        }
        Ok(())
    }
}
/// Strip tracking, playlist and time parameters; never hand arbitrary URLs to an extractor.
pub fn youtube_url(text: &str) -> Result<String> {
    let u = url::Url::parse(text.trim()).context("YouTube-Link ungültig / Invalid YouTube link")?;
    ensure!(
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none_or(|p| p == 443),
        "Use an HTTPS YouTube link without credentials / HTTPS-YouTube-Link ohne Zugangsdaten verwenden"
    );
    let host = u.host_str().unwrap_or_default();
    let parts: Vec<_> = u.path_segments().into_iter().flatten().collect();
    let id = match host {
        "youtu.be" | "www.youtu.be" if parts.len() == 1 => parts[0].to_owned(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if u.path() == "/watch" {
                let ids: Vec<_> = u.query_pairs().filter(|(k, _)| k == "v").collect();
                ensure!(
                    ids.len() == 1,
                    "Use a single video link / Einzelnen Videolink verwenden"
                );
                ids[0].1.to_string()
            } else {
                ensure!(
                    parts.len() == 2 && matches!(parts[0], "live" | "shorts" | "embed"),
                    "Use a video link, not a playlist / Videolink statt Playlist verwenden"
                );
                parts[1].to_owned()
            }
        }
        _ => anyhow::bail!(
            "Only YouTube video links are supported / Bitte einen YouTube-Videolink verwenden"
        ),
    };
    ensure!(
        id.len() == 11
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "Invalid YouTube video ID / Ungültige YouTube-Video-ID"
    );
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    version: u32,
    pub filename: String,
    pub sha256: String,
    pub source: Value,
}
fn request(config: &UrlImport) -> Value {
    json!({"version":1,"url":config.url,"firstSeconds":config.first_seconds})
}
fn base(config: &UrlImport, ffmpeg: &Path) -> Command {
    let mut c = Command::new(&config.yt_dlp);
    c.args([
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-remote-components",
        "--no-cache-dir",
        "--no-playlist",
        "--no-update",
        "--no-js-runtimes",
        "--js-runtimes",
    ])
    .arg(format!("deno:{}", config.deno.display()))
    .args([
        "--socket-timeout",
        "20",
        "--retries",
        "2",
        "--fragment-retries",
        "2",
        "--extractor-retries",
        "1",
        "--abort-on-unavailable-fragments",
        "--format",
        "bestaudio[ext=m4a]/bestaudio",
        "--max-filesize",
    ])
    .arg(MAX_BYTES.to_string())
    .arg("--ffmpeg-location")
    .arg(ffmpeg.parent().unwrap_or(Path::new(".")));
    c
}
fn metadata(info: &Value, config: &UrlImport) -> Result<Value> {
    ensure!(
        !matches!(info["_type"].as_str(), Some("playlist" | "multi_video"))
            && info["is_live"] != true
            && !matches!(
                info["live_status"].as_str(),
                Some("is_live" | "is_upcoming" | "post_live")
            ),
        "Liveübertragung noch nicht vollständig veröffentlicht. Später erneut starten / Live archive not ready; retry later"
    );
    let duration = info["duration"]
        .as_f64()
        .context("Video duration unavailable")?;
    ensure!(
        duration.is_finite() && duration > 0. && duration <= MAX_SECONDS,
        "Videos bis 24 Stunden werden unterstützt / Videos up to 24 hours are supported"
    );
    ensure!(
        info["vcodec"] == "none" && info["acodec"].as_str().is_some_and(|c| c != "none"),
        "Separate audio stream unavailable / Keine separate Audiospur verfügbar"
    );
    ensure!(
        config
            .url
            .ends_with(info["id"].as_str().context("Missing video ID")?),
        "Source identity changed"
    );
    let end = config.first_seconds.unwrap_or(duration).min(duration);
    let title = info["title"].as_str().unwrap_or("YouTube");
    ensure!(title.len() <= 8192, "Source title too long");
    let mut chapters = vec![];
    for ch in info["chapters"].as_array().into_iter().flatten().take(2000) {
        if let (Some(start), Some(title)) = (ch["start_time"].as_f64(), ch["title"].as_str())
            && start.is_finite()
            && start >= 0.
            && start < end
            && title.len() <= 4096
        {
            chapters.push(json!({"id":uuid::Uuid::new_v4().to_string().to_uppercase(),"start":start,"title":title,"origin":"YouTube"}));
        }
    }
    let date = info["upload_date"]
        .as_str()
        .filter(|d| d.len() == 8 && d.bytes().all(|c| c.is_ascii_digit()))
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
    let mut source = json!({"url":config.url,"provider":"YouTube","externalID":info["id"],"originalTitle":title,"publisher":info["channel"].as_str().or_else(||info["uploader"].as_str()),"publishedOn":date,"language":info["language"],"originalDuration":duration,"chapters":chapters,"importedAt":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs_f64()-978307200.});
    if config.first_seconds.is_some() {
        source["clipStart"] = json!(0.);
        source["clipEnd"] = json!(end);
    }
    Ok(source)
}
fn media_file(folder: &Path, filename: &str) -> Result<PathBuf> {
    ensure!(
        [
            "m4a", "webm", "opus", "ogg", "mp3", "aac", "flac", "wav", "mp4"
        ]
        .iter()
        .any(|e| filename == format!("audio.{e}")),
        "Invalid downloaded filename"
    );
    let path = folder.join(filename);
    ensure!(
        path.is_file() && !fs::symlink_metadata(&path)?.file_type().is_symlink(),
        "Downloaded audio missing or replaced"
    );
    Ok(path)
}
/// Reuse only a hash-verified completed download; yt-dlp itself resumes .part files.
pub fn download(
    config: &UrlImport,
    folder: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<Download> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Download paused / Download pausiert"
    );
    ensure!(
        youtube_url(&config.url)? == config.url,
        "URL is not normalized"
    );
    fs::create_dir_all(folder)?;
    ensure!(
        !fs::symlink_metadata(folder)?.file_type().is_symlink(),
        "Download folder is a symlink"
    );
    let request_path = folder.join("request.json");
    if request_path.exists() {
        ensure!(
            serde_json::from_slice::<Value>(&read_json_bytes(&request_path)?)? == request(config),
            "Download source changed"
        );
    } else {
        atomic_json(&request_path, &request(config))?;
    }
    let result_path = folder.join("download.json");
    if result_path.exists() {
        let result: Download = serde_json::from_slice(&read_json_bytes(&result_path)?)?;
        ensure!(
            result.version == 1 && result.source["url"] == config.url,
            "Invalid download checkpoint"
        );
        ensure!(
            digest(&media_file(folder, &result.filename)?)? == result.sha256,
            "Downloaded audio changed; keep this job for inspection / Heruntergeladene Audiodatei wurde verändert"
        );
        progress(1., "Verified cached audio / Vorhandenes Audio geprüft");
        return Ok(result);
    }
    for (name, p) in [
        ("yt-dlp", config.yt_dlp.as_path()),
        ("Deno", config.deno.as_path()),
        ("FFmpeg", ffmpeg),
        ("FFprobe", ffprobe),
    ] {
        ensure!(
            p.is_file(),
            "{name} fehlt. Unter Modelle / Programme einrichten / Configure {name} under Models / programs"
        );
    }
    ensure!(
        fs2::available_space(folder)? > 1024 * 1024 * 1024,
        "At least 1 GiB free space required / Mindestens 1 GiB freier Speicher nötig"
    );
    progress(0., "YouTube-Quelle prüfen / Checking YouTube source");
    let probe_path = folder.join("source.json");
    let mut probe = base(config, ffmpeg);
    // Print a small public metadata subset, never persist signed media URLs or cookies.
    probe.args(["--skip-download", "--print", "%(.{id,title,channel,uploader,upload_date,language,duration,live_status,is_live,_type,vcodec,acodec,chapters})j", "--"]).arg(&config.url);
    run_capture(probe, &folder.join("probe.log"), Some(&probe_path), cancel, |_| {})
        .context("YouTube-Zugriff fehlgeschlagen (privat, entfernt oder Anmeldung erforderlich). Öffentlichen Link und yt-dlp-Version prüfen / YouTube access failed; check public availability and yt-dlp version")?;
    let source = metadata(
        &serde_json::from_slice(&read_json_bytes(&probe_path)?)?,
        config,
    )?;
    atomic_json(&probe_path, &source)?;
    progress(0.05, "Audiospur herunterladen / Downloading audio");
    let path_file = folder.join("download-path.txt");
    if path_file.exists() {
        fs::remove_file(&path_file)?;
    }
    let mut cmd = base(config, ffmpeg);
    cmd.args([
        "--newline",
        "--progress",
        "--progress-template",
        "download:progress %(progress._percent_str)s",
        "--continue",
        "--fixup",
        "never",
        "--output",
    ])
    .arg(folder.join("audio.%(ext)s"))
    .args(["--print-to-file", "after_move:filepath"])
    .arg(&path_file);
    if config.first_seconds.is_some() {
        cmd.arg("--download-sections")
            .arg(format!("*0-{}", source["clipEnd"].as_f64().unwrap()));
    }
    cmd.arg("--").arg(&config.url);
    run_capture(cmd, &folder.join("download.log"), None, cancel, |p| {
        progress(
            0.05 + p * 0.9,
            "Audiospur herunterladen / Downloading audio",
        )
    })?;
    let output = fs::read_to_string(&path_file)
        .context("Download did not produce audio (size limit or unavailable format)")?;
    let output = PathBuf::from(output.trim());
    ensure!(
        output.parent() == Some(folder),
        "Download escaped job folder"
    );
    let filename = output
        .file_name()
        .context("Missing download filename")?
        .to_string_lossy()
        .into_owned();
    let audio = media_file(folder, &filename)?;
    ensure!(
        fs::metadata(&audio)?.len() > 0 && fs::metadata(&audio)?.len() <= MAX_BYTES,
        "Audio exceeds 2 GiB download limit"
    );
    let mut probe = Command::new(ffprobe);
    probe
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(&audio);
    let audio_probe = folder.join("audio-probe.json");
    run_capture(
        probe,
        &folder.join("audio-probe.log"),
        Some(&audio_probe),
        cancel,
        |_| {},
    )?;
    let info: Value = serde_json::from_slice(&read_json_bytes(&audio_probe)?)?;
    let streams = info["streams"]
        .as_array()
        .context("Audio streams missing")?;
    ensure!(
        streams.iter().any(|s| s["codec_type"] == "audio")
            && !streams.iter().any(|s| s["codec_type"] == "video"),
        "Download must contain audio only"
    );
    let duration: f64 = info["format"]["duration"]
        .as_str()
        .context("Audio duration missing")?
        .parse()?;
    let expected = source["clipEnd"]
        .as_f64()
        .unwrap_or(source["originalDuration"].as_f64().unwrap());
    ensure!(
        duration.is_finite()
            && duration > 0.
            && (duration - expected).abs() <= 5_f64.max(expected * 0.01),
        "Download is incomplete or has unexpected duration / Audiodownload unvollständig"
    );
    let result = Download {
        version: 1,
        filename,
        sha256: digest(&audio)?,
        source,
    };
    atomic_json(&result_path, &result)?;
    progress(1., "Audio lokal gespeichert / Audio saved locally");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_rejects_live_video_and_keeps_only_relevant_source_chapters() {
        let config = UrlImport {
            url: "https://www.youtube.com/watch?v=12345678901".into(),
            yt_dlp: "yt-dlp".into(),
            deno: "deno".into(),
            first_seconds: Some(30.),
        };
        let mut info = json!({"id":"12345678901","title":"Talk","duration":420.,"vcodec":"none","acodec":"aac","upload_date":"20261022","url":"signed-private-url","chapters":[{"start_time":0.,"title":"Intro"},{"start_time":29.,"title":"Discussion"},{"start_time":31.,"title":"Later"}]});
        let result = metadata(&info, &config).unwrap();
        assert_eq!(result["chapters"].as_array().unwrap().len(), 2);
        assert_eq!(result["publishedOn"], "2026-10-22");
        assert!(!result.to_string().contains("signed-private"));
        for state in ["is_live", "is_upcoming", "post_live"] {
            info["live_status"] = json!(state);
            assert!(metadata(&info, &config).is_err());
        }
        info["live_status"] = json!("was_live");
        assert!(metadata(&info, &config).is_ok());
        info["vcodec"] = json!("h264");
        assert!(metadata(&info, &config).is_err());
        let mut bad = config;
        bad.first_seconds = Some(f64::INFINITY);
        assert!(bad.normalize().is_err());
    }
}
