use laut_core::{conference::*, *};
use serde_json::json;
fn doc() -> Document {
    Document::parse(include_bytes!("../../../fixtures/conversation.json")).unwrap()
}
fn attachment(text: &str) -> serde_json::Value {
    json!({"id":"B0000000-0000-4000-8000-000000000001","filename":"documents/reference.pdf","title":"Reference","nameCandidates":[],"pages":[{"number":1,"text":text,"ocrText":"Scanned crucial figure 37 percent","future":42}]})
}
#[test]
fn conference_changes_keep_originals_and_swift_history() {
    let mut d = doc();
    let original = d.raw().clone();
    let mut c = context(&d);
    c["future"] = json!({"keep":true});
    c["documents"]
        .as_array_mut()
        .unwrap()
        .push(attachment("Reference text"));
    c["slides"] = json!([{"id":id(),"documentID":c["documents"][0]["id"],"page":1,"start":1.5,"confirmed":true,"useful":true}]);
    execute(&mut d, Command::EditConference { value: c.clone() }).unwrap();
    assert_eq!(d.raw()["segments"], original["segments"]);
    assert_eq!(d.raw()["futureMetadata"], original["futureMetadata"]);
    let mut d = Document::parse(&serde_json::to_vec(d.raw()).unwrap()).unwrap();
    execute(&mut d, Command::Undo).unwrap();
    assert_eq!(d.raw()["conference"], original["conference"]);
    execute(&mut d, Command::Redo).unwrap();
    assert_eq!(d.raw()["conference"], c);
    assert!(page_text(&d.view().conference.unwrap().documents[0].pages[0]).contains("37 percent"));
}
#[test]
fn invalid_conference_is_transactional() {
    for invalid in [
        json!({"id":id(),"documentID":"missing","page":1,"start":1.0,"confirmed":true}),
        json!({"id":id(),"documentID":"missing","page":1,"start":-2.0,"confirmed":false}),
    ] {
        let mut d = doc();
        let before = d.raw().clone();
        let mut c = context(&d);
        c["slides"] = json!([invalid]);
        assert!(execute(&mut d, Command::EditConference { value: c }).is_err());
        assert_eq!(before, *d.raw());
    }
    let d = doc();
    let mut c = context(&d);
    let mut a = attachment("x");
    a["filename"] = json!("documents/../escape.pdf");
    c["documents"] = json!([a]);
    assert!(validate(&c, 12.).is_err());
    c = context(&d);
    c["ranges"] = json!([{"id":id(),"start":3,"end":13,"confirmed":true,"excluded":true}]);
    assert!(validate(&c, 12.).is_err());
}
#[test]
fn proposals_never_exclude_and_confirmation_is_reversible() {
    let mut d = doc();
    let mut raw = d.raw().clone();
    raw["duration"] = json!(60);
    raw["segments"][0]["text"] = json!("Let's watch a video");
    raw["segments"][2]["text"] = json!("That video shows the outcome");
    raw["segments"][2]["start"] = json!(40);
    raw["segments"][2]["end"] = json!(45);
    d = Document::from_value(raw).unwrap();
    let original = plain_text(&d.view());
    let mut c = context(&d);
    c["ranges"] = json!(range_suggestions(&d.view()));
    assert_eq!(c["ranges"].as_array().unwrap().len(), 1);
    execute(&mut d, Command::EditConference { value: c.clone() }).unwrap();
    assert_eq!(plain_text(&d.view()), original);
    c["ranges"][0]["confirmed"] = json!(true);
    c["ranges"][0]["excluded"] = json!(true);
    execute(&mut d, Command::EditConference { value: c }).unwrap();
    assert!(!plain_text(&d.view()).contains("Second sentence"));
    assert_eq!(d.view().segments.len(), 3);
    execute(&mut d, Command::Undo).unwrap();
    assert_eq!(plain_text(&d.view()), original);
}
#[test]
fn thematic_matches_need_specific_evidence_and_keep_rejections() {
    let text = "Quantum photonics architecture silicon lasers coherent networking optical switching reduces electricity consumption across advanced computational datacenter infrastructure";
    let mut raw = doc().raw().clone();
    raw["segments"][0]["text"] = json!(text);
    let mut d = Document::from_value(raw).unwrap();
    let mut c = context(&d);
    c["documents"] = json!([attachment(text)]);
    execute(&mut d, Command::EditConference { value: c }).unwrap();
    let p = slide_suggestions(&d.view());
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["confirmed"], false);
    assert_eq!(p[0]["portableMatch"]["visualMatch"], false);
    let mut c = context(&d);
    c["slides"] = json!(p);
    c["slides"][0]["useful"] = json!(false);
    execute(&mut d, Command::EditConference { value: c }).unwrap();
    assert!(slide_suggestions(&d.view()).is_empty());
}
#[test]
fn names_require_explicit_intro_not_a_mention() {
    let mut raw = doc().raw().clone();
    raw["conference"]["vocabulary"] = json!("Jane Example\nJohn Sample");
    raw["segments"][0]["text"] = json!("Thank you Jane Example for the help");
    let r = Document::from_value(raw.clone()).unwrap();
    assert!(name_suggestions(&r.view()).is_empty());
    raw["segments"][0]["text"] = json!("My name is Jane Example, hello.");
    let d = Document::from_value(raw).unwrap();
    let p = name_suggestions(&d.view());
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["name"], "Jane Example");
    assert_eq!(p[0]["speakerID"], "a");
}
