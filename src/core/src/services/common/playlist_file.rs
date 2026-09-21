use std::path::{Path, PathBuf};

/// One entry of a written playlist file.
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub duration_secs: Option<u64>,
}

/// Renders an extended M3U. Paths are written relative to the playlist's own
/// directory when possible so the folder stays portable.
pub fn render_m3u8(entries: &[PlaylistEntry], base_dir: &Path) -> String {
    let mut out = String::from("#EXTM3U\n");
    for e in entries {
        let secs = e.duration_secs.map(|d| d as i64).unwrap_or(-1);
        let label = if e.artist.trim().is_empty() {
            e.title.clone()
        } else {
            format!("{} - {}", e.artist, e.title)
        };
        out.push_str(&format!("#EXTINF:{},{}\n", secs, label));
        let rel = e
            .path
            .strip_prefix(base_dir)
            .unwrap_or(e.path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        out.push_str(&rel);
        out.push('\n');
    }
    out
}

/// Builds an entry from a file that has already been tagged, so the `#EXTINF`
/// label reads as "Artist - Title" rather than whatever the filename template
/// happened to produce. Falls back to the file stem when the tags cannot be read.
pub fn entry_from_tagged_file(path: PathBuf) -> PlaylistEntry {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::{Accessor, ItemKey};

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let Ok(tagged) = lofty::read_from_path(&path) else {
        return PlaylistEntry {
            path,
            title: stem,
            artist: String::new(),
            duration_secs: None,
        };
    };
    let duration_secs = Some(tagged.properties().duration().as_secs()).filter(|d| *d > 0);
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let title = tag
        .and_then(|t| t.title().map(|c| c.into_owned()))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(stem);
    let artist = tag
        .and_then(|t| t.get_string(ItemKey::TrackArtist).map(str::to_string))
        .unwrap_or_default();

    PlaylistEntry {
        path,
        title,
        artist,
        duration_secs,
    }
}

/// Writes `<dir>/<name>.m3u8` listing the tracks that actually landed.
pub async fn write_playlist_file(
    dir: &Path,
    name: &str,
    entries: &[PlaylistEntry],
    on_log: impl Fn(String),
) {
    if entries.is_empty() {
        return;
    }
    let stem = crate::services::common::pipeline::safe_name(name);
    let stem = if stem.trim().is_empty() {
        "Playlist".to_string()
    } else {
        stem
    };
    let path = dir.join(format!("{stem}.m3u8"));
    match tokio::fs::write(&path, render_m3u8(entries, dir)).await {
        Ok(()) => on_log(format!("  ✓ playlist → {}", path.display())),
        Err(e) => on_log(format!("  ⚠ could not write {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_relative_and_labelled() {
        let dir = Path::new("/music/Playlists/Mix");
        let entries = vec![
            PlaylistEntry {
                path: dir.join("01. Daft Punk - Give Life Back to Music.m4a"),
                title: "Give Life Back to Music".into(),
                artist: "Daft Punk".into(),
                duration_secs: Some(272),
            },
            PlaylistEntry {
                path: dir.join("02. Untitled.m4a"),
                title: "Untitled".into(),
                artist: String::new(),
                duration_secs: None,
            },
        ];
        let m3u = render_m3u8(&entries, dir);
        let lines: Vec<&str> = m3u.lines().collect();
        assert_eq!(lines[0], "#EXTM3U");
        assert_eq!(lines[1], "#EXTINF:272,Daft Punk - Give Life Back to Music");
        assert_eq!(lines[2], "01. Daft Punk - Give Life Back to Music.m4a");
        assert_eq!(lines[3], "#EXTINF:-1,Untitled");
        assert_eq!(lines[4], "02. Untitled.m4a");
    }

    #[test]
    fn paths_outside_the_folder_stay_absolute() {
        let dir = Path::new("/music/Playlists/Mix");
        let entries = vec![PlaylistEntry {
            path: PathBuf::from("/elsewhere/track.m4a"),
            title: "Track".into(),
            artist: "A".into(),
            duration_secs: Some(1),
        }];
        assert!(render_m3u8(&entries, dir).contains("/elsewhere/track.m4a"));
    }
}
