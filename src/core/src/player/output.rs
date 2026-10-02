//! cpal device and output stream management.
//!
//! The decode thread pushes interleaved f32 samples into a bounded ring buffer;
//! the audio callback drains it. The callback runs on a realtime thread, so it
//! never allocates, never blocks, and treats an empty buffer as silence rather
//! than as an error.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

use crate::errors::{MhError, MhResult};

use super::clock::PlaybackClock;

/// Roughly four seconds of stereo audio at 48 kHz. Large enough to ride out
/// network stalls on a streamed track, small enough that a seek does not have
/// to discard much.
const RING_CAPACITY: usize = 48_000 * 2 * 4;

/// Hold output silent until this much audio is queued. Starting the moment the
/// first samples land leaves no cushion, and the device reports an underrun as
/// soon as decoding hiccups.
const PREBUFFER: usize = 48_000 * 2 / 4;

/// Samples to ramp in after a flush. Resuming mid-waveform steps the signal
/// instantly from whatever the device last played to the new position's
/// amplitude, which is audible as a click.
const DECLICK: usize = 256;

/// Bounded SPSC sample queue shared between the decode thread and the audio
/// callback.
///
/// A `Mutex` guards the buffer, but the callback only ever does a `try_lock`
/// and falls back to silence, so it never blocks the realtime thread.
pub struct SampleRing {
    buf: Mutex<std::collections::VecDeque<f32>>,
    /// `buf.len()`, mirrored so a reader can ask "is there audio left?" without
    /// taking the lock. Only ever written while the lock is held, so it cannot drift.
    buffered: AtomicUsize,
    /// Signals the decode thread that space has freed up.
    space: Condvar,
    capacity: usize,
    /// Set when the decoder has delivered the last sample of the track.
    finished: AtomicBool,
    /// Cleared until the initial cushion has built up, and after every flush.
    primed: AtomicBool,
    /// Counts down the post-flush fade-in.
    ramp: AtomicU64,
    /// Counts down a fade-out from the last played level after a flush, so the
    /// signal decays to silence instead of being cut mid-waveform.
    fade_out: AtomicU64,
    /// Last sample handed to the device, as bits, for that fade-out.
    last_sample: AtomicU64,
    /// Bumped on every seek/stop so a late decoder push can be discarded.
    generation: AtomicU64,
    /// Set whenever the callback had to pad a period with silence.
    starving: AtomicBool,
}

impl SampleRing {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buf: Mutex::new(std::collections::VecDeque::with_capacity(RING_CAPACITY)),
            buffered: AtomicUsize::new(0),
            space: Condvar::new(),
            capacity: RING_CAPACITY,
            finished: AtomicBool::new(false),
            primed: AtomicBool::new(false),
            ramp: AtomicU64::new(0),
            fade_out: AtomicU64::new(0),
            last_sample: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            starving: AtomicBool::new(false),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// True while the device is being fed silence the decoder failed to supply.
    pub fn is_starving(&self) -> bool {
        self.starving.load(Ordering::Acquire)
    }

    /// Drop everything buffered and invalidate in-flight decoder pushes.
    pub fn flush(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.finished.store(false, Ordering::Release);
        self.primed.store(false, Ordering::Release);
        self.ramp.store(DECLICK as u64, Ordering::Release);
        self.fade_out.store(DECLICK as u64, Ordering::Release);
        if let Ok(mut buf) = self.buf.lock() {
            buf.clear();
            self.buffered.store(0, Ordering::Release);
        }
        self.space.notify_all();
    }

    pub fn mark_finished(&self) {
        self.finished.store(true, Ordering::Release);
    }

    /// True once the decoder is done and the buffer has drained.
    ///
    /// Reads the mirrored length rather than locking: this is polled on a 50 ms timer
    /// at end of track, and the audio callback only ever `try_lock`s — so every
    /// blocking acquisition here was a chance to make the callback fall back to
    /// silence, which is a click plus a stalled position readout.
    pub fn is_drained(&self) -> bool {
        self.finished.load(Ordering::Acquire) && self.buffered.load(Ordering::Acquire) == 0
    }

    /// Push decoded samples, blocking while the ring is full.
    ///
    /// Returns `false` if a flush happened during the wait, meaning these
    /// samples belong to a position the listener has already seeked away from.
    pub fn push(&self, samples: &[f32], generation: u64) -> bool {
        let mut buf = match self.buf.lock() {
            Ok(b) => b,
            Err(_) => return false,
        };

        for chunk in samples.chunks(1024) {
            while buf.len() + chunk.len() > self.capacity {
                if self.generation.load(Ordering::Acquire) != generation {
                    return false;
                }
                buf = match self
                    .space
                    .wait_timeout(buf, std::time::Duration::from_millis(100))
                {
                    Ok((b, _)) => b,
                    Err(_) => return false,
                };
            }
            if self.generation.load(Ordering::Acquire) != generation {
                return false;
            }
            buf.extend(chunk.iter().copied());
            self.buffered.store(buf.len(), Ordering::Release);
        }
        true
    }

