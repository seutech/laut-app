use laut_engine::{
    Session,
    downloads::{UrlImport, youtube_url},
    jobs::JobConfig,
    library::Library,
    queue::Queue,
};
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
fn worker() -> PathBuf {
    static WORKER: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = WORKER.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join(if cfg!(windows) {
            "url-worker.exe"
        } else {
            "url-worker"
        });
        let status = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--edition=2024")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/url_worker.rs"
            ))
            .arg("-o")
            .arg(output)
            .status()
            .unwrap();
        assert!(status.success());
        dir
    });
    dir.path().join(if cfg!(windows) {
        "url-worker.exe"
    } else {
        "url-worker"
    })
}
fn config() -> JobConfig {
    JobConfig {
        input: PathBuf::new(),
        output: PathBuf::new(),
        whisper: worker(),
        model: worker(),
        ffmpeg: worker(),
        ffprobe: worker(),
        language: "en".into(),
        use_gpu: false,
        compact_audio: true,
        prompt: "Merz, Huang".into(),
    }
}
fn source(id: &str) -> UrlImport {
    UrlImport {
        url: format!("https://youtu.be/{id}"),
        yt_dlp: worker(),
        deno: worker(),
        first_seconds: Some(30.),
    }
}
#[test]
fn urls_normalize_without_tracking_and_reject_non_video_inputs() {
    for url in [
        "https://youtu.be/-DmX1TkPHnM?si=test",
        "https://www.youtube.com/live/-DmX1TkPHnM",
        "https://m.youtube.com/shorts/-DmX1TkPHnM?t=30",
        "https://youtube.com/watch?v=-DmX1TkPHnM&list=ignored",
    ] {
        assert_eq!(
            youtube_url(url).unwrap(),
            "https://www.youtube.com/watch?v=-DmX1TkPHnM"
        );
    }
    for url in [
        "file:///etc/passwd",
        "https://youtube.com.evil.example/watch?v=-DmX1TkPHnM",
        "https://user:password@youtube.com/watch?v=-DmX1TkPHnM",
        "https://youtube.com/playlist?list=abc",
        "https://youtu.be/../../etc/passwd",
        "https://youtube.com/watch?v=-DmX1TkPHnM&v=12345678901",
        "https://youtu.be/-DmX1TkPHnM/extra",
        "https://youtu.be/$(touch x)",
    ] {
        assert!(youtube_url(url).is_err(), "{url}");
    }
}
#[test]
fn failed_url_does_not_block_batch_and_library_survives_cache_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::create(&temp.path().join("Library with spaces ü")).unwrap();
    let queue = Queue::new(temp.path().join("Jobs with spaces ü"));
    let bad = queue
        .enqueue_url(config(), library.root.clone(), None, source("private0000"))
        .unwrap();
    let good = queue
        .enqueue_url(config(), library.root.clone(), None, source("-DmX1TkPHnM"))
        .unwrap();
    assert!(
        queue
            .enqueue_url(config(), library.root.clone(), None, source("-DmX1TkPHnM"))
            .is_err()
    );
    let batch = queue.run_batch(&[bad.id.clone(), good.id.clone()], &false.into(), |_, _| {});
    assert_eq!(batch.failed.len(), 1);
    assert_eq!(batch.completed, 1, "{:?}", batch.failed);
    let path = batch.last.unwrap();
    let session = Session::open(&path).unwrap();
    assert_eq!(session.document.raw()["title"], "Talk: Merz / Huang?");
    assert_eq!(
        session.document.raw()["source"]["url"],
        "https://www.youtube.com/watch?v=-DmX1TkPHnM"
    );
    assert_eq!(session.document.raw()["source"]["clipEnd"], 30.);
    assert_eq!(
        session.document.raw()["conference"]["chapters"][0]["title"],
        "Opening"
    );
    assert_eq!(session.document.view().segments.len(), 1);
    assert_eq!(queue.run(&good.id, &false.into(), |_, _| {}).unwrap(), path);
    let audio = session.audio_path().unwrap();
    std::fs::write(&audio, "changed").unwrap();
    assert!(queue.cleanup(&good.id).is_err()); // Never delete the only verified source.
    std::fs::write(&audio, "decoded audio").unwrap();
    queue.cleanup(&good.id).unwrap();
    assert!(!good.config.output.exists());
    assert!(!queue.root.join(format!("download-{}", good.id)).exists());
    assert!(audio.is_file());
    assert_eq!(queue.run(&good.id, &false.into(), |_, _| {}).unwrap(), path);
    assert!(queue.cleanup(&bad.id).is_err());
}
#[test]
fn resume_uses_verified_download_without_yt_dlp_and_rejects_changed_audio() {
    let temp = tempfile::tempdir().unwrap();
    let lib = Library::create(&temp.path().join("lib")).unwrap();
    let queue = Queue::new(temp.path().join("jobs"));
    let mut cfg = config();
    cfg.model = temp.path().join("missing-model");
    let item = queue
        .enqueue_url(cfg, lib.root, None, source("-DmX1TkPHnM"))
        .unwrap();
    assert!(queue.run(&item.id, &false.into(), |_, _| {}).is_err());
    let json_path = queue.root.join(format!("{}.json", item.id));
    let mut job: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&json_path).unwrap()).unwrap();
    job["config"]["model"] = serde_json::json!(worker());
    job["url_import"]["yt_dlp"] = serde_json::json!("missing-downloader");
    std::fs::write(&json_path, serde_json::to_vec(&job).unwrap()).unwrap();
    let audio = queue.root.join(format!("download-{}/audio.m4a", item.id));
    std::fs::write(&audio, "changed").unwrap();
    assert!(
        queue
            .run(&item.id, &false.into(), |_, _| {})
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
    std::fs::write(&audio, "audio fixture").unwrap();
    queue
        .run(&item.id, &false.into(), |_, _| {})
        .expect("resume from verified cached audio");
}
#[test]
fn cancel_kills_download_children_and_retains_paused_job() {
    let temp = tempfile::tempdir().unwrap();
    let lib = Library::create(&temp.path().join("lib")).unwrap();
    let queue = Queue::new(temp.path().join("jobs"));
    let item = queue
        .enqueue_url(config(), lib.root, None, source("cancel00000"))
        .unwrap();
    let cancel = AtomicBool::new(false);
    let part = queue.root.join(format!("download-{}/audio.part", item.id));
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let start = Instant::now();
            while !part.exists() && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(25));
            }
            std::thread::sleep(Duration::from_millis(150));
            cancel.store(true, Ordering::Relaxed);
        });
        assert!(queue.run(&item.id, &cancel, |_, _| {}).is_err());
    });
    assert!(part.exists());
    assert_eq!(queue.list().unwrap()[0].state, "paused");
    std::thread::sleep(Duration::from_millis(2200));
    assert!(!part.with_extension("orphan").exists());
    assert!(
        !queue
            .root
            .join(format!("download-{}/download.json", item.id))
            .exists()
    );
}

