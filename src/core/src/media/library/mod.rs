pub mod covers;
pub mod db;
pub mod saved_state;
pub mod scanner;
pub mod walker;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};
use crate::ipc_contract::{LibraryChangedEvent, LibraryQueryRequest};

use covers::CoverCache;
use db::{AlbumRow, ArtistRow, LibraryDb, PlaylistRow, TrackRow};
use scanner::{scan_root, ScanContext, ScanStats};
use watcher::{start_watching, WatcherHandle};

use crate::EventEmitter;

pub struct Library {
    ctx: Arc<ScanContext>,
    data_dir: PathBuf,
    covers_dir: PathBuf,
    watcher: RwLock<Option<WatcherHandle>>,
    watch_roots: RwLock<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryAlbumDto {
    pub album_key: String,
    pub title: String,
    pub artist: String,
    pub year: Option<String>,
    pub cover_id: Option<String>,
    pub track_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_dir: Option<String>,
    #[serde(default, rename = "album_kind")]
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryTrackDto {
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_key: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub duration_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub size: i64,
    pub mtime_ns: i64,
    pub is_video: bool,
    pub cover_id: Option<String>,
    pub primary_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barcode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mb_recording_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mb_release_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyricist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conductor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engineer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rg_track_gain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rg_track_peak: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rg_album_gain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rg_album_peak: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compilation: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_lyrics: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryArtistDto {
    pub key: String,
    pub display: String,
    pub album_count: i64,
    pub track_count: i64,
    pub cover_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryPlaylistDto {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub track_count: i64,
    pub cover_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LibraryQueryItem {
    Album(LibraryAlbumDto),
    Artist(LibraryArtistDto),
    Playlist(LibraryPlaylistDto),
    Track(Box<LibraryTrackDto>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryQueryResult {
    pub items: Vec<LibraryQueryItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryAlbumDetail {
    pub album: LibraryAlbumDto,
    pub tracks: Vec<LibraryTrackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryArtistDetail {
    pub key: String,
    pub display: String,
    pub albums: Vec<LibraryAlbumDto>,
    pub tracks: Vec<LibraryTrackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryPlaylistDetail {
    pub playlist: LibraryPlaylistDto,
    pub tracks: Vec<LibraryTrackDto>,
}

impl Library {
    pub fn open(user_data: &Path, emitter: Arc<dyn EventEmitter>) -> MhResult<Arc<Self>> {
        let lib_dir = user_data.join("library");
        std::fs::create_dir_all(&lib_dir).map_err(MhError::Io)?;
        let covers_dir = lib_dir.join("covers");
        let db_path = lib_dir.join("library.sqlite");

        let db = Arc::new(LibraryDb::open(&db_path)?);
        let covers = Arc::new(CoverCache::new(covers_dir.clone()));

        let ctx = Arc::new(ScanContext {
            db,
            covers,
            emitter,
        });

        Ok(Arc::new(Self {
            ctx,
            data_dir: user_data.to_path_buf(),
            covers_dir,
            watcher: RwLock::new(None),
            watch_roots: RwLock::new(Vec::new()),
        }))
    }

    pub fn covers_dir(&self) -> &Path {
        &self.covers_dir
    }

    /// The app's own data directory. Anything that caches to disk beside the
    /// library — the radio directory mirrors — hangs off this.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn ensure_cover(&self, cover_id: &str) -> bool {
        scanner::ensure_local_cover(&self.ctx.covers, &self.ctx.db, cover_id)
    }

    pub fn db(&self) -> &Arc<db::LibraryDb> {
        &self.ctx.db
    }

    /// The shared cover cache. Anything that needs to store an image the user
    /// supplied goes through it, so one blob is kept once however many things
    /// point at it.
    pub fn covers(&self) -> &Arc<covers::CoverCache> {
        &self.ctx.covers
    }

    pub async fn scan(&self, dir: &Path, force: bool) -> MhResult<ScanStats> {
        scan_root(&self.ctx, dir, force).await
    }

    pub async fn set_watch_roots(self: &Arc<Self>, roots: Vec<PathBuf>) -> MhResult<()> {
        {
            let mut w = self.watcher.write().await;
            *w = None;
        }

        // Moving the library folder used to leave every track from the old one in the
        // database for good: scanning and pruning are both scoped to a root, so nothing
        // ever looked at the abandoned path again.
        let dropped = self.ctx.db.delete_outside_roots(&roots)?;
        if dropped > 0 {
            self.ctx.db.rebuild_albums()?;
            self.ctx.emitter.emit_library_changed(&LibraryChangedEvent {
                directory: roots
                    .first()
                    .map(|p| db::path_to_string(p))
                    .unwrap_or_default(),
                added: 0,
                updated: 0,
                removed: dropped as u64,
            });
        }

        let cleaned: Vec<PathBuf> = roots.into_iter().filter(|p| p.exists()).collect();
        if cleaned.is_empty() {
            *self.watch_roots.write().await = Vec::new();
            return Ok(());
        }
        let handle = start_watching(cleaned.clone(), self.ctx.clone())?;
        *self.watcher.write().await = Some(handle);
        *self.watch_roots.write().await = cleaned.clone();

        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            for root in cleaned {
                let _ = scanner::prune_missing(&ctx, &root).await;
            }
        });
        Ok(())
    }

    pub async fn watch_roots(&self) -> Vec<PathBuf> {
        self.watch_roots.read().await.clone()
    }

    pub fn query(&self, req: &LibraryQueryRequest) -> MhResult<LibraryQueryResult> {
        let kind = req.kind.as_deref().unwrap_or("albums");
        let offset = req.offset.unwrap_or(0).max(0);
        let limit = req.limit.unwrap_or(100).clamp(1, 500);
        let search = req.search.as_deref();
        let sort = req.sort.as_deref().unwrap_or("recent");

        match kind {
            "videos" => {
                let (rows, total) = self.ctx.db.query_videos(offset, limit, search, sort)?;
                Ok(LibraryQueryResult {
                    items: rows
                        .into_iter()
                        .map(|r| LibraryQueryItem::Track(Box::new(track_to_dto(r))))
                        .collect(),
                    total,
                })
            }
            "tracks" => {
                let (rows, total) = self.ctx.db.query_tracks(offset, limit, search, sort)?;
                Ok(LibraryQueryResult {
                    items: rows
                        .into_iter()
                        .map(|r| LibraryQueryItem::Track(Box::new(track_to_dto(r))))
                        .collect(),
                    total,
                })
            }
            "artists" => {
                let (rows, total) = self.ctx.db.query_artists(offset, limit, search, sort)?;
                Ok(LibraryQueryResult {
                    items: rows
                        .into_iter()
                        .map(|r| LibraryQueryItem::Artist(artist_to_dto(r)))
                        .collect(),
                    total,
                })
            }
            "playlists" => {
                let mut items: Vec<LibraryQueryItem> = self
                    .ctx
                    .db
                    .playlist_list()?
                    .into_iter()
                    .map(|r| LibraryQueryItem::Playlist(playlist_to_dto(r)))
                    .collect();
                items.extend(
                    self.ctx
                        .db
                        .collections()?
                        .into_iter()
                        .map(|a| LibraryQueryItem::Playlist(collection_to_playlist_dto(a))),
                );
                let total = items.len() as i64;
                Ok(LibraryQueryResult { items, total })
            }
            _ => {
                let (rows, total) = self.ctx.db.query_albums(offset, limit, search, sort)?;
                Ok(LibraryQueryResult {
                    items: rows
                        .into_iter()
                        .map(|r| LibraryQueryItem::Album(album_to_dto(r)))
                        .collect(),
                    total,
                })
            }
        }
    }

    pub fn album_detail(&self, album_key: &str) -> MhResult<Option<LibraryAlbumDetail>> {
        let Some(album) = self.ctx.db.album_by_key(album_key)? else {
            return Ok(None);
        };
        let tracks = self.ctx.db.tracks_in_album(album_key)?;
        Ok(Some(LibraryAlbumDetail {
            album: album_to_dto(album),
            tracks: tracks.into_iter().map(track_to_dto).collect(),
        }))
    }

    pub fn artist_detail(&self, key: &str) -> MhResult<Option<LibraryArtistDetail>> {
        let display = match self.ctx.db.artist_display_for_key(key)? {
            Some(d) => d,
            None => return Ok(None),
        };
        let albums = self.ctx.db.albums_for_primary_artist(key)?;
        let tracks = self.ctx.db.tracks_for_primary_artist(key)?;
        Ok(Some(LibraryArtistDetail {
            key: key.to_string(),
            display,
            albums: albums.into_iter().map(album_to_dto).collect(),
            tracks: tracks.into_iter().map(track_to_dto).collect(),
        }))
    }

    /// Rewrites one file's tags and pulls the result straight back into the library.
    ///
    /// The whole tag set is rebuilt from the stored row before the edits are applied,
    /// because the tagger writes every field it knows and would otherwise clear the ones
    /// this request does not mention. Lyrics are the exception: their body is never
    /// stored, so the field is excluded and the file keeps what it has.
    pub async fn write_tags(
        &self,
        req: &crate::ipc_contract::LibraryWriteTagsRequest,
    ) -> MhResult<()> {
        use crate::services::common::pipeline::tagger::{tag_file, TrackMetadata};

        let path = PathBuf::from(&req.path);
        let current = self
            .ctx
            .db
            .track_by_path(&req.path)?
            .ok_or_else(|| MhError::NotFound(format!("{} is not in the library", req.path)))?;

        let pick = |edit: &Option<String>, stored: &Option<String>| -> Option<String> {
            match edit {
                Some(v) if v.trim().is_empty() => None,
                Some(v) => Some(v.clone()),
                None => stored.clone(),
            }
        };
        let split = |v: Option<String>| -> Vec<String> {
            v.into_iter()
                .flat_map(|s| {
                    s.split(&[';', ','][..])
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect()
        };

        let metadata = TrackMetadata {
            title: pick(&req.title, &current.title),
            artist: split(pick(&req.artist, &current.artist)),
            album: pick(&req.album, &current.album),
            album_artist: split(pick(&req.album_artist, &current.album_artist)),
            year: pick(&req.year, &current.date.clone().or(current.year.clone())),
            original_date: current.original_date.clone(),
            genre: split(pick(&req.genre, &current.genre)),
            track_number: req.track_no.or(current.track_no),
            disc_number: req.disc_no.or(current.disc_no),
            total_tracks: req.track_total.or(current.track_total),
            total_discs: req.disc_total.or(current.disc_total),
            comment: pick(&req.comment, &current.comment),
            lyrics: None,
            isrc: pick(&req.isrc, &current.isrc),
            upc: pick(&req.barcode, &current.barcode),
            copyright: pick(&req.copyright, &current.copyright),
            label: pick(&req.label, &current.label),
            composer: split(pick(&req.composer, &current.composer)),
            conductor: split(current.conductor.clone()),
            performer: split(current.performer.clone()),
            lyricist: split(pick(&req.lyricist, &current.lyricist)),
            producer: split(pick(&req.producer, &current.producer)),
            engineer: split(current.engineer.clone()),
            mixer: split(current.mixer.clone()),
            bpm: req
                .bpm
                .map(|v| v.to_string())
                .or(current.bpm.map(|v| v.to_string())),
            replaygain_track_gain: current.rg_track_gain.clone(),
            replaygain_track_peak: current.rg_track_peak.clone(),
            replaygain_album_gain: current.rg_album_gain.clone(),
            replaygain_album_peak: current.rg_album_peak.clone(),
            description: current.description.clone(),
            purchase_date: None,
            grouping: pick(&req.grouping, &current.grouping),
        };

        tag_file(&path, &metadata, None, &["lyrics".to_string()]).await?;
        scanner::ingest_paths(
            &self.ctx,
            vec![path.clone()],
            path.parent().unwrap_or(&path),
        )
        .await
    }

    pub fn radio_from(&self, seed_path: &str) -> MhResult<Vec<LibraryTrackDto>> {
        Ok(self
            .ctx
            .db
            .radio_from(seed_path, 60)?
            .into_iter()
            .map(track_to_dto)
            .collect())
    }

    pub fn track_gain(&self, path: &str) -> MhResult<Option<db::TrackGain>> {
        self.ctx.db.track_gain(path)
    }

    pub fn record_play(&self, path: &str) -> MhResult<()> {
        self.ctx.db.record_play(path)
    }

    pub fn playlist_list(&self) -> MhResult<Vec<LibraryPlaylistDto>> {
        Ok(self
            .ctx
            .db
            .playlist_list()?
            .into_iter()
            .map(playlist_to_dto)
            .collect())
    }

    pub fn playlist_create(&self, name: &str) -> MhResult<i64> {
        self.ctx.db.playlist_create(name.trim())
    }

    pub fn playlist_rename(&self, id: i64, name: &str) -> MhResult<()> {
        self.ctx.db.playlist_rename(id, name.trim())
    }

    pub fn playlist_delete(&self, id: i64) -> MhResult<()> {
        self.ctx.db.playlist_delete(id)
    }

    pub fn playlist_get(&self, id: i64) -> MhResult<Option<LibraryPlaylistDetail>> {
        let Some((row, tracks)) = self.ctx.db.playlist_get(id)? else {
            return Ok(None);
        };
        Ok(Some(LibraryPlaylistDetail {
            playlist: playlist_to_dto(row),
            tracks: tracks.into_iter().map(track_to_dto).collect(),
        }))
    }

    pub fn playlist_add_tracks(&self, id: i64, paths: &[String]) -> MhResult<()> {
        self.ctx.db.playlist_add_tracks(id, paths)
    }

    pub fn playlist_remove_at(&self, id: i64, position: i64) -> MhResult<()> {
        self.ctx.db.playlist_remove_at(id, position)
    }

    pub fn playlist_reorder(&self, id: i64, from: i64, to: i64) -> MhResult<()> {
        self.ctx.db.playlist_reorder(id, from, to)
    }

    pub fn playlist_import_m3u(&self, source: &Path) -> MhResult<i64> {
        let contents = std::fs::read(source).map_err(MhError::Io)?;
        let text = decode_m3u(&contents);
        let base = source.parent();
        let name = source
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Imported".to_string());
        let id = self.ctx.db.playlist_create(&name)?;

        let mut paths: Vec<String> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let candidate = PathBuf::from(line);
            let resolved = if candidate.is_absolute() && candidate.exists() {
                Some(candidate.clone())
            } else if let Some(b) = base {
                let joined = b.join(line);
                if joined.exists() {
                    Some(joined)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(p) = resolved {
                paths.push(db::path_to_string(&p));
            } else if let Some(fname) = candidate.file_name().and_then(|s| s.to_str()) {
                if let Ok(Some(found)) = self.ctx.db.find_track_path_by_filename(fname) {
                    paths.push(found);
                }
            }
        }

        if !paths.is_empty() {
            self.ctx.db.playlist_add_tracks(id, &paths)?;
        }
        Ok(id)
    }

    pub fn playlist_export_m3u(&self, id: i64, dest: &Path) -> MhResult<()> {
        let Some((_, tracks)) = self.ctx.db.playlist_get(id)? else {
            return Err(MhError::NotFound("Playlist not found".into()));
        };
        let mut out = String::from("#EXTM3U\n");
        for t in tracks {
            let dur = t.duration_secs.map(|d| d.round() as i64).unwrap_or(-1);
            let artist = t.artist.clone().unwrap_or_default();
            let title = t.title.clone().unwrap_or_default();
            out.push_str(&format!("#EXTINF:{},{} - {}\n", dur, artist, title));
            out.push_str(&t.path);
            out.push('\n');
        }
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(MhError::Io)?;
            }
        }
        std::fs::write(dest, out).map_err(MhError::Io)?;
        Ok(())
    }

    pub fn cover_path(&self, cover_id: &str) -> PathBuf {
        self.covers_dir
            .join(format!("{}.jpg", sanitize_id(cover_id)))
    }
}

pub trait CoverMaterializer: Send + Sync {
    fn ensure(&self, cover_id: &str) -> bool;
}

impl CoverMaterializer for Library {
    fn ensure(&self, cover_id: &str) -> bool {
        self.ensure_cover(cover_id)
    }
}

fn album_to_dto(a: AlbumRow) -> LibraryAlbumDto {
    LibraryAlbumDto {
        album_key: a.album_key,
        title: a.title,
        artist: a.artist,
        year: a.year,
        cover_id: a.cover_id,
        track_count: a.track_count,
        artist_id: None,
        year_end: a.year_end,
        disc_total: a.disc_total,
        track_total: a.track_total,
        codec: a.codec,
        bit_depth: a.bit_depth,
        sample_rate: a.sample_rate,
        release_dir: a.release_dir,
        kind: a.kind,
    }
}

fn artist_to_dto(a: ArtistRow) -> LibraryArtistDto {
    LibraryArtistDto {
        key: a.key,
        display: a.display,
        album_count: a.album_count,
        track_count: a.track_count,
        cover_id: a.cover_id,
    }
}

/// A folder-backed collection wearing the playlist shape, so it lists beside the user's
/// own playlists. `service_id` carries the album key the detail view opens with — a
/// collection has no row in the `playlists` table and cannot be renamed or deleted.
fn collection_to_playlist_dto(a: AlbumRow) -> LibraryPlaylistDto {
    LibraryPlaylistDto {
        id: 0,
        name: a.title,
        created_at: 0,
        updated_at: 0,
        track_count: a.track_count,
        cover_ids: a.cover_id.into_iter().collect(),
        service_id: Some(a.album_key),
        description: None,
        owner: Some(a.artist),
    }
}

fn playlist_to_dto(p: PlaylistRow) -> LibraryPlaylistDto {
    LibraryPlaylistDto {
        id: p.id,
        name: p.name,
        created_at: p.created_at,
        updated_at: p.updated_at,
        track_count: p.track_count,
        cover_ids: p.cover_ids,
        service_id: None,
        description: None,
        owner: None,
    }
}

fn track_to_dto(t: TrackRow) -> LibraryTrackDto {
    LibraryTrackDto {
        path: t.path,
        title: t.title,
        artist: t.artist,
        album: t.album,
        album_key: t.album_key,
        album_artist: t.album_artist,
        year: t.year,
        genre: t.genre,
        duration_secs: t.duration_secs,
        track_no: t.track_no,
        disc_no: t.disc_no,
        size: t.size,
        mtime_ns: t.mtime_ns,
        is_video: t.is_video,
        cover_id: t.cover_id,
        primary_artist: t.primary_artist,
        date: t.date,
        original_date: t.original_date,
        isrc: t.isrc,
        barcode: t.barcode,
        mb_recording_id: t.mb_recording_id,
        mb_release_id: t.mb_release_id,
        composer: t.composer,
        lyricist: t.lyricist,
        producer: t.producer,
        conductor: t.conductor,
        performer: t.performer,
        engineer: t.engineer,
        mixer: t.mixer,
        label: t.label,
        copyright: t.copyright,
        comment: t.comment,
        grouping: t.grouping,
        description: t.description,
        codec: t.codec,
        rg_track_gain: t.rg_track_gain,
        rg_track_peak: t.rg_track_peak,
        rg_album_gain: t.rg_album_gain,
        rg_album_peak: t.rg_album_peak,
        release_dir: t.release_dir,
        track_total: t.track_total,
        disc_total: t.disc_total,
        bitrate: t.bitrate,
        sample_rate: t.sample_rate,
        bit_depth: t.bit_depth,
        channels: t.channels,
        bpm: t.bpm,
        added_at: t.added_at,
        play_count: t.play_count,
        last_played_at: t.last_played_at,
        compilation: t.compilation,
        has_lyrics: t.has_lyrics,
        ..Default::default()
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(32)
        .collect()
}

fn decode_m3u(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    bytes.iter().map(|&b| b as char).collect()
}
