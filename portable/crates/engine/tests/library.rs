use laut_core::Command;
use laut_engine::{Session, library::Library};
use std::fs;

fn source(root: &std::path::Path) -> Session {
    fs::create_dir(root).unwrap();
    fs::write(
        root.join("recording.json"),
        include_bytes!("../../../fixtures/conversation.json"),
    )
    .unwrap();
    Session::open(root).unwrap()
}
#[test]
fn import_save_reopen_and_undo_preserve_source() {
    let temp = tempfile::tempdir().unwrap();
    let original = source(&temp.path().join("original"));
    let bytes = fs::read(&original.source).unwrap();
    let library = Library::create(&temp.path().join("library")).unwrap();
    let mut loaded = library.import(&original).unwrap();
    let id = loaded.session.document.view().id;
    let segment = loaded.session.document.view().segments[0].id.clone();
    loaded
        .session
        .execute(Command::EditSegment {
            id: segment,
            text: "Corrected locally.".into(),
        })
        .unwrap();
    library
        .save(&mut loaded.session, &mut loaded.revision)
        .unwrap();
    assert!(!loaded.session.dirty);
    let mut reopened = library.load(&id).unwrap();
    assert_eq!(
        reopened.session.document.view().segments[0].text,
        "Corrected locally."
    );
    reopened.session.execute(Command::Undo).unwrap();
    library
        .save(&mut reopened.session, &mut reopened.revision)
        .unwrap();
    assert_eq!(
        library.load(&id).unwrap().session.document.raw()["segments"],
        original.document.raw()["segments"]
    );
    assert_eq!(fs::read(&original.source).unwrap(), bytes);
    assert!(library.import(&original).is_err());
    assert_eq!(library.catalog().unwrap().entries.len(), 1);
    assert!(Library::owner(&original.source).unwrap().is_none());
    assert!(Library::owner(&loaded.session.source).unwrap().is_some());
}
#[test]
fn stale_writers_are_refused_without_losing_either_edit() {
    let temp = tempfile::tempdir().unwrap();
    let original = source(&temp.path().join("original"));
    let library = Library::create(&temp.path().join("library")).unwrap();
    let mut first = library.import(&original).unwrap();
    let id = first.session.document.view().id;
    let mut second = library.load(&id).unwrap();
    for (loaded, text) in [(&mut first, "first"), (&mut second, "second")] {
        let segment = loaded.session.document.view().segments[0].id.clone();
        loaded
            .session
            .execute(Command::EditSegment {
                id: segment,
                text: text.into(),
            })
            .unwrap();
    }
    library
        .save(&mut first.session, &mut first.revision)
        .unwrap();
    assert!(
        library
            .save(&mut second.session, &mut second.revision)
            .is_err()
    );
    assert_eq!(second.session.document.view().segments[0].text, "second");
    assert_eq!(
        library.load(&id).unwrap().session.document.view().segments[0].text,
        "first"
    );
}
#[test]
fn recovery_keeps_damaged_bytes_and_last_valid_recording() {
    let temp = tempfile::tempdir().unwrap();
    let original = source(&temp.path().join("original"));
    let library = Library::create(&temp.path().join("library")).unwrap();
    let mut loaded = library.import(&original).unwrap();
    library
        .save(&mut loaded.session, &mut loaded.revision)
        .unwrap();
    let id = loaded.session.document.view().id;
    fs::write(&loaded.session.source, b"broken json").unwrap();
    let catalog = library.catalog().unwrap();
    assert!(catalog.entries[0].recovered);
    assert_eq!(catalog.warnings.len(), 1);
    let mut recovered = library.load(&id).unwrap();
    assert!(recovered.recovered);
    library
        .save(&mut recovered.session, &mut recovered.revision)
        .unwrap();
    assert!(!library.load(&id).unwrap().recovered);
    let damaged = fs::read_dir(recovered.session.source.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .find(|p| {
            p.file_name()
                .to_string_lossy()
                .starts_with("recording.damaged-")
        })
        .unwrap();
    assert_eq!(fs::read(damaged.path()).unwrap(), b"broken json");
    fs::remove_file(&recovered.session.source).unwrap();
    assert!(library.load(&id).unwrap().recovered);
}
#[test]
fn refuses_adopting_existing_data_and_leaves_unreadable_items_visible_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    let original = source(&temp.path().join("original"));
    assert!(Library::create(original.source.parent().unwrap()).is_err());
    let library = Library::create(&temp.path().join("library")).unwrap();
    let loaded = library.import(&original).unwrap();
    fs::write(loaded.session.source, b"broken").unwrap();
    let catalog = library.catalog().unwrap();
    assert!(catalog.entries.is_empty());
    assert_eq!(catalog.warnings.len(), 1);
    assert!(library.load("../outside").is_err());
}
#[cfg(unix)]
#[test]
fn rejects_symlinked_library_records() {
    let temp = tempfile::tempdir().unwrap();
    let original = source(&temp.path().join("original"));
    let library = Library::create(&temp.path().join("library")).unwrap();
    let id = original.document.view().id;
    std::os::unix::fs::symlink(original.source.parent().unwrap(), library.root.join(&id)).unwrap();
    assert!(library.load(&id).is_err());
    assert_eq!(library.catalog().unwrap().warnings.len(), 1);
}
