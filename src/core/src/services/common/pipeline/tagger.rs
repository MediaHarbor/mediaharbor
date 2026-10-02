use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::ogg::tag::VorbisComments;
use lofty::picture::{Picture, PictureType};
use lofty::tag::{ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};
use tempfile::NamedTempFile;

use crate::errors::{MhError, MhResult};

#[derive(Debug, Default, Clone)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Vec<String>,
    pub year: Option<String>,
    pub original_date: Option<String>,
    pub genre: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub total_tracks: Option<u32>,
    pub total_discs: Option<u32>,
    pub comment: Option<String>,
    pub lyrics: Option<String>,
    pub isrc: Option<String>,
    pub upc: Option<String>,
    pub copyright: Option<String>,
    pub label: Option<String>,
    pub composer: Vec<String>,
    pub conductor: Vec<String>,
    pub performer: Vec<String>,
    pub lyricist: Vec<String>,
    pub producer: Vec<String>,
    pub engineer: Vec<String>,
    pub mixer: Vec<String>,
    pub bpm: Option<String>,
    pub replaygain_track_gain: Option<String>,
    pub replaygain_track_peak: Option<String>,
    pub replaygain_album_gain: Option<String>,
    pub replaygain_album_peak: Option<String>,
    pub description: Option<String>,
    pub purchase_date: Option<String>,
    pub grouping: Option<String>,
}

/// Wraps a single optional string as the one-element list the multi-value fields
/// take, so a service that only ever publishes one name does not have to spell out
/// the `Vec` dance at every call site.
pub fn one(value: Option<String>) -> Vec<String> {
    value.into_iter().filter(|v| !v.trim().is_empty()).collect()
}

pub async fn download_cover_art(client: &reqwest::Client, url: &str) -> MhResult<NamedTempFile> {
    cover_temp_file(&cover_bytes(client, url).await?)
}

/// Cover art for one URL, fetched at most once for the whole run. Every track on a
/// release points at the same image, so without the cache a twenty-track album
/// downloaded the same JPEG twenty times.
///
/// The bytes are cached rather than the temp file: each track still needs its own
/// file on disk, because the tagger and the sidecar writer both take a path.
pub async fn download_cover_art_cached(
    client: &reqwest::Client,
    url: &str,
    cache: &crate::services::common::pipeline::CoverCache,
) -> MhResult<NamedTempFile> {
    cover_temp_file(&cover_bytes_cached(client, url, cache).await?)
}

/// The cover bytes for one URL, fetched at most once for the whole run.
///
/// Callers that need a file on disk as well should keep these bytes rather than read
/// the file back: the sidecar writer wants bytes and the tagger wants a path, and
/// serving both from one fetch is the difference between one write per release and a
/// write plus a read per track.
pub(crate) async fn cover_bytes_cached(
    client: &reqwest::Client,
    url: &str,
    cache: &crate::services::common::pipeline::CoverCache,
) -> MhResult<Vec<u8>> {
    if let Some(hit) = cache.get(url) {
        return match hit {
            Some(bytes) => Ok(bytes),
            None => Err(MhError::Other(format!("cover art unavailable at {url}"))),
        };
    }
    match cover_bytes(client, url).await {
        Ok(bytes) => {
            cache.put(url, Some(bytes.clone()));
            Ok(bytes)
        }
        Err(e) => {
            cache.put(url, None);
            Err(e)
        }
    }
}

pub async fn cover_bytes(client: &reqwest::Client, url: &str) -> MhResult<Vec<u8>> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "cover art download failed: HTTP {}",
            resp.status().as_u16()
        )));
    }
    let bytes = resp.bytes().await?;
    if bytes.is_empty() {
        return Err(MhError::Other(
            "cover art download returned empty body".into(),
        ));
    }
    Ok(bytes.to_vec())
}

pub(crate) fn cover_temp_file(bytes: &[u8]) -> MhResult<NamedTempFile> {
    let mut tmp = NamedTempFile::new()?;
    use std::io::Write;
    tmp.write_all(bytes)?;
    Ok(tmp)
}

