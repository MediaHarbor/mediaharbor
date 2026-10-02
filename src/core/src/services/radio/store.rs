use std::collections::{HashMap, HashSet};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::errors::{MhError, MhResult};
use crate::media::library::db::LibraryDb;
use crate::services::radio::model::{Station, StreamKind};
use crate::services::radio::overrides::{StationEdit, StationOverrides};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS radio_stations (
  uuid          TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  url           TEXT NOT NULL,
  homepage      TEXT,
  favicon       TEXT,
  tags          TEXT,
  country_code  TEXT,
  language      TEXT,
  codec         TEXT,
  bitrate       INTEGER,
  favorite      INTEGER NOT NULL DEFAULT 0,
  play_count    INTEGER NOT NULL DEFAULT 0,
  last_played_at INTEGER,
  saved_at      INTEGER NOT NULL,
  source        TEXT NOT NULL DEFAULT 'radiobrowser',
  country       TEXT,
  stream_kind   TEXT NOT NULL DEFAULT 'direct',
  alt_urls      TEXT,
  cover_id      TEXT,
  headers       TEXT,
  overrides     TEXT
);
CREATE INDEX IF NOT EXISTS idx_radio_favorite ON radio_stations(favorite);
CREATE INDEX IF NOT EXISTS idx_radio_played   ON radio_stations(last_played_at);

