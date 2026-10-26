use laut_core::Command;
use laut_engine::{Session, library::Library};
use std::{collections::BTreeMap, fs, path::Path};

fn setup(root: &Path) -> (Library, laut_engine::library::Loaded) {
    let original = root.join("Original");
    fs::create_dir(&original).unwrap();
    fs::write(
        original.join("recording.json"),
        include_bytes!("../../../fixtures/conversation.json"),
    )
    .unwrap();
    let lib = Library::create(&root.join("Library ü")).unwrap();
    let loaded = lib.import(&Session::open(&original).unwrap()).unwrap();
    (lib, loaded)
}
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, map: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                walk(root, &e.path(), map);
            } else if e.file_name() != ".edit.lock" {
                map.insert(
                    e.path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(e.path()).unwrap(),
                );
            }
        }
    }
    let mut map = BTreeMap::new();
    walk(root, root, &mut map);
    map
}
#[test]
fn trash_restore_and_backup_keep_all_files_history_and_originals() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, mut loaded) = setup(temp.path());
    let id = loaded.session.document.view().id;
    loaded
        .session
        .execute(Command::UpdateDetails {
            notes: "Original notes: Grüße 👋".into(),
            title: "Retained title".into(),
        })
        .unwrap();
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
    let folder = loaded.session.source.parent().unwrap();
    fs::create_dir(folder.join("documents")).unwrap();
    fs::write(folder.join("documents/extra.pdf"), b"original PDF bytes").unwrap();
    fs::write(folder.join("microphone.flac"), b"separate microphone track").unwrap();
    fs::write(folder.join("future-asset.bin"), [1, 2, 3]).unwrap();
    let before = tree(folder);
    let original = tree(&temp.path().join("Original"));
    let storage = lib.storage().unwrap();
    let active_bytes = storage.active_bytes;
    assert_eq!(storage.projects.len(), 1);
    assert_eq!(storage.projects[0].bytes, active_bytes);
    let token = lib.move_to_trash(&id, &loaded.revision).unwrap();
    assert!(lib.catalog().unwrap().entries.is_empty());
    assert!(lib.search("Retained").unwrap().hits.is_empty());
    assert_eq!(tree(&lib.root.join(".trash").join(&token)), before);
    let storage = lib.storage().unwrap();
    assert_eq!(storage.active_bytes, 0);
    assert_eq!(storage.trash_bytes, active_bytes);
    assert_eq!(storage.trash[0].token, token);
    assert!(lib.load(&id).is_err());
    let backup = temp.path().join("Backup");
    lib.backup(&backup).unwrap();
    let restored = Library::restore_backup(&backup, &temp.path().join("Restored")).unwrap();
    assert!(restored.catalog().unwrap().entries.is_empty());
    assert_eq!(restored.storage().unwrap().trash.len(), 1);
    let mut recovered = restored.restore_from_trash(&token).unwrap();
    assert_eq!(tree(recovered.session.source.parent().unwrap()), before);
    recovered.session.execute(Command::Undo).unwrap();
    restored
        .save(&mut recovered.session, &mut recovered.revision)
        .unwrap();
    assert_eq!(
        recovered.session.document.view().title,
        "Laut portable example"
    );
    assert_eq!(tree(&temp.path().join("Original")), original);
    let returned = lib.restore_from_trash(&token).unwrap();
    assert_eq!(tree(returned.session.source.parent().unwrap()), before);
    assert!(lib.storage().unwrap().trash.is_empty());
}
#[test]
fn refuses_stale_moves_busy_projects_and_conflicting_restores() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, mut loaded) = setup(temp.path());
    let id = loaded.session.document.view().id;
    let stale = loaded.revision.clone();
    loaded
        .session
        .execute(Command::UpdateDetails {
            notes: "Original notes: Grüße 👋".into(),
            title: "New title".into(),
        })
        .unwrap();
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
    assert!(lib.move_to_trash(&id, &stale).is_err());
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(loaded.session.source.parent().unwrap().join(".edit.lock"))
        .unwrap();
    fs2::FileExt::try_lock_shared(&lock).unwrap();
    assert!(lib.move_to_trash(&id, &loaded.revision).is_err());
    drop(lock);
    let token = lib.move_to_trash(&id, &loaded.revision).unwrap();
    let new_copy = lib
        .import(&Session::open(&temp.path().join("Original")).unwrap())
        .unwrap();
    let before = tree(new_copy.session.source.parent().unwrap());
    assert!(lib.restore_from_trash(&token).is_err());
    assert_eq!(tree(new_copy.session.source.parent().unwrap()), before);
    assert_eq!(lib.storage().unwrap().trash.len(), 1);
    for bad in [
        "../outside",
        "/outside",
        "0--bad--bad",
        "0--00000000-0000-0000-0000-000000000000--../outside",
    ] {
        assert!(lib.restore_from_trash(bad).is_err());
    }
}
#[test]
fn library_lock_coordinates_moves_saves_imports_and_backups() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, mut loaded) = setup(temp.path());
    let id = loaded.session.document.view().id;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lib.root.join(".library.lock"))
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    assert!(lib.move_to_trash(&id, &loaded.revision).is_err());
    assert!(lib.save(&mut loaded.session, &mut loaded.revision).is_err());
    assert!(lib.backup(&temp.path().join("Backup")).is_err());
    assert!(lib.import(&loaded.session).is_err());
    drop(lock);
    assert_eq!(lib.catalog().unwrap().entries.len(), 1);
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
}
#[test]
fn damaged_trash_entry_does_not_hide_recoverable_projects() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, mut loaded) = setup(temp.path());
    let id = loaded.session.document.view().id;
    lib.save(&mut loaded.session, &mut loaded.revision).unwrap();
    let token = lib.move_to_trash(&id, &loaded.revision).unwrap();
    fs::write(
        lib.root.join(".trash").join(&token).join("recording.json"),
        b"broken",
    )
    .unwrap();
    fs::create_dir(lib.root.join(".trash/unknown-folder")).unwrap();
    let storage = lib.storage().unwrap();
    assert_eq!(storage.trash.len(), 1);
    assert!(storage.trash[0].recovered);
    assert!(!storage.warnings.is_empty());
    assert!(lib.backup(&temp.path().join("Backup")).is_err());
    let recovered = lib.restore_from_trash(&token).unwrap();
    assert!(recovered.recovered);
    assert_eq!(fs::read(&recovered.session.source).unwrap(), b"broken");
}
#[cfg(unix)]
#[test]
fn symlinks_are_not_counted_or_followed_by_moves() {
    let temp = tempfile::tempdir().unwrap();
    let (lib, loaded) = setup(temp.path());
    let id = loaded.session.document.view().id;
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("large"), vec![1; 100000]).unwrap();
    std::os::unix::fs::symlink(&outside, lib.root.join(".trash")).unwrap();
    let storage = lib.storage().unwrap();
    assert_eq!(storage.trash_bytes, 0);
    assert!(!storage.warnings.is_empty());
    assert!(lib.move_to_trash(&id, &loaded.revision).is_err());
    assert!(lib.load(&id).is_ok());
    assert!(lib.backup(&temp.path().join("Backup")).is_err());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}
