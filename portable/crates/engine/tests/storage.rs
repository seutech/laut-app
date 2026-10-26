use laut_engine::{Session, jobs::parse_whisper, media, write_new};
use serde_json::{Value, json};
use std::fs;
fn fixture() -> Value {
    serde_json::from_slice(include_bytes!("../../../fixtures/conversation.json")).unwrap()
}
#[test]
fn independent_copy_preserves_metadata_and_materializes_shared_media() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("assets")).unwrap();
    fs::create_dir(root.join("recording")).unwrap();
    fs::write(root.join("conference-storage.json"), b"{\"version\":1}").unwrap();
    let hash = "a".repeat(64);
    let asset = format!("assets/{hash}.wav");
    fs::write(root.join(&asset), b"audio fixture").unwrap();
    let mut value = fixture();
    value["audioFilename"] = json!("source.wav");
    let original = serde_json::to_vec(&value).unwrap();
    let dir = root.join("recording");
    fs::write(dir.join("recording.json"), &original).unwrap();
    fs::write(
        dir.join("media-references.json"),
        serde_json::to_vec(&json!({"version":1,"root":"..","files":{"source.wav":asset}})).unwrap(),
    )
    .unwrap();
    let s = Session::open(&dir).unwrap();
    assert_eq!(fs::read(s.audio_path().unwrap()).unwrap(), b"audio fixture");
    s.save_copy(&root.join("copy")).unwrap();
    let copied = Session::open(&root.join("copy")).unwrap();
    assert_eq!(copied.document.raw(), &value);
    assert_eq!(
        fs::read(copied.audio_path().unwrap()).unwrap(),
        b"audio fixture"
    );
    assert_eq!(fs::read(dir.join("recording.json")).unwrap(), original);
    assert!(!root.join("copy/media-references.json").exists());
    assert!(s.save_copy(&root.join("copy")).is_err());
}
#[test]
fn missing_media_does_not_publish_half_copy() {
    let temp = tempfile::tempdir().unwrap();
    let mut v = fixture();
    v["audioFilename"] = json!("missing.wav");
    fs::write(
        temp.path().join("recording.json"),
        serde_json::to_vec(&v).unwrap(),
    )
    .unwrap();
    let s = Session::open(temp.path()).unwrap();
    assert!(s.save_copy(&temp.path().join("copy")).is_err());
    assert!(!temp.path().join("copy").exists());
}
#[test]
fn traversal_and_windows_paths_are_rejected_on_every_platform() {
    let temp = tempfile::tempdir().unwrap();
    for path in [
        "../secret",
        "/etc/passwd",
        "C:/private.wav",
        "..\\secret",
        "a//b",
    ] {
        assert!(media::resolve(temp.path(), path).is_err());
    }
}
#[cfg(unix)]
#[test]
fn symlink_outside_project_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("project")).unwrap();
    fs::write(temp.path().join("outside.wav"), b"outside").unwrap();
    std::os::unix::fs::symlink(
        temp.path().join("outside.wav"),
        temp.path().join("project/source.wav"),
    )
    .unwrap();
    assert!(media::resolve(&temp.path().join("project"), "source.wav").is_err());
}
#[test]
fn exports_never_overwrite_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path().join("text.txt");
    write_new(&p, b"first").unwrap();
    assert!(write_new(&p, b"second").is_err());
    assert_eq!(fs::read(p).unwrap(), b"first");
}
#[test]
fn whisper_uses_local_segment_times_and_rejects_invalid_results() {
    let output = json!({"transcription":[{"text":" Hello. ","offsets":{"from":1250,"to":3000}}]});
    let result = parse_whisper(&output, 300., 5.).unwrap();
    assert_eq!(result[0]["start"], 301.25);
    assert_eq!(result[0]["end"], 303.);
    assert_eq!(result[0]["text"], "Hello.");
    assert_eq!(result[0]["words"], json!([]));
    assert!(
        parse_whisper(
            &json!({"transcription":[{"text":"Bad","offsets":{"from":-1,"to":3000}}]}),
            0.,
            5.
        )
        .is_err()
    );
    assert!(parse_whisper(&json!({"text":"incompatible"}), 0., 5.).is_err());
}

#[test]
fn whisper_padding_keeps_text_and_original_timing_without_exceeding_audio() {
    let output =
        json!({"transcription":[{"text":"Final words.","offsets":{"from":296360,"to":301240}}]});
    let result = parse_whisper(&output, 300., 300.).unwrap();
    assert_eq!(result[0]["start"], 596.36);
    assert_eq!(result[0]["end"], 600.);
    assert_eq!(result[0]["text"], "Final words.");
    assert_eq!(result[0]["workerTiming"]["to"], 301.24);
    assert!(parse_whisper(&output, 0., 200.).is_err());
    assert!(parse_whisper(&output, f64::NAN, 300.).is_err());
}

#[test]
fn project_copy_materializes_windows_safe_names_and_retains_original_name() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("assets")).unwrap();
    fs::write(root.join("conference-storage.json"), b"{\"version\":1}").unwrap();
    let asset = format!("assets/{}.m4a", "b".repeat(64));
    fs::write(root.join(&asset), b"audio").unwrap();
    let name = "Interview | Q&A.m4a";
    let mut value = fixture();
    value["audioFilename"] = json!(name);
    let before = value["conference"].clone();
    value["conference"]["documents"] =
        json!([{"id":"document","filename":name,"title":"Attachment"}]);
    value["editHistory"]["undo"] =
        json!([{"conference":{"before":before,"after":value["conference"]}}]);
    fs::write(
        root.join("recording.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("media-references.json"),
        serde_json::to_vec(&json!({"version":1,"root":".","files":{name:asset}})).unwrap(),
    )
    .unwrap();
    Session::open(root)
        .unwrap()
        .save_copy(&root.join("copy"))
        .unwrap();
    let mut copy = Session::open(&root.join("copy")).unwrap();
    let mapped = media::portable_name(name);
    assert_ne!(mapped, name);
    assert_eq!(copy.document.raw()["audioFilename"], mapped);
    assert_eq!(
        copy.document.raw()["portableMediaOriginalNames"][&mapped],
        name
    );
    assert_eq!(fs::read(copy.audio_path().unwrap()).unwrap(), b"audio");
    assert_eq!(
        copy.document.raw()["conference"]["documents"][0]["filename"],
        mapped
    );
    copy.execute(laut_core::Command::Undo).unwrap();
    assert_eq!(copy.document.raw()["conference"]["documents"], json!([]));
    copy.execute(laut_core::Command::Redo).unwrap();
    assert_eq!(
        copy.document.raw()["conference"]["documents"][0]["filename"],
        mapped
    );
    for bad in ["CON.wav", "folder/LPT1.pdf", "trailing.", "who?.mp3"] {
        assert_ne!(media::portable_name(bad), bad);
    }
}