#[test]
fn captured_audio_is_saved_before_asr_and_transcribed_in_the_same_project() {
    use laut_engine::media::digest;
    let temp = tempfile::tempdir().unwrap();
    let library = Library::create(&temp.path().join("Library ü")).unwrap();
    let source = temp.path().join("Capture");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(
        source.join("microphone.flac"),
        b"original microphone evidence",
    )
    .unwrap();
    std::fs::write(source.join("system.flac"), b"original system evidence").unwrap();
    std::fs::write(source.join("audio.m4a"), b"mixed audio").unwrap();
    let id = uuid::Uuid::new_v4().to_string().to_uppercase();
    let hash = digest(&source.join("audio.m4a")).unwrap();
    let raw = serde_json::json!({"id":id,"title":"Interview ü","createdAt":100.,"kind":"meeting","duration":30.,"audioFilename":"audio.m4a","segments":[],"speakers":[],"notes":"Keep this note","portableCapture":{"version":1,"id":id,"audioSha256":hash,"tracks":[{"filename":"microphone.flac"},{"filename":"system.flac"}]}});
    std::fs::write(
        source.join("recording.json"),
        serde_json::to_vec(&raw).unwrap(),
    )
    .unwrap();
    let saved = library.import(&Session::open(&source).unwrap()).unwrap();
    let path = saved.session.source.clone();
    let queue = Queue::new(temp.path().join("Jobs"));
    let job = queue
        .enqueue_recording(config(), library.root.clone(), &id, None)
        .unwrap();
    assert!(
        queue
            .enqueue_recording(config(), library.root.clone(), &id, None)
            .is_err()
    );
    assert!(saved.session.document.view().segments.is_empty());
    assert_eq!(queue.run(&job.id, &false.into(), |_, _| {}).unwrap(), path);
    let result = Session::open(&path).unwrap();
    assert!(!result.document.view().segments.is_empty());
    assert_eq!(result.document.view().id, id);
    assert_eq!(
        result.document.raw()["portableCapture"],
        raw["portableCapture"]
    );
    assert_eq!(result.document.view().notes, "Keep this note");
    assert_eq!(library.catalog().unwrap().entries.len(), 1);
    assert!(
        queue
            .enqueue_recording(config(), library.root.clone(), &id, None)
            .is_err()
    );
    queue.cleanup(&job.id).unwrap();
    assert_eq!(digest(&result.audio_path().unwrap()).unwrap(), hash);
    assert_eq!(
        std::fs::read(path.parent().unwrap().join("system.flac")).unwrap(),
        b"original system evidence"
    );
    let copy = temp.path().join("Project copy");
    result.save_copy(&copy).unwrap();
    assert_eq!(
        std::fs::read(copy.join("microphone.flac")).unwrap(),
        b"original microphone evidence"
    );
    // A crash after the library save but before the queue commit resumes without duplicate ASR.
    let queue_path = queue.root.join(format!("{}.json", job.id));
    let mut interrupted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&queue_path).unwrap()).unwrap();
    interrupted["result"] = serde_json::Value::Null;
    interrupted["config"]["whisper"] = serde_json::json!("missing-worker");
    std::fs::write(&queue_path, serde_json::to_vec(&interrupted).unwrap()).unwrap();
    assert_eq!(queue.run(&job.id, &false.into(), |_, _| {}).unwrap(), path);
    assert_eq!(library.catalog().unwrap().entries.len(), 1);
}
