//! Local Poppler/Tesseract workers. PDFs stay byte-identical; text/OCR are derived evidence.
use crate::{
    Session,
    jobs::{run, run_capture},
    library::{Library, Revision},
    media,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PdfConfig {
    pub pdfinfo: PathBuf,
    pub pdftotext: PathBuf,
    pub pdftoppm: PathBuf,
    pub tesseract: PathBuf,
    pub ocr_language: String,
    pub auto_ocr: bool,
    pub traineddata: std::collections::BTreeMap<String, PathBuf>,
}
impl Default for PdfConfig {
    fn default() -> Self {
        Self {
            pdfinfo: "pdfinfo".into(),
            pdftotext: "pdftotext".into(),
            pdftoppm: "pdftoppm".into(),
            tesseract: "tesseract".into(),
            ocr_language: "eng".into(),
            auto_ocr: true,
            traineddata: Default::default(),
        }
    }
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Cancelled; originals retained"
    );
    Ok(())
}
fn read_text(path: &Path) -> Result<String> {
    ensure!(
        fs::metadata(path)?.len() <= 2_000_000,
        "PDF page text exceeds 2 MB"
    );
    Ok(fs::read_to_string(path)?.replace('\u{c}', ""))
}
fn safe_directory(path: &Path) -> Result<()> {
    if path.exists() {
        ensure!(
            !fs::symlink_metadata(path)?.file_type().is_symlink() && path.is_dir(),
            "Invalid derived folder"
        );
    } else {
        fs::create_dir(path)?;
    }
    Ok(())
}
pub fn extract(
    pdf: &Path,
    config: &PdfConfig,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<Value> {
    cancelled(cancel)?;
    ensure!(
        fs::metadata(pdf)?.len() <= 512 * 1024 * 1024,
        "PDF exceeds 512 MiB"
    );
    let mut header = [0u8; 5];
    fs::File::open(pdf)?.read_exact(&mut header)?;
    ensure!(&header == b"%PDF-", "Not a PDF file");
    let before = media::digest(pdf)?;
    let work = tempfile::tempdir()?;
    let info = work.path().join("info.txt");
    let log = work.path().join("worker.log");
    let mut cmd = Command::new(&config.pdfinfo);
    cmd.env("LC_ALL", "C").arg(pdf);
    run_capture(cmd, &log, Some(&info), cancel, |_| {})?;
    let info = fs::read_to_string(info)?;
    let count = info
        .lines()
        .find_map(|s| s.strip_prefix("Pages:")?.trim().parse::<u32>().ok())
        .context("PDF page count unavailable")?;
    ensure!((1..=2000).contains(&count), "PDF must contain 1–2000 pages");
    let mut pages = vec![];
    for number in 1..=count {
        cancelled(cancel)?;
        progress(
            (number - 1) as f32 / count as f32,
            &format!("PDF {number}/{count}"),
        );
        let output = work.path().join("text.txt");
        let mut cmd = Command::new(&config.pdftotext);
        cmd.args([
            "-f",
            &number.to_string(),
            "-l",
            &number.to_string(),
            "-layout",
            "-enc",
            "UTF-8",
            "-eol",
            "unix",
        ])
        .arg(pdf)
        .arg(&output);
        run(cmd, &log, cancel)?;
        let embedded = read_text(&output)?;
        let mut page = json!({"number":number,"text":embedded,"extraction":"Poppler PDF text","portableReview":"unreviewed"});
        if config.auto_ocr && embedded.chars().filter(|c| !c.is_whitespace()).count() < 40 {
            match ocr_page(pdf, number, config, cancel) {
                Ok(text) => {
                    page["ocrText"] = json!(text);
                    page["extraction"] = json!("PDF text + Tesseract OCR (unreviewed)");
                }
                Err(e) => {
                    cancelled(cancel)?;
                    page["portableWarning"] =
                        json!(format!("OCR unavailable; original page retained: {e:#}"));
                }
            }
        }
        pages.push(page);
    }
    ensure!(
        before == media::digest(pdf)?,
        "PDF changed during extraction"
    );
    progress(1., "PDF text ready");
    let id = laut_core::conference::id();
    Ok(
        json!({"id":id,"filename":format!("documents/{id}.pdf"),"title":pdf.file_name().unwrap_or_default().to_string_lossy(),"pages":pages,"nameCandidates":[],"portableSource":{"sha256":before,"originalPath":pdf.to_string_lossy(),"method":"Poppler/Tesseract local CPU","ocrLanguage":config.ocr_language}}),
    )
}
fn render_into(
    pdf: &Path,
    number: u32,
    size: u32,
    config: &PdfConfig,
    folder: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    ensure!((1..=2000).contains(&number), "Invalid page number");
    let base = folder.join("page");
    let mut cmd = Command::new(&config.pdftoppm);
    cmd.args([
        "-f",
        &number.to_string(),
        "-l",
        &number.to_string(),
        "-singlefile",
        "-scale-to",
        &size.to_string(),
        "-png",
    ])
    .arg(pdf)
    .arg(&base);
    run(cmd, &folder.join("render.log"), cancel)?;
    let output = base.with_extension("png");
    ensure!(
        fs::metadata(&output)?.len() <= 32 * 1024 * 1024,
        "Page image exceeds limit"
    );
    Ok(output)
}
pub fn ocr_page(
    pdf: &Path,
    number: u32,
    config: &PdfConfig,
    cancel: &AtomicBool,
) -> Result<String> {
    ensure!(
        !config.ocr_language.is_empty()
            && config.ocr_language.len() <= 80
            && config
                .ocr_language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'+'),
        "Invalid OCR language"
    );
    let work = tempfile::tempdir()?;
    let png = render_into(pdf, number, 2200, config, work.path(), cancel)?;
    let data = work.path().join("tessdata");
    let managed = config
        .ocr_language
        .split('+')
        .all(|lang| config.traineddata.get(lang).is_some_and(|p| p.is_file()));
    if managed {
        fs::create_dir(&data)?;
        for lang in config.ocr_language.split('+') {
            fs::copy(
                &config.traineddata[lang],
                data.join(format!("{lang}.traineddata")),
            )?;
        }
    }
    let out = work.path().join("ocr");
    let mut cmd = Command::new(&config.tesseract);
    if managed {
        cmd.arg("--tessdata-dir").arg(&data);
    }
    cmd.env("OMP_THREAD_LIMIT", "4").arg(png).arg(&out).args([
        "-l",
        &config.ocr_language,
        "--psm",
        "3",
    ]);
    run(cmd, &work.path().join("ocr.log"), cancel)?;
    read_text(&out.with_extension("txt"))
}
pub fn page_path(session: &Session, document_id: &str, number: u32) -> Result<PathBuf> {
    let r = session.document.view();
    let c = r.conference.context("No conference documents")?;
    let doc = c
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .context("Document not found")?;
    ensure!(
        doc.pages.iter().any(|p| p.number == number),
        "Page not found"
    );
    media::resolve(session.source.parent().unwrap(), &doc.filename)
}
pub fn render_page(
    session: &Session,
    document_id: &str,
    number: u32,
    config: &PdfConfig,
    cache: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    cancelled(cancel)?;
    let pdf = page_path(session, document_id, number)?;
    fs::create_dir_all(cache)?;
    ensure!(
        !fs::symlink_metadata(cache)?.file_type().is_symlink(),
        "Invalid PDF cache"
    );
    let hash = media::digest(&pdf)?;
    let path = cache.join(format!("{hash}-{number}-1600.png"));
    if path.exists() {
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Invalid cached page"
        );
        return Ok(path);
    }
    let work = tempfile::tempdir_in(cache)?;
    let png = render_into(&pdf, number, 1600, config, work.path(), cancel)?;
    ensure!(hash == media::digest(&pdf)?, "PDF changed during rendering");
    let mut temp = tempfile::NamedTempFile::new_in(cache)?;
    std::io::copy(&mut fs::File::open(png)?, &mut temp)?;
    temp.flush()?;
    match temp.persist_noclobber(&path) {
        Ok(_) => {}
        Err(e) if path.is_file() => {
            drop(e);
        }
        Err(e) => return Err(e.error.into()),
    };
    Ok(path)
}
/// Copies new PDFs before publishing metadata. A failed save removes only this attempt's copies.
pub fn import_documents(
    session: &mut Session,
    library: &Library,
    revision: &mut Revision,
    files: &[PathBuf],
    config: &PdfConfig,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<()> {
    ensure!(!files.is_empty() && files.len() <= 100, "Choose 1–100 PDFs");
    let owner = Library::owner(&session.source)?.context("Import PDFs into a library project")?;
    ensure!(
        owner.0.root == library.root && owner.1.eq_ignore_ascii_case(&session.document.view().id),
        "Session belongs to another library"
    );
    let folder = session
        .source
        .parent()
        .context("Missing project folder")?
        .to_owned();
    let docs_folder = folder.join("documents");
    safe_directory(&docs_folder)?;
    let mut c = laut_core::conference::context(&session.document);
    let mut created = vec![];
    let original = session.clone();
    let result = (|| -> Result<()> {
        for (i, file) in files.iter().enumerate() {
            let file = file.canonicalize()?;
            let hash = media::digest(&file)?;
            if c["documents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["portableSource"]["sha256"] == hash)
            {
                continue;
            }
            let d = extract(&file, config, cancel, |p, s| {
                progress((i as f32 + p) / files.len() as f32, s)
            })?;
            let dest = folder.join(d["filename"].as_str().unwrap());
            let mut tmp = tempfile::NamedTempFile::new_in(&docs_folder)?;
            std::io::copy(&mut fs::File::open(&file)?, &mut tmp)?;
            tmp.flush()?;
            tmp.as_file().sync_all()?;
            ensure!(
                media::digest(tmp.path())? == hash && media::digest(&file)? == hash,
                "PDF changed during import"
            );
            tmp.persist_noclobber(&dest).map_err(|e| e.error)?;
            created.push(dest);
            c["documents"].as_array_mut().unwrap().push(d);
        }
        cancelled(cancel)?;
        session.execute(laut_core::Command::EditConference { value: c })?;
        library.save(session, revision)?;
        Ok(())
    })();
    if result.is_err() {
        *session = original;
        for file in created {
            let _ = fs::remove_file(file);
        }
    }
    result
}