/// Writes tags in place with lofty, which maps each field onto the container's
/// native representation. The previous ffmpeg `-metadata` approach silently lost
/// every key outside the muxer's whitelist — ISRC, barcode, label, lyrics and all
/// credit roles vanished from M4A and MP3.
pub async fn tag_file(
    path: &Path,
    metadata: &TrackMetadata,
    cover_path: Option<&Path>,
    exclude_tags: &[String],
) -> MhResult<()> {
    let path: PathBuf = path.to_path_buf();
    let metadata = metadata.clone();
    let cover = cover_path.map(|p| p.to_path_buf());
    let excluded: HashSet<String> = exclude_tags.iter().map(|s| s.to_lowercase()).collect();

    tokio::task::spawn_blocking(move || write_tags(&path, &metadata, cover.as_deref(), &excluded))
        .await
        .map_err(|e| MhError::Other(format!("tagging task failed to join: {e}")))?
}

fn write_tags(
    path: &Path,
    metadata: &TrackMetadata,
    cover_path: Option<&Path>,
    excluded: &HashSet<String>,
) -> MhResult<()> {
    let mut tagged = lofty::read_from_path(path).map_err(|e| {
        MhError::Other(format!(
            "could not read {} for tagging: {e}",
            path.display()
        ))
    })?;

    let tag_type = tagged.primary_tag_type();
    if tagged.primary_tag_mut().is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let Some(tag) = tagged.primary_tag_mut() else {
        return Err(MhError::Other(format!(
            "{} does not support a writable tag",
            path.display()
        )));
    };

    let single: [(&str, ItemKey, Option<&String>); 10] = [
        ("title", ItemKey::TrackTitle, metadata.title.as_ref()),
        ("album", ItemKey::AlbumTitle, metadata.album.as_ref()),
        ("comment", ItemKey::Comment, metadata.comment.as_ref()),
        (
            "lyrics",
            if tag_type == TagType::Id3v2 {
                ItemKey::UnsyncLyrics
            } else {
                ItemKey::Lyrics
            },
            metadata.lyrics.as_ref(),
        ),
        ("isrc", ItemKey::Isrc, metadata.isrc.as_ref()),
        ("upc", ItemKey::Barcode, metadata.upc.as_ref()),
        (
            "copyright",
            ItemKey::CopyrightMessage,
            metadata.copyright.as_ref(),
        ),
        ("label", ItemKey::Label, metadata.label.as_ref()),
        (
            "description",
            ItemKey::Description,
            metadata.description.as_ref(),
        ),
        (
            "grouping",
            ItemKey::ContentGroup,
            metadata.grouping.as_ref(),
        ),
    ];
    for (field, key, value) in single {
        set_text(tag, field, key, value, excluded);
    }

    set_text(
        tag,
        "bpm",
        ItemKey::IntegerBpm,
        metadata.bpm.as_ref(),
        excluded,
    );
    if tag_type == TagType::VorbisComments {
        set_text(tag, "bpm", ItemKey::Bpm, metadata.bpm.as_ref(), excluded);
    }

    let replaygain: [(&str, ItemKey, Option<&String>); 4] = [
        (
            "replaygain_track_gain",
            ItemKey::ReplayGainTrackGain,
            metadata.replaygain_track_gain.as_ref(),
        ),
        (
            "replaygain_track_peak",
            ItemKey::ReplayGainTrackPeak,
            metadata.replaygain_track_peak.as_ref(),
        ),
        (
            "replaygain_album_gain",
            ItemKey::ReplayGainAlbumGain,
            metadata.replaygain_album_gain.as_ref(),
        ),
        (
            "replaygain_album_peak",
            ItemKey::ReplayGainAlbumPeak,
            metadata.replaygain_album_peak.as_ref(),
        ),
    ];
    for (field, key, value) in replaygain {
        set_text(tag, field, key, value, excluded);
    }

    let multi: [(&str, ItemKey, &Vec<String>); 10] = [
        ("artist", ItemKey::TrackArtist, &metadata.artist),
        ("album_artist", ItemKey::AlbumArtist, &metadata.album_artist),
        ("genre", ItemKey::Genre, &metadata.genre),
        ("composer", ItemKey::Composer, &metadata.composer),
        ("conductor", ItemKey::Conductor, &metadata.conductor),
        ("performer", ItemKey::Performer, &metadata.performer),
        ("lyricist", ItemKey::Lyricist, &metadata.lyricist),
        ("producer", ItemKey::Producer, &metadata.producer),
        ("engineer", ItemKey::Engineer, &metadata.engineer),
        ("mixer", ItemKey::MixEngineer, &metadata.mixer),
    ];
    for (field, key, values) in multi {
        set_multi(tag, tag_type, field, key, values, excluded);
    }

    write_date(tag, metadata.year.as_deref(), excluded);
    set_text(
        tag,
        "original_date",
        ItemKey::OriginalReleaseDate,
        metadata.original_date.as_ref(),
        excluded,
    );
    write_number(
        tag,
        "tracknumber",
        ItemKey::TrackNumber,
        ItemKey::TrackTotal,
        metadata.track_number,
        metadata.total_tracks,
        excluded,
    );
    write_number(
        tag,
        "discnumber",
        ItemKey::DiscNumber,
        ItemKey::DiscTotal,
        metadata.disc_number,
        metadata.total_discs,
        excluded,
    );

    if let Some(cover) = cover_path {
        attach_cover(tag, cover)?;
    }

    let purchased = metadata
        .purchase_date
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty() && !excluded.contains("purchase_date"))
        .filter(|_| tag_type == TagType::VorbisComments)
        .map(|v| (v.to_string(), tag.clone()));

    match purchased {
        Some((value, owned)) => {
            let mut comments = VorbisComments::from(owned);
            comments.insert("PURCHASE_DATE".to_string(), value);
            comments.save_to_path(path, WriteOptions::default())
        }
        None => tagged.save_to_path(path, WriteOptions::default()),
    }
    .map_err(|e| MhError::Other(format!("could not write tags to {}: {e}", path.display())))?;

    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("flac"))
    {
        verify_flac_structure(path)?;
    }
    Ok(())
}

