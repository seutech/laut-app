//! Explicit, resumable HTTPS installs. Inference never calls this module implicitly.
use crate::{jobs::atomic_json, media::digest, read_json_bytes};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub role: String,
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub archive: bool,
    pub entry: String,
    pub license: String,
    pub source: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Manifest {
    version: u32,
    package: Package,
    files: BTreeMap<String, String>,
}
pub fn catalog() -> Vec<Package> {
    serde_json::from_str(include_str!("model-catalog.json")).expect("built-in catalog")
}
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
pub fn package(id: &str) -> Result<Package> {
    catalog()
        .into_iter()
        .find(|p| p.id == id)
        .context("Unknown model package")
}
fn relative(p: &Path) -> bool {
    !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}
fn manifest(root: &Path, p: &Package) -> Result<Manifest> {
    let m: Manifest =
        serde_json::from_slice(&read_json_bytes(&root.join(&p.id).join("installed.json"))?)?;
    ensure!(
        m.version == 1 && m.package.sha256 == p.sha256 && m.package.id == p.id,
        "Package manifest changed"
    );
    Ok(m)
}
pub fn installed(root: &Path, p: &Package) -> Option<PathBuf> {
    let m = manifest(root, p).ok()?;
    let entry = root.join(&p.id).join(&p.entry);
    (entry.is_file() && m.files.contains_key(&p.entry)).then_some(entry)
}
pub fn verify(root: &Path, p: &Package) -> Result<PathBuf> {
    let m = manifest(root, p)?;
    ensure!(
        !m.files.is_empty() && m.files.contains_key(&p.entry),
        "Incomplete package"
    );
    for (name, sha) in m.files {
        ensure!(relative(Path::new(&name)), "Invalid manifest path");
        let path = root.join(&p.id).join(name);
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink() && digest(&path)? == sha,
            "Installed package changed: {}",
            path.display()
        );
    }
    Ok(root.join(&p.id).join(&p.entry))
}
fn lock(root: &Path, id: &str) -> Result<fs::File> {
    fs::create_dir_all(root)?;
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!(".{id}.lock")))?;
    fs2::FileExt::try_lock_exclusive(&f).context("Package is in use by another installer")?;
    Ok(f)
}
pub fn install(
    root: &Path,
    id: &str,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<PathBuf> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Download paused; no network request started"
    );
    let p = package(id)?;
    ensure!(
        p.platform == "all" || p.platform == platform(),
        "Package is for another platform"
    );
    let _lock = lock(root, id)?;
    if root.join(id).exists() {
        return verify(root, &p);
    }
    ensure!(
        fs2::available_space(root)?
            > p.bytes.saturating_mul(if p.archive { 8 } else { 2 }) + 128 * 1024 * 1024,
        "Insufficient space for model installation"
    );
    let part = root.join(format!(".{id}.download"));
    let mut offset = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    ensure!(
        offset <= p.bytes,
        "Partial download exceeds expected size; remove partial download"
    );
    if offset < p.bytes {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(60)));
        #[cfg(windows)]
        let config = config.tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        );
        let agent: ureq::Agent = config.build().into();
        let mut request = agent.get(&p.url).header("Accept-Encoding", "identity");
        if offset > 0 {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let mut response = request.call()?;
        if response.status() == 206 {
            let range = response
                .headers()
                .get("content-range")
                .and_then(|s| s.to_str().ok())
                .context("Missing partial response bounds")?;
            ensure!(
                range == format!("bytes {}-{}/{}", offset, p.bytes - 1, p.bytes),
                "Unexpected partial response"
            );
        } else {
            ensure!(response.status() == 200, "Download failed");
            offset = 0;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(&part)?;
        let mut reader = response.body_mut().as_reader();
        let mut buffer = [0u8; 65536];
        loop {
            ensure!(
                !cancel.load(Ordering::Relaxed),
                "Download paused; resume from model manager"
            );
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            ensure!(offset + n as u64 <= p.bytes, "Download exceeds pinned size");
            file.write_all(&buffer[..n])?;
            offset += n as u64;
            progress(
                offset as f32 / p.bytes as f32,
                "Model download / Modelldownload",
            );
        }
        file.sync_all()?;
    }
    ensure!(
        offset == p.bytes,
        "Download incomplete; resume from model manager"
    );
    if digest(&part)? != p.sha256 {
        // A rejected file must not be resumed indefinitely or installed.
        fs::rename(
            &part,
            root.join(format!(".{id}.rejected-{}", uuid::Uuid::new_v4())),
        )?;
        bail!("Download checksum mismatch; file quarantined, model not installed");
    }
    progress(1., "Verifying package / Paket prüfen");
    let staging = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(root)?;
    if p.archive {
        if p.url.ends_with(".zip") {
            extract_zip(&part, staging.path(), cancel)?;
        } else {
            extract(&part, staging.path(), cancel)?;
        }
    } else {
        fs::copy(&part, staging.path().join(&p.entry))?;
    }
    ensure!(
        staging.path().join(&p.entry).is_file(),
        "Package entry is missing"
    );
    #[cfg(unix)]
    if matches!(p.role.as_str(), "downloader" | "javascript_runtime") {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            staging.path().join(&p.entry),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    let mut files = BTreeMap::new();
    inventory(staging.path(), staging.path(), &mut files)?;
    atomic_json(
        &staging.path().join("installed.json"),
        &Manifest {
            version: 1,
            package: p.clone(),
            files,
        },
    )?;
    ensure!(!cancel.load(Ordering::Relaxed), "Installation paused");
    fs::rename(staging.path(), root.join(id))?;
    fs::remove_file(part)?;
    verify(root, &p)
}
fn inventory(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let e = entry?;
        let path = e.path();
        ensure!(!e.file_type()?.is_symlink(), "Unexpected package symlink");
        if e.file_type()?.is_dir() {
            inventory(base, &path, out)?;
        } else {
            out.insert(
                path.strip_prefix(base)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                digest(&path)?,
            );
        }
    }
    Ok(())
}
fn extract_zip(archive: &Path, out: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    ensure!(zip.len() <= 10000, "Too many ZIP entries");
    let mut size = 0_u64;
    for i in 0..zip.len() {
        ensure!(!cancel.load(Ordering::Relaxed), "Installation paused");
        let mut entry = zip.by_index(i)?;
        let path = entry.enclosed_name().context("Unsafe ZIP path")?;
        ensure!(
            relative(&path) && !entry.name().contains('\\') && !entry.is_symlink(),
            "Unsafe ZIP entry"
        );
        let dest = out.join(path);
        if entry.is_dir() {
            fs::create_dir_all(dest)?;
            continue;
        }
        size = size.saturating_add(entry.size());
        ensure!(
            size <= 2 * 1024 * 1024 * 1024,
            "Expanded package is too large"
        );
        fs::create_dir_all(dest.parent().unwrap())?;
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(dest)?;
        std::io::copy(&mut entry, &mut f)?;
        f.sync_all()?;
    }
    Ok(())
}
fn extract(archive: &Path, out: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(fs::File::open(archive)?));
    let mut links = vec![];
    let mut size = 0u64;
    for entry in tar.entries()? {
        ensure!(!cancel.load(Ordering::Relaxed), "Installation paused");
        let mut e = entry?;
        let p = e.path()?.into_owned();
        ensure!(relative(&p), "Unsafe archive path");
        let dest = out.join(&p);
        let kind = e.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if kind.is_symlink() || kind.is_hard_link() {
            let target = e.link_name()?.context("Missing link target")?.into_owned();
            // Materialize safe internal links as files; never create filesystem symlinks.
            let candidate = if kind.is_symlink() {
                p.parent().unwrap().join(target)
            } else {
                target
            };
            let mut normalized = PathBuf::new();
            for c in candidate.components() {
                match c {
                    Component::Normal(s) => normalized.push(s),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        ensure!(normalized.pop(), "Archive link escapes package");
                    }
                    _ => bail!("Absolute archive link"),
                }
            }
            ensure!(relative(&normalized), "Invalid archive link");
            links.push((p, normalized));
            continue;
        }
        ensure!(kind.is_file(), "Unsupported archive entry");
        size = size.saturating_add(e.size());
        ensure!(
            size <= 2 * 1024 * 1024 * 1024,
            "Expanded package is too large"
        );
        fs::create_dir_all(dest.parent().unwrap())?;
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&dest)?;
        std::io::copy(&mut e, &mut f)?;
        f.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &dest,
                fs::Permissions::from_mode(if e.header().mode()? & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }),
            )?;
        }
    }
    for _ in 0..8 {
        let mut remaining = vec![];
        for (p, target) in links {
            if out.join(&target).is_file() {
                fs::create_dir_all(out.join(&p).parent().unwrap())?;
                ensure!(!out.join(&p).exists(), "Duplicate archive entry");
                fs::copy(out.join(target), out.join(p))?;
            } else {
                remaining.push((p, target));
            }
        }
        links = remaining;
        if links.is_empty() {
            break;
        }
    }
    ensure!(links.is_empty(), "Unresolved archive links");
    Ok(())
}
/// Only installer-owned folders can be removed. External model files are never deleted.
pub fn remove(root: &Path, id: &str) -> Result<()> {
    let p = package(id)?;
    let _lock = lock(root, id)?;
    if root.join(id).exists() {
        manifest(root, &p)?;
        fs::remove_dir_all(root.join(id))?;
    }
    let part = root.join(format!(".{id}.download"));
    if part.exists() {
        fs::remove_file(part)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zip_program_install_rejects_traversal_and_checks_contents() {
        let temp = tempfile::tempdir().unwrap();
        for (name, valid) in [
            ("deno.exe", true),
            ("../escape.exe", false),
            ("folder\\escape.exe", false),
        ] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let mut zip = zip::ZipWriter::new(file.reopen().unwrap());
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"program").unwrap();
            zip.finish().unwrap();
            let out = temp.path().join(uuid::Uuid::new_v4().to_string());
            fs::create_dir(&out).unwrap();
            assert_eq!(extract_zip(file.path(), &out, &false.into()).is_ok(), valid);
            if valid {
                assert_eq!(fs::read(out.join(name)).unwrap(), b"program");
            }
        }
        assert!(!temp.path().join("escape.exe").exists());
    }
    fn archive(path: &Path, escape: bool) {
        let file = fs::File::create(path).unwrap();
        let encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "bundle/lib/file", &b"data"[..])
            .unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_cksum();
        tar.append_link(
            &mut link,
            "bundle/lib/alias",
            if escape { "../../../outside" } else { "file" },
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    #[test]
    fn archive_links_are_materialized_and_escaping_links_rejected() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("safe.bz2");
        archive(&p, false);
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        extract(&p, &out, &false.into()).unwrap();
        assert_eq!(fs::read(out.join("bundle/lib/alias")).unwrap(), b"data");
        assert!(
            !fs::symlink_metadata(out.join("bundle/lib/alias"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let p = t.path().join("bad.bz2");
        archive(&p, true);
        let out = t.path().join("bad");
        fs::create_dir(&out).unwrap();
        assert!(extract(&p, &out, &false.into()).is_err());
        assert!(!t.path().join("outside").exists());
    }
    #[test]
    fn cancelled_install_makes_no_request_and_bad_complete_download_is_quarantined() {
        let t = tempfile::tempdir().unwrap();
        assert!(install(t.path(), "ggml-base", &true.into(), |_, _| {}).is_err());
        assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
        let p = package("ggml-base").unwrap();
        let partial = t.path().join(".ggml-base.download");
        fs::File::create(&partial)
            .unwrap()
            .set_len(p.bytes)
            .unwrap();
        assert!(
            install(t.path(), &p.id, &false.into(), |_, _| {})
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );
        assert!(!partial.exists());
        assert!(!t.path().join(&p.id).exists());
        assert!(fs::read_dir(t.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ggml-base.rejected-")
        }));
    }
    #[test]
    fn modified_installed_files_fail_verification() {
        let t = tempfile::tempdir().unwrap();
        let p = package("ggml-base").unwrap();
        let root = t.path().join(&p.id);
        fs::create_dir(&root).unwrap();
        fs::write(root.join(&p.entry), "model").unwrap();
        let files = BTreeMap::from([(p.entry.clone(), digest(&root.join(&p.entry)).unwrap())]);
        atomic_json(
            &root.join("installed.json"),
            &Manifest {
                version: 1,
                package: p.clone(),
                files,
            },
        )
        .unwrap();
        assert!(verify(t.path(), &p).is_ok());
        fs::write(root.join(&p.entry), "changed").unwrap();
        assert!(verify(t.path(), &p).is_err());
    }
}
