//! Structured container metadata.
//!
//! Today most demuxers expose chapters and attachments as flat
//! [`Demuxer::metadata`](crate::Demuxer::metadata) entries — strings
//! like `chapter:0:start_ms` / `attachment:2:filename`. Those keep
//! working, but consumers that want to iterate chapters or pull an
//! attachment's payload should use the structured
//! [`Demuxer::chapters`](crate::Demuxer::chapters) and
//! [`Demuxer::attachments`](crate::Demuxer::attachments) accessors
//! instead. Both default to an empty slice on the trait, so demuxers
//! that don't carry such data — and demuxers that haven't been ported
//! to the structured API yet — keep compiling unchanged.
//!
//! The two structs are deliberately container-agnostic. They cover the
//! intersection of MKV (`Chapters` / `Attachments` master elements),
//! MP4 (chapter track + `iTunSMPB`-style chapter atoms; `meta`/`covr`
//! adjacent for attachments), Ogg (`CHAPTERnn=…` Vorbis comments),
//! and DVD/Blu-ray IFO chapter tables.

use crate::time::Timestamp;

/// One chapter / cue point inside a container.
///
/// Containers that only carry a start time (Vorbis-comment chapters,
/// DVD IFO PGCs without explicit end times) set `end == start`. The
/// `id` field is whatever the container uses internally — MKV's
/// `ChapterUID`, MP4 chapter track sample index, or a synthesised
/// counter for formats without a stable ID.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    /// Container-native chapter identifier. Stable across demuxer
    /// re-opens of the same file but **not** comparable across
    /// different containers.
    pub id: u64,
    /// Chapter start time. The [`Timestamp`]'s time base is whatever
    /// the demuxer reports; consumers should
    /// [`rescale`](Timestamp::rescale) to a common base before
    /// comparing chapters from different sources.
    pub start: Timestamp,
    /// Chapter end time. Equal to `start` when the container does not
    /// store an explicit end (the next chapter's start is the
    /// implicit end in that case).
    pub end: Timestamp,
    /// Display title in the chapter's primary language, if present.
    pub title: Option<String>,
    /// BCP-47 / ISO 639 language tag for the title (`"en"`, `"jpn"`,
    /// …) when the container labels it. `None` means "unspecified" —
    /// not "neutral".
    pub language: Option<String>,
}

/// One file-shaped payload attached to a container.
///
/// Distinct from [`AttachedPicture`](crate::AttachedPicture): an
/// `Attachment` is an arbitrary byte blob with a filename — fonts
/// (MKV `application/x-truetype-font`), thumbnail strips, subtitle
/// fragments, README text — whereas `AttachedPicture` is the
/// ID3v2/FLAC/MP4 cover-art pathway that carries a typed
/// [`PictureType`](crate::PictureType). MKV `Attachments` map
/// cleanly onto this struct; MP4 `meta` boxes and Ogg
/// `METADATA_BLOCK_PICTURE` are emitted as
/// [`attached_pictures`](crate::Demuxer::attached_pictures) instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// Original filename as stored in the container (no path
    /// stripping, no normalisation). Containers that don't track a
    /// name still populate this — synthesise something stable like
    /// `attachment_<id>.bin` so callers always have a routing handle.
    pub name: String,
    /// IANA media type (`"image/png"`, `"application/x-truetype-font"`,
    /// …) when the container declares one. `None` means "unspecified"
    /// — callers are free to sniff the bytes.
    pub mime: Option<String>,
    /// Free-form description supplied by the tagger.
    pub description: Option<String>,
    /// Raw attachment bytes exactly as stored in the container.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::TimeBase;

    #[test]
    fn chapter_clone_eq_round_trip() {
        let base = TimeBase::new(1, 1000);
        let c = Chapter {
            id: 42,
            start: Timestamp::new(0, base),
            end: Timestamp::new(5_000, base),
            title: Some("Intro".into()),
            language: Some("en".into()),
        };
        let c2 = c.clone();
        assert_eq!(c, c2);
        assert_eq!(c.id, 42);
        assert_eq!(c.start.value, 0);
        assert_eq!(c.end.value, 5_000);
        assert_eq!(c.title.as_deref(), Some("Intro"));
        assert_eq!(c.language.as_deref(), Some("en"));
    }

    #[test]
    fn chapter_optional_fields_default_to_none() {
        let base = TimeBase::new(1, 1);
        let c = Chapter {
            id: 1,
            start: Timestamp::new(0, base),
            end: Timestamp::new(0, base),
            title: None,
            language: None,
        };
        assert!(c.title.is_none());
        assert!(c.language.is_none());
        // Containers without an explicit end time set end == start.
        assert_eq!(c.start, c.end);
    }

    #[test]
    fn attachment_clone_eq_round_trip() {
        let a = Attachment {
            name: "cover.png".into(),
            mime: Some("image/png".into()),
            description: Some("Album front".into()),
            data: vec![0x89, b'P', b'N', b'G'],
        };
        let a2 = a.clone();
        assert_eq!(a, a2);
        assert_eq!(a.name, "cover.png");
        assert_eq!(a.mime.as_deref(), Some("image/png"));
        assert_eq!(a.description.as_deref(), Some("Album front"));
        assert_eq!(a.data, vec![0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn attachment_optional_fields_default_to_none() {
        let a = Attachment {
            name: "blob.bin".into(),
            mime: None,
            description: None,
            data: Vec::new(),
        };
        assert!(a.mime.is_none());
        assert!(a.description.is_none());
        assert!(a.data.is_empty());
    }
}
