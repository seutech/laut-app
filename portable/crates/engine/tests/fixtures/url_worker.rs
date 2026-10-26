// Cross-platform subprocess fixture, compiled by the integration test. No network or ASR model.
use std::{env,fs,path::PathBuf,process::Command,time::Duration};
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    let has = |arg: &str| args.iter().any(|a| a == arg);
    let value = |arg: &str| args[args.iter().position(|a| a == arg).unwrap()+1].clone();
    if has("--child") {
        std::thread::sleep(Duration::from_secs(2));
        fs::write(value("--child"),"child survived").unwrap();
    } else if has("--skip-download") {
        let url = args.last().unwrap();
        if url.ends_with("private0000") { eprintln!("ERROR: Private video"); std::process::exit(1); }
        let id = url.split("v=").nth(1).unwrap_or_else(|| url.rsplit('/').next().unwrap()).split('?').next().unwrap();
        let duration = if url.contains("podcast.example") { "" } else { "\"duration\":30," };
        let extractor = if url.contains("vimeo.com") { "Vimeo" } else if url.contains("podcast.example") { "Generic" } else { "Youtube" };
        println!("{{\"id\":\"{id}\",\"title\":\"Talk: Merz / Huang?\",{duration}\"vcodec\":\"none\",\"acodec\":\"aac\",\"upload_date\":\"20261022\",\"extractor_key\":\"{extractor}\",\"chapters\":[{{\"start_time\":0,\"title\":\"Opening\"}}]}}");
    } else if has("--print-to-file") {
        let out = PathBuf::from(value("--output").replace("%(ext)s","m4a"));
        if args.last().unwrap().ends_with("cancel00000") {
            fs::write(out.with_extension("part"), "partial audio").unwrap();
            let _child = Command::new(env::current_exe().unwrap()).arg("--child").arg(out.with_extension("orphan")).spawn().unwrap();
            std::thread::sleep(Duration::from_secs(10));
        }
        fs::write(&out,"audio fixture").unwrap();
        let i = args.iter().position(|a| a=="--print-to-file").unwrap();
        fs::write(&args[i+2],out.to_str().unwrap()).unwrap();
    } else if has("-show_streams") || has("-show_entries") {
        println!("{{\"streams\":[{{\"codec_type\":\"audio\",\"codec_name\":\"aac\"}}],\"format\":{{\"duration\":\"30\"}}}}");
    } else if has("-ojf") {
        fs::write(PathBuf::from(value("-of")).with_extension("json"),r#"{"transcription":[{"text":"Hello from the conference.","offsets":{"from":0,"to":2000}}]}"#).unwrap();
    } else if has("-i") { fs::write(args.last().unwrap(),"decoded audio").unwrap(); }
}
