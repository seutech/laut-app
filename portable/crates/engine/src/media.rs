use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn safe(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

/// macOS allows characters/reserved names that cannot be created on Windows.
/// Short, deterministic replacements also keep copied projects below common path limits.
pub fn portable_name(name: &str) -> String {
    let compatible = name.len() <= 120
        && name.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.contains(['<', '>', ':', '"', '\\', '|', '?', '*'])
                && !part.chars().any(char::is_control)
                && !part.ends_with(['.', ' '])
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
        });
    if compatible {
        return name.to_owned();
    }
    let extension = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .filter(|s| s.len() <= 10 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or("media")
        .to_ascii_lowercase();
    format!("media-{:x}.{extension}", Sha256::digest(name.as_bytes()))
}
#[derive(Deserialize)]
struct References {
    version: u32,
    root: String,
    files: BTreeMap<String, String>,
}
pub fn resolve(folder: &Path, logical: &str) -> Result<PathBuf> {
    ensure!(safe(logical), "Invalid media path");
    let sidecar = folder.join("media-references.json");
    if sidecar.exists() {
        let refs: References = serde_json::from_slice(&crate::read_json_bytes(&sidecar)?)?;
        ensure!(refs.version == 1, "Unknown media reference version");
        if let Some(asset) = refs.files.get(logical) {
            let parts: Vec<_> = refs.root.split('/').collect();
            ensure!(
                refs.root == "." || (parts.len() <= 8 && parts.iter().all(|p| *p == "..")),
                "Invalid shared root"
            );
            let name = asset
                .strip_prefix("assets/")
                .context("Invalid shared media path")?;
            let (hash, ext) = name.split_once('.').context("Invalid shared filename")?;
            ensure!(
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && !ext.is_empty()
                    && ext
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
                "Invalid shared filename"
            );
            let root = folder.join(&refs.root).canonicalize()?;
            let header: serde_json::Value = serde_json::from_slice(&crate::read_json_bytes(
                &root.join("conference-storage.json"),
            )?)?;
            ensure!(header["version"] == 1, "Missing shared media archive");
            let base = root.join("assets").canonicalize()?;
            ensure!(base.starts_with(&root), "Media directory escapes archive");
            let file = root.join(asset).canonicalize()?;
            ensure!(
                file.starts_with(base) && file.is_file(),
                "Shared media escapes archive or is missing"
            );
            return Ok(file);
        }
    }
    let base = folder.canonicalize()?;
    let file = folder
        .join(logical)
        .canonicalize()
        .context("Media missing; transcript is still available")?;
    ensure!(
        file.starts_with(base) && file.is_file(),
        "Media escapes project folder"
    );
    Ok(file)
}
pub fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
