pub mod converter;
pub mod downloader;
pub mod orchestrator;
pub mod runner;
pub mod tagger;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// How a track is named in the progress bar — the same "Artist - Title" shape the
/// native Apple Music and Spotify engines report.
pub fn track_label(artist: &str, title: &str) -> String {
    match (artist.trim(), title.trim()) {
        ("", t) => t.to_string(),
        (a, "") => a.to_string(),
        (a, t) => format!("{a} - {t}"),
    }
}

/// Where a track's file is going, which decides both whether an album folder is
/// created for it and how its cover art is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackPlacement {
    /// On its own — gets an album folder built from the naming template.
    Loose,
    /// Inside a release folder its siblings share, so one `cover.jpg` serves all.
    Release,
    /// Inside a playlist folder holding unrelated releases, so each track keeps its
    /// own cover rather than fighting over a shared one.
    Playlist,
}

impl TrackPlacement {
    pub fn in_collection(self) -> bool {
        !matches!(self, Self::Loose)
    }
}

/// Snaps a requested cover size onto the nearest size a service actually publishes.
/// Asking a CDN for an arbitrary edge length returns a 404, not a resize.
pub fn nearest_cover_size(requested: u32, available: &[u32]) -> u32 {
    available
        .iter()
        .copied()
        .min_by_key(|s| s.abs_diff(requested))
        .unwrap_or(requested)
}

/// `(id, "Artist - Title", record)` for every entry of a playlist payload that has
/// an id. The record rides along so the download does not have to ask for the same
/// track again, one round-trip at a time.
pub fn playlist_rows(items: &serde_json::Value, artist_pointer: &str) -> Vec<TrackRow> {
    items
        .as_array()
        .map(|a| release_rows(a, artist_pointer, None))
        .unwrap_or_default()
}

/// One pass over a listing, keeping every track's id, label, disc and raw record
/// together so they cannot fall out of alignment.
///
/// `disc_key` is the per-service field holding the disc number (`disk_number`,
/// `media_number`, `volumeNumber`); `None` means the caller has no discs to track.
pub fn release_rows(
    items: &[serde_json::Value],
    artist_pointer: &str,
    disc_key: Option<&str>,
) -> Vec<TrackRow> {
    items
        .iter()
        .filter_map(|t| {
            let id = t["id"].as_u64()?.to_string();
            let artist = t
                .pointer(artist_pointer)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Some(TrackRow {
                id,
                label: track_label(artist, t["title"].as_str().unwrap_or("")),
                disc: disc_key.and_then(|k| t[k].as_u64()).unwrap_or(1) as u32,
                record: t.clone(),
            })
        })
        .collect()
}

/// Whether a track produced a new file or matched one the dedup ledger already holds.
#[derive(Debug, Clone)]
pub enum TrackOutcome {
    Downloaded(PathBuf),
    Skipped(PathBuf),
}

impl TrackOutcome {
    pub fn path(&self) -> &Path {
        match self {
            Self::Downloaded(p) | Self::Skipped(p) => p,
        }
    }

    pub fn into_path(self) -> PathBuf {
        match self {
            Self::Downloaded(p) | Self::Skipped(p) => p,
        }
    }

    pub fn was_skipped(&self) -> bool {
        matches!(self, Self::Skipped(_))
    }
}

static OPT_SEGMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[\[(][^\])\[]*\{[^}]+\}[^\])\[]*[\])]").unwrap());
static VALUE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\{(\w+)(?::(\d+))?\}").unwrap());

