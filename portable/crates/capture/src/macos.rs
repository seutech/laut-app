use super::*;
use screencapturekit::{cm::CMClock, prelude::*, stream::delegate_trait::StreamCallbacks};

#[allow(unsafe_code)]
pub(super) fn authorize_microphone(stop: &AtomicBool) -> Result<()> {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: AVMediaTypeAudio is Apple's immutable, process-lifetime media-type
    // constant. These class methods accept it on any thread. The heap-owned block
    // captures only a Send channel; AVFoundation copies/retains it until completion.
    let media = unsafe { AVMediaTypeAudio }.context("macOS audio media type unavailable")?;
    // SAFETY: `media` is the valid immutable system constant described above.
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) };
    if status == AVAuthorizationStatus::Authorized {
        return Ok(());
    }
    ensure!(
        status == AVAuthorizationStatus::NotDetermined,
        "Microphone permission denied. Enable Laut in macOS Privacy & Security → Microphone, then retry."
    );
    let (tx, rx) = mpsc::sync_channel(1);
    let callback = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
        let _ = tx.try_send(granted.as_bool());
    });
    // SAFETY: The media-type and callback lifetime/ABI meet the contract above.
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &callback) };
    loop {
        ensure!(
            !stop.load(Ordering::Relaxed),
            "Recording cancelled before start"
        );
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(true) => return Ok(()),
            Ok(false) => bail!("Microphone permission denied; no audio was recorded"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

pub(super) fn prepare(sink: Sink) -> Result<SCStream> {
    let content=SCShareableContent::get().context("System audio permission is required in macOS Privacy & Security → Screen & System Audio Recording")?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .context("No display available for system audio capture")?;
    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build()?;
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_minimum_frame_interval(&CMTime::new(1, 1))
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(48000)
        .with_channel_count(1);
    let errors = sink.clone();
    let delegate = StreamCallbacks::new()
        .on_error(move |error| errors.error(format!("System audio stopped: {error}")));
    let mut stream = SCStream::new_with_delegate(&filter, &config, delegate)?;
    stream.add_output_handler(
        move |buffer: CMSampleBuffer, kind: SCStreamOutputType| {
            if kind != SCStreamOutputType::Audio {
                return;
            }
            let result = (|| -> Result<()> {
                let format = buffer
                    .format_description()
                    .context("Missing system audio format")?;
                ensure!(
                    format.audio_is_float()
                        && !format.audio_is_big_endian()
                        && format.audio_bits_per_channel() == Some(32)
                        && format.audio_channel_count() == Some(1)
                        && format.audio_sample_rate() == Some(48000.),
                    "Unexpected system audio format"
                );
                let buffers = buffer
                    .audio_buffer_list()
                    .map_err(|e| anyhow::anyhow!("Cannot read system audio: {e}"))?;
                let audio = buffers.get(0).context("Empty system audio buffer")?;
                ensure!(
                    audio.number_channels == 1 && audio.data().len() % 4 == 0,
                    "Invalid system audio buffer"
                );
                let samples = audio
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect();
                let now = CMClock::host_time_clock()
                    .time()
                    .as_seconds()
                    .context("Invalid host clock")?;
                let pts = buffer
                    .presentation_timestamp()
                    .as_seconds()
                    .context("Invalid system audio timestamp")?;
                sink.send(1, (now - pts).max(0.), samples);
                Ok(())
            })();
            if let Err(e) = result {
                sink.error(e);
            }
        },
        SCStreamOutputType::Audio,
    )?;
    Ok(stream)
}
