//! Local, reversible library cleanup. A single directory rename retains every file.
use crate::library::{Library, Loaded, Revision, load_folder};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const TRASH: &str = ".trash";

#[derive(Clone, Debug, Serialize)]
pub struct StoredProject {
    pub id: String,
    pub title: String,
    pub bytes: u64,
    #[serde(skip)]
    pub revision: Revision,
}
#[derive(Clone, Debug, Serialize)]
pub struct TrashedProject {
    pub token: String,
    pub id: String,
    pub title: String,
    pub removed_at_ms: u64,
    pub bytes: u64,
    pub recovered: bool,
}
#[derive(Default, Debug, Serialize)]
pub struct Storage {
    pub active_bytes: u64,
    pub trash_bytes: u64,
    pub other_bytes: u64,
    pub available_bytes: u64,
    pub projects: Vec<StoredProject>,
    pub trash: Vec<TrashedProject>,
    pub warnings: Vec<String>,
}

fn directory(path: &Path, parent: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink(),
        "Invalid storage folder: {}",
        path.display()
    );
    ensure!(
        path.canonicalize()?.parent() == Some(parent),
        "Storage folder escapes library"
    );
    Ok(())
}

// Logical file sizes, not physical disk allocation. Never follow symlinks.
fn size(path: &Path, depth: usize, warnings: &mut Vec<String>) -> u64 {
    let result = (|| -> Result<u64> {
        ensure!(depth < 64, "Folder nesting exceeds scan limit");
        let m = fs::symlink_metadata(path)?;
        ensure!(
            !m.file_type().is_symlink(),
            "Symbolic link was not followed"
        );
        if m.is_file() {
            return Ok(m.len());
        }
        ensure!(m.is_dir(), "Special file was not counted");
        let mut total = 0u64;
        for child in fs::read_dir(path)? {
            match child {
                Ok(child) => total = total.saturating_add(size(&child.path(), depth + 1, warnings)),
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            }
        }
        Ok(total)
    })();
    match result {
        Ok(value) => value,
        Err(e) => {
            warnings.push(format!("{}: {e:#}", path.display()));
            0
        }
    }
}

fn parse_token(token: &str) -> Result<(u64, String)> {
    let parts: Vec<_> = token.split("--").collect();
    ensure!(parts.len() == 3, "Invalid trash entry");
    let time: u64 = parts[0].parse().context("Invalid removal time")?;
    let nonce = uuid::Uuid::parse_str(parts[1])?;
    let id = uuid::Uuid::parse_str(parts[2])?.to_string().to_uppercase();
    ensure!(
        token == format!("{time}--{nonce}--{id}"),
        "Invalid trash entry name"
    );
    Ok((time, id))
}

