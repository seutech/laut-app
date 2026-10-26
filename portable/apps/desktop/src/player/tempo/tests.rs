use super::*;
use std::{
    fs,
    io::{Cursor, Write},
};

#[test]
fn underruns_freeze_original_time_and_never_swap_stereo_channels() {
    let (tx, rx) = mpsc::sync_channel(2);
    let progress = Arc::new(Progress::default());
    let mut source = TempoSource {
        rx,
        block: vec![],
        offset: 0,
        silent_right: false,
        progress: progress.clone(),
    };
    assert_eq!(source.next(), Some(0.));
    tx.send(vec![0.25, 0.75, -0.25, -0.75]).unwrap();
    assert_eq!(source.next(), Some(0.)); // finish the silent frame first
    assert_eq!(progress.position(3600., 2.), 3600.);
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(0.75));
    assert!((progress.position(3600., 2.) - (3600. + 2. / 48000.)).abs() < 1e-9);
    assert_eq!(source.next(), Some(-0.25));
    assert_eq!(source.next(), Some(-0.75));
    let at = progress.position(3600., 2.);
    for _ in 0..2000 {
        assert_eq!(source.next(), Some(0.));
    }
    assert_eq!(progress.position(3600., 2.), at);
    drop(tx);
    assert_eq!(source.next(), None);
}

#[test]
fn bounded_pcm_producer_cancels_even_while_the_consumer_is_paused() {
    struct QueueFill {
        input: Cursor<Vec<u8>>,
        ready: Option<mpsc::Sender<()>>,
    }
    impl Read for QueueFill {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.input.position() as usize >= QUEUE_BLOCKS * BLOCK_SAMPLES * 4
                && let Some(ready) = self.ready.take()
            {
                ready.send(()).unwrap();
            }
            self.input.read(out)
        }
    }
    let (tx, rx) = mpsc::sync_channel(QUEUE_BLOCKS);
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = read_pcm(
            QueueFill {
                input: Cursor::new(vec![0; BLOCK_SAMPLES * 4 * 100]),
                ready: Some(ready_tx),
            },
            &tx,
            &flag,
        );
        done_tx.send(result.is_ok()).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(done_rx.try_recv().is_err());
    cancelled.store(true, Ordering::Relaxed);
    assert!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(rx.iter().count(), QUEUE_BLOCKS);
}

#[test]
fn partial_pcm_frames_are_rejected() {
    let (tx, _rx) = mpsc::sync_channel(QUEUE_BLOCKS);
    assert!(read_pcm(Cursor::new(vec![0; 7]), &tx, &AtomicBool::new(false)).is_err());
}

#[test]
fn rodio_pause_resume_and_rate_switch_keep_the_original_clock_without_a_device() {
    let (mixer, mut output) =
        rodio::mixer::mixer(2.try_into().unwrap(), SAMPLE_RATE.try_into().unwrap());
    let mut sink = rodio::Player::connect_new(&mixer);
    let mut base = 600.;
    for rate in [1.5, 0.75, 2.] {
        sink.stop();
        sink = rodio::Player::connect_new(&mixer);
        sink.pause();
        let (_tx, rx) = mpsc::sync_channel(QUEUE_BLOCKS);
        let progress = Arc::new(Progress::default());
        sink.append(TempoSource {
            rx,
            block: vec![0.25; SAMPLE_RATE as usize * 4],
            offset: 0,
            silent_right: false,
            progress: progress.clone(),
        });
        for _ in 0..4800 {
            output.next();
        }
        let paused = progress.position(base, rate);
        for _ in 0..48000 {
            output.next();
        }
        assert_eq!(progress.position(base, rate), paused);
        sink.play();
        for _ in 0..48000 {
            output.next();
        }
        let played = progress.position(base, rate) - paused;
        assert!(
            (played - 0.5 * rate).abs() < 0.02,
            "rate={rate}, elapsed source seconds={played}"
        );
        sink.pause();
        for _ in 0..4800 {
            output.next();
        }
        let stopped = progress.position(base, rate);
        for _ in 0..48000 {
            output.next();
        }
        assert_eq!(progress.position(base, rate), stopped);
        base = stopped;
    }
    assert!(base > 602. && base < 602.2);
}

