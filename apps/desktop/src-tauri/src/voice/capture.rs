//! Microphone capture (§54).
//!
//! `cpal`'s `Stream` is tied to the platform audio host and never crosses a
//! thread boundary here — the whole open → record → stop lifecycle runs on
//! one dedicated thread, spun up by `Recording::start` and joined by
//! `Recording::stop`. Only the accumulated samples (built up behind a plain
//! `Mutex`, which *is* thread-safe) and a stop flag cross into or out of it.
//!
//! This machine has no microphone attached to develop against — confirmed by
//! `cpal` itself reporting no default input device, not assumed. The pure
//! resampling math below is tested against synthetic data; the device
//! open/record/stop lifecycle is not exercised by any test here and needs
//! verification on a machine with a real microphone before this ships. See
//! docs/HANDOFF.md.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

use super::{Result, VoiceError};

/// What whisper.cpp expects: 16kHz, mono, 32-bit float PCM.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// How long to wait for the capture thread to confirm the stream is actually
/// running before giving up and reporting a device error to the caller.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the capture thread checks whether it has been asked to stop.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// A cheap, cloneable handle onto a recording's samples while it is still in
/// progress — what the streaming poll loop (§54 streaming, D30) reads from
/// without needing to touch `Recording` itself, so a `voice_stop_recording`
/// or `voice_cancel_recording` call and an in-flight poll never contend for
/// the same lock.
#[derive(Clone)]
pub struct AudioTap {
    buffer: Arc<Mutex<Vec<f32>>>,
    channels: u16,
    sample_rate: u32,
}

impl AudioTap {
    /// Everything captured so far, resampled to 16kHz mono. Safe to call
    /// repeatedly while recording continues — it clones the buffer rather
    /// than draining it, so nothing about the final `stop()` result changes
    /// because a poll happened to run first.
    pub fn snapshot(&self) -> Vec<f32> {
        let raw = self.buffer.lock().unwrap().clone();
        resample_mono_16k(&raw, self.channels, self.sample_rate)
    }
}

/// A recording in progress. Lives in `AppState` between `start` and `stop`.
pub struct Recording {
    stop: Arc<AtomicBool>,
    tap: AudioTap,
    join: JoinHandle<()>,
}

impl Recording {
    /// Begin capturing from the default input device on its own thread.
    ///
    /// Fails fast if there is no input device at all, before ever spinning up
    /// a thread for nothing; every other failure (device busy, format
    /// rejected, stream would not start) is reported back from inside the
    /// thread through a small rendezvous channel, so it surfaces here too
    /// rather than silently recording nothing.
    pub fn start() -> Result<Self> {
        cpal::default_host()
            .default_input_device()
            .ok_or(VoiceError::NoInputDevice)?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let buffer_thread = buffer.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(u16, u32)>>();

        let join = std::thread::Builder::new()
            .name("voice-capture".into())
            .spawn(move || run_capture(stop_thread, buffer_thread, ready_tx))
            .map_err(|e| VoiceError::Device(e.to_string()))?;

        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok((channels, sample_rate))) => Ok(Self {
                stop,
                tap: AudioTap {
                    buffer,
                    channels,
                    sample_rate,
                },
                join,
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                let _ = join.join();
                Err(VoiceError::Device(
                    "capture did not start in time".to_string(),
                ))
            }
        }
    }

    /// A cloneable handle for reading samples while recording continues.
    pub fn tap(&self) -> AudioTap {
        self.tap.clone()
    }

    /// Stop capturing and return what was recorded, resampled to 16kHz mono.
    pub fn stop(self) -> Vec<f32> {
        let samples = self.tap.snapshot();
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.join.join();
        samples
    }

    /// Stop and discard — for a cancelled recording.
    pub fn cancel(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.join.join();
    }
}

