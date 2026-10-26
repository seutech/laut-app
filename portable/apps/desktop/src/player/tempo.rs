//! Bounded local FFmpeg tempo stream. The device callback never waits for a process.
use anyhow::{Context, Result, bail, ensure};
use rodio::Source;
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    time::{Duration, Instant},
};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const BLOCK_SAMPLES: usize = 960 * CHANNELS; // 20 ms
const QUEUE_BLOCKS: usize = 8;
pub use laut_engine::preferences::PLAYBACK_SPEEDS as RATES;

pub fn valid_rate(rate: f64) -> bool {
    RATES.contains(&rate)
}

#[derive(Default)]
pub struct Progress {
    frames: AtomicU64,
    pub buffering: AtomicBool,
    error: Mutex<Option<String>>,
}
impl Progress {
    pub fn position(&self, base: f64, rate: f64) -> f64 {
        base + self.frames.load(Ordering::Relaxed) as f64 / f64::from(SAMPLE_RATE) * rate
    }
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().ok()?.take()
    }
}

pub struct Control {
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
    pub progress: Arc<Progress>,
}
impl Drop for Control {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        // The reader owns reaping; neither the UI nor the audio callback joins it.
    }
}

pub struct TempoSource {
    rx: Receiver<Vec<f32>>,
    block: Vec<f32>,
    offset: usize,
    silent_right: bool,
    progress: Arc<Progress>,
}
impl Iterator for TempoSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        // Always emit a complete silent stereo frame on underrun. Data arriving
        // between L/R must not swap channels or advance the original-media clock.
        if self.silent_right {
            self.silent_right = false;
            return Some(0.);
        }
        if self.offset == self.block.len() {
            match self.rx.try_recv() {
                Ok(block) => {
                    self.block = block;
                    self.offset = 0;
                    self.progress.buffering.store(false, Ordering::Relaxed);
                }
                Err(TryRecvError::Empty) => {
                    self.progress.buffering.store(true, Ordering::Relaxed);
                    self.silent_right = true;
                    return Some(0.);
                }
                Err(TryRecvError::Disconnected) => return None,
            }
        }
        let sample = self.block[self.offset];
        self.offset += 1;
        if self.offset.is_multiple_of(CHANNELS) {
            self.progress.frames.fetch_add(1, Ordering::Relaxed);
        }
        Some(sample)
    }
}
impl Source for TempoSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(BLOCK_SAMPLES)
    }
    fn channels(&self) -> rodio::ChannelCount {
        (CHANNELS as u16).try_into().unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        SAMPLE_RATE.try_into().unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub fn start(
    ffmpeg: &Path,
    path: &Path,
    at: f64,
    rate: f64,
    active: &AtomicBool,
) -> Result<(TempoSource, Control)> {
    ensure!(valid_rate(rate), "Unsupported playback speed");
    ensure!(at.is_finite() && at >= 0., "Invalid audio position");
    // A local, canonical file also prevents interpreting a name as a network URL.
    let path = path.canonicalize().context("Audio file unavailable")?;
    ensure!(path.is_file(), "Audio file unavailable");
    let mut command = Command::new(ffmpeg);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "file,pipe",
        "-ss",
        &format!("{at:.6}"),
        "-threads",
        "1",
        "-i",
    ]);
    command
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-filter_threads",
            "1",
            "-af",
            &format!("atempo={rate}"),
            "-ar",
            "48000",
            "-ac",
            "2",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .context("Playback speed needs local FFmpeg; select it under Models / Local programs")?;
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let child = Arc::new(Mutex::new(child));
    let cancelled = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Progress::default());
    let control = Control {
        child: child.clone(),
        cancelled: cancelled.clone(),
        progress: progress.clone(),
    };
    let (tx, rx) = mpsc::sync_channel(QUEUE_BLOCKS);
    // Drain stderr continuously, retaining at most 8 KiB even for a broken worker.
    let log = std::thread::Builder::new()
        .name("laut-tempo-log".into())
        .spawn(move || {
            let mut tail = Vec::new();
            let mut bytes = [0; 2048];
            while let Ok(n) = stderr.read(&mut bytes) {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&bytes[..n]);
                if tail.len() > 8192 {
                    tail.drain(..tail.len() - 8192);
                }
            }
            String::from_utf8_lossy(&tail).trim().to_owned()
        })?;
    let reader_progress = progress.clone();
    std::thread::Builder::new()
        .name("laut-tempo-pcm".into())
        .spawn(move || {
            let result = read_pcm(stdout, &tx, &cancelled);
            // Do not hold the child lock while waiting: cancellation must always work.
            let deadline = Instant::now() + Duration::from_secs(2);
            let status = loop {
                let status = child.lock().unwrap().try_wait();
                match status {
                    Ok(Some(status)) => break Ok(status),
                    Err(e) => break Err(e),
                    Ok(None) => {
                        if cancelled.load(Ordering::Relaxed)
                            || result.is_err()
                            || Instant::now() >= deadline
                        {
                            let _ = child.lock().unwrap().kill();
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            };
            let log = log.join().unwrap_or_default();
            if !cancelled.load(Ordering::Relaxed) {
                let error = match (result, status) {
                    (Err(e), _) => Some(format!("Audio processing: {e:#}")),
                    (_, Ok(s)) if !s.success() => {
                        Some(format!("FFmpeg audio processing failed: {log}"))
                    }
                    (_, Err(e)) => Some(format!("Audio processing: {e}")),
                    _ => None,
                };
                *reader_progress.error.lock().unwrap() = error;
            }
            // Closing tx signals EOF only after the exit status/error is available.
        })?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let first = loop {
        ensure!(active.load(Ordering::Relaxed), "Audio cancelled");
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(block) => break block,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(error) = progress.take_error() {
                    bail!(error);
                }
                break vec![]; // Seeking to the exact end is a successful empty stream.
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                ensure!(Instant::now() < deadline, "Audio processing timed out")
            }
        }
    };
    Ok((
        TempoSource {
            rx,
            block: first,
            offset: 0,
            silent_right: false,
            progress,
        },
        control,
    ))
}

fn read_pcm(mut input: impl Read, tx: &SyncSender<Vec<f32>>, cancelled: &AtomicBool) -> Result<()> {
    let mut bytes = vec![0; BLOCK_SAMPLES * 4];
    loop {
        let mut count = 0;
        while count < bytes.len() && !cancelled.load(Ordering::Relaxed) {
            match input.read(&mut bytes[count..]) {
                Ok(0) => break,
                Ok(n) => count += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        if cancelled.load(Ordering::Relaxed) || count == 0 {
            return Ok(());
        }
        ensure!(
            count.is_multiple_of(4 * CHANNELS),
            "Truncated stereo PCM frame"
        );
        let mut block = bytes[..count]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(());
            }
            match tx.try_send(block) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return Ok(()),
                Err(TrySendError::Full(b)) => {
                    block = b;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
