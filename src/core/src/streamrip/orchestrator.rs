use std::path::{Path, PathBuf};
use regex::Regex;

use crate::errors::{MhError, MhResult};
use crate::defaults::Settings;
use crate::streamrip::deezer_client::DeezerClient;
use crate::streamrip::qobuz_client::{QobuzClient, ArtistFilters};
use crate::streamrip::tidal_client::TidalClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Deezer,
    Qobuz,
    Tidal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Track,
    Album,
    Playlist,
    Artist,
    Label,
    Video,
}

fn extract_id(url: &str, patterns: &[Regex]) -> Option<String> {
    for re in patterns {
        if let Some(caps) = re.captures(url) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }
    }
    None
}

pub fn detect_platform_and_type(url: &str) -> Option<(Platform, ContentType)> {
    let platform = if url.contains("deezer.com") {
        Platform::Deezer
    } else if url.contains("qobuz.com") || url.contains("play.qobuz.com") {
        Platform::Qobuz
    } else if url.contains("tidal.com") {
        Platform::Tidal
    } else {
        return None;
    };

    let content_type = if url.contains("/video/") {
        ContentType::Video
    } else if url.contains("/artist/") || url.contains("/interpreter/") {
        ContentType::Artist
    } else if url.contains("/label/") {
        ContentType::Label
    } else if url.contains("/track/") {
        ContentType::Track
    } else if url.contains("/album/") {
        ContentType::Album
    } else if url.contains("/playlist/") {
        ContentType::Playlist
    } else if regex::Regex::new(r"^\d+$").ok()?.is_match(url) {
        ContentType::Track
    } else {
        return None;
    };

    Some((platform, content_type))
}

fn extract_deezer_id(url: &str, content_type: ContentType) -> Option<String> {
    let patterns: &[&str] = match content_type {
        ContentType::Track => &[
            r"deezer\.com/(?:[a-z]{2}/)?track/(\d+)",
            r"^(\d+)$",
        ],
        ContentType::Album => &[r"deezer\.com/(?:[a-z]{2}/)?album/(\d+)"],
        ContentType::Playlist => &[r"deezer\.com/(?:[a-z]{2}/)?playlist/(\d+)"],
        ContentType::Artist => &[r"deezer\.com/(?:[a-z]{2}/)?artist/(\d+)"],
        ContentType::Label => &[],
        ContentType::Video => &[],
    };
    let compiled: Vec<Regex> = patterns.iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();
    extract_id(url, &compiled)
}

fn extract_qobuz_id(url: &str, content_type: ContentType) -> Option<String> {
    let patterns: &[&str] = match content_type {
        ContentType::Track => &[
            r"play\.qobuz\.com/track/(\w+)",
            r"qobuz\.com/[a-z-]+/album/[^/]+/(\d+)",
            r"^(\d+)$",
        ],
        ContentType::Album => &[
            r"play\.qobuz\.com/album/(\w+)",
            r"qobuz\.com/[a-z-]+/album/[^/]+/(\w+)",
            r"^(\w+)$",
        ],
        ContentType::Playlist => &[
            r"play\.qobuz\.com/playlist/(\d+)",
            r"qobuz\.com/[a-z-]+/playlist/[^/]+/(\d+)",
            r"^(\d+)$",
        ],
        ContentType::Artist => &[
            r"qobuz\.com/[a-z-]+/interpreter/[^/]+/(\d+)",
            r"qobuz\.com/[a-z-]+/artist/(\d+)",
        ],
        ContentType::Label => &[
            r"qobuz\.com/[a-z-]+/label/[^/]+/(\d+)",
        ],
        ContentType::Video => &[],
    };
    let compiled: Vec<Regex> = patterns.iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();
    extract_id(url, &compiled)
}

fn extract_tidal_id(url: &str, content_type: ContentType) -> Option<String> {
    let patterns: &[&str] = match content_type {
        ContentType::Track => &[
            r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?track/(\d+)",
            r"^(\d+)$",
        ],
        ContentType::Album => &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?album/(\d+)"],
        ContentType::Playlist => &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?playlist/([a-z0-9-]+)"],
        ContentType::Artist => &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?artist/(\d+)"],
        ContentType::Video => &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?video/(\d+)"],
        ContentType::Label => &[],
    };
    let compiled: Vec<Regex> = patterns.iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();
    extract_id(url, &compiled)
}