    /// Fill `out` from the ring, returning how many samples were real audio.
    ///
    /// Called from the audio callback: it must not block, so a contended lock
    /// yields silence for that period instead of waiting.
    fn drain_into(&self, out: &mut [f32]) -> usize {
        let Ok(mut buf) = self.buf.try_lock() else {
            out.fill(0.0);
            return 0;
        };

        let mut fading = self.fade_out.load(Ordering::Acquire);
        if fading > 0 {
            let from = f32::from_bits(self.last_sample.load(Ordering::Acquire) as u32);
            for slot in out.iter_mut() {
                if fading == 0 {
                    *slot = 0.0;
                    continue;
                }
                *slot = from * (fading as f32 / DECLICK as f32);
                fading -= 1;
            }
            self.fade_out.store(fading, Ordering::Release);
            return 0;
        }

        if !self.primed.load(Ordering::Acquire) {
            if buf.len() < PREBUFFER && !self.finished.load(Ordering::Acquire) {
                out.fill(0.0);
                self.starving.store(true, Ordering::Release);
                return 0;
            }
            self.primed.store(true, Ordering::Release);
        }

        let n = out.len().min(buf.len());
        {
            let (head, tail) = buf.as_slices();
            let from_head = n.min(head.len());
            out[..from_head].copy_from_slice(&head[..from_head]);
            if from_head < n {
                out[from_head..n].copy_from_slice(&tail[..n - from_head]);
            }
        }
        buf.drain(..n);
        self.buffered.store(buf.len(), Ordering::Release);
        out[n..].fill(0.0);

        let mut remaining = self.ramp.load(Ordering::Acquire);
        if remaining > 0 {
            for slot in out.iter_mut().take(n) {
                if remaining == 0 {
                    break;
                }
                let gain = 1.0 - (remaining as f32 / DECLICK as f32);
                *slot *= gain;
                remaining -= 1;
            }
            self.ramp.store(remaining, Ordering::Release);
        }

        self.starving.store(
            n < out.len() && !self.finished.load(Ordering::Acquire),
            Ordering::Release,
        );

        if n > 0 {
            self.last_sample
                .store(out[n - 1].to_bits() as u64, Ordering::Release);
            self.space.notify_one();
        }
        n
    }
}

/// A device error, split by whether playback survives it.
///
/// cpal recovers an xrun itself (`prepare` + `start`) and keeps the worker
/// running, so reporting one as a playback failure stops audio the device was
/// still perfectly able to play.
pub enum OutputError {
    Recovered(String),
    Fatal(String),
}

fn classify(err: cpal::Error) -> OutputError {
    let message = format!("audio output error: {err}");
    match err.kind() {
        cpal::ErrorKind::Xrun
        | cpal::ErrorKind::DeviceChanged
        | cpal::ErrorKind::RealtimeDenied => OutputError::Recovered(message),
        _ => OutputError::Fatal(message),
    }
}

/// One stream configuration to attempt, sample format included.
///
/// The format has to travel with the rate and channel count because it is the
/// part a device is most likely to refuse: a Windows endpoint set to "24 bit,
/// 48000 Hz" accepts nothing but I24, whatever rate is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Candidate {
    rate: u32,
    channels: u16,
    format: SampleFormat,
}

impl std::fmt::Display for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} Hz {}ch {:?}", self.rate, self.channels, self.format)
    }
}

/// Configurations to try, best first.
///
/// Every entry is *attempted*, never pre-judged, because a device's advertised
/// support cannot be trusted. cpal's WASAPI backend enumerates output formats
/// without probing any of them — it assumes `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`
/// makes `Initialize` accept anything — and then refuses, at build time, every
/// format `IsFormatSupported` does not answer `S_OK` for. WASAPI answers `S_OK`
/// only for the endpoint's exact shared-mode mix format, so asking a 48 kHz
/// device for the 44.1 kHz a track actually carries fails with "Stream
/// configuration is not supported in shared mode" — after enumeration promised
/// it would work.
///
/// The device default is always in the list: on WASAPI it is `GetMixFormat`,
/// the one configuration guaranteed to open.
fn candidates(
    rate: u32,
    channels: usize,
    default: &SupportedStreamConfig,
    supported: &[SupportedStreamConfigRange],
) -> Vec<Candidate> {
    let rate = rate.max(1);
    let requested_channels = channels.clamp(1, u16::MAX as usize) as u16;

    let mut out = vec![
        Candidate {
            rate,
            channels: requested_channels,
            format: SampleFormat::F32,
        },
        Candidate {
            rate,
            channels: default.channels(),
            format: default.sample_format(),
        },
        Candidate {
            rate: default.sample_rate(),
            channels: default.channels(),
            format: default.sample_format(),
        },
        Candidate {
            rate: default.sample_rate(),
            channels: default.channels(),
            format: SampleFormat::F32,
        },
    ];

    for range in supported {
        out.push(Candidate {
            rate: rate.clamp(range.min_sample_rate(), range.max_sample_rate()),
            channels: range.channels(),
            format: range.sample_format(),
        });
    }

    let mut seen = std::collections::HashSet::new();
    out.retain(|c| c.rate > 0 && c.channels > 0 && seen.insert(*c));
    out
}

/// Largest magnitude safe to hand to a sample conversion.
///
/// dasp's float-to-integer conversions are documented as assuming
/// `-1.0 <= s < 1.0`, and the 24-bit one builds its result with
/// `I24::new_unchecked` — so a sample at or above +1.0 produces a value outside
/// the 24-bit range that comes back out as wraparound noise rather than as
/// clipping. A loud master, and the resampler's own sinc overshoot, both reach
/// there routinely. `1.0 - EPSILON` is the largest float that scales inside
/// every integer width; -1.0 is exactly representable and needs no headroom.
const PEAK: f32 = 1.0 - f32::EPSILON;

