//! An explicitly owned portable library. External/Swift projects are copied on import.
//! JSON remains authoritative; saves retain the previous valid version and detect stale writers.
use crate::{Session, read_json_bytes, write_new};
use anyhow::{Context, Result, bail, ensure};
use laut_core::Document;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const MARKER: &str = "laut-library.json";
#[derive(Clone, Debug)]
pub struct Library {
    pub root: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Revision {
    current: Option<String>,
    previous: Option<String>,
}
pub struct Loaded {
    pub session: Session,
    pub revision: Revision,
    pub recovered: bool,
}
#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub duration: f64,
    pub created_at: f64,
    pub recovered: bool,
}
#[derive(Serialize, Default)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

pub fn default_root() -> Result<PathBuf> {
    if cfg!(target_os = "windows") {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("LautPortable"))
            .context("LOCALAPPDATA is unavailable; choose a library folder");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .context("Home folder unavailable; choose a library folder")?;
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Application Support/LautPortable"))
    } else {
        Ok(std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"))
            .join("laut-portable"))
    }
}

impl Library {
    pub fn create(root: &Path) -> Result<Self> {
        if root.join(MARKER).exists() {
            return Self::open(root);
        }
        if root.exists() {
            ensure!(
                root.is_dir() && fs::read_dir(root)?.next().is_none(),
                "Choose an empty folder; existing data is not adopted automatically"
            );
        } else {
            fs::create_dir_all(root)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        write_new(
            &root.join(MARKER),
            b"{\"kind\":\"laut-portable-library\",\"version\":1}\n",
        )?;
        Self::open(root)
    }
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize().context("Library folder missing")?;
        ensure!(
            !root.join("COPY-INCOMPLETE").exists(),
            "Library copy or restore is incomplete; open the original library"
        );
        let marker: serde_json::Value =
            serde_json::from_slice(&read_json_bytes(&root.join(MARKER))?)?;
        ensure!(
            marker["kind"] == "laut-portable-library" && marker["version"] == 1,
            "Unsupported portable library"
        );
        Ok(Self { root })
    }
    /// Returns an owner only for an explicitly marked Rust library, never the Swift library.
    pub fn owner(path: &Path) -> Result<Option<(Self, String)>> {
        if !path.is_dir() && path.file_name().is_none_or(|name| name != "recording.json") {
            return Ok(None);
        }
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().context("Missing project folder")?
        };
        let Some(root) = folder.parent() else {
            return Ok(None);
        };
        if !root.join(MARKER).exists() {
            return Ok(None);
        }
        let id = folder
            .file_name()
            .and_then(|v| v.to_str())
            .context("Invalid recording folder")?;
        Ok(Some((Self::open(root)?, id.to_owned())))
    }
    pub(crate) fn folder(&self, id: &str) -> Result<PathBuf> {
        let uuid = uuid::Uuid::parse_str(id).context("Invalid recording ID")?;
        let folder = self.root.join(uuid.to_string().to_uppercase());
        ensure!(
            !fs::symlink_metadata(&folder)?.file_type().is_symlink(),
            "Recording folder is a symlink"
        );
        ensure!(
            folder.canonicalize()?.parent() == Some(self.root.as_path()),
            "Recording escapes library"
        );
        validate_project_folder(&folder)?;
        Ok(folder)
    }
    /// Coordinates directory moves with saves, imports and complete backups.
    pub(crate) fn lock(&self, exclusive: bool) -> Result<fs::File> {
        let path = self.root.join(".library.lock");
        if let Ok(meta) = fs::symlink_metadata(&path) {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Invalid library lock"
            );
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("Cannot open library lock {}", path.display()))?;
        if exclusive {
            fs2::FileExt::try_lock_exclusive(&lock)
        } else {
            fs2::FileExt::try_lock_shared(&lock)
        }
        .context("Library is busy in another operation; retry when it finishes")?;
        Ok(lock)
    }
    pub fn import(&self, session: &Session) -> Result<Loaded> {
        let _library_lock = self.lock(true).context("Cannot lock library for import")?;
        let id = uuid::Uuid::parse_str(&session.document.view().id)?
            .to_string()
            .to_uppercase();
        session
            .save_copy(&self.root.join(&id))
            .context("Cannot copy project into library")?;
        self.load(&id).context("Cannot open imported project")
    }
    pub fn load(&self, id: &str) -> Result<Loaded> {
        let folder = self.folder(id)?;
        load_folder(&folder, id)
    }
    pub fn catalog(&self) -> Result<Catalog> {
        let mut result = Catalog::default();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if uuid::Uuid::parse_str(&id).is_err() {
                continue;
            }
            match self.load(&id) {
                Ok(loaded) => {
                    let view = loaded.session.document.view();
                    result.entries.push(Entry {
                        id: view.id,
                        title: view.title,
                        duration: view.duration,
                        created_at: loaded.session.document.raw()["createdAt"]
                            .as_f64()
                            .unwrap_or(0.),
                        recovered: loaded.recovered,
                    });
                    if loaded.recovered {
                        result.warnings.push(format!(
                            "Recovered previous version; damaged original retained: {id}"
                        ));
                    }
                }
                Err(error) => result.warnings.push(format!("{id}: {error:#}")),
            }
        }
        result.entries.sort_by(|a, b| {
            b.created_at
                .total_cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(result)
    }
    pub fn save(&self, session: &mut Session, expected: &mut Revision) -> Result<()> {
        let _library_lock = self.lock(false)?;
        let id = session.document.view().id;
        let folder = self.folder(&id)?;
        ensure!(
            session.source == folder.join("recording.json"),
            "Session belongs to another project"
        );
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(folder.join(".edit.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("This recording is being saved by another process")?;
        let current = optional_bytes(&session.source)?;
        let previous = optional_bytes(&folder.join("recording.previous.json"))?;
        ensure!(
            &revision(&current, &previous) == expected,
            "Recording changed elsewhere; reopen before saving. Your in-memory edit has not been written"
        );
        let data = serde_json::to_vec_pretty(session.document.raw())?;
        ensure!(
            data.len() as u64 <= crate::MAX_JSON_BYTES,
            "Project exceeds the local document size limit; previous version retained"
        );
        ensure!(
            fs2::available_space(&folder)?
                > (data.len() as u64)
                    .saturating_mul(3)
                    .saturating_add(512 * 1024 * 1024),
            "Insufficient free space for a safe save"
        );
        if let Some(bytes) = &current {
            if Document::parse(bytes).is_ok_and(|d| d.view().id == id) {
                atomic_write(&folder.join("recording.previous.json"), bytes)?;
            } else {
                write_new(
                    &folder.join(format!("recording.damaged-{}.json", uuid::Uuid::new_v4())),
                    bytes,
                )?;
            }
        }
        atomic_write(&session.source, &data)?;
        *expected = revision(
            &Some(data),
            &optional_bytes(&folder.join("recording.previous.json"))?,
        );
        session.dirty = false;
        Ok(())
    }
}

pub(crate) fn validate_project_folder(folder: &Path) -> Result<()> {
    ensure!(
        !folder.join("COPY-INCOMPLETE").exists(),
        "Project copy was interrupted; original is retained"
    );
    for name in ["recording.json", "recording.previous.json", ".edit.lock"] {
        if let Ok(meta) = fs::symlink_metadata(folder.join(name)) {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Invalid library file: {name}"
            );
        }
    }
    Ok(())
}
pub(crate) fn load_folder(folder: &Path, id: &str) -> Result<Loaded> {
    validate_project_folder(folder)?;
    let current = optional_bytes(&folder.join("recording.json"))?;
    let previous = optional_bytes(&folder.join("recording.previous.json"))?;
    let decode = |bytes: &Option<Vec<u8>>| -> Option<Document> {
        let doc = Document::parse(bytes.as_ref()?).ok()?;
        (uuid::Uuid::parse_str(&doc.view().id).ok()? == uuid::Uuid::parse_str(id).ok()?)
            .then_some(doc)
    };
    let (document, recovered) = if let Some(doc) = decode(&current) {
        (doc, false)
    } else if let Some(doc) = decode(&previous) {
        (doc, true)
    } else {
        bail!(
            "Recording and previous version are unreadable; files retained: {}",
            folder.display()
        );
    };
    Ok(Loaded {
        session: Session {
            document,
            source: folder.join("recording.json"),
            dirty: false,
        },
        revision: revision(&current, &previous),
        recovered,
    })
}
fn optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(_) => Ok(Some(read_json_bytes(path)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn revision(current: &Option<Vec<u8>>, previous: &Option<Vec<u8>>) -> Revision {
    let hash = |data: &Option<Vec<u8>>| data.as_ref().map(|b| format!("{:x}", Sha256::digest(b)));
    Revision {
        current: hash(current),
        previous: hash(previous),
    }
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(bytes)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