/// Walks the metadata block chain and confirms audio actually follows it.
///
/// A tagger that silently produces undecodable files is worse than one that
/// fails, and the fixtures this module tests against all carry padding — so the
/// corruption above went unnoticed. This check runs on the real output.
fn verify_flac_structure(path: &Path) -> MhResult<()> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"fLaC" {
        return Err(MhError::Other(format!(
            "{} is not a FLAC stream after tagging",
            path.display()
        )));
    }

    let size = file.metadata()?.len();
    let mut offset: u64 = 4;
    for _ in 0..128 {
        let mut header = [0u8; 4];
        file.seek(SeekFrom::Start(offset))?;
        if file.read_exact(&mut header).is_err() {
            return Err(MhError::Other(format!(
                "{} has a corrupt metadata chain: a block header at {offset} runs past \
                 the end of the {size}-byte file",
                path.display()
            )));
        }
        let last = header[0] & 0x80 != 0;
        let len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as u64;
        offset += 4 + len;
        if last {
            let mut sync = [0u8; 2];
            file.seek(SeekFrom::Start(offset))?;
            if file.read_exact(&mut sync).is_err() {
                return Err(MhError::Other(format!(
                    "{} has a corrupt metadata chain: the last block ends at {offset}, \
                     past the end of the {size}-byte file",
                    path.display()
                )));
            }
            if sync[0] == 0xFF && sync[1] & 0xFC == 0xF8 {
                return Ok(());
            }
            return Err(MhError::Other(format!(
                "{} has a corrupt metadata chain: the block marked last is followed \
                 by {:02x}{:02x}, not an audio frame",
                path.display(),
                sync[0],
                sync[1]
            )));
        }
    }
    Err(MhError::Other(format!(
        "{} has more than 128 metadata blocks; refusing to trust it",
        path.display()
    )))
}

/// `year` arrives as either a bare year or a full ISO date depending on the service;
/// both the year and the full recording date are written so players that prefer
/// either one show something.
fn write_date(tag: &mut Tag, value: Option<&str>, excluded: &HashSet<String>) {
    if excluded.contains("year") {
        return;
    }
    let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        tag.remove_key(ItemKey::RecordingDate);
        tag.remove_key(ItemKey::Year);
        return;
    };
    tag.insert_text(ItemKey::RecordingDate, raw.to_string());
    if let Some(year) = raw.split(['-', '/', 'T']).next().filter(|y| y.len() == 4) {
        tag.insert_text(ItemKey::Year, year.to_string());
    }
}