/// Build a stream that converts the ring's f32 samples to `T` on the way out.
///
/// `T` is whatever the device wants; the decoder and resampler only ever deal
/// in f32, so this is the single place the two meet.
fn build_stream<T>(
    device: &Device,
    config: &StreamConfig,
    ring: Arc<SampleRing>,
    clock: Arc<PlaybackClock>,
    tap: Arc<PlaybackTap>,
    volume: Arc<Mutex<f32>>,
    on_error: Arc<dyn Fn(OutputError) + Send + Sync>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let channels = (config.channels as usize).max(1);
    let mut scratch: Vec<f32> = Vec::new();

    device.build_output_stream(
        *config,
        move |out: &mut [T], _info| {
            let filled = std::panic::catch_unwind(AssertUnwindSafe(|| {
                if scratch.len() < out.len() {
                    scratch.resize(out.len(), 0.0);
                }
                let buf = &mut scratch[..out.len()];
                let filled = ring.drain_into(buf);
                let gain = volume.try_lock().map(|v| *v).unwrap_or(1.0);
                for (slot, sample) in out.iter_mut().zip(buf.iter()) {
                    *slot = T::from_sample((*sample * gain).clamp(-1.0, PEAK));
                }
                // What the device is about to hear, which is what the
                // visualiser should be drawing.
                tap.offer(&buf[..filled]);
                filled
            }))
            .unwrap_or_else(|_| {
                out.fill(T::EQUILIBRIUM);
                0
            });
            clock.advance((filled / channels) as u64);
        },
        move |err: cpal::Error| on_error(classify(err)),
        None,
    )
}

/// Dispatch `build_stream` on a sample format only known at runtime.
fn build_for(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    ring: Arc<SampleRing>,
    clock: Arc<PlaybackClock>,
    tap: Arc<PlaybackTap>,
    volume: Arc<Mutex<f32>>,
    on_error: Arc<dyn Fn(OutputError) + Send + Sync>,
) -> Result<Stream, String> {
    macro_rules! build {
        ($t:ty) => {
            build_stream::<$t>(device, config, ring, clock, tap, volume, on_error)
                .map_err(|e| e.to_string())
        };
    }

    match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I24 => build!(cpal::I24),
        SampleFormat::I32 => build!(i32),
        SampleFormat::F64 => build!(f64),
        SampleFormat::U8 => build!(u8),
        SampleFormat::I8 => build!(i8),
        SampleFormat::U16 => build!(u16),
        other => Err(format!("sample format {other:?} is not supported")),
    }
}

fn describe(name: &str, config: &StreamConfig, format: SampleFormat) -> String {
    format!(
        "{name} @ {} Hz {}ch {format:?}",
        config.sample_rate, config.channels
    )
}

/// Name this process's audio stream in the desktop's volume mixer.
///
/// Must run before the process spawns a thread: `set_var` races a concurrent
/// `getenv`, and cpal builds the PipeWire context from the audio thread. Only
/// the PipeWire host reads this — see `host` for what the PulseAudio fallback
/// reports instead.
///
/// `application.icon-name` has to match the icon the running package installed,
/// which is the app id under Flatpak and the binary name everywhere else. An
/// existing `PIPEWIRE_PROPS` is left alone so it stays overridable.
pub fn init_stream_identity() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("PIPEWIRE_PROPS").is_some() {
            return;
        }
        // The app id and the icon name are not the same string outside Flatpak.
        // `application.id` is looked up as a desktop-entry id, and the bundler
        // names that file from `productName` — `MediaHarbor.desktop`. The icon
        // is looked up as a filename, and every packaging path installs
        // `mediaharbor.png`. Under Flatpak both are the app id, so one value
        // serves.
        let (app_id, icon) = match std::env::var("FLATPAK_ID") {
            Ok(id) => (id.clone(), id),
            Err(_) => ("MediaHarbor".to_string(), "mediaharbor".to_string()),
        };
        std::env::set_var("PIPEWIRE_PROPS", stream_identity_props(&app_id, &icon));
    }
}

#[cfg(target_os = "linux")]
fn stream_identity_props(app_id: &str, icon: &str) -> String {
    format!(
        "{{ application.name = \"MediaHarbor\" \
         application.id = \"{app_id}\" \
         application.icon-name = \"{icon}\" \
         node.name = \"MediaHarbor\" \
         media.name = \"Playback\" \
         media.role = \"Music\" }}"
    )
}

