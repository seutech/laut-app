use laut_core::*;
use serde_json::json;
fn doc() -> Document {
    Document::parse(include_bytes!("../../../fixtures/conversation.json")).unwrap()
}
#[test]
fn import_keeps_unknown_fields_and_precise_numbers() {
    let d = doc();
    let bytes = serde_json::to_vec(d.raw()).unwrap();
    let re = Document::parse(&bytes).unwrap();
    assert_eq!(d.raw(), re.raw());
    assert_eq!(
        re.raw()["futureMetadata"]["largeInteger"].to_string(),
        "9007199254740993"
    );
}
#[test]
fn edits_preserve_original_words_and_other_metadata() {
    let mut d = doc();
    let before = d.raw().clone();
    let id = d.view().segments[0].id.clone();
    execute(
        &mut d,
        Command::EditSegment {
            id,
            text: "Corrected 👋 text".into(),
        },
    )
    .unwrap();
    assert_eq!(
        d.raw()["segments"][0]["originalText"],
        before["segments"][0]["originalText"]
    );
    assert_eq!(
        d.raw()["segments"][0]["words"],
        before["segments"][0]["words"]
    );
    for key in [
        "futureMetadata",
        "source",
        "createdAt",
        "conference",
        "notes",
    ] {
        assert_eq!(d.raw()[key], before[key]);
    }
    assert!(word_ranges(&d.view().segments[0]).is_empty());
    let mut reopened = Document::parse(&serde_json::to_vec(d.raw()).unwrap()).unwrap();
    execute(&mut reopened, Command::Undo).unwrap();
    assert_eq!(reopened.raw()["segments"], before["segments"]);
    execute(&mut reopened, Command::Redo).unwrap();
    assert_eq!(reopened.raw()["segments"][0]["text"], "Corrected 👋 text");
}
#[test]
fn grouping_matches_swift_and_manual_names_keep_flow() {
    let mut d = doc();
    assert_eq!(paragraphs(&d.view()).len(), 2);
    execute(
        &mut d,
        Command::RenameSpeaker {
            id: "a".into(),
            name: "Confirmed person".into(),
        },
    )
    .unwrap();
    assert_eq!(paragraphs(&d.view()).len(), 2);
    assert_eq!(d.raw()["segments"][0]["speakerLocked"], true);
    let second_id = d.view().segments[1].id.clone();
    execute(
        &mut d,
        Command::AssignSpeaker {
            segment_id: second_id,
            speaker_id: Some("a".into()),
        },
    )
    .unwrap();
    assert_eq!(paragraphs(&d.view()).len(), 3);
}
#[test]
fn chapters_and_legacy_locks_break_paragraphs() {
    let mut value = doc().raw().clone();
    value["conference"]["chapters"]
        .as_array_mut()
        .unwrap()
        .push(json!({"start":3,"title":"Next"}));
    assert_eq!(
        paragraphs(&Document::from_value(value).unwrap().view()).len(),
        3
    );
    let mut value = doc().raw().clone();
    value["segments"][1]["speakerLocked"] = json!(true);
    assert_eq!(
        paragraphs(&Document::from_value(value).unwrap().view()).len(),
        3
    );
}
#[test]
fn exclusion_is_only_a_projection_and_edited_boundary_text_survives() {
    let mut value = doc().raw().clone();
    value["conference"]["ranges"] = json!([{"start":1,"end":2,"confirmed":true,"excluded":true}]);
    let mut d = Document::from_value(value).unwrap();
    let original = d.raw().clone();
    assert!(plain_text(&d.view()).starts_with("Grüße Welt!"));
    assert_eq!(d.raw(), &original);
    let id = d.view().segments[0].id.clone();
    execute(
        &mut d,
        Command::EditSegment {
            id,
            text: "Edited boundary 👋".into(),
        },
    )
    .unwrap();
    assert!(plain_text(&d.view()).contains("Edited boundary 👋"));
}
#[test]
fn unconfirmed_ranges_are_never_hidden() {
    let mut value = doc().raw().clone();
    value["conference"]["ranges"] = json!([{"start":0,"end":12,"confirmed":false,"excluded":true}]);
    assert_eq!(
        plain_text(&Document::from_value(value).unwrap().view()),
        plain_text(&doc().view())
    );
}
#[test]
fn word_navigation_and_source_offset_are_distinct() {
    let r = doc().view();
    let ranges = word_ranges(&r.segments[0]);
    assert_eq!(&r.segments[0].text[ranges[1].start..ranges[1].end], "👋");
    assert_eq!(ranges[1].time, 1.);
    assert_eq!(r.source_time(1.), 1801.);
    assert!(markdown(&r).contains("00:30:00"));
}
#[test]
fn history_conflict_is_transactional() {
    let mut d = doc();
    let id = d.view().segments[0].id.clone();
    execute(
        &mut d,
        Command::EditSegment {
            id,
            text: "Change".into(),
        },
    )
    .unwrap();
    let mut value = d.raw().clone();
    value["segments"][0]["text"] = json!("External edit");
    let mut d = Document::from_value(value).unwrap();
    let before = d.raw().clone();
    assert!(execute(&mut d, Command::Undo).is_err());
    assert_eq!(d.raw(), &before);
}
#[test]
fn invalid_commands_and_duplicate_ids_do_not_modify_document() {
    let mut d = doc();
    let before = d.raw().clone();
    assert!(
        execute(
            &mut d,
            Command::RenameSpeaker {
                id: "missing".into(),
                name: "Name".into()
            }
        )
        .is_err()
    );
    assert_eq!(d.raw(), &before);
    let mut value = before;
    value["segments"][1]["id"] = value["segments"][0]["id"].clone();
    assert!(Document::from_value(value).is_err());
}
#[test]
fn seven_hour_search_and_export_keep_all_text() {
    let mut value = doc().raw().clone();
    let template = value["segments"][0].clone();
    let mut segments = vec![];
    for i in 0..10000 {
        let mut s = template.clone();
        s["id"] = json!(uuid::Uuid::new_v4().to_string());
        s["start"] = json!(i as f64 * 2.52);
        s["end"] = json!(i as f64 * 2.52 + 2.);
        s["text"] = json!(format!("Sentence {i} about AI."));
        s["originalText"] = s["text"].clone();
        s["words"] = json!([]);
        segments.push(s);
    }
    value["segments"] = json!(segments);
    value["duration"] = json!(25200.);
    let r = Document::from_value(value).unwrap().view();
    let started = std::time::Instant::now();
    let text = plain_text(&r);
    let hits = search(&r, "about AI");
    assert_eq!(hits.len(), 10000);
    assert!(text.contains("Sentence 9999 about AI."));
    eprintln!(
        "Seven-hour fixture: search + export {:?}",
        started.elapsed()
    );
}