fn write_number(
    tag: &mut Tag,
    field: &str,
    num_key: ItemKey,
    total_key: ItemKey,
    num: Option<u32>,
    total: Option<u32>,
    excluded: &HashSet<String>,
) {
    if excluded.contains(field) {
        return;
    }
    match num.filter(|n| *n > 0) {
        Some(n) => {
            tag.insert_text(num_key, n.to_string());
        }
        None => tag.remove_key(num_key),
    }
    match total.filter(|t| *t > 0) {
        Some(t) => {
            tag.insert_text(total_key, t.to_string());
        }
        None => tag.remove_key(total_key),
    }
}

fn set_text(
    tag: &mut Tag,
    field: &str,
    key: ItemKey,
    value: Option<&String>,
    excluded: &HashSet<String>,
) {
    if excluded.contains(field) {
        return;
    }
    match value.map(|v| v.trim()).filter(|v| !v.is_empty()) {
        Some(v) => {
            tag.insert_text(key, v.to_string());
        }
        None => tag.remove_key(key),
    }
}

/// Vorbis comments express several artists — or composers, or genres — by repeating
/// the key, not by joining them into one string. Every other container takes a single
/// value, so those keep the joined form they have always had.
fn set_multi(
    tag: &mut Tag,
    tag_type: TagType,
    field: &str,
    key: ItemKey,
    values: &[String],
    excluded: &HashSet<String>,
) {
    if excluded.contains(field) {
        return;
    }
    let values: Vec<&str> = values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect();
    tag.remove_key(key);
    if let Some(plural) = plural_key(key) {
        tag.remove_key(plural);
    }
    if values.is_empty() {
        return;
    }
    if tag_type == TagType::VorbisComments {
        for v in values {
            tag.push(TagItem::new(key, ItemValue::Text(v.to_string())));
        }
        return;
    }
    tag.insert_text(key, values.join(", "));
    if values.len() > 1 {
        if let Some(plural) = plural_key(key) {
            for v in values {
                tag.push(TagItem::new(plural, ItemValue::Text(v.to_string())));
            }
        }
    }
}

/// MP4 and ID3 have no multi-value form of the standard artist frames, so a record with
/// three credited artists used to flatten to one `"a, b, c"` string. The plural keys are
/// separate side channels — `TXXX:ARTISTS` and `----:com.apple.iTunes:ARTISTS` — so the
/// joined value stays put for players that only read the standard frame.
fn plural_key(key: ItemKey) -> Option<ItemKey> {
    match key {
        ItemKey::TrackArtist => Some(ItemKey::TrackArtists),
        ItemKey::AlbumArtist => Some(ItemKey::AlbumArtists),
        _ => None,
    }
}

