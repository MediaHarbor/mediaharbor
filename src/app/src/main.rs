#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use mediaharbor_core::{
    ipc_contract,
    BackendState,
    EventEmitter,
};

struct TauriEmitter(AppHandle);

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
}

struct AppState(Arc<BackendState>);

#[tauri::command]
async fn get_settings(
    state: State<'_, AppState>,
) -> Result<ipc_contract::GetSettingsResponse, String> {
    Ok(state.0.get_settings().await)
}

#[tauri::command]
async fn set_settings(
    state: State<'_, AppState>,
    req: ipc_contract::SetSettingsRequest,
) -> Result<ipc_contract::SetSettingsResponse, String> {
    Ok(state.0.set_settings(req).await)
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
async fn dialog_open_file(
    app: AppHandle,
) -> Result<ipc_contract::DialogOpenFileResponse, String> {
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
    let platform = req.platform.clone();
    let results = state.0.perform_search(req).await.map_err(|e| e.to_string())?;
    Ok(ipc_contract::PerformSearchResponse { results, platform })
}

#[tauri::command]
async fn play_media(
    state: State<'_, AppState>,
    req: ipc_contract::PlayMediaRequest,
) -> Result<ipc_contract::PlayMediaResponse, String> {
    state.0.play_media(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn pause_media(
    state: State<'_, AppState>,
) -> Result<ipc_contract::PauseMediaResponse, String> {
    Ok(state.0.pause_media().await)
}

#[tauri::command]
async fn spotify_oauth_login(
    state: State<'_, AppState>,
) -> Result<ipc_contract::SpotifyOAuthLoginResponse, String> {
    state.0.spotify_oauth_login().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn spotify_oauth_logout(
    state: State<'_, AppState>,
) -> Result<ipc_contract::SpotifyOAuthLogoutResponse, String> {
    Ok(state.0.spotify_oauth_logout().await)
}

#[tauri::command]
async fn spotify_oauth_status(
    state: State<'_, AppState>,
) -> Result<ipc_contract::SpotifyOAuthStatusResponse, String> {
    Ok(state.0.spotify_oauth_status().await)
}

#[tauri::command]
async fn spotify_get_token(
    state: State<'_, AppState>,
) -> Result<ipc_contract::SpotifyGetTokenResponse, String> {
    Ok(state.0.spotify_get_token().await)
}

#[tauri::command]
async fn clear_spotify_credentials(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.0.clear_spotify_credentials().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn tidal_start_auth(
    state: State<'_, AppState>,
) -> Result<ipc_contract::TidalStartAuthResponse, String> {
    state.0.tidal_start_auth().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn tidal_exchange_code(
    state: State<'_, AppState>,
    req: ipc_contract::TidalExchangeCodeRequest,
) -> Result<ipc_contract::TidalExchangeCodeResponse, String> {
    state.0.tidal_exchange_code(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_artist_details(
    state: State<'_, AppState>,
    req: ipc_contract::GetArtistDetailsRequest,
) -> Result<ipc_contract::MediaDetailsResponse, String> {
    state.0.get_artist_details(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_album_details(
    state: State<'_, AppState>,
    req: ipc_contract::GetAlbumDetailsRequest,
) -> Result<ipc_contract::MediaDetailsResponse, String> {
    state.0.get_album_details(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_playlist_details(
    state: State<'_, AppState>,
    req: ipc_contract::GetPlaylistDetailsRequest,
) -> Result<ipc_contract::MediaDetailsResponse, String> {
    state.0.get_playlist_details(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_yt_music_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartYtMusicDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_yt_music_download(req).await)
}

#[tauri::command]
async fn start_yt_video_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartYtVideoDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_yt_video_download(req).await)
}

#[tauri::command]
async fn start_spotify_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartSpotifyDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_spotify_download(req).await)
}

#[tauri::command]
async fn start_apple_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartAppleDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_apple_download(req).await)
}

#[tauri::command]
async fn start_qobuz_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartQobuzDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_qobuz_download(req).await)
}

#[tauri::command]
async fn start_deezer_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartDeezerDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_deezer_download(req).await)
}

#[tauri::command]
async fn start_tidal_download(
    state: State<'_, AppState>,
    req: ipc_contract::StartTidalDownloadRequest,
) -> Result<ipc_contract::StartDownloadResponse, String> {
    Ok(state.0.start_tidal_download(req).await)
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
async fn scan_directory(
    state: State<'_, AppState>,
    req: ipc_contract::ScanDirectoryRequest,
) -> Result<serde_json::Value, String> {
    state.0.scan_directory(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_database(
    state: State<'_, AppState>,
) -> Result<ipc_contract::ClearDatabaseResponse, String> {
    Ok(state.0.clear_database(false, false))
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
    state.0.check_updates().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_deps(
    state: State<'_, AppState>,
) -> Result<ipc_contract::CheckDepsResponse, String> {
    Ok(state.0.check_deps().await)
}

#[tauri::command]
async fn install_dep(
    state: State<'_, AppState>,
    req: ipc_contract::InstallDepRequest,
) -> Result<ipc_contract::InstallDepResponse, String> {
    Ok(state.0.install_dep(req).await)
}

#[tauri::command]
async fn get_dependency_versions(
    state: State<'_, AppState>,
) -> Result<ipc_contract::GetDependencyVersionsResponse, String> {
    Ok(state.0.get_dependency_versions().await)
}

#[allow(deprecated)]
#[tauri::command]
async fn open_external(
    app: AppHandle,
    url: String,
) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    app.shell().open(&url, None).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let emitter: Arc<dyn EventEmitter> = Arc::new(TauriEmitter(app_handle));

            let user_data = app.path().app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));

            let state = tauri::async_runtime::block_on(async {
                BackendState::init(user_data, emitter).await
                    .expect("Failed to initialize MediaHarbor backend")
            });

            app.manage(AppState(Arc::new(state)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            dialog_open_folder,
            dialog_open_file,
            perform_search,
            play_media,
            pause_media,
            spotify_oauth_login,
            spotify_oauth_logout,
            spotify_oauth_status,
            spotify_get_token,
            clear_spotify_credentials,
            tidal_start_auth,
            tidal_exchange_code,
            get_artist_details,
            get_album_details,
            get_playlist_details,
            start_yt_music_download,
            start_yt_video_download,
            start_spotify_download,
            start_apple_download,
            start_qobuz_download,
            start_deezer_download,
            start_tidal_download,
            cancel_download,
            show_item_in_folder,
            scan_directory,
            clear_database,
            get_version,
            check_updates,
            check_deps,
            install_dep,
            get_dependency_versions,
            open_external,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaHarbor application");
}