impl Library {
    pub(crate) fn trash_folder(&self, token: &str) -> Result<(PathBuf, String, u64)> {
        let (time, id) = parse_token(token)?;
        let root = self.root.join(TRASH);
        directory(&root, &self.root)?;
        let folder = root.join(token);
        directory(&folder, &root)?;
        Ok((folder, id, time))
    }
    pub(crate) fn trash_catalog(&self) -> Result<Vec<TrashedProject>> {
        let root = self.root.join(TRASH);
        match fs::symlink_metadata(&root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        directory(&root, &self.root)?;
        let mut entries = vec![];
        for entry in fs::read_dir(root)? {
            let token = entry?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Invalid trash entry name"))?;
            let (folder, id, removed_at_ms) = self.trash_folder(&token)?;
            let loaded = load_folder(&folder, &id)?;
            entries.push(TrashedProject {
                token,
                id,
                title: loaded.session.document.view().title,
                removed_at_ms,
                bytes: 0,
                recovered: loaded.recovered,
            });
        }
        entries.sort_by(|a, b| {
            b.removed_at_ms
                .cmp(&a.removed_at_ms)
                .then_with(|| a.token.cmp(&b.token))
        });
        Ok(entries)
    }
    pub fn storage(&self) -> Result<Storage> {
        let _lock = self.lock(false)?;
        let mut result = Storage {
            available_bytes: fs2::available_space(&self.root)?,
            ..Default::default()
        };
        for child in fs::read_dir(&self.root)? {
            let child = child?;
            let name = child.file_name().to_string_lossy().into_owned();
            let bytes = size(&child.path(), 0, &mut result.warnings);
            if name == TRASH {
                result.trash_bytes = bytes;
            } else if uuid::Uuid::parse_str(&name).is_ok() {
                result.active_bytes = result.active_bytes.saturating_add(bytes);
                match self.load(&name) {
                    Ok(loaded) => {
                        let view = loaded.session.document.view();
                        result.projects.push(StoredProject {
                            id: view.id,
                            title: view.title,
                            bytes,
                            revision: loaded.revision,
                        });
                        if loaded.recovered {
                            result
                                .warnings
                                .push(format!("{name}: previous valid version recovered"));
                        }
                    }
                    Err(e) => result.warnings.push(format!("{name}: {e:#}")),
                }
            } else {
                result.other_bytes = result.other_bytes.saturating_add(bytes);
            }
        }
        // List entries independently: one damaged item must not hide the rest of the bin.
        let trash = self.root.join(TRASH);
        if trash.try_exists()? {
            let scan = (|| -> Result<()> {
                directory(&trash, &self.root)?;
                for child in fs::read_dir(&trash)? {
                    let child = child?;
                    let token = child.file_name().to_string_lossy().into_owned();
                    let entry = (|| -> Result<TrashedProject> {
                        let (folder, id, removed_at_ms) = self.trash_folder(&token)?;
                        let loaded = load_folder(&folder, &id)?;
                        Ok(TrashedProject {
                            token: token.clone(),
                            id,
                            title: loaded.session.document.view().title,
                            removed_at_ms,
                            bytes: size(&folder, 0, &mut result.warnings),
                            recovered: loaded.recovered,
                        })
                    })();
                    match entry {
                        Ok(entry) => result.trash.push(entry),
                        Err(e) => result.warnings.push(format!("{token}: {e:#}")),
                    }
                }
                Ok(())
            })();
            if let Err(e) = scan {
                result.warnings.push(format!("Trash: {e:#}"));
            }
        }
        result
            .projects
            .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.id.cmp(&b.id)));
        result.trash.sort_by(|a, b| {
            b.removed_at_ms
                .cmp(&a.removed_at_ms)
                .then_with(|| a.token.cmp(&b.token))
        });
        Ok(result)
    }
    pub fn move_to_trash(&self, id: &str, expected: &Revision) -> Result<String> {
        let _library_lock = self.lock(true)?;
        let folder = self.folder(id)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(folder.join(".edit.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).context("Recording is in use; retry later")?;
        let loaded = self.load(id)?;
        ensure!(
            &loaded.revision == expected,
            "Recording changed elsewhere; refresh before moving it to trash"
        );
        let root = self.root.join(TRASH);
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        directory(&root, &self.root)?;
        let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let token = format!(
            "{time}--{}--{}",
            uuid::Uuid::new_v4(),
            uuid::Uuid::parse_str(&loaded.session.document.view().id)?
                .to_string()
                .to_uppercase()
        );
        let target = root.join(&token);
        ensure!(!target.try_exists()?, "Trash entry already exists");
        fs::rename(folder, target)
            .context("Cannot move recording; files remain at their original location")?;
        Ok(token)
    }
    pub fn restore_from_trash(&self, token: &str) -> Result<Loaded> {
        let _library_lock = self.lock(true)?;
        let (folder, id, _) = self.trash_folder(token)?;
        let _loaded = load_folder(&folder, &id)?;
        let target = self.root.join(&id);
        ensure!(
            fs::symlink_metadata(&target).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "A recording with this ID already exists; nothing was overwritten. Keep both copies or choose another library"
        );
        fs::rename(folder, &target).context("Cannot restore recording; files remain in trash")?;
        self.load(&id)
    }
}
