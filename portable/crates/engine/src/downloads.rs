//! Explicit public-link audio download. No shell, browser cookies, playlists or implicit updates.
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
    net::IpAddr,
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
        self.url = source_url(&self.url)?;
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
/// Accept one public HTTPS media page. YouTube links are reduced to a stable video URL;
/// other providers keep query parameters because podcast/CDN links may require them.
pub fn source_url(text: &str) -> Result<String> {
    let mut u = url::Url::parse(text.trim()).context("Link ungültig / Invalid link")?;
    ensure!(
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none_or(|p| p == 443),
        "Öffentlichen HTTPS-Link ohne Zugangsdaten verwenden / Use a public HTTPS link without credentials"
    );
    ensure!(text.trim().len() <= 8192, "Link too long / Link zu lang");
    let host = u
        .host_str()
        .context("Link host missing")?
        .to_ascii_lowercase();
    ensure!(
        !matches!(
            host.as_str(),
            "localhost" | "localhost.localdomain" | "metadata.google.internal"
        ) && ![".localhost", ".local", ".internal", ".home", ".lan"]
            .iter()
            .any(|suffix| host.ends_with(suffix)),
        "Lokale Netzwerkadressen werden nicht als Medienquelle geöffnet / Local network addresses are not opened as media sources"
    );
    let ip_text = host
        .strip_prefix('[')
        .and_then(|text| text.strip_suffix(']'))
        .unwrap_or(&host);
    if let Ok(ip) = ip_text.parse::<IpAddr>() {
        ensure!(public_ip(ip), "Public Internet address required");
    }
    u.set_fragment(None);
    if !matches!(
        host.as_str(),
        "youtu.be"
            | "www.youtu.be"
            | "youtube.com"
            | "www.youtube.com"
            | "m.youtube.com"
            | "music.youtube.com"
    ) {
        return Ok(u.to_string());
    }
    let parts: Vec<_> = u.path_segments().into_iter().flatten().collect();
    let id = match host.as_str() {
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
        _ => anyhow::bail!("Use an individual YouTube video link"),
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

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 168)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && segments[0] & 0xfe00 != 0xfc00
                && segments[0] & 0xffc0 != 0xfe80
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && ip
                    .to_ipv4_mapped()
                    .is_none_or(|mapped| public_ip(mapped.into()))
        }
    }
}

/// Vimeo's public page endpoint may demand a logged-in web client even when its
/// matching embedded player remains public. Keep the user's source URL in the
/// project, but use the public player endpoint for extraction when possible.
fn extractor_url(source: &str) -> String {
    let Ok(url) = url::Url::parse(source) else {
        return source.to_owned();
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if matches!(host.as_str(), "vimeo.com" | "www.vimeo.com")
        && let Some(id) = url
            .path_segments()
            .into_iter()
            .flatten()
            .next_back()
            .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return format!("https://player.vimeo.com/video/{id}");
    }
    source.to_owned()
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
    let duration = info["duration"].as_f64();
    ensure!(
        duration.is_none_or(|duration| {
            duration.is_finite() && duration > 0. && duration <= MAX_SECONDS
        }),
        "Einzelquellen bis 24 Stunden werden unterstützt / Individual sources up to 24 hours are supported"
    );
    ensure!(
        info["vcodec"] == "none",
        "Separate audio stream unavailable / Keine separate Audiospur verfügbar"
    );
    let end = config
        .first_seconds
        .map(|requested| duration.map_or(requested, |duration| requested.min(duration)))
        .or(duration)
        .unwrap_or(MAX_SECONDS);
    let provider = provider(info, &config.url);
    let title = info["title"].as_str().unwrap_or(&provider);
    ensure!(title.len() <= 8192, "Source title too long");
    let mut chapters = vec![];
    for ch in info["chapters"].as_array().into_iter().flatten().take(2000) {
        if let (Some(start), Some(title)) = (ch["start_time"].as_f64(), ch["title"].as_str())
            && start.is_finite()
            && start >= 0.
            && start < end
            && title.len() <= 4096
        {
            chapters.push(json!({"id":uuid::Uuid::new_v4().to_string().to_uppercase(),"start":start,"title":title,"origin":provider.clone()}));
        }
    }
    let date = info["release_date"]
        .as_str()
        .or_else(|| info["upload_date"].as_str())
        .filter(|d| d.len() == 8 && d.bytes().all(|c| c.is_ascii_digit()))
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
    let publisher = info["channel"]
        .as_str()
        .or_else(|| info["uploader"].as_str())
        .or_else(|| info["creator"].as_str())
        .or_else(|| info["series"].as_str());
    let mut source = json!({"url":config.url,"provider":provider,"externalID":info["id"],"originalTitle":title,"publisher":publisher,"publishedOn":date,"language":info["language"],"originalDuration":duration,"chapters":chapters,"importedAt":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs_f64()-978307200.});
    if config.first_seconds.is_some() {
        source["clipStart"] = json!(0.);
        source["clipEnd"] = json!(end);
    }
    Ok(source)
}

