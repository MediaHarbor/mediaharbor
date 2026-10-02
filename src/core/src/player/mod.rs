pub mod clock;
pub mod crossfade;
pub mod decode;
pub mod output;
pub mod resample;
pub mod source;
pub mod spectrum;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::Client;
use tokio::runtime::Handle;

use crate::errors::{MhError, MhResult};
use crate::ipc_contract::UndecodableStream;

use decode::StreamDecoder;
use output::{AudioDevice, OpenOutput};
use resample::Resampler;
use source::MhMediaSource;
use spectrum::SpectrumAnalyser;

/// Playback state snapshot pushed to the frontend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerState {
    pub playing: bool,
    pub ended: bool,
    pub buffering: bool,
}

/// What the player reports back to the host application.
///
/// Kept as plain callbacks, not `EventEmitter` calls, so `NativePlayer` stays
/// usable on its own — `examples/probe_player.rs` drives it with nothing else
/// wired up. `PlayerEventBridge` in `lib.rs` is what connects these to IPC.
///
/// Two of these have no `EventEmitter` counterpart: `warning` flattens into the
/// generic log channel, and `opened` reaches the log file only.
pub trait PlayerObserver: Send + Sync {
    fn position(&self, position_secs: f64, duration_secs: Option<f64>);
    fn state(&self, state: PlayerState);
    /// A decode or device failure. Playback that goes silent without one of
    /// these is the bug this player exists to eliminate.
    fn error(&self, message: String);
    /// The stream opened but nothing here can decode it. Carries what the
    /// decoder found so the frontend can offer a report worth filing.
    fn undecodable(&self, report: UndecodableStream) {
        self.error(format!("Can't decode: {}", report.detail));
    }
    /// A glitch the device recovered from. Playback continues, so this must not
    /// reach the frontend as a playback failure.
    fn warning(&self, _message: String) {}
    /// The device and format playback actually negotiated. Worth recording:
    /// which of the candidate configurations won is the first thing anyone
    /// needs to know when audio misbehaves on an unfamiliar machine.
    fn opened(&self, _description: String) {}
    fn spectrum(&self, _bars: Vec<u8>) {}
    /// The queued track has taken over from the one that was playing.
    ///
    /// Fires when the handover actually completes, which is the only moment at
    /// which advertising the new track is truthful — `crossfade_to` returns as
    /// soon as the command is sent, a whole fade earlier.
    fn track_changed(&self, _at_secs: f64) {}
}

/// Commands handed to the decode thread.
enum Command {
    Seek(f64),
    Stop,
    /// Begin mixing into `url` over `duration_secs`, then continue on it.
    CrossfadeTo {
        url: String,
        mime: Option<String>,
        duration_secs: f64,
    },
}

struct Shared {
    playing: AtomicBool,
    /// Set once the decode thread has probed the stream, so `load` can report
    /// duration and surface open failures to its caller.
    ready: (Mutex<Option<Result<(), String>>>, std::sync::Condvar),
    /// Bumped whenever a new track is loaded, so an old decode thread exits.
    epoch: AtomicU64,
    spectrum_on: AtomicBool,
    duration: Mutex<Option<f64>>,
}

pub struct NativePlayer {
    client: Client,
    handle: Handle,
    observer: Arc<dyn PlayerObserver>,
    shared: Arc<Shared>,
    /// The one thread allowed to touch cpal. See `output::AudioDevice`.
    device: Arc<AudioDevice>,
    /// Ring, clock and negotiated format of whatever is currently open.
    active: Arc<Mutex<Option<OpenOutput>>>,
    commands: Mutex<Option<std::sync::mpsc::Sender<Command>>>,
    volume: Mutex<f32>,
    track_gain: Mutex<f32>,
    muted: AtomicBool,
}

/// Unblock `load`, reporting whether the stream opened.
/// Pass a failure to open a stream to the observer, keeping an undecodable
/// stream's details instead of flattening them into a message.
fn report_open_failure(observer: &dyn PlayerObserver, err: &MhError) {
    match err {
        MhError::Undecodable(report) => observer.undecodable((**report).clone()),
        other => observer.error(other.to_string()),
    }
}

fn signal_ready(shared: &Shared, result: Result<(), String>) {
    let (lock, cvar) = &shared.ready;
    *lock.lock().unwrap() = Some(result);
    cvar.notify_all();
}

