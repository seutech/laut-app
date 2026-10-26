use laut_core::{Command, Document, execute};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/conversation.json")).unwrap()
}
fn apply(v: Value, turns: Value) -> Document {
    let mut doc = Document::from_value(v).unwrap();
    let mut mapping = json!({});
    for (cluster, id) in [("one", "a"), ("two", "b")] {
        if turns
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["speaker"] == cluster)
        {
            mapping[cluster] = json!(id);
        }
    }
    let command:Command=serde_json::from_value(json!({"command":"apply_speaker_analysis","analysis":{"id":"test","turns":turns},"mapping":mapping})).unwrap();
    execute(&mut doc, command).unwrap();
    doc
}
#[test]
fn word_split_preserves_text_unknown_fields_and_undo() {
    let before = fixture();
    let mut doc = apply(
        before.clone(),
        json!([{"start":0.,"end":1.,"speaker":"one"},{"start":1.,"end":3.,"speaker":"two"},{"start":3.,"end":6.,"speaker":"one"},{"start":7.,"end":10.,"speaker":"two"}]),
    );
    let r = doc.view();
    assert_eq!(r.segments.len(), 4);
    assert_eq!(r.segments[0].text, "Grüße");
    assert_eq!(r.segments[1].text, "👋 Welt!");
    assert_eq!(r.segments[1].speaker_id.as_deref(), Some("b"));
    assert_eq!(doc.raw()["segments"][1]["futureFlag"], "keep");
    assert_eq!(
        doc.raw()["portableDiarizationSources"][before["segments"][0]["id"].as_str().unwrap()],
        before["segments"][0]
    );
    execute(&mut doc, Command::Undo).unwrap();
    assert_eq!(doc.raw()["segments"], before["segments"]);
    assert_eq!(doc.raw()["speakers"], before["speakers"]);
    execute(&mut doc, Command::Redo).unwrap();
    assert_eq!(doc.view().segments.len(), 4);
}
#[test]
fn locks_and_edited_text_are_not_split_or_reconstructed() {
    let mut before = fixture();
    before["segments"][0]["speakerLocked"] = json!(true);
    before["segments"][1]["text"] = json!("My correction");
    let doc = apply(
        before.clone(),
        json!([{"start":0.,"end":4.,"speaker":"two"},{"start":4.,"end":6.,"speaker":"one"}]),
    );
    assert_eq!(doc.raw()["segments"][0], before["segments"][0]);
    assert_eq!(doc.view().segments[1].text, "My correction");
    assert_eq!(doc.view().segments.len(), 3);
    assert!(doc.view().segments[1].speaker_id.is_none());
}
#[test]
fn overlap_stays_unknown_and_same_voice_gap_does_not_create_false_change() {
    let doc = apply(
        fixture(),
        json!([{"start":0.,"end":1.,"speaker":"one"},{"start":2.,"end":4.,"speaker":"one"},{"start":7.,"end":10.,"speaker":"one"},{"start":7.,"end":10.,"speaker":"two"}]),
    );
    assert_eq!(doc.view().segments[0].speaker_id.as_deref(), Some("a"));
    assert!(doc.view().segments.last().unwrap().speaker_id.is_none());
}
#[test]
fn short_interjection_is_retained_and_no_wrong_nearest_speaker() {
    let doc = apply(
        fixture(),
        json!([{"start":0.,"end":1.,"speaker":"one"},{"start":1.,"end":1.3,"speaker":"two"},{"start":1.3,"end":3.,"speaker":"one"},{"start":4.,"end":4.5,"speaker":"two"}]),
    );
    // A brief competing voice prevents filling the whole pause with speaker one.
    assert_eq!(doc.view().segments[0].speaker_id.as_deref(), Some("a"));
    assert!(
        doc.view()
            .segments
            .iter()
            .find(|s| s.start == 3.)
            .unwrap()
            .speaker_id
            .is_none()
    );
}
#[test]
fn names_are_suggested_only_from_locked_unambiguous_evidence() {
    let mut raw = fixture();
    raw["segments"][0]["speakerLocked"] = json!(true);
    let doc = Document::from_value(raw).unwrap();
    let turns = json!([{"start":0.,"end":3.,"speaker":"cluster_0"},{"start":7.,"end":10.,"speaker":"cluster_1"}]);
    let map = laut_core::speakers::suggested_mapping(&doc, &turns).unwrap();
    assert_eq!(map, BTreeMap::from([("cluster_0".into(), "a".into())]));
}
#[test]
fn invalid_analysis_is_transactional() {
    let mut doc = Document::from_value(fixture()).unwrap();
    let before = doc.raw().clone();
    let result = execute(
        &mut doc,
        Command::ApplySpeakerAnalysis {
            analysis: json!({"turns":[{"start":2.,"end":1.,"speaker":"bad"}]}),
            mapping: BTreeMap::new(),
        },
    );
    assert!(result.is_err());
    assert_eq!(doc.raw(), &before);
}

#[test]
fn seven_hour_alignment_keeps_all_words_and_does_not_scan_every_turn_per_word() {
    let mut raw = fixture();
    raw["duration"] = json!(25200.);
    let mut segments = vec![];
    let mut turns = vec![];
    for i in 0..8400 {
        let start = (i * 3) as f64;
        segments.push(json!({"id":uuid::Uuid::new_v4().to_string(),"start":start,"end":start+3.,"text":"one two three","originalText":"one two three","words":[{"text":"one","start":start,"end":start+1.},{"text":"two","start":start+1.,"end":start+2.},{"text":"three","start":start+2.,"end":start+3.}]}));
        turns.push(json!({"start":start,"end":start+3.,"speaker":if i%2==0{"one"}else{"two"}}));
    }
    raw["segments"] = json!(segments);
    let doc = apply(raw, json!(turns));
    let r = doc.view();
    assert_eq!(r.segments.len(), 8400);
    assert_eq!(
        r.segments.iter().map(|s| s.words.len()).sum::<usize>(),
        25200
    );
    assert!(r.segments.iter().all(|s| s.text == "one two three"));
}

#[test]
fn confirmed_new_names_lock_attribution_and_stale_recording_is_rejected() {
    let mut doc = Document::from_value(fixture()).unwrap();
    let mut analysis = json!({"recordingID":doc.view().id,"id":"names-test","names":{"voice":"Confirmed person"},"turns":[{"start":0.,"end":10.,"speaker":"voice"}]});
    execute(
        &mut doc,
        Command::ApplySpeakerAnalysis {
            analysis: analysis.clone(),
            mapping: BTreeMap::new(),
        },
    )
    .unwrap();
    assert!(
        doc.view()
            .speakers
            .iter()
            .any(|s| s.name == "Confirmed person")
    );
    assert!(doc.view().segments.iter().all(|s| s.speaker_locked));
    analysis["recordingID"] = json!(uuid::Uuid::new_v4().to_string());
    let before = doc.raw().clone();
    assert!(
        execute(
            &mut doc,
            Command::ApplySpeakerAnalysis {
                analysis,
                mapping: BTreeMap::new()
            }
        )
        .is_err()
    );
    assert_eq!(doc.raw(), &before);
}