/// The cpal host to open devices on.
///
/// `default_host()` prefers PipeWire, then PulseAudio, then ALSA — all three are
/// compiled in (see the cpal features in `Cargo.toml`), so a desktop running a
/// sound server no longer reaches the hardware through the ALSA compat plugin,
/// which costs a buffer of latency and reports generic `default`/`pulse` device
/// names. `MEDIAHARBOR_AUDIO_HOST` forces one by name (`pipewire`,
/// `pulseaudio`, `alsa`) for stacks where the preferred host misbehaves; an
/// unmatched name falls back to the default, and `host_name` reports what
/// actually resolved.
///
/// Only the PipeWire host can name the stream in the volume mixer: it picks up
/// the `PIPEWIRE_PROPS` set at the top of the app's `main`. cpal hardcodes its
/// PulseAudio client name to `cpal-pulseaudio-<pid>` with no icon and offers no
/// way to override it, so falling back to that host also gives up the mixer
/// entry's identity.
fn host() -> cpal::Host {
    let Ok(want) = std::env::var("MEDIAHARBOR_AUDIO_HOST") else {
        return cpal::default_host();
    };
    let want = want.trim();
    cpal::available_hosts()
        .into_iter()
        .find(|id| id.name().eq_ignore_ascii_case(want))
        .and_then(|id| cpal::host_from_id(id).ok())
        .unwrap_or_else(cpal::default_host)
}

fn device_name(device: &Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unnamed device".to_string())
}

/// An open output stream. Dropping this closes the device.
///
/// The most recent samples actually handed to the device.
///
/// The visualiser used to be fed from the decode thread, on the buffer about to
/// enter the ring. That reads whatever is being *decoded*, not what is being
/// *played*: it runs ahead by the ring depth, and — worse — it stops entirely
/// whenever the decode thread blocks. A live radio stream decodes a couple of
/// seconds in a burst and then waits on the network, so the bars froze and
/// lurched instead of moving with the music.
///
/// Filling this from the audio callback ties the visualiser to the device
/// clock, which is the only cadence that matches what is audible. The callback
/// is realtime, so it only ever `try_lock`s and never grows the buffer — a
/// missed frame is a dropped visualiser update, never an audio glitch.
pub struct PlaybackTap {
    samples: Mutex<Vec<f32>>,
    /// Only filled while something is actually drawing the spectrum.
    enabled: AtomicBool,
}

impl PlaybackTap {
    /// Enough for a generous device buffer at 8 channels; anything longer is
    /// truncated rather than reallocated on the realtime thread.
    const CAPACITY: usize = 16 * 1024;

    fn new() -> Arc<Self> {
        Arc::new(Self {
            samples: Mutex::new(Vec::with_capacity(Self::CAPACITY)),
            enabled: AtomicBool::new(false),
        })
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Release);
        if !on {
            if let Ok(mut buf) = self.samples.try_lock() {
                buf.clear();
            }
        }
    }

    /// Called from the audio callback. Never blocks, never allocates.
    fn offer(&self, played: &[f32]) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut buf) = self.samples.try_lock() {
            let take = played.len().min(Self::CAPACITY);
            buf.clear();
            buf.extend_from_slice(&played[played.len() - take..]);
        }
    }

    /// Take whatever has been played since the last call.
    ///
    /// The swap hands the tap the caller's buffer, which starts empty — so without the
    /// `reserve` the next `offer` would grow it, allocating on the audio callback.
    /// Reserving here puts that cost on this thread instead.
    pub fn drain(&self, out: &mut Vec<f32>) {
        out.clear();
        if let Ok(mut buf) = self.samples.try_lock() {
            std::mem::swap(out, &mut *buf);
            buf.clear();
            buf.reserve(Self::CAPACITY);
        }
    }
}

/// Created and dropped only on the `AudioDevice` thread — see the comment on
/// `AudioDevice` for why that matters.
pub struct AudioOutput {
    stream: Stream,
    pub config: StreamConfig,
    pub sample_format: SampleFormat,
    pub device_name: String,
    pub ring: Arc<SampleRing>,
    pub clock: Arc<PlaybackClock>,
    pub tap: Arc<PlaybackTap>,
    volume: Arc<Mutex<f32>>,
}

impl AudioOutput {
    /// Open an output device, preferring `rate`/`channels`.
    ///
    /// Works down `candidates` on the default device and then on every other
    /// output device, so `config` is what the device actually runs at and what
    /// the decode thread's `Resampler` converts every chunk to. Only if every
    /// combination fails does this return an error, and that error names each
    /// one — the diagnostic that makes an unfamiliar audio stack debuggable.
    pub fn open(
        rate: u32,
        channels: usize,
        on_error: Arc<dyn Fn(OutputError) + Send + Sync>,
    ) -> MhResult<Self> {
        let host = host();
        let host_name = host.id().name();

        let mut devices: Vec<Device> = Vec::new();
        if let Some(default) = host.default_output_device() {
            devices.push(default);
        }
        if let Ok(rest) = host.output_devices() {
            devices.extend(rest);
        }

        let mut seen = std::collections::HashSet::new();
        devices.retain(|d| seen.insert(device_name(d)));

        if devices.is_empty() {
            return Err(MhError::Other(
                "no audio output device available".to_string(),
            ));
        }

        let ring = SampleRing::new();
        let tap = PlaybackTap::new();
        let volume = Arc::new(Mutex::new(1.0f32));
        let mut attempts: Vec<String> = Vec::new();

        for device in &devices {
            let name = device_name(device);

            let default_config = match device.default_output_config() {
                Ok(c) => c,
                Err(e) => {
                    attempts.push(format!("{name}: no usable output config ({e})"));
                    continue;
                }
            };
            let supported: Vec<_> = device
                .supported_output_configs()
                .map(|it| it.collect())
                .unwrap_or_default();

            for candidate in candidates(rate, channels, &default_config, &supported) {
                let config = StreamConfig {
                    channels: candidate.channels,
                    sample_rate: candidate.rate,
                    buffer_size: cpal::BufferSize::Default,
                };
                let clock = PlaybackClock::new(candidate.rate);

                match build_for(
                    device,
                    &config,
                    candidate.format,
                    ring.clone(),
                    clock.clone(),
                    tap.clone(),
                    volume.clone(),
                    on_error.clone(),
                ) {
                    Ok(stream) => match stream.play() {
                        Ok(()) => {
                            return Ok(Self {
                                stream,
                                config,
                                sample_format: candidate.format,
                                device_name: name,
                                ring,
                                clock,
                                tap,
                                volume,
                            })
                        }
                        Err(e) => {
                            attempts.push(format!("{name} @ {candidate}: would not start ({e})"))
                        }
                    },
                    Err(e) => attempts.push(format!("{name} @ {candidate}: {e}")),
                }
            }
        }

        Err(MhError::Other(format!(
            "could not open audio output for {rate} Hz {channels}ch on the {host_name} host — tried {}",
            attempts.join("; ")
        )))
    }