/// How far short of the container's duration counts as a broken stream rather
/// than the end of the track.
const PREMATURE_END_SECS: f64 = 5.0;

const STALL_POLL: std::time::Duration = std::time::Duration::from_millis(120);

/// Report buffering while the device runs dry.
///
/// This cannot live on the decode thread: the whole point is to say something
/// while that thread is blocked inside a stalled read, so it runs beside it and
/// watches the ring the audio callback is draining.
fn spawn_stall_watchdog(
    ring: Arc<output::SampleRing>,
    shared: Arc<Shared>,
    observer: Arc<dyn PlayerObserver>,
    epoch: u64,
) {
    let _ = std::thread::Builder::new()
        .name("mh-audio-stall".into())
        .spawn(move || {
            let mut streak = 0u32;
            let mut reported = false;
            loop {
                std::thread::sleep(STALL_POLL);
                if shared.epoch.load(Ordering::Acquire) != epoch {
                    return;
                }
                if !shared.playing.load(Ordering::Acquire) {
                    streak = 0;
                    reported = false;
                    continue;
                }

                streak = if ring.is_starving() { streak + 1 } else { 0 };
                let stalled = streak >= 2;
                if stalled != reported {
                    reported = stalled;
                    observer.state(PlayerState {
                        playing: true,
                        ended: false,
                        buffering: stalled,
                    });
                }
            }
        });
}

/// Emit spectrum frames at a steady cadence from what the device is playing.
///
/// This cannot live on the decode thread. That thread runs in bursts — it
/// decodes whatever a network read delivered and then blocks for the next one —
/// so a live stream produced a flurry of frames followed by seconds of silence.
/// Reading the playback tap instead ties the bars to the device clock, which is
/// the only thing that matches what is audible.
fn spawn_spectrum(
    tap: Arc<output::PlaybackTap>,
    shared: Arc<Shared>,
    observer: Arc<dyn PlayerObserver>,
    epoch: u64,
    channels: usize,
) {
    let _ = std::thread::Builder::new()
        .name("mh-audio-spectrum".into())
        .spawn(move || {
            let mut analyser = SpectrumAnalyser::new();
            let mut played: Vec<f32> = Vec::new();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(33));
                if shared.epoch.load(Ordering::Acquire) != epoch {
                    return;
                }
                if !shared.spectrum_on.load(Ordering::Acquire) {
                    continue;
                }
                tap.drain(&mut played);
                if played.is_empty() {
                    continue;
                }
                analyser.push(&played, channels);
                observer.spectrum(analyser.bars());
            }
        });
}

impl NativePlayer {
    pub fn new(client: Client, handle: Handle, observer: Arc<dyn PlayerObserver>) -> Arc<Self> {
        Arc::new(Self {
            client,
            handle,
            observer,
            shared: Arc::new(Shared {
                playing: AtomicBool::new(false),
                ready: (Mutex::new(None), std::sync::Condvar::new()),
                epoch: AtomicU64::new(0),
                spectrum_on: AtomicBool::new(false),
                duration: Mutex::new(None),
            }),
            device: AudioDevice::new(),
            active: Arc::new(Mutex::new(None)),
            commands: Mutex::new(None),
            volume: Mutex::new(1.0),
            track_gain: Mutex::new(1.0),
            muted: AtomicBool::new(false),
        })
    }

