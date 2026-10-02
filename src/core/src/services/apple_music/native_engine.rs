use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use serde_json::Value;

use crate::defaults::Settings;
use crate::downloads::dedup::{quality_rank, DedupLedger};
use crate::downloads::native_common::{
    cover_for_track, dest_dir_for, disc_dir_for, native_track_name, remux_or_keep,
    run_progress_tick_loop, settle_cover_file, NativeTrack, ReleaseFolder, TickSlice,
};
use crate::downloads::BatchProgress;
use crate::errors::{MhError, MhResult};
use crate::services::apple_music::api::AppleMusicApiClient;
use crate::services::apple_music::meta::parse_apple_music_url;
use crate::services::apple_music::playback::AppleMusicService;
use crate::services::common::download::BatchOutcome;
use crate::services::common::library::string_at;
use crate::services::common::lyrics::{
    lyrics_wanted, prefers_ttml, resolve_track_lyrics, write_chosen_sidecar, write_sidecars,
    FoundLyrics, LyricsSidecars,
};
use crate::services::common::pipeline::converter::{AudioCodec, ConversionSource};
use crate::services::common::pipeline::depth_rate_label;
use crate::services::common::pipeline::downloader::{
    excluded_tags, existing_final_file_for, explicit_suffix, maybe_convert, short_rip_reason,
    CommonTrackFields,
};
use crate::services::common::pipeline::tagger::{one, tag_file, TrackMetadata};
use crate::services::common::playlist_file::{write_playlist_file, PlaylistEntry};

struct AppleQuality {
    id: &'static str,
    /// Names the tier for a human — logs and the settings dropdown — so it spells
    /// out the codec.
    label: &'static str,
    /// The `{quality}` template value, which must NOT name the codec: `{format}`
    /// already does, and a template of `{format} {quality}` rendered "AAC AAC 256 kbps".
    quality_label: &'static str,
    /// Full-match regex against an `#EXT-X-STREAM-INF` `AUDIO="…"` group id.
    /// Apple names a whole family per codec (`audio-stereo-256`,
    /// `audio-alac-stereo-192`…), so a pattern picks the family and the highest
    /// bandwidth member of it wins.
    group_pattern: &'static str,
    codec_label: &'static str,
    /// Absent from the master the web token can fetch; only the wrapper's
    /// `_default` master carries it.
    needs_wrapper: bool,
}

/// The rendition families Apple publishes, named after the codec ids gamdl uses so
/// the two tools agree. The bitrate-pinned entries below them are kept so existing
/// saved settings and dedup ledger rows keep resolving.
const APPLE_QUALITIES: &[AppleQuality] = &[
    AppleQuality {
        id: "alac",
        label: "ALAC (Lossless)",
        quality_label: "Lossless",
        group_pattern: r"audio-alac-.*",
        codec_label: "ALAC",
        needs_wrapper: true,
    },
    AppleQuality {
        id: "atmos",
        label: "Dolby Atmos (E-AC-3)",
        quality_label: "Atmos",
        group_pattern: r"audio-atmos-.*",
        codec_label: "E-AC-3",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "ac3",
        label: "Dolby Digital (AC-3)",
        quality_label: "Dolby Digital",
        group_pattern: r"audio-ac3-.*",
        codec_label: "AC-3",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac",
        label: "AAC (best available)",
        quality_label: "best available",
        group_pattern: r"audio-stereo-\d+",
        codec_label: "AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-he",
        label: "HE-AAC (best available)",
        quality_label: "best available",
        group_pattern: r"audio-HE-stereo-\d+",
        codec_label: "HE-AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-binaural",
        label: "AAC Binaural",
        quality_label: "best available (Binaural)",
        group_pattern: r"audio-stereo-\d+-binaural",
        codec_label: "AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-downmix",
        label: "AAC Downmix",
        quality_label: "best available (Downmix)",
        group_pattern: r"audio-stereo-\d+-downmix",
        codec_label: "AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-he-binaural",
        label: "HE-AAC Binaural",
        quality_label: "best available (Binaural)",
        group_pattern: r"audio-HE-stereo-\d+-binaural",
        codec_label: "HE-AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-he-downmix",
        label: "HE-AAC Downmix",
        quality_label: "best available (Downmix)",
        group_pattern: r"audio-HE-stereo-\d+-downmix",
        codec_label: "HE-AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-256",
        label: "AAC 256 kbps (Stereo)",
        quality_label: "256 kbps (Stereo)",
        group_pattern: r"audio-stereo-256",
        codec_label: "AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-128",
        label: "AAC 128 kbps (Stereo)",
        quality_label: "128 kbps (Stereo)",
        group_pattern: r"audio-stereo-128",
        codec_label: "AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-he-64",
        label: "HE-AAC 64 kbps (Stereo)",
        quality_label: "64 kbps (Stereo)",
        group_pattern: r"audio-HE-stereo-64",
        codec_label: "HE-AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-256-binaural",
        label: "AAC 256 kbps (Binaural)",
        quality_label: "256 kbps (Binaural)",
        group_pattern: r"audio-stereo-256-binaural",
        codec_label: "AAC",
        needs_wrapper: false,
    },
    AppleQuality {
        id: "aac-256-downmix",
        label: "AAC 256 kbps (Downmix)",
        quality_label: "256 kbps (Downmix)",
        group_pattern: r"audio-stereo-256-downmix",
        codec_label: "AAC",
        needs_wrapper: false,
    },
];

impl AppleQuality {
    /// Whether an audio group id belongs to this rendition family.
    fn group_pattern_matches(&self, group: &str) -> bool {
        regex::Regex::new(&format!("^(?:{})$", self.group_pattern))
            .map(|re| re.is_match(group))
            .unwrap_or(false)
    }
}

fn quality_for(id: &str) -> Option<&'static AppleQuality> {
    APPLE_QUALITIES.iter().find(|q| q.id == id)
}

fn khz(rate: u32) -> String {
    if rate.is_multiple_of(1000) {
        format!("{}", rate / 1000)
    } else {
        format!("{:.1}", rate as f64 / 1000.0)
    }
}

/// What the rendition Apple actually served should be called.
///
/// The request is only a preference: when a rendition is missing from the
/// account's variant set Apple quietly serves a lesser one, so naming a file
/// after the requested quality can label an AAC 256 rip "Dolby Atmos". The audio
/// group id carries the nominal quality — `audio-stereo-256` is 256 kbps AAC,
/// `audio-atmos-2768` is the 768 kbps Atmos ladder, `audio-alac-stereo-44100-24`
/// is 44.1 kHz 24-bit ALAC — so that is what gets used.
fn rendition_label(group: &str) -> (String, &'static str) {
    let (base, suffix) = match group.strip_suffix("-binaural") {
        Some(b) => (b, " (Binaural)"),
        None => match group.strip_suffix("-downmix") {
            Some(b) => (b, " (Downmix)"),
            None => (group, ""),
        },
    };

    if let Some(rest) = base.strip_prefix("audio-alac-stereo-") {
        let mut parts = rest.split('-');
        let rate = parts.next().and_then(|s| s.parse::<u32>().ok());
        let depth = parts.next().and_then(|s| s.parse::<u32>().ok());
        return match (rate, depth) {
            (Some(r), Some(d)) => (format!("{}{suffix}", depth_rate_label(d, r)), "ALAC"),
            (Some(r), None) => (format!("{} kHz{suffix}", khz(r)), "ALAC"),
            _ => (format!("Lossless{suffix}"), "ALAC"),
        };
    }
    if base.starts_with("audio-alac") {
        return (format!("Lossless{suffix}"), "ALAC");
    }
    if let Some(rest) = base.strip_prefix("audio-atmos-") {
        return match rest.parse::<u32>() {
            Ok(n) => {
                let kbps = if n > 2000 { n - 2000 } else { n };
                (format!("{kbps} kbps{suffix}"), "E-AC-3")
            }
            Err(_) => (format!("Atmos{suffix}"), "E-AC-3"),
        };
    }
    if let Some(rest) = base.strip_prefix("audio-ac3-") {
        return match rest.parse::<u32>() {
            Ok(n) => (format!("{n} kbps{suffix}"), "AC-3"),
            Err(_) => (format!("Dolby Digital{suffix}"), "AC-3"),
        };
    }
    if let Some(rest) = base.strip_prefix("audio-HE-stereo-") {
        if let Ok(n) = rest.parse::<u32>() {
            return (format!("{n} kbps{suffix}"), "HE-AAC");
        }
    }
    if let Some(rest) = base.strip_prefix("audio-stereo-") {
        if let Ok(n) = rest.parse::<u32>() {
            return (format!("{n} kbps{suffix}"), "AAC");
        }
    }
    (group.to_string(), "AAC")
}