    /// How the device ended up running, for the log.
    pub fn describe(&self) -> String {
        describe(&self.device_name, &self.config, self.sample_format)
    }

    pub fn set_volume(&self, v: f32) {
        if let Ok(mut guard) = self.volume.lock() {
            *guard = v.clamp(0.0, 1.0);
        }
    }

    pub fn pause(&self) -> MhResult<()> {
        self.stream
            .pause()
            .map_err(|e| MhError::Other(format!("could not pause audio output: {e}")))
    }

    pub fn play(&self) -> MhResult<()> {
        self.stream
            .play()
            .map_err(|e| MhError::Other(format!("could not resume audio output: {e}")))
    }
}

/// What a caller keeps after a successful open.
///
/// The `Stream` is deliberately absent: it stays on the device thread, so no
/// other thread can create or drop it.
#[derive(Clone)]
pub struct OpenOutput {
    pub config: StreamConfig,
    pub sample_format: SampleFormat,
    pub device_name: String,
    pub ring: Arc<SampleRing>,
    pub clock: Arc<PlaybackClock>,
    pub tap: Arc<PlaybackTap>,
}

impl OpenOutput {
    /// How the device ended up running, for the log.
    pub fn describe(&self) -> String {
        describe(&self.device_name, &self.config, self.sample_format)
    }
}

enum DeviceCommand {
    Open {
        rate: u32,
        channels: usize,
        volume: f32,
        on_error: Arc<dyn Fn(OutputError) + Send + Sync>,
        reply: Sender<MhResult<OpenOutput>>,
    },
    Play(Sender<MhResult<()>>),
    Pause(Sender<MhResult<()>>),
    SetVolume(f32),
    Close,
}

/// Owns every cpal call on one thread that lives as long as the process.
///
/// cpal initialises COM per thread through a thread-local whose `Drop` calls
/// `CoUninitialize`. Opening the device from the per-track decode thread
/// therefore creates every WASAPI object — `IAudioClient`, `IAudioRenderClient`,
/// the endpoint notification client — inside that thread's apartment, and tears
/// the apartment down the moment the track ends, while cpal's own audio thread
/// is still using those interfaces. Dropping the stream from the IPC thread is
/// the mirror image of the same mistake.
///
/// Funnelling every open, pause, resume and drop through one long-lived thread
/// keeps the apartment alive for the whole run, and has the side benefit that
/// the device is no longer torn down and reopened between tracks.
pub struct AudioDevice {
    tx: Mutex<Sender<DeviceCommand>>,
}

impl AudioDevice {
    pub fn new() -> Arc<Self> {
        let (tx, rx) = channel::<DeviceCommand>();

        let _ = std::thread::Builder::new()
            .name("mh-audio-device".into())
            .spawn(move || {
                let mut current: Option<AudioOutput> = None;
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        DeviceCommand::Open {
                            rate,
                            channels,
                            volume,
                            on_error,
                            reply,
                        } => {
                            current = None;
                            let opened = AudioOutput::open(rate, channels, on_error);
                            let _ = reply.send(opened.map(|out| {
                                out.set_volume(volume);
                                let handle = OpenOutput {
                                    config: out.config,
                                    sample_format: out.sample_format,
                                    device_name: out.device_name.clone(),
                                    ring: out.ring.clone(),
                                    clock: out.clock.clone(),
                                    tap: out.tap.clone(),
                                };
                                current = Some(out);
                                handle
                            }));
                        }
                        DeviceCommand::Play(reply) => {
                            let _ = reply.send(match current.as_ref() {
                                Some(out) => out.play(),
                                None => Ok(()),
                            });
                        }
                        DeviceCommand::Pause(reply) => {
                            let _ = reply.send(match current.as_ref() {
                                Some(out) => out.pause(),
                                None => Ok(()),
                            });
                        }
                        DeviceCommand::SetVolume(v) => {
                            if let Some(out) = current.as_ref() {
                                out.set_volume(v);
                            }
                        }
                        DeviceCommand::Close => current = None,
                    }
                }
            });

        Arc::new(Self { tx: Mutex::new(tx) })
    }

    fn send(&self, cmd: DeviceCommand) -> MhResult<()> {
        self.tx
            .lock()
            .map_err(|_| MhError::Other("audio device thread is unavailable".to_string()))?
            .send(cmd)
            .map_err(|_| MhError::Other("audio device thread has stopped".to_string()))
    }

    /// Open the device, blocking until it has succeeded or exhausted every
    /// configuration. Replaces whatever was open.
    pub fn open(
        &self,
        rate: u32,
        channels: usize,
        volume: f32,
        on_error: Arc<dyn Fn(OutputError) + Send + Sync>,
    ) -> MhResult<OpenOutput> {
        let (reply, answer) = channel();
        self.send(DeviceCommand::Open {
            rate,
            channels,
            volume,
            on_error,
            reply,
        })?;
        answer
            .recv()
            .map_err(|_| MhError::Other("audio device thread stopped while opening".to_string()))?
    }

    pub fn play(&self) -> MhResult<()> {
        let (reply, answer) = channel();
        self.send(DeviceCommand::Play(reply))?;
        answer
            .recv()
            .map_err(|_| MhError::Other("audio device thread stopped while resuming".to_string()))?
    }

    pub fn pause(&self) -> MhResult<()> {
        let (reply, answer) = channel();
        self.send(DeviceCommand::Pause(reply))?;
        answer
            .recv()
            .map_err(|_| MhError::Other("audio device thread stopped while pausing".to_string()))?
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.send(DeviceCommand::SetVolume(v));
    }

    pub fn close(&self) {
        let _ = self.send(DeviceCommand::Close);
    }
}