pub fn extract_platform_id(url: &str, platform: Platform, content_type: ContentType) -> Option<String> {
    match platform {
        Platform::Deezer => extract_deezer_id(url, content_type),
        Platform::Qobuz => extract_qobuz_id(url, content_type),
        Platform::Tidal => extract_tidal_id(url, content_type),
    }
}

use super::build_album_folder;

fn qobuz_filters_from_settings(settings: &Settings) -> ArtistFilters {
    ArtistFilters {
        extras: settings.qobuz_filters_extras,
        repeats: settings.qobuz_repeats,
        non_albums: settings.qobuz_non_albums,
        features: settings.qobuz_features,
        non_studio_albums: settings.qobuz_non_studio_albums,
        non_remaster: settings.qobuz_non_remaster,
    }
}

pub type ProgressFn = Box<dyn Fn(u64, u64) + Send + Sync>;

fn divided_progress<F>(track_index: usize, track_total: usize, parent: F) -> impl Fn(u64, u64) + Send + 'static
where
    F: Fn(u64, u64) + Send + 'static,
{
    let scale: u64 = 1_000_000;
    let n = track_total.max(1) as u64;
    let offset = track_index as u64;
    move |done, total| {
        let track_frac = if total > 0 { done * scale / total } else { 0 };
        let overall = (offset * scale + track_frac) / n;
        parent(overall, scale);
    }
}

pub struct StreamripClients {
    pub deezer: Option<DeezerClient>,
    pub qobuz: Option<QobuzClient>,
    pub tidal: Option<TidalClient>,
}

pub async fn download_url(
    url: &str,
    settings: &Settings,
    clients: &StreamripClients,
    on_progress: impl Fn(u64, u64) + Clone + Send + 'static,
    on_log: impl Fn(String) + Clone + Send + 'static,
) -> MhResult<()> {
    let (platform, content_type) = detect_platform_and_type(url)
        .ok_or_else(|| MhError::Other(format!("Could not detect platform/type for URL: {}", url)))?;

    on_log(format!("Detected: {:?} {:?}", platform, content_type));

    let base_dir = if settings.create_platform_subfolders {
        let label = match platform {
            Platform::Deezer => "Deezer",
            Platform::Qobuz => "Qobuz",
            Platform::Tidal => "Tidal",
        };
        PathBuf::from(&settings.download_location).join(label)
    } else {
        PathBuf::from(&settings.download_location)
    };
    tokio::fs::create_dir_all(&base_dir).await?;

    match platform {
        Platform::Deezer => {
            let client = clients.deezer.as_ref()
                .ok_or_else(|| MhError::Auth("Deezer client not initialized".into()))?;
            download_deezer(client, url, content_type, &base_dir, settings, on_progress, on_log).await?;
        }
        Platform::Qobuz => {
            let client = clients.qobuz.as_ref()
                .ok_or_else(|| MhError::Auth("Qobuz client not initialized".into()))?;
            download_qobuz(client, url, content_type, &base_dir, settings, on_progress, on_log).await?;
        }
        Platform::Tidal => {
            let client = clients.tidal.as_ref()
                .ok_or_else(|| MhError::Auth("Tidal client not initialized".into()))?;
            download_tidal(client, url, content_type, &base_dir, settings, on_progress, on_log).await?;
        }
    }

    Ok(())
}

