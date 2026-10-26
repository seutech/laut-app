//! Files and local workers. Deliberately independent of desktop/egui and audio devices.
pub mod backup;
pub mod conference_archive;
pub mod diarization;
pub mod documents;
pub mod downloads;
pub mod jobs;
pub mod library;
pub mod library_search;
pub mod library_storage;
pub mod media;
pub mod models;
pub mod preferences;
pub mod queue;
pub mod recording;
pub mod research;
use anyhow::{Context, Result, ensure};
use laut_core::{Command, Document, execute};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Session {
    pub document: Document,
    pub source: PathBuf,
    pub dirty: bool,
}
impl Session {
    pub fn open(path: &Path) -> Result<Self> {
        let file = if path.is_dir() {
            path.join("recording.json")
        } else {
            path.to_owned()
        };
        let source = file.canonicalize().context("Cannot open recording.json")?;
        ensure!(
            !source.parent().unwrap().join("COPY-INCOMPLETE").exists(),
            "Project copy was interrupted; open the original project"
        );
        let bytes = read_json_bytes(&source)?;
        Ok(Self {
            document: Document::parse(&bytes)?,
            source,
            dirty: false,
        })
    }
    pub fn execute(&mut self, cmd: Command) -> Result<Value> {
        let result = execute(&mut self.document, cmd)?;
        if result["changed"] == true {
            self.dirty = true;
        }
        Ok(result)
    }
    pub fn audio_path(&self) -> Result<PathBuf> {
        let name = self
            .document
            .view()
            .audio_filename
            .context("No audio attached")?;
        media::resolve(self.source.parent().unwrap(), &name)
    }
    /// A new independent folder. Never overwrites the live Swift library or existing exports.
    pub fn save_copy(&self, destination: &Path) -> Result<()> {
        ensure!(
            !destination.exists(),
            "Destination already exists; choose a new folder"
        );
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        ensure!(parent.is_dir(), "Parent directory does not exist");
        let staging = tempfile::Builder::new()
            .prefix(".laut-copy-")
            .tempdir_in(parent)?;
        let recording = self.document.view();
        let mut copied_document = self.document.raw().clone();
        let mut remapped = std::collections::BTreeMap::new();
        let mut files = vec![];
        let mut total = 0u64;
        let mut media_names: std::collections::BTreeSet<String> = recording
            .media_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        for direction in ["undo", "redo"] {
            for revision in self.document.raw()["editHistory"][direction]
                .as_array()
                .into_iter()
                .flatten()
            {
                for side in ["before", "after"] {
                    for doc in revision["conference"][side]["documents"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if let Some(name) = doc["filename"].as_str() {
                            media_names.insert(name.to_owned());
                        }
                    }
                }
            }
        }
        for name in &media_names {
            let source = media::resolve(self.source.parent().unwrap(), name)?;
            total = total
                .checked_add(fs::metadata(&source)?.len())
                .context("Media size overflow")?;
            let portable = media::portable_name(name);
            if portable != *name {
                remapped.insert(name.to_owned(), portable.clone());
            }
            files.push((portable, source));
        }
        if !remapped.is_empty() {
            remap_media(&mut copied_document, &remapped)?;
        }
        let mut destinations = std::collections::BTreeSet::new();
        for (name, _) in &files {
            ensure!(
                destinations.insert(name.to_lowercase()),
                "Media filenames collide on a case-insensitive filesystem"
            );
        }
        let data = serde_json::to_vec_pretty(&copied_document)?;
        ensure!(
            data.len() as u64 <= MAX_JSON_BYTES,
            "Project exceeds the local document size limit"
        );
        ensure!(
            fs2::available_space(parent)?
                > total
                    .saturating_add(data.len() as u64)
                    .saturating_add(512 * 1024 * 1024),
            "Insufficient free space (512 MiB reserve)"
        );
        for (name, source) in files {
            let target = staging.path().join(name);
            fs::create_dir_all(target.parent().unwrap())?;
            let before = media::digest(&source)?;
            fs::copy(&source, &target)?;
            ensure!(
                before == media::digest(&target)? && before == media::digest(&source)?,
                "Media changed while copying"
            );
        }
        fs::write(staging.path().join("recording.json"), &data)?;
        fs::File::open(staging.path().join("recording.json"))?.sync_all()?;
        // Reserve the destination with create_dir: unlike rename, this cannot replace an
        // empty directory created concurrently. A marker makes an interrupted publish clear.
        fs::create_dir(destination)?;
        let marker = destination.join("COPY-INCOMPLETE");
        fs::write(
            &marker,
            b"Copy incomplete. Original project is unchanged.\n",
        )?;
        // Close the directory enumeration handle before moving its children. Windows can
        // otherwise reject the rename with ERROR_ACCESS_DENIED while ReadDir is still alive.
        let staged = fs::read_dir(staging.path())?
            .map(|entry| {
                let entry = entry?;
                Ok((entry.path(), entry.file_name()))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        for (source, name) in staged {
            fs::rename(&source, destination.join(&name)).with_context(|| {
                format!("Cannot publish copied project file {}", source.display())
            })?;
        }
        fs::remove_file(marker)?;
        Ok(())
    }
}

fn remap_media(
    document: &mut Value,
    names: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    fn replace(value: &mut Value, names: &std::collections::BTreeMap<String, String>) {
        if let Some(name) = value.as_str().and_then(|s| names.get(s)) {
            *value = Value::String(name.clone());
        }
    }
    for key in ["audioFilename", "secondaryAudioFilename"] {
        if let Some(value) = document.get_mut(key) {
            replace(value, names);
        }
    }
    fn conference(value: &mut Value, names: &std::collections::BTreeMap<String, String>) {
        if let Some(documents) = value.get_mut("documents").and_then(Value::as_array_mut) {
            for attachment in documents {
                if let Some(value) = attachment.get_mut("filename") {
                    replace(value, names);
                }
            }
        }
    }
    if let Some(value) = document.get_mut("conference") {
        conference(value, names);
    }
    if let Some(tracks) = document
        .get_mut("portableCapture")
        .and_then(|v| v.get_mut("tracks"))
        .and_then(Value::as_array_mut)
    {
        for track in tracks {
            if let Some(name) = track.get_mut("filename") {
                replace(name, names);
            }
        }
    }
    // Revisions compare whole conference snapshots; keep their media references aligned.
    if let Some(history) = document.get_mut("editHistory") {
        for direction in ["undo", "redo"] {
            if let Some(revisions) = history.get_mut(direction).and_then(Value::as_array_mut) {
                for revision in revisions {
                    if let Some(change) = revision.get_mut("conference") {
                        for side in ["before", "after"] {
                            if let Some(value) = change.get_mut(side) {
                                conference(value, names);
                            }
                        }
                    }
                }
            }
        }
    }
    let originals = document
        .as_object_mut()
        .context("Invalid recording")?
        .entry("portableMediaOriginalNames")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("Unknown media name metadata")?;
    for (original, portable) in names {
        ensure!(
            !originals.contains_key(portable),
            "Media name history collision"
        );
        originals.insert(portable.clone(), Value::String(original.clone()));
    }
    Ok(())
}
pub const MAX_JSON_BYTES: u64 = 512 * 1024 * 1024;
pub fn read_json_bytes(path: &Path) -> Result<Vec<u8>> {
    // Bound accidental huge/binary imports; no truncation is accepted as a valid document.
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_JSON_BYTES,
        "JSON exceeds 512 MiB limit"
    );
    Ok(bytes)
}
pub fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}