pub(crate) fn build_file_name(
    template: &str,
    vars: &std::collections::HashMap<&str, String>,
    restrict: bool,
    truncate: usize,
) -> String {
    let tpl = OPT_SEGMENT_RE.replace_all(template, |caps: &regex::Captures| {
        let seg = &caps[0];
        let all_empty = VALUE_RE.captures_iter(seg).all(|c| {
            let key = c.get(1).map(|m| m.as_str()).unwrap_or("");
            vars.get(key)
                .map(|v| v.is_empty() || v == "0")
                .unwrap_or(true)
        });
        if all_empty {
            String::new()
        } else {
            seg.to_string()
        }
    });

    let mut name = VALUE_RE
        .replace_all(&tpl, |caps: &regex::Captures| {
            let key = &caps[1];
            let pad = caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
            let val = vars.get(key).cloned().unwrap_or_default();
            if let Some(p) = pad {
                if val.chars().all(|c| c.is_ascii_digit()) && !val.is_empty() {
                    return format!("{:0>width$}", val, width = p);
                }
            }
            val
        })
        .to_string();

    name = if restrict {
        safe_name(&name)
    } else {
        name.chars()
            .map(|c| match c {
                '/' => '⁄',
                '\\' => '＼',
                '\x00'..='\x1f' => '_',
                other => other,
            })
            .collect()
    };

    if truncate > 0 && name.len() > truncate {
        name = truncate_str_bytes(&name, truncate).to_string();
    }
    trim_orphan_separators(&name)
}

/// Trims whitespace plus separator punctuation left dangling once an optional
/// template segment resolved empty (`"Artist - Album -"` → `"Artist - Album"`).
pub(crate) fn trim_orphan_separators(s: &str) -> String {
    let sep = |c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | '_' | ',' | ';');
    s.trim_start_matches(|c: char| sep(c) || c == '.')
        .trim_end_matches(sep)
        .to_string()
}

pub(crate) fn truncate_str_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod truncate_tests {
    use super::truncate_str_bytes;

    #[test]
    fn truncates_on_char_boundary() {
        let s = "：：：：";
        assert_eq!(truncate_str_bytes(s, 4), "：");
        assert_eq!(truncate_str_bytes(s, 6), "：：");
        assert_eq!(truncate_str_bytes(s, 100), s);
    }

    #[test]
    fn ascii_unchanged() {
        assert_eq!(truncate_str_bytes("hello", 3), "hel");
        assert_eq!(truncate_str_bytes("hi", 5), "hi");
    }
}

/// One track of a release or playlist, as the listing already described it.
///
/// `record` is the service's own payload for the track. Every service fetches
/// these with the release and used to throw them away, then re-request the same
/// fields one track at a time, mid-download, with the progress bar frozen for
/// each round-trip.
#[derive(Debug, Clone, Default)]
pub struct TrackRow {
    pub id: String,
    /// What the progress bar calls the track.
    pub label: String,
    pub disc: u32,
    pub record: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct AlbumInfo {
    pub tracks: Vec<TrackRow>,
    /// The release payload the track records came from, for the fields that live on
    /// the album rather than the track.
    pub album_record: serde_json::Value,
    pub number_of_volumes: u32,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub genre: String,
    pub label: String,
    pub bit_depth: Option<u32>,
    pub sampling_rate: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct PlaylistInfo {
    pub tracks: Vec<TrackRow>,
    pub title: String,
    pub artist: String,
}

/// Cover art fetched once per URL for the whole run. A release's tracks all point
/// at the same image, so without this a 20-track album downloaded the same JPEG 20
/// times — the native Apple Music engine has always cached it.
#[derive(Default)]
pub struct CoverCache {
    seen: std::sync::Mutex<std::collections::HashMap<String, Option<Vec<u8>>>>,
}

impl CoverCache {
    pub fn get(&self, url: &str) -> Option<Option<Vec<u8>>> {
        self.seen.lock().ok()?.get(url).cloned()
    }

