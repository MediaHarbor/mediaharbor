use std::sync::Mutex;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;

use serde::Deserialize;
use souvlaki::{MediaControlEvent, MediaControls, MediaPosition, PlatformConfig, SeekDirection};
#[cfg(not(target_os = "macos"))]
use souvlaki::{MediaMetadata, MediaPlayback};
use tauri::{AppHandle, Emitter, Manager};

/// On macOS the controls only hold the remote-command handlers; nothing reads them
/// back, because `now_playing` publishes the info.
pub struct MediaState(
    #[cfg_attr(target_os = "macos", allow(dead_code))] pub Mutex<Option<MediaControls>>,
);

/// The "backend-log" topic carries `BackendLogEvent`, not a bare string — the
/// frontend log panel reads the level/source fields off it.
fn log_error(app: &AppHandle, message: String) {
    let _ = app.emit(
        "backend-log",
        mediaharbor_core::ipc_contract::BackendLogEvent::error(
            "media-controls",
            "Media controls",
            message,
        ),
    );
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadataInput {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover_url: Option<String>,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlaybackInput {
    pub playing: bool,
    pub position_secs: Option<f64>,
}

fn event_name(ev: &MediaControlEvent) -> Option<serde_json::Value> {
    let action = match ev {
        MediaControlEvent::Play => "play",
        MediaControlEvent::Pause => "pause",
        MediaControlEvent::Toggle => "toggle",
        MediaControlEvent::Next => "next",
        MediaControlEvent::Previous => "previous",
        MediaControlEvent::Stop => "stop",
        MediaControlEvent::Seek(SeekDirection::Forward) => "seek_forward",
        MediaControlEvent::Seek(SeekDirection::Backward) => "seek_backward",
        MediaControlEvent::SeekBy(dir, dur) => {
            let d = if matches!(dir, SeekDirection::Forward) {
                dur.as_secs_f64()
            } else {
                -dur.as_secs_f64()
            };
            return Some(serde_json::json!({ "action": "seek_by", "seconds": d }));
        }
        MediaControlEvent::SetPosition(MediaPosition(pos)) => {
            return Some(
                serde_json::json!({ "action": "set_position", "seconds": pos.as_secs_f64() }),
            );
        }
        _ => return None,
    };
    Some(serde_json::json!({ "action": action }))
}

pub fn setup(app: &AppHandle) {
    let dbus_name = "org.mediaharbor.MediaHarbor";
    let display_name = "MediaHarbor";

    #[cfg(target_os = "windows")]
    let hwnd = {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        app.get_webview_window("main")
            .and_then(|w| w.window_handle().ok().map(|h| h.as_raw()))
            .and_then(|raw| match raw {
                RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
                _ => None,
            })
    };
    #[cfg(not(target_os = "windows"))]
    let hwnd = None;

    let config = PlatformConfig {
        dbus_name,
        display_name,
        hwnd,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            log_error(app, format!("media controls init failed: {e:?}"));
            return;
        }
    };

    let handle = app.clone();
    let attach = controls.attach(move |event: MediaControlEvent| {
        if let Some(payload) = event_name(&event) {
            let _ = handle.emit("media-control", payload);
        }
    });
    if let Err(e) = attach {
        log_error(app, format!("media controls attach failed: {e:?}"));
        return;
    }

    app.manage(MediaState(Mutex::new(Some(controls))));
}

#[tauri::command]
pub fn media_set_metadata(
    app: AppHandle,
    state: tauri::State<'_, MediaState>,
    meta: MediaMetadataInput,
) -> Result<(), String> {
    set_metadata(&app, &state, meta)
}

#[tauri::command]
pub fn media_set_playback(
    app: AppHandle,
    state: tauri::State<'_, MediaState>,
    playback: MediaPlaybackInput,
) -> Result<(), String> {
    set_playback(&app, &state, playback)
}

#[cfg(not(target_os = "macos"))]
fn set_metadata(
    _app: &AppHandle,
    state: &MediaState,
    meta: MediaMetadataInput,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(controls) = guard.as_mut() {
        controls
            .set_metadata(MediaMetadata {
                title: meta.title.as_deref(),
                artist: meta.artist.as_deref(),
                album: meta.album.as_deref(),
                cover_url: meta.cover_url.as_deref(),
                duration: meta.duration_secs.map(Duration::from_secs_f64),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_playback(
    _app: &AppHandle,
    state: &MediaState,
    playback: MediaPlaybackInput,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(controls) = guard.as_mut() {
        let progress = playback
            .position_secs
            .map(|s| MediaPosition(Duration::from_secs_f64(s.max(0.0))));
        let pb = if playback.playing {
            MediaPlayback::Playing { progress }
        } else {
            MediaPlayback::Paused { progress }
        };
        controls.set_playback(pb).map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

// On macOS souvlaki only carries the remote commands; `now_playing` publishes the
// info itself (see there for why).
#[cfg(target_os = "macos")]
fn set_metadata(
    app: &AppHandle,
    _state: &MediaState,
    meta: MediaMetadataInput,
) -> Result<(), String> {
    now_playing::set_metadata(app, meta)
}

#[cfg(target_os = "macos")]
fn set_playback(
    app: &AppHandle,
    _state: &MediaState,
    playback: MediaPlaybackInput,
) -> Result<(), String> {
    now_playing::set_playback(app, playback)
}

/// The macOS Now Playing info, published here instead of through souvlaki.
///
/// souvlaki loads the cover with `-[NSImage initWithContentsOfURL:]` and stores the
/// result without a nil check, so a cover that fails to load aborts the app. It also
/// rebuilds the whole dictionary on every metadata push, and the system widget
/// redraws the cover whenever the dictionary briefly lacks one or carries a new
/// `MPMediaItemArtwork` — which showed as a flash each time the player re-sent the
/// same track (it does once the decoder settles the duration). So each change is
/// published as one complete dictionary, the artwork object is reused for as long
/// as the cover URL stays the same, and unchanged pushes publish nothing.
///
/// All state lives on the main thread; the cover fetch and the grace timer hop back
/// to it.
#[cfg(target_os = "macos")]
mod now_playing {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_app_kit::NSImage;
    use objc2_foundation::{NSData, NSMutableDictionary, NSNumber, NSSize, NSString};
    use objc2_media_player::{
        MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
        MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
        MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
        MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState,
    };
    use tauri::AppHandle;

    use super::{MediaMetadataInput, MediaPlaybackInput};

    const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
    /// How long a new cover holds back the publish, so the widget switches once
    /// instead of showing the title over an empty tile first.
    const COVER_GRACE: Duration = Duration::from_millis(800);
    /// The system extrapolates elapsed time from the published rate, so a position
    /// report is only published when it disagrees with that by more than this.
    const DRIFT_TOLERANCE_SECS: f64 = 1.0;
    /// Decoded covers kept, so stepping back to a recent track reuses its artwork.
    const ARTWORK_CACHE: usize = 4;

    #[derive(Default, Clone, PartialEq)]
    struct Text {
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        duration: Option<f64>,
    }

    impl Text {
        fn same_track(&self, other: &Text) -> bool {
            self.title == other.title && self.artist == other.artist && self.album == other.album
        }
    }

    #[derive(Default)]
    struct State {
        text: Text,
        cover_url: Option<String>,
        /// Bumped when the cover changes, so a late fetch or timer for an older
        /// cover is ignored.
        generation: u64,
        /// Publishing waits for the current cover or for `COVER_GRACE`.
        holding: bool,
        /// Most recently used first.
        artworks: VecDeque<(String, Retained<MPMediaItemArtwork>)>,
        playing: Option<bool>,
        /// Elapsed seconds as of `anchor`.
        elapsed: f64,
        anchor: Option<Instant>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::default();
    }

    impl State {
        fn position(&self, now: Instant) -> f64 {
            match (self.playing, self.anchor) {
                (Some(true), Some(at)) => self.elapsed + now.duration_since(at).as_secs_f64(),
                _ => self.elapsed,
            }
        }

        fn current_artwork(&self) -> Option<&MPMediaItemArtwork> {
            let url = self.cover_url.as_ref()?;
            self.artworks
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, artwork)| &**artwork)
        }

        /// Moves a cached cover to the front; `false` when it is not cached.
        fn promote(&mut self, url: &str) -> bool {
            let Some(i) = self.artworks.iter().position(|(u, _)| u == url) else {
                return false;
            };
            if let Some(entry) = self.artworks.remove(i) {
                self.artworks.push_front(entry);
            }
            true
        }

        fn remember(&mut self, url: String, artwork: Retained<MPMediaItemArtwork>) {
            self.artworks.retain(|(u, _)| *u != url);
            self.artworks.push_front((url, artwork));
            self.artworks.truncate(ARTWORK_CACHE);
        }

        fn publish(&self) {
            let info = NSMutableDictionary::<NSString, AnyObject>::new();
            let rate = if self.playing == Some(true) { 1.0 } else { 0.0 };
            // SAFETY: the keys are immutable framework constants, and each value has
            // the type the info center documents for its key.
            unsafe {
                let text = [
                    (MPMediaItemPropertyTitle, &self.text.title),
                    (MPMediaItemPropertyArtist, &self.text.artist),
                    (MPMediaItemPropertyAlbumTitle, &self.text.album),
                ];
                for (key, value) in text {
                    if let Some(value) = value {
                        info.insert(key, &*NSString::from_str(value));
                    }
                }
                if let Some(duration) = self.text.duration {
                    info.insert(
                        MPMediaItemPropertyPlaybackDuration,
                        &*NSNumber::new_f64(duration),
                    );
                }
                info.insert(
                    MPNowPlayingInfoPropertyElapsedPlaybackTime,
                    &*NSNumber::new_f64(self.position(Instant::now())),
                );
                info.insert(
                    MPNowPlayingInfoPropertyPlaybackRate,
                    &*NSNumber::new_f64(rate),
                );
                if let Some(artwork) = self.current_artwork() {
                    info.insert(MPMediaItemPropertyArtwork, artwork);
                }
                MPNowPlayingInfoCenter::defaultCenter().setNowPlayingInfo(Some(&info));
            }
        }
    }

    fn on_main(app: &AppHandle, f: impl FnOnce(&mut State) + Send + 'static) -> Result<(), String> {
        app.run_on_main_thread(move || STATE.with_borrow_mut(f))
            .map_err(|e| e.to_string())
    }

    pub fn set_metadata(app: &AppHandle, meta: MediaMetadataInput) -> Result<(), String> {
        let handle = app.clone();
        on_main(app, move |state| update_metadata(state, &handle, meta))
    }

    pub fn set_playback(app: &AppHandle, playback: MediaPlaybackInput) -> Result<(), String> {
        on_main(app, move |state| update_playback(state, playback))
    }

    fn update_metadata(state: &mut State, app: &AppHandle, meta: MediaMetadataInput) {
        let text = Text {
            title: meta.title,
            artist: meta.artist,
            album: meta.album,
            duration: meta.duration_secs.filter(|d| d.is_finite() && *d > 0.0),
        };
        let cover_url = meta.cover_url.filter(|u| !u.is_empty());
        if text == state.text && cover_url == state.cover_url {
            return;
        }
        if !text.same_track(&state.text) {
            state.elapsed = 0.0;
            state.anchor = Some(Instant::now());
        }
        state.text = text;
        if cover_url != state.cover_url {
            state.generation += 1;
            state.holding = false;
            state.cover_url.clone_from(&cover_url);
            if let Some(url) = cover_url {
                if !state.promote(&url) {
                    state.holding = true;
                    fetch_cover(app, state.generation, url);
                }
            }
        }
        if !state.holding {
            state.publish();
        }
    }

    fn update_playback(state: &mut State, playback: MediaPlaybackInput) {
        let now = Instant::now();
        let expected = state.position(now);
        let position = playback
            .position_secs
            .filter(|p| p.is_finite())
            .map_or(expected, |p| p.max(0.0));
        let changed = state.playing != Some(playback.playing);
        state.playing = Some(playback.playing);
        state.elapsed = position;
        state.anchor = Some(now);
        if changed {
            let playback_state = if playback.playing {
                MPNowPlayingPlaybackState::Playing
            } else {
                MPNowPlayingPlaybackState::Paused
            };
            // SAFETY: a plain value setter on the shared info center.
            unsafe { MPNowPlayingInfoCenter::defaultCenter().setPlaybackState(playback_state) };
        }
        if !state.holding && (changed || (position - expected).abs() > DRIFT_TOLERANCE_SECS) {
            state.publish();
        }
    }

    fn fetch_cover(app: &AppHandle, generation: u64, url: String) {
        let grace = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(COVER_GRACE).await;
            let _ = on_main(&grace, move |state| {
                if state.generation == generation && state.holding {
                    state.holding = false;
                    state.publish();
                }
            });
        });

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let bytes = download(&url).await;
            let _ = on_main(&app, move |state| {
                if state.generation != generation {
                    return;
                }
                let artwork = bytes.as_deref().and_then(artwork_from);
                if artwork.is_none() && !state.holding {
                    return;
                }
                if let Some(artwork) = artwork {
                    state.remember(url, artwork);
                }
                state.holding = false;
                state.publish();
            });
        });
    }

    async fn download(url: &str) -> Option<Vec<u8>> {
        use mediaharbor_core::services::common::pipeline::tagger::cover_bytes;
        let client = mediaharbor_core::http_client::shared_client().ok()?;
        tokio::time::timeout(FETCH_TIMEOUT, cover_bytes(&client, url))
            .await
            .ok()?
            .ok()
    }

    /// `None` for bytes that are not a decodable image: the nil check souvlaki
    /// lacks.
    fn artwork_from(bytes: &[u8]) -> Option<Retained<MPMediaItemArtwork>> {
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
        let size = image.size();
        let handler = RcBlock::new(move |_: NSSize| -> NonNull<NSImage> { NonNull::from(&*image) });
        // SAFETY: the block owns `image`, so the pointer it returns stays valid for
        // as long as the artwork can call it.
        Some(unsafe {
            MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                MPMediaItemArtwork::alloc(),
                size,
                &handler,
            )
        })
    }
}
