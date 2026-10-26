//! Audio device adapters; recording, conversion and durable publication stay in laut-engine.
#[cfg(target_os = "macos")]
mod macos;
use anyhow::{Context, Result, bail, ensure};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use laut_engine::recording::{EncoderControl, Track, Writer};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Config {
    pub root: PathBuf,
    pub library: PathBuf,
    pub title: String,
    pub meeting: bool,
    pub microphone: String,
    pub output: String,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}
#[derive(Clone, Debug, Default)]
pub struct Devices {
    pub microphones: Vec<String>,
    pub outputs: Vec<String>,
}
pub fn devices() -> Result<Devices> {
    let host = cpal::default_host();
    let microphones = host
        .input_devices()?
        .map(|d| {
            d.description()
                .map(|description| description.name().to_owned())
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    #[cfg(windows)]
    let outputs = host
        .output_devices()?
        .map(|d| {
            d.description()
                .map(|description| description.name().to_owned())
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    #[cfg(not(windows))]
    let outputs = vec![];
    Ok(Devices {
        microphones,
        outputs,
    })
}
pub fn meeting_supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}
pub enum Event {
    Started(PathBuf),
    Level {
        seconds: f64,
        microphone: f32,
        system: f32,
    },
    Finishing,
    Finished(Result<PathBuf, String>),
}
#[derive(Default)]
struct Startup(AtomicU8);
impl Startup {
    fn begin(&self) -> bool {
        self.0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    fn cancel(&self) -> bool {
        self.0
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}
pub struct Handle {
    startup: Arc<Startup>,
    sender: mpsc::SyncSender<Event>,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    control: EncoderControl,
    pub events: mpsc::Receiver<Event>,
}
impl Handle {
    pub fn stop(&self) {
        if self.stop.swap(true, Ordering::Relaxed) {
            return;
        }
        if self.startup.cancel() {
            self.finished.store(true, Ordering::Release);
            let _ = self.sender.try_send(Event::Finished(Err(
                "Recording cancelled before start".into()
            )));
            return;
        }
        let done = self.finished.clone();
        let control = self.control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            if !done.load(Ordering::Acquire) {
                control.abort();
            }
        });
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.startup.cancel();
        if !self.finished.load(Ordering::Acquire) {
            self.control.abort();
        }
    }
}
struct Packet {
    track: usize,
    at: f64,
    samples: Vec<f32>,
}
#[derive(Clone)]
struct Sink {
    sender: SyncSender<Packet>,
    origin: Arc<OnceLock<Instant>>,
    failure: Arc<Mutex<Option<String>>>,
    peaks: Arc<[AtomicU32; 2]>,
}
impl Sink {
    fn error(&self, error: impl ToString) {
        let mut e = self.failure.lock().unwrap();
        if e.is_none() {
            *e = Some(error.to_string());
        }
    }
    fn send(&self, track: usize, latency: f64, samples: Vec<f32>) {
        let Some(origin) = self.origin.get() else {
            return;
        };
        let peak = samples.iter().fold(0f32, |a, x| a.max(x.abs()));
        self.peaks[track].fetch_max(peak.to_bits(), Ordering::Relaxed);
        let at = (origin.elapsed().as_secs_f64() - latency).max(0.);
        if self.sender.try_send(Packet { track, at, samples }).is_err() {
            self.error("Audio buffers could not be saved fast enough; recording stopped to avoid unnoticed gaps");
        }
    }
}
pub fn start(config: Config) -> Handle {
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let control = EncoderControl::default();
    let (tx, events) = mpsc::sync_channel(32);
    let stop_worker = stop.clone();
    let done = finished.clone();
    let children = control.clone();
    let startup = Arc::new(Startup::default());
    let gate = startup.clone();
    let sender = tx.clone();
    std::thread::spawn(move || {
        let result =
            record(config, &stop_worker, &gate, children, &tx).map_err(|e| format!("{e:#}"));
        if !done.swap(true, Ordering::AcqRel) {
            let _ = tx.send(Event::Finished(result));
        }
    });
    let gate = startup.clone();
    let timeout_stop = stop.clone();
    let timeout_done = finished.clone();
    let timeout_sender = sender.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        if !timeout_done.load(Ordering::Acquire) && gate.cancel() {
            timeout_stop.store(true, Ordering::Relaxed);
            timeout_done.store(true, Ordering::Release);
            let _ = timeout_sender.try_send(Event::Finished(Err(
                "Audio device did not respond. Check system permissions and the selected device, then restart Laut.".into()
            )));
        }
    });
    Handle {
        startup,
        sender,
        stop,
        finished,
        control,
        events,
    }
}
fn choose(host: &cpal::Host, name: &str, output: bool) -> Result<cpal::Device> {
    if name.is_empty() {
        return if output {
            host.default_output_device()
        } else {
            host.default_input_device()
        }
        .context("No audio device available");
    }
    let devices = if output {
        host.output_devices()?
    } else {
        host.input_devices()?
    };
    let mut found = devices.filter(|d| d.description().is_ok_and(|d| d.name() == name));
    let device = found
        .next()
        .context("Selected audio device is disconnected; choose it again")?;
    ensure!(
        found.next().is_none(),
        "Several audio devices have this name; choose the system default"
    );
    Ok(device)
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    index: usize,
    sink: Sink,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::Sample;
    let channels = config.channels as usize;
    let errors = sink.clone();
    ensure!(channels > 0 && channels <= 64, "Unsupported channel count");
    Ok(device.build_input_stream(
        *config,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            let samples = data
                .chunks_exact(channels)
                .map(|frame| {
                    frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32
                })
                .collect();
            let time = info.timestamp();
            let latency = time
                .callback
                .checked_duration_since(time.capture)
                .map_or(0., |d| d.as_secs_f64());
            sink.send(index, latency, samples);
        },
        move |e| errors.error(format!("Audio device stopped: {e}")),
        Some(Duration::from_secs(5)),
    )?)
}
fn input(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    index: usize,
    sink: Sink,
) -> Result<cpal::Stream> {
    let config = supported.config();
    match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(device, &config, index, sink),
        cpal::SampleFormat::F64 => build::<f64>(device, &config, index, sink),
        cpal::SampleFormat::I8 => build::<i8>(device, &config, index, sink),
        cpal::SampleFormat::I16 => build::<i16>(device, &config, index, sink),
        cpal::SampleFormat::I24 => build::<cpal::I24>(device, &config, index, sink),
        cpal::SampleFormat::U24 => build::<cpal::U24>(device, &config, index, sink),
        cpal::SampleFormat::I32 => build::<i32>(device, &config, index, sink),
        cpal::SampleFormat::I64 => build::<i64>(device, &config, index, sink),
        cpal::SampleFormat::U8 => build::<u8>(device, &config, index, sink),
        cpal::SampleFormat::U16 => build::<u16>(device, &config, index, sink),
        cpal::SampleFormat::U32 => build::<u32>(device, &config, index, sink),
        cpal::SampleFormat::U64 => build::<u64>(device, &config, index, sink),
        _ => bail!("Unsupported audio sample format"),
    }
}
fn record(
    config: Config,
    stop: &AtomicBool,
    startup: &Startup,
    control: EncoderControl,
    events: &mpsc::SyncSender<Event>,
) -> Result<PathBuf> {
    ensure!(
        !config.meeting || meeting_supported(),
        "System audio capture is not implemented on this platform yet"
    );
    #[cfg(target_os = "macos")]
    macos::authorize_microphone(stop)?;
    ensure!(
        !stop.load(Ordering::Relaxed),
        "Recording cancelled before start"
    );
    let host = cpal::default_host();
    let mic = choose(&host, &config.microphone, false)?;
    let mic_config = mic.default_input_config()?;
    let (tx, rx) = mpsc::sync_channel(256);
    let sink = Sink {
        sender: tx,
        origin: Arc::new(OnceLock::new()),
        failure: Arc::new(Mutex::new(None)),
        peaks: Arc::new([AtomicU32::new(0), AtomicU32::new(0)]),
    };
    let microphone = input(&mic, &mic_config, 0, sink.clone())
        .context("Cannot open microphone; check the app's microphone permission")?;
    #[allow(unused_mut)] // Linux currently captures microphone only.
    let mut tracks = vec![Track::new(
        "microphone",
        mic.description()?.name().into(),
        mic_config.sample_rate(),
    )];
    #[cfg(windows)]
    let system = if config.meeting {
        let device = choose(&host, &config.output, true)?;
        let format = device.default_output_config()?;
        tracks.push(Track::new(
            "system",
            device.description()?.name().into(),
            format.sample_rate(),
        ));
        Some(
            input(&device, &format, 1, sink.clone())
                .context("Cannot capture selected Windows audio output")?,
        )
    } else {
        None
    };
    #[cfg(target_os = "macos")]
    let system = if config.meeting {
        let stream = macos::prepare(sink.clone())?;
        tracks.push(Track::new("system", "macOS system audio".into(), 48000));
        Some(stream)
    } else {
        None
    };
    ensure!(
        !stop.load(Ordering::Relaxed),
        "Recording cancelled before start"
    );
    let mut writer = Writer::create(
        &config.root,
        &config.library,
        config.title,
        config.meeting,
        tracks,
        &config.ffmpeg,
        control,
    )?;
    let folder = writer.folder.clone();
    ensure!(startup.begin(), "Recording cancelled before start");
    sink.origin.set(Instant::now()).unwrap();
    let started = (|| -> Result<()> {
        microphone.play()?;
        #[cfg(windows)]
        if let Some(stream) = &system {
            stream.play()?;
        }
        #[cfg(target_os = "macos")]
        if let Some(stream) = &system {
            stream.start_capture()?;
        }
        Ok(())
    })();
    if let Err(e) = started {
        drop(microphone);
        #[cfg(any(windows, target_os = "macos"))]
        drop(system);
        return writer.finish(0., Some(format!("Cannot start capture: {e:#}")));
    }
    let _ = events.send(Event::Started(folder.clone()));
    let mut last = Instant::now();
    let mut progress = Instant::now();
    let mut mic_seen = Instant::now();
    let mut error = None;
    while !stop.load(Ordering::Relaxed) {
        if last.elapsed() > Duration::from_secs(5) {
            error=Some("Recording interrupted by sleep or a stalled audio writer; verify the partial recording".into());
            break;
        }
        last = Instant::now();
        if let Some(e) = sink.failure.lock().unwrap().take() {
            error = Some(e);
            break;
        }
        if mic_seen.elapsed() > Duration::from_secs(5) {
            error = Some(
                "Microphone stopped delivering samples; check permission and device connection"
                    .into(),
            );
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(packet) => {
                if packet.track == 0 {
                    mic_seen = Instant::now();
                }
                if let Err(e) = writer.write(packet.track, packet.at, &packet.samples) {
                    error = Some(format!("{e:#}"));
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                error = Some("Capture connection closed unexpectedly".into());
                break;
            }
        }
        if progress.elapsed() > Duration::from_millis(100) {
            let seconds = sink.origin.get().unwrap().elapsed().as_secs_f64();
            let _ = events.try_send(Event::Level {
                seconds,
                microphone: f32::from_bits(sink.peaks[0].swap(0, Ordering::Relaxed)),
                system: f32::from_bits(sink.peaks[1].swap(0, Ordering::Relaxed)),
            });
            progress = Instant::now();
            if seconds >= 86400. {
                error = Some("24-hour recording limit reached; audio is retained".into());
                break;
            }
        }
    }
    let duration = sink.origin.get().unwrap().elapsed().as_secs_f64();
    drop(microphone);
    #[cfg(target_os = "macos")]
    if let Some(stream) = system
        && let Err(e) = stream.stop_capture()
    {
        error.get_or_insert_with(|| e.to_string());
    }
    #[cfg(windows)]
    drop(system);
    for packet in rx.try_iter() {
        if let Err(e) = writer.write(packet.track, packet.at, &packet.samples) {
            error.get_or_insert_with(|| e.to_string());
            break;
        }
    }
    if let Some(e) = sink.failure.lock().unwrap().take() {
        error.get_or_insert(e);
    }
    let duration = if error.is_none() {
        duration
    } else {
        writer
            .manifest
            .tracks
            .iter()
            .map(|t| t.frames as f64 / t.rate as f64)
            .fold(0., f64::max)
    };
    let _ = events.send(Event::Finishing);
    writer.finish(duration, error)?;
    laut_engine::recording::publish(
        &folder,
        &config.ffmpeg,
        &config.ffprobe,
        false,
        &AtomicBool::new(false),
    )
}

#[cfg(test)]
mod tests {
    use super::Startup;
    #[test]
    fn cancelled_device_initialization_cannot_start_later() {
        let gate = Startup::default();
        assert!(gate.cancel());
        assert!(!gate.begin());
        assert!(!gate.cancel());
        let running = Startup::default();
        assert!(running.begin());
        assert!(!running.cancel()); // A running capture must finish/save through its worker.
    }
}