/// Apple ships every rendition in an `.m4a`, so the container says nothing about
/// what is inside. The spatial ladders are flagged as surround because no
/// re-encode can carry Atmos through.
fn conversion_source(real_codec: &str) -> ConversionSource {
    match real_codec {
        "ALAC" => ConversionSource::with_codec(AudioCodec::Alac),
        "AAC" | "HE-AAC" => ConversionSource::with_codec(AudioCodec::Aac),
        _ => ConversionSource {
            codec: None,
            surround: true,
        },
    }
}

pub fn is_supported_quality(q: &str) -> bool {
    quality_for(q).is_some()
}

pub fn normalize_apple_url(url: &str) -> String {
    if !url.contains("music.apple.com/") {
        return url.to_string();
    }
    for prefix in [
        "https://music.apple.com/",
        "https://classical.music.apple.com/",
    ] {
        for needle in ["library/song/", "library/songs/"] {
            let full = format!("{prefix}{needle}");
            if let Some(rest) = url.strip_prefix(&full) {
                let id_part = rest.split(['?', '#', '/']).next().unwrap_or("");
                if !id_part.is_empty() && id_part.chars().all(|c| c.is_ascii_digit()) {
                    let tail_start = full.len() + id_part.len();
                    let tail = &url[tail_start..];
                    return format!("{prefix}us/song/x/{id_part}{tail}");
                }
                return url.to_string();
            }
        }
    }
    let kinds = [
        "song",
        "album",
        "playlist",
        "artist",
        "music-video",
        "post",
        "audiobook",
    ];
    for prefix in [
        "https://music.apple.com/",
        "https://classical.music.apple.com/",
    ] {
        let Some(after) = url.strip_prefix(prefix) else {
            continue;
        };
        let mut parts = after.splitn(3, '/');
        let first = parts.next().unwrap_or("");
        if !kinds.contains(&first) {
            continue;
        }
        let second = parts.next().unwrap_or("");
        let rest = parts.next();
        let (id, query) = match second.split_once('?') {
            Some((i, q)) => (i, format!("?{q}")),
            None => (second, String::new()),
        };
        return match rest {
            Some(_) => format!("{prefix}us/{after}"),
            None => format!("{prefix}us/{first}/x/{id}{query}"),
        };
    }
    url.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleLinkKind {
    Song,
    Album,
    Playlist,
    MusicVideo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleUrlSupport {
    Supported(AppleLinkKind),
    Unsupported(&'static str),
    NotApple,
}

pub fn classify_apple_url(url: &str) -> AppleUrlSupport {
    use AppleUrlSupport::*;
    let lower = url.to_lowercase();
    if !lower.contains("music.apple.com") {
        return NotApple;
    }
    if url.contains("/post/") {
        return Unsupported("post");
    }
    if url.contains("/artist/") && !url.contains("?i=") {
        return Unsupported("artist top-songs");
    }
    if url.contains("/library/songs/i.") || url.contains("/library/song/i.") {
        return Unsupported("library item (right-click → Share → Copy Link for a catalog URL)");
    }
    let normalized = normalize_apple_url(url);
    let Some(info) = parse_apple_music_url(&normalized) else {
        return Unsupported("unrecognised URL");
    };
    match info.content_type.as_str() {
        "song" => Supported(AppleLinkKind::Song),
        "album" => Supported(AppleLinkKind::Album),
        "playlist" => Supported(AppleLinkKind::Playlist),
        "music-video" => Supported(AppleLinkKind::MusicVideo),
        _ => Unsupported("unsupported link kind"),
    }
}

#[derive(Debug, Clone, Default)]
struct AppleTrackMeta {
    id: String,
    track_url: String,
    title: String,
    artist: String,
    album: String,
    album_artist: String,
    track_number: Option<u32>,
    total_tracks: Option<u32>,
    disc_number: Option<u32>,
    year: Option<String>,
    date: Option<String>,
    disc_total: Option<u32>,
    genre: Option<String>,
    label: Option<String>,
    composer: Option<String>,
    explicit: bool,
    cover_url: Option<String>,
    copyright: Option<String>,
    upc: Option<String>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    playable: bool,
    meta_source: &'static str,
}

/// Apple serves artwork from a template (`{w}x{h}{c}.{f}`) or, on the iTunes lookup
/// path, at a fixed `100x100`. Both are rewritten to the size and format the user
/// configured instead of a hardcoded 1200px JPEG.
fn cover_url_at(template: &str, size: u32, format: &str) -> String {
    let size = size.clamp(64, 5000);
    let format = match format.trim().to_ascii_lowercase().as_str() {
        "png" => "png",
        _ => "jpg",
    };
    if template.contains("{w}") || template.contains("{h}") {
        return template
            .replace("{w}", &size.to_string())
            .replace("{h}", &size.to_string())
            .replace("{f}", format)
            .replace("{c}", "");
    }
    template.replace("100x100", &format!("{size}x{size}"))
}

/// AMP returns `2001-03-07`, the iTunes lookup returns `2001-03-07T08:00:00Z`.
/// Both become a plain calendar date; the tagger derives the year from it.
fn release_year(raw: &str) -> Option<String> {
    raw.trim()
        .get(..4)
        .filter(|y| y.len() == 4)
        .map(str::to_string)
}

fn release_date(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let day = raw
        .get(..10)
        .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-');
    match day {
        Some(d) => Some(d.to_string()),
        None => raw.get(..4).filter(|y| y.len() == 4).map(str::to_string),
    }
}

fn build_track_url(info_storefront: &str, song_id: &str) -> String {
    format!(
        "https://music.apple.com/{}/song/x/{}",
        info_storefront, song_id
    )
}

fn meta_from_itunes_song(storefront: &str, song: &Value) -> Option<AppleTrackMeta> {
    let id = song["trackId"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| song["trackId"].as_str().map(String::from))?;
    Some(AppleTrackMeta {
        id: id.clone(),
        track_url: build_track_url(storefront, &id),
        title: song["trackName"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string(),
        artist: string_at(song, &["/artistName"]),
        album: string_at(song, &["/collectionName"]),
        album_artist: song["collectionArtistName"]
            .as_str()
            .or_else(|| song["artistName"].as_str())
            .unwrap_or("")
            .to_string(),
        track_number: song["trackNumber"].as_u64().map(|n| n as u32),
        total_tracks: song["trackCount"].as_u64().map(|n| n as u32),
        disc_number: song["discNumber"].as_u64().map(|n| n as u32),
        year: song["releaseDate"].as_str().and_then(release_year),
        date: song["releaseDate"].as_str().and_then(release_date),
        disc_total: song["discCount"].as_u64().map(|n| n as u32),
        genre: song["primaryGenreName"].as_str().map(String::from),
        label: None,
        composer: song["composerName"].as_str().map(String::from),
        explicit: song["trackExplicitness"].as_str() == Some("explicit"),
        cover_url: song["artworkUrl100"].as_str().map(str::to_string),
        copyright: song["copyright"].as_str().map(String::from),
        upc: song["collectionUpc"].as_str().map(String::from),
        isrc: song["isrc"].as_str().map(String::from),
        duration_ms: song["trackTimeMillis"].as_u64(),
        playable: song["isStreamable"].as_bool().unwrap_or(true),
        meta_source: "itunes",
    })
}

/// `album_attrs` is the album record's own `attributes`, supplied when the song came
/// from an album listing. Those listings carry no `relationships.albums`, so the
/// release-level fields have to come from the album call that produced them.
fn meta_from_apple_song(
    storefront: &str,
    song: &Value,
    album_attrs: Option<&Value>,
) -> Option<AppleTrackMeta> {
    let id = song.get("id").and_then(|v| v.as_str())?.to_string();
    let attrs = song.get("attributes")?;
    let title = attrs
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown Title")
        .to_string();
    let artist = attrs
        .get("artistName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let album = attrs
        .get("albumName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let track_number = attrs
        .get("trackNumber")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let disc_number = attrs
        .get("discNumber")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let raw_release = attrs.get("releaseDate").and_then(|v| v.as_str());
    let year = raw_release.and_then(release_year);
    let date = raw_release.and_then(release_date);
    let genre = attrs
        .get("genreNames")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|g| g.as_str())
        .map(String::from);
    let cover_url = attrs
        .get("artwork")
        .and_then(|a| a.get("url"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let copyright = attrs
        .get("copyright")
        .or_else(|| album_attrs.and_then(|a| a.get("copyright")))
        .and_then(|v| v.as_str())
        .map(String::from);
    let isrc = attrs.get("isrc").and_then(|v| v.as_str()).map(String::from);
    let composer = attrs
        .get("composerName")
        .and_then(|v| v.as_str())
        .map(String::from);
    let explicit = attrs.get("contentRating").and_then(|v| v.as_str()) == Some("explicit");
    let duration_ms = attrs.get("durationInMillis").and_then(|v| v.as_u64());
    let playable = attrs
        .pointer("/playParams/id")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    let from_album = |key: &str| -> Option<Value> {
        song.pointer(&format!("/relationships/albums/data/0/attributes/{key}"))
            .or_else(|| album_attrs.and_then(|a| a.get(key)))
            .cloned()
    };
    let label = from_album("recordLabel")
        .as_ref()
        .and_then(|v| v.as_str())
        .map(String::from);
    let total_tracks = from_album("trackCount")
        .as_ref()
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let disc_total = from_album("discCount")
        .as_ref()
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let upc = from_album("upc")
        .as_ref()
        .and_then(|v| v.as_str())
        .map(String::from);
    let album_artist = attrs
        .get("albumArtistName")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            from_album("artistName")
                .as_ref()
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| artist.clone());
    Some(AppleTrackMeta {
        id: id.clone(),
        track_url: build_track_url(storefront, &id),
        title,
        artist,
        album,
        album_artist,
        track_number,
        total_tracks,
        disc_number,
        year,
        date,
        disc_total,
        genre,
        label,
        composer,
        explicit,
        cover_url,
        copyright,
        upc,
        isrc,
        duration_ms,
        playable,
        meta_source: "amp-api",
    })
}

async fn resolve_tracks(
    service: &Arc<AppleMusicService>,
    api: &AppleMusicApiClient,
    url: &str,
    on_log: &(dyn Fn(String) + Send + Sync),
) -> MhResult<(Vec<AppleTrackMeta>, Option<String>)> {
    let normalized = normalize_apple_url(url);
    let info = parse_apple_music_url(&normalized)
        .ok_or_else(|| MhError::Other(format!("Could not parse Apple Music URL: {url}")))?;
    let storefront = info.storefront.clone();

    match info.content_type.as_str() {
        "song" => {
            let mut amp_err: Option<String> = None;
            match service
                .get_song_catalog_metadata_in(&info.content_id, Some(&storefront))
                .await
            {
                Ok(song) => {
                    if let Some(meta) = meta_from_apple_song(&storefront, &song, None) {
                        return Ok((vec![meta], None));
                    }
                    on_log(format!(
                        "Native Apple Music: fetched AMP catalog record for song {} but could not \
                         parse it; falling back to iTunes lookup",
                        info.content_id
                    ));
                }
                Err(e) => {
                    let s = e.to_string();
                    on_log(format!(
                        "Native Apple Music: AMP catalog lookup failed for song {}: {} — \
                         falling back to iTunes lookup",
                        info.content_id, s
                    ));
                    amp_err = Some(s);
                }
            }
            let v = api.lookup_by_id_in(&info.content_id, &storefront).await?;
            if v.is_null() {
                on_log(format!(
                    "Native Apple Music: iTunes lookup returned no results for song {}",
                    info.content_id
                ));
                return Err(MhError::Other(match amp_err {
                    Some(e) => format!(
                        "Apple Music returned no metadata for song {}. Catalog error: {} \
                         (try the Gamdl backend if this persists)",
                        info.content_id, e
                    ),
                    None => format!(
                        "Apple Music returned no metadata for song {} (try the Gamdl backend if this persists)",
                        info.content_id
                    ),
                }));
            }
            meta_from_itunes_song(&storefront, &v)
                .map(|m| (vec![m], None))
                .ok_or_else(|| MhError::Other("Apple Music song record missing id".into()))
        }
        "album" => {
            match service
                .album_catalog_tracks(&info.content_id, Some(&storefront))
                .await
            {
                Ok((album_attrs, tracks_val)) => {
                    let mut out: Vec<AppleTrackMeta> = tracks_val
                        .iter()
                        .filter_map(|t| meta_from_apple_song(&storefront, t, Some(&album_attrs)))
                        .collect();
                    if let Some(discs) = out.iter().filter_map(|t| t.disc_number).max() {
                        for t in out.iter_mut() {
                            t.disc_total.get_or_insert(discs);
                        }
                    }
                    if out.len() != tracks_val.len() {
                        on_log(format!(
                            "Native Apple Music: {} of {} album track record(s) could not be \
                             parsed and were left out",
                            tracks_val.len() - out.len(),
                            tracks_val.len()
                        ));
                    }
                    let disc_total = out.iter().filter_map(|m| m.disc_number).max();
                    for m in out.iter_mut() {
                        m.disc_total = m.disc_total.or(disc_total);
                    }
                    let album_name = album_attrs
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    Ok((out, album_name))
                }
                Err(e) => {
                    on_log(format!(
                        "Native Apple Music: catalog album lookup failed for {}: {e} — falling \
                         back to the iTunes lookup",
                        info.content_id
                    ));
                    let v = api.get_album_tracks(&info.content_id, &storefront).await?;
                    let tracks_val = v["tracks"].as_array().cloned().unwrap_or_default();
                    let out: Vec<AppleTrackMeta> = tracks_val
                        .iter()
                        .filter_map(|t| meta_from_itunes_song(&storefront, t))
                        .collect();
                    let album_name = v["album"]["title"]
                        .as_str()
                        .or_else(|| v["album"]["name"].as_str())
                        .map(str::to_string);
                    Ok((out, album_name))
                }
            }
        }
        "playlist" => {
            let (playlist_name, ids) = service
                .playlist_track_ids(&info.content_id, Some(&storefront))
                .await?;
            let mut out = Vec::with_capacity(ids.len());
            for tid in &ids {
                match service.get_song_catalog_metadata(tid).await {
                    Ok(song) => {
                        if let Some(m) = meta_from_apple_song(&storefront, &song, None) {
                            out.push(m);
                        }
                    }
                    Err(e) => on_log(format!(
                        "Native Apple Music: skipping playlist track {tid}: {e}"
                    )),
                }
            }
            Ok((out, Some(playlist_name)))
        }
        other => Err(MhError::Unsupported(format!(
            "Native Apple Music backend can't download {other} links — switch to Gamdl in Settings."
        ))),
    }
}

fn non_empty(s: &str) -> Option<String> {
    (!s.trim().is_empty()).then(|| s.to_string())
}

impl AppleTrackMeta {
    fn name_fields(&self, quality_label: &str, format_label: &str) -> CommonTrackFields {
        CommonTrackFields {
            title: self.title.clone(),
            artist: self.artist.clone(),
            albumartist: self.album_artist.clone(),
            album: self.album.clone(),
            track_num: self.track_number.unwrap_or(0),
            disc_num: self.disc_number.unwrap_or(1),
            tracktotal: self.total_tracks.map(|n| n.to_string()).unwrap_or_default(),
            disctotal: self.disc_total.map(|n| n.to_string()).unwrap_or_default(),
            year: self.year.clone().unwrap_or_default(),
            genre: self.genre.clone().unwrap_or_default(),
            explicit: explicit_suffix(self.explicit),
            isrc: self.isrc.clone().unwrap_or_default(),
            label: self.label.clone().unwrap_or_default(),
            date: self
                .date
                .clone()
                .or_else(|| self.year.clone())
                .unwrap_or_default(),
            quality_label: quality_label.to_string(),
            format_label: format_label.to_string(),
            composer: self.composer.clone().unwrap_or_default(),
        }
    }
}

impl NativeTrack for AppleTrackMeta {
    fn track_metadata(&self) -> TrackMetadata {
        TrackMetadata {
            title: non_empty(&self.title),
            artist: one(non_empty(&self.artist)),
            album: non_empty(&self.album),
            album_artist: one(non_empty(&self.album_artist)),
            year: self.date.clone().or_else(|| self.year.clone()),
            genre: one(self.genre.clone()),
            track_number: self.track_number,
            disc_number: self.disc_number,
            total_tracks: self.total_tracks,
            total_discs: self.disc_total,
            label: self.label.clone(),
            composer: one(self.composer.clone()),
            copyright: self.copyright.clone(),
            upc: self.upc.clone(),
            isrc: self.isrc.clone(),
            ..Default::default()
        }
    }
}

/// Downloads one Apple Music music video, honouring the Music Video settings the
/// Gamdl backend already exposes (codec priority, max resolution, remux format).
async fn download_music_video(
    settings: &Settings,
    service: &AppleMusicService,
    api: &AppleMusicApiClient,
    url: &str,
    dest_dir: &Path,
    on_progress: &impl Fn(BatchProgress),
    on_log: &(dyn Fn(String) + Send + Sync),
    cancel: &Arc<AtomicBool>,
) -> MhResult<BatchOutcome> {
    let normalized = normalize_apple_url(url);
    let info = parse_apple_music_url(&normalized)
        .ok_or_else(|| MhError::Other(format!("Could not parse Apple Music URL: {url}")))?;

    let meta = match service.get_song_catalog_metadata(&info.content_id).await {
        Ok(v) => meta_from_apple_song(&info.storefront, &v, None),
        Err(e) => {
            on_log(format!(
                "Native Apple Music: catalog lookup failed for video {}: {e} — using iTunes data",
                info.content_id
            ));
            api.lookup_by_id_in(&info.content_id, &info.storefront)
                .await
                .ok()
                .and_then(|v| meta_from_itunes_song(&info.storefront, &v))
        }
    }
    .unwrap_or_else(|| AppleTrackMeta {
        id: info.content_id.clone(),
        title: format!("video_{}", info.content_id),
        ..Default::default()
    });

    let max_height =
        crate::services::common::video::parse_height(&settings.apple_mv_resolution).unwrap_or(1080);
    let codec_priority: Vec<String> = settings
        .apple_mv_codec_priority
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    let ext = match settings
        .apple_mv_remux_format
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" => "mp4",
        _ => "m4v",
    };

    let label = format!("{} - {}", meta.artist, meta.title);
    on_log(format!(
        "Native Apple Music: music video {label} at up to {max_height}p ({})",
        settings.apple_mv_codec_priority
    ));

    let mut outcome = BatchOutcome::new(1);
    let bp = crate::downloads::ByteProgress::default();
    let fut =
        service.get_music_video_for_download(&normalized, max_height, &codec_priority, Some(&bp));
    tokio::pin!(fut);
    let stream = run_progress_tick_loop(
        fut,
        &bp,
        TickSlice {
            start: 0.0,
            ceiling: 95.0,
            label: &label,
            completed: 0,
            total: 1,
        },
        cancel,
        on_progress,
    )
    .await?;
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            on_log(format!("  ✗ {label}: {e}"));
            outcome.record_failure(&meta.id, &label, e);
            return outcome.require_any("Apple Music");
        }
    };

    tokio::fs::create_dir_all(dest_dir).await?;
    let basename = native_track_name(
        settings,
        &meta.name_fields(&format!("{max_height}p"), "MP4"),
    );
    let dest_path = dest_dir.join(format!("{basename}.{ext}"));

    let tmp_video = dest_dir.join(format!("{basename}.video.tmp.mp4"));
    let tmp_audio = dest_dir.join(format!("{basename}.audio.tmp.m4a"));
    tokio::fs::write(&tmp_video, &stream.video).await?;
    tokio::fs::write(&tmp_audio, &stream.audio).await?;
    let mux = mux_video_and_audio(&tmp_video, &tmp_audio, &dest_path, ext).await;
    let _ = tokio::fs::remove_file(&tmp_video).await;
    let _ = tokio::fs::remove_file(&tmp_audio).await;
    if let Err(e) = mux {
        on_log(format!("  ✗ {label}: {e}"));
        outcome.record_failure(&meta.id, &label, e);
        return outcome.require_any("Apple Music");
    }

    match tag_file(
        &dest_path,
        &meta.track_metadata(),
        None,
        &excluded_tags(settings),
    )
    .await
    {
        Ok(()) => on_log(format!("  ✓ tagged → {}", dest_path.display())),
        Err(e) => on_log(format!("  ⚠ tagging failed: {e}")),
    }

    outcome.record_success();
    on_progress(BatchProgress::at(100.0, label, 1, 1));
    Ok(outcome.with_dest(dest_dir))
}

/// ffmpeg only has to interleave here: both tracks are already decrypted mp4.
async fn mux_video_and_audio(video: &Path, audio: &Path, dest: &Path, ext: &str) -> MhResult<()> {
    let ffmpeg = crate::venv_manager::resolve_ffmpeg();
    let mut cmd = tokio::process::Command::new(&ffmpeg);
    cmd.arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(audio)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"])
        .args(["-f", if ext == "mp4" { "mp4" } else { "mov" }])
        .arg(dest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::subprocess::apply_no_window(&mut cmd);
    let out = cmd
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("ffmpeg spawn failed: {e}")))?;
    if !out.status.success() {
        return Err(MhError::Subprocess(format!(
            "muxing the music video failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Fetches Apple's timed lyrics for one track. The native path uses the shared
/// native lyrics settings; the `apple_*` keys stay reserved for the Gamdl backend,
/// which writes them into its own config file.
async fn apple_lyrics_for(
    service: &AppleMusicService,
    settings: &Settings,
    track_url: &str,
    title: &str,
    artist: &str,
    on_log: &(dyn Fn(String) + Send + Sync),
) -> Option<crate::services::apple_music::playback::DownloadLyrics> {
    if !lyrics_wanted(settings) {
        return None;
    }
    let native = match service.fetch_download_lyrics(track_url).await {
        Ok(Some(lyrics)) => {
            if lyrics.lrc.is_some() || lyrics.ttml.is_some() {
                return Some(lyrics);
            }
            FoundLyrics {
                plain: lyrics.plain.clone(),
                ..Default::default()
            }
        }
        Ok(None) => {
            on_log("  · Apple Music published no lyrics for this track".to_string());
            FoundLyrics::default()
        }
        Err(e) => {
            on_log(format!("  ⚠ lyrics lookup failed: {e}"));
            FoundLyrics::default()
        }
    };

    let found = resolve_track_lyrics(native, title, artist, None, settings, on_log).await;
    if found.is_empty() {
        return None;
    }
    let sidecars = found.sidecars(Some(title), Some(artist));
    Some(crate::services::apple_music::playback::DownloadLyrics {
        ttml: sidecars.ttml,
        lrc: sidecars.lrc,
        plain: found.plain,
    })
}

async fn write_apple_lyric_sidecars(
    lyrics: Option<&crate::services::apple_music::playback::DownloadLyrics>,
    settings: &Settings,
    final_path: &Path,
    on_log: &(dyn Fn(String) + Send + Sync),
) {
    let Some(lyrics) = lyrics else { return };
    let sidecars = apple_sidecars(lyrics);
    write_sidecars(final_path, &sidecars, settings, on_log).await;
    if settings.save_lrc_files && lyrics.lrc.is_none() && lyrics.ttml.is_none() {
        on_log("  · Apple Music published lyrics without timings; no sidecar written".to_string());
    }
}

/// Apple's own TTML is word-timed when it nests a `<span>` per word inside each
/// line; a line-only document holds nothing the `.lrc` does not.
fn apple_sidecars(
    lyrics: &crate::services::apple_music::playback::DownloadLyrics,
) -> LyricsSidecars {
    LyricsSidecars {
        word_timed: lyrics
            .ttml
            .as_deref()
            .map(|t| t.contains("<span"))
            .unwrap_or(false),
        lrc: lyrics.lrc.clone(),
        ttml: lyrics.ttml.clone(),
    }
}

pub async fn download_with_native_apple(
    settings: &Settings,
    service: Arc<AppleMusicService>,
    api: AppleMusicApiClient,
    url: &str,
    quality: Option<&str>,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
    dedup: Option<&DedupLedger>,
) -> MhResult<BatchOutcome> {
    if let AppleUrlSupport::Unsupported(kind) = classify_apple_url(url) {
        return Err(MhError::Unsupported(format!(
            "Native Apple Music downloader can't handle {kind} links. \
             Open Settings → Apple Music → Downloader and switch to Gamdl to download this link."
        )));
    }

    let requested_quality = quality.unwrap_or("");
    let quality_id = quality
        .filter(|q| is_supported_quality(q))
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let s = &settings.apple_native_quality;
            if is_supported_quality(s) {
                s.clone()
            } else {
                "aac-256".to_string()
            }
        });
    let q = quality_for(&quality_id).unwrap_or_else(|| quality_for("aac-256").unwrap());
    let quality_label = q.label;
    let quality_token = q.quality_label;

    on_log(format!(
        "Native Apple Music: requested='{}' resolved='{}' (group={}, label={})",
        requested_quality, q.id, q.group_pattern, quality_label
    ));
    if matches!(
        classify_apple_url(url),
        AppleUrlSupport::Supported(AppleLinkKind::MusicVideo)
    ) {
        let base_dir = if settings.create_platform_subfolders {
            Path::new(&settings.download_location).join("Apple Music")
        } else {
            PathBuf::from(&settings.download_location)
        };
        return download_music_video(
            settings,
            &service,
            &api,
            url,
            &base_dir,
            &on_progress,
            &on_log,
            &cancel,
        )
        .await;
    }

    on_log(format!("Native Apple Music: resolving {url}"));
    let (tracks, collection_name) = resolve_tracks(&service, &api, url, &on_log).await?;
    if tracks.is_empty() {
        return Err(MhError::Other(
            "No tracks found at this Apple Music URL.".into(),
        ));
    }
    let total = tracks.len();
    on_log(format!(
        "Native Apple Music: {} track(s) queued at {}",
        total, quality_label
    ));

    let base_dir = if settings.create_platform_subfolders {
        Path::new(&settings.download_location).join("Apple Music")
    } else {
        PathBuf::from(&settings.download_location)
    };
    let first = &tracks[0];
    let is_collection = tracks.len() > 1;
    let is_playlist = matches!(
        classify_apple_url(url),
        AppleUrlSupport::Supported(AppleLinkKind::Playlist)
    );
    let album_dir_for = |quality_label: &str, codec_label: &str| {
        dest_dir_for(
            settings,
            &base_dir,
            &ReleaseFolder {
                album_artist: &first.album_artist,
                album: &first.album,
                year: first.year.as_deref().unwrap_or(""),
                genre: first.genre.as_deref().unwrap_or(""),
                label: first.label.as_deref().unwrap_or(""),
                quality_label,
                format: codec_label,
            },
        )
    };
    let fixed_dest_dir = match (is_collection, is_playlist, collection_name.as_deref()) {
        (true, true, Some(name)) if !name.is_empty() => Some(dest_dir_for(
            settings,
            &base_dir,
            &ReleaseFolder {
                album: name,
                quality_label: quality_token,
                format: q.codec_label,
                ..Default::default()
            },
        )),
        _ => None,
    };
    let mut dest_dir = match &fixed_dest_dir {
        Some(d) => {
            tokio::fs::create_dir_all(d).await?;
            d.clone()
        }
        None => album_dir_for(quality_token, q.codec_label),
    };
    let mut dest_dir_settled = fixed_dest_dir.is_some();

    let http = crate::http_client::build_ua_client("MediaHarbor/native-apple")?;

    let mut cover_cache: std::collections::HashMap<String, Option<Bytes>> =
        std::collections::HashMap::new();
    let mut shared_cover_path: Option<PathBuf> = None;
    let exclude_tags = excluded_tags(settings);
    let mut playlist_entries: Vec<PlaylistEntry> = Vec::new();

    let wrapper =
        match crate::services::apple_music::wrapper::WrapperConfig::from_settings(settings) {
            Some(cfg) => Some(crate::services::apple_music::wrapper::WrapperClient::new(
                cfg,
            )?),
            None if q.needs_wrapper => {
                return Err(MhError::Unsupported(format!(
                    "{} is only served to the wrapper. Turn on Settings → Apple Music → Wrapper \
                 and point it at your running wrapper-v2.",
                    q.label
                )))
            }
            None => None,
        };

    let mut outcome = BatchOutcome::new(total);
    let requested_rank = quality_rank("apple", &quality_id);
    for (i, track) in tracks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MhError::Cancelled);
        }
        let completed = outcome.settled();
        if !track.playable {
            let label = format!("{} - {}", track.artist, track.title);
            let reason = format!(
                "{} is listed on this release but Apple has no playable asset for it —                  only a 30-second preview exists, so there is nothing to download",
                track.title
            );
            on_log(format!("[{}/{}] ✗ {}: {}", i + 1, total, label, reason));
            outcome.record_failure(&track.id, &label, MhError::NotFound(reason));
            on_progress(BatchProgress::item_settled(
                i,
                total,
                label,
                outcome.settled(),
            ));
            continue;
        }
        if let Some(d) = dedup {
            if d.should_skip("apple", &track.id, requested_rank) {
                on_log(format!(
                    "[{}/{}] skipped — already downloaded",
                    i + 1,
                    total
                ));
                outcome.record_skip();
                on_progress(BatchProgress::item_settled(
                    i,
                    total,
                    String::new(),
                    outcome.settled(),
                ));
                continue;
            }
        }
        let label = format!("{} - {}", track.artist, track.title);
        on_log(format!("[{}/{}] {}", i + 1, total, label));
        on_log(format!(
            "  meta source={} title='{}' artist='{}' cover={}",
            track.meta_source,
            track.title,
            track.artist,
            if track.cover_url.is_some() {
                "yes"
            } else {
                "no"
            },
        ));
        on_progress(BatchProgress::item_started(
            i,
            total,
            label.clone(),
            completed,
        ));

        if settings.apple_synced_lyrics_only {
            if !dest_dir_settled {
                tokio::fs::create_dir_all(&dest_dir).await?;
                dest_dir_settled = true;
            }
            let basename =
                native_track_name(settings, &track.name_fields(quality_token, q.codec_label));
            let final_path = dest_dir.join(format!("{basename}.m4a"));
            let lyrics = apple_lyrics_for(
                &service,
                settings,
                &track.track_url,
                &track.title,
                &track.artist,
                &on_log,
            )
            .await;
            let sidecars = lyrics.as_ref().map(apple_sidecars).unwrap_or_default();
            if sidecars.is_empty() {
                let reason = "no lyrics published for this track".to_string();
                on_log(format!("  ✗ {label}: {reason}"));
                outcome.record_failure(&track.id, &label, MhError::Other(reason));
                continue;
            }
            write_chosen_sidecar(&final_path, &sidecars, prefers_ttml(settings), &on_log).await;
            outcome.record_success();
            on_progress(BatchProgress::item_settled(
                i,
                total,
                label,
                outcome.settled(),
            ));
            continue;
        }

        let slice_start = (i as f32 / total as f32) * 100.0;
        let slice_ceiling = ((i as f32 + 0.9) / total as f32) * 100.0;
        let bp = crate::downloads::ByteProgress::default();
        let stream = {
            let fut = async {
                match wrapper.as_ref() {
                    Some(w) => {
                        service
                            .get_wrapper_stream_for_download(
                                &track.track_url,
                                w,
                                q.group_pattern,
                                q.codec_label,
                                Some(&bp),
                                Some(&on_log),
                            )
                            .await
                    }
                    None => {
                        service
                            .get_track_stream_for_download_rendition(
                                &track.track_url,
                                q.group_pattern,
                                Some(&bp),
                            )
                            .await
                    }
                }
            };
            tokio::pin!(fut);
            let res = run_progress_tick_loop(
                fut,
                &bp,
                TickSlice {
                    start: slice_start,
                    ceiling: slice_ceiling,
                    label: &label,
                    completed,
                    total: total as u32,
                },
                &cancel,
                &on_progress,
            )
            .await?;
            match res {
                Ok(s) => s,
                Err(e) => {
                    on_log(format!("  ✗ stream failed for {label}: {e}"));
                    outcome.record_failure(&track.id, &label, e);
                    continue;
                }
            }
        };

        let (real_label, real_codec) = match stream.rendition.as_deref() {
            Some(group) => rendition_label(group),
            None => (quality_token.to_string(), q.codec_label),
        };
        if let Some(group) = stream.rendition.as_deref() {
            if !q.group_pattern_matches(group) {
                on_log(format!(
                    "  ⚠ {} unavailable for this track; Apple served {real_label} instead",
                    q.label
                ));
            }
        }

        if !dest_dir_settled {
            dest_dir = album_dir_for(&real_label, real_codec);
            dest_dir_settled = true;
        }
        tokio::fs::create_dir_all(&dest_dir).await?;
        let track_dir = disc_dir_for(
            settings,
            &dest_dir,
            track.disc_number.unwrap_or(1),
            track.disc_total,
        );
        if track_dir != dest_dir {
            tokio::fs::create_dir_all(&track_dir).await?;
        }
        let basename = native_track_name(settings, &track.name_fields(&real_label, real_codec));
        let mut final_path = track_dir.join(format!("{basename}.m4a"));

        on_progress(BatchProgress::at(
            slice_ceiling,
            label.clone(),
            completed,
            total as u32,
        ));

        if let Some(existing) =
            existing_final_file_for(&final_path, settings, settings.apple_overwrite).await
        {
            on_log(format!(
                "  · already on disk, keeping {}",
                existing.display()
            ));
            outcome.record_skip();
            playlist_entries.push(PlaylistEntry {
                path: existing,
                title: track.title.clone(),
                artist: track.artist.clone(),
                duration_secs: None,
            });
            on_progress(BatchProgress::item_settled(
                i,
                total,
                label,
                outcome.settled(),
            ));
            continue;
        }
        let raw_path = track_dir.join(format!("{basename}.raw.m4a"));
        if let Err(e) = tokio::fs::write(&raw_path, &stream.data).await {
            on_log(format!("  ✗ write failed for {label}: {e}"));
            outcome.record_failure(&track.id, &label, e);
            continue;
        }

        remux_or_keep(&raw_path, &final_path, "mp4", false, &on_log).await;

        if let Some(reason) = short_rip_reason(
            &final_path,
            track.duration_ms.map(|ms| ms as f64 / 1000.0),
            None,
        )
        .await
        {
            let _ = tokio::fs::remove_file(&final_path).await;
            let _ = tokio::fs::remove_file(&raw_path).await;
            on_log(format!("  ✗ {label}: {reason}"));
            outcome.record_failure(&track.id, &label, MhError::Other(reason));
            continue;
        }

        match maybe_convert(final_path.clone(), settings, conversion_source(real_codec)).await {
            Ok((converted, note, _)) => {
                if let Some(n) = note {
                    on_log(format!("  {n}"));
                }
                final_path = converted;
            }
            Err(e) => {
                on_log(format!("  ✗ conversion failed for {label}: {e}"));
                outcome.record_failure(&track.id, &label, e);
                continue;
            }
        }

        let (cover_path, shared_cover) = cover_for_track(
            &http,
            &mut cover_cache,
            track
                .cover_url
                .as_deref()
                .map(|t| cover_url_at(t, settings.apple_cover_size, &settings.apple_cover_format)),
            &dest_dir,
            &base_dir,
            &basename,
            &mut shared_cover_path,
            settings,
            &on_log,
        )
        .await;

        let lyrics = apple_lyrics_for(
            &service,
            settings,
            &track.track_url,
            &track.title,
            &track.artist,
            &on_log,
        )
        .await;
        write_apple_lyric_sidecars(lyrics.as_ref(), settings, &final_path, &on_log).await;
        let mut meta_for_tags = track.track_metadata();
        if settings.embed_lyrics {
            meta_for_tags.lyrics = lyrics.as_ref().and_then(|l| l.plain.clone());
        }
        match tag_file(
            &final_path,
            &meta_for_tags,
            settings
                .embed_cover
                .then_some(cover_path.as_deref())
                .flatten(),
            &exclude_tags,
        )
        .await
        {
            Ok(()) => on_log(format!("  ✓ tagged → {}", final_path.display())),
            Err(e) => on_log(format!("  ⚠ tagging failed: {e}")),
        }
        settle_cover_file(cover_path, shared_cover, settings, &on_log).await;

        if let Some(d) = dedup {
            d.record(
                "apple",
                &track.id,
                &final_path.to_string_lossy(),
                requested_rank,
            );
        }
        playlist_entries.push(PlaylistEntry {
            path: final_path.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            duration_secs: None,
        });
        outcome.record_success();
        on_progress(BatchProgress::item_settled(
            i,
            total,
            label,
            outcome.settled(),
        ));
    }

    if let Some(p) = shared_cover_path {
        if settings.save_album_cover {
            on_log(format!("  ✓ cover → {}", p.display()));
        } else {
            let _ = tokio::fs::remove_file(&p).await;
        }
    }

    if settings.save_playlist_file {
        if let Some(name) = collection_name.as_deref() {
            write_playlist_file(&dest_dir, name, &playlist_entries, &on_log).await;
        }
    }

    let outcome = outcome.require_any("Apple Music")?.with_dest(&dest_dir);
    on_log(outcome.failure_report("Native Apple Music"));
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_supported() {
        assert!(matches!(
            classify_apple_url("https://music.apple.com/us/album/born-to-die/1440818910"),
            AppleUrlSupport::Supported(AppleLinkKind::Album)
        ));
        assert!(matches!(
            classify_apple_url("https://music.apple.com/us/album/song/1440818910?i=1440819033"),
            AppleUrlSupport::Supported(AppleLinkKind::Song)
        ));
    }

    #[test]
    fn classify_storefront_less_url() {
        assert!(matches!(
            classify_apple_url("https://music.apple.com/song/1440855552"),
            AppleUrlSupport::Supported(AppleLinkKind::Song)
        ));
        assert!(matches!(
            classify_apple_url("https://music.apple.com/album/1440818910"),
            AppleUrlSupport::Supported(AppleLinkKind::Album)
        ));
        let canonical = "https://music.apple.com/us/album/born-to-die/1440818910";
        assert_eq!(normalize_apple_url(canonical), canonical);
        assert_eq!(
            normalize_apple_url("https://example.com/foo"),
            "https://example.com/foo"
        );
    }

    #[test]
    fn classify_library_numeric_url() {
        assert!(matches!(
            classify_apple_url("https://music.apple.com/library/song/417723943"),
            AppleUrlSupport::Supported(AppleLinkKind::Song)
        ));
        assert!(matches!(
            classify_apple_url("https://music.apple.com/library/songs/417723943"),
            AppleUrlSupport::Supported(AppleLinkKind::Song)
        ));
        let normalized = normalize_apple_url("https://music.apple.com/library/song/417723943");
        assert!(
            normalized.contains("/us/song/x/417723943"),
            "got: {normalized}"
        );
    }

    #[test]
    fn classify_library_private_item() {
        match classify_apple_url("https://music.apple.com/library/songs/i.aBcDeF12") {
            AppleUrlSupport::Unsupported(msg) => {
                assert!(msg.contains("library item"), "got: {msg}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn classify_album_song_with_extra_query() {
        assert!(matches!(
            classify_apple_url(
                "https://music.apple.com/us/album/kissing-a-fool/395918916?i=395918966&uo=4"
            ),
            AppleUrlSupport::Supported(AppleLinkKind::Song)
        ));
        assert!(matches!(
            classify_apple_url(
                "https://music.apple.com/us/album/born-to-die/1440818910?i=1440819033&app=music"
            ),
            AppleUrlSupport::Supported(AppleLinkKind::Song)
        ));
    }

    #[test]
    fn music_videos_are_handled_natively() {
        assert!(matches!(
            classify_apple_url("https://music.apple.com/us/music-video/blank-space/1218040623"),
            AppleUrlSupport::Supported(AppleLinkKind::MusicVideo)
        ));
    }

    #[test]
    fn classify_unsupported() {
        assert!(matches!(
            classify_apple_url("https://music.apple.com/us/artist/taylor-swift/159260351"),
            AppleUrlSupport::Unsupported(_)
        ));
    }

    #[test]
    fn classify_not_apple() {
        assert!(matches!(
            classify_apple_url("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            AppleUrlSupport::NotApple
        ));
    }

    #[test]
    fn a_rendition_is_named_after_what_apple_served() {
        assert_eq!(
            rendition_label("audio-atmos-2768"),
            ("768 kbps".to_string(), "E-AC-3")
        );
        assert_eq!(
            rendition_label("audio-atmos-2448"),
            ("448 kbps".to_string(), "E-AC-3")
        );
        assert_eq!(
            rendition_label("audio-stereo-256"),
            ("256 kbps".to_string(), "AAC")
        );
        assert_eq!(
            rendition_label("audio-HE-stereo-64"),
            ("64 kbps".to_string(), "HE-AAC")
        );
        assert_eq!(
            rendition_label("audio-stereo-256-binaural"),
            ("256 kbps (Binaural)".to_string(), "AAC")
        );
        assert_eq!(
            rendition_label("audio-alac-stereo-44100-24"),
            ("24-bit ⁄ 44.1kHz".to_string(), "ALAC")
        );
        assert_eq!(
            rendition_label("audio-alac-stereo-96000-24"),
            ("24-bit ⁄ 96kHz".to_string(), "ALAC")
        );
    }

    /// `{quality}` and `{format}` are separate template vars, so a template of
    /// `{format} {quality}` must not read "AAC AAC 256 kbps". Naming the codec in
    /// both halves is the bug this locks out.
    /// `{year}` is a folder token and has to stay four digits; the tag wants the
    /// whole calendar date. AMP gives `2001-03-07`, iTunes `2001-03-07T08:00:00Z`.
    #[test]
    fn a_release_date_splits_into_a_year_and_a_calendar_date() {
        assert_eq!(release_year("2001-03-07"), Some("2001".to_string()));
        assert_eq!(release_date("2001-03-07"), Some("2001-03-07".to_string()));
        assert_eq!(
            release_date("2001-03-07T08:00:00Z"),
            Some("2001-03-07".to_string())
        );
        assert_eq!(release_date("2001"), Some("2001".to_string()));
        assert_eq!(release_date(""), None);
    }

    /// A compilation's album artist is the album's, never the first track's.
    #[test]
    fn a_compilation_keeps_the_albums_own_artist() {
        let song = serde_json::json!({
            "id": "1",
            "attributes": {
                "name": "Get Lucky",
                "artistName": "Daft Punk",
                "albumName": "A Compilation",
                "playParams": { "id": "1" }
            }
        });
        let album_attrs = serde_json::json!({
            "artistName": "Various Artists",
            "recordLabel": "Columbia Records",
            "trackCount": 20,
            "upc": "0000000000000"
        });
        let meta = meta_from_apple_song("us", &song, Some(&album_attrs)).unwrap();
        assert_eq!(meta.album_artist, "Various Artists");
        assert_eq!(meta.artist, "Daft Punk");
        assert_eq!(meta.label.as_deref(), Some("Columbia Records"));
        assert_eq!(meta.upc.as_deref(), Some("0000000000000"));
        assert_eq!(meta.disc_total, None);

        let tags = meta.track_metadata();
        assert_eq!(tags.album_artist, vec!["Various Artists".to_string()]);
        assert_eq!(tags.label.as_deref(), Some("Columbia Records"));
    }

    /// Apple puts `albumArtistName` on the track, which is the most direct source
    /// and works even when no album record came along.
    #[test]
    fn a_tracks_own_album_artist_wins_over_the_album_record() {
        let song = serde_json::json!({
            "id": "1",
            "attributes": {
                "name": "Get Lucky",
                "artistName": "Daft Punk",
                "albumArtistName": "Various Artists",
                "playParams": { "id": "1" }
            }
        });
        let meta = meta_from_apple_song("us", &song, None).unwrap();
        assert_eq!(meta.album_artist, "Various Artists");
    }

    #[test]
    fn a_quality_label_never_repeats_its_own_codec() {
        for q in APPLE_QUALITIES {
            assert!(
                !q.quality_label.contains(q.codec_label),
                "{} pairs quality {:?} with codec {:?}",
                q.id,
                q.quality_label,
                q.codec_label
            );
        }
        for group in [
            "audio-stereo-256",
            "audio-HE-stereo-64",
            "audio-atmos-2768",
            "audio-ac3-448",
            "audio-alac-stereo-96000-24",
            "audio-stereo-256-downmix",
        ] {
            let (quality, codec) = rendition_label(group);
            assert!(
                !quality.contains(codec),
                "{group} renders {quality:?} beside codec {codec:?}"
            );
        }
    }

    /// The whole point of the rendition label: asking for Atmos and being handed
    /// AAC must not produce a folder that claims Atmos.
    #[test]
    fn a_downgraded_rendition_is_not_labelled_as_the_request() {
        let atmos = quality_for("atmos").unwrap();
        assert!(atmos.group_pattern_matches("audio-atmos-2768"));
        assert!(!atmos.group_pattern_matches("audio-stereo-256"));
        let (label, codec) = rendition_label("audio-stereo-256");
        assert_ne!(label, atmos.quality_label);
        assert_eq!(codec, "AAC");
    }

    /// The Hercules OST lists "Go The Distance (Single)" but Apple has no asset for
    /// it: no playParams, no extendedAssetUrls, no duration. It has to be recognised
    /// before the download is attempted, or the failure surfaces from the HLS layer
    /// as a subscription problem.
    #[test]
    fn a_track_without_play_params_is_not_playable() {
        let listed_only = serde_json::json!({
            "id": "1412859040",
            "attributes": { "name": "Go The Distance (Single)", "trackNumber": 12 }
        });
        let meta = meta_from_apple_song("mx", &listed_only, None).unwrap();
        assert!(!meta.playable);
        assert_eq!(meta.duration_ms, None);

        let real = serde_json::json!({
            "id": "1412859039",
            "attributes": {
                "name": "A Star Is Born",
                "trackNumber": 11,
                "durationInMillis": 124200,
                "playParams": { "id": "1412859039", "kind": "song" }
            }
        });
        let meta = meta_from_apple_song("mx", &real, None).unwrap();
        assert!(meta.playable);
        assert_eq!(meta.duration_ms, Some(124200));
    }

    /// Album listings carry no `relationships.albums`, so the release-level fields
    /// have to come from the album record that produced them.
    #[test]
    fn album_attributes_fill_the_release_fields() {
        let song = serde_json::json!({
            "id": "1851266390",
            "attributes": {
                "name": "World of Our Own",
                "playParams": { "id": "1851266390", "kind": "song" }
            }
        });
        let album = serde_json::json!({
            "name": "25 - The Ultimate Collection",
            "recordLabel": "Sony Music UK",
            "trackCount": 23,
            "copyright": "℗ 2026 Sony Music Entertainment UK Ltd"
        });
        let meta = meta_from_apple_song("us", &song, Some(&album)).unwrap();
        assert_eq!(meta.label.as_deref(), Some("Sony Music UK"));
        assert_eq!(meta.total_tracks, Some(23));
        assert!(meta.copyright.is_some());
    }

    /// A 32 ms rip with a 500 KB cover embedded clears any byte floor worth setting,
    /// so the catalog duration is what decides whether the file is complete.
    #[tokio::test]
    async fn a_truncated_rip_is_caught_by_duration_not_size() {
        use std::io::Write;
        let write = |n: usize| {
            let mut f = tempfile::NamedTempFile::new().unwrap();
            f.write_all(&vec![0u8; n]).unwrap();
            f
        };
        let tiny = write(512);
        assert!(short_rip_reason(tiny.path(), None, None).await.is_some());
        let big = write(516_456);
        assert!(short_rip_reason(big.path(), None, None).await.is_none());
        assert!(short_rip_reason(big.path(), Some(210.04), None)
            .await
            .is_none());
    }

    #[test]
    fn quality_recognised() {
        for q in APPLE_QUALITIES {
            assert!(is_supported_quality(q.id), "{}", q.id);
        }
        assert!(is_supported_quality("atmos"));
        assert!(is_supported_quality("aac-256-binaural"));
        assert!(is_supported_quality("aac-128"));
        assert!(is_supported_quality("alac"));
        assert!(!is_supported_quality("nonsense"));
        assert!(quality_for("alac").unwrap().needs_wrapper);
        assert!(!quality_for("aac-256").unwrap().needs_wrapper);
        assert_eq!(
            quality_for("atmos").unwrap().group_pattern,
            r"audio-atmos-.*"
        );
        assert_eq!(quality_for("atmos").unwrap().codec_label, "E-AC-3");
    }
}

#[cfg(test)]
mod conversion_source_tests {
    use super::{conversion_source, rendition_label, APPLE_QUALITIES};

    #[test]
    fn stereo_ladders_are_convertible_and_only_the_spatial_ones_are_not() {
        assert!(!conversion_source("AAC").surround);
        assert!(!conversion_source("HE-AAC").surround);
        assert!(!conversion_source("ALAC").surround);
        assert!(conversion_source("E-AC-3").surround);
        assert!(conversion_source("AC-3").surround);
    }

    /// Every codec label the ladder and the rendition parser can produce has to be
    /// classified deliberately; the catch-all arm means "surround", which silently
    /// cancels the re-encode.
    #[test]
    fn every_published_codec_label_is_classified() {
        let known = ["ALAC", "AAC", "HE-AAC", "E-AC-3", "AC-3"];
        for q in APPLE_QUALITIES {
            assert!(
                known.contains(&q.codec_label),
                "unclassified codec label {:?} would be treated as surround",
                q.codec_label
            );
        }
        for group in [
            "audio-stereo-256",
            "audio-HE-stereo-64",
            "audio-alac-stereo-44100-16",
            "audio-atmos-2768",
            "audio-ac3-640",
        ] {
            let (_, codec) = rendition_label(group);
            assert!(
                known.contains(&codec),
                "unclassified codec label {codec:?} from {group}"
            );
        }
    }
}
