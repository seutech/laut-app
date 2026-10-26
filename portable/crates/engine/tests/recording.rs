use laut_engine::{
    Session,
    library::Library,
    recording::{EncoderControl, Track, Writer, alignment, list, publish},
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
#[test]
fn real_gaps_are_retained_and_repeated_buffers_are_not_duplicated() {
    assert_eq!(alignment(48000, 48004, 48000, 480), (0, 0));
    assert_eq!(alignment(48000, 96000, 48000, 480), (48000, 0));
    assert_eq!(alignment(48000, 0, 48000, 480), (0, 480));
    assert_eq!(alignment(48000, 46000, 48000, 4800), (0, 2000));
    assert_eq!(alignment(0, 24000, 48000, 480), (24000, 0));
}
#[test]
fn capture_rejects_escaping_or_duplicate_track_paths_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::create(&temp.path().join("Library")).unwrap();
    let mut track = Track::new("microphone", "Test".into(), 48000);
    track.filename = "../outside.flac".into();
    assert!(
        Writer::create(
            &temp.path().join("captures"),
            &library.root,
            "Title".into(),
            false,
            vec![track],
            Path::new("ffmpeg"),
            EncoderControl::default()
        )
        .is_err()
    );
    assert!(!temp.path().join("captures").exists());
    assert!(
        Writer::create(
            &temp.path().join("captures"),
            &library.root,
            "Title".into(),
            true,
            vec![
                Track::new("microphone", "Test".into(), 48000),
                Track::new("microphone", "Test".into(), 48000)
            ],
            Path::new("ffmpeg"),
            EncoderControl::default()
        )
        .is_err()
    );
}
fn tools() -> (PathBuf, PathBuf) {
    (
        std::env::var_os("LAUT_TEST_FFMPEG")
            .expect("Set LAUT_TEST_FFMPEG")
            .into(),
        std::env::var_os("LAUT_TEST_FFPROBE")
            .expect("Set LAUT_TEST_FFPROBE")
            .into(),
    )
}
fn sine(start: usize, count: usize, hz: f32, rate: u32) -> Vec<f32> {
    (start..start + count)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 0.2)
        .collect()
}
#[test]
#[ignore = "Requires local FFmpeg and FFprobe; does not access audio hardware"]
fn actual_codecs_save_mix_copy_and_recover_interrupted_capture() {
    let (ffmpeg, ffprobe) = tools();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Captures ü");
    let library = Library::create(&temp.path().join("Library")).unwrap();
    let mut w = Writer::create(
        &root,
        &library.root,
        "Meeting test".into(),
        true,
        vec![
            Track::new("microphone", "Test mic".into(), 48000),
            Track::new("system", "Test output".into(), 44100),
        ],
        &ffmpeg,
        EncoderControl::default(),
    )
    .unwrap();
    let folder = w.folder.clone();
    assert!(
        publish(&folder, &ffmpeg, &ffprobe, true, &false.into())
            .unwrap_err()
            .to_string()
            .contains("active")
    );
    for n in 0..30 {
        w.write(0, n as f64 * 0.1, &sine(n * 4800, 4800, 440., 48000))
            .unwrap();
        if n >= 10 {
            w.write(1, n as f64 * 0.1, &sine(n * 4410, 4410, 880., 44100))
                .unwrap();
        }
    }
    w.finish(3., None).unwrap();
    let target = publish(&folder, &ffmpeg, &ffprobe, false, &false.into()).unwrap();
    let session = Session::open(&target).unwrap();
    assert!((session.document.view().duration - 3.).abs() < 0.1);
    assert_eq!(session.document.view().media_names().len(), 3);
    assert!(!folder.join("microphone.flac").exists());
    assert!(list(&root).unwrap().is_empty());
    assert_eq!(
        publish(&folder, &ffmpeg, &ffprobe, false, &false.into()).unwrap(),
        target
    );
    let copy = temp.path().join("Copy");
    session.save_copy(&copy).unwrap();
    assert!(copy.join("system.flac").exists());
    let audio = session.audio_path().unwrap();
    let output = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(audio)
        .args(["-f", "f32le", "-ac", "1", "-ar", "24000", "pipe:1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let samples: Vec<f32> = output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let amplitude = |from: usize, hz: f32| -> f32 {
        let data = &samples[from..from + 6000];
        let (sin, cos) = data.iter().enumerate().fold((0., 0.), |(a, b), (i, x)| {
            let phase = i as f32 * hz * std::f32::consts::TAU / 24000.;
            (a + x * phase.sin(), b + x * phase.cos())
        });
        2. * (sin * sin + cos * cos).sqrt() / data.len() as f32
    };
    assert!(amplitude(6000, 440.) > 0.1);
    assert!(
        amplitude(6000, 880.) < 0.02,
        "system track must start one second later"
    );
    assert!(amplitude(36000, 880.) > 0.1);
    assert!(
        session.document.raw()["portableCapture"]["tracks"][0]["peak"]
            .as_f64()
            .unwrap()
            > 0.1
    );
    let mut silent = Writer::create(
        &root,
        &library.root,
        "Silent input".into(),
        false,
        vec![Track::new("microphone", "Muted mic".into(), 48000)],
        &ffmpeg,
        EncoderControl::default(),
    )
    .unwrap();
    let silent_folder = silent.folder.clone();
    silent.write(0, 0., &vec![0.; 48000]).unwrap();
    silent.finish(1., None).unwrap();
    let silent =
        Session::open(&publish(&silent_folder, &ffmpeg, &ffprobe, false, &false.into()).unwrap())
            .unwrap();
    assert_eq!(
        silent.document.raw()["portableCapture"]["tracks"][0]["peak"],
        0.
    );
    assert!(silent.audio_path().unwrap().is_file());
    assert!(
        silent.document.raw()["portableCapture"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("only silence"))
    );
    let mut partial = Writer::create(
        &root,
        &library.root,
        "Recovered mic".into(),
        true,
        vec![
            Track::new("microphone", "Test".into(), 48000),
            Track::new("system", "Missing".into(), 48000),
        ],
        &ffmpeg,
        EncoderControl::default(),
    )
    .unwrap();
    let folder = partial.folder.clone();
    for n in 0..20 {
        partial
            .write(0, n as f64 * 0.1, &sine(n * 4800, 4800, 440., 48000))
            .unwrap();
    }
    assert!(
        partial
            .finish(2., Some("Device disconnected".into()))
            .is_err()
    );
    assert!(publish(&folder, &ffmpeg, &ffprobe, false, &false.into()).is_err());
    let recovered = publish(&folder, &ffmpeg, &ffprobe, true, &AtomicBool::new(false)).unwrap();
    let recovered = Session::open(&recovered).unwrap();
    assert_eq!(
        recovered.document.raw()["portableCapture"]["recovered"],
        true
    );
    assert_eq!(recovered.document.view().media_names().len(), 1);
    assert!(
        folder.join("microphone.flac").is_file(),
        "recovery retains original partial files"
    );
    assert!(
        !recovered.document.raw()["portableCapture"]["warnings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