    pub fn put(&self, url: &str, bytes: Option<Vec<u8>>) {
        if let Ok(mut g) = self.seen.lock() {
            g.insert(url.to_string(), bytes);
        }
    }
}

/// Formats a bit depth and sample rate the way every service in this pipeline
/// spells them, so `{quality}` reads the same whichever one produced the file.
pub fn depth_rate_label(bit_depth: u32, sample_rate_hz: u32) -> String {
    let khz = sample_rate_hz as f64 / 1000.0;
    let rate = if khz.fract() == 0.0 {
        format!("{}kHz", khz as u32)
    } else {
        format!("{:.1}kHz", khz)
    };
    format!("{}-bit ⁄ {}", bit_depth, rate)
}

/// What `{format}` and `{quality}` should say once `maybe_convert` has run.
///
/// Both are decided before the first byte lands, from the tier the service agreed to
/// serve. When conversion is on those bytes do not survive, so naming a folder after
/// them labels an MP3 file "AAC 256 kbps".
pub fn converted_quality_format(
    settings: &crate::defaults::Settings,
    served_quality: &str,
    served_format: &str,
) -> (String, String) {
    use crate::services::common::pipeline::converter::parse_codec;
    if !settings.conversion_check {
        return (served_quality.to_string(), served_format.to_string());
    }
    let Some(codec) = parse_codec(&settings.conversion_codec) else {
        return (served_quality.to_string(), served_format.to_string());
    };
    let format = codec.label().to_string();
    if !codec.is_lossless() {
        let kbps = crate::services::common::pipeline::converter::effective_lossy_kbps(
            codec,
            settings.conversion_lossy_bitrate,
        );
        return (format!("{kbps}kbps"), format);
    }
    match (
        settings.conversion_bit_depth,
        settings.conversion_sampling_rate,
    ) {
        (Some(depth), Some(rate)) => (depth_rate_label(depth, rate), format),
        _ => (served_quality.to_string(), format),
    }
}

pub fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' => '＜',
            '>' => '＞',
            ':' => '：',
            '"' => '＂',
            '/' => '⁄',
            '\\' => '＼',
            '|' => '｜',
            '?' => '？',
            '*' => '＊',
            '\x00'..='\x1f' => '_',
            other => other,
        })
        .collect()
}

/// The longest a single path component may get, whatever the user asked for.
/// Most filesystems cap a component at 255 bytes; staying under it keeps room for
/// the extension and the `.raw`/`.tmp` suffixes the download path appends.
const FOLDER_NAME_CEILING: usize = 200;

/// How a folder name is sanitised and clipped. Kept as a struct so the two settings
/// that govern it travel together rather than growing the argument list — and so
/// folders can no longer silently ignore them, which is what they used to do.
#[derive(Debug, Clone, Copy)]
pub struct FolderNaming {
    pub restrict: bool,
    /// `filepaths_truncate_to`; 0 means "as long as the filesystem allows".
    pub truncate: usize,
}

impl Default for FolderNaming {
    fn default() -> Self {
        Self {
            restrict: true,
            truncate: 0,
        }
    }
}

impl FolderNaming {
    pub fn from_settings(settings: &crate::defaults::Settings) -> Self {
        Self {
            restrict: settings.filepaths_restrict_characters,
            truncate: settings.filepaths_truncate_to as usize,
        }
    }

    fn limit(&self) -> usize {
        if self.truncate == 0 {
            FOLDER_NAME_CEILING
        } else {
            self.truncate.min(FOLDER_NAME_CEILING)
        }
    }
}