async fn download_deezer(
    client: &DeezerClient,
    url: &str,
    content_type: ContentType,
    base_dir: &Path,
    settings: &Settings,
    on_progress: impl Fn(u64, u64) + Clone + Send + 'static,
    on_log: impl Fn(String) + Clone + Send + 'static,
) -> MhResult<()> {
    match content_type {
        ContentType::Track => {
            let id = extract_deezer_id(url, ContentType::Track)
                .ok_or_else(|| MhError::Other(format!("Could not extract Deezer track ID from: {}", url)))?;
            on_log(format!("Track: {}", id));
            tokio::fs::create_dir_all(base_dir).await?;
            client.download_track(&id, base_dir, settings, on_progress, false).await?;
        }
        ContentType::Album => {
            let id = extract_deezer_id(url, ContentType::Album)
                .ok_or_else(|| MhError::Other(format!("Could not extract Deezer album ID from: {}", url)))?;
            let album_info = client.get_album_tracks(&id).await?;
            let dest_dir = make_collection_dir(base_dir, settings, &album_info.title, &album_info.artist, &album_info.year, &album_info.genre, &album_info.label);
            tokio::fs::create_dir_all(&dest_dir).await?;
            let total = album_info.track_ids.len();
            on_log(format!("Album: {} — {} ({} tracks)", album_info.title, album_info.artist, total));
            let use_disc_dirs = settings.disc_subdirectories
                && (album_info.number_of_volumes > 1
                    || album_info.track_disc_numbers.iter().any(|&d| d > 1));
            for (i, track_id) in album_info.track_ids.iter().enumerate() {
                let disc = album_info.track_disc_numbers.get(i).copied().unwrap_or(1);
                let track_dir = if use_disc_dirs {
                    let d = dest_dir.join(format!("Disc {}", disc));
                    let _ = tokio::fs::create_dir_all(&d).await;
                    d
                } else {
                    dest_dir.clone()
                };
                on_log(format!("Track {}/{}: {}", i + 1, total, track_id));
                let p = divided_progress(i, total, on_progress.clone());
                if let Err(e) = client.download_track(track_id, &track_dir, settings, p, true).await {
                    on_log(format!("  failed: {}", e));
                }
            }
        }
        ContentType::Playlist => {
            let id = extract_deezer_id(url, ContentType::Playlist)
                .ok_or_else(|| MhError::Other(format!("Could not extract Deezer playlist ID from: {}", url)))?;
            let info = client.get_playlist_tracks(&id).await?;
            let dest_dir = make_collection_dir(base_dir, settings, &info.title, &info.artist, "", "", "");
            tokio::fs::create_dir_all(&dest_dir).await?;
            let total = info.track_ids.len();
            on_log(format!("Playlist: {} ({} tracks)", info.title, total));
            for (i, track_id) in info.track_ids.iter().enumerate() {
                on_log(format!("Track {}/{}: {}", i + 1, total, track_id));
                let p = divided_progress(i, total, on_progress.clone());
                if let Err(e) = client.download_track(track_id, &dest_dir, settings, p, true).await {
                    on_log(format!("  failed: {}", e));
                }
            }
        }
        ContentType::Artist => {
            let id = extract_deezer_id(url, ContentType::Artist)
                .ok_or_else(|| MhError::Other(format!("Could not extract Deezer artist ID from: {}", url)))?;
            let album_ids = client.get_artist_albums(&id).await?;
            on_log(format!("Artist: {} albums", album_ids.len()));
            for album_id in &album_ids {
                let album_url = format!("https://www.deezer.com/album/{}", album_id);
                let _ = Box::pin(download_deezer(client, &album_url, ContentType::Album, base_dir, settings, on_progress.clone(), on_log.clone())).await;
            }
        }
        ContentType::Label => {
            return Err(MhError::Unsupported("Deezer does not support label downloads.".into()));
        }
        ContentType::Video => {
            return Err(MhError::Unsupported("Deezer does not support video downloads.".into()));
        }
    }
    Ok(())
}

