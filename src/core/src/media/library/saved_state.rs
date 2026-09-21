use rusqlite::params;

use crate::errors::{MhError, MhResult};
use crate::services::common::library::OwnedPlaylistRow;

use super::db::LibraryDb;

pub const SAVED_STATE_TTL_SECS: i64 = 60;

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl LibraryDb {
    pub fn saved_state_upsert(
        &self,
        platform: &str,
        kind: &str,
        ids: &[String],
        library_ids: &[Option<String>],
    ) -> MhResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        if !library_ids.is_empty() && library_ids.len() != ids.len() {
            return Err(MhError::Other(
                "saved_state_upsert: library_ids length must match ids length".into(),
            ));
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = now_secs();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO service_saved_state \
                     (platform, kind, service_id, library_id, saved_at, fetched_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5) \
                     ON CONFLICT(platform, kind, service_id) DO UPDATE SET \
                       library_id = COALESCE(excluded.library_id, library_id), \
                       fetched_at = excluded.fetched_at",
            )?;
            for (i, id) in ids.iter().enumerate() {
                let lib_id = library_ids.get(i).and_then(|x| x.clone());
                stmt.execute(params![platform, kind, id, lib_id, now])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn saved_state_remove(&self, platform: &str, kind: &str, ids: &[String]) -> MhResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "DELETE FROM service_saved_state \
                     WHERE platform = ?1 AND kind = ?2 AND service_id = ?3",
            )?;
            for id in ids {
                stmt.execute(params![platform, kind, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn saved_state_replace_all(
        &self,
        platform: &str,
        kind: &str,
        ids: &[String],
        library_ids: &[Option<String>],
    ) -> MhResult<()> {
        if !library_ids.is_empty() && library_ids.len() != ids.len() {
            return Err(MhError::Other(
                "saved_state_replace_all: library_ids length must match ids length".into(),
            ));
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM service_saved_state WHERE platform = ?1 AND kind = ?2",
            params![platform, kind],
        )?;
        let now = now_secs();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO service_saved_state \
                     (platform, kind, service_id, library_id, saved_at, fetched_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )?;
            for (i, id) in ids.iter().enumerate() {
                let lib_id = library_ids.get(i).and_then(|x| x.clone());
                stmt.execute(params![platform, kind, id, lib_id, now])?;
            }
        }
        tx.execute(
            "INSERT INTO service_state_sync (platform, kind, last_full_refresh) VALUES (?1, ?2, ?3) \
             ON CONFLICT(platform, kind) DO UPDATE SET last_full_refresh = excluded.last_full_refresh",
            params![platform, kind, now],
        )
        ?;
        tx.commit()?;
        Ok(())
    }

    pub fn saved_state_ids_for(&self, platform: &str, kind: &str) -> MhResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT service_id FROM service_saved_state \
                 WHERE platform = ?1 AND kind = ?2 ORDER BY saved_at DESC",
        )?;
        let rows = stmt
            .query_map(params![platform, kind], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn saved_state_last_refresh(&self, platform: &str, kind: &str) -> MhResult<i64> {
        let conn = self.conn.lock().unwrap();
        let r: Option<i64> = conn
            .query_row(
                "SELECT last_full_refresh FROM service_state_sync \
                 WHERE platform = ?1 AND kind = ?2",
                params![platform, kind],
                |row| row.get(0),
            )
            .ok();
        Ok(r.unwrap_or(0))
    }

    pub fn saved_state_is_stale(&self, platform: &str, kind: &str) -> MhResult<bool> {
        let last = self.saved_state_last_refresh(platform, kind)?;
        Ok(now_secs() - last >= SAVED_STATE_TTL_SECS)
    }

    pub fn owned_playlists_replace_all(
        &self,
        platform: &str,
        rows: &[OwnedPlaylistRow],
    ) -> MhResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM service_playlist_owned WHERE platform = ?1",
            params![platform],
        )?;
        let now = now_secs();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO service_playlist_owned \
                     (platform, service_id, name, cover_id, track_count, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for r in rows {
                stmt.execute(params![
                    platform,
                    r.service_id,
                    r.name,
                    r.cover_id,
                    r.track_count,
                    now,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn owned_playlists_for(&self, platform: &str) -> MhResult<Vec<OwnedPlaylistRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT service_id, name, cover_id, track_count, updated_at \
                 FROM service_playlist_owned WHERE platform = ?1 \
                 ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![platform], |row| {
                Ok(OwnedPlaylistRow {
                    platform: platform.to_string(),
                    service_id: row.get::<_, String>(0)?,
                    name: row.get::<_, String>(1)?,
                    cover_id: row.get::<_, Option<String>>(2)?,
                    track_count: row.get::<_, i64>(3)?,
                    updated_at: row.get::<_, i64>(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn owned_playlists_upsert_one(
        &self,
        platform: &str,
        service_id: &str,
        name: &str,
        cover_id: Option<&str>,
        track_count: i64,
    ) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = now_secs();
        conn.execute(
            "INSERT INTO service_playlist_owned \
             (platform, service_id, name, cover_id, track_count, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(platform, service_id) DO UPDATE SET \
               name = excluded.name, \
               cover_id = COALESCE(excluded.cover_id, cover_id), \
               track_count = excluded.track_count, \
               updated_at = excluded.updated_at",
            params![platform, service_id, name, cover_id, track_count, now],
        )?;
        Ok(())
    }

    pub fn owned_playlists_delete_one(&self, platform: &str, service_id: &str) -> MhResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM service_playlist_owned WHERE platform = ?1 AND service_id = ?2",
            params![platform, service_id],
        )?;
        Ok(())
    }
}
