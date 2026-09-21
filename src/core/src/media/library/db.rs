use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{named_params, params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::errors::MhResult;

const USER_VERSION: i64 = 15;

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS tracks (
  path           TEXT PRIMARY KEY,
  parent_dir     TEXT NOT NULL,
  size           INTEGER NOT NULL,
  mtime_ns       INTEGER NOT NULL,
  is_video       INTEGER NOT NULL,
  title          TEXT,
  artist         TEXT,
  album          TEXT,
  album_artist   TEXT,
  album_key      TEXT,
  year           TEXT,
  genre          TEXT,
  duration_secs  REAL,
  track_no       INTEGER,
  disc_no        INTEGER,
  cover_id       TEXT,
  scanned_at     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tracks_album_key ON tracks(album_key);
CREATE INDEX IF NOT EXISTS idx_tracks_parent    ON tracks(parent_dir);
CREATE INDEX IF NOT EXISTS idx_tracks_artist    ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_is_video  ON tracks(is_video);

CREATE TABLE IF NOT EXISTS albums (
  album_key      TEXT PRIMARY KEY,
  title          TEXT NOT NULL,
  artist         TEXT NOT NULL,
  year           TEXT,
  cover_id       TEXT,
  track_count    INTEGER NOT NULL DEFAULT 0,
  updated_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

const SCHEMA_V2_ADD: &str = r#"
CREATE INDEX IF NOT EXISTS idx_tracks_primary_artist ON tracks(primary_artist);

CREATE TABLE IF NOT EXISTS playlists (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
  playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  track_path  TEXT NOT NULL,
  position    INTEGER NOT NULL,
  PRIMARY KEY (playlist_id, position)
);

CREATE INDEX IF NOT EXISTS idx_pt_track ON playlist_tracks(track_path);
"#;

const SCHEMA_V11_ADD: &str = r#"
CREATE TABLE IF NOT EXISTS service_saved_state (
  platform    TEXT NOT NULL,
  kind        TEXT NOT NULL,
  service_id  TEXT NOT NULL,
  library_id  TEXT,
  saved_at    INTEGER NOT NULL,
  fetched_at  INTEGER NOT NULL,
  PRIMARY KEY (platform, kind, service_id)
);
CREATE INDEX IF NOT EXISTS idx_sss_platform_kind ON service_saved_state(platform, kind);
CREATE INDEX IF NOT EXISTS idx_sss_lib_id        ON service_saved_state(platform, kind, library_id);

CREATE TABLE IF NOT EXISTS service_state_sync (
  platform           TEXT NOT NULL,
  kind               TEXT NOT NULL,
  last_full_refresh  INTEGER NOT NULL DEFAULT 0,
  etag               TEXT,
  PRIMARY KEY (platform, kind)
);

CREATE TABLE IF NOT EXISTS service_playlist_owned (
  platform     TEXT NOT NULL,
  service_id   TEXT NOT NULL,
  name         TEXT NOT NULL,
  cover_id     TEXT,
  track_count  INTEGER NOT NULL DEFAULT 0,
  updated_at   INTEGER NOT NULL,
  PRIMARY KEY (platform, service_id)
);
CREATE INDEX IF NOT EXISTS idx_spo_platform ON service_playlist_owned(platform);
"#;

const DOWNLOADED_TRACKS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS downloaded_tracks (
  platform       TEXT NOT NULL,
  track_id       TEXT NOT NULL,
  path           TEXT NOT NULL,
  quality_rank   INTEGER NOT NULL DEFAULT 0,
  downloaded_at  INTEGER NOT NULL,
  PRIMARY KEY (platform, track_id)
);
"#;

/// Columns added in v12, when the scanner started reading back the whole tag set
/// `tagger.rs` writes instead of ten fields of it. SQLite takes one column per
/// `ALTER TABLE`, so they are applied from a list rather than a batch.
const SCHEMA_V12_COLUMNS: &[(&str, &str)] = &[
    ("release_dir", "TEXT"),
    ("track_total", "INTEGER"),
    ("disc_total", "INTEGER"),
    ("date", "TEXT"),
    ("original_date", "TEXT"),
    ("compilation", "INTEGER"),
    ("isrc", "TEXT"),
    ("barcode", "TEXT"),
    ("mb_recording_id", "TEXT"),
    ("mb_release_id", "TEXT"),
    ("composer", "TEXT"),
    ("lyricist", "TEXT"),
    ("producer", "TEXT"),
    ("conductor", "TEXT"),
    ("performer", "TEXT"),
    ("engineer", "TEXT"),
    ("mixer", "TEXT"),
    ("label", "TEXT"),
    ("copyright", "TEXT"),
    ("comment", "TEXT"),
    ("grouping", "TEXT"),
    ("description", "TEXT"),
    ("codec", "TEXT"),
    ("bitrate", "INTEGER"),
    ("sample_rate", "INTEGER"),
    ("bit_depth", "INTEGER"),
    ("channels", "INTEGER"),
    ("bpm", "INTEGER"),
    ("rg_track_gain", "TEXT"),
    ("rg_track_peak", "TEXT"),
    ("rg_album_gain", "TEXT"),
    ("rg_album_peak", "TEXT"),
    ("has_lyrics", "INTEGER"),
    ("added_at", "INTEGER"),
    ("play_count", "INTEGER"),
    ("last_played_at", "INTEGER"),
    ("skip_count", "INTEGER"),
];

const SCHEMA_V12_ADD: &str = r#"
CREATE INDEX IF NOT EXISTS idx_tracks_release_dir ON tracks(release_dir);
CREATE INDEX IF NOT EXISTS idx_tracks_isrc        ON tracks(isrc);
CREATE INDEX IF NOT EXISTS idx_tracks_added_at    ON tracks(added_at);

DROP TABLE IF EXISTS albums;
CREATE TABLE albums (
  album_key      TEXT PRIMARY KEY,
  title          TEXT NOT NULL,
  artist         TEXT NOT NULL,
  year           TEXT,
  year_end       TEXT,
  cover_id       TEXT,
  track_count    INTEGER NOT NULL DEFAULT 0,
  disc_total     INTEGER,
  track_total    INTEGER,
  codec          TEXT,
  bit_depth      INTEGER,
  sample_rate    INTEGER,
  release_dir    TEXT,
  kind           TEXT NOT NULL DEFAULT 'album',
  added_at       INTEGER,
  updated_at     INTEGER NOT NULL
);
"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackRow {
    pub path: String,
    pub parent_dir: String,
    pub size: i64,
    pub mtime_ns: i64,
    pub is_video: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub album_key: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub duration_secs: Option<f64>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub cover_id: Option<String>,
    pub primary_artist: Option<String>,
    pub release_dir: Option<String>,
    pub track_total: Option<u32>,
    pub disc_total: Option<u32>,
    pub date: Option<String>,
    pub original_date: Option<String>,
    pub compilation: Option<bool>,
    pub isrc: Option<String>,
    pub barcode: Option<String>,
    pub mb_recording_id: Option<String>,
    pub mb_release_id: Option<String>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub producer: Option<String>,
    pub conductor: Option<String>,
    pub performer: Option<String>,
    pub engineer: Option<String>,
    pub mixer: Option<String>,
    pub label: Option<String>,
    pub copyright: Option<String>,
    pub comment: Option<String>,
    pub grouping: Option<String>,
    pub description: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    pub bpm: Option<u32>,
    pub rg_track_gain: Option<String>,
    pub rg_track_peak: Option<String>,
    pub rg_album_gain: Option<String>,
    pub rg_album_peak: Option<String>,
    pub has_lyrics: bool,
    pub added_at: Option<i64>,
    pub play_count: Option<i64>,
    pub last_played_at: Option<i64>,
    pub skip_count: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlbumRow {
    pub album_key: String,
    pub title: String,
    pub artist: String,
    pub year: Option<String>,
    pub year_end: Option<String>,
    pub cover_id: Option<String>,
    pub track_count: i64,
    pub disc_total: Option<u32>,
    pub track_total: Option<u32>,
    pub codec: Option<String>,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    pub release_dir: Option<String>,
    /// `album` for a release whose tracks agree on an album tag; `collection` for a
    /// folder holding one artist's tracks drawn from many releases — a downloaded
    /// playlist or best-of. Grouping those is right; calling them albums is not.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistRow {
    pub key: String,
    pub display: String,
    pub album_count: i64,
    pub track_count: i64,
    pub cover_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistRow {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub track_count: i64,
    pub cover_ids: Vec<String>,
}

/// ReplayGain as stored: the tag values are strings like `-11.01 dB`.
#[derive(Debug, Clone, Default)]
pub struct TrackGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

fn parse_db(v: Option<String>) -> Option<f32> {
    v.as_deref()?
        .split_whitespace()
        .next()?
        .parse::<f32>()
        .ok()
        .filter(|f| f.is_finite())
}

pub struct LibraryDb {
    pub(super) conn: Mutex<Connection>,
}

impl LibraryDb {
    pub fn open(path: &Path) -> MhResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", -20000i64)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "wal_autocheckpoint", 1000i64)?;
        conn.pragma_update(None, "mmap_size", 0i64)?;

        conn.execute_batch(SCHEMA_V1)?;
        conn.execute_batch(DOWNLOADED_TRACKS_SCHEMA)?;
        conn.execute_batch(crate::services::radio::store::SCHEMA)?;

        let cur: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);

        if cur < USER_VERSION {
            let has_pa: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('tracks') WHERE name='primary_artist'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if has_pa == 0 {
                conn.execute("ALTER TABLE tracks ADD COLUMN primary_artist TEXT", [])?;
            }
            conn.execute_batch(SCHEMA_V2_ADD)?;
            backfill_primary_artist(&conn)?;
            conn.execute_batch(SCHEMA_V11_ADD)?;
            migrate_to_v12(&conn, cur)?;
            migrate_to_v14(&conn)?;
            migrate_to_v15(&conn)?;
            rebuild_albums_inline(&conn, None)?;
            conn.pragma_update(None, "user_version", USER_VERSION)?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Shared with the sibling stores that keep their tables in the same file — the
    /// radio favourites, the saved-state mirrors — so they need not open a second
    /// connection to the database this one already owns.
    pub(crate) fn conn(&self) -> &Mutex<Connection> {
        &self.conn
    }

    pub fn list_under_root(&self, root: &Path) -> MhResult<Vec<(PathBuf, i64, i64)>> {
        let prefix = path_to_string(root);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT path, mtime_ns, size FROM tracks \
                 WHERE path = ?1 OR path LIKE ?2",
        )?;
        let like = format!("{}{}%", prefix, std::path::MAIN_SEPARATOR);
        let rows = stmt.query_map(params![prefix, like], |row| {
            let p: String = row.get(0)?;
            let m: i64 = row.get(1)?;
            let s: i64 = row.get(2)?;
            Ok((PathBuf::from(p), m, s))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn upsert_tracks(&self, rows: &[TrackRow]) -> MhResult<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(track_upsert_sql())?;
            let now = now_secs();
            for r in rows {
                let mut values = track_bind(r);
                values.push(rusqlite::types::Value::from(now));
                values.push(rusqlite::types::Value::from(now));
                stmt.execute(rusqlite::params_from_iter(values))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_paths(&self, paths: &[PathBuf]) -> MhResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM tracks WHERE path = ?1")?;
            for p in paths {
                stmt.execute(params![path_to_string(p)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drops every track that no longer lives under one of the library's folders.
    ///
    /// Scanning and pruning are both scoped to a root, so changing the library location
    /// left every row from the old one behind for good — visible in the library, pointing
    /// at files the app no longer manages, and never rescanned because nothing walks
    /// there any more.
    ///
    /// Takes the *configured* roots rather than the ones that happen to exist, so an
    /// unplugged external drive keeps its tracks instead of being treated as a move.
    /// Prefixes are compared with `substr` rather than `LIKE`, because a path containing
    /// `_` or `%` would otherwise match the wrong rows.
    pub fn delete_outside_roots(&self, roots: &[PathBuf]) -> MhResult<usize> {
        if roots.is_empty() {
            return Ok(0);
        }
        let sep = std::path::MAIN_SEPARATOR;
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        for root in roots {
            let base = path_to_string(root);
            let base = base.trim_end_matches(sep).to_string();
            let prefix = format!("{base}{sep}");
            clauses.push(format!(
                "path = ?{} OR substr(path, 1, length(?{})) = ?{}",
                params.len() + 1,
                params.len() + 2,
                params.len() + 2
            ));
            params.push(rusqlite::types::Value::from(base));
            params.push(rusqlite::types::Value::from(prefix));
        }
        let sql = format!(
            "DELETE FROM tracks WHERE NOT ({})",
            clauses.join(") AND NOT (")
        );
        let conn = self.conn.lock().unwrap();
        let removed = conn.execute(&sql, rusqlite::params_from_iter(params))?;
        Ok(removed)
    }

    pub fn delete_under_prefix(&self, prefix: &Path) -> MhResult<usize> {
        let p = path_to_string(prefix);
        let like = format!("{}{}%", p, std::path::MAIN_SEPARATOR);
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM tracks WHERE path = ?1 OR path LIKE ?2",
            params![p, like],
        )?;
        Ok(n)
    }

    pub fn downloaded_track(
        &self,
        platform: &str,
        track_id: &str,
    ) -> MhResult<Option<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .prepare_cached(
                "SELECT path, quality_rank FROM downloaded_tracks \
                 WHERE platform = ?1 AND track_id = ?2",
            )?
            .query_row(params![platform, track_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?;
        Ok(row)
    }

    pub fn record_download(
        &self,
        platform: &str,
        track_id: &str,
        path: &str,
        quality_rank: i64,
    ) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.prepare_cached(
            "INSERT INTO downloaded_tracks (platform, track_id, path, quality_rank, downloaded_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(platform, track_id) DO UPDATE SET \
               path = excluded.path, quality_rank = excluded.quality_rank, \
               downloaded_at = excluded.downloaded_at",
        )?
        .execute(params![platform, track_id, path, quality_rank, now_secs()])?;
        Ok(())
    }

    #[cfg(test)]
    fn count_tracks(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn rebuild_albums(&self) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        rebuild_albums_inline(&conn, None)
    }

    pub fn rebuild_albums_scoped(&self, dirs: &[String], keys: &[String]) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        rebuild_albums_inline(&conn, Some(&RebuildScope { dirs, keys }))
    }

    pub fn album_keys_for_paths(&self, paths: &[PathBuf]) -> MhResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT album_key FROM tracks WHERE path = ?1")?;
        let mut out: Vec<String> = Vec::new();
        for p in paths {
            if let Some(k) = stmt
                .query_row(params![path_to_string(p)], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .optional()?
                .flatten()
            {
                out.push(k);
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    pub fn query_albums(
        &self,
        offset: i64,
        limit: i64,
        search: Option<&str>,
        sort: &str,
    ) -> MhResult<(Vec<AlbumRow>, i64)> {
        let conn = self.conn.lock().unwrap();
        let order = match sort {
            "title" => "title COLLATE NOCASE ASC",
            "artist" => "artist COLLATE NOCASE ASC, title COLLATE NOCASE ASC",
            "year" => "year DESC NULLS LAST, title COLLATE NOCASE ASC",
            _ => "added_at DESC NULLS LAST, updated_at DESC",
        };
        paged_query(
            &conn,
            ALBUM_COLS,
            "FROM albums WHERE kind = 'album' \
             AND (:q IS NULL OR title LIKE :q OR artist LIKE :q)",
            order,
            like_param(search),
            limit,
            offset,
            album_mapper,
        )
    }

    /// Folder-backed collections, listed beside the user's own playlists instead of
    /// among the albums.
    pub fn collections(&self) -> MhResult<Vec<AlbumRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ALBUM_COLS} FROM albums WHERE kind = 'collection' \
             ORDER BY added_at DESC NULLS LAST, title COLLATE NOCASE ASC"
        ))?;
        let rows = stmt
            .query_map([], album_mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn album_by_key(&self, album_key: &str) -> MhResult<Option<AlbumRow>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<AlbumRow> = conn
            .query_row(
                &format!("SELECT {ALBUM_COLS} FROM albums WHERE album_key = ?1"),
                params![album_key],
                album_mapper,
            )
            .optional()?;
        Ok(row)
    }

    pub fn query_videos(
        &self,
        offset: i64,
        limit: i64,
        search: Option<&str>,
        sort: &str,
    ) -> MhResult<(Vec<TrackRow>, i64)> {
        let conn = self.conn.lock().unwrap();
        let order = match sort {
            "name" => "title COLLATE NOCASE ASC",
            "size" => "size DESC",
            "duration" => "duration_secs DESC NULLS LAST",
            _ => "added_at DESC NULLS LAST, scanned_at DESC",
        };
        paged_query(
            &conn,
            track_cols(),
            "FROM tracks WHERE is_video=1 AND (:q IS NULL OR title LIKE :q)",
            order,
            like_param(search),
            limit,
            offset,
            track_mapper,
        )
    }

    pub fn query_tracks(
        &self,
        offset: i64,
        limit: i64,
        search: Option<&str>,
        sort: &str,
    ) -> MhResult<(Vec<TrackRow>, i64)> {
        let conn = self.conn.lock().unwrap();
        let order = match sort {
            "title" => "title COLLATE NOCASE ASC",
            "artist" => "artist COLLATE NOCASE ASC, title COLLATE NOCASE ASC",
            "album" => "album COLLATE NOCASE ASC, disc_no NULLS LAST, track_no NULLS LAST",
            "duration" => "duration_secs DESC NULLS LAST",
            "bitrate" => "bitrate DESC NULLS LAST, title COLLATE NOCASE ASC",
            "year" => "year DESC NULLS LAST, title COLLATE NOCASE ASC",
            "plays" => "play_count DESC NULLS LAST, title COLLATE NOCASE ASC",
            _ => "added_at DESC NULLS LAST, scanned_at DESC",
        };
        paged_query(
            &conn,
            track_cols(),
            "FROM tracks WHERE is_video=0 \
             AND (:q IS NULL OR title LIKE :q OR artist LIKE :q OR album LIKE :q)",
            order,
            like_param(search),
            limit,
            offset,
            track_mapper,
        )
    }

    pub fn query_artists(
        &self,
        offset: i64,
        limit: i64,
        search: Option<&str>,
        sort: &str,
    ) -> MhResult<(Vec<ArtistRow>, i64)> {
        let conn = self.conn.lock().unwrap();
        let order = match sort {
            "tracks" => "track_count DESC, display COLLATE NOCASE ASC",
            "albums" => "album_count DESC, display COLLATE NOCASE ASC",
            _ => "display COLLATE NOCASE ASC",
        };
        paged_query(
            &conn,
            "primary_artist AS key, \
             COALESCE(MAX(COALESCE(album_artist, artist)), primary_artist) AS display, \
             COUNT(DISTINCT album_key) AS album_count, \
             COUNT(*) AS track_count, \
             (SELECT cover_id FROM tracks t2 \
                WHERE t2.primary_artist = tracks.primary_artist \
                  AND t2.is_video = 0 AND t2.cover_id IS NOT NULL \
                ORDER BY t2.scanned_at DESC LIMIT 1) AS cover_id",
            "FROM tracks \
             WHERE is_video=0 AND primary_artist IS NOT NULL AND primary_artist != '' \
             AND (:q IS NULL OR primary_artist LIKE :q OR artist LIKE :q) \
             GROUP BY primary_artist",
            order,
            like_param(search).map(|q| q.to_lowercase()),
            limit,
            offset,
            artist_mapper,
        )
    }

    pub fn albums_for_primary_artist(&self, key: &str) -> MhResult<Vec<AlbumRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ALBUM_COLS} FROM albums \
             WHERE album_key IN ( \
               SELECT DISTINCT album_key FROM tracks \
                 WHERE primary_artist = ?1 AND is_video=0 AND album_key IS NOT NULL \
             ) \
             ORDER BY year DESC NULLS LAST, title COLLATE NOCASE ASC"
        ))?;
        let rows = stmt
            .query_map(params![key], album_mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn tracks_for_primary_artist(&self, key: &str) -> MhResult<Vec<TrackRow>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM tracks WHERE primary_artist = ?1 AND is_video=0 \
             ORDER BY album COLLATE NOCASE, disc_no NULLS LAST, track_no NULLS LAST, title COLLATE NOCASE",
            track_cols()
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(params![key], track_mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn artist_display_for_key(&self, key: &str) -> MhResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let r: Option<String> = conn
            .query_row(
                "SELECT COALESCE(album_artist, artist) FROM tracks \
                 WHERE primary_artist = ?1 AND is_video=0 \
                 ORDER BY scanned_at DESC LIMIT 1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(r)
    }

    pub fn tracks_in_album(&self, album_key: &str) -> MhResult<Vec<TrackRow>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM tracks WHERE album_key = ?1 \
             ORDER BY disc_no NULLS LAST, track_no NULLS LAST, title COLLATE NOCASE",
            track_cols()
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(params![album_key], track_mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn playlist_list(&self) -> MhResult<Vec<PlaylistRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare_cached(
                "SELECT p.id, p.name, p.created_at, p.updated_at, \
                        (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) AS tc \
                 FROM playlists p ORDER BY p.updated_at DESC",
            )
            ?;
        let metas: Vec<(i64, String, i64, i64, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut covers_stmt = conn.prepare_cached(
            "SELECT t.cover_id FROM playlist_tracks pt \
                 JOIN tracks t ON t.path = pt.track_path \
                 WHERE pt.playlist_id = ?1 AND t.cover_id IS NOT NULL \
                 ORDER BY pt.position LIMIT 4",
        )?;

        let mut out = Vec::with_capacity(metas.len());
        for (id, name, c, u, tc) in metas {
            let covers: Vec<String> = covers_stmt
                .query_map(params![id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.push(PlaylistRow {
                id,
                name,
                created_at: c,
                updated_at: u,
                track_count: tc,
                cover_ids: covers,
            });
        }
        Ok(out)
    }

    pub fn playlist_create(&self, name: &str) -> MhResult<i64> {
        let conn = self.conn.lock().unwrap();
        let now = now_secs();
        conn.execute(
            "INSERT INTO playlists (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![name, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn playlist_rename(&self, id: i64, name: &str) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = now_secs();
        conn.execute(
            "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, now, id],
        )?;
        Ok(())
    }

    pub fn playlist_delete(&self, id: i64) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn playlist_get(&self, id: i64) -> MhResult<Option<(PlaylistRow, Vec<TrackRow>)>> {
        let meta = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT id, name, created_at, updated_at FROM playlists WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
        };
        let Some((id, name, c, u)) = meta else {
            return Ok(None);
        };

        let tracks = {
            let conn = self.conn.lock().unwrap();
            let sql = format!(
                "SELECT {} FROM tracks t \
                 JOIN playlist_tracks pt ON pt.track_path = t.path \
                 WHERE pt.playlist_id = ?1 ORDER BY pt.position",
                track_cols_for(Some("t"))
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows: Vec<TrackRow> = stmt
                .query_map(params![id], track_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        let cover_ids: Vec<String> = tracks
            .iter()
            .filter_map(|t| t.cover_id.clone())
            .take(4)
            .collect();

        Ok(Some((
            PlaylistRow {
                id,
                name,
                created_at: c,
                updated_at: u,
                track_count: tracks.len() as i64,
                cover_ids,
            },
            tracks,
        )))
    }

    pub fn playlist_add_tracks(&self, id: i64, paths: &[String]) -> MhResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let start_pos: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO playlist_tracks (playlist_id, track_path, position) \
                     VALUES (?1, ?2, ?3)",
            )?;
            for (i, p) in paths.iter().enumerate() {
                stmt.execute(params![id, p, start_pos + i as i64])?;
            }
        }
        tx.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn playlist_remove_at(&self, id: i64, position: i64) -> MhResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND position = ?2",
            params![id, position],
        )?;
        tx.execute(
            "UPDATE playlist_tracks SET position = position - 1 \
             WHERE playlist_id = ?1 AND position > ?2",
            params![id, position],
        )?;
        tx.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Moves one track to another position.
    ///
    /// The rows are rewritten from a permuted list rather than shifted in place.
    /// A shift cannot be expressed safely: `(playlist_id, position)` is unique,
    /// SQLite gives an `UPDATE` no row order, and moving a track *up* has to
    /// increment positions — the first row incremented lands on the second row's
    /// position and the statement fails on the constraint.
    pub fn playlist_reorder(&self, id: i64, from: i64, to: i64) -> MhResult<()> {
        if from == to {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut paths: Vec<String> = {
            let mut stmt = tx.prepare_cached(
                "SELECT track_path FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let rows = stmt
                .query_map(params![id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let len = paths.len() as i64;
        if from < 0 || to < 0 || from >= len || to >= len {
            return Ok(());
        }
        let moved = paths.remove(from as usize);
        paths.insert(to as usize, moved);

        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
            params![id],
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO playlist_tracks (playlist_id, track_path, position) \
                 VALUES (?1, ?2, ?3)",
            )?;
            for (position, path) in paths.iter().enumerate() {
                stmt.execute(params![id, path, position as i64])?;
            }
        }
        tx.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Local files had no listening history at all while every streaming service got a
    /// full scrobble, so "most played" and "recently played" could only ever mean the
    /// remote half of the library.
    /// An endless mix seeded from one track, scored over the tags the scanner now reads.
    ///
    /// Local playback had no radio of any kind while every streaming service did; the
    /// fields that make this possible — genre, year, BPM, play count — were all being
    /// written to disk and thrown away at scan time.
    pub fn radio_from(&self, seed_path: &str, limit: i64) -> MhResult<Vec<TrackRow>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "WITH seed AS (SELECT * FROM tracks WHERE path = :seed) \
             SELECT {} FROM tracks, seed \
             WHERE tracks.is_video = 0 \
               AND tracks.path != seed.path \
               AND tracks.duration_secs > 30 \
             ORDER BY ( \
                 CASE WHEN tracks.primary_artist = seed.primary_artist THEN 40 ELSE 0 END \
               + CASE WHEN tracks.genre IS NOT NULL AND tracks.genre = seed.genre THEN 25 ELSE 0 END \
               + CASE WHEN tracks.album_key = seed.album_key THEN -15 ELSE 0 END \
               + CASE WHEN tracks.year IS NOT NULL AND seed.year IS NOT NULL \
                       AND ABS(CAST(tracks.year AS INTEGER) - CAST(seed.year AS INTEGER)) <= 5 \
                      THEN 12 ELSE 0 END \
               + CASE WHEN tracks.bpm IS NOT NULL AND seed.bpm IS NOT NULL \
                       AND ABS(tracks.bpm - seed.bpm) <= 15 THEN 15 ELSE 0 END \
               + MIN(COALESCE(tracks.play_count, 0), 5) \
             ) DESC, RANDOM() \
             LIMIT :limit",
            track_cols_for(Some("tracks"))
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(
                named_params! { ":seed": seed_path, ":limit": limit },
                track_mapper,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn track_by_path(&self, path: &str) -> MhResult<Option<TrackRow>> {
        static SQL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let sql =
            SQL.get_or_init(|| format!("SELECT {} FROM tracks WHERE path = ?1", track_cols()));
        let conn = self.conn.lock().unwrap();
        let row = conn
            .prepare_cached(sql)?
            .query_row(params![path], track_mapper)
            .optional()?;
        Ok(row)
    }

    pub fn track_gain(&self, path: &str) -> MhResult<Option<TrackGain>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .prepare_cached(
                "SELECT rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak \
                 FROM tracks WHERE path = ?1",
            )?
            .query_row(params![path], |r| {
                Ok(TrackGain {
                    track_gain: parse_db(r.get::<_, Option<String>>(0)?),
                    track_peak: parse_db(r.get::<_, Option<String>>(1)?),
                    album_gain: parse_db(r.get::<_, Option<String>>(2)?),
                    album_peak: parse_db(r.get::<_, Option<String>>(3)?),
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn record_play(&self, path: &str) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.prepare_cached(
            "UPDATE tracks \
             SET play_count = COALESCE(play_count, 0) + 1, last_played_at = ?1 \
             WHERE path = ?2",
        )?
        .execute(params![now_secs(), path])?;
        Ok(())
    }

    pub fn paths_in_dir(&self, dir: &Path) -> MhResult<Vec<PathBuf>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT path FROM tracks WHERE parent_dir = ?1")?;
        let rows = stmt
            .query_map(params![path_to_string(dir)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(PathBuf::from).collect())
    }

    pub fn find_track_path_by_cover_id(&self, cover_id: &str) -> MhResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let r: Option<String> = conn
            .query_row(
                "SELECT path FROM tracks WHERE cover_id = ?1 LIMIT 1",
                params![cover_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(r)
    }

    pub fn find_track_path_by_filename(&self, filename: &str) -> MhResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let like = format!("%{}{}", std::path::MAIN_SEPARATOR, filename);
        let r: Option<String> = conn
            .query_row(
                "SELECT path FROM tracks WHERE path LIKE ?1 LIMIT 1",
                params![like],
                |row| row.get(0),
            )
            .optional()?;
        Ok(r)
    }
}

/// Every column `upsert_tracks` writes, in one order that drives the INSERT list,
/// the conflict-update list and the value binder. Adding a field means touching
/// this list and [`track_bind`] — nothing else — so the three cannot drift apart.
const TRACK_WRITE_COLUMNS: &[&str] = &[
    "path",
    "parent_dir",
    "size",
    "mtime_ns",
    "is_video",
    "title",
    "artist",
    "album",
    "album_artist",
    "album_key",
    "year",
    "genre",
    "duration_secs",
    "track_no",
    "disc_no",
    "cover_id",
    "primary_artist",
    "release_dir",
    "track_total",
    "disc_total",
    "date",
    "original_date",
    "compilation",
    "isrc",
    "barcode",
    "mb_recording_id",
    "mb_release_id",
    "composer",
    "lyricist",
    "producer",
    "conductor",
    "performer",
    "engineer",
    "mixer",
    "label",
    "copyright",
    "comment",
    "grouping",
    "description",
    "codec",
    "bitrate",
    "sample_rate",
    "bit_depth",
    "channels",
    "bpm",
    "rg_track_gain",
    "rg_track_peak",
    "rg_album_gain",
    "rg_album_peak",
    "has_lyrics",
];

/// Columns a rescan must never overwrite: `added_at` is stamped once when the file
/// first appears, and the play counters belong to the player. They are selected
/// but left out of the INSERT's conflict-update list.
const TRACK_KEPT_COLUMNS: &[&str] = &["added_at", "play_count", "last_played_at", "skip_count"];

fn track_bind(r: &TrackRow) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value as V;
    vec![
        V::from(r.path.clone()),
        V::from(r.parent_dir.clone()),
        V::from(r.size),
        V::from(r.mtime_ns),
        V::from(r.is_video),
        V::from(r.title.clone()),
        V::from(r.artist.clone()),
        V::from(r.album.clone()),
        V::from(r.album_artist.clone()),
        V::from(r.album_key.clone()),
        V::from(r.year.clone()),
        V::from(r.genre.clone()),
        V::from(r.duration_secs),
        V::from(r.track_no),
        V::from(r.disc_no),
        V::from(r.cover_id.clone()),
        V::from(r.primary_artist.clone()),
        V::from(r.release_dir.clone()),
        V::from(r.track_total),
        V::from(r.disc_total),
        V::from(r.date.clone()),
        V::from(r.original_date.clone()),
        V::from(r.compilation),
        V::from(r.isrc.clone()),
        V::from(r.barcode.clone()),
        V::from(r.mb_recording_id.clone()),
        V::from(r.mb_release_id.clone()),
        V::from(r.composer.clone()),
        V::from(r.lyricist.clone()),
        V::from(r.producer.clone()),
        V::from(r.conductor.clone()),
        V::from(r.performer.clone()),
        V::from(r.engineer.clone()),
        V::from(r.mixer.clone()),
        V::from(r.label.clone()),
        V::from(r.copyright.clone()),
        V::from(r.comment.clone()),
        V::from(r.grouping.clone()),
        V::from(r.description.clone()),
        V::from(r.codec.clone()),
        V::from(r.bitrate),
        V::from(r.sample_rate),
        V::from(r.bit_depth),
        V::from(r.channels),
        V::from(r.bpm),
        V::from(r.rg_track_gain.clone()),
        V::from(r.rg_track_peak.clone()),
        V::from(r.rg_album_gain.clone()),
        V::from(r.rg_album_peak.clone()),
        V::from(r.has_lyrics),
    ]
}

/// `col` quoted, optionally table-qualified. Every generated statement quotes, so
/// column names that collide with SQL keywords (`date`, `comment`, `grouping`)
/// need no special casing.
fn quoted(alias: Option<&str>, col: &str) -> String {
    match alias {
        Some(a) => format!("{a}.\"{col}\""),
        None => format!("\"{col}\""),
    }
}

fn track_cols_for(alias: Option<&str>) -> String {
    TRACK_WRITE_COLUMNS
        .iter()
        .chain(TRACK_KEPT_COLUMNS.iter())
        .map(|c| quoted(alias, c))
        .collect::<Vec<_>>()
        .join(", ")
}

fn track_cols() -> &'static str {
    static COLS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    COLS.get_or_init(|| track_cols_for(None))
}

fn track_upsert_sql() -> &'static str {
    static SQL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SQL.get_or_init(|| {
        let mut insert: Vec<String> = TRACK_WRITE_COLUMNS
            .iter()
            .map(|c| quoted(None, c))
            .collect();
        insert.push(quoted(None, "scanned_at"));
        insert.push(quoted(None, "added_at"));
        let placeholders = vec!["?"; insert.len()].join(", ");
        let mut updates: Vec<String> = TRACK_WRITE_COLUMNS
            .iter()
            .filter(|c| **c != "path")
            .map(|c| format!("\"{c}\" = excluded.\"{c}\""))
            .collect();
        updates.push("\"scanned_at\" = excluded.\"scanned_at\"".to_string());
        format!(
            "INSERT INTO tracks ({}) VALUES ({placeholders}) \
             ON CONFLICT(path) DO UPDATE SET {}",
            insert.join(", "),
            updates.join(", ")
        )
    })
}

fn track_mapper(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
    let u32_of = |name: &str| -> rusqlite::Result<Option<u32>> {
        Ok(row.get::<_, Option<i64>>(name)?.map(|n| n as u32))
    };
    Ok(TrackRow {
        path: row.get("path")?,
        parent_dir: row.get("parent_dir")?,
        size: row.get("size")?,
        mtime_ns: row.get("mtime_ns")?,
        is_video: row.get::<_, i64>("is_video")? != 0,
        title: row.get("title")?,
        artist: row.get("artist")?,
        album: row.get("album")?,
        album_artist: row.get("album_artist")?,
        album_key: row.get("album_key")?,
        year: row.get("year")?,
        genre: row.get("genre")?,
        duration_secs: row.get("duration_secs")?,
        track_no: u32_of("track_no")?,
        disc_no: u32_of("disc_no")?,
        cover_id: row.get("cover_id")?,
        primary_artist: row.get("primary_artist")?,
        release_dir: row.get("release_dir")?,
        track_total: u32_of("track_total")?,
        disc_total: u32_of("disc_total")?,
        date: row.get("date")?,
        original_date: row.get("original_date")?,
        compilation: row.get::<_, Option<i64>>("compilation")?.map(|n| n != 0),
        isrc: row.get("isrc")?,
        barcode: row.get("barcode")?,
        mb_recording_id: row.get("mb_recording_id")?,
        mb_release_id: row.get("mb_release_id")?,
        composer: row.get("composer")?,
        lyricist: row.get("lyricist")?,
        producer: row.get("producer")?,
        conductor: row.get("conductor")?,
        performer: row.get("performer")?,
        engineer: row.get("engineer")?,
        mixer: row.get("mixer")?,
        label: row.get("label")?,
        copyright: row.get("copyright")?,
        comment: row.get("comment")?,
        grouping: row.get("grouping")?,
        description: row.get("description")?,
        codec: row.get("codec")?,
        bitrate: u32_of("bitrate")?,
        sample_rate: u32_of("sample_rate")?,
        bit_depth: u32_of("bit_depth")?,
        channels: u32_of("channels")?,
        bpm: u32_of("bpm")?,
        rg_track_gain: row.get("rg_track_gain")?,
        rg_track_peak: row.get("rg_track_peak")?,
        rg_album_gain: row.get("rg_album_gain")?,
        rg_album_peak: row.get("rg_album_peak")?,
        has_lyrics: row.get::<_, Option<i64>>("has_lyrics")?.unwrap_or(0) != 0,
        added_at: row.get("added_at")?,
        play_count: row.get("play_count")?,
        last_played_at: row.get("last_played_at")?,
        skip_count: row.get("skip_count")?,
    })
}

const ALBUM_COLS: &str = "album_key, title, artist, year, year_end, cover_id, track_count, \
     disc_total, track_total, codec, bit_depth, sample_rate, release_dir, kind";

fn album_mapper(row: &rusqlite::Row<'_>) -> rusqlite::Result<AlbumRow> {
    let u32_of = |name: &str| -> rusqlite::Result<Option<u32>> {
        Ok(row.get::<_, Option<i64>>(name)?.map(|n| n as u32))
    };
    Ok(AlbumRow {
        album_key: row.get("album_key")?,
        title: row.get("title")?,
        artist: row.get("artist")?,
        year: row.get("year")?,
        year_end: row.get("year_end")?,
        cover_id: row.get("cover_id")?,
        track_count: row.get("track_count")?,
        disc_total: u32_of("disc_total")?,
        track_total: u32_of("track_total")?,
        codec: row.get("codec")?,
        bit_depth: u32_of("bit_depth")?,
        sample_rate: u32_of("sample_rate")?,
        release_dir: row.get("release_dir")?,
        kind: row.get("kind")?,
    })
}

fn artist_mapper(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtistRow> {
    Ok(ArtistRow {
        key: row.get(0)?,
        display: row.get(1)?,
        album_count: row.get(2)?,
        track_count: row.get(3)?,
        cover_id: row.get(4)?,
    })
}

fn like_param(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"))
}

/// One page of `cols` out of `from_where`, plus the unpaged total for the same
/// filter. `from_where` must accept `:q` as its only bound name — write the
/// search as `(:q IS NULL OR col LIKE :q)` so an absent search binds NULL and
/// the condition drops out, instead of needing a second statement.
fn paged_query<T, F>(
    conn: &Connection,
    cols: &str,
    from_where: &str,
    order: &str,
    q: Option<String>,
    limit: i64,
    offset: i64,
    map: F,
) -> MhResult<(Vec<T>, i64)>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM (SELECT 1 {from_where})"),
        named_params! { ":q": q },
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {cols} {from_where} ORDER BY {order} LIMIT :limit OFFSET :offset"
    ))?;
    let rows = stmt
        .query_map(
            named_params! { ":q": q, ":limit": limit, ":offset": offset },
            map,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((rows, total))
}

pub fn path_to_string(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Adds the v12 columns and primes them for backfill.
///
/// Two details matter. `added_at` has to survive later rescans, so it is seeded
/// from `scanned_at` here and then left out of the upsert's conflict-update list.
/// And the incremental scan skips any file whose size and mtime still match the
/// row, so without resetting `mtime_ns` every new column would stay NULL forever
/// — the reset is what makes the next ordinary scan re-read the tags.
fn migrate_to_v12(conn: &Connection, from_version: i64) -> MhResult<()> {
    let existing: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('tracks')")?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.into_iter().collect()
    };
    let mut added = false;
    for (name, ty) in SCHEMA_V12_COLUMNS {
        if existing.contains(*name) {
            continue;
        }
        conn.execute(
            &format!("ALTER TABLE tracks ADD COLUMN \"{name}\" {ty}"),
            [],
        )?;
        added = true;
    }
    conn.execute_batch(SCHEMA_V12_ADD)?;
    // The `albums` table is derived, so later versions recreate it rather than altering
    // it. Only a library that has never had the wider `tracks` columns needs the one-off
    // re-read that fills them — repeating it on every schema bump would make each upgrade
    // rescan the whole library for nothing.
    if !added && from_version >= 12 {
        return Ok(());
    }
    conn.execute(
        "UPDATE tracks SET added_at = scanned_at WHERE added_at IS NULL",
        [],
    )?;
    backfill_release_dir(conn)?;
    conn.execute("UPDATE tracks SET mtime_ns = -1", [])?;
    Ok(())
}

/// A station's identity became `(source, id)` when the directory stopped being a
/// single service. Before v14 the primary key was a bare radio-browser uuid, so
/// every existing favourite and play row is prefixed in place — nothing is lost
/// and nothing has to be re-fetched.
///
/// Re-running is a no-op in both halves: the columns are guarded, and a key that
/// already carries a source has a colon in it.
fn migrate_to_v14(conn: &Connection) -> MhResult<()> {
    add_missing_columns(
        conn,
        "radio_stations",
        crate::services::radio::store::V14_COLUMNS,
    )?;
    conn.execute(
        "UPDATE radio_stations SET uuid = 'radiobrowser:' || uuid WHERE instr(uuid, ':') = 0",
        [],
    )?;
    conn.execute_batch(crate::services::radio::store::V14_INDEXES)?;
    Ok(())
}

/// Stations became editable in v15. All three columns hold data no directory
/// supplies, so nothing has to be backfilled — an absent value simply means the
/// user has not changed anything.
fn migrate_to_v15(conn: &Connection) -> MhResult<()> {
    add_missing_columns(
        conn,
        "radio_stations",
        crate::services::radio::store::V15_COLUMNS,
    )
}

/// `ALTER TABLE` takes one column at a time and errors on a name that is already
/// there, so every radio migration adds its columns from a guarded list.
fn add_missing_columns(conn: &Connection, table: &str, columns: &[(&str, &str)]) -> MhResult<()> {
    let existing: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.into_iter().collect()
    };
    for (name, ty) in columns {
        if existing.contains(*name) {
            continue;
        }
        conn.execute(
            &format!("ALTER TABLE \"{table}\" ADD COLUMN \"{name}\" {ty}"),
            [],
        )?;
    }
    Ok(())
}

/// `release_dir` follows from `parent_dir` alone, so an upgraded library regroups
/// its albums the moment it opens rather than waiting for the tag re-read.
fn backfill_release_dir(conn: &Connection) -> MhResult<()> {
    let mut list_stmt = conn.prepare(
        "SELECT DISTINCT parent_dir FROM tracks \
         WHERE parent_dir IS NOT NULL AND parent_dir != ''",
    )?;
    let parents: Vec<String> = list_stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(list_stmt);
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE tracks SET release_dir = ?1 WHERE parent_dir = ?2")?;
        for parent in &parents {
            let release = crate::media::library::scanner::release_dir_of_parent(Path::new(parent));
            stmt.execute(params![path_to_string(&release), parent])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// The share of a folder's tracks that must carry the same artist before the folder
/// is accepted as one release despite disagreeing album tags.
///
/// Calibrated against real libraries rather than picked: a downloaded best-of folder
/// runs 87–100% one artist while a service output directory holding unrelated singles
/// runs 11–56%, so the threshold sits in a wide empty band between the two.
const RELEASE_ARTIST_DOMINANCE: f64 = 0.70;

/// Album keys derived from the folder are prefixed so they cannot collide with the
/// tag-derived `title::artist` keys that loose files still use.
const DIR_KEY_PREFIX: &str = "dir:";
const ALBUM_SCOPE_FILTER: &str = " AND album_key IN (SELECT album_key FROM album_scope)";

struct DirFacts {
    dir: String,
    tracks: i64,
    distinct_albums: i64,
    dominant_artist: i64,
}

impl DirFacts {
    fn is_release(&self) -> bool {
        if self.tracks < 2 {
            return false;
        }
        if self.distinct_albums == 1 {
            return true;
        }
        self.dominant_artist as f64 / self.tracks as f64 >= RELEASE_ARTIST_DOMINANCE
    }
}

/// Groups tracks into albums by the folder they sit in, refined by tags.
///
/// Tag-only grouping split one downloaded compilation into twenty-five one-song
/// albums (each track keeps its original release's tags) while merging five separate
/// on-disk copies of the same record into one impossible twenty-eight-track album. The
/// folder is the unit that gets both right; tags supply the title, artist and year on
/// top of it. Folders that hold unrelated files — a library root, a service output
/// directory — fail the checks in [`DirFacts::is_release`] and keep the tag-derived key.
fn rebuild_albums_inline(conn: &Connection, scope: Option<&RebuildScope<'_>>) -> MhResult<()> {
    let facts = dir_facts(conn)?;
    let containers = container_dirs(facts.iter().map(|f| f.dir.as_str()));
    let releases: Vec<&DirFacts> = facts
        .iter()
        .filter(|f| f.is_release() && !containers.contains(&f.dir))
        .collect();

    let scoped = match scope {
        Some(s) => open_album_scope(conn, s, &facts)?,
        None => false,
    };

    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "UPDATE tracks SET album_key = ?1 WHERE is_video = 0 AND release_dir = ?2",
        )?;
        for f in &releases {
            stmt.execute(params![format!("{DIR_KEY_PREFIX}{}", f.dir), f.dir])?;
        }
    }
    tx.commit()?;

    if scoped {
        conn.execute(
            "INSERT OR IGNORE INTO album_scope(album_key) \
             SELECT DISTINCT album_key FROM tracks \
              WHERE album_key IS NOT NULL AND release_dir IN (SELECT dir FROM dir_scope)",
            [],
        )?;
    }

    write_album_rows(conn, scoped)
}

/// The directories a watcher batch touched, plus the album keys of rows it is about to
/// remove — those are unrecoverable from the table once `delete_paths` has run.
pub struct RebuildScope<'a> {
    pub dirs: &'a [String],
    pub keys: &'a [String],
}

/// Narrows the album rebuild to the albums a batch can actually have changed.
///
/// Grouping stays global — whether a folder is a release depends on the folders around
/// it — but the seven aggregate scans and the `albums` rewrite that follow do not. The
/// scope is widened to any known directory that is an ancestor or descendant of a
/// touched one, because adding a release below a folder can turn that folder into a
/// container and move its tracks to a different key.
///
/// Returns false when the scope would be empty, which falls back to a full rebuild.
fn open_album_scope(
    conn: &Connection,
    scope: &RebuildScope<'_>,
    facts: &[DirFacts],
) -> MhResult<bool> {
    if scope.dirs.is_empty() && scope.keys.is_empty() {
        return Ok(false);
    }
    let sep = std::path::MAIN_SEPARATOR;
    let mut dirs: std::collections::HashSet<String> = scope.dirs.iter().cloned().collect();
    for f in facts {
        if scope.dirs.iter().any(|d| {
            f.dir.starts_with(&format!("{d}{sep}")) || d.starts_with(&format!("{}{sep}", f.dir))
        }) {
            dirs.insert(f.dir.clone());
        }
    }

    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS album_scope(album_key TEXT PRIMARY KEY);          CREATE TEMP TABLE IF NOT EXISTS dir_scope(dir TEXT PRIMARY KEY);          DELETE FROM album_scope;          DELETE FROM dir_scope;",
    )?;
    {
        let mut stmt = conn.prepare_cached("INSERT OR IGNORE INTO dir_scope(dir) VALUES (?1)")?;
        for d in &dirs {
            stmt.execute(params![d])?;
        }
        let mut key_stmt =
            conn.prepare_cached("INSERT OR IGNORE INTO album_scope(album_key) VALUES (?1)")?;
        for k in scope.keys {
            key_stmt.execute(params![k])?;
        }
    }
    conn.execute(
        "INSERT OR IGNORE INTO album_scope(album_key) \
         SELECT DISTINCT album_key FROM tracks \
          WHERE album_key IS NOT NULL AND release_dir IN (SELECT dir FROM dir_scope)",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO album_scope(album_key) \
         SELECT album_key FROM albums WHERE release_dir IN (SELECT dir FROM dir_scope)",
        [],
    )?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM album_scope", [], |r| r.get(0))?;
    Ok(n > 0)
}

fn dir_facts(conn: &Connection) -> MhResult<Vec<DirFacts>> {
    let mut stmt = conn.prepare_cached(
        "SELECT release_dir, \
                COUNT(*), \
                COUNT(DISTINCT CASE WHEN album IS NOT NULL AND TRIM(album) != '' \
                                    THEN LOWER(TRIM(album)) END) \
         FROM tracks \
         WHERE is_video = 0 AND release_dir IS NOT NULL AND release_dir != '' \
         GROUP BY release_dir",
    )?;
    let mut facts: Vec<DirFacts> = stmt
        .query_map([], |r| {
            Ok(DirFacts {
                dir: r.get(0)?,
                tracks: r.get(1)?,
                distinct_albums: r.get(2)?,
                dominant_artist: 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let dominant = mode_counts(
        conn,
        "SELECT release_dir, LOWER(TRIM(artist)), COUNT(*) FROM tracks \
         WHERE is_video = 0 AND release_dir IS NOT NULL AND artist IS NOT NULL \
           AND TRIM(artist) != '' \
         GROUP BY release_dir, LOWER(TRIM(artist))",
    )?;
    for f in &mut facts {
        f.dominant_artist = dominant.get(&f.dir).map(|m| m.votes).unwrap_or(0);
    }
    Ok(facts)
}

/// Folders that contain a deeper release folder. A library root or a per-service
/// output directory looks exactly like a release folder from its own tracks alone —
/// what separates it is that releases live *below* it. Disc subfolders do not count,
/// because they already resolve to their parent's `release_dir`.
fn container_dirs<'a, I: Iterator<Item = &'a str>>(dirs: I) -> std::collections::HashSet<String> {
    let sep = std::path::MAIN_SEPARATOR;
    let mut sorted: Vec<&str> = dirs.collect();
    sorted.sort_unstable();
    let mut out = std::collections::HashSet::new();
    for (i, dir) in sorted.iter().enumerate() {
        let prefix = format!("{dir}{sep}");
        if sorted[i + 1..]
            .iter()
            .take_while(|d| d.starts_with(&prefix))
            .next()
            .is_some()
        {
            out.insert((*dir).to_string());
        }
    }
    out
}

/// `GROUP BY key, value` counts reduced to the most common value per key. Used for the
/// fields where an album's value is whatever most of its tracks agree on — the dominant
/// artist, the cover, the codec — rather than an arbitrary `MAX()`.
/// The most common value for a field across one album's tracks, and how much of the
/// album agreed.
#[derive(Default)]
struct Mode {
    value: String,
    votes: i64,
    /// Tracks that carried the field at all. A release-level tag such as ALBUMARTIST is
    /// routinely present on only some files of a folder, and a missing tag is not a
    /// dissenting vote — so agreement is judged against this, not the track count.
    present: i64,
}

impl Mode {
    /// Agreement among the tracks that carried the field at all.
    fn agreed(&self) -> Option<&str> {
        (self.present > 0 && self.votes as f64 / self.present as f64 >= RELEASE_ARTIST_DOMINANCE)
            .then_some(self.value.as_str())
    }

    /// Agreement across the whole release, where a track without the tag counts as one
    /// that did not vote for this value.
    fn agreed_across(&self, tracks: i64) -> Option<&str> {
        (tracks > 0 && self.votes as f64 / tracks as f64 >= RELEASE_ARTIST_DOMINANCE)
            .then_some(self.value.as_str())
    }
}

type Modes = HashMap<String, Mode>;

/// Reduces a `SELECT key, value, COUNT(*) ... GROUP BY key, value` to the winning value
/// per key. Used for every field where an album's value is whatever its tracks mostly
/// agree on — title, artist, cover, codec — rather than the arbitrary `MAX()` the
/// tag-only rebuild picked.
fn mode_counts(conn: &Connection, sql: &str) -> MhResult<Modes> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut out: Modes = HashMap::new();
    for row in rows {
        let (key, value, count) = row?;
        let entry = out.entry(key).or_default();
        entry.present += count;
        if count > entry.votes {
            entry.value = value;
            entry.votes = count;
        }
    }
    Ok(out)
}

struct AlbumAgg {
    album_key: String,
    tracks: i64,
    year: Option<String>,
    year_end: Option<String>,
    disc_total: Option<i64>,
    track_total: Option<i64>,
    bit_depth: Option<i64>,
    sample_rate: Option<i64>,
    release_dir: Option<String>,
    added_at: Option<i64>,
    updated_at: i64,
}

fn write_album_rows(conn: &Connection, scoped: bool) -> MhResult<()> {
    let scope = if scoped { ALBUM_SCOPE_FILTER } else { "" };
    let mut agg_stmt = conn.prepare_cached(&format!(
        "SELECT album_key, \
                COUNT(*), \
                MIN(year), MAX(year), \
                MAX(disc_total), MAX(track_total), \
                MIN(bit_depth), MIN(sample_rate), \
                MAX(release_dir), MIN(added_at), MAX(scanned_at) \
         FROM tracks WHERE is_video = 0 AND album_key IS NOT NULL{scope} \
         GROUP BY album_key"
    ))?;
    let aggs: Vec<AlbumAgg> = agg_stmt
        .query_map([], |r| {
            Ok(AlbumAgg {
                album_key: r.get(0)?,
                tracks: r.get(1)?,
                year: r.get(2)?,
                year_end: r.get(3)?,
                disc_total: r.get(4)?,
                track_total: r.get(5)?,
                bit_depth: r.get(6)?,
                sample_rate: r.get(7)?,
                release_dir: r.get(8)?,
                added_at: r.get(9)?,
                updated_at: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(agg_stmt);

    let titles = mode_counts(
        conn,
        &format!(
            "SELECT album_key, TRIM(album), COUNT(*) FROM tracks \
         WHERE is_video = 0 AND album_key IS NOT NULL{scope} AND album IS NOT NULL \
           AND TRIM(album) != '' \
         GROUP BY album_key, TRIM(album)"
        ),
    )?;
    let album_artists = mode_counts(
        conn,
        &format!(
            "SELECT album_key, TRIM(album_artist), COUNT(*) FROM tracks \
         WHERE is_video = 0 AND album_key IS NOT NULL{scope} AND album_artist IS NOT NULL \
           AND TRIM(album_artist) != '' \
         GROUP BY album_key, TRIM(album_artist)"
        ),
    )?;
    let artists = mode_counts(
        conn,
        &format!(
            "SELECT album_key, TRIM(artist), COUNT(*) FROM tracks \
         WHERE is_video = 0 AND album_key IS NOT NULL{scope} AND artist IS NOT NULL \
           AND TRIM(artist) != '' \
         GROUP BY album_key, TRIM(artist)"
        ),
    )?;
    // Featured guests make the raw artist strings disagree on a record that really has
    // one artist, so consensus is judged on the normalised name the scanner derives.
    let primary_artists = mode_counts(
        conn,
        &format!(
            "SELECT album_key, primary_artist, COUNT(*) FROM tracks \
         WHERE is_video = 0 AND album_key IS NOT NULL{scope} AND primary_artist IS NOT NULL \
           AND primary_artist != '' \
         GROUP BY album_key, primary_artist"
        ),
    )?;
    let covers = mode_counts(
        conn,
        &format!(
            "SELECT album_key, cover_id, COUNT(*) FROM tracks \
         WHERE is_video = 0 AND album_key IS NOT NULL{scope} AND cover_id IS NOT NULL \
         GROUP BY album_key, cover_id"
        ),
    )?;
    let codecs = mode_counts(
        conn,
        &format!(
            "SELECT album_key, codec, COUNT(*) FROM tracks \
         WHERE is_video = 0 AND album_key IS NOT NULL{scope} AND codec IS NOT NULL \
         GROUP BY album_key, codec"
        ),
    )?;

    let tx = conn.unchecked_transaction()?;
    if scoped {
        tx.execute(
            "DELETE FROM albums WHERE album_key IN (SELECT album_key FROM album_scope)",
            [],
        )?;
    } else {
        tx.execute("DELETE FROM albums", [])?;
    }
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO albums (album_key, title, artist, year, year_end, cover_id, \
                                 track_count, disc_total, track_total, codec, bit_depth, \
                                 sample_rate, release_dir, kind, added_at, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        )?;
        for a in &aggs {
            let key = a.album_key.as_str();
            let across = |m: &Modes| {
                m.get(key)
                    .and_then(|x| x.agreed_across(a.tracks))
                    .map(str::to_string)
            };

            // An ALBUMARTIST carried by most of the release names it. Failing that, a
            // release whose tracks agree on one performer is named after them — which is
            // what rescues a library where an older migration dropped the redundant
            // ALBUMARTIST tags and left a single stray behind. Only if neither holds does
            // a sparse but self-consistent ALBUMARTIST get to decide.
            let artist = across(&album_artists)
                .or_else(|| {
                    across(&primary_artists).and_then(|_| artists.get(key).map(|m| m.value.clone()))
                })
                .or_else(|| {
                    album_artists
                        .get(key)
                        .filter(|m| m.present >= 2)
                        .and_then(Mode::agreed)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| {
                    if artists.contains_key(&a.album_key) {
                        "Various Artists".to_string()
                    } else {
                        "Unknown".to_string()
                    }
                });
            // A folder whose tracks agree on an album tag is a release. One that only
            // agrees on the artist is a downloaded playlist or best-of: still one thing
            // rather than twenty-five one-song albums, but not an album.
            let tagged_title = titles.get(key).and_then(Mode::agreed).map(str::to_string);
            let kind = if tagged_title.is_some() {
                "album"
            } else {
                "collection"
            };
            let title = tagged_title
                .or_else(|| {
                    a.release_dir
                        .as_deref()
                        .and_then(folder_display_name)
                        .map(|name| strip_artist_prefix(&name, &artist))
                })
                .or_else(|| titles.get(&a.album_key).map(|m| m.value.clone()))
                .unwrap_or_else(|| "Unknown".to_string());
            let year_end = match (&a.year, &a.year_end) {
                (Some(from), Some(to)) if from != to => Some(to.clone()),
                _ => None,
            };
            stmt.execute(params![
                a.album_key,
                title,
                artist,
                a.year,
                year_end,
                covers.get(&a.album_key).map(|m| m.value.clone()),
                a.tracks,
                a.disc_total,
                a.track_total,
                codecs.get(&a.album_key).map(|m| m.value.clone()),
                a.bit_depth,
                a.sample_rate,
                a.release_dir,
                kind,
                a.added_at,
                a.updated_at,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// The folder's own name, for releases whose tracks disagree on the album tag. A
/// downloaded compilation carries each track's original album, so the folder name is
/// the only thing that describes the release as a whole.
/// Download folders are named `Artist - Release`, so using the folder as the album
/// title would print the artist twice in a view that already shows it underneath.
fn strip_artist_prefix(name: &str, artist: &str) -> String {
    if artist.is_empty() {
        return name.to_string();
    }
    let Some(rest) = name
        .get(..artist.len())
        .filter(|head| head.eq_ignore_ascii_case(artist))
        .and_then(|_| name.get(artist.len()..))
    else {
        return name.to_string();
    };
    let trimmed = rest
        .trim_start()
        .trim_start_matches(['-', '\u{2013}', '\u{2014}']);
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == rest.trim() {
        name.to_string()
    } else {
        trimmed.to_string()
    }
}

fn folder_display_name(dir: &str) -> Option<String> {
    Path::new(dir)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

fn backfill_primary_artist(conn: &Connection) -> MhResult<()> {
    type Row = (String, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> = {
        let mut stmt =
            conn.prepare("SELECT path, artist, album_artist, album FROM tracks WHERE is_video=0")?;
        let collected: Vec<Row> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        collected
    };
    if rows.is_empty() {
        return Ok(());
    }
    conn.execute(
        "UPDATE tracks SET album_artist = NULL WHERE album_artist IS NOT NULL AND album_artist = artist AND is_video = 0",
        [],
    )
    .ok();
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt =
            tx.prepare("UPDATE tracks SET primary_artist = ?1, album_key = ?2 WHERE path = ?3")?;
        for (path, artist, album_artist, album) in rows {
            let aa_for_key = match (&album_artist, &artist) {
                (Some(aa), Some(a)) if aa == a => None,
                _ => album_artist.clone(),
            };
            let pa = crate::media::library::scanner::normalize_primary_artist(
                artist.as_deref(),
                aa_for_key.as_deref(),
            );
            let ak = crate::media::library::scanner::album_key_for(
                album.as_deref(),
                aa_for_key.as_deref(),
                artist.as_deref(),
            );
            stmt.execute(params![pa, ak, path])?;
        }
        drop(stmt);
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str) -> TrackRow {
        TrackRow {
            path: path.to_string(),
            parent_dir: Path::new(path)
                .parent()
                .map(path_to_string)
                .unwrap_or_default(),
            size: 1,
            mtime_ns: 1,
            is_video: false,
            ..Default::default()
        }
    }

    fn tmp_db() -> (LibraryDb, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = LibraryDb::open(&dir.path().join("library.sqlite")).unwrap();
        (db, dir)
    }

    #[test]
    fn delete_under_prefix_removes_folder_subtree() {
        let (db, _d) = tmp_db();
        db.upsert_tracks(&[
            row("/lib/AlbumA/01.flac"),
            row("/lib/AlbumA/02.flac"),
            row("/lib/AlbumB/01.flac"),
        ])
        .unwrap();
        let removed = db.delete_under_prefix(Path::new("/lib/AlbumA")).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(db.count_tracks(), 1);
        let removed = db
            .delete_under_prefix(Path::new("/lib/AlbumB/01.flac"))
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(db.count_tracks(), 0);
    }

    /// Changing the library folder has to take the old folder's rows with it, or the
    /// library keeps showing tracks it no longer manages and never rescans.
    #[test]
    fn changing_the_library_folder_drops_the_old_folders_rows() {
        let (db, _d) = tmp_db();
        db.upsert_tracks(&[
            row("/home/u/Music/Artist/Album/01.flac"),
            row("/home/u/Music/Artist/Album/02.flac"),
            row("/home/u/Downloads/New/01.flac"),
        ])
        .unwrap();

        let removed = db
            .delete_outside_roots(&[PathBuf::from("/home/u/Downloads")])
            .unwrap();
        assert_eq!(removed, 2);
        assert_eq!(db.count_tracks(), 1);
    }

    /// An empty root list means the settings have not loaded yet, not that the user owns
    /// no music. Wiping the library there would be unrecoverable.
    #[test]
    fn no_roots_is_never_treated_as_every_root_being_gone() {
        let (db, _d) = tmp_db();
        db.upsert_tracks(&[row("/home/u/Music/01.flac")]).unwrap();
        assert_eq!(db.delete_outside_roots(&[]).unwrap(), 0);
        assert_eq!(db.count_tracks(), 1);
    }

    /// A sibling folder whose name merely starts with the root's must not be swept up,
    /// and `_` in a path must not act as a wildcard the way it would under LIKE.
    #[test]
    fn prefix_matching_respects_folder_boundaries_and_literal_underscores() {
        let (db, _d) = tmp_db();
        db.upsert_tracks(&[
            row("/home/u/Music/01.flac"),
            row("/home/u/Music Backup/01.flac"),
            row("/home/u/M_sic/01.flac"),
        ])
        .unwrap();
        let removed = db
            .delete_outside_roots(&[PathBuf::from("/home/u/Music")])
            .unwrap();
        assert_eq!(removed, 2, "the sibling and the underscore path both go");
        let kept = db.list_under_root(Path::new("/home/u/Music")).unwrap();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].0.ends_with("Music/01.flac"));
    }

    /// Several library folders are all valid homes; only what sits outside every one of
    /// them is stale.
    #[test]
    fn a_track_under_any_configured_root_survives() {
        let (db, _d) = tmp_db();
        db.upsert_tracks(&[
            row("/home/u/Music/01.flac"),
            row("/home/u/Downloads/02.flac"),
            row("/home/u/Videos/03.flac"),
        ])
        .unwrap();
        let removed = db
            .delete_outside_roots(&[
                PathBuf::from("/home/u/Music"),
                PathBuf::from("/home/u/Downloads"),
            ])
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(db.count_tracks(), 2);
    }

    #[test]
    fn downloaded_tracks_ledger_roundtrip() {
        let (db, _d) = tmp_db();
        assert!(db.downloaded_track("tidal", "123").unwrap().is_none());
        db.record_download("tidal", "123", "/lib/x.flac", 3)
            .unwrap();
        assert_eq!(
            db.downloaded_track("tidal", "123").unwrap(),
            Some(("/lib/x.flac".to_string(), 3))
        );
        db.record_download("tidal", "123", "/lib/y.flac", 4)
            .unwrap();
        assert_eq!(
            db.downloaded_track("tidal", "123").unwrap(),
            Some(("/lib/y.flac".to_string(), 4))
        );
        assert!(db.downloaded_track("qobuz", "123").unwrap().is_none());
    }

    fn titled(path: &str, title: &str, artist: &str, video: bool) -> TrackRow {
        TrackRow {
            title: Some(title.to_string()),
            artist: Some(artist.to_string()),
            primary_artist: Some(artist.to_string()),
            is_video: video,
            ..row(path)
        }
    }

    fn seeded() -> (LibraryDb, tempfile::TempDir) {
        let (db, d) = tmp_db();
        db.upsert_tracks(&[
            titled("/lib/a.flac", "Get Lucky", "Daft Punk", false),
            titled("/lib/b.flac", "Instant Crush", "Daft Punk", false),
            titled("/lib/c.flac", "Get Lucky", "Nile Rodgers", false),
            titled(
                "/lib/v.mp4",
                "Get Lucky (Official Video)",
                "Daft Punk",
                true,
            ),
        ])
        .unwrap();
        (db, d)
    }

    #[test]
    fn an_absent_search_returns_everything_and_a_search_filters() {
        let (db, _d) = seeded();
        for empty in [None, Some(""), Some("   ")] {
            let (rows, total) = db.query_tracks(0, 50, empty, "title").unwrap();
            assert_eq!((rows.len(), total), (3, 3), "search={empty:?}");
        }
        let (rows, total) = db.query_tracks(0, 50, Some("lucky"), "title").unwrap();
        assert_eq!((rows.len(), total), (2, 2));
        let (rows, _) = db.query_tracks(0, 50, Some("daft punk"), "title").unwrap();
        assert_eq!(rows.len(), 2, "search matches the artist column too");
    }

    #[test]
    fn the_total_counts_every_match_not_just_the_page() {
        let (db, _d) = seeded();
        let (rows, total) = db.query_tracks(0, 1, Some("lucky"), "title").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(total, 2, "total must ignore LIMIT");
        let (page2, _) = db.query_tracks(1, 1, Some("lucky"), "title").unwrap();
        assert_ne!(page2[0].path, rows[0].path, "OFFSET must advance the page");
    }

    #[test]
    fn videos_and_tracks_stay_on_their_own_side_of_is_video() {
        let (db, _d) = seeded();
        assert_eq!(db.query_tracks(0, 50, None, "title").unwrap().1, 3);
        assert_eq!(db.query_videos(0, 50, None, "recent").unwrap().1, 1);
        assert_eq!(
            db.query_videos(0, 50, Some("lucky"), "recent").unwrap().1,
            1
        );
        assert_eq!(
            db.query_videos(0, 50, Some("crush"), "recent").unwrap().1,
            0
        );
    }

    #[test]
    fn grouped_artist_rows_count_groups_not_tracks() {
        let (db, _d) = seeded();
        let (rows, total) = db.query_artists(0, 50, None, "name").unwrap();
        assert_eq!((rows.len(), total), (2, 2), "two distinct primary artists");
        let (rows, total) = db.query_artists(0, 50, Some("NILE"), "name").unwrap();
        assert_eq!((rows.len(), total), (1, 1), "search is case-insensitive");
        assert_eq!(rows[0].track_count, 1);
    }

    /// Mirrors what `build_audio_row` stores, `primary_artist` included — the grouping
    /// rules read it, so a fixture without one is not the shape the scanner produces.
    fn in_dir(dir: &str, file: &str, title: &str, artist: &str, album: Option<&str>) -> TrackRow {
        let path = format!("{dir}/{file}");
        TrackRow {
            title: Some(title.to_string()),
            artist: Some(artist.to_string()),
            album: album.map(str::to_string),
            album_key: album.map(|a| format!("{}::{}", a.to_lowercase(), artist.to_lowercase())),
            release_dir: Some(dir.to_string()),
            primary_artist: crate::media::library::scanner::normalize_primary_artist(
                Some(artist),
                None,
            ),
            ..row(&path)
        }
    }

    /// The reported case: a downloaded best-of whose files each carry their original
    /// release's tags. Tag-only grouping made twenty-five one-song albums out of it.
    #[test]
    fn a_folder_of_one_artist_is_one_album_however_its_tracks_are_tagged() {
        let (db, _d) = tmp_db();
        let dir = "/music/Daft Punk - Musique Vol. 1";
        let rows: Vec<TrackRow> = (0..6)
            .map(|i| {
                in_dir(
                    dir,
                    &format!("{i:02}.flac"),
                    &format!("Song {i}"),
                    "Daft Punk",
                    Some(&format!("Original Release {i}")),
                )
            })
            .collect();
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();

        assert_eq!(
            db.query_albums(0, 50, None, "title").unwrap().1,
            0,
            "a best-of is not a release, so it stays out of the albums list"
        );
        let collections = db.collections().unwrap();
        assert_eq!(
            collections.len(),
            1,
            "one folder, one artist, one collection"
        );
        assert_eq!(collections[0].track_count, 6);
        assert_eq!(collections[0].artist, "Daft Punk");
        assert_eq!(
            collections[0].title, "Musique Vol. 1",
            "artist prefix stripped"
        );
    }

    /// A folder whose tracks agree on an album tag is a release; one that only agrees on
    /// the artist is a downloaded playlist. Both get folded into a single entry — that
    /// was the point — but only the first belongs in the Albums list.
    #[test]
    fn a_release_is_an_album_and_a_best_of_is_a_collection() {
        let (db, _d) = tmp_db();
        for i in 0..5 {
            db.upsert_tracks(&[in_dir(
                "/music/Daft Punk/Discovery",
                &format!("{i:02}.flac"),
                &format!("Song {i}"),
                "Daft Punk",
                Some("Discovery"),
            )])
            .unwrap();
            db.upsert_tracks(&[in_dir(
                "/music/Daft Punk - Best Of",
                &format!("{i:02}.flac"),
                &format!("Hit {i}"),
                "Daft Punk",
                Some(&format!("Source Album {i}")),
            )])
            .unwrap();
        }
        db.rebuild_albums().unwrap();

        let (albums, total) = db.query_albums(0, 50, None, "title").unwrap();
        assert_eq!(total, 1, "only the real release");
        assert_eq!(albums[0].title, "Discovery");
        assert_eq!(albums[0].kind, "album");

        let collections = db.collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].title, "Best Of");
        assert_eq!(collections[0].kind, "collection");
        assert_eq!(
            collections[0].track_count, 5,
            "still folded, not five singles"
        );
    }

    /// The other half: a folder holding unrelated singles must not be swept into one
    /// album just because it is a folder.
    #[test]
    fn a_folder_of_unrelated_singles_stays_unmerged() {
        let (db, _d) = tmp_db();
        let dir = "/music/Downloads";
        let rows: Vec<TrackRow> = (0..6)
            .map(|i| {
                in_dir(
                    dir,
                    &format!("{i:02}.flac"),
                    &format!("Song {i}"),
                    &format!("Artist {i}"),
                    Some(&format!("Album {i}")),
                )
            })
            .collect();
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        assert_eq!(db.query_albums(0, 50, None, "title").unwrap().1, 6);
    }

    /// One featured artist used to convert a whole record to Various Artists, and the
    /// runtime path re-did it after every scan.
    #[test]
    fn one_guest_does_not_make_a_compilation() {
        let (db, _d) = tmp_db();
        let dir = "/music/Daft Punk/Discovery";
        let mut rows: Vec<TrackRow> = (0..9)
            .map(|i| {
                in_dir(
                    dir,
                    &format!("{i:02}.flac"),
                    &format!("Song {i}"),
                    "Daft Punk",
                    Some("Discovery"),
                )
            })
            .collect();
        rows.push(in_dir(
            dir,
            "09.flac",
            "Face to Face",
            "Daft Punk feat. Todd Edwards",
            Some("Discovery"),
        ));
        for r in &mut rows {
            r.album_artist = Some("Daft Punk".to_string());
        }
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();

        let (albums, _) = db.query_albums(0, 50, None, "title").unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist, "Daft Punk");
        assert_eq!(albums[0].track_count, 10);
    }

    /// A stray ALBUMARTIST on one file of a folder must not name the whole release.
    #[test]
    fn the_majority_album_artist_wins_over_a_stray_tag() {
        let (db, _d) = tmp_db();
        let dir = "/music/Daft Punk - Musique Vol. 1";
        let mut rows: Vec<TrackRow> = (0..6)
            .map(|i| {
                in_dir(
                    dir,
                    &format!("{i:02}.flac"),
                    &format!("Song {i}"),
                    "Daft Punk",
                    Some("Best Of"),
                )
            })
            .collect();
        for r in rows.iter_mut() {
            r.album_artist = Some("Daft Punk".to_string());
        }
        rows[3].album_artist = Some("Pharrell Williams".to_string());
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        assert_eq!(
            db.query_albums(0, 50, None, "title").unwrap().0[0].artist,
            "Daft Punk"
        );
    }

    /// The shape an upgraded library actually has: an older migration cleared every
    /// ALBUMARTIST that merely repeated the track artist, so a folder can arrive with one
    /// stray tag and twenty-six blanks. The stray must not name the release.
    #[test]
    fn a_lone_surviving_album_artist_does_not_outvote_the_performers() {
        let (db, _d) = tmp_db();
        let dir = "/music/Daft Punk - Musique Vol. 1";
        let mut rows: Vec<TrackRow> = (0..8)
            .map(|i| {
                in_dir(
                    dir,
                    &format!("{i:02}.flac"),
                    &format!("Song {i}"),
                    "Daft Punk",
                    Some(&format!("Original Release {i}")),
                )
            })
            .collect();
        rows[3].album_artist = Some("Pharrell Williams".to_string());
        rows[3].primary_artist = Some("pharrell williams".to_string());

        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();

        let collections = db.collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].artist, "Daft Punk");
        assert_eq!(collections[0].title, "Musique Vol. 1");
    }

    /// A guest credit on two of three tracks used to drop a record to Various Artists,
    /// because the raw artist strings disagree even though the performer does not.
    #[test]
    fn a_guest_credit_does_not_split_the_artist_consensus() {
        let (db, _d) = tmp_db();
        let dir = "/music/Daft Punk/Human After All";
        let rows = vec![
            in_dir(
                dir,
                "01.flac",
                "Robot Rock",
                "Daft Punk",
                Some("Human After All"),
            ),
            in_dir(
                dir,
                "02.flac",
                "Technologic",
                "Daft Punk",
                Some("Human After All"),
            ),
            in_dir(
                dir,
                "03.flac",
                "Human After All",
                "Daft Punk & Nile Rodgers",
                Some("Human After All"),
            ),
        ];
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        assert_eq!(
            db.query_albums(0, 50, None, "title").unwrap().0[0].artist,
            "Daft Punk"
        );
    }

    /// Two records that share a title and artist but sit in different folders are two
    /// copies on disk, not one album with twice the tracks.
    #[test]
    fn the_same_release_in_two_folders_stays_two_albums() {
        let (db, _d) = tmp_db();
        let mut rows = Vec::new();
        for dir in [
            "/music/flac/Daft Punk/Human After All",
            "/music/mp3/Daft Punk/Human After All",
        ] {
            for i in 0..4 {
                rows.push(in_dir(
                    dir,
                    &format!("{i:02}.flac"),
                    &format!("Song {i}"),
                    "Daft Punk",
                    Some("Human After All"),
                ));
            }
        }
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        let (albums, total) = db.query_albums(0, 50, None, "title").unwrap();
        assert_eq!(total, 2);
        assert!(albums.iter().all(|a| a.track_count == 4));
    }

    /// A folder whose subfolders hold the actual releases is a library root, and would
    /// otherwise pass the one-artist check on its own loose files.
    #[test]
    fn a_folder_containing_releases_is_not_itself_one() {
        let (db, _d) = tmp_db();
        let mut rows = vec![
            in_dir("/music", "loose-a.flac", "Loose A", "Someone", Some("A")),
            in_dir("/music", "loose-b.flac", "Loose B", "Someone", Some("B")),
        ];
        for i in 0..4 {
            rows.push(in_dir(
                "/music/Someone/Real Album",
                &format!("{i:02}.flac"),
                &format!("Song {i}"),
                "Someone",
                Some("Real Album"),
            ));
        }
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        let (albums, _) = db.query_albums(0, 50, None, "title").unwrap();
        let real = albums.iter().find(|a| a.title == "Real Album").unwrap();
        assert_eq!(real.track_count, 4, "the release folder folds");
        assert_eq!(albums.len(), 3, "the two loose files stay their own albums");
    }

    /// A rescan rewrites `scanned_at` for every row, so "Recently added" can only mean
    /// something if the first-seen stamp survives it.
    #[test]
    fn added_at_survives_a_rescan() {
        let (db, _d) = tmp_db();
        let mut r = in_dir("/music/A", "01.flac", "One", "X", Some("A"));
        db.upsert_tracks(&[r.clone()]).unwrap();
        let first = db.query_tracks(0, 1, None, "recent").unwrap().0[0]
            .added_at
            .expect("added_at stamped on insert");
        r.size = 999;
        db.upsert_tracks(&[r]).unwrap();
        let rows = db.query_tracks(0, 1, None, "recent").unwrap().0;
        let after = &rows[0];
        assert_eq!(after.added_at, Some(first));
        assert_eq!(after.size, 999, "the rest of the row still updates");
    }

    #[test]
    fn album_search_survives_the_null_guard() {
        let (db, _d) = tmp_db();
        db.upsert_tracks(&[TrackRow {
            album: Some("TRON: Legacy".into()),
            album_key: Some("k1".into()),
            ..titled("/lib/x.flac", "Derezzed", "Daft Punk", false)
        }])
        .unwrap();
        db.rebuild_albums().unwrap();
        assert_eq!(db.query_albums(0, 50, None, "title").unwrap().1, 1);
        assert_eq!(
            db.query_albums(0, 50, Some("legacy"), "title").unwrap().1,
            1
        );
        assert_eq!(
            db.query_albums(0, 50, Some("nothing"), "title").unwrap().1,
            0
        );
    }

    fn album_snapshot(db: &LibraryDb) -> Vec<(String, String, String, i64)> {
        let mut out: Vec<(String, String, String, i64)> = db
            .query_albums(0, 500, None, "title")
            .unwrap()
            .0
            .into_iter()
            .chain(db.collections().unwrap())
            .map(|a| (a.album_key, a.title, a.artist, a.track_count))
            .collect();
        out.sort();
        out
    }

    /// A scoped rebuild must land the `albums` table in exactly the state a full one
    /// would. Grouping stays global; only the aggregates and the rewrite are narrowed,
    /// so any album the scope misses would silently keep a stale row.
    #[test]
    fn a_scoped_rebuild_matches_a_full_one_after_an_ingest() {
        let (db, _d) = tmp_db();
        let a = "/music/Air - Moon Safari";
        let b = "/music/Boards of Canada - Geogaddi";
        let mut rows: Vec<TrackRow> = (0..5)
            .map(|i| {
                in_dir(
                    a,
                    &format!("{i:02}.flac"),
                    &format!("A{i}"),
                    "Air",
                    Some("Moon Safari"),
                )
            })
            .collect();
        rows.extend((0..4).map(|i| {
            in_dir(
                b,
                &format!("{i:02}.flac"),
                &format!("B{i}"),
                "Boards of Canada",
                Some("Geogaddi"),
            )
        }));
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        let before = album_snapshot(&db);
        assert_eq!(before.len(), 2, "two releases to start with");

        db.upsert_tracks(&[in_dir(a, "05.flac", "A5", "Air", Some("Moon Safari"))])
            .unwrap();
        db.rebuild_albums_scoped(&[a.to_string()], &[]).unwrap();
        let scoped = album_snapshot(&db);

        db.rebuild_albums().unwrap();
        assert_eq!(
            scoped,
            album_snapshot(&db),
            "scoped rebuild diverged from full"
        );
        assert_ne!(scoped, before, "the touched album should have changed");
        let air = scoped
            .iter()
            .find(|r| r.1 == "Moon Safari")
            .expect("album a");
        assert_eq!(air.3, 6, "the added track is counted");
        let boc = scoped.iter().find(|r| r.1 == "Geogaddi").expect("album b");
        assert_eq!(boc.3, 4, "the untouched album is left intact");
    }

    /// Deleting the last track of an album has to remove its row, and the keys are gone
    /// from `tracks` by the time the rebuild runs — so the caller passes them in.
    #[test]
    fn a_scoped_rebuild_drops_an_album_whose_tracks_all_went_away() {
        let (db, _d) = tmp_db();
        let a = "/music/Air - Moon Safari";
        let b = "/music/Boards of Canada - Geogaddi";
        let mut rows: Vec<TrackRow> = (0..3)
            .map(|i| {
                in_dir(
                    a,
                    &format!("{i:02}.flac"),
                    &format!("A{i}"),
                    "Air",
                    Some("Moon Safari"),
                )
            })
            .collect();
        rows.extend((0..4).map(|i| {
            in_dir(
                b,
                &format!("{i:02}.flac"),
                &format!("B{i}"),
                "Boards of Canada",
                Some("Geogaddi"),
            )
        }));
        db.upsert_tracks(&rows).unwrap();
        db.rebuild_albums().unwrap();
        assert_eq!(album_snapshot(&db).len(), 2);

        let gone: Vec<PathBuf> = (0..3)
            .map(|i| PathBuf::from(format!("{a}/{i:02}.flac")))
            .collect();
        let keys = db.album_keys_for_paths(&gone).unwrap();
        assert!(
            !keys.is_empty(),
            "the doomed rows still carry their album key"
        );
        db.delete_paths(&gone).unwrap();
        db.rebuild_albums_scoped(&[a.to_string()], &keys).unwrap();
        let scoped = album_snapshot(&db);

        db.rebuild_albums().unwrap();
        assert_eq!(
            scoped,
            album_snapshot(&db),
            "scoped rebuild diverged from full"
        );
        assert_eq!(scoped.len(), 1, "the emptied album is gone");
        assert_eq!(scoped[0].1, "Geogaddi");
    }
    /// Dragging a track *up* used to fail outright: the in-place shift
    /// incremented positions with an `UPDATE`, whose row order SQLite does not
    /// define, and the first row moved collided with the second on the unique
    /// `(playlist_id, position)` key.
    #[test]
    fn playlist_reorder_moves_in_both_directions() {
        let (db, _d) = tmp_db();
        let paths = ["/l/a.flac", "/l/b.flac", "/l/c.flac", "/l/d.flac"];
        db.upsert_tracks(&paths.iter().map(|p| row(p)).collect::<Vec<_>>())
            .unwrap();
        let id = db.playlist_create("p").unwrap();
        db.playlist_add_tracks(id, &paths.map(String::from))
            .unwrap();

        let order = |db: &LibraryDb| {
            db.playlist_get(id)
                .unwrap()
                .unwrap()
                .1
                .into_iter()
                .map(|t| t.path)
                .collect::<Vec<_>>()
        };

        db.playlist_reorder(id, 0, 3).unwrap();
        assert_eq!(
            order(&db),
            ["/l/b.flac", "/l/c.flac", "/l/d.flac", "/l/a.flac"]
        );

        db.playlist_reorder(id, 3, 1).unwrap();
        assert_eq!(
            order(&db),
            ["/l/b.flac", "/l/a.flac", "/l/c.flac", "/l/d.flac"]
        );
    }

    /// A library written before radio had more than one directory keys its
    /// stations on a bare radio-browser uuid. The upgrade has to prefix them in
    /// place: a favourite that loses its identity is a favourite the user has to
    /// find again.
    #[test]
    fn v14_prefixes_existing_stations_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE radio_stations (
                   uuid TEXT PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL,
                   homepage TEXT, favicon TEXT, tags TEXT, country_code TEXT,
                   language TEXT, codec TEXT, bitrate INTEGER,
                   favorite INTEGER NOT NULL DEFAULT 0,
                   play_count INTEGER NOT NULL DEFAULT 0,
                   last_played_at INTEGER, saved_at INTEGER NOT NULL);
                 INSERT INTO radio_stations (uuid, name, url, favorite, play_count, saved_at)
                   VALUES ('abc-123', 'Old Favourite', 'https://s.example/1', 1, 7, 1);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 13i64).unwrap();
        }

        let read_back = |db: &LibraryDb| {
            let conn = db.conn().lock().unwrap();
            conn.query_row(
                "SELECT uuid, source, favorite, play_count FROM radio_stations",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .unwrap()
        };

        let db = LibraryDb::open(&path).unwrap();
        assert_eq!(
            read_back(&db),
            ("radiobrowser:abc-123".into(), "radiobrowser".into(), 1, 7)
        );
        drop(db);

        // Re-opening must not prefix a second time.
        let db = LibraryDb::open(&path).unwrap();
        assert_eq!(
            read_back(&db),
            ("radiobrowser:abc-123".into(), "radiobrowser".into(), 1, 7)
        );
    }

    /// v15 runs on top of v14 on the same database. The columns it adds hold
    /// user edits, so a station that had none must come back untouched.
    #[test]
    fn v15_adds_the_edit_columns_and_leaves_the_station_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE radio_stations (
                   uuid TEXT PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL,
                   homepage TEXT, favicon TEXT, tags TEXT, country_code TEXT,
                   language TEXT, codec TEXT, bitrate INTEGER,
                   favorite INTEGER NOT NULL DEFAULT 0,
                   play_count INTEGER NOT NULL DEFAULT 0,
                   last_played_at INTEGER, saved_at INTEGER NOT NULL);
                 INSERT INTO radio_stations (uuid, name, url, favorite, saved_at)
                   VALUES ('abc', 'Old', 'https://s.example/1', 1, 1);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 13i64).unwrap();
        }

        let db = LibraryDb::open(&path).unwrap();
        let row: (String, Option<String>, Option<String>, Option<String>) = {
            let conn = db.conn().lock().unwrap();
            conn.query_row(
                "SELECT uuid, cover_id, headers, overrides FROM radio_stations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
        };
        assert_eq!(row, ("radiobrowser:abc".into(), None, None, None));

        let version: i64 = {
            let conn = db.conn().lock().unwrap();
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(version, USER_VERSION);
    }
}