async fn download_qobuz(
    client: &QobuzClient,
    url: &str,
    content_type: ContentType,
    base_dir: &Path,
    settings: &Settings,
    on_progress: impl Fn(u64, u64) + Clone + Send + 'static,
    on_log: impl Fn(String) + Clone + Send + 'static,
) -> MhResult<()> {
    let quality = settings.qobuz_quality;
    match content_type {
        ContentType::Track => {
            let id = extract_qobuz_id(url, ContentType::Track)
                .ok_or_else(|| MhError::Other(format!("Could not extract Qobuz track ID from: {}", url)))?;
            on_log(format!("Track: {}", id));
            tokio::fs::create_dir_all(base_dir).await?;
            client.download_track(&id, quality, base_dir, settings, on_progress, false).await?;
        }
        ContentType::Album => {
            let id = extract_qobuz_id(url, ContentType::Album)
                .ok_or_else(|| MhError::Other(format!("Could not extract Qobuz album ID from: {}", url)))?;
            let album_info = client.get_album_tracks(&id).await?;
            let dest_dir = make_collection_dir(base_dir, settings, &album_info.title, &album_info.artist, &album_info.year, &album_info.genre, &album_info.label);
            tokio::fs::create_dir_all(&dest_dir).await?;
            let total = album_info.track_ids.len();
            on_log(format!("Album: {} — {} ({} tracks)", album_info.title, album_info.artist, total));
            for (i, track_id) in album_info.track_ids.iter().enumerate() {
                on_log(format!("Track {}/{}: {}", i + 1, total, track_id));
                let p = divided_progress(i, total, on_progress.clone());
                if let Err(e) = client.download_track(track_id, quality, &dest_dir, settings, p, true).await {
                    on_log(format!("  failed: {}", e));
                }
            }
            if settings.qobuz_download_booklets {
                let _ = client.download_booklet(&id, &dest_dir).await;
            }
        }
        ContentType::Playlist => {
            let id = extract_qobuz_id(url, ContentType::Playlist)
                .ok_or_else(|| MhError::Other(format!("Could not extract Qobuz playlist ID from: {}", url)))?;
            let info = client.get_playlist_tracks(&id).await?;
            let dest_dir = make_collection_dir(base_dir, settings, &info.title, &info.artist, "", "", "");
            tokio::fs::create_dir_all(&dest_dir).await?;
            let total = info.track_ids.len();
            on_log(format!("Playlist: {} ({} tracks)", info.title, total));
            for (i, track_id) in info.track_ids.iter().enumerate() {
                on_log(format!("Track {}/{}: {}", i + 1, total, track_id));
                let p = divided_progress(i, total, on_progress.clone());
                if let Err(e) = client.download_track(track_id, quality, &dest_dir, settings, p, true).await {
                    on_log(format!("  failed: {}", e));
                }
            }
        }
        ContentType::Artist => {
            let id = extract_qobuz_id(url, ContentType::Artist)
                .ok_or_else(|| MhError::Other(format!("Could not extract Qobuz artist ID from: {}", url)))?;
            let filters = qobuz_filters_from_settings(settings);
            let album_ids = client.get_artist_albums(&id, &filters).await?;
            on_log(format!("Artist: {} albums", album_ids.len()));
            for album_id in &album_ids {
                let album_url = format!("https://play.qobuz.com/album/{}", album_id);
                let _ = Box::pin(download_qobuz(client, &album_url, ContentType::Album, base_dir, settings, on_progress.clone(), on_log.clone())).await;
            }
        }
        ContentType::Label => {
            let id = extract_qobuz_id(url, ContentType::Label)
                .ok_or_else(|| MhError::Other(format!("Could not extract Qobuz label ID from: {}", url)))?;
            let album_ids = client.get_label_albums(&id).await?;
            on_log(format!("Label: {} albums", album_ids.len()));
            for album_id in &album_ids {
                let album_url = format!("https://play.qobuz.com/album/{}", album_id);
                let _ = Box::pin(download_qobuz(client, &album_url, ContentType::Album, base_dir, settings, on_progress.clone(), on_log.clone())).await;
            }
        }
        ContentType::Video => {
            return Err(MhError::Unsupported("Qobuz does not support video downloads.".into()));
        }
    }
    Ok(())
}

