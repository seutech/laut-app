//! Verified directory backups. Restoration always creates a separate library.
use crate::{library::Library, media, read_json_bytes, write_new};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const MANIFEST: &str = "laut-backup.json";
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    files: BTreeMap<String, String>,
}
fn files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "Backup refuses symlinks: {}",
            entry.path().display()
        );
        if kind.is_dir() {
            files(root, &entry.path(), out)?;
        } else {
            ensure!(kind.is_file(), "Backup requires regular files");
            if entry.file_name() == ".edit.lock" {
                continue;
            }
            let relative = entry.path().strip_prefix(root)?.to_owned();
            if relative != Path::new(MANIFEST) {
                out.push(relative);
            }
        }
    }
    Ok(())
}
fn name(path: &Path) -> Result<String> {
    ensure!(
        path.components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Unsafe backup path"
    );
    let value = path
        .to_str()
        .context("Non-Unicode backup path")?
        .replace(std::path::MAIN_SEPARATOR, "/");
    ensure!(
        !value.contains('\\') && !value.contains(':'),
        "Non-portable backup path"
    );
    Ok(value)
}
fn copy_checked(source: &Path, target: &Path) -> Result<String> {
    let before = media::digest(source)?;
    fs::create_dir_all(target.parent().context("Missing destination parent")?)?;
    fs::copy(source, target)?;
    fs::File::open(target)?.sync_all()?;
    ensure!(
        before == media::digest(target)? && before == media::digest(source)?,
        "File changed during backup"
    );
    Ok(before)
}
fn staging(destination: &Path) -> Result<tempfile::TempDir> {
    ensure!(
        !destination.exists(),
        "Destination already exists; choose a new folder"
    );
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ensure!(parent.is_dir(), "Destination parent is missing");
    Ok(tempfile::Builder::new()
        .prefix(".laut-backup-")
        .tempdir_in(parent)?)
}
fn publish(stage: tempfile::TempDir, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    let marker = destination.join("COPY-INCOMPLETE");
    fs::write(&marker, b"Incomplete backup or restore; original retained")?;
    for entry in fs::read_dir(stage.path())? {
        let e = entry?;
        fs::rename(e.path(), destination.join(e.file_name()))?;
    }
    fs::remove_file(marker)?;
    #[cfg(unix)]
    fs::File::open(destination)?.sync_all()?;
    Ok(())
}
impl Library {
    pub fn backup(&self, destination: &Path) -> Result<()> {
        let _library_lock = self.lock(false)?;
        let stage = staging(destination)?;
        ensure!(
            !stage.path().canonicalize()?.starts_with(&self.root),
            "Choose a backup location outside the library"
        );
        let catalog = self.catalog()?;
        ensure!(
            catalog.warnings.is_empty(),
            "Resolve unreadable/recovered projects before creating a backup: {}",
            catalog.warnings.join("; ")
        );
        let mut manifest = Manifest {
            version: 1,
            files: BTreeMap::new(),
        };
        manifest.files.insert(
            "laut-library.json".into(),
            copy_checked(
                &self.root.join("laut-library.json"),
                &stage.path().join("laut-library.json"),
            )?,
        );
        let mut projects: Vec<_> = catalog
            .entries
            .into_iter()
            .map(|e| (self.root.join(e.id.to_uppercase()), e.id))
            .collect();
        for entry in self.trash_catalog()? {
            ensure!(
                !entry.recovered,
                "Resolve recovered trash projects before backing up"
            );
            projects.push((self.trash_folder(&entry.token)?.0, entry.id));
        }
        for (folder, id) in projects {
            // Keep a whole recording, including its previous version, consistent during copying.
            let lock = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(folder.join(".edit.lock"))?;
            fs2::FileExt::try_lock_shared(&lock)
                .context("A project is being saved; retry backup")?;
            let loaded = crate::library::load_folder(&folder, &id)?;
            for media in loaded.session.document.view().media_names() {
                crate::media::resolve(&folder, media)?;
            }
            let mut paths = vec![];
            files(&self.root, &folder, &mut paths)?;
            let total = paths.iter().try_fold(0u64, |sum, p| -> Result<u64> {
                Ok(sum.saturating_add(fs::metadata(self.root.join(p))?.len()))
            })?;
            ensure!(
                fs2::available_space(stage.path())? > total.saturating_add(512 * 1024 * 1024),
                "Insufficient space for backup"
            );
            for p in paths {
                manifest.files.insert(
                    name(&p)?,
                    copy_checked(&self.root.join(&p), &stage.path().join(&p))?,
                );
            }
        }
        write_new(
            &stage.path().join(MANIFEST),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        publish(stage, destination)
    }
    pub fn restore_backup(backup: &Path, destination: &Path) -> Result<Self> {
        ensure!(
            !backup.join("COPY-INCOMPLETE").exists(),
            "Backup is incomplete"
        );
        ensure!(
            !fs::symlink_metadata(backup.join(MANIFEST))?
                .file_type()
                .is_symlink(),
            "Invalid backup manifest"
        );
        let manifest: Manifest = serde_json::from_slice(&read_json_bytes(&backup.join(MANIFEST))?)?;
        ensure!(
            manifest.version == 1 && manifest.files.contains_key("laut-library.json"),
            "Unsupported backup"
        );
        let mut paths = vec![];
        files(backup, backup, &mut paths)?;
        let mut actual = paths.iter().map(|p| name(p)).collect::<Result<Vec<_>>>()?;
        actual.sort();
        ensure!(
            actual == manifest.files.keys().cloned().collect::<Vec<_>>(),
            "Backup contents differ from manifest"
        );
        let stage = staging(destination)?;
        let total = paths.iter().try_fold(0u64, |sum, p| -> Result<u64> {
            Ok(sum.saturating_add(fs::metadata(backup.join(p))?.len()))
        })?;
        ensure!(
            fs2::available_space(stage.path())? > total.saturating_add(512 * 1024 * 1024),
            "Insufficient space for restore"
        );
        for p in paths {
            let key = name(&p)?;
            let digest = copy_checked(&backup.join(&p), &stage.path().join(&p))?;
            ensure!(
                manifest.files.get(&key) == Some(&digest),
                "Backup checksum failed: {key}"
            );
        }
        let restored = Library::open(stage.path())?;
        let validated = restored.catalog()?;
        ensure!(
            validated.warnings.is_empty(),
            "Backup contains unreadable projects"
        );
        for entry in &validated.entries {
            let loaded = restored.load(&entry.id)?;
            for media in loaded.session.document.view().media_names() {
                crate::media::resolve(loaded.session.source.parent().unwrap(), media)?;
            }
        }
        for entry in restored.trash_catalog()? {
            ensure!(
                !entry.recovered,
                "Backup contains a recovered trash project"
            );
            let folder = restored.trash_folder(&entry.token)?.0;
            let loaded = crate::library::load_folder(&folder, &entry.id)?;
            for media in loaded.session.document.view().media_names() {
                crate::media::resolve(&folder, media)?;
            }
        }
        publish(stage, destination)?;
        Library::open(destination)
    }
}