pub fn build_album_folder(
    folder_format: &str,
    albumartist: &str,
    album: &str,
    year: &str,
    genre: &str,
    label: &str,
    quality: &str,
    format: &str,
    naming: FolderNaming,
) -> String {
    let template = if folder_format.trim().is_empty() {
        "{albumartist} - {album} ({year})"
    } else {
        folder_format
    };

    let vars: std::collections::HashMap<&str, String> = [
        ("album", album),
        ("albumartist", albumartist),
        ("artist", albumartist),
        ("year", year),
        ("genre", genre),
        ("label", label),
        ("quality", quality),
        ("format", format),
    ]
    .into_iter()
    .map(|(k, v)| (k, v.to_string()))
    .collect();

    let rendered = build_file_name(template, &vars, naming.restrict, 0)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let folder = trim_orphan_separators(&rendered);
    let folder = trim_orphan_separators(truncate_str_bytes(&folder, naming.limit()));

    if folder.is_empty() {
        safe_name(album)
    } else {
        folder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_size_snaps_to_one_the_service_publishes() {
        let tidal = &[80u32, 160, 320, 640, 750, 1080, 1280];
        assert_eq!(nearest_cover_size(1280, tidal), 1280);
        assert_eq!(nearest_cover_size(1000, tidal), 1080);
        assert_eq!(nearest_cover_size(700, tidal), 750);
        assert_eq!(nearest_cover_size(1, tidal), 80);
        assert_eq!(nearest_cover_size(9999, tidal), 1280);
    }

    #[test]
    fn path_separators_are_replaced_even_when_restriction_is_off() {
        let vars: std::collections::HashMap<&str, String> =
            [("title", "Thomas Bangalter/Guy-Manuel: Live?".to_string())]
                .into_iter()
                .collect();
        assert_eq!(
            build_file_name("{title}", &vars, false, 0),
            "Thomas Bangalter⁄Guy-Manuel: Live?"
        );
        assert_eq!(
            build_file_name("{title}", &vars, true, 0),
            "Thomas Bangalter⁄Guy-Manuel： Live？"
        );
    }

    #[test]
    fn an_unknown_track_number_does_not_become_a_zero_prefix() {
        let vars: std::collections::HashMap<&str, String> = [
            ("tracknumber", String::new()),
            ("artist", "Daft Punk".to_string()),
            ("title", "Contact".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            build_file_name("{tracknumber:02}. {artist} - {title}", &vars, true, 0),
            "Daft Punk - Contact"
        );
    }

    #[test]
    fn a_real_track_number_is_still_zero_padded() {
        let vars: std::collections::HashMap<&str, String> = [
            ("tracknumber", "4".to_string()),
            ("artist", "Daft Punk".to_string()),
            ("title", "Doin' It Right".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            build_file_name("{tracknumber:02}. {artist} - {title}", &vars, true, 0),
            "04. Daft Punk - Doin' It Right"
        );
    }

    #[test]
    fn album_folder_drops_empty_optional_year() {
        assert_eq!(
            build_album_folder(
                "",
                "Thomas Bangalter/Guy-Manuel",
                "Random Access Memories",
                "",
                "",
                "",
                "",
                "",
                FolderNaming::default()
            ),
            "Thomas Bangalter⁄Guy-Manuel - Random Access Memories"
        );
        assert_eq!(
            build_album_folder(
                "",
                "Thomas Bangalter/Guy-Manuel",
                "Random Access Memories",
                "2013",
                "",
                "",
                "",
                "",
                FolderNaming::default()
            ),
            "Thomas Bangalter⁄Guy-Manuel - Random Access Memories (2013)"
        );
    }

    #[test]
    fn album_folder_sanitises_and_falls_back() {
        assert_eq!(
            build_album_folder(
                "{albumartist}/{album}",
                "Thomas Bangalter/Guy-Manuel",
                "Homework",
                "",
                "",
                "",
                "",
                "",
                FolderNaming::default()
            ),
            "Thomas Bangalter⁄Guy-Manuel⁄Homework"
        );
        assert_eq!(
            build_album_folder(
                "({year})",
                "",
                "Fallback Album",
                "",
                "",
                "",
                "",
                "",
                FolderNaming::default()
            ),
            "Fallback Album"
        );
    }

    #[test]
    fn album_folder_keeps_populated_optional_segments() {
        assert_eq!(
            build_album_folder(
                "{albumartist} - {album} [{quality}]",
                "Artist",
                "Album",
                "2020",
                "",
                "",
                "24-bit ⁄ 96kHz",
                "FLAC",
                FolderNaming::default()
            ),
            "Artist - Album [24-bit ⁄ 96kHz]"
        );
    }

    /// The folder used to force character restriction and a hardcoded 150-char cap,
    /// so both naming settings applied to filenames only.
    #[test]
    fn a_folder_obeys_the_naming_settings_it_used_to_ignore() {
        let loose = FolderNaming {
            restrict: false,
            truncate: 0,
        };
        assert_eq!(
            build_album_folder(
                "{album}",
                "",
                "Live: Thomas Bangalter/Guy-Manuel",
                "",
                "",
                "",
                "",
                "",
                loose
            ),
            "Live: Thomas Bangalter⁄Guy-Manuel"
        );
        assert_eq!(
            build_album_folder(
                "{album}",
                "",
                "Live: Thomas Bangalter/Guy-Manuel",
                "",
                "",
                "",
                "",
                "",
                FolderNaming::default()
            ),
            "Live： Thomas Bangalter⁄Guy-Manuel"
        );

        let short = FolderNaming {
            restrict: true,
            truncate: 10,
        };
        assert_eq!(
            build_album_folder(
                "{album}",
                "",
                "An Extremely Long Album Title",
                "",
                "",
                "",
                "",
                "",
                short
            ),
            "An Extreme"
        );
    }

    /// A folder is still held under a filesystem-safe ceiling when the user asks for
    /// "unlimited", because a 255-byte component is a hard limit, not a preference.
    #[test]
    fn unlimited_still_stops_short_of_the_filesystem_limit() {
        let long = "x".repeat(400);
        let out = build_album_folder(
            "{album}",
            "",
            &long,
            "",
            "",
            "",
            "",
            "",
            FolderNaming::default(),
        );
        assert_eq!(out.len(), FOLDER_NAME_CEILING);
    }

    /// `{format}` and `{quality}` are settled before the first byte lands, from the tier
    /// the service agreed to serve — but `maybe_convert` then replaces those bytes. A
    /// folder read "[AAC 256 kbps]" while holding 74 kbps MP3s.
    #[test]
    fn naming_follows_the_re_encode_when_conversion_is_on() {
        let mut s = crate::defaults::Settings {
            conversion_check: false,
            ..Default::default()
        };
        assert_eq!(
            converted_quality_format(&s, "256 kbps", "AAC"),
            ("256 kbps".to_string(), "AAC".to_string())
        );

        s.conversion_check = true;
        s.conversion_codec = "MP3".into();
        s.conversion_lossy_bitrate = 320;
        assert_eq!(
            converted_quality_format(&s, "256 kbps", "AAC"),
            ("320kbps".to_string(), "MP3".to_string())
        );

        s.conversion_codec = "FLAC".into();
        assert_eq!(
            converted_quality_format(&s, "24-bit ⁄ 96kHz", "FLAC"),
            ("24-bit ⁄ 96kHz".to_string(), "FLAC".to_string())
        );
        assert_eq!(
            converted_quality_format(&s, "256 kbps", "AAC"),
            ("256 kbps".to_string(), "FLAC".to_string())
        );

        s.conversion_bit_depth = Some(16);
        s.conversion_sampling_rate = Some(44_100);
        assert_eq!(
            converted_quality_format(&s, "24-bit ⁄ 96kHz", "FLAC"),
            ("16-bit ⁄ 44.1kHz".to_string(), "FLAC".to_string())
        );

        s.conversion_codec = "WMA".into();
        assert_eq!(
            converted_quality_format(&s, "256 kbps", "AAC"),
            ("256 kbps".to_string(), "AAC".to_string())
        );
    }
}

/// The `DownloadProvider` impl for a backend that just runs the shared pipeline.
/// Byte-identical in every streamrip service, so it lives here once.
#[macro_export]
macro_rules! pipeline_provider {
    ($ty:ty) => {
        #[async_trait::async_trait]
        impl $crate::services::common::DownloadProvider for $ty {
            async fn start(
                &self,
                req: serde_json::Value,
                ctx: &$crate::services::common::DownloadContext,
                download_id: u64,
            ) -> $crate::ipc_contract::StartDownloadResponse {
                $crate::services::common::pipeline::runner::run_pipeline_download(
                    std::sync::Arc::new(self.clone()),
                    req,
                    ctx,
                    download_id,
                )
                .await
            }
        }
    };
}