fn run_capture(
    stop: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
    ready: std::sync::mpsc::Sender<Result<(u16, u32)>>,
) {
    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            let _ = ready.send(Err(VoiceError::NoInputDevice));
            return;
        }
    };
    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(VoiceError::Device(e.to_string())));
            return;
        }
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let sample_format = config.sample_format();

    let stream = build_stream(&device, &config.into(), sample_format, buffer);
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    if let Err(e) = stream.play() {
        let _ = ready.send(Err(VoiceError::Device(e.to_string())));
        return;
    }
    let _ = ready.send(Ok((channels, sample_rate)));

    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(POLL_INTERVAL);
    }
    drop(stream);
}

/// Build the input stream in whatever format the device actually reports.
///
/// A real device is not guaranteed to hand back `f32` — WASAPI shared mode
/// usually does, but assuming it without checking is exactly the kind of
/// thing that works on the one machine it was built on and fails on the
/// first different one, which here is *every* machine, since this one has no
/// microphone to check against at all.
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    buffer: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream> {
    let err_fn = |err: cpal::StreamError| tracing::warn!(error = %err, "voice capture stream error");

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                buffer.lock().unwrap().extend_from_slice(data);
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let mut buf = buffer.lock().unwrap();
                buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let mut buf = buffer.lock().unwrap();
                buf.extend(
                    data.iter()
                        .map(|&s| (s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0)),
                );
            },
            err_fn,
            None,
        ),
        other => {
            return Err(VoiceError::Device(format!(
                "unsupported input sample format: {other:?}"
            )))
        }
    };

    stream.map_err(|e| VoiceError::Device(e.to_string()))
}

/// Down-mix to mono and resample to 16kHz — what whisper.cpp requires.
///
/// Nearest-neighbour resampling, not a proper sinc filter: dictation is short
/// utterances of speech, not music, and the accuracy this trades away does
/// not matter next to shipping the feature. If quality ever demands better,
/// this is the one function that would need to change.
pub fn resample_mono_16k(samples: &[f32], channels: u16, input_rate: u32) -> Vec<f32> {
    if samples.is_empty() || channels == 0 {
        return Vec::new();
    }

    let mono: Vec<f32> = samples
        .chunks(channels as usize)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();

    if input_rate == TARGET_SAMPLE_RATE {
        return mono;
    }

    let ratio = input_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let out_len = (mono.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src = (i as f64 * ratio) as usize;
            mono[src.min(mono.len().saturating_sub(1))]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_downmixed_by_averaging_channels() {
        // L=1.0, R=-1.0 on every frame; the average is silence.
        let samples = vec![1.0, -1.0, 1.0, -1.0];
        let mono = resample_mono_16k(&samples, 2, TARGET_SAMPLE_RATE);
        assert!(mono.iter().all(|&s| s.abs() < 1e-6));
        assert_eq!(mono.len(), 2);
    }

    #[test]
    fn a_higher_input_rate_is_downsampled_to_16k() {
        // 48kHz input, 1 second of audio: 48000 samples in, ~16000 out.
        let samples = vec![0.5_f32; 48_000];
        let out = resample_mono_16k(&samples, 1, 48_000);
        assert!(
            (out.len() as i64 - 16_000).abs() < 10,
            "expected ~16000 samples, got {}",
            out.len()
        );
    }

    #[test]
    fn matching_the_target_rate_is_a_no_op() {
        let samples = vec![0.1, 0.2, 0.3];
        let out = resample_mono_16k(&samples, 1, TARGET_SAMPLE_RATE);
        assert_eq!(out, samples);
    }

    #[test]
    fn empty_input_produces_empty_output() {
        assert!(resample_mono_16k(&[], 1, TARGET_SAMPLE_RATE).is_empty());
    }

    #[test]
    fn zero_channels_cannot_divide_and_produces_nothing_instead_of_panicking() {
        assert!(resample_mono_16k(&[1.0, 2.0], 0, TARGET_SAMPLE_RATE).is_empty());
    }
}
