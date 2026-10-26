use laut_engine::{
    diarization::parse_output,
    jobs::{JobConfig, parse_whisper},
    library::Library,
    queue::Queue,
};
use serde_json::json;
use std::path::PathBuf;
#[test]
fn tokens_become_words_only_when_text_and_times_match() {
    let output = json!({"transcription":[{"text":" Well, hello!","offsets":{"from":0,"to":2000},"tokens":[{"text":" We","offsets":{"from":0,"to":200}},{"text":"ll","offsets":{"from":200,"to":400}},{"text":",","offsets":{"from":400,"to":400}},{"text":" hello","offsets":{"from":1000,"to":1800}},{"text":"!","offsets":{"from":1800,"to":2000}},{"text":"[_EOT_]"}]}]});
    let result = parse_whisper(&output, 300., 3.).unwrap();
    assert_eq!(result[0]["words"].as_array().unwrap().len(), 2);
    assert_eq!(result[0]["words"][0]["text"], "Well,");
    assert_eq!(result[0]["words"][1]["start"], 301.);
    let mut bad = output;
    bad["transcription"][0]["text"] = json!("Different text");
    let result = parse_whisper(&bad, 0., 3.).unwrap();
    assert_eq!(result[0]["words"], json!([]));
    assert!(result[0]["workerTokens"].is_array());
}
#[test]
fn speaker_output_accepts_overlap_but_rejects_bad_bounds() {
    let turns = parse_output(
        "Config\nStarted\n0.318 -- 6.865 speaker_00\n6.000 -- 7.500 speaker_01\n",
        8.,
    )
    .unwrap();
    assert_eq!(turns.len(), 2);
    assert!(parse_output("3 -- 2 speaker_00", 8.).is_err());
    assert!(parse_output("0 -- 100 speaker_00", 8.).is_err());
    assert!(parse_output("Started", 8.).is_err());
}
#[test]
fn queue_persists_prevents_duplicates_and_keeps_failed_jobs() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("audio.mp4");
    std::fs::write(&source, "source").unwrap();
    let library = Library::create(&temp.path().join("library")).unwrap();
    let queue = Queue::new(temp.path().join("jobs"));
    let config = JobConfig {
        input: source.clone(),
        output: PathBuf::new(),
        whisper: "missing".into(),
        model: "missing".into(),
        ffmpeg: "missing".into(),
        ffprobe: "missing".into(),
        language: "de".into(),
        use_gpu: false,
        compact_audio: true,
        prompt: String::new(),
    };
    let item = queue
        .enqueue(config.clone(), library.root.clone(), None)
        .unwrap();
    assert!(queue.enqueue(config, library.root, None).is_err());
    let reopened = Queue::new(queue.root.clone());
    assert_eq!(reopened.list().unwrap()[0].state, "queued");
    assert!(reopened.run(&item.id, &false.into(), |_, _| {}).is_err());
    let jobs = reopened.list().unwrap();
    assert_eq!(jobs[0].state, "failed");
    assert!(jobs[0].error.is_some());
    assert!(source.is_file());
    assert!(reopened.cleanup(&item.id).is_err());
}
#[test]
fn catalog_pins_hashes_sizes_platforms_and_safe_entries() {
    let catalog = laut_engine::models::catalog();
    assert!(catalog.iter().any(|p| p.platform == "windows-x86_64"));
    for p in catalog {
        assert!(p.url.starts_with("https://"));
        assert_eq!(p.sha256.len(), 64);
        assert!(p.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(p.bytes > 0);
        assert!(!p.entry.contains(".."));
    }
}