fn provider(info: &Value, source: &str) -> String {
    let host = url::Url::parse(source)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "Online".into())
        .to_ascii_lowercase();
    if host == "youtu.be" || host.ends_with(".youtube.com") || host == "youtube.com" {
        return "YouTube".into();
    }
    if host == "vimeo.com" || host.ends_with(".vimeo.com") {
        return "Vimeo".into();
    }
    info["extractor_key"]
        .as_str()
        .or_else(|| info["extractor"].as_str())
        .filter(|name| !name.eq_ignore_ascii_case("generic") && name.len() <= 128)
        .map(str::to_owned)
        .unwrap_or_else(|| host.strip_prefix("www.").unwrap_or(&host).to_owned())
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
        source_url(&config.url)? == config.url,
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
    progress(0., "Link prüfen / Checking link");
    let probe_path = folder.join("source.json");
    let mut probe = base(config, ffmpeg);
    // Print a small public metadata subset, never persist signed media URLs or cookies.
    probe.args(["--skip-download", "--print", "%(.{id,title,channel,uploader,creator,series,upload_date,release_date,language,duration,live_status,is_live,_type,vcodec,acodec,chapters,extractor,extractor_key})j", "--"]).arg(extractor_url(&config.url));
    run_capture(probe, &folder.join("probe.log"), Some(&probe_path), cancel, |_| {})
        .context("Link konnte nicht als öffentliche Einzelquelle gelesen werden. Verfügbarkeit und yt-dlp-Version prüfen / Link could not be read as a public individual source; check availability and yt-dlp version")?;
    let mut source = metadata(
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
    cmd.arg("--").arg(extractor_url(&config.url));
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
    ensure!(
        duration.is_finite() && duration > 0. && duration <= MAX_SECONDS,
        "Downloaded audio has an invalid duration / Heruntergeladenes Audio hat eine ungültige Dauer"
    );
    if let Some(expected) = source["originalDuration"].as_f64() {
        let target = source["clipEnd"].as_f64().unwrap_or(expected);
        ensure!(
            (duration - target).abs() <= 5_f64.max(target * 0.01),
            "Download is incomplete or has unexpected duration / Audiodownload unvollständig"
        );
    } else if source["clipEnd"].is_number() {
        ensure!(
            duration <= source["clipEnd"].as_f64().unwrap() + 5.,
            "Downloaded excerpt exceeds its requested duration / Ausschnitt ist länger als angefordert"
        );
        source["clipEnd"] = json!(duration);
    } else {
        source["originalDuration"] = json!(duration);
    }
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

    #[test]
    fn metadata_uses_the_actual_provider_and_podcast_fields() {
        let config = UrlImport {
            url: "https://vimeo.com/76979871".into(),
            yt_dlp: "yt-dlp".into(),
            deno: "deno".into(),
            first_seconds: None,
        };
        let info = json!({
            "id":"76979871",
            "title":"The New Vimeo Player",
            "duration":62.,
            "vcodec":"none",
            "acodec":"aac",
            "release_date":"20261023",
            "creator":"Vimeo Staff",
            "extractor_key":"Vimeo",
            "chapters":[{"start_time":0.,"title":"Opening"}]
        });
        let result = metadata(&info, &config).unwrap();
        assert_eq!(result["provider"], "Vimeo");
        assert_eq!(result["publisher"], "Vimeo Staff");
        assert_eq!(result["publishedOn"], "2026-10-23");
        assert_eq!(result["chapters"][0]["origin"], "Vimeo");

        let generic = UrlImport {
            url: "https://cdn.podcast.example/episode.mp3".into(),
            ..config
        };
        let mut generic_info = info;
        generic_info["extractor_key"] = json!("Generic");
        generic_info.as_object_mut().unwrap().remove("duration");
        assert_eq!(
            metadata(&generic_info, &generic).unwrap()["originalDuration"],
            Value::Null
        );
        assert_eq!(
            metadata(&generic_info, &generic).unwrap()["provider"],
            "cdn.podcast.example"
        );
    }

    #[test]
    fn vimeo_pages_use_the_public_player_without_changing_the_saved_source() {
        assert_eq!(
            extractor_url("https://vimeo.com/863362136?share=copy"),
            "https://player.vimeo.com/video/863362136"
        );
        assert_eq!(
            extractor_url("https://vimeo.com/channels/staffpicks/863362136"),
            "https://player.vimeo.com/video/863362136"
        );
        assert_eq!(
            extractor_url("https://player.vimeo.com/video/863362136"),
            "https://player.vimeo.com/video/863362136"
        );
    }
}
