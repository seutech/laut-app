use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use laut_core::Command;
use laut_engine::{
    Session,
    jobs::{JobConfig, transcribe},
    library::{Collection, Library},
    write_new,
};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Parser)]
#[command(
    version,
    about = "Laut: local transcript commands and optional whisper.cpp jobs"
)]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Local CPU semantic search, source-linked answer, or resumable complete analysis.
    Research {
        root: PathBuf,
        config: PathBuf,
        cache: PathBuf,
        #[arg(long)]
        recording: Option<String>,
        #[arg(long, default_value="search", value_parser=["search","answer","analyse"])]
        mode: String,
        #[arg(long)]
        query: String,
        #[arg(long)]
        no_pdfs: bool,
    },
    PdfImport {
        root: PathBuf,
        id: String,
        config: PathBuf,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    PdfPreview {
        recording: PathBuf,
        document_id: String,
        page: u32,
        config: PathBuf,
        cache: PathBuf,
    },
    PdfOcr {
        root: PathBuf,
        id: String,
        document_id: String,
        page: u32,
        config: PathBuf,
    },
    ConferenceExport {
        recording: PathBuf,
        config: PathBuf,
        out: PathBuf,
    },
    ConferenceSuggest {
        recording: PathBuf,
        #[arg(value_parser=["slides","chapters","ranges","names"])]
        kind: String,
    },
    /// Create an independent portable library; never adopts a nonempty unmarked folder.
    LibraryInit {
        root: PathBuf,
    },
    LibrarySearch {
        root: PathBuf,
        query: String,
    },
    LibraryBackup {
        root: PathBuf,
        out: PathBuf,
    },
    LibraryRestore {
        backup: PathBuf,
        out: PathBuf,
    },
    LibraryList {
        root: PathBuf,
    },
    /// List event/topic collections stored with the portable library.
    LibraryCollections {
        root: PathBuf,
    },
    /// Create or replace one named collection without moving or duplicating recordings.
    LibraryCollectionSet {
        root: PathBuf,
        name: String,
        #[arg(long = "recording", required = true)]
        recordings: Vec<String>,
    },
    /// Logical file sizes and recoverable trash. Does not follow symbolic links.
    LibraryStorage {
        root: PathBuf,
    },
    /// Move the entire project to the library's local trash without deleting files.
    LibraryTrash {
        root: PathBuf,
        id: String,
    },
    /// Restore a trash token without overwriting an existing project.
    LibraryUntrash {
        root: PathBuf,
        token: String,
    },
    LibraryImport {
        root: PathBuf,
        recording: PathBuf,
    },
    /// JSON-lines commands; successful document changes are saved with conflict detection.
    LibraryCommands {
        root: PathBuf,
        id: String,
    },
    Inspect {
        recording: PathBuf,
    },
    Export {
        recording: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long,default_value="txt",value_parser=["txt","md","srt","json"])]
        format: String,
    },
    Search {
        recording: PathBuf,
        query: String,
    },
    Copy {
        recording: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// JSON-lines commands on stdin. Changes stay in memory unless --out saves a new project.
    Commands {
        recording: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Models {
        root: PathBuf,
    },
    ModelInstall {
        root: PathBuf,
        id: String,
    },
    ModelVerify {
        root: PathBuf,
        id: String,
    },
    ModelRemove {
        root: PathBuf,
        id: String,
    },
    QueueAdd {
        root: PathBuf,
        library: PathBuf,
        config: PathBuf,
        #[arg(long)]
        diarization: Option<PathBuf>,
    },
    /// Queue a public media link; config contains the same local ASR paths as queue-add.
    QueueAddUrl {
        root: PathBuf,
        library: PathBuf,
        config: PathBuf,
        url: String,
        #[arg(long)]
        yt_dlp: PathBuf,
        #[arg(long)]
        deno: PathBuf,
        #[arg(long)]
        first_seconds: Option<f64>,
        #[arg(long)]
        diarization: Option<PathBuf>,
    },
    QueueList {
        root: PathBuf,
    },
    QueueRun {
        root: PathBuf,
        id: String,
    },
    QueueCleanup {
        root: PathBuf,
        id: String,
    },
    Diarize {
        recording: PathBuf,
        config: PathBuf,
        cache: PathBuf,
        out: PathBuf,
    },
    /// Explicit local paths; no network, automatic model downloads or API calls.
    Transcribe {
        #[arg(long)]
        config: PathBuf,
    },
}
fn main() -> Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;
    let progress = |fraction: f32, status: &str| eprintln!("{:.0}% {status}", fraction * 100.);
    match Args::parse().action {
        Action::Research {
            root,
            config,
            cache,
            recording,
            mode,
            query,
            no_pdfs,
        } => {
            use laut_engine::research::{self, Corpus, LocalInference, ResearchConfig};
            let config: ResearchConfig =
                serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            let corpus =
                Corpus::from_library(&Library::open(&root)?, recording.as_deref(), !no_pdfs)?;
            let engine = LocalInference::new(config.clone(), mode != "analyse", mode != "search")?;
            if mode == "search" {
                let result = research::search(
                    &engine,
                    &corpus,
                    &cache.join("vectors"),
                    &query,
                    &cancel,
                    progress,
                )?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let report = if mode == "analyse" {
                    research::analyse(
                        &engine,
                        &corpus,
                        &cache.join("steps"),
                        &query,
                        &config.language,
                        &cancel,
                        progress,
                    )?
                } else {
                    let hits = research::search(
                        &engine,
                        &corpus,
                        &cache.join("vectors"),
                        &query,
                        &cancel,
                        progress,
                    )?;
                    research::answer(&engine, &corpus, &hits, &query, &config.language, &cancel)?
                };
                let path = research::save_report(&cache.join("results"), &report)?;
                eprintln!("Saved: {}", path.display());
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        Action::PdfImport {
            root,
            id,
            config,
            files,
        } => {
            let library = Library::open(&root)?;
            let mut loaded = library.load(&id)?;
            let config = serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            laut_engine::documents::import_documents(
                &mut loaded.session,
                &library,
                &mut loaded.revision,
                &files,
                &config,
                &cancel,
                progress,
            )?;
            println!("{}", loaded.session.source.display());
        }
        Action::PdfPreview {
            recording,
            document_id,
            page,
            config,
            cache,
        } => {
            let config = serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            println!(
                "{}",
                laut_engine::documents::render_page(
                    &Session::open(&recording)?,
                    &document_id,
                    page,
                    &config,
                    &cache,
                    &cancel
                )?
                .display()
            );
        }
        Action::PdfOcr {
            root,
            id,
            document_id,
            page,
            config,
        } => {
            let library = Library::open(&root)?;
            let mut loaded = library.load(&id)?;
            let config = serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            let pdf = laut_engine::documents::page_path(&loaded.session, &document_id, page)?;
            let text = laut_engine::documents::ocr_page(&pdf, page, &config, &cancel)?;
            let mut c = laut_core::conference::context(&loaded.session.document);
            for d in c["documents"].as_array_mut().unwrap() {
                if d["id"] == document_id {
                    for p in d["pages"].as_array_mut().unwrap() {
                        if p["number"] == page {
                            p["ocrText"] = serde_json::json!(text);
                            p["extraction"] =
                                serde_json::json!("PDF text + Tesseract OCR (unreviewed)");
                        }
                    }
                }
            }
            loaded
                .session
                .execute(Command::EditConference { value: c })?;
            library.save(&mut loaded.session, &mut loaded.revision)?;
        }
        Action::ConferenceExport {
            recording,
            config,
            out,
        } => {
            let config = serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            laut_engine::conference_archive::export(
                &Session::open(&recording)?,
                &out,
                &config,
                &cancel,
                progress,
            )?;
            println!("{}", out.display());
        }
        Action::ConferenceSuggest { recording, kind } => {
            let r = Session::open(&recording)?.document.view();
            let result = match kind.as_str() {
                "slides" => laut_core::conference::slide_suggestions(&r),
                "chapters" => laut_core::conference::chapter_suggestions(&r),
                "names" => laut_core::conference::name_suggestions(&r),
                _ => laut_core::conference::range_suggestions(&r),
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Action::Models { root } => {
            for p in laut_engine::models::catalog() {
                println!(
                    "{}",
                    serde_json::json!({"package":p,"installed":laut_engine::models::installed(&root,&p)})
                );
            }
        }
        Action::ModelInstall { root, id } => println!(
            "{}",
            laut_engine::models::install(&root, &id, &cancel, progress)?.display()
        ),
        Action::ModelVerify { root, id } => println!(
            "{}",
            laut_engine::models::verify(&root, &laut_engine::models::package(&id)?)?.display()
        ),
        Action::ModelRemove { root, id } => laut_engine::models::remove(&root, &id)?,
        Action::QueueAdd {
            root,
            library,
            config,
            diarization,
        } => {
            let config = serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            let diarization = diarization
                .map(|p| -> Result<_> {
                    Ok(serde_json::from_slice(&laut_engine::read_json_bytes(&p)?)?)
                })
                .transpose()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&laut_engine::queue::Queue::new(root).enqueue(
                    config,
                    library,
                    diarization
                )?)?
            );
        }
        Action::QueueAddUrl {
            root,
            library,
            config,
            url,
            yt_dlp,
            deno,
            first_seconds,
            diarization,
        } => {
            let config: JobConfig =
                serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            let diarization = diarization
                .map(|p| -> Result<_> {
                    Ok(serde_json::from_slice(&laut_engine::read_json_bytes(&p)?)?)
                })
                .transpose()?;
            let item = laut_engine::queue::Queue::new(root).enqueue_url(
                config,
                library,
                diarization,
                laut_engine::downloads::UrlImport {
                    url,
                    yt_dlp,
                    deno,
                    first_seconds,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&item)?);
        }
        Action::QueueList { root } => println!(
            "{}",
            serde_json::to_string_pretty(&laut_engine::queue::Queue::new(root).list()?)?
        ),
        Action::QueueRun { root, id } => println!(
            "{}",
            laut_engine::queue::Queue::new(root)
                .run(&id, &cancel, progress)?
                .display()
        ),
        Action::QueueCleanup { root, id } => laut_engine::queue::Queue::new(root).cleanup(&id)?,
        Action::Diarize {
            recording,
            config,
            cache,
            out,
        } => {
            let session = Session::open(&recording)?;
            let config = serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            let analysis =
                laut_engine::diarization::analyze(&session, &config, &cache, &cancel, progress)?;
            write_new(&out, &serde_json::to_vec_pretty(&analysis)?)?;
            println!("{}", out.display());
        }
        Action::LibraryInit { root } => println!("{}", Library::create(&root)?.root.display()),
        Action::LibrarySearch { root, query } => println!(
            "{}",
            serde_json::to_string_pretty(&Library::open(&root)?.search(&query)?)?
        ),
        Action::LibraryBackup { root, out } => {
            Library::open(&root)?.backup(&out)?;
            println!("{}", out.display());
        }
        Action::LibraryRestore { backup, out } => {
            let lib = Library::restore_backup(&backup, &out)?;
            println!("{}", lib.root.display());
        }
        Action::LibraryList { root } => println!(
            "{}",
            serde_json::to_string_pretty(&Library::open(&root)?.catalog()?)?
        ),
        Action::LibraryCollections { root } => println!(
            "{}",
            serde_json::to_string_pretty(&Library::open(&root)?.collections()?)?
        ),
        Action::LibraryCollectionSet {
            root,
            name,
            recordings,
        } => {
            let library = Library::open(&root)?;
            let active: std::collections::BTreeSet<_> = library
                .catalog()?
                .entries
                .into_iter()
                .map(|entry| entry.id.to_uppercase())
                .collect();
            let recordings: Vec<_> = recordings.into_iter().map(|id| id.to_uppercase()).collect();
            ensure!(
                recordings.iter().all(|id| active.contains(id)),
                "Every collection recording must exist in the active library"
            );
            let mut collections = library.collections()?;
            let trimmed = name.trim();
            let collection = if let Some(existing) = collections
                .iter_mut()
                .find(|collection| collection.name.eq_ignore_ascii_case(trimmed))
            {
                existing.name = trimmed.into();
                existing.recording_ids = recordings;
                existing.clone()
            } else {
                let mut collection = Collection::new(trimmed);
                collection.recording_ids = recordings;
                collections.push(collection.clone());
                collection
            };
            library.save_collections(&collections)?;
            println!("{}", serde_json::to_string_pretty(&collection)?);
        }
        Action::LibraryImport { root, recording } => {
            let loaded = Library::open(&root)?.import(&Session::open(&recording)?)?;
            println!("{}", loaded.session.source.display());
        }
        Action::LibraryStorage { root } => println!(
            "{}",
            serde_json::to_string_pretty(&Library::open(&root)?.storage()?)?
        ),
        Action::LibraryTrash { root, id } => {
            let library = Library::open(&root)?;
            let loaded = library.load(&id)?;
            println!("{}", library.move_to_trash(&id, &loaded.revision)?);
        }
        Action::LibraryUntrash { root, token } => println!(
            "{}",
            Library::open(&root)?
                .restore_from_trash(&token)?
                .session
                .source
                .display()
        ),
        Action::LibraryCommands { root, id } => {
            let library = Library::open(&root)?;
            let mut loaded = library.load(&id)?;
            if loaded.recovered {
                eprintln!("Loaded previous valid recording; damaged original is retained");
            }
            for line in io::stdin().lock().lines() {
                let line = line?;
                anyhow::ensure!(line.len() <= 4 * 1024 * 1024, "Command exceeds 4 MiB");
                let command: Command = serde_json::from_str(&line)?;
                let result = loaded.session.execute(command)?;
                if loaded.session.dirty {
                    library.save(&mut loaded.session, &mut loaded.revision)?;
                }
                println!("{}", serde_json::json!({"ok":true,"result":result}));
                io::stdout().flush()?;
            }
        }
        Action::Inspect { recording } => {
            println!("{}", Session::open(&recording)?.execute(Command::Inspect)?)
        }
        Action::Search { recording, query } => println!(
            "{}",
            Session::open(&recording)?.execute(Command::Search { query })?
        ),
        Action::Export {
            recording,
            out,
            format,
        } => {
            let mut s = Session::open(&recording)?;
            let text = match format.as_str() {
                "srt" => laut_core::srt(&s.document.view()),
                "json" => serde_json::to_string_pretty(s.document.raw())?,
                "md" => s
                    .execute(Command::ExportMarkdown)?
                    .as_str()
                    .context("Export failed")?
                    .to_owned(),
                _ => s
                    .execute(Command::ExportText)?
                    .as_str()
                    .context("Export failed")?
                    .to_owned(),
            };
            write_new(&out, text.as_bytes())?;
        }
        Action::Copy { recording, out } => Session::open(&recording)?.save_copy(&out)?,
        Action::Commands { recording, out } => {
            let mut s = Session::open(&recording)?;
            for line in io::stdin().lock().lines() {
                let line = line?;
                if line.len() > 4 * 1024 * 1024 {
                    anyhow::bail!("Command exceeds 4 MiB");
                }
                let result = serde_json::from_str::<Command>(&line)
                    .map_err(anyhow::Error::from)
                    .and_then(|c| s.execute(c));
                match result {
                    Ok(value) => println!("{}", serde_json::json!({"ok":true,"result":value})),
                    Err(error) => println!(
                        "{}",
                        serde_json::json!({"ok":false,"error":error.to_string()})
                    ),
                }
                io::stdout().flush()?;
            }
            if let Some(out) = out {
                s.save_copy(&out)?;
            }
        }
        Action::Transcribe { config } => {
            let config: JobConfig =
                serde_json::from_slice(&laut_engine::read_json_bytes(&config)?)?;
            let output = transcribe(&config, &cancel, |fraction, status| {
                eprintln!("{:.0}% {status}", fraction * 100.)
            })?;
            println!("{}", output.display());
        }
    }
    Ok(())
}