async fn download_tidal(
    client: &TidalClient,
    url: &str,
    content_type: ContentType,
    base_dir: &Path,
    settings: &Settings,
    on_progress: impl Fn(u64, u64) + Clone + Send + 'static,
    on_log: impl Fn(String) + Clone + Send + 'static,
) -> MhResult<()> {
    let quality = settings.tidal_quality;
    match content_type {
        ContentType::Track => {
            let id = extract_tidal_id(url, ContentType::Track)
                .ok_or_else(|| MhError::Other(format!("Could not extract Tidal track ID from: {}", url)))?;
            on_log(format!("Track: {}", id));
            tokio::fs::create_dir_all(base_dir).await?;
            client.download_track(&id, quality, base_dir, settings, on_progress, false).await?;
        }
        ContentType::Album => {
            let id = extract_tidal_id(url, ContentType::Album)
                .ok_or_else(|| MhError::Other(format!("Could not extract Tidal album ID from: {}", url)))?;
            let album_info = client.get_album_tracks(&id).await?;
            let dest_dir = make_collection_dir(base_dir, settings, &album_info.title, &album_info.artist, &album_info.year, &album_info.genre, &album_info.label);
            tokio::fs::create_dir_all(&dest_dir).await?;
            let total = album_info.track_ids.len();
            on_log(format!("Album: {} — {} ({} tracks)", album_info.title, album_info.artist, total));
            let use_disc_dirs = settings.disc_subdirectories
                && (album_info.number_of_volumes > 1
                    || album_info.track_disc_numbers.iter().any(|&d| d > 1));
            for (i, track_id) in album_info.track_ids.iter().enumerate() {
                let disc_num = album_info.track_disc_numbers.get(i).copied().unwrap_or(1);
                let track_dir = if use_disc_dirs {
                    let d = dest_dir.join(format!("Disc {}", disc_num));
                    let _ = tokio::fs::create_dir_all(&d).await;
                    d
                } else {
                    dest_dir.clone()
                };
                on_log(format!("Track {}/{}: {}", i + 1, total, track_id));
                let p = divided_progress(i, total, on_progress.clone());
                if let Err(e) = client.download_track(track_id, quality, &track_dir, settings, p, true).await {
                    on_log(format!("  failed: {}", e));
                }
            }
        }
        ContentType::Playlist => {
            let id = extract_tidal_id(url, ContentType::Playlist)
                .ok_or_else(|| MhError::Other(format!("Could not extract Tidal playlist ID from: {}", url)))?;
            let info = client.get_playlist_tracks(&id).await?;
            let dest_dir = make_collection_dir(base_dir, settings, &info.title, &info.artist, "", "", "");
            tokio::fs::create_dir_all(&dest_dir).await?;
            let total = info.track_ids.len();
            on_log(format!("Playlist: {} ({} tracks)", info.title, total));
            for (i, track_id) in info.track_ids.iter().enumerate() {
                on_log(format!("Track {}/{}: {}", i + 1, total, track_id));
                let p = divided_progress(i, total, on_progress.clone());
                if let Err(e) = client.download_track(track_id, quality, &dest_dir, settings, p, true).await {
                    on_log(format!("  failed: {}", e));
                }
            }
        }
        ContentType::Artist => {
            let id = extract_tidal_id(url, ContentType::Artist)
                .ok_or_else(|| MhError::Other(format!("Could not extract Tidal artist ID from: {}", url)))?;
            let album_ids = client.get_artist_albums(&id).await?;
            on_log(format!("Artist: {} albums", album_ids.len()));
            for album_id in &album_ids {
                let album_url = format!("https://tidal.com/browse/album/{}", album_id);
                let _ = Box::pin(download_tidal(client, &album_url, ContentType::Album, base_dir, settings, on_progress.clone(), on_log.clone())).await;
            }
        }
        ContentType::Video => {
            let id = extract_tidal_id(url, ContentType::Video)
                .ok_or_else(|| MhError::Other(format!("Could not extract Tidal video ID from: {}", url)))?;
            on_log(format!("Video: {}", id));
            tokio::fs::create_dir_all(base_dir).await?;
            client.download_video(&id, base_dir, "ffmpeg", on_progress).await?;
        }
        ContentType::Label => {
            return Err(MhError::Unsupported("Tidal does not support label downloads.".into()));
        }
    }
    Ok(())
}

fn make_collection_dir(
    base: &Path,
    settings: &Settings,
    title: &str,
    artist: &str,
    year: &str,
    genre: &str,
    label: &str,
) -> PathBuf {
    let folder_name = build_album_folder(&settings.filepaths_folder_format, artist, title, year, genre, label);
    base.join(folder_name)
}

pub async fn download_from_file(
    path: &Path,
    settings: &Settings,
    clients: &StreamripClients,
    on_progress: impl Fn(u64, u64) + Clone + Send + 'static,
    on_log: impl Fn(String) + Clone + Send + 'static,
) -> MhResult<()> {
    let content = tokio::fs::read_to_string(path).await
        .map_err(|e| MhError::Io(e))?;

    let urls: Vec<&str> = content.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    for url in urls {
        let p = on_progress.clone();
        if let Err(e) = download_url(url, settings, clients, p, on_log.clone()).await {
            tracing::warn!("download_from_file: failed for {}: {}", url, e);
        }
    }

    Ok(())
}