fn fixture(path: &Path) {
    let samples = SAMPLE_RATE * 8 * 2;
    let mut out = fs::File::create(path).unwrap();
    out.write_all(b"RIFF").unwrap();
    out.write_all(&(36 + samples * 2).to_le_bytes()).unwrap();
    out.write_all(b"WAVEfmt ").unwrap();
    out.write_all(&16u32.to_le_bytes()).unwrap();
    out.write_all(&1u16.to_le_bytes()).unwrap();
    out.write_all(&2u16.to_le_bytes()).unwrap();
    out.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
    out.write_all(&(SAMPLE_RATE * 4).to_le_bytes()).unwrap();
    out.write_all(&4u16.to_le_bytes()).unwrap();
    out.write_all(&16u16.to_le_bytes()).unwrap();
    out.write_all(b"data").unwrap();
    out.write_all(&(samples * 2).to_le_bytes()).unwrap();
    let mut pcm = Vec::with_capacity(samples as usize * 2);
    for i in 0..SAMPLE_RATE * 8 {
        let time = f64::from(i) / f64::from(SAMPLE_RATE);
        for frequency in [440., 660.] {
            let frequency = frequency * if time < 4. { 1. } else { 2. };
            let value = (12000. * (time * frequency * std::f64::consts::TAU).sin()) as i16;
            pcm.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.write_all(&pcm).unwrap();
}

fn collect(mut source: TempoSource) -> Vec<f32> {
    let mut out = vec![];
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "tempo stream hung");
        let before = source.progress.frames.load(Ordering::Relaxed);
        let Some(left) = source.next() else {
            break;
        };
        let right = source.next().expect("complete stereo frame");
        if before == source.progress.frames.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        } else {
            out.extend_from_slice(&[left, right]);
        }
    }
    out
}

fn frequency(samples: &[f32], channel: usize, start: f64, end: f64) -> f64 {
    let start = (start * f64::from(SAMPLE_RATE)) as usize;
    let end = (end * f64::from(SAMPLE_RATE)) as usize;
    let crossings = (start + 1..end)
        .filter(|&i| samples[(i - 1) * 2 + channel] <= 0. && samples[i * 2 + channel] > 0.)
        .count();
    crossings as f64 / ((end - start) as f64 / f64::from(SAMPLE_RATE))
}

#[test]
#[ignore = "requires LAUT_TEST_FFMPEG; offline codec test, no audio device"]
fn real_tempo_preserves_pitch_channels_original_seek_time_and_bounded_cancellation() {
    let ffmpeg = std::env::var_os("LAUT_TEST_FFMPEG").expect("Set LAUT_TEST_FFMPEG");
    let ffmpeg = Path::new(&ffmpeg);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Stimmen ü mit Leerzeichen.wav");
    fixture(&path);
    let active = AtomicBool::new(true);
    let at = 2.;
    for rate in RATES.into_iter().filter(|r| *r != 1.) {
        let (source, control) = start(ffmpeg, &path, at, rate, &active).unwrap();
        let samples = collect(source);
        assert_eq!(control.progress.take_error(), None);
        let duration = samples.len() as f64 / 2. / f64::from(SAMPLE_RATE);
        assert!(
            (duration - 6. / rate).abs() < 0.08,
            "rate={rate}, duration={duration}"
        );
        // Original second 4 changes both frequencies. Seeking to second 2 means
        // that transition must still occur at second 4 on the displayed clock.
        for (channel, hz) in [(0, 440.), (1, 660.)] {
            let before = frequency(&samples, channel, 0.3 / rate, 1.3 / rate);
            let after = frequency(&samples, channel, 2.3 / rate, 3.3 / rate);
            assert!(
                (before - hz).abs() < 4.,
                "rate={rate}, channel={channel}, pitch={before}"
            );
            assert!(
                (after - 2. * hz).abs() < 4.,
                "rate={rate}, channel={channel}, pitch={after}"
            );
        }
        assert!((control.progress.position(at, rate) - (at + duration * rate)).abs() < 1e-9);
        eprintln!(
            "rate={rate}: output={duration:.4}s, original clock={:.4}s; stereo pitch and seek pass",
            control.progress.position(at, rate)
        );
    }
    let (source, control) = start(ffmpeg, &path, 0., 0.5, &active).unwrap();
    let child = control.child.clone();
    std::thread::sleep(Duration::from_millis(100));
    assert!(child.lock().unwrap().try_wait().unwrap().is_none());
    drop(control); // keep source/receiver alive, as with a suspended device
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.lock().unwrap().try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "cancelled FFmpeg survived");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(source);
    fs::write(&path, b"not audio").unwrap();
    assert!(start(ffmpeg, &path, 0., 1.5, &active).is_err());
    assert!(start(&temp.path().join("missing-ffmpeg"), &path, 0., 1.5, &active).is_err());
    assert!(start(ffmpeg, &path, 0., 3., &active).is_err());
    assert!(
        !temp
            .path()
            .read_dir()
            .unwrap()
            .any(|e| e.unwrap().path() != path)
    );
}
