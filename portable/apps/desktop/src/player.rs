//! Audio device and file operations never run on the UI thread.
mod tempo;
use anyhow::{Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player as AudioPlayer, Source};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
pub use tempo::RATES;

enum Command {
    Seek(f64, bool),
    Rate(f64, PathBuf),
    Pause,
}
#[derive(Default)]
struct State {
    revision: u64,
    position: f64,
    playing: bool,
    buffering: bool,
    error: Option<String>,
}

pub struct Player {
    tx: mpsc::Sender<(u64, Command)>,
    rx: mpsc::Receiver<State>,
    alive: Arc<AtomicBool>,
    state: State,
    revision: u64,
}
impl Player {
    pub fn open(path: &Path, ffmpeg: PathBuf, rate: f64) -> Result<Self> {
        anyhow::ensure!(tempo::valid_rate(rate), "Unsupported playback speed");
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let active = alive.clone();
        let path = path.to_owned();
        std::thread::Builder::new()
            .name("laut-audio".into())
            .spawn(move || {
                let mut transport = Transport {
                    path,
                    ffmpeg,
                    rate,
                    position: 0.,
                    audio: None,
                };
                let mut revision = 0;
                loop {
                    let command = commands.recv_timeout(Duration::from_millis(80));
                    if !active.load(Ordering::Relaxed) {
                        break;
                    }
                    let result = match command {
                        Ok((id, command)) => {
                            revision = id;
                            transport.apply(command, &active)
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mut state = transport.state();
                    state.revision = revision;
                    if let Err(error) = result {
                        state.error = Some(format!("{error:#}"));
                    }
                    if events.send(state).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            tx,
            rx,
            alive,
            state: State::default(),
            revision: 0,
        })
    }
    pub fn poll(&mut self) -> Option<String> {
        let mut error = None;
        while let Ok(mut state) = self.rx.try_recv() {
            if let Some(message) = state.error.take() {
                error = Some(message);
            }
            // A periodic event from before a seek must not acknowledge that seek.
            if state.revision == self.revision {
                self.state = state;
            }
        }
        error
    }
    pub fn position(&self) -> f64 {
        self.state.position
    }
    pub fn playing(&self) -> bool {
        self.state.playing
    }
    pub fn buffering(&self) -> bool {
        self.state.buffering
    }
    pub fn pending(&self) -> bool {
        self.state.revision != self.revision
    }
    fn send(&mut self, command: Command) -> Result<()> {
        let revision = self.revision + 1;
        self.tx.send((revision, command))?;
        self.revision = revision;
        Ok(())
    }
    pub fn pause(&mut self) {
        let _ = self.send(Command::Pause);
    }
    pub fn seek(&mut self, time: f64, play: bool) -> Result<()> {
        anyhow::ensure!(time.is_finite() && time >= 0., "Invalid audio position");
        self.send(Command::Seek(time, play))
    }
    pub fn set_rate(&mut self, rate: f64, ffmpeg: PathBuf) -> Result<()> {
        anyhow::ensure!(tempo::valid_rate(rate), "Unsupported playback speed");
        self.send(Command::Rate(rate, ffmpeg))
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        // A slow device/file may finish opening after its project was closed. Never play it.
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.tx.send((self.revision, Command::Pause));
    }
}

struct Transport {
    path: PathBuf,
    ffmpeg: PathBuf,
    rate: f64,
    position: f64,
    audio: Option<Audio>,
}
impl Transport {
    fn state(&self) -> State {
        self.audio.as_ref().map(Audio::state).unwrap_or(State {
            position: self.position,
            ..State::default()
        })
    }
    fn apply(&mut self, command: Command, active: &AtomicBool) -> Result<()> {
        match command {
            Command::Seek(at, play) => self.seek(at, play, active),
            Command::Pause => {
                if let Some(audio) = &self.audio {
                    audio.sink.pause();
                }
                Ok(())
            }
            Command::Rate(rate, ffmpeg) => {
                // Read the source clock here, not the UI's 80 ms old snapshot.
                let (at, play) = self
                    .audio
                    .as_ref()
                    .map(|a| (a.position(), a.playing()))
                    .unwrap_or((self.position, false));
                self.rate = rate;
                self.ffmpeg = ffmpeg;
                self.seek(at, play, active)
            }
        }
    }
    fn seek(&mut self, at: f64, play: bool, active: &AtomicBool) -> Result<()> {
        self.position = at;
        if self.audio.is_none() && play {
            self.audio = Some(Audio::open(self.path.clone())?);
        }
        if let Some(audio) = &mut self.audio {
            audio.seek(at, play, self.rate, &self.ffmpeg, active)?;
        }
        Ok(())
    }
}

struct Audio {
    stream: MixerDeviceSink,
    sink: AudioPlayer,
    path: PathBuf,
    base: f64,
    rate: f64,
    tempo: Option<tempo::Control>,
}
impl Audio {
    fn open(path: PathBuf) -> Result<Self> {
        let stream = DeviceSinkBuilder::open_default_sink().context("Audio output unavailable")?;
        let sink = AudioPlayer::connect_new(stream.mixer());
        sink.pause();
        Ok(Self {
            stream,
            sink,
            path,
            base: 0.,
            rate: 1.,
            tempo: None,
        })
    }
    fn seek(
        &mut self,
        time: f64,
        play: bool,
        rate: f64,
        ffmpeg: &Path,
        active: &AtomicBool,
    ) -> Result<()> {
        self.sink.stop();
        self.tempo = None; // kill/reap the old converter, including while paused
        self.sink = AudioPlayer::connect_new(self.stream.mixer());
        self.sink.pause();
        self.base = time;
        self.rate = rate;
        // Paused navigation and rate changes need neither decoding nor a worker.
        if !play {
            return Ok(());
        }
        if rate == 1. {
            let mut decoder = Decoder::try_from(File::open(&self.path)?)?;
            // Seek before handing the decoder to the callback; never wait on a
            // device callback for seek feedback (a suspended device could stall).
            decoder
                .try_seek(Duration::from_secs_f64(time))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            self.sink.append(decoder);
        } else {
            let (source, control) = tempo::start(ffmpeg, &self.path, time, rate, active)?;
            self.tempo = Some(control);
            self.sink.append(source);
        }
        if active.load(Ordering::Relaxed) {
            self.sink.play();
        }
        Ok(())
    }
    fn position(&self) -> f64 {
        self.tempo
            .as_ref()
            .map(|t| t.progress.position(self.base, self.rate))
            .unwrap_or_else(|| self.base + self.sink.get_pos().as_secs_f64())
    }
    fn playing(&self) -> bool {
        !self.sink.is_paused() && !self.sink.empty()
    }
    fn state(&self) -> State {
        State {
            position: self.position(),
            playing: self.playing(),
            buffering: self.playing()
                && self
                    .tempo
                    .as_ref()
                    .is_some_and(|t| t.progress.buffering.load(Ordering::Relaxed)),
            error: self.tempo.as_ref().and_then(|t| t.progress.take_error()),
            ..State::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_transport_preserves_original_time_without_audio_device_or_ffmpeg() {
        let active = AtomicBool::new(true);
        let mut t = Transport {
            path: "missing.wav".into(),
            ffmpeg: "missing-ffmpeg".into(),
            rate: 1.,
            position: 0.,
            audio: None,
        };
        t.apply(Command::Seek(7. * 3600. - 10., false), &active)
            .unwrap();
        for rate in RATES {
            t.apply(Command::Rate(rate, "missing-ffmpeg".into()), &active)
                .unwrap();
            assert_eq!(t.state().position, 25190.);
            assert!(!t.state().playing);
            assert!(t.audio.is_none());
        }
        t.apply(Command::Seek(123.456, false), &active).unwrap();
        assert_eq!(t.state().position, 123.456);
    }
    #[test]
    fn old_events_do_not_complete_new_seek() {
        let (tx, _) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let mut player = Player {
            tx,
            rx,
            alive: Arc::new(AtomicBool::new(true)),
            state: State::default(),
            revision: 2,
        };
        events
            .send(State {
                revision: 1,
                position: 10.,
                ..State::default()
            })
            .unwrap();
        player.poll();
        assert!(player.pending());
        events
            .send(State {
                revision: 2,
                position: 42.,
                ..State::default()
            })
            .unwrap();
        player.poll();
        assert!(!player.pending());
        assert_eq!(player.position(), 42.);
    }
}