fn attach_cover(tag: &mut Tag, cover_path: &Path) -> MhResult<()> {
    let mut file = std::fs::File::open(cover_path)?;
    let mut picture = Picture::from_reader(&mut file).map_err(|e| {
        MhError::Other(format!(
            "cover art at {} is not a usable image: {e}",
            cover_path.display()
        ))
    })?;
    picture.set_pic_type(PictureType::CoverFront);
    tag.remove_picture_type(PictureType::CoverFront);
    tag.push_picture(picture);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excluded(fields: &[&str]) -> HashSet<String> {
        fields.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn date_writes_year_and_full_date() {
        let mut tag = Tag::new(TagType::VorbisComments);
        write_date(&mut tag, Some("1995-11-07"), &excluded(&[]));
        assert_eq!(tag.get_string(ItemKey::RecordingDate), Some("1995-11-07"));
        assert_eq!(tag.get_string(ItemKey::Year), Some("1995"));
    }

    #[test]
    fn bare_year_is_accepted() {
        let mut tag = Tag::new(TagType::VorbisComments);
        write_date(&mut tag, Some("1995"), &excluded(&[]));
        assert_eq!(tag.get_string(ItemKey::Year), Some("1995"));
    }

    #[test]
    fn empty_date_clears_rather_than_writing_blanks() {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.insert_text(ItemKey::Year, "2000".into());
        write_date(&mut tag, Some("   "), &excluded(&[]));
        assert_eq!(tag.get_string(ItemKey::Year), None);
    }

    #[test]
    fn excluded_fields_are_left_alone() {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.insert_text(ItemKey::Year, "2000".into());
        write_date(&mut tag, Some("1995"), &excluded(&["year"]));
        assert_eq!(tag.get_string(ItemKey::Year), Some("2000"));
    }

    /// The regression this whole module exists to prevent: ffmpeg's `-metadata`
    /// silently dropped ISRC, barcode, label, lyrics and every credit role on M4A and
    /// MP3. Each container is written and read back to prove they survive.
    #[test]
    fn extended_tags_survive_a_round_trip_in_every_container() {
        let Some(ffmpeg) = which_ffmpeg() else {
            panic!("ffmpeg is required to build the tagging fixtures");
        };
        let dir = tempfile::tempdir().expect("tempdir");

        let metadata = TrackMetadata {
            title: Some("Give Life Back to Music".into()),
            artist: vec!["Daft Punk".into(), "Nile Rodgers".into()],
            album: Some("Random Access Memories".into()),
            album_artist: one(Some("Thomas Bangalter/Guy-Manuel".into())),
            year: Some("2013-05-17".into()),
            original_date: Some("2013-05-17".into()),
            genre: one(Some("Disco".into())),
            bpm: Some("93".into()),
            replaygain_track_gain: Some("-8.11 dB".into()),
            replaygain_track_peak: Some("0.988312".into()),
            track_number: Some(1),
            total_tracks: Some(13),
            disc_number: Some(1),
            total_discs: Some(1),
            isrc: Some("USTES1300001".into()),
            upc: Some("0888800000001".into()),
            label: Some("Columbia Records".into()),
            copyright: Some("(C) 2013 Columbia Records".into()),
            composer: one(Some("Daft Punk".into())),
            producer: one(Some("Daft Punk".into())),
            engineer: one(Some("Peter Franco".into())),
            mixer: one(Some("Mick Guzauski".into())),
            lyricist: one(Some("Daft Punk".into())),
            lyrics: Some("[00:01.00]Line one".into()),
            grouping: Some("Nu-Disco".into()),
            ..Default::default()
        };

        let cover = dir.path().join("cover.jpg");
        assert!(std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=32x32",
                "-frames:v",
                "1",
            ])
            .arg(&cover)
            .status()
            .expect("spawn ffmpeg")
            .success());

        for ext in ["flac", "m4a", "mp3", "opus", "ogg"] {
            let path = dir.path().join(format!("fixture.{ext}"));
            let status = std::process::Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=44100:cl=stereo",
                    "-t",
                    "0.2",
                ])
                .arg(&path)
                .status()
                .expect("spawn ffmpeg");
            assert!(status.success(), "ffmpeg could not build a .{ext} fixture");

            write_tags(&path, &metadata, Some(&cover), &excluded(&[])).unwrap_or_else(|e| {
                panic!("tagging .{ext} failed: {e}");
            });

            let tagged = lofty::read_from_path(&path).expect("re-read");
            let tag = tagged
                .primary_tag()
                .or_else(|| tagged.first_tag())
                .unwrap_or_else(|| panic!(".{ext} came back with no tag"));

            for (key, expected) in [
                (ItemKey::TrackTitle, "Give Life Back to Music"),
                (ItemKey::AlbumArtist, "Thomas Bangalter/Guy-Manuel"),
                (ItemKey::Genre, "Disco"),
                (ItemKey::Isrc, "USTES1300001"),
                (ItemKey::Barcode, "0888800000001"),
                (ItemKey::Lyricist, "Daft Punk"),
                (ItemKey::ContentGroup, "Nu-Disco"),
            ] {
                assert_eq!(tag.get_string(key), Some(expected), ".{ext} lost {key:?}");
            }
            assert_eq!(
                tag.get_string(ItemKey::Label)
                    .or_else(|| tag.get_string(ItemKey::Publisher)),
                Some("Columbia Records"),
                ".{ext} lost the label"
            );
            assert_eq!(
                tag.get_string(ItemKey::Lyrics)
                    .or_else(|| tag.get_string(ItemKey::UnsyncLyrics)),
                Some("[00:01.00]Line one"),
                ".{ext} lost the lyrics"
            );

            if ext != "mp3" {
                for (key, expected) in [
                    (ItemKey::Producer, "Daft Punk"),
                    (ItemKey::Engineer, "Peter Franco"),
                    (ItemKey::MixEngineer, "Mick Guzauski"),
                ] {
                    assert_eq!(tag.get_string(key), Some(expected), ".{ext} lost {key:?}");
                }
            }
            assert_eq!(
                tag.get_string(ItemKey::RecordingDate),
                Some("2013-05-17"),
                ".{ext} recording date"
            );
            if ext == "flac" {
                assert_eq!(tag.get_string(ItemKey::Year), Some("2013"), ".{ext} year");
            }
            for (key, expected) in [
                (ItemKey::OriginalReleaseDate, "2013-05-17"),
                (ItemKey::ReplayGainTrackGain, "-8.11 dB"),
                (ItemKey::ReplayGainTrackPeak, "0.988312"),
            ] {
                assert_eq!(tag.get_string(key), Some(expected), ".{ext} lost {key:?}");
            }
            assert_eq!(
                tag.get_string(ItemKey::IntegerBpm)
                    .or_else(|| tag.get_string(ItemKey::Bpm)),
                Some("93"),
                ".{ext} lost the bpm"
            );
            let artists: Vec<&str> = tag.get_strings(ItemKey::TrackArtist).collect();
            if ext == "flac" || ext == "opus" || ext == "ogg" {
                assert_eq!(
                    artists,
                    ["Daft Punk", "Nile Rodgers"],
                    ".{ext} multi-value artist"
                );
            } else {
                assert_eq!(artists, ["Daft Punk, Nile Rodgers"], ".{ext} joined artist");
            }
            assert_eq!(
                tag.get_string(ItemKey::TrackNumber),
                Some("1"),
                ".{ext} track number"
            );

            let pictures = tag.pictures();
            assert_eq!(pictures.len(), 1, ".{ext} cover art");
            if ext != "m4a" {
                assert_eq!(
                    pictures[0].pic_type(),
                    PictureType::CoverFront,
                    ".{ext} cover is not marked as the front cover"
                );
            }
        }
    }

    fn which_ffmpeg() -> Option<String> {
        let candidate = crate::venv_manager::resolve_ffmpeg();
        std::process::Command::new(&candidate)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| candidate)
    }

    /// Strips the trailing PADDING block, reproducing the shape Qobuz's CDN
    /// serves: STREAMINFO, a SEEKTABLE, no padding.
    fn strip_trailing_padding(path: &std::path::Path) {
        let bytes = std::fs::read(path).expect("read fixture");
        assert_eq!(&bytes[..4], b"fLaC");
        let mut offset = 4usize;
        let mut blocks: Vec<(usize, usize, u8)> = Vec::new();
        loop {
            let header = &bytes[offset..offset + 4];
            let last = header[0] & 0x80 != 0;
            let ty = header[0] & 0x7f;
            let len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
            blocks.push((offset, 4 + len, ty));
            offset += 4 + len;
            if last {
                break;
            }
        }
        let audio = &bytes[offset..];
        let kept: Vec<&(usize, usize, u8)> = blocks.iter().filter(|(_, _, ty)| *ty != 1).collect();
        let mut out = b"fLaC".to_vec();
        for (i, (start, len, _)) in kept.iter().enumerate() {
            let mut block = bytes[*start..*start + *len].to_vec();
            block[0] &= 0x7f;
            if i + 1 == kept.len() {
                block[0] |= 0x80;
            }
            out.extend_from_slice(&block);
        }
        out.extend_from_slice(audio);
        std::fs::write(path, out).expect("write stripped fixture");
    }

    /// Appends a SEEKTABLE as the final metadata block — the shape Qobuz serves,
    /// and the one that triggered the corruption.
    fn append_seektable(path: &std::path::Path) {
        const POINTS: usize = 1;
        let bytes = std::fs::read(path).expect("read fixture");
        let mut offset = 4usize;
        let mut last_header;
        loop {
            let header = &bytes[offset..offset + 4];
            let last = header[0] & 0x80 != 0;
            let len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
            last_header = offset;
            offset += 4 + len;
            if last {
                break;
            }
        }
        let mut out = bytes[..offset].to_vec();
        out[last_header] &= 0x7f;
        let payload = POINTS * 18;
        let mut block = vec![0u8; 4 + payload];
        block[0] = 0x80 | 3;
        block[1..4].copy_from_slice(&(payload as u32).to_be_bytes()[1..]);
        out.extend_from_slice(&block);
        out.extend_from_slice(&bytes[offset..]);
        std::fs::write(path, out).expect("write fixture with seektable");
    }

    fn seektable_len(path: &std::path::Path) -> Option<usize> {
        let bytes = std::fs::read(path).ok()?;
        let mut offset = 4usize;
        loop {
            let header = bytes.get(offset..offset + 4)?;
            let last = header[0] & 0x80 != 0;
            let ty = header[0] & 0x7f;
            let len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
            if ty == 3 {
                return Some(len);
            }
            offset += 4 + len;
            if last {
                return None;
            }
        }
    }

    /// lofty 0.22.4 corrupts a FLAC that has no trailing PADDING: it leaves the
    /// real last block flagged `is_last` and splices a PADDING after it, so every
    /// decoder stops there and reads the padding header as an audio frame. Every
    /// Qobuz download hit this; Deezer and Tidal did not, because their files
    /// already carried padding — which is also why the fixtures above never
    /// caught it.
    #[test]
    fn a_source_without_padding_stays_decodable_after_tagging() {
        let Some(ffmpeg) = which_ffmpeg() else {
            panic!("ffmpeg is required to build the tagging fixtures");
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nopad.flac");
        assert!(std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "0.3",
            ])
            .arg(&path)
            .status()
            .expect("spawn ffmpeg")
            .success());
        strip_trailing_padding(&path);
        append_seektable(&path);
        verify_flac_structure(&path).expect("the stripped fixture is still valid going in");
        assert_eq!(
            seektable_len(&path),
            Some(18),
            "fixture carries a seektable"
        );

        let metadata = TrackMetadata {
            title: Some("No Padding".into()),
            artist: vec!["Tester".into()],
            ..Default::default()
        };
        write_tags(&path, &metadata, None, &excluded(&[])).expect("tagging");

        verify_flac_structure(&path).expect("still decodable after tagging");
        assert_eq!(
            seektable_len(&path),
            Some(18),
            "the seektable was dropped while making room for padding"
        );
        let tagged = lofty::read_from_path(&path).expect("re-read");
        assert_eq!(
            tagged
                .primary_tag()
                .and_then(|t| t.get_string(ItemKey::TrackTitle)),
            Some("No Padding")
        );
    }

    #[test]
    fn zero_track_numbers_are_not_written() {
        let mut tag = Tag::new(TagType::VorbisComments);
        write_number(
            &mut tag,
            "tracknumber",
            ItemKey::TrackNumber,
            ItemKey::TrackTotal,
            Some(0),
            Some(12),
            &excluded(&[]),
        );
        assert_eq!(tag.get_string(ItemKey::TrackNumber), None);
        assert_eq!(tag.get_string(ItemKey::TrackTotal), Some("12"));
    }

    /// Repeating the key is how Vorbis says "two artists". Everything else takes one
    /// joined string, which is what these containers did before multi-value existed.
    #[test]
    fn multi_value_repeats_on_vorbis_and_joins_elsewhere() {
        let values = vec!["Daft Punk".to_string(), "Nile Rodgers".to_string()];

        let mut vorbis = Tag::new(TagType::VorbisComments);
        set_multi(
            &mut vorbis,
            TagType::VorbisComments,
            "artist",
            ItemKey::TrackArtist,
            &values,
            &excluded(&[]),
        );
        let got: Vec<&str> = vorbis.get_strings(ItemKey::TrackArtist).collect();
        assert_eq!(got, ["Daft Punk", "Nile Rodgers"]);

        let mut mp4 = Tag::new(TagType::Mp4Ilst);
        set_multi(
            &mut mp4,
            TagType::Mp4Ilst,
            "artist",
            ItemKey::TrackArtist,
            &values,
            &excluded(&[]),
        );
        let got: Vec<&str> = mp4.get_strings(ItemKey::TrackArtist).collect();
        assert_eq!(got, ["Daft Punk, Nile Rodgers"]);
    }

    /// An instrumental has no lyrics, and `LYRICS=` trips up parsers that expect a
    /// value. Blank must clear the key, not write an empty one.
    #[test]
    fn blank_values_clear_the_key_instead_of_writing_an_empty_one() {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.insert_text(ItemKey::Lyrics, "stale".into());
        set_text(
            &mut tag,
            "lyrics",
            ItemKey::Lyrics,
            Some(&"   ".to_string()),
            &excluded(&[]),
        );
        assert_eq!(tag.get_string(ItemKey::Lyrics), None);

        let mut tag = Tag::new(TagType::VorbisComments);
        set_multi(
            &mut tag,
            TagType::VorbisComments,
            "composer",
            ItemKey::Composer,
            &["".to_string(), "  ".to_string()],
            &excluded(&[]),
        );
        assert_eq!(tag.get_string(ItemKey::Composer), None);
    }

    /// Qobuz is the only service that publishes a purchase date, and there is no
    /// `ItemKey` for it — so it goes straight onto the comment block. The rest of the
    /// tag has to survive that detour.
    #[test]
    fn a_purchase_date_reaches_the_comment_block_without_losing_the_rest() {
        let Some(ffmpeg) = which_ffmpeg() else {
            panic!("ffmpeg is required to build the tagging fixtures");
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bought.flac");
        assert!(std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "0.2",
            ])
            .arg(&path)
            .status()
            .expect("spawn ffmpeg")
            .success());

        let metadata = TrackMetadata {
            title: Some("Get Lucky".into()),
            artist: vec!["Daft Punk".into(), "Pharrell Williams".into()],
            album: Some("Random Access Memories".into()),
            purchase_date: Some("2013-01-01".into()),
            ..Default::default()
        };
        write_tags(&path, &metadata, None, &excluded(&[])).expect("tagging");
        verify_flac_structure(&path).expect("still decodable after tagging");

        let file = lofty::flac::FlacFile::read_from(
            &mut std::fs::File::open(&path).expect("open"),
            lofty::config::ParseOptions::new(),
        )
        .expect("re-read flac");
        let comments = file.vorbis_comments().expect("vorbis comments");
        assert_eq!(comments.get("PURCHASE_DATE"), Some("2013-01-01"));
        assert_eq!(comments.get("TITLE"), Some("Get Lucky"));
        assert_eq!(comments.get("ALBUM"), Some("Random Access Memories"));
        let artists: Vec<&str> = comments.get_all("ARTIST").collect();
        assert_eq!(artists, ["Daft Punk", "Pharrell Williams"]);
    }

    /// MP4 and ID3 have no multi-value artist frame, so the joined string stays put for
    /// ordinary players while the plural side channel carries the real list.
    #[test]
    fn multi_value_artists_reach_mp4_and_id3_through_the_plural_keys() {
        let Some(ffmpeg) = which_ffmpeg() else {
            panic!("ffmpeg is required to build the tagging fixtures");
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let metadata = TrackMetadata {
            title: Some("Get Lucky".into()),
            artist: vec!["Daft Punk".into(), "Pharrell Williams".into()],
            album_artist: vec!["Daft Punk".into(), "Nile Rodgers".into()],
            ..Default::default()
        };

        for ext in ["m4a", "mp3"] {
            let path = dir.path().join(format!("multi.{ext}"));
            assert!(std::process::Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=44100:cl=stereo",
                    "-t",
                    "0.2",
                ])
                .arg(&path)
                .status()
                .expect("spawn ffmpeg")
                .success());
            write_tags(&path, &metadata, None, &excluded(&[]))
                .unwrap_or_else(|e| panic!("tagging .{ext} failed: {e}"));

            let tagged = lofty::read_from_path(&path).expect("re-read");
            let tag = tagged
                .primary_tag()
                .or_else(|| tagged.first_tag())
                .unwrap_or_else(|| panic!(".{ext} came back with no tag"));

            assert_eq!(
                tag.get_string(ItemKey::TrackArtist),
                Some("Daft Punk, Pharrell Williams"),
                ".{ext} keeps the joined value on the standard frame"
            );
            let artists: Vec<&str> = tag.get_strings(ItemKey::TrackArtists).collect();
            assert_eq!(
                artists,
                ["Daft Punk", "Pharrell Williams"],
                ".{ext} plural artists"
            );
            let album_artists: Vec<&str> = tag.get_strings(ItemKey::AlbumArtists).collect();
            assert_eq!(
                album_artists,
                ["Daft Punk", "Nile Rodgers"],
                ".{ext} plural album artists"
            );
        }
    }
}
