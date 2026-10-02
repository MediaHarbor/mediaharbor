use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::media::library::db::LibraryDb;

pub fn quality_rank(platform: &str, quality: &str) -> i64 {
    match crate::services::common::library::normalize_platform_key(platform).as_str() {
        "spotify" => match quality {
            "aac-high" => 2,
            "aac-medium" => 1,
            _ => 0,
        },
        "apple" | "applemusic" | "apple_music" => match quality {
            "alac" => 6,
            "atmos" => 5,
            "ac3" => 4,
            "aac" | "aac-256" | "aac-binaural" | "aac-downmix" => 3,
            "aac-128" => 2,
            "aac-he" | "aac-he-64" | "aac-he-binaural" | "aac-he-downmix" => 1,
            _ => 0,
        },
        "tidal" | "qobuz" => quality.parse::<i64>().unwrap_or(0),
        "tidal-video" => quality
            .trim()
            .trim_end_matches(['p', 'P'])
            .parse::<i64>()
            .unwrap_or(0),
        "deezer" => match quality.to_uppercase().as_str() {
            "FLAC" | "2" => 3,
            "MP3_320" | "1" => 2,
            _ => 1,
        },
        _ => 0,
    }
}

pub struct DedupLedger {
    db: Arc<LibraryDb>,
    enabled: bool,
    seen: Mutex<HashMap<String, String>>,
}

impl DedupLedger {
    pub fn new(db: Arc<LibraryDb>, enabled: bool) -> Self {
        Self {
            db,
            enabled,
            seen: Mutex::new(HashMap::new()),
        }
    }

    fn key(platform: &str, track_id: &str) -> String {
        format!("{platform}\u{1}{track_id}")
    }

    pub fn existing_path(
        &self,
        platform: &str,
        track_id: &str,
        requested_rank: i64,
    ) -> Option<String> {
        if !self.enabled {
            return None;
        }
        if let Some(p) = self
            .seen
            .lock()
            .unwrap()
            .get(&Self::key(platform, track_id))
        {
            return Some(p.clone());
        }
        match self.db.downloaded_track(platform, track_id) {
            Ok(Some((path, rank))) if rank >= requested_rank && Path::new(&path).exists() => {
                Some(path)
            }
            _ => None,
        }
    }

    pub fn should_skip(&self, platform: &str, track_id: &str, requested_rank: i64) -> bool {
        self.existing_path(platform, track_id, requested_rank)
            .is_some()
    }

    pub fn record(&self, platform: &str, track_id: &str, path: &str, rank: i64) {
        self.seen
            .lock()
            .unwrap()
            .insert(Self::key(platform, track_id), path.to_string());
        let _ = self.db.record_download(platform, track_id, path, rank);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_rank_order() {
        assert!(quality_rank("spotify", "aac-high") > quality_rank("spotify", "aac-medium"));
    }

    #[test]
    fn apple_rank_order() {
        assert!(quality_rank("apple", "atmos") > quality_rank("apple", "aac-256"));
        assert!(quality_rank("apple", "aac-256") > quality_rank("apple", "aac-128"));
        assert!(quality_rank("apple", "aac-128") > quality_rank("apple", "aac-he-64"));
    }

    #[test]
    fn numeric_tiers() {
        assert!(quality_rank("qobuz", "27") > quality_rank("qobuz", "6"));
        assert!(quality_rank("tidal", "3") > quality_rank("tidal", "1"));
    }

    #[test]
    fn deezer_flac_beats_mp3() {
        assert!(quality_rank("deezer", "FLAC") > quality_rank("deezer", "320"));
    }
}
