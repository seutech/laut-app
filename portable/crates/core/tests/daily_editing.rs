use laut_core::*;
use serde_json::json;
fn doc() -> Document {
    Document::parse(include_bytes!("../../../fixtures/conversation.json")).unwrap()
}
#[test]
fn json_command_split_accepts_fractional_times_with_precise_json_numbers() {
    let mut d = doc();
    let value = json!({"command":"split_segment","id":d.view().segments[0].id,"at":1.5,"offset":6});
    let command: Command = serde_json::from_str(&value.to_string()).unwrap();
    execute(&mut d, command).unwrap();
    assert_eq!(d.view().segments[0].end, 1.5);
}
#[test]
fn details_replacements_and_speaker_merge_survive_reopen_and_undo() {
    let mut d = doc();
    let original = d.raw().clone();
    execute(
        &mut d,
        Command::UpdateDetails {
            title: "Interview 👋".into(),
            notes: "Fact-check names\nKeep source.".into(),
        },
    )
    .unwrap();
    execute(
        &mut d,
        Command::ReplaceText {
            find: "Welt".into(),
            replacement: "World".into(),
        },
    )
    .unwrap();
    execute(
        &mut d,
        Command::MergeSpeakers {
            from: "b".into(),
            into: "a".into(),
        },
    )
    .unwrap();
    let mut reopened = Document::parse(&serde_json::to_vec(d.raw()).unwrap()).unwrap();
    assert_eq!(reopened.view().title, "Interview 👋");
    assert_eq!(reopened.view().speakers.len(), 1);
    assert!(reopened.view().segments[2].speaker_locked);
    assert_eq!(
        reopened.raw()["segments"][0]["originalText"],
        original["segments"][0]["originalText"]
    );
    assert_eq!(
        reopened.raw()["segments"][0]["words"],
        original["segments"][0]["words"]
    );
    for _ in 0..3 {
        execute(&mut reopened, Command::Undo).unwrap();
    }
    for field in ["title", "notes", "speakers", "segments", "futureMetadata"] {
        assert_eq!(reopened.raw()[field], original[field]);
    }
    for _ in 0..3 {
        execute(&mut reopened, Command::Redo).unwrap();
    }
    assert_eq!(reopened.view().segments[0].text, "Grüße 👋 World!");
}
#[test]
fn unicode_split_keeps_evidence_and_merge_does_not_invent_timing() {
    let mut value = doc().raw().clone();
    value["segments"][0]["text"] = json!("A 👨‍👩‍👧‍👦 e\u{301} Z");
    let mut d = Document::from_value(value).unwrap();
    let before = d.raw()["segments"].clone();
    let id = d.view().segments[0].id.clone();
    execute(
        &mut d,
        Command::SplitSegment {
            id,
            at: 1.5,
            offset: 3,
        },
    )
    .unwrap();
    let r = d.view();
    assert_eq!(r.segments[0].text, "A 👨‍👩‍👧‍👦");
    assert_eq!(r.segments[1].text, "e\u{301} Z");
    assert!(r.segments[0].words.is_empty());
    assert!(r.segments[1].speaker_locked);
    assert_eq!(
        d.raw()["segments"][0]["portableEditSources"][0]["words"],
        before[0]["words"]
    );
    execute(
        &mut d,
        Command::MergeNext {
            id: r.segments[0].id.clone(),
        },
    )
    .unwrap();
    assert_eq!(d.view().segments.len(), 3);
    assert!(d.view().segments[0].words.is_empty());
    let mut re = Document::parse(&serde_json::to_vec(d.raw()).unwrap()).unwrap();
    execute(&mut re, Command::Undo).unwrap();
    execute(&mut re, Command::Undo).unwrap();
    assert_eq!(re.raw()["segments"], before);
}
#[test]
fn invalid_editor_actions_are_atomic() {
    let mut d = doc();
    let before = d.raw().clone();
    let id = d.view().segments[0].id.clone();
    for command in [
        Command::SplitSegment {
            id: id.clone(),
            at: 0.,
            offset: 3,
        },
        Command::SplitSegment {
            id,
            at: 1.,
            offset: 999,
        },
        Command::MergeNext {
            id: d.view().segments[2].id.clone(),
        },
        Command::MergeSpeakers {
            from: "a".into(),
            into: "missing".into(),
        },
        Command::ReplaceText {
            find: "".into(),
            replacement: "x".into(),
        },
        Command::UpdateDetails {
            title: " ".into(),
            notes: "x".into(),
        },
    ] {
        assert!(execute(&mut d, command).is_err());
        assert_eq!(d.raw(), &before);
    }
}
#[test]
fn srt_preserves_local_clock_and_confirmed_exclusion_policy() {
    let mut raw = doc().raw().clone();
    raw["segments"][2]["end"] = json!(10.125);
    raw["conference"]["ranges"] = json!([{"start":0,"end":3,"confirmed":true,"excluded":true}]);
    let r = Document::from_value(raw).unwrap().view();
    let export = srt(&r);
    assert!(export.starts_with("1\n00:00:03,000 --> 00:00:06,000\nSpeaker A: Second sentence."));
    assert!(export.contains("00:00:10,125"));
    assert!(!export.contains("Grüße"));
    assert!(!export.contains("00:30:"));
}
#[test]
fn notes_missing_in_legacy_document_can_be_edited_and_undone() {
    let mut raw = doc().raw().clone();
    raw.as_object_mut().unwrap().remove("notes");
    let mut d = Document::from_value(raw).unwrap();
    let title = d.view().title;
    execute(
        &mut d,
        Command::UpdateDetails {
            title,
            notes: "New note".into(),
        },
    )
    .unwrap();
    execute(&mut d, Command::Undo).unwrap();
    assert!(d.view().notes.is_empty());
    execute(&mut d, Command::Redo).unwrap();
    assert_eq!(d.view().notes, "New note");
}

#[test]
fn applying_all_drafts_is_one_undo_and_validates_every_reference() {
    let mut d = doc();
    let original = d.raw().clone();
    let id = d.view().segments[0].id.clone();
    execute(
        &mut d,
        Command::ApplyDraft {
            title: "All edits".into(),
            notes: "New notes".into(),
            texts: std::collections::BTreeMap::from([(id, "New text".into())]),
            names: std::collections::BTreeMap::from([("a".into(), "Confirmed name".into())]),
        },
    )
    .unwrap();
    assert_eq!(d.raw()["editHistory"]["undo"].as_array().unwrap().len(), 1);
    execute(&mut d, Command::Undo).unwrap();
    for field in ["title", "notes", "segments", "speakers"] {
        assert_eq!(d.raw()[field], original[field]);
    }
    let before = d.raw().clone();
    assert!(
        execute(
            &mut d,
            Command::ApplyDraft {
                title: "No partial write".into(),
                notes: "No".into(),
                texts: std::collections::BTreeMap::from([("missing".into(), "X".into())]),
                names: Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(d.raw(), &before);
}
