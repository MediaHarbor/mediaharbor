#![recursion_limit = "512"]
#![cfg_attr(
    all(not(debug_assertions), not(feature = "console")),
    windows_subsystem = "windows"
)]

mod media_controls;

use mediaharbor_core::{
    build_tidal_authorize_url, ipc_contract, tidal_exchange_code_inner, BackendState, EventEmitter,
    TIDAL_REDIRECT_URI,
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

struct TauriEmitter(AppHandle);

macro_rules! emit_events {
    ($($name:ident => $ty:ident, $topic:literal;)*) => {
        $(
            fn $name(&self, event: &ipc_contract::$ty) {
                let _ = self.0.emit($topic, event);
            }
        )*
    };
}

impl EventEmitter for TauriEmitter {
    fn emit_log(&self, entry: &ipc_contract::BackendLogEvent) {
        let _ = self.0.emit("backend-log", entry);
    }
    fn emit_download_info(&self, event: &ipc_contract::DownloadInfoEvent) {
        let _ = self.0.emit("download-info", event);
    }
    fn emit_progress(&self, event: &ipc_contract::DownloadProgressEvent) {
        let _ = self.0.emit("download-progress", event);
    }
    fn emit_stream_ready(&self, event: &ipc_contract::StreamReadyEvent) {
        let _ = self.0.emit("stream-ready", event);
    }
    fn emit_install_progress(&self, event: &ipc_contract::InstallationProgressEvent) {
        let _ = self.0.emit("install-progress", event);
    }
    fn emit_app_error(&self, event: &ipc_contract::AppErrorEvent) {
        let _ = self.0.emit("app-error", event);
    }
    fn emit_stdin_prompt(&self, event: &ipc_contract::ProcessStdinPromptEvent) {
        let _ = self.0.emit("process-stdin-prompt", event);
    }
    emit_events! {
        emit_download_summary => DownloadSummaryEvent, "download-summary";
        emit_library_scan_progress => LibraryScanProgressEvent, "library-scan-progress";
        emit_library_changed => LibraryChangedEvent, "library-changed";
        emit_radio_metadata => RadioMetadataEvent, "radio-metadata";
        emit_saved_state_changed => SavedStateChangedEvent, "saved-state-changed";
        emit_service_playlist_changed => ServicePlaylistChangedEvent, "service-playlist-changed";
        emit_credential_status_changed => CredentialStatusChangedEvent, "credential-status-changed";
        emit_player_position => PlayerPositionEvent, "player-position";
        emit_player_state => PlayerStateEvent, "player-state";
        emit_player_error => PlayerErrorEvent, "player-error";
        emit_audio_spectrum => AudioSpectrumEvent, "audio-spectrum";
        emit_player_track_changed => PlayerTrackChangedEvent, "player-track-changed";
    }
}

pub(crate) struct AppState(pub(crate) Arc<BackendState>);

/// Generates the `#[tauri::command]` shim for a `BackendState` method of the
/// same name. `infallible` is for methods that return the response type
/// directly instead of an `MhResult`.
///
/// The inner call is boxed because Tauri only boxes command futures in dev
/// builds (`respond_async_serialized`, gated on `debug_assertions`); a release
/// build hands the future to `async_runtime::spawn` by value, so its entire
/// size lands on the IPC thread's stack. `play_media`'s future is ~650 KB,
/// which overflows the 1 MB stack Windows reserves for the main thread and
/// kills the process with no panic and no log line. Boxing keeps the frame
/// pointer-sized.
macro_rules! tauri_delegate {
    ($name:ident, $req:ty, $ret:ty) => {
        #[tauri::command]
        async fn $name(state: State<'_, AppState>, req: $req) -> Result<$ret, String> {
            Box::pin(state.0.$name(req))
                .await
                .map_err(|e| e.to_string())
        }
    };
    ($name:ident, $ret:ty) => {
        #[tauri::command]
        async fn $name(state: State<'_, AppState>) -> Result<$ret, String> {
            Box::pin(state.0.$name()).await.map_err(|e| e.to_string())
        }
    };
    (infallible $name:ident, $req:ty, $ret:ty) => {
        #[tauri::command]
        async fn $name(state: State<'_, AppState>, req: $req) -> Result<$ret, String> {
            Ok(Box::pin(state.0.$name(req)).await)
        }
    };
    (infallible $name:ident, $ret:ty) => {
        #[tauri::command]
        async fn $name(state: State<'_, AppState>) -> Result<$ret, String> {
            Ok(Box::pin(state.0.$name()).await)
        }
    };
}

/// One `tauri_delegate!` per line. `name(Req) -> Ret;` takes a request,
/// `name -> Ret;` does not, and either may be prefixed with `infallible`.
/// Matched one row at a time — a single repetition would make the optional
/// leading `infallible` ambiguous against `$name:ident`.
macro_rules! tauri_delegates {
    () => {};
    (infallible $name:ident($req:ty) -> $ret:ty; $($rest:tt)*) => {
        tauri_delegate!(infallible $name, $req, $ret);
        tauri_delegates!($($rest)*);
    };
    (infallible $name:ident -> $ret:ty; $($rest:tt)*) => {
        tauri_delegate!(infallible $name, $ret);
        tauri_delegates!($($rest)*);
    };
    ($name:ident($req:ty) -> $ret:ty; $($rest:tt)*) => {
        tauri_delegate!($name, $req, $ret);
        tauri_delegates!($($rest)*);
    };
    ($name:ident -> $ret:ty; $($rest:tt)*) => {
        tauri_delegate!($name, $ret);
        tauri_delegates!($($rest)*);
    };
}

tauri_delegates! {
    infallible get_settings -> ipc_contract::GetSettingsResponse;
    infallible set_settings(ipc_contract::SetSettingsRequest) -> ipc_contract::SetSettingsResponse;
    play_media(ipc_contract::PlayMediaRequest) -> ipc_contract::PlayMediaResponse;
    infallible pause_media -> ipc_contract::PauseMediaResponse;
    player_load(ipc_contract::PlayerLoadRequest) -> ();
    player_play -> ();
    player_pause -> ();
    player_stop -> ();
    player_seek(ipc_contract::PlayerSeekRequest) -> ();
    player_crossfade_to(ipc_contract::PlayerCrossfadeRequest) -> ();
    player_set_volume(ipc_contract::PlayerVolumeRequest) -> ();
    player_set_muted(ipc_contract::PlayerMutedRequest) -> ();
    player_set_spectrum_enabled(ipc_contract::PlayerSpectrumRequest) -> ();
    player_list_devices -> ipc_contract::PlayerDevicesResponse;
    spotify_oauth_login -> ipc_contract::SpotifyOAuthLoginResponse;
    infallible spotify_oauth_logout -> ipc_contract::SpotifyOAuthLogoutResponse;
    infallible spotify_oauth_status -> ipc_contract::SpotifyOAuthStatusResponse;
    infallible spotify_get_token -> ipc_contract::SpotifyGetTokenResponse;
    clear_spotify_credentials -> ();
    tidal_start_auth -> ipc_contract::TidalStartAuthResponse;
    tidal_exchange_code(ipc_contract::TidalExchangeCodeRequest) -> ipc_contract::TidalExchangeCodeResponse;
    infallible start_yt_music_download(ipc_contract::StartYtMusicDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible start_yt_video_download(ipc_contract::StartYtVideoDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible start_spotify_download(ipc_contract::StartSpotifyDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible start_apple_download(ipc_contract::StartAppleDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible start_qobuz_download(ipc_contract::StartQobuzDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible start_deezer_download(ipc_contract::StartDeezerDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible start_tidal_download(ipc_contract::StartTidalDownloadRequest) -> ipc_contract::StartDownloadResponse;
    library_scan(ipc_contract::ScanDirectoryRequest) -> ();
    library_query(ipc_contract::LibraryQueryRequest) -> mediaharbor_core::media::library::LibraryQueryResult;
    library_album(ipc_contract::LibraryAlbumRequest) -> Option<mediaharbor_core::media::library::LibraryAlbumDetail>;
    library_set_watch(ipc_contract::LibraryWatchRequest) -> ();
    infallible library_cover_url(ipc_contract::LibraryCoverUrlRequest) -> Option<String>;
    library_artist(ipc_contract::LibraryArtistRequest) -> Option<mediaharbor_core::media::library::LibraryArtistDetail>;
    library_playlist_create(ipc_contract::LibraryPlaylistCreateRequest) -> ipc_contract::LibraryPlaylistCreateResponse;
    library_playlist_rename(ipc_contract::LibraryPlaylistRenameRequest) -> ();
    library_playlist_delete(ipc_contract::LibraryPlaylistIdRequest) -> ();
    library_playlist_get(ipc_contract::LibraryPlaylistIdRequest) -> Option<mediaharbor_core::media::library::LibraryPlaylistDetail>;
    library_playlist_add_tracks(ipc_contract::LibraryPlaylistAddTracksRequest) -> ();
    library_playlist_remove_track(ipc_contract::LibraryPlaylistRemoveTrackRequest) -> ();
    library_playlist_reorder(ipc_contract::LibraryPlaylistReorderRequest) -> ();
    library_playlist_import_m3u(ipc_contract::LibraryPlaylistImportRequest) -> ipc_contract::LibraryPlaylistCreateResponse;
    library_playlist_export_m3u(ipc_contract::LibraryPlaylistExportRequest) -> ();
    library_record_play(ipc_contract::LibraryRecordPlayRequest) -> ();
    library_write_tags(ipc_contract::LibraryWriteTagsRequest) -> ();
    library_radio(ipc_contract::LibraryRadioRequest) -> Vec<mediaharbor_core::media::library::LibraryTrackDto>;
    radio_sources -> Vec<mediaharbor_core::services::radio::DirectorySource>;
    radio_set_sources(ipc_contract::RadioSetSourcesRequest) -> ();
    radio_search(ipc_contract::RadioSearchRequest) -> Vec<mediaharbor_core::services::radio::Station>;
    radio_facets(ipc_contract::RadioFacetRequest) -> Vec<mediaharbor_core::services::radio::Facet>;
    radio_suggest(ipc_contract::RadioSuggestRequest) -> Vec<mediaharbor_core::services::radio::Facet>;
    radio_station(ipc_contract::RadioStationRequest) -> Option<mediaharbor_core::services::radio::Station>;
    radio_favorites -> Vec<mediaharbor_core::services::radio::Station>;
    radio_recent -> Vec<mediaharbor_core::services::radio::Station>;
    radio_set_favorite(ipc_contract::RadioFavoriteRequest) -> ();
    radio_forget(ipc_contract::RadioForgetRequest) -> ();
    radio_lists -> Vec<mediaharbor_core::services::radio::RadioListRow>;
    radio_list_create(ipc_contract::RadioListCreateRequest) -> ipc_contract::RadioListCreateResponse;
    radio_list_rename(ipc_contract::RadioListRenameRequest) -> ();
    radio_list_delete(ipc_contract::RadioListIdRequest) -> ();
    radio_list_get(ipc_contract::RadioListIdRequest) -> Option<mediaharbor_core::services::radio::RadioListDetail>;
    radio_list_add(ipc_contract::RadioListAddRequest) -> ();
    radio_list_remove(ipc_contract::RadioListRemoveRequest) -> ();
    radio_list_reorder(ipc_contract::RadioListReorderRequest) -> ();
    radio_import_url(ipc_contract::RadioImportUrlRequest) -> mediaharbor_core::services::radio::Station;
    radio_import_playlist(ipc_contract::RadioImportPlaylistRequest) -> mediaharbor_core::services::radio::Station;
    radio_import_icecast(ipc_contract::RadioImportIcecastRequest) -> Vec<mediaharbor_core::services::radio::Station>;
    radio_save_stations(ipc_contract::RadioSaveStationsRequest) -> ();
    radio_station_detail(ipc_contract::RadioStationRequest) -> Option<mediaharbor_core::services::radio::RadioStationDetail>;
    radio_update_station(ipc_contract::RadioStationEditRequest) -> ();
    radio_reset_station(ipc_contract::RadioStationRequest) -> ();
    radio_set_cover(ipc_contract::RadioSetCoverRequest) -> ();
    radio_test_stream(ipc_contract::RadioTestStreamRequest) -> mediaharbor_core::services::radio::import::StreamProbe;
    radio_parse_curl(ipc_contract::RadioParseCurlRequest) -> mediaharbor_core::services::radio::curl_parse::ParsedCurl;
    service_library_capabilities(ipc_contract::ServicePlatformRequest) -> mediaharbor_core::services::common::library::ServiceCapabilities;
    service_library_query(ipc_contract::ServiceLibraryQueryRequest) -> serde_json::Value;
    service_library_recommendations(ipc_contract::ServicePlatformRequest) -> mediaharbor_core::services::common::library::RecommendationsPage;
    service_library_explore(ipc_contract::ServicePlatformRequest) -> mediaharbor_core::services::common::library::RecommendationsPage;
    service_library_activity_feed(ipc_contract::ServicePlatformRequest) -> serde_json::Value;
    service_library_album_page(ipc_contract::ServiceLibraryIdRequest) -> serde_json::Value;
    service_library_artist_page(ipc_contract::ServiceLibraryIdRequest) -> serde_json::Value;
    service_library_explore_page(ipc_contract::ServiceLibraryIdRequest) -> mediaharbor_core::services::common::library::RecommendationsPage;
    service_library_episode_bookmarks(ipc_contract::ServicePlatformRequest) -> Vec<mediaharbor_core::media::library::LibraryTrackDto>;
    service_library_followers(ipc_contract::ServicePlatformRequest) -> Vec<mediaharbor_core::media::library::LibraryArtistDto>;
    service_library_following(ipc_contract::ServicePlatformRequest) -> Vec<mediaharbor_core::media::library::LibraryArtistDto>;
    service_library_report_playback(ipc_contract::ServiceLibraryPlaybackRequest) -> ();
    service_library_canvas(ipc_contract::ServiceLibraryIdRequest) -> Option<String>;
    service_library_set_cover(ipc_contract::ServiceLibraryCoverRequest) -> ();
    service_library_transcript(ipc_contract::ServiceLibraryIdRequest) -> serde_json::Value;
    service_library_follow_user(ipc_contract::ServiceLibraryIdRequest) -> ();
    service_library_unfollow_user(ipc_contract::ServiceLibraryIdRequest) -> ();
    service_library_album(ipc_contract::ServiceLibraryIdRequest) -> mediaharbor_core::media::library::LibraryAlbumDetail;
    service_library_artist(ipc_contract::ServiceLibraryIdRequest) -> mediaharbor_core::media::library::LibraryArtistDetail;
    service_library_playlist(ipc_contract::ServiceLibraryIdRequest) -> mediaharbor_core::media::library::LibraryPlaylistDetail;
    service_library_saved_state_for(ipc_contract::ServiceLibrarySavedStateRequest) -> Vec<String>;
    service_library_saved_state_refresh(ipc_contract::ServiceLibrarySavedStateRefreshRequest) -> ();
    service_library_set_saved(ipc_contract::ServiceLibrarySetSavedRequest) -> ipc_contract::SavedStateChangedEvent;
    service_library_playlist_create(ipc_contract::ServiceLibraryPlaylistCreateRequest) -> mediaharbor_core::services::common::library::PlaylistMutateResult;
    service_library_playlist_rename(ipc_contract::ServiceLibraryPlaylistRenameRequest) -> ();
    service_library_playlist_delete(ipc_contract::ServiceLibraryPlaylistIdRequest) -> ();
    service_library_playlist_add_tracks(ipc_contract::ServiceLibraryPlaylistMutateTracksRequest) -> mediaharbor_core::services::common::library::PlaylistMutateResult;
    service_library_playlist_remove_tracks(ipc_contract::ServiceLibraryPlaylistMutateTracksRequest) -> mediaharbor_core::services::common::library::PlaylistMutateResult;
    service_library_playlist_reorder(ipc_contract::ServiceLibraryPlaylistReorderRequest) -> mediaharbor_core::services::common::library::PlaylistMutateResult;
    service_library_radio_for(ipc_contract::ServiceLibraryRadioRequest) -> mediaharbor_core::services::common::library::RadioResult;
    service_library_radio_continue(ipc_contract::ServiceLibraryRadioContinueRequest) -> mediaharbor_core::services::common::library::RadioResult;
    infallible install_dep(ipc_contract::InstallDepRequest) -> ipc_contract::InstallDepResponse;
    infallible get_dependency_versions -> ipc_contract::GetDependencyVersionsResponse;
    infallible start_orpheus_download(ipc_contract::StartOrpheusDownloadRequest) -> ipc_contract::StartDownloadResponse;
    infallible check_orpheus_deps -> ipc_contract::CheckOrpheusDepsResponse;
    infallible install_orpheus_module(ipc_contract::InstallOrpheusModuleRequest) -> ipc_contract::InstallOrpheusModuleResponse;
    infallible send_process_stdin(ipc_contract::SendProcessStdinRequest) -> ipc_contract::SendProcessStdinResponse;
    tidal_import_token(ipc_contract::TidalImportTokenRequest) -> ipc_contract::TidalImportTokenResponse;
    get_lyrics(ipc_contract::GetLyricsRequest) -> ipc_contract::GetLyricsResponse;
}

#[tauri::command]
async fn normalize_cover_image(bytes: Vec<u8>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        mediaharbor_core::media::image_pipeline::normalize_cover_base64(&bytes)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn dialog_open_folder(
    app: AppHandle,
) -> Result<ipc_contract::DialogOpenFolderResponse, String> {
    use tauri_plugin_dialog::DialogExt;
    let app2 = app.clone();
    let path = tokio::task::spawn_blocking(move || app2.dialog().file().blocking_pick_folder())
        .await
        .ok()
        .flatten()
        .map(|p| p.to_string());
    Ok(ipc_contract::DialogOpenFolderResponse { path })
}

#[tauri::command]
async fn dialog_open_file(app: AppHandle) -> Result<ipc_contract::DialogOpenFileResponse, String> {
    use tauri_plugin_dialog::DialogExt;
    let app2 = app.clone();
    let path = tokio::task::spawn_blocking(move || app2.dialog().file().blocking_pick_file())
        .await
        .ok()
        .flatten()
        .map(|p| p.to_string());
    Ok(ipc_contract::DialogOpenFileResponse { path })
}

#[tauri::command]
async fn perform_search(
    state: State<'_, AppState>,
    req: ipc_contract::PerformSearchRequest,
) -> Result<ipc_contract::PerformSearchResponse, String> {
    let platform = req.platform;
    let results = state
        .0
        .perform_search(req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ipc_contract::PerformSearchResponse { results, platform })
}

#[tauri::command]
async fn search_suggestions(
    state: State<'_, AppState>,
    req: ipc_contract::SearchSuggestionsRequest,
) -> Result<Vec<String>, String> {
    state
        .0
        .search_suggestions(req)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_download(
    state: State<'_, AppState>,
    req: ipc_contract::CancelDownloadRequest,
) -> Result<ipc_contract::CancelDownloadResponse, String> {
    let success = state.0.cancel_download(req.download_id).await;
    Ok(ipc_contract::CancelDownloadResponse { success })
}

#[tauri::command]
async fn show_item_in_folder(
    state: State<'_, AppState>,
    req: ipc_contract::ShowItemInFolderRequest,
) -> Result<ipc_contract::ShowItemInFolderResponse, String> {
    Ok(state.0.show_item_in_folder(req))
}

#[tauri::command]
async fn service_library_owned_playlists(
    state: State<'_, AppState>,
    req: ipc_contract::ServicePlatformRequest,
) -> Result<Vec<mediaharbor_core::services::common::library::OwnedPlaylistRow>, String> {
    state
        .0
        .service_library_owned_playlists(&req.platform)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn credentials_health_snapshot(
    state: State<'_, AppState>,
) -> Result<ipc_contract::CredentialsHealthSnapshot, String> {
    Ok(state.0.credentials_health.snapshot().await)
}

#[tauri::command]
async fn resolve_share_link(
    url: String,
) -> Result<mediaharbor_core::services::common::share_links::ResolvedLink, String> {
    mediaharbor_core::services::common::share_links::resolve_share_url(&url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn credentials_health_recheck(
    state: State<'_, AppState>,
    platform: Option<String>,
) -> Result<ipc_contract::CredentialsHealthSnapshot, String> {
    use std::str::FromStr;
    match platform {
        Some(p) => {
            let svc = mediaharbor_core::services::common::library::ServicePlatform::from_str(&p)
                .map_err(|e| e.to_string())?;
            Box::pin(state.0.credentials_health.recheck(
                svc,
                &state.0.settings,
                Some(&state.0.librespot),
            ))
            .await;
        }
        None => {
            Box::pin(
                state
                    .0
                    .credentials_health
                    .sweep_all(&state.0.settings, Some(&state.0.librespot)),
            )
            .await;
        }
    }
    Ok(state.0.credentials_health.snapshot().await)
}

#[tauri::command]
async fn clear_database(
    state: State<'_, AppState>,
) -> Result<ipc_contract::ClearDatabaseResponse, String> {
    Ok(state.0.clear_database(false, false))
}

#[tauri::command]
async fn check_deps(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, bool>, String> {
    Ok(state.0.check_deps().await)
}

#[tauri::command]
async fn read_orpheus_settings() -> Result<String, String> {
    mediaharbor_core::orpheus::read_settings_json()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_orpheus_settings(content: String) -> Result<(), String> {
    mediaharbor_core::orpheus::write_raw_settings_json(&content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn probe_apple_wrapper(
    state: tauri::State<'_, AppState>,
    req: ipc_contract::WrapperProbeRequest,
) -> Result<ipc_contract::WrapperProbeResponse, String> {
    Ok(state.0.probe_apple_wrapper(req).await)
}

#[allow(deprecated)]
#[tauri::command]
async fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    app.shell().open(&url, None).map_err(|e| e.to_string())
}

const QOBUZ_LOGIN_WINDOW_LABEL: &str = "qobuz-login";

fn random_nonce() -> String {
    mediaharbor_core::services::common::ids::rand_hex(48)
}

fn build_qobuz_login_script(port: u16, nonce: &str) -> String {
    format!(
        r#"
(() => {{
  const TARGET = '/api.json/0.2/user/login';
  const ENDPOINT = 'http://127.0.0.1:{port}/qobuz/capture';
  const NONCE = '{nonce}';
  let sent = false;

  const beacon = (userId, token, appId) => {{
    if (sent) return;
    sent = true;
    const u = encodeURIComponent(userId);
    const t = encodeURIComponent(token);
    const a = encodeURIComponent(appId || '');
    const url = ENDPOINT + '?n=' + NONCE + '&u=' + u + '&t=' + t + '&a=' + a;
    try {{ new Image().src = url; }} catch (e) {{}}
    try {{ fetch(url, {{ mode: 'no-cors', keepalive: true }}).catch(() => {{}}); }} catch (e) {{}}
  }};

  const headerAppId = (headers) => {{
    try {{
      if (!headers) return '';
      if (typeof headers.get === 'function') return headers.get('X-App-Id') || headers.get('x-app-id') || '';
      if (Array.isArray(headers)) {{
        for (const [k, v] of headers) if (String(k).toLowerCase() === 'x-app-id') return v;
        return '';
      }}
      for (const k of Object.keys(headers)) if (k.toLowerCase() === 'x-app-id') return headers[k];
    }} catch (e) {{}}
    return '';
  }};

  const tryExtract = (text, appId) => {{
    try {{
      const j = JSON.parse(text);
      const id = j && j.user && j.user.id;
      const tok = j && j.user_auth_token;
      if (id && tok) beacon(String(id), String(tok), appId);
    }} catch (e) {{}}
  }};

  const origFetch = window.fetch;
  if (origFetch) {{
    window.fetch = function(...args) {{
      let appId = '';
      try {{
        if (args[0] && typeof args[0] !== 'string' && args[0].headers) appId = headerAppId(args[0].headers);
        if (!appId && args[1] && args[1].headers) appId = headerAppId(args[1].headers);
      }} catch (e) {{}}
      const p = origFetch.apply(this, args);
      return p.then((resp) => {{
        try {{
          const url = typeof args[0] === 'string' ? args[0] : (args[0] && args[0].url) || '';
          if (url && url.indexOf(TARGET) !== -1) {{
            resp.clone().text().then((txt) => tryExtract(txt, appId)).catch(() => {{}});
          }}
        }} catch (e) {{}}
        return resp;
      }});
    }};
  }}

  const OrigXHR = window.XMLHttpRequest;
  function PatchedXHR() {{
    const xhr = new OrigXHR();
    let url = '';
    let appId = '';
    const origOpen = xhr.open;
    const origSet = xhr.setRequestHeader;
    xhr.open = function(m, u) {{ url = u || ''; return origOpen.apply(this, arguments); }};
    xhr.setRequestHeader = function(name, value) {{
      try {{ if (String(name).toLowerCase() === 'x-app-id') appId = String(value); }} catch (e) {{}}
      return origSet.apply(this, arguments);
    }};
    xhr.addEventListener('load', function() {{
      try {{ if (url.indexOf(TARGET) !== -1 && xhr.responseText) tryExtract(xhr.responseText, appId); }} catch (e) {{}}
    }});
    return xhr;
  }}
  window.XMLHttpRequest = PatchedXHR;
}})();
"#
    )
}

/// Focuses an already-open login window, or hands back the streaming-server port a new
/// one needs. `Ok(None)` means the window was already there and there is nothing to do.
///
/// The two login commands diverge sharply after this point — Tidal runs a PKCE exchange
/// and a navigation interceptor, Qobuz an incognito window and a settings write — but
/// this preamble was identical in both.
fn login_window_server<'a>(
    app: &AppHandle,
    state: &'a AppState,
    label: &str,
) -> Result<Option<&'a mediaharbor_core::streaming_server::StreamingServer>, String> {
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.set_focus();
        return Ok(None);
    }
    state
        .0
        .streaming_server
        .as_ref()
        .ok_or_else(|| "streaming server not running".to_string())
        .map(Some)
}

#[tauri::command]
async fn qobuz_open_login_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(srv) = login_window_server(&app, &state, QOBUZ_LOGIN_WINDOW_LABEL)? else {
        return Ok(());
    };
    let port = srv.port;
    let nonce = random_nonce();
    let rx = srv.qobuz_captures().register(nonce.clone());

    let script = build_qobuz_login_script(port, &nonce);

    let parsed: tauri::Url = "https://play.qobuz.com/login"
        .parse()
        .map_err(|e: <tauri::Url as std::str::FromStr>::Err| e.to_string())?;
    let url = tauri::WebviewUrl::External(parsed);
    tauri::WebviewWindowBuilder::new(&app, QOBUZ_LOGIN_WINDOW_LABEL, url)
        .title("Sign in with Qobuz")
        .inner_size(1280.0, 860.0)
        .min_inner_size(1100.0, 700.0)
        .resizable(true)
        .focused(true)
        .incognito(true)
        .initialization_script(&script)
        .build()
        .map_err(|e| e.to_string())?;

    let app_clone = app.clone();
    let backend = state.0.clone();
    let nonce_for_cleanup = nonce.clone();
    tauri::async_runtime::spawn(async move {
        let captured = tokio::time::timeout(std::time::Duration::from_secs(300), rx).await;
        if let Some(srv) = backend.streaming_server.as_ref() {
            srv.qobuz_captures().unregister(&nonce_for_cleanup);
        }
        let Ok(Ok(cap)) = captured else {
            return;
        };
        if cap.user_id.trim().is_empty() || cap.user_auth_token.trim().is_empty() {
            return;
        }

        let mut settings = backend.get_settings().await.settings;
        settings.qobuz_email_or_userid = cap.user_id.clone();
        settings.qobuz_password_or_token = cap.user_auth_token.clone();
        if !cap.app_id.is_empty() {
            settings.qobuz_app_id = cap.app_id.clone();
            if let Ok(http) = mediaharbor_core::http_client::build_mozilla_client() {
                if let Some(secret) =
                    mediaharbor_core::services::qobuz::app_credentials::live_secret_for(
                        &http,
                        &cap.app_id,
                    )
                    .await
                {
                    settings.qobuz_secrets = secret;
                }
            }
        }
        let _ = backend
            .set_settings(ipc_contract::SetSettingsRequest { settings })
            .await;
        *backend.qobuz_client_cache.write().await = None;

        let _ = app_clone.emit(
            "qobuz-login-captured",
            &serde_json::json!({ "userId": cap.user_id }),
        );

        if let Some(win) = app_clone.get_webview_window(QOBUZ_LOGIN_WINDOW_LABEL) {
            let _ = win.close();
        }
    });

    Ok(())
}

const TIDAL_LOGIN_WINDOW_LABEL: &str = "tidal-login";

fn build_tidal_login_script(port: u16, nonce: &str) -> String {
    format!(
        r#"
(() => {{
  const TARGET = '{redirect}';
  const ENDPOINT = 'http://127.0.0.1:{port}/tidal/capture';
  const NONCE = '{nonce}';
  let sent = false;

  const beacon = (code) => {{
    if (sent) return;
    sent = true;
    const url = ENDPOINT + '?n=' + NONCE + '&c=' + encodeURIComponent(code);
    try {{ new Image().src = url; }} catch (e) {{}}
    try {{ fetch(url, {{ mode: 'no-cors', keepalive: true }}).catch(() => {{}}); }} catch (e) {{}}
  }};

  const check = () => {{
    try {{
      const href = location.href || '';
      if (href.indexOf(TARGET) !== 0) return;
      const u = new URL(href);
      const code = u.searchParams.get('code');
      if (code) beacon(code);
    }} catch (e) {{}}
  }};

  check();
  try {{ window.addEventListener('DOMContentLoaded', check); }} catch (e) {{}}
  try {{ window.addEventListener('load', check); }} catch (e) {{}}
  try {{ window.addEventListener('hashchange', check); }} catch (e) {{}}
  try {{ window.addEventListener('popstate', check); }} catch (e) {{}}
  setInterval(check, 500);
}})();
"#,
        redirect = TIDAL_REDIRECT_URI,
    )
}

#[tauri::command]
async fn tidal_open_login_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(srv) = login_window_server(&app, &state, TIDAL_LOGIN_WINDOW_LABEL)? else {
        return Ok(());
    };

    let (code_verifier, auth_url) = build_tidal_authorize_url().map_err(|e| e.to_string())?;

    let port = srv.port;
    let nonce = random_nonce();
    let rx = srv.tidal_captures().register(nonce.clone());

    let deliver_capture = srv.tidal_capture_deliverer();
    let nonce_for_nav = nonce.clone();

    let script = build_tidal_login_script(port, &nonce);

    let parsed: tauri::Url = auth_url
        .parse()
        .map_err(|e: <tauri::Url as std::str::FromStr>::Err| e.to_string())?;
    let url = tauri::WebviewUrl::External(parsed);
    tauri::WebviewWindowBuilder::new(&app, TIDAL_LOGIN_WINDOW_LABEL, url)
        .title("Sign in with Tidal")
        .inner_size(960.0, 720.0)
        .resizable(true)
        .focused(true)
        .initialization_script(&script)
        .on_navigation(move |nav_url| {
            let is_callback = nav_url.as_str().starts_with(TIDAL_REDIRECT_URI)
                || nav_url.path().ends_with("/login/auth");
            if is_callback {
                let code = nav_url
                    .query_pairs()
                    .find(|(k, _)| k == "code")
                    .map(|(_, v)| v.into_owned())
                    .or_else(|| {
                        nav_url.fragment().and_then(|frag| {
                            frag.split('&')
                                .find_map(|kv| kv.strip_prefix("code=").map(|c| c.to_string()))
                        })
                    });
                if let Some(code) = code {
                    deliver_capture(&nonce_for_nav, code);
                    return false;
                }
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;

    let app_clone = app.clone();
    let backend = state.0.clone();
    let nonce_for_cleanup = nonce.clone();
    tauri::async_runtime::spawn(async move {
        let captured = tokio::time::timeout(std::time::Duration::from_secs(300), rx).await;
        if let Some(srv) = backend.streaming_server.as_ref() {
            srv.tidal_captures().unregister(&nonce_for_cleanup);
        }
        let Ok(Ok(cap)) = captured else {
            return;
        };
        let code = cap.code;

        let tokens = match tidal_exchange_code_inner(&code, &code_verifier).await {
            Ok(t) => t,
            Err(e) => {
                let _ = app_clone.emit(
                    "tidal-login-error",
                    &serde_json::json!({ "error": e.to_string() }),
                );
                if let Some(win) = app_clone.get_webview_window(TIDAL_LOGIN_WINDOW_LABEL) {
                    let _ = win.close();
                }
                return;
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let expiry_time = now + tokens.expires_in as f64;

        let mut settings = backend.get_settings().await.settings;
        settings.tidal_access_token = tokens.access_token.clone();
        settings.tidal_refresh_token = tokens.refresh_token.clone();
        settings.tidal_token_expiry = (expiry_time as i64).to_string();
        settings.tidal_user_id = tokens.user_id.clone();
        settings.tidal_country_code = tokens.country_code.clone();
        let _ = backend
            .set_settings(ipc_contract::SetSettingsRequest { settings })
            .await;

        backend
            .credentials_health
            .recheck(
                mediaharbor_core::services::common::library::ServicePlatform::Tidal,
                &backend.settings,
                Some(&backend.librespot),
            )
            .await;

        let _ = app_clone.emit(
            "tidal-login-captured",
            &serde_json::json!({
                "userId": tokens.user_id,
                "countryCode": tokens.country_code,
                "accessToken": tokens.access_token,
                "refreshToken": tokens.refresh_token,
                "expiryTime": expiry_time,
            }),
        );

        if let Some(win) = app_clone.get_webview_window(TIDAL_LOGIN_WINDOW_LABEL) {
            let _ = win.close();
        }
    });

    Ok(())
}

#[tauri::command]
async fn get_version(
    state: State<'_, AppState>,
) -> Result<ipc_contract::GetVersionResponse, String> {
    Ok(state.0.get_version())
}

#[tauri::command]
async fn check_updates(
    state: State<'_, AppState>,
) -> Result<ipc_contract::CheckUpdatesResponse, String> {
    Ok(state.0.check_updates().await.unwrap_or_else(|_| ipc_contract::CheckUpdatesResponse {
        update_available: false,
        latest_version: None,
        release_url: None,
        release_notes: None,
    }))
}

/// Append panics to the log file before letting the default hook run.
///
/// Playback work happens on threads — `mh-audio-decode`, `mh-audio-device`,
/// cpal's own audio thread — whose panics unwind into nothing and kill only
/// that thread. In a release build there is no console to notice on, so
/// without this the symptom is silence or a crash with no record of either.
///
/// Note this cannot catch an access violation or a stack overflow: those never
/// reach a Rust panic. An empty log after a crash is itself a signal.
fn install_panic_hook(logger: mediaharbor_core::Logger) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        logger.error(
            "panic",
            &format!(
                "thread '{}' panicked at {}: {}\n{}",
                thread.name().unwrap_or("unnamed"),
                info.location()
                    .map(|l| l.to_string())
                    .unwrap_or_else(|| "unknown location".to_string()),
                info.payload()
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown payload".to_string()),
                std::backtrace::Backtrace::force_capture()
            ),
        );
        previous(info);
    }));
}

fn main() {
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    // Nothing here suppresses WebKitGTK's own MPRIS session. It used to set
    // WEBKIT_DISABLE_MEDIA_SESSION_API=1, which reads like a fix and is not one:
    // no such environment variable exists in WebKitGTK. Dumping the strings in
    // libwebkit2gtk-4.1.so.0 (2.52.6) shows every WEBKIT_* symbol is an enum
    // name, while the MPRIS registration code is very much present. The real
    // switch is the `MediaSessionEnabled` feature, reachable only through
    // `webkit_settings_set_feature_enabled`, which webkit2gtk 2.0.2 does not
    // bind — so it would take raw FFI.

    mediaharbor_core::player::output::init_stream_identity();

    mediaharbor_core::venv_manager::ensure_managed_bin_dir();
    mediaharbor_core::venv_manager::augment_process_path();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    tauri::async_runtime::set(rt.handle().clone());
    let _rt = rt;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let emitter: Arc<dyn EventEmitter> = Arc::new(TauriEmitter(app_handle));

            let user_data = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("mediaharbor"));

            let state = tauri::async_runtime::block_on(async {
                BackendState::init(user_data, emitter)
                    .await
                    .expect("Failed to initialize MediaHarbor backend")
            });

            install_panic_hook(state.logger.clone());

            app.manage(AppState(Arc::new(state)));
            media_controls::setup(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            dialog_open_folder,
            dialog_open_file,
            perform_search,
            search_suggestions,
            play_media,
            pause_media,
            player_load,
            player_play,
            player_pause,
            player_stop,
            player_seek,
            player_crossfade_to,
            player_set_volume,
            player_set_muted,
            player_set_spectrum_enabled,
            player_list_devices,
            media_controls::media_set_metadata,
            media_controls::media_set_playback,
            spotify_oauth_login,
            spotify_oauth_logout,
            spotify_oauth_status,
            spotify_get_token,
            clear_spotify_credentials,
            tidal_start_auth,
            tidal_exchange_code,
            tidal_open_login_window,
            tidal_import_token,
            start_yt_music_download,
            start_yt_video_download,
            start_spotify_download,
            start_apple_download,
            start_qobuz_download,
            start_deezer_download,
            start_tidal_download,
            cancel_download,
            show_item_in_folder,
            library_scan,
            library_query,
            library_album,
            library_set_watch,
            library_cover_url,
            library_artist,
            library_playlist_create,
            library_playlist_rename,
            library_playlist_delete,
            library_playlist_get,
            library_playlist_add_tracks,
            library_playlist_remove_track,
            library_playlist_reorder,
            library_playlist_import_m3u,
            library_playlist_export_m3u,
            library_record_play,
            library_write_tags,
            library_radio,
            radio_sources,
            radio_set_sources,
            radio_search,
            radio_facets,
            radio_suggest,
            radio_station,
            radio_favorites,
            radio_recent,
            radio_set_favorite,
            radio_forget,
            radio_lists,
            radio_list_create,
            radio_list_rename,
            radio_list_delete,
            radio_list_get,
            radio_list_add,
            radio_list_remove,
            radio_list_reorder,
            radio_import_url,
            radio_import_playlist,
            radio_import_icecast,
            radio_save_stations,
            radio_station_detail,
            radio_update_station,
            radio_reset_station,
            radio_set_cover,
            radio_test_stream,
            radio_parse_curl,
            service_library_capabilities,
            service_library_query,
            service_library_recommendations,
            service_library_explore,
            service_library_activity_feed,
            service_library_album_page,
            service_library_artist_page,
            service_library_explore_page,
            service_library_episode_bookmarks,
            service_library_followers,
            service_library_following,
            service_library_report_playback,
            service_library_canvas,
            service_library_set_cover,
            service_library_transcript,
            normalize_cover_image,
            service_library_follow_user,
            service_library_unfollow_user,
            service_library_album,
            service_library_artist,
            service_library_playlist,
            service_library_saved_state_for,
            service_library_saved_state_refresh,
            service_library_owned_playlists,
            service_library_set_saved,
            service_library_playlist_create,
            service_library_playlist_rename,
            service_library_playlist_delete,
            service_library_playlist_add_tracks,
            service_library_playlist_remove_tracks,
            service_library_playlist_reorder,
            service_library_radio_for,
            service_library_radio_continue,
            credentials_health_snapshot,
            credentials_health_recheck,
            resolve_share_link,
            clear_database,
            get_version,
            check_updates,
            check_deps,
            install_dep,
            get_dependency_versions,
            start_orpheus_download,
            check_orpheus_deps,
            install_orpheus_module,
            read_orpheus_settings,
            write_orpheus_settings,
            send_process_stdin,
            probe_apple_wrapper,
            open_external,
            get_lyrics,
            qobuz_open_login_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaHarbor application");
}