/// Names of the available output devices, for a device picker.
///
/// ALSA advertises the same hardware many times over (one entry per plugin and
/// per sub-device), so identical names are collapsed and the null sink is
/// dropped — otherwise a typical Linux box lists dozens of duplicates. The
/// PipeWire host needs the same treatment for a different reason: it reports
/// its own synthetic entries alongside the real sinks.
pub fn list_output_devices() -> MhResult<Vec<String>> {
    let host = host();
    let devices = host
        .output_devices()
        .map_err(|e| MhError::Other(format!("could not enumerate audio devices: {e}")))?;

    let mut seen = std::collections::HashSet::new();
    Ok(devices
        .filter_map(|d| d.description().ok().map(|desc| desc.name().to_string()))
        .filter(|name| !is_null_sink(name))
        .filter(|name| !is_host_placeholder(name))
        .filter(|name| seen.insert(name.clone()))
        .collect())
}

/// Entries cpal invents rather than reads off the graph, which mean nothing in a
/// picker.
///
/// The PipeWire host publishes `default_sink`/`default_output`/`default_input`
/// as devices that follow whatever the session manager currently defaults to,
/// and falls back to the literal `unknown` for a node that advertises neither a
/// name nor a description. Opening still starts from `default_output_device()`,
/// so dropping these costs nothing — it only keeps raw identifiers out of the
/// list shown to a user.
fn is_host_placeholder(name: &str) -> bool {
    matches!(
        name,
        "default_sink" | "default_source" | "default_output" | "default_input" | "unknown"
    )
}