CREATE TABLE IF NOT EXISTS radio_lists (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS radio_list_stations (
  list_id     INTEGER NOT NULL REFERENCES radio_lists(id) ON DELETE CASCADE,
  station_key TEXT NOT NULL,
  position    INTEGER NOT NULL,
  PRIMARY KEY (list_id, position)
);

CREATE INDEX IF NOT EXISTS idx_rls_station ON radio_list_stations(station_key);
"#;

/// Indexes that reference a v14 column, and so cannot live in [`SCHEMA`].
///
/// `SCHEMA` runs unconditionally on every open, before the version ladder. On an
/// existing library `CREATE TABLE IF NOT EXISTS radio_stations` is a no-op, so an
/// index over `source` in there would be built against a table that has no such
/// column yet — and the library would fail to open at all on upgrade.
pub const V14_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_radio_source ON radio_stations(source);
"#;

/// The columns v14 added to a `radio_stations` that predates qualified keys.
/// `db.rs` owns the version ladder; the shape stays here beside the `SCHEMA`
/// that a fresh database is built from.
pub const V14_COLUMNS: &[(&str, &str)] = &[
    ("source", "TEXT NOT NULL DEFAULT 'radiobrowser'"),
    ("country", "TEXT"),
    ("stream_kind", "TEXT NOT NULL DEFAULT 'direct'"),
    ("alt_urls", "TEXT"),
];

/// The columns v15 added, when stations became editable.
///
/// All three hold data no directory supplies, which is why `remember()` leaves
/// them alone: it names the columns it updates, and these are not among them.
pub const V15_COLUMNS: &[(&str, &str)] = &[
    ("cover_id", "TEXT"),
    ("headers", "TEXT"),
    ("overrides", "TEXT"),
];

const COLUMNS: &str = "uuid, source, name, url, alt_urls, stream_kind, homepage, favicon, tags, \
                       country, country_code, language, codec, bitrate, cover_id, headers, \
                       overrides";

/// The same list qualified for the join in `list_get`. Spelled out rather than
/// derived from [`COLUMNS`] at run time, so the SQL text is a constant and the
/// statement cache can hold it. `map_station` reads them positionally, so the
/// two lists have to stay in the same order — which the test below pins.
const JOINED_COLUMNS: &str = "st.uuid, st.source, st.name, st.url, st.alt_urls, st.stream_kind, \
                              st.homepage, st.favicon, st.tags, st.country, st.country_code, \
                              st.language, st.codec, st.bitrate, st.cover_id, st.headers, \
                              st.overrides";

/// A station split into what the directory published and what the user changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStationDetail {
    pub base: Station,
    pub overrides: StationOverrides,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListRow {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub station_count: i64,
    /// Up to four station icons, for the list's cover mosaic.
    pub favicons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListDetail {
    #[serde(flatten)]
    pub list: RadioListRow,
    pub stations: Vec<Station>,
}

/// Favourites, history, the user's own stations and their lists live in
/// `library.sqlite` beside the rest of the library rather than in browser
/// storage, so they survive a cache clear and are reachable from the backend
/// that has to resolve a stream.
pub struct RadioStore;

impl RadioStore {
    pub fn remember(db: &LibraryDb, station: &Station) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        let alt = serde_json::to_string(&station.alt_urls).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO radio_stations \
               (uuid, source, name, url, alt_urls, stream_kind, homepage, favicon, tags, \
                country, country_code, language, codec, bitrate, saved_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) \
             ON CONFLICT(uuid) DO UPDATE SET \
               source = excluded.source, name = excluded.name, \
               url = CASE WHEN excluded.url <> '' THEN excluded.url ELSE radio_stations.url END, \
               alt_urls = excluded.alt_urls, stream_kind = excluded.stream_kind, \
               homepage = excluded.homepage, favicon = excluded.favicon, \
               tags = excluded.tags, country = excluded.country, \
               country_code = excluded.country_code, language = excluded.language, \
               codec = excluded.codec, bitrate = excluded.bitrate",
            rusqlite::params![
                station.key,
                station.source,
                station.name,
                station.stream_url,
                alt,
                station.stream_kind.as_str(),
                station.homepage,
                station.favicon,
                station.tags,
                station.country,
                station.country_code,
                station.language,
                station.codec,
                station.bitrate,
                now_secs(),
            ],
        )?;
        Ok(())
    }

    pub fn set_favorite(db: &LibraryDb, key: &str, favorite: bool) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        conn.execute(
            "UPDATE radio_stations SET favorite = ?1 WHERE uuid = ?2",
            rusqlite::params![favorite as i64, key],
        )?;
        Ok(())
    }

    pub fn record_play(db: &LibraryDb, key: &str) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        conn.execute(
            "UPDATE radio_stations \
             SET play_count = play_count + 1, last_played_at = ?1 WHERE uuid = ?2",
            rusqlite::params![now_secs(), key],
        )?;
        Ok(())
    }

    /// Drops a station the user owns outright, along with its place in every
    /// list. Directory stations are only ever un-favourited, never deleted.
    pub fn forget(db: &LibraryDb, key: &str) -> MhResult<()> {
        let mut conn = db.conn().lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM radio_list_stations WHERE station_key = ?1",
            rusqlite::params![key],
        )?;
        tx.execute(
            "DELETE FROM radio_stations WHERE uuid = ?1",
            rusqlite::params![key],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn favorites(db: &LibraryDb) -> MhResult<Vec<Station>> {
        Self::query(
            db,
            "WHERE favorite = 1 ORDER BY saved_at DESC",
            200,
            &[],
        )
    }

    /// What was on recently. The shelf that shows it is a fixed size, so the
    /// window is the store's business rather than a caller's.
    pub fn recent(db: &LibraryDb) -> MhResult<Vec<Station>> {
        const RECENT_LIMIT: i64 = 24;
        Self::query(
            db,
            "WHERE last_played_at IS NOT NULL ORDER BY last_played_at DESC",
            RECENT_LIMIT,
            &[],
        )
    }

    pub fn by_source(db: &LibraryDb, source: &str, limit: i64) -> MhResult<Vec<Station>> {
        Self::query(
            db,
            "WHERE source = ?1 ORDER BY name COLLATE NOCASE",
            limit,
            &[&source],
        )
    }

    pub fn get(db: &LibraryDb, key: &str) -> MhResult<Option<Station>> {
        Ok(
            Self::query(db, "WHERE uuid = ?1", 1, &[&key])?
                .into_iter()
                .next(),
        )
    }

    /// The station as the directory gave it, alongside the user's changes.
    /// The editor needs both so it can offer "reset to the directory's value".
    pub fn detail(db: &LibraryDb, key: &str) -> MhResult<Option<RadioStationDetail>> {
        let conn = db.conn().lock().unwrap();
        let sql = format!("SELECT {COLUMNS} FROM radio_stations WHERE uuid = ?1");
        let mut stmt = conn.prepare_cached(&sql)?;
        let found = stmt
            .query_row(rusqlite::params![key], map_row)
            .optional()?
            .map(|(base, overrides)| RadioStationDetail { base, overrides });
        Ok(found)
    }

    /// Applies an edit, storing only what actually differs from the directory's
    /// own values. Headers go straight to their column — nothing upstream
    /// supplies them, so there is nothing to diff against.
    pub fn update(db: &LibraryDb, key: &str, edit: &StationEdit) -> MhResult<()> {
        let Some(detail) = Self::detail(db, key)? else {
            return Err(MhError::NotFound(format!("station {key} is not saved")));
        };
        let overrides = StationOverrides::from_edit(&detail.base, edit);
        let blob = if overrides.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&overrides).map_err(|e| MhError::Other(e.to_string()))?)
        };
        let (touch_headers, header_blob) = match &edit.headers {
            Some(headers) if headers.is_empty() => (true, None),
            Some(headers) => (
                true,
                Some(serde_json::to_string(headers).map_err(|e| MhError::Other(e.to_string()))?),
            ),
            None => (false, None),
        };
        let conn = db.conn().lock().unwrap();
        conn.execute(
            // One statement for both columns. An edit that does not mention
            // headers must leave that column alone rather than clear it, which is
            // what the CASE is guarding.
            "UPDATE radio_stations SET overrides = ?1, \
                    headers = CASE WHEN ?2 THEN ?3 ELSE headers END \
             WHERE uuid = ?4",
            rusqlite::params![blob, touch_headers, header_blob, key],
        )?;
        Ok(())
    }

    /// Drops every edit, putting the station back to what the directory says.
    pub fn reset(db: &LibraryDb, key: &str) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        conn.execute(
            "UPDATE radio_stations SET overrides = NULL, cover_id = NULL, headers = NULL \
             WHERE uuid = ?1",
            rusqlite::params![key],
        )?;
        Ok(())
    }

    /// Points the station at a cover already ingested into the shared cover
    /// cache. `None` goes back to the directory's own icon.
    pub fn set_cover(db: &LibraryDb, key: &str, cover_id: Option<&str>) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        conn.execute(
            "UPDATE radio_stations SET cover_id = ?1 WHERE uuid = ?2",
            rusqlite::params![cover_id, key],
        )?;
        Ok(())
    }

    /// Every station read goes through here. The limit is bound rather than
    /// interpolated so the statement text is fixed for a given `tail` and
    /// `prepare_cached` can actually cache it — this runs on every play.
    fn query(
        db: &LibraryDb,
        tail: &str,
        limit: i64,
        params: &[&dyn rusqlite::ToSql],
    ) -> MhResult<Vec<Station>> {
        let conn = db.conn().lock().unwrap();
        let sql = format!("SELECT {COLUMNS} FROM radio_stations {tail} LIMIT ?");
        let mut stmt = conn.prepare_cached(&sql)?;
        let mut bound: Vec<&dyn rusqlite::ToSql> = params.to_vec();
        bound.push(&limit);
        let rows = stmt
            .query_map(bound.as_slice(), map_station)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Which of `keys` already have a row, in one statement rather than one
    /// `get` per key.
    pub fn existing_keys(db: &LibraryDb, keys: &[String]) -> MhResult<HashSet<String>> {
        if keys.is_empty() {
            return Ok(HashSet::new());
        }
        let conn = db.conn().lock().unwrap();
        let placeholders = vec!["?"; keys.len()].join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT uuid FROM radio_stations WHERE uuid IN ({placeholders})"
        ))?;
        let found = stmt
            .query_map(rusqlite::params_from_iter(keys), |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(found)
    }

    pub fn list_create(db: &LibraryDb, name: &str) -> MhResult<i64> {
        let conn = db.conn().lock().unwrap();
        let now = now_secs();
        conn.execute(
            "INSERT INTO radio_lists (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
            rusqlite::params![name, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_rename(db: &LibraryDb, id: i64, name: &str) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        conn.execute(
            "UPDATE radio_lists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![name, now_secs(), id],
        )?;
        Ok(())
    }

    pub fn list_delete(db: &LibraryDb, id: i64) -> MhResult<()> {
        let conn = db.conn().lock().unwrap();
        conn.execute(
            "DELETE FROM radio_lists WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    /// Every list with its cover mosaic, in two statements rather than one per
    /// list: a window function takes the first four icons of each partition, so
    /// ten lists cost two queries instead of eleven.
    pub fn lists(db: &LibraryDb) -> MhResult<Vec<RadioListRow>> {
        let conn = db.conn().lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT l.id, l.name, l.created_at, l.updated_at, \
                    (SELECT COUNT(*) FROM radio_list_stations s WHERE s.list_id = l.id) \
             FROM radio_lists l ORDER BY l.updated_at DESC",
        )?;
        let metas: Vec<(i64, String, i64, i64, i64)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut icons = conn.prepare_cached(
            "SELECT list_id, favicon FROM ( \
                 SELECT rls.list_id AS list_id, st.favicon AS favicon, \
                        ROW_NUMBER() OVER (PARTITION BY rls.list_id ORDER BY rls.position) AS rn \
                 FROM radio_list_stations rls \
                 JOIN radio_stations st ON st.uuid = rls.station_key \
                 WHERE st.favicon IS NOT NULL AND st.favicon <> '' \
             ) WHERE rn <= 4",
        )?;
        let mut mosaics: HashMap<i64, Vec<String>> = HashMap::new();
        for row in icons.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (list_id, favicon) = row?;
            mosaics.entry(list_id).or_default().push(favicon);
        }

        Ok(metas
            .into_iter()
            .map(
                |(id, name, created_at, updated_at, station_count)| RadioListRow {
                    favicons: mosaics.remove(&id).unwrap_or_default(),
                    id,
                    name,
                    created_at,
                    updated_at,
                    station_count,
                },
            )
            .collect())
    }

    pub fn list_get(db: &LibraryDb, id: i64) -> MhResult<Option<RadioListDetail>> {
        let conn = db.conn().lock().unwrap();
        let meta: Option<(String, i64, i64)> = conn
            .query_row(
                "SELECT name, created_at, updated_at FROM radio_lists WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((name, created_at, updated_at)) = meta else {
            return Ok(None);
        };
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {JOINED_COLUMNS} FROM radio_list_stations rls \
             JOIN radio_stations st ON st.uuid = rls.station_key \
             WHERE rls.list_id = ?1 ORDER BY rls.position"
        ))?;
        let stations: Vec<Station> = stmt
            .query_map(rusqlite::params![id], map_station)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let favicons = stations
            .iter()
            .filter_map(|s| s.favicon.clone())
            .take(4)
            .collect();
        Ok(Some(RadioListDetail {
            list: RadioListRow {
                id,
                name,
                created_at,
                updated_at,
                station_count: stations.len() as i64,
                favicons,
            },
            stations,
        }))
    }

    pub fn list_add(db: &LibraryDb, id: i64, keys: &[String]) -> MhResult<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut conn = db.conn().lock().unwrap();
        let tx = conn.transaction()?;
        let mut position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM radio_list_stations WHERE list_id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO radio_list_stations (list_id, station_key, position) \
                 VALUES (?1, ?2, ?3)",
            )?;
            let mut seen = tx.prepare_cached(
                "SELECT 1 FROM radio_list_stations WHERE list_id = ?1 AND station_key = ?2",
            )?;
            for key in keys {
                // A list is an ordered set: adding a station twice would give it
                // two positions and make reordering ambiguous.
                if seen.exists(rusqlite::params![id, key])? {
                    continue;
                }
                stmt.execute(rusqlite::params![id, key, position])?;
                position += 1;
            }
        }
        tx.execute(
            "UPDATE radio_lists SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_secs(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_remove_at(db: &LibraryDb, id: i64, position: i64) -> MhResult<()> {
        let mut conn = db.conn().lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM radio_list_stations WHERE list_id = ?1 AND position = ?2",
            rusqlite::params![id, position],
        )?;
        tx.execute(
            "UPDATE radio_list_stations SET position = position - 1 \
             WHERE list_id = ?1 AND position > ?2",
            rusqlite::params![id, position],
        )?;
        tx.execute(
            "UPDATE radio_lists SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_secs(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Moves one station to another position.
    ///
    /// The rows are rewritten from a permuted list rather than shifted in place.
    /// A shift cannot be expressed safely here: `(list_id, position)` is unique,
    /// SQLite gives an `UPDATE` no row order, and moving a station *up* has to
    /// increment positions — the first row incremented lands on the second row's
    /// position and the statement fails on the constraint.
    pub fn list_reorder(db: &LibraryDb, id: i64, from: i64, to: i64) -> MhResult<()> {
        if from == to {
            return Ok(());
        }
        let mut conn = db.conn().lock().unwrap();
        let tx = conn.transaction()?;
        let mut keys: Vec<String> = {
            let mut stmt = tx.prepare_cached(
                "SELECT station_key FROM radio_list_stations \
                 WHERE list_id = ?1 ORDER BY position",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let len = keys.len() as i64;
        if from < 0 || to < 0 || from >= len || to >= len {
            return Ok(());
        }
        let moved = keys.remove(from as usize);
        keys.insert(to as usize, moved);

        tx.execute(
            "DELETE FROM radio_list_stations WHERE list_id = ?1",
            rusqlite::params![id],
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO radio_list_stations (list_id, station_key, position) \
                 VALUES (?1, ?2, ?3)",
            )?;
            for (position, key) in keys.iter().enumerate() {
                stmt.execute(rusqlite::params![id, key, position as i64])?;
            }
        }
        tx.execute(
            "UPDATE radio_lists SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_secs(), id],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// The stored row exactly as the directory left it, plus whatever the user
/// changed on top of it. Kept separate so the editor can show both.
fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Station, StationOverrides)> {
    let key: String = r.get(0)?;
    let source: String = r.get(1)?;
    let source_id = key
        .split_once(':')
        .map(|(_, id)| id.to_string())
        .unwrap_or_else(|| key.clone());
    let alt: Option<String> = r.get(4)?;
    let headers: Option<String> = r.get(15)?;
    let overrides: Option<String> = r.get(16)?;
    let base = Station {
        key,
        source,
        source_id,
        name: r.get(2)?,
        stream_url: r.get(3)?,
        alt_urls: alt
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        stream_kind: StreamKind::parse(&r.get::<_, String>(5)?),
        homepage: r.get(6)?,
        favicon: r.get(7)?,
        tags: r.get(8)?,
        country: r.get(9)?,
        country_code: r.get(10)?,
        language: r.get(11)?,
        codec: r.get(12)?,
        bitrate: r.get::<_, Option<i64>>(13)?.map(|v| v as u32),
        votes: None,
        clickcount: None,
        cover_id: r.get(14)?,
        headers: headers
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
    };
    // A blob that no longer parses is treated as no overrides rather than as a
    // failure to read the station at all.
    let overrides = overrides
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok((base, overrides))
}

/// What every ordinary read wants: the station as the user has it.
fn map_station(r: &rusqlite::Row<'_>) -> rusqlite::Result<Station> {
    let (mut station, overrides) = map_row(r)?;
    overrides.apply(&mut station);
    Ok(station)
}

/// Seconds since the epoch, as SQLite stores them. The shared helper answers in
/// `u64`; every timestamp column here is signed.
fn now_secs() -> i64 {
    crate::services::common::ids::now_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::radio::model::station_key;

    fn tmp_db() -> (LibraryDb, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = LibraryDb::open(&dir.path().join("library.sqlite")).unwrap();
        (db, dir)
    }

    fn station(source: &str, source_id: &str, name: &str) -> Station {
        let mut s = Station::new(
            source,
            source_id,
            name.to_string(),
            format!("https://stream.example/{name}"),
        );
        s.favicon = Some(format!("https://icons.example/{name}.png"));
        s.tags = Some("jazz,soul".into());
        s
    }

    /// `map_station` reads both column lists positionally, so a column added to
    /// one and not the other would silently shift every field after it.
    #[test]
    fn the_plain_and_joined_column_lists_are_the_same_columns_in_the_same_order() {
        let plain: Vec<&str> = COLUMNS.split(", ").map(str::trim).collect();
        let joined: Vec<String> = JOINED_COLUMNS
            .split(", ")
            .map(|c| c.trim().trim_start_matches("st.").to_string())
            .collect();
        assert_eq!(plain, joined);
    }

    #[test]
    fn a_qualified_key_round_trips_including_a_source_id_full_of_colons() {
        let (db, _d) = tmp_db();
        // A custom station's id is the stream URL the user pasted.
        let url = "http://lissen.to:8000/chillsynth.mp3";
        let mut s = station("custom", url, "Chillsynth");
        s.alt_urls = vec!["http://backup.example:8000/chillsynth.mp3".into()];
        RadioStore::remember(&db, &s).unwrap();

        let back = RadioStore::get(&db, &station_key("custom", url))
            .unwrap()
            .expect("stored");
        assert_eq!(back.source, "custom");
        assert_eq!(back.source_id, url);
        assert_eq!(back.key, format!("custom:{url}"));
        assert_eq!(back.alt_urls, s.alt_urls);
    }

    #[test]
    fn favourites_and_plays_are_kept_across_a_refresh_of_the_station_itself() {
        let (db, _d) = tmp_db();
        let s = station("radiobrowser", "abc-123", "Test");
        RadioStore::remember(&db, &s).unwrap();
        RadioStore::set_favorite(&db, &s.key, true).unwrap();
        RadioStore::record_play(&db, &s.key).unwrap();

        let mut renamed = s.clone();
        renamed.name = "Test (renamed)".into();
        RadioStore::remember(&db, &renamed).unwrap();

        assert_eq!(RadioStore::favorites(&db).unwrap().len(), 1);
        assert_eq!(
            RadioStore::recent(&db).unwrap()[0].name,
            "Test (renamed)"
        );
    }

    /// A station browsed but not yet resolved has no stream URL. Re-remembering
    /// it must not blank the URL a resolve already found.
    #[test]
    fn an_unresolved_station_does_not_erase_a_known_stream_url() {
        let (db, _d) = tmp_db();
        let resolved = station("somafm", "groovesalad", "Groove Salad");
        RadioStore::remember(&db, &resolved).unwrap();

        let mut browsed = resolved.clone();
        browsed.stream_url = String::new();
        RadioStore::remember(&db, &browsed).unwrap();

        let back = RadioStore::get(&db, &resolved.key).unwrap().unwrap();
        assert_eq!(back.stream_url, resolved.stream_url);
    }

    fn seeded_list(db: &LibraryDb) -> (i64, Vec<String>) {
        let keys: Vec<String> = ["a", "b", "c", "d"]
            .iter()
            .map(|n| {
                let s = station("radiobrowser", n, n);
                RadioStore::remember(db, &s).unwrap();
                s.key
            })
            .collect();
        let id = RadioStore::list_create(db, "Late night").unwrap();
        RadioStore::list_add(db, id, &keys).unwrap();
        (id, keys)
    }

    fn names(db: &LibraryDb, id: i64) -> Vec<String> {
        RadioStore::list_get(db, id)
            .unwrap()
            .unwrap()
            .stations
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    #[test]
    fn list_append_is_ordered_and_ignores_a_station_already_in_the_list() {
        let (db, _d) = tmp_db();
        let (id, keys) = seeded_list(&db);
        assert_eq!(names(&db, id), ["a", "b", "c", "d"]);

        RadioStore::list_add(&db, id, &keys[..2]).unwrap();
        assert_eq!(names(&db, id), ["a", "b", "c", "d"], "no duplicates");
    }

    /// Removing from the middle has to close the gap, or the next append lands
    /// on a position that is already taken.
    #[test]
    fn list_remove_from_the_middle_closes_the_gap() {
        let (db, _d) = tmp_db();
        let (id, _) = seeded_list(&db);
        RadioStore::list_remove_at(&db, id, 1).unwrap();
        assert_eq!(names(&db, id), ["a", "c", "d"]);

        let extra = station("radiobrowser", "e", "e");
        RadioStore::remember(&db, &extra).unwrap();
        RadioStore::list_add(&db, id, &[extra.key]).unwrap();
        assert_eq!(names(&db, id), ["a", "c", "d", "e"]);
    }

    #[test]
    fn list_reorder_moves_in_both_directions() {
        let (db, _d) = tmp_db();
        let (id, _) = seeded_list(&db);

        RadioStore::list_reorder(&db, id, 0, 3).unwrap();
        assert_eq!(names(&db, id), ["b", "c", "d", "a"]);

        RadioStore::list_reorder(&db, id, 3, 1).unwrap();
        assert_eq!(names(&db, id), ["b", "a", "c", "d"]);

        RadioStore::list_reorder(&db, id, 2, 2).unwrap();
        assert_eq!(names(&db, id), ["b", "a", "c", "d"], "a no-op move");
    }

    #[test]
    fn deleting_a_list_takes_its_rows_with_it_but_leaves_the_stations() {
        let (db, _d) = tmp_db();
        let (id, keys) = seeded_list(&db);
        RadioStore::list_delete(&db, id).unwrap();
        assert!(RadioStore::list_get(&db, id).unwrap().is_none());
        assert!(RadioStore::get(&db, &keys[0]).unwrap().is_some());
    }

    /// Forgetting a station the user owns has to take it out of every list too,
    /// or the list keeps a row pointing at nothing.
    #[test]
    fn forgetting_a_station_removes_it_from_every_list() {
        let (db, _d) = tmp_db();
        let (id, keys) = seeded_list(&db);
        RadioStore::forget(&db, &keys[1]).unwrap();
        assert!(RadioStore::get(&db, &keys[1]).unwrap().is_none());
        assert_eq!(names(&db, id), ["a", "c", "d"]);
    }
}
