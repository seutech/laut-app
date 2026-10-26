//! Independent research package with original PDFs, page-separated search text and reviewed slides.
use crate::{
    Session,
    documents::{PdfConfig, render_page},
    media, write_new,
};
use anyhow::{Result, ensure};
use serde_json::json;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn url(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub fn export(
    session: &Session,
    destination: &Path,
    config: &PdfConfig,
    cancel: &AtomicBool,
    progress: impl Fn(f32, &str),
) -> Result<()> {
    ensure!(!destination.exists(), "Choose a new export folder");
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent)?;
    let root = stage.path().join("archive");
    ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
    session.save_copy(&root)?;
    let copy = Session::open(&root)?;
    let r = copy.document.view();
    let derived = root.join("derived/documents");
    fs::create_dir_all(&derived)?;
    let mut manifest = vec![];
    let mut md = laut_core::markdown(&r);
    let mut body = format!(
        "<h1>{}</h1><p>Local research copy. PDF text and OCR are search aids; check charts, tables and footnotes in the originals.</p>",
        html(&r.title)
    );
    if let Some(audio) = &r.audio_filename {
        body += &format!(
            "<audio controls preload=\"none\" src=\"{}\"></audio>",
            url(audio)
        );
    }
    if let Some(c) = &r.conference {
        md += "\n## Original PDFs\n\n";
        body += "<h2>Original PDFs</h2><ul>";
        for doc in &c.documents {
            ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
            let folder = derived.join(&doc.id);
            ensure!(
                uuid::Uuid::parse_str(&doc.id).is_ok(),
                "Invalid document ID"
            );
            fs::create_dir(&folder)?;
            let mut text = format!(
                "{}\nOriginal: {}\nText is a search aid, not a replacement for the PDF.\n",
                doc.title, doc.filename
            );
            for p in &doc.pages {
                text += &format!(
                    "\n--- Page {} · {} ---\n{}\n",
                    p.number,
                    p.extraction.as_deref().unwrap_or("unknown extraction"),
                    p.text
                );
                if let Some(ocr) = &p.ocr_text {
                    text += &format!("\nOCR (unreviewed):\n{ocr}\n");
                }
            }
            write_new(&folder.join("text.txt"), text.as_bytes())?;
            let raw = copy.document.raw()["conference"]["documents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["id"] == doc.id)
                .unwrap();
            write_new(&folder.join("pages.json"), &serde_json::to_vec_pretty(raw)?)?;
            let link = url(&doc.filename);
            md += &format!("- [{}]({})\n", doc.title.replace(['[', ']'], ""), link);
            body += &format!(
                "<li><a href=\"{}\">{}</a> · <a href=\"derived/documents/{}/text.txt\">Search text</a></li>",
                link,
                html(&doc.title),
                doc.id
            );
            manifest.push(json!({"id":doc.id,"title":doc.title,"original":doc.filename,"sha256":media::digest(&root.join(&doc.filename))?,"text":format!("derived/documents/{}/text.txt",doc.id)}));
        }
        body += "</ul>";
        let slides: Vec<_> = c
            .slides
            .iter()
            .filter(|s| s.confirmed && s.useful == Some(true))
            .collect();
        let images = root.join("slides");
        fs::create_dir(&images)?;
        let cache = stage.path().join("page-cache");
        for (i, s) in slides.iter().enumerate() {
            progress(
                i as f32 / slides.len().max(1) as f32,
                "Exporting reviewed slides",
            );
            let doc = c
                .documents
                .iter()
                .find(|d| d.id == s.document_id)
                .ok_or_else(|| anyhow::anyhow!("Missing PDF reference"))?;
            let rendered = render_page(&copy, &s.document_id, s.page, config, &cache, cancel)?;
            let name = format!("slides/{}-{}.png", doc.id, s.page);
            if !root.join(&name).exists() {
                fs::copy(rendered, root.join(&name))?;
            }
            md += &format!(
                "\n### {} · {}, page {}\n\n[Original PDF]({}#page={})\n\n![Reviewed slide]({})\n",
                laut_core::timestamp(r.source_time(s.start)),
                doc.title,
                s.page,
                url(&doc.filename),
                s.page,
                name
            );
        }
    }
    body += &transcript_html(&r);
    body += "<h2>PDF search text</h2>";
    for doc in r.conference.iter().flat_map(|c| &c.documents) {
        for page in &doc.pages {
            body += &format!(
                "<details><summary><a href=\"{}#page={}\">{} · page {}</a></summary><pre>{}</pre></details>",
                url(&doc.filename),
                page.number,
                html(&doc.title),
                page.number,
                html(&laut_core::conference::page_text(page))
            );
        }
    }
    let html = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>{}</title><style>body{{max-width:1000px;margin:3rem auto;padding:0 1.5rem;font:18px/1.65 system-ui;color:#222}}img{{max-width:100%;height:auto}}figure{{padding:1rem;background:#f1f5f5}}pre{{white-space:pre-wrap;font:inherit}}small,figcaption{{color:#506363}}audio{{width:100%}}</style>{body}</html>",
        html(&r.title)
    );
    write_new(&root.join("index.html"), html.as_bytes())?;
    write_new(&root.join("transcript.md"), md.as_bytes())?;
    write_new(
        &root.join("transcript.txt"),
        laut_core::plain_text(&r).as_bytes(),
    )?;
    write_new(&root.join("transcript.srt"), laut_core::srt(&r).as_bytes())?;
    let originals = copy
        .document
        .raw()
        .get("originalSegments")
        .filter(|v| v.is_array())
        .unwrap_or(&copy.document.raw()["segments"]);
    write_new(
        &root.join("transcript-original.json"),
        &serde_json::to_vec_pretty(originals)?,
    )?;
    write_new(
        &derived.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    write_new(&root.join("README.txt"),b"Open index.html for the local research copy. Original PDFs and audio are retained. PDF text and OCR cannot replace charts, layout or table relationships. Only confirmed useful slides are exported as images; thematic suggestions are not proof a slide was shown. recording.json retains corrections and undo history. Transcript times in Markdown/HTML include source offsets; SRT uses local audio time. No cloud inference or external resources.\n")?;
    ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
    fs::create_dir(destination)?;
    fs::write(
        destination.join("COPY-INCOMPLETE"),
        b"Export interrupted; source retained",
    )?;
    for e in fs::read_dir(&root)? {
        let e = e?;
        fs::rename(e.path(), destination.join(e.file_name()))?;
    }
    fs::remove_file(destination.join("COPY-INCOMPLETE"))?;
    progress(1., "Research export ready");
    Ok(())
}

fn slide_figure(r: &laut_core::Recording, s: &laut_core::Slide) -> String {
    format!(
        "<figure><a href=\"{}#page={}\"><img loading=\"lazy\" src=\"slides/{}-{}.png\" alt=\"Reviewed PDF page {}\"></a><figcaption>Reviewed reference · page {} · {}</figcaption></figure>",
        url(&r
            .conference
            .as_ref()
            .unwrap()
            .documents
            .iter()
            .find(|d| d.id == s.document_id)
            .unwrap()
            .filename),
        s.page,
        s.document_id,
        s.page,
        s.page,
        s.page,
        laut_core::timestamp(r.source_time(s.start))
    )
}

fn transcript_html(r: &laut_core::Recording) -> String {
    let mut body = String::from("<h2>Transcript</h2>");
    let mut slides: Vec<_> = r
        .conference
        .iter()
        .flat_map(|c| &c.slides)
        .filter(|s| s.confirmed && s.useful == Some(true))
        .collect();
    slides.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut next = 0;
    for p in laut_core::paragraphs(r) {
        while next < slides.len() && slides[next].start <= p.start() {
            body += &slide_figure(r, slides[next]);
            next += 1;
        }
        body += &format!(
            "<section><small>{} · {}</small><p>{}</p></section>",
            laut_core::timestamp(r.source_time(p.start())),
            html(r.speaker_name(p.speaker_id())),
            html(&p.text)
        );
    }
    for slide in &slides[next..] {
        body += &slide_figure(r, slide);
    }
    body
}

#[cfg(test)]
mod tests {
    #[test]
    fn reviewed_reference_after_last_paragraph_start_is_not_dropped() {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/conversation.json")).unwrap();
        let late_start = raw["segments"].as_array().unwrap().last().unwrap()["end"]
            .as_f64()
            .unwrap()
            - 0.01;
        raw["conference"] = serde_json::json!({"documents":[{"id":"doc","filename":"documents/page.pdf","title":"Evidence","pages":[]}],"slides":[{"documentID":"doc","page":1,"start":late_start,"confirmed":true,"useful":true}]});
        let r: laut_core::Recording = serde_json::from_value(raw).unwrap();
        let output = super::transcript_html(&r);
        assert!(output.contains("slides/doc-1.png"));
        assert_eq!(output.matches("<figure>").count(), 1);
        assert!(output.rfind("<figure>").unwrap() > output.rfind("</section>").unwrap());
    }
}
