use laut_core::{Command, conference};
use laut_engine::{
    Session,
    documents::{self, PdfConfig},
    library::Library,
};
use serde_json::json;
use std::{fs, sync::atomic::AtomicBool};
fn library(root: &std::path::Path) -> (Library, String) {
    let src = root.join("source");
    fs::create_dir(&src).unwrap();
    fs::write(
        src.join("recording.json"),
        include_bytes!("../../../fixtures/conversation.json"),
    )
    .unwrap();
    let lib = Library::create(&root.join("library")).unwrap();
    let loaded = lib.import(&Session::open(&src).unwrap()).unwrap();
    (lib, loaded.session.document.view().id)
}
#[test]
fn pdf_history_media_survives_undo_and_project_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, id) = library(tmp.path());
    let mut loaded = lib.load(&id).unwrap();
    let folder = loaded.session.source.parent().unwrap();
    fs::create_dir(folder.join("documents")).unwrap();
    fs::write(folder.join("documents/a.pdf"), b"source pdf bytes").unwrap();
    let mut c = conference::context(&loaded.session.document);
    c["documents"] = json!([{"id":conference::id(),"filename":"documents/a.pdf","title":"PDF","nameCandidates":[],"pages":[{"number":1,"text":"native","ocrText":"OCR unique searchable evidence"}]}]);
    loaded
        .session
        .execute(Command::EditConference { value: c })
        .unwrap();
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
    assert_eq!(lib.search("unique searchable").unwrap().hits.len(), 1);
    loaded.session.execute(Command::Undo).unwrap();
    let copy = tmp.path().join("copy");
    loaded.session.save_copy(&copy).unwrap();
    assert_eq!(
        fs::read(copy.join("documents/a.pdf")).unwrap(),
        b"source pdf bytes"
    );
    let mut reopened = Session::open(&copy).unwrap();
    reopened.execute(Command::Redo).unwrap();
    assert!(
        documents::page_path(
            &reopened,
            &reopened.document.view().conference.unwrap().documents[0].id,
            1
        )
        .unwrap()
        .is_file()
    );
}
#[test]
fn cancelled_or_invalid_pdf_does_not_publish_or_run_worker() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, id) = library(tmp.path());
    let mut loaded = lib.load(&id).unwrap();
    let original = fs::read(&loaded.session.source).unwrap();
    let pdf = tmp.path().join("fake.pdf");
    fs::write(&pdf, b"not a PDF").unwrap();
    assert!(
        documents::import_documents(
            &mut loaded.session,
            &lib,
            &mut loaded.revision,
            std::slice::from_ref(&pdf),
            &PdfConfig::default(),
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
    assert_eq!(fs::read(&loaded.session.source).unwrap(), original);
    assert_eq!(
        fs::read_dir(loaded.session.source.parent().unwrap().join("documents"))
            .unwrap()
            .count(),
        0
    );
    assert!(
        documents::extract(
            &pdf,
            &PdfConfig::default(),
            &AtomicBool::new(true),
            |_, _| {}
        )
        .unwrap_err()
        .to_string()
        .contains("Cancelled")
    );
}
#[test]
fn archive_is_self_contained_and_escapes_html_without_changing_original() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, id) = library(tmp.path());
    let mut loaded = lib.load(&id).unwrap();
    loaded
        .session
        .execute(Command::UpdateDetails {
            title: "<script>alert(1)</script>".into(),
            notes: "note".into(),
        })
        .unwrap();
    let original = fs::read(&loaded.session.source).unwrap();
    let out = tmp.path().join("research");
    laut_engine::conference_archive::export(
        &loaded.session,
        &out,
        &PdfConfig::default(),
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    let html = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(out.join("transcript-original.json").is_file());
    assert_eq!(fs::read(&loaded.session.source).unwrap(), original);
    assert!(
        laut_engine::conference_archive::export(
            &loaded.session,
            &out,
            &PdfConfig::default(),
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
}