/// The always-present sink that discards everything, which no picker should offer.
///
/// Each host names it differently: ALSA describes it, PulseAudio reports the
/// sink's own description, and PipeWire's pulse server falls back to the raw
/// `auto_null` sink name when it has no description to give.
fn is_null_sink(name: &str) -> bool {
    name.starts_with("Discard all samples")
        || name.eq_ignore_ascii_case("auto_null")
        || name.eq_ignore_ascii_case("dummy output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_null_sink_is_dropped_whichever_host_named_it() {
        for name in [
            "Discard all samples (playback) or generate zero samples (capture)",
            "auto_null",
            "Dummy Output",
        ] {
            assert!(
                is_null_sink(name),
                "{name:?} is the discard sink — offering it in the picker gives silent playback"
            );
        }
        assert!(!is_null_sink("Built-in Audio Analog Stereo"));
    }

    #[test]
    fn the_pipewire_hosts_synthetic_entries_stay_out_of_the_picker() {
        for name in [
            "default_sink",
            "default_output",
            "default_input",
            "default_source",
            "unknown",
        ] {
            assert!(
                is_host_placeholder(name),
                "{name:?} is cpal's own placeholder, not a sink a user can pick"
            );
        }
        assert!(!is_host_placeholder("Easy Effects Sink"));
        assert!(!is_host_placeholder("Stereo Swapped"));
    }

    /// Outside Flatpak the two identifiers differ, and using one for both
    /// breaks whichever lookup it does not match: `MediaHarbor.desktop` is what
    /// the bundler installs, `mediaharbor.png` is what every path installs as
    /// the icon.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_desktop_id_and_the_icon_name_are_not_the_same_string() {
        let props = stream_identity_props("MediaHarbor", "mediaharbor");
        assert!(
            props.contains("application.id = \"MediaHarbor\""),
            "the desktop id must match MediaHarbor.desktop: {props:?}"
        );
        assert!(
            props.contains("application.icon-name = \"mediaharbor\""),
            "the icon name must match mediaharbor.png: {props:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_stream_property_is_separated_from_the_next() {
        // A `\`-continued literal swallows the following line's indentation, so
        // a missing trailing space silently welds two properties together and
        // PipeWire parses neither.
        let props =
            stream_identity_props("org.mediaharbor.MediaHarbor", "org.mediaharbor.MediaHarbor");
        for pair in [
            ("application.name", "MediaHarbor"),
            ("application.id", "org.mediaharbor.MediaHarbor"),
            ("application.icon-name", "org.mediaharbor.MediaHarbor"),
            ("node.name", "MediaHarbor"),
            ("media.name", "Playback"),
            ("media.role", "Music"),
        ] {
            let (key, value) = pair;
            assert!(
                props.contains(&format!("{key} = \"{value}\"")),
                "{key} is missing or malformed in {props:?}"
            );
        }
        assert!(
            props.starts_with("{ ") && props.ends_with(" }"),
            "{props:?}"
        );
        assert!(
            !props.contains("\"application"),
            "two properties are welded together: {props:?}"
        );
    }

    #[test]
    fn an_xrun_does_not_stop_playback() {
        for kind in [
            cpal::ErrorKind::Xrun,
            cpal::ErrorKind::DeviceChanged,
            cpal::ErrorKind::RealtimeDenied,
        ] {
            assert!(
                matches!(classify(kind.into()), OutputError::Recovered(_)),
                "{kind:?} is recoverable — reporting it as fatal stops audio the device can still play"
            );
        }
    }

    #[test]
    fn a_lost_device_is_fatal() {
        for kind in [
            cpal::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::StreamInvalidated,
        ] {
            assert!(
                matches!(classify(kind.into()), OutputError::Fatal(_)),
                "{kind:?}"
            );
        }
    }

    fn default_config(rate: u32, channels: u16, format: SampleFormat) -> SupportedStreamConfig {
        SupportedStreamConfig::new(
            channels,
            rate,
            cpal::SupportedBufferSize::Range { min: 64, max: 4096 },
            format,
        )
    }

    /// The device default must always be reachable: on WASAPI it is the mix
    /// format, the one configuration guaranteed to open.
    #[test]
    fn the_device_default_is_always_a_candidate() {
        let default = default_config(48_000, 2, SampleFormat::F32);
        let list = candidates(44_100, 2, &default, &[]);

        assert!(
            list.contains(&Candidate {
                rate: 48_000,
                channels: 2,
                format: SampleFormat::F32,
            }),
            "a 44.1 kHz track on a 48 kHz device must still be able to fall back: {list:?}"
        );
    }

    /// The source's own format is tried first, so the resampler stays a
    /// pass-through whenever the device will take it.
    #[test]
    fn the_source_format_is_tried_first() {
        let default = default_config(48_000, 2, SampleFormat::F32);
        let list = candidates(44_100, 2, &default, &[]);

        assert_eq!(
            list[0],
            Candidate {
                rate: 44_100,
                channels: 2,
                format: SampleFormat::F32,
            }
        );
    }

    /// A 16- or 24-bit endpoint accepts nothing else. Only ever offering F32 is
    /// what makes every single track fail on such a device.
    #[test]
    fn a_non_float_device_gets_its_own_sample_format_offered() {
        let default = default_config(48_000, 2, SampleFormat::I24);
        let list = candidates(44_100, 2, &default, &[]);

        assert!(
            list.iter().any(|c| c.format == SampleFormat::I24),
            "an I24 device was never offered I24: {list:?}"
        );
    }

    #[test]
    fn supported_ranges_are_swept_at_a_rate_they_accept() {
        let default = default_config(48_000, 2, SampleFormat::F32);
        let dmix = SupportedStreamConfigRange::new(
            2,
            48_000,
            48_000,
            cpal::SupportedBufferSize::Range { min: 64, max: 4096 },
            SampleFormat::I16,
        );

        let list = candidates(44_100, 2, &default, &[dmix]);

        assert!(
            list.contains(&Candidate {
                rate: 48_000,
                channels: 2,
                format: SampleFormat::I16,
            }),
            "an S16-only ALSA default was never offered S16: {list:?}"
        );
    }

    #[test]
    fn candidates_are_deduplicated() {
        let default = default_config(44_100, 2, SampleFormat::F32);
        let list = candidates(44_100, 2, &default, &[]);

        let mut unique = list.clone();
        unique.dedup();
        assert_eq!(list.len(), unique.len());
        assert_eq!(
            list.len(),
            1,
            "a device already running at the source format needs exactly one candidate: {list:?}"
        );
    }

    #[test]
    fn a_zero_rate_source_still_produces_openable_candidates() {
        let default = default_config(48_000, 2, SampleFormat::F32);
        let list = candidates(0, 0, &default, &[]);

        assert!(!list.is_empty());
        assert!(list.iter().all(|c| c.rate > 0 && c.channels > 0));
    }

    /// The conversion the audio callback performs, isolated from cpal.
    fn convert<T: SizedSample + FromSample<f32>>(sample: f32, gain: f32) -> T {
        T::from_sample((sample * gain).clamp(-1.0, PEAK))
    }

    /// Every integer width must clip rather than wrap. I24 is the dangerous one:
    /// dasp builds it with `new_unchecked`, so an out-of-range value is not
    /// caught and reappears as noise at the opposite polarity.
    #[test]
    fn a_hot_sample_clips_instead_of_wrapping() {
        for sample in [1.0f32, 1.5, 3.0, f32::MAX] {
            assert!(
                convert::<i16>(sample, 1.0) > 32_000,
                "i16 wrapped at {sample}"
            );
            assert!(
                convert::<cpal::I24>(sample, 1.0).inner() > 8_000_000,
                "i24 wrapped at {sample}"
            );
            assert!(convert::<i32>(sample, 1.0) > 2_000_000_000, "i32 wrapped");
        }

        for sample in [-1.0f32, -1.5, -3.0, f32::MIN] {
            assert!(convert::<i16>(sample, 1.0) < -32_000, "i16 wrapped");
            assert!(
                convert::<cpal::I24>(sample, 1.0).inner() < -8_000_000,
                "i24 wrapped at {sample}"
            );
        }
    }

    /// An I24 has to stay inside 24 signed bits; cpal packs only three bytes of
    /// it, so anything wider is silently truncated into a different sample.
    #[test]
    fn i24_conversion_stays_in_range() {
        for sample in [-1.0f32, -0.5, 0.0, 0.5, 1.0, 9.0] {
            let v = convert::<cpal::I24>(sample, 1.0).inner();
            assert!(
                (-8_388_608..=8_388_607).contains(&v),
                "{sample} produced {v}, outside 24 bits"
            );
        }
    }

    #[test]
    fn volume_scales_before_conversion() {
        assert_eq!(convert::<i16>(1.0, 0.0), 0);
        assert!(convert::<i16>(1.0, 0.5) > 16_000);
        assert!(convert::<i16>(1.0, 0.5) < 16_500);
    }

    #[test]
    fn drain_yields_silence_when_empty() {
        let ring = SampleRing::new();
        let mut out = [1.0f32; 8];
        assert_eq!(ring.drain_into(&mut out), 0);
        assert!(out.iter().all(|s| *s == 0.0), "underrun must be silence");
    }

    /// Fill past the prebuffer threshold so `drain_into` starts serving audio.
    fn primed_ring(head: &[f32]) -> Arc<SampleRing> {
        let ring = SampleRing::new();
        let gen = ring.generation();
        ring.push(head, gen);
        ring.push(&vec![0.0; PREBUFFER], gen);
        ring
    }

    #[test]
    fn samples_come_back_in_order() {
        let ring = primed_ring(&[1.0, 2.0, 3.0, 4.0]);

        let mut out = [0.0f32; 4];
        assert_eq!(ring.drain_into(&mut out), 4);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn output_stays_silent_until_prebuffered() {
        let ring = SampleRing::new();
        ring.push(&[1.0; 64], ring.generation());

        let mut out = [9.0f32; 8];
        assert_eq!(ring.drain_into(&mut out), 0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_finished_track_plays_out_even_below_the_threshold() {
        let ring = SampleRing::new();
        ring.push(&[0.25; 4], ring.generation());
        ring.mark_finished();

        let mut out = [0.0f32; 4];
        assert_eq!(ring.drain_into(&mut out), 4);
    }

    #[test]
    fn partial_reads_are_padded_with_silence() {
        let ring = primed_ring(&[0.5, 0.5]);

        let mut out = [9.0f32; 4];
        assert_eq!(ring.drain_into(&mut out), 4);
        assert_eq!(&out[..2], &[0.5, 0.5]);
    }

    #[test]
    fn flush_discards_buffered_audio() {
        let ring = SampleRing::new();
        ring.push(&[1.0; 16], ring.generation());
        ring.flush();

        let mut out = [7.0f32; 4];
        assert_eq!(ring.drain_into(&mut out), 0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_push_from_before_a_flush_is_rejected() {
        let ring = SampleRing::new();
        let stale = ring.generation();
        ring.flush();
        assert!(!ring.push(&[1.0, 2.0], stale));

        let mut out = [0.0f32; 2];
        assert_eq!(ring.drain_into(&mut out), 0);
    }

    #[test]
    fn drained_only_once_finished_and_empty() {
        let ring = SampleRing::new();
        ring.push(&[1.0, 2.0], ring.generation());
        assert!(!ring.is_drained(), "still has audio queued");

        ring.mark_finished();
        assert!(!ring.is_drained(), "finished but not yet played out");

        let mut out = [0.0f32; 2];
        while !ring.is_drained() {
            ring.drain_into(&mut out);
        }
        assert!(ring.is_drained());
    }

    /// Largest sample-to-sample jump in a buffer; a click is a large step.
    fn max_step(prev: f32, xs: &[f32]) -> f32 {
        let mut last = prev;
        let mut worst = 0.0f32;
        for x in xs {
            worst = worst.max((x - last).abs());
            last = *x;
        }
        worst
    }

    #[test]
    fn a_seek_does_not_step_the_waveform() {
        let ring = SampleRing::new();
        let gen = ring.generation();
        ring.push(&vec![0.9f32; PREBUFFER + 4096], gen);

        let mut out = vec![0.0f32; 1024];
        ring.drain_into(&mut out);
        let last = *out.last().unwrap();

        ring.flush();
        let gen = ring.generation();
        ring.push(&vec![-0.9f32; PREBUFFER + 4096], gen);

        let mut after = vec![0.0f32; 1024];
        ring.drain_into(&mut after);

        let step = max_step(last, &after);
        assert!(
            step < 0.05,
            "seek produced a {step:.3} discontinuity — audible as a click"
        );
    }
}