    /// Begin playing `url`, replacing whatever was playing.
    pub fn load(self: &Arc<Self>, url: &str, mime: Option<&str>) -> MhResult<()> {
        self.stop()?;

        *self.shared.ready.0.lock().unwrap() = None;
        let epoch = self.shared.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let (tx, rx) = std::sync::mpsc::channel();
        *self.commands.lock().unwrap() = Some(tx);

        let observer = self.observer.clone();
        let err_observer = self.observer.clone();
        let shared = self.shared.clone();
        let client = self.client.clone();
        let handle = self.handle.clone();
        let client_for_fade = self.client.clone();
        let handle_for_fade = self.handle.clone();
        let url = url.to_string();
        let mime = mime.map(str::to_string);
        let volume = self.effective_volume();
        let device = self.device.clone();
        let active_for_thread = self.active.clone();

        self.shared.playing.store(true, Ordering::Release);
        observer.state(PlayerState {
            playing: true,
            ..Default::default()
        });

        std::thread::Builder::new()
            .name("mh-audio-decode".into())
            .stack_size(4 * 1024 * 1024)
            .spawn(move || {
                let opened = MhMediaSource::open(client, handle, &url)
                    .map_err(|e| MhError::Other(format!("could not open stream: {e}")))
                    .and_then(|src| StreamDecoder::new(Box::new(src), mime.as_deref(), Some(&url)));

                let mut decoder = match opened {
                    Ok(d) => d,
                    Err(e) => {
                        report_open_failure(&*observer, &e);
                        shared.playing.store(false, Ordering::Release);
                        observer.state(PlayerState::default());
                        signal_ready(&shared, Err(e.to_string()));
                        return;
                    }
                };

                let info = decoder.info();
                *shared.duration.lock().unwrap() = info.duration;

                let first = match decoder.next_chunk() {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        observer.error(e.to_string());
                        shared.playing.store(false, Ordering::Release);
                        observer.state(PlayerState::default());
                        signal_ready(&shared, Err(e.to_string()));
                        return;
                    }
                };
                let (rate, channels) = match &first {
                    Some(chunk) => (chunk.rate, chunk.channels),
                    None => (info.rate, info.channels),
                };

                let output = match device.open(
                    rate,
                    channels,
                    volume,
                    Arc::new(move |e| match e {
                        output::OutputError::Fatal(msg) => err_observer.error(msg),
                        output::OutputError::Recovered(msg) => err_observer.warning(msg),
                    }),
                ) {
                    Ok(o) => o,
                    Err(e) => {
                        observer.error(e.to_string());
                        shared.playing.store(false, Ordering::Release);
                        observer.state(PlayerState::default());
                        signal_ready(&shared, Err(e.to_string()));
                        return;
                    }
                };
                observer.opened(output.describe());

                let ring = output.ring.clone();
                let clock = output.clock.clone();
                let tap_for_thread = output.tap.clone();
                // A device opened mid-session must honour whatever the
                // visualiser asked for before it existed.
                tap_for_thread.set_enabled(shared.spectrum_on.load(Ordering::Acquire));
                let out_rate = output.config.sample_rate;
                let out_channels = (output.config.channels as usize).max(1);
                *active_for_thread.lock().unwrap() = Some(output);
                signal_ready(&shared, Ok(()));

                spawn_stall_watchdog(ring.clone(), shared.clone(), observer.clone(), epoch);
                spawn_spectrum(tap_for_thread, shared.clone(), observer.clone(), epoch, out_channels);

                let mut resampler = Resampler::new(out_rate, out_channels);
                let mut incoming_resampler = Resampler::new(out_rate, out_channels);
                let mut ready: Vec<f32> = Vec::new();
                let mut pending_out: Vec<f32> = Vec::new();
                let mut pending_incoming: Vec<f32> = Vec::new();

                let mut generation = ring.generation();
                if let Some(chunk) = first {
                    let converted = resampler.process(&chunk.samples, chunk.rate, chunk.channels);
                    if !ring.push(converted, generation) {
                        generation = ring.generation();
                    }
                }

                let mut incoming: Option<StreamDecoder> = None;
                let mut fade_frames_total = 0usize;
                let mut fade_frames_done = 0usize;
                let mut last_emit = std::time::Instant::now();

                loop {
                    if shared.epoch.load(Ordering::Acquire) != epoch {
                        return;
                    }

                    match rx.try_recv() {
                        Ok(Command::Stop) => break,
                        Ok(Command::Seek(secs)) => {
                            ring.flush();
                            generation = ring.generation();
                            resampler.reset();
                            incoming_resampler.reset();
                            pending_out.clear();
                            pending_incoming.clear();
                            match decoder.seek(secs) {
                                Ok(actual) => clock.reset_to(actual),
                                Err(e) => observer.error(e.to_string()),
                            }
                            continue;
                        }
                        Ok(Command::CrossfadeTo {
                            url,
                            mime,
                            duration_secs,
                        }) => {
                            match MhMediaSource::open(client_for_fade.clone(), handle_for_fade.clone(), &url)
                                .map_err(|e| MhError::Other(format!("could not open stream: {e}")))
                                .and_then(|src| StreamDecoder::new(Box::new(src), mime.as_deref(), Some(&url)))
                            {
                                Ok(next) => {
                                    fade_frames_total =
                                        (duration_secs.max(0.1) * f64::from(out_rate)) as usize;
                                    fade_frames_done = 0;
                                    incoming_resampler.reset();
                                    pending_incoming.clear();
                                    incoming = Some(next);
                                }
                                Err(e) => report_open_failure(&*observer, &e),
                            }
                            continue;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    }

                    if !shared.playing.load(Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }

                    match decoder.next_chunk() {
                        Ok(Some(chunk)) => {
                            pending_out.extend_from_slice(resampler.process(
                                &chunk.samples,
                                chunk.rate,
                                chunk.channels,
                            ));
                            decoder.recycle(chunk.samples);

                            while incoming.is_some() && pending_incoming.len() < pending_out.len() {
                                let decoded =
                                    incoming.as_mut().expect("incoming present").next_chunk();
                                match decoded {
                                    Ok(Some(other)) => {
                                        pending_incoming.extend_from_slice(
                                            incoming_resampler.process(
                                                &other.samples,
                                                other.rate,
                                                other.channels,
                                            ),
                                        );
                                        if let Some(inc) = incoming.as_mut() {
                                            inc.recycle(other.samples);
                                        }
                                    }
                                    Ok(None) => incoming = None,
                                    Err(e) => {
                                        observer.error(e.to_string());
                                        incoming = None;
                                    }
                                }
                            }

                            ready.clear();
                            if incoming.is_some() {
                                let frames = crossfade::mix_frames(
                                    &mut ready,
                                    &pending_out,
                                    &pending_incoming,
                                    out_channels,
                                    fade_frames_done,
                                    fade_frames_total,
                                );

                                pending_out.drain(..frames * out_channels);
                                pending_incoming.drain(..frames * out_channels);
                                fade_frames_done += frames;

                                if fade_frames_done >= fade_frames_total {
                                    pending_out.clear();
                                    std::mem::swap(&mut pending_out, &mut pending_incoming);
                                    decoder = incoming.take().expect("incoming present");
                                    resampler = std::mem::replace(
                                        &mut incoming_resampler,
                                        Resampler::new(out_rate, out_channels),
                                    );
                                    *shared.duration.lock().unwrap() = decoder.info().duration;
                                    observer.track_changed(clock.position());
                                    clock.reset_to(0.0);
                                    fade_frames_done = 0;
                                    fade_frames_total = 0;
                                    observer.state(PlayerState {
                                        playing: true,
                                        ..Default::default()
                                    });
                                }
                            } else {
                                std::mem::swap(&mut ready, &mut pending_out);
                            }

                            if !ring.push(&ready, generation) {
                                generation = ring.generation();
                            }
                        }
                        Ok(None) if incoming.is_some() => {
                            decoder = incoming.take().expect("incoming present");
                            resampler = std::mem::replace(
                                &mut incoming_resampler,
                                Resampler::new(out_rate, out_channels),
                            );
                            pending_out.clear();
                            std::mem::swap(&mut pending_out, &mut pending_incoming);
                            *shared.duration.lock().unwrap() = decoder.info().duration;
                            observer.track_changed(clock.position());
                            clock.reset_to(0.0);
                            fade_frames_done = 0;
                            fade_frames_total = 0;
                            continue;
                        }
                        Ok(None) => {
                            ring.mark_finished();
                            while !ring.is_drained() {
                                if shared.epoch.load(Ordering::Acquire) != epoch {
                                    return;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                            let position = clock.position();
                            let duration = *shared.duration.lock().unwrap();
                            shared.playing.store(false, Ordering::Release);
                            observer.position(position, duration);
                            if let Some(total) = duration {
                                if total - position > PREMATURE_END_SECS {
                                    observer.error(format!(
                                        "stream ended at {position:.0}s of {total:.0}s — the source stopped sending audio"
                                    ));
                                }
                            }
                            observer.state(PlayerState {
                                playing: false,
                                ended: true,
                                buffering: false,
                            });
                            return;
                        }
                        Err(e) => {
                            if shared.epoch.load(Ordering::Acquire) == epoch {
                                observer.error(e.to_string());
                                shared.playing.store(false, Ordering::Release);
                                observer.state(PlayerState::default());
                            }
                            return;
                        }
                    }

                    if last_emit.elapsed().as_millis() >= 100 {
                        observer.position(clock.position(), *shared.duration.lock().unwrap());
                        last_emit = std::time::Instant::now();
                    }
                }
            })
            .map_err(|e| MhError::Other(format!("could not start decode thread: {e}")))?;

        let (lock, cvar) = &self.shared.ready;
        let mut ready = lock.lock().unwrap();
        while ready.is_none() {
            let (guard, timeout) = cvar
                .wait_timeout(ready, std::time::Duration::from_secs(30))
                .unwrap();
            ready = guard;
            if timeout.timed_out() {
                return Err(MhError::Other("timed out opening stream".to_string()));
            }
        }
        match ready.clone() {
            Some(Err(e)) => Err(MhError::Other(e)),
            _ => Ok(()),
        }
    }

    pub fn play(&self) -> MhResult<()> {
        self.shared.playing.store(true, Ordering::Release);
        self.device.play()?;
        self.observer.state(PlayerState {
            playing: true,
            ..Default::default()
        });
        Ok(())
    }

    pub fn pause(&self) -> MhResult<()> {
        self.shared.playing.store(false, Ordering::Release);
        self.device.pause()?;
        self.observer.state(PlayerState::default());
        Ok(())
    }

    pub fn seek(&self, seconds: f64) -> MhResult<()> {
        let target = match self.duration() {
            Some(d) if d > 0.0 => seconds.clamp(0.0, (d - 0.25).max(0.0)),
            _ => seconds.max(0.0),
        };

        let tx = self.commands.lock().unwrap().clone();
        match tx {
            Some(tx) => tx
                .send(Command::Seek(target))
                .map_err(|_| MhError::Other("player is not running".to_string())),
            None => Err(MhError::Other("nothing is loaded".to_string())),
        }
    }

    /// Fade into `url` over `duration_secs` and continue playing it.
    ///
    /// Unlike `load`, this keeps the current output stream and decode thread
    /// running, so the two tracks overlap instead of one replacing the other.
    pub fn crossfade_to(&self, url: &str, mime: Option<&str>, duration_secs: f64) -> MhResult<()> {
        let tx = self.commands.lock().unwrap().clone();
        match tx {
            Some(tx) => tx
                .send(Command::CrossfadeTo {
                    url: url.to_string(),
                    mime: mime.map(str::to_string),
                    duration_secs,
                })
                .map_err(|_| MhError::Other("player is not running".to_string())),
            None => Err(MhError::Other(
                "nothing is playing to fade from".to_string(),
            )),
        }
    }

    pub fn stop(&self) -> MhResult<()> {
        self.shared.epoch.fetch_add(1, Ordering::AcqRel);
        self.shared.playing.store(false, Ordering::Release);
        if let Some(tx) = self.commands.lock().unwrap().take() {
            let _ = tx.send(Command::Stop);
        }
        if let Some(out) = self.active.lock().unwrap().take() {
            out.ring.flush();
        }
        self.device.close();
        Ok(())
    }

    pub fn set_volume(&self, v: f32) {
        *self.volume.lock().unwrap() = v.clamp(0.0, 1.0);
        self.device.set_volume(self.effective_volume());
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Release);
        self.device.set_volume(self.effective_volume());
    }

    /// Spectrum data is only computed while something is rendering it.
    pub fn set_spectrum_enabled(&self, enabled: bool) {
        self.shared.spectrum_on.store(enabled, Ordering::Release);
        if let Some(out) = self.active.lock().unwrap().as_ref() {
            out.tap.set_enabled(enabled);
        }
    }

    pub fn position(&self) -> f64 {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|o| o.clock.position())
            .unwrap_or(0.0)
    }

    pub fn duration(&self) -> Option<f64> {
        *self.shared.duration.lock().unwrap()
    }

    /// User volume and per-track ReplayGain multiply. Keeping them apart means a volume
    /// change does not discard the track's gain, and a track change does not fight the
    /// user's volume.
    fn effective_volume(&self) -> f32 {
        if self.muted.load(Ordering::Acquire) {
            0.0
        } else {
            *self.volume.lock().unwrap() * *self.track_gain.lock().unwrap()
        }
    }

    /// `gain_db` as written by the tagger, with `peak` limiting it so a loud master is
    /// not amplified into clipping — the standard ReplayGain prevent-clipping rule.
    pub fn set_track_gain(&self, gain_db: Option<f32>, peak: Option<f32>) {
        let linear = match gain_db {
            Some(db) if db.is_finite() => {
                let raw = 10f32.powf(db / 20.0);
                match peak.filter(|p| *p > 0.0) {
                    Some(p) => raw.min(1.0 / p),
                    None => raw,
                }
            }
            _ => 1.0,
        };
        *self.track_gain.lock().unwrap() = linear.clamp(0.05, 4.0);
        self.device.set_volume(self.effective_volume());
    }
}
