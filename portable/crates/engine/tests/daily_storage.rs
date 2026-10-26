use laut_core::Command;
use laut_engine::{
    Session,
    library::{Collection, Library},
    preferences::{Draft, Preferences, StateStore},
};
use std::{collections::BTreeMap, fs};
fn library(root: &std::path::Path) -> (Library, String) {
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("recording.json"),
        include_bytes!("../../../fixtures/conversation.json"),
    )
    .unwrap();
    let lib = Library::create(&root.join("library")).unwrap();
    let loaded = lib.import(&Session::open(&source).unwrap()).unwrap();
    (lib, loaded.session.document.view().id)
}
#[test]
fn preferences_and_multiple_drafts_survive_a_new_store_without_touching_source() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, id) = library(temp.path());
    let session = lib.load(&id).unwrap().session;
    let original = fs::read(&session.source).unwrap();
    let store = StateStore::new(temp.path().join("state"));
    let prefs = Preferences {
        english: true,
        library: Some(lib.root.clone()),
        last_project: Some(session.source.clone()),
        reading_font_size: 18.,
        follow_playback: false,
        playback_speed: 1.5,
        show_navigation: false,
        ..Default::default()
    };
    store.save_preferences(&prefs).unwrap();
    let segment = session.document.view().segments[0].id.clone();
    let draft = Draft {
        version: 1,
        source: session.source.clone(),
        document: session.document.raw().clone(),
        dirty: false,
        selected: Some(segment.clone()),
        texts: BTreeMap::from([(segment, "Unapplied correction".into())]),
        title: "Pending title".into(),
        notes: "Pending note".into(),
        speaker_names: vec![("a".into(), "Confirmed person".into())],
    };
    let a = StateStore::draft_id();
    let b = StateStore::draft_id();
    store.save_draft(&a, &draft).unwrap();
    store.save_draft(&b, &draft).unwrap();
    let reopened = StateStore::new(store.root.clone());
    assert_eq!(reopened.preferences().unwrap(), prefs);
    let (drafts, warnings) = reopened.drafts().unwrap();
    assert!(warnings.is_empty());
    assert_eq!(drafts.len(), 2);
    assert_eq!(drafts[0].1, draft);
    assert_eq!(
        drafts[0].1.session().unwrap().document.raw(),
        session.document.raw()
    );
    reopened.remove_draft(&a).unwrap();
    assert_eq!(reopened.drafts().unwrap().0.len(), 1);
    assert_eq!(fs::read(&session.source).unwrap(), original);
    fs::write(store.root.join("preferences.json"), b"broken").unwrap();
    assert!(reopened.preferences().is_err());
    assert_eq!(
        fs::read(store.root.join("preferences.json")).unwrap(),
        b"broken"
    );
    fs::write(store.root.join(format!("draft-{a}.json")), b"broken").unwrap();
    assert_eq!(reopened.drafts().unwrap().1.len(), 1);
}
#[test]
fn older_preferences_get_readable_workspace_defaults() {
    let prefs: Preferences = serde_json::from_str(r#"{"version":1,"english":true}"#).unwrap();
    assert!(prefs.english);
    assert_eq!(prefs.reading_font_size, 16.);
    assert_eq!(prefs.playback_speed, 1.);
    assert!(prefs.follow_playback && prefs.show_navigation);
    assert_eq!(prefs.research.generation_backend, "local");
    assert_eq!(prefs.research.external_profile, "openai-default");
    let encoded = serde_json::to_string(&prefs).unwrap();
    assert!(!encoded.contains("api_key"));
    assert!(!encoded.contains("Bearer "));
}
#[test]
fn collections_round_trip_and_reject_ambiguous_or_invalid_members() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, id) = library(temp.path());
    let mut event = Collection::new("Telekom AI Investor Day");
    event.id = event.id.to_lowercase();
    event.name = "  Telekom AI Investor Day  ".into();
    event.recording_ids.push(id.to_lowercase());
    lib.save_collections(&[event.clone()]).unwrap();
    event.id = event.id.to_uppercase();
    event.name = event.name.trim().into();
    event.recording_ids[0] = id.clone();
    assert_eq!(lib.collections().unwrap(), [event.clone()]);

    let mut duplicate_name = Collection::new("telekom ai investor day");
    duplicate_name.recording_ids.push(id.clone());
    assert!(
        lib.save_collections(&[event.clone(), duplicate_name])
            .is_err()
    );
    assert_eq!(lib.collections().unwrap(), [event.clone()]);

    event.recording_ids.push(id);
    assert!(lib.save_collections(&[event]).is_err());
}
#[test]
fn invalid_playback_speed_is_normalized_without_losing_preferences() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::new(temp.path().join("state"));
    for speed in [-1., 0., 3., 1.1] {
        store
            .save_preferences(&Preferences {
                english: true,
                playback_speed: speed,
                ..Default::default()
            })
            .unwrap();
        let prefs = store.preferences().unwrap();
        assert!(prefs.english);
        assert_eq!(prefs.playback_speed, 1.);
    }
}
#[test]
fn credentials_embedded_in_an_external_endpoint_are_never_persisted() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::new(temp.path().join("state"));
    let mut preferences = Preferences::default();
    preferences.research.generation_backend = "external".into();
    preferences.research.external_endpoint = "https://secret@example.test/v1".into();
    assert!(store.save_preferences(&preferences).is_err());
    assert!(!store.root.join("preferences.json").exists());
    preferences.research.external_endpoint = "https://example.test/v1?api_key=secret".into();
    assert!(store.save_preferences(&preferences).is_err());
    assert!(!store.root.join("preferences.json").exists());
}
#[test]
fn backup_restore_keeps_media_history_and_previous_version_and_rejects_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, id) = library(temp.path());
    let mut loaded = lib.load(&id).unwrap();
    loaded
        .session
        .execute(Command::UpdateDetails {
            title: "Saved title".into(),
            notes: "Saved note".into(),
        })
        .unwrap();
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
    let media = loaded.session.source.parent().unwrap().join("extra.pdf");
    fs::write(&media, b"local original attachment").unwrap();
    let mut collection = Collection::new("Investor day");
    collection.recording_ids.push(id.clone());
    lib.save_collections(&[collection.clone()]).unwrap();
    let backup = temp.path().join("backup");
    lib.backup(&backup).unwrap();
    assert!(lib.backup(&backup).is_err());
    let restored = Library::restore_backup(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.collections().unwrap(), [collection]);
    let mut re = restored.load(&id).unwrap();
    assert_eq!(re.session.document.raw(), loaded.session.document.raw());
    assert_eq!(
        fs::read(restored.root.join(&id).join("extra.pdf")).unwrap(),
        b"local original attachment"
    );
    assert!(
        restored
            .root
            .join(&id)
            .join("recording.previous.json")
            .exists()
    );
    re.session.execute(Command::Undo).unwrap();
    restored.save(&mut re.session, &mut re.revision).unwrap();
    assert_eq!(re.session.document.view().title, "Laut portable example");
    fs::write(backup.join(&id).join("extra.pdf"), b"tampered").unwrap();
    let failed = temp.path().join("failed");
    assert!(Library::restore_backup(&backup, &failed).is_err());
    assert!(!failed.exists());
    assert_eq!(
        lib.load(&id).unwrap().session.document.view().title,
        "Saved title"
    );
    assert!(lib.backup(&lib.root.join("nested-backup")).is_err());
}
#[test]
fn library_search_finds_notes_titles_speakers_and_pdf_text_without_excluded_segments() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, id) = library(temp.path());
    let mut loaded = lib.load(&id).unwrap();
    let mut raw = loaded.session.document.raw().clone();
    raw["notes"] = serde_json::json!("Nämé needle note");
    raw["conference"]["documents"] = serde_json::json!([{"id":"paper","filename":"paper.pdf","title":"Report","pages":[{"number":2,"text":"Needle in PDF"}]}]);
    raw["conference"]["ranges"] =
        serde_json::json!([{"start":7,"end":10,"confirmed":true,"excluded":true}]);
    loaded.session.document = laut_core::Document::from_value(raw).unwrap();
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
    let results = lib.search("NEEDLE").unwrap();
    assert_eq!(results.hits.len(), 2);
    assert!(results.hits.iter().any(|h| h.kind == "document"));
    assert!(results.hits.iter().any(|h| h.kind == "notes"));
    assert_eq!(lib.search("Speaker A").unwrap().hits.len(), 2);
    assert!(lib.search("Here is the answer").unwrap().hits.is_empty());
    assert_eq!(
        lib.search("portable example").unwrap().hits[0].kind,
        "title"
    );
    assert!(
        lib.backup(&temp.path().join("missing-media-backup"))
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn backups_refuse_symlinks_and_restore_never_follows_them() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, id) = library(temp.path());
    let outside = temp.path().join("outside");
    fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, lib.root.join(&id).join("link")).unwrap();
    assert!(lib.backup(&temp.path().join("backup")).is_err());
}
