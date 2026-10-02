//! Attached picture metadata (cover art, artist photos, etc.).
//!
//! Used by containers that can carry embedded image data — notably MP3
//! (via `APIC` / `PIC` frames in an ID3v2 tag), FLAC (via
//! `METADATA_BLOCK_PICTURE`), MP4 (`covr` atoms), and Ogg/Vorbis (base64-
//! encoded `METADATA_BLOCK_PICTURE` inside a Vorbis comment).
//!
//! The picture type values follow the ID3v2 `APIC` spec (which FLAC and
//! Ogg reuse verbatim), so a `PictureType` round-trips cleanly between
//! all four containers.

/// A single attached picture as it appears in a file's metadata.
///
/// `data` is the raw encoded image bytes exactly as stored in the file —
/// the container does not decode the image. Callers that want pixels
/// should feed `data` through the appropriate image decoder (JPEG, PNG,
/// ...) using `mime_type` as a routing hint.
#[derive(Clone, Debug)]
pub struct AttachedPicture {
    /// MIME type of the image payload (`"image/jpeg"`, `"image/png"`,
    /// ...). The special value `"-->"` means `data` is a URL string
    /// pointing to an external image rather than inline bytes (ID3v2).
    pub mime_type: String,
    /// Semantic role of the picture (cover art, artist photo, ...).
    pub picture_type: PictureType,
    /// Human-readable description supplied by the tagger. Often empty.
    pub description: String,
    /// Raw image bytes (or URL bytes when `mime_type == "-->"`).
    pub data: Vec<u8>,
}

impl AttachedPicture {
    /// Construct a new `AttachedPicture` with the given MIME type and
    /// picture role. `description` defaults to empty and `data` to an
    /// empty `Vec`; chain [`with_description`](Self::with_description)
    /// and [`with_data`](Self::with_data) to fill them in.
    ///
    /// Convenience over the public-field struct literal for producers
    /// (ID3v2 / FLAC / MP4 / Vorbis) that build the picture
    /// incrementally as bytes scroll past the parser.
    pub fn new(mime_type: impl Into<String>, picture_type: PictureType) -> Self {
        Self {
            mime_type: mime_type.into(),
            picture_type,
            description: String::new(),
            data: Vec::new(),
        }
    }

    /// Chainable setter for the human-readable description field.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Chainable setter for the raw image bytes (or URL bytes when
    /// `mime_type == "-->"`).
    pub fn with_data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    /// Replace the picture-type field. Builder counterpart to the
    /// public `picture_type` field for callers that initialize with a
    /// placeholder role and refine it once the parse position settles.
    pub fn with_picture_type(mut self, picture_type: PictureType) -> Self {
        self.picture_type = picture_type;
        self
    }

    /// `true` when `mime_type` carries the ID3v2 sentinel `"-->"`,
    /// indicating that `data` is a URL string pointing to an external
    /// image rather than inline bytes. Pure sugar — equivalent to
    /// `self.mime_type == "-->"` — but expressing the intent at call
    /// sites that branch on link-vs-inline semantics.
    pub fn is_external_link(&self) -> bool {
        self.mime_type == "-->"
    }
}

/// ID3v2 `APIC` picture-type taxonomy (also reused by FLAC and Vorbis).
///
/// The numeric values match the ID3v2 specification and are stable:
/// callers are free to cast a `PictureType` to `u8`. New values added to
/// future revisions of the spec will land as `Unknown` rather than
/// breaking existing code — the enum is `#[non_exhaustive]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[repr(u8)]
pub enum PictureType {
    /// Picture with no more specific role.
    Other = 0x00,
    /// 32×32 pixel file icon (PNG only, per the ID3v2 spec).
    FileIcon32x32 = 0x01,
    /// Other file icon (no size restriction).
    FileIcon = 0x02,
    /// Front cover of the release.
    FrontCover = 0x03,
    /// Back cover of the release.
    BackCover = 0x04,
    /// Leaflet page from the release packaging.
    LeafletPage = 0x05,
    /// Picture of the physical media itself (e.g. disc label).
    Media = 0x06,
    /// Lead artist / lead performer / soloist.
    LeadArtist = 0x07,
    /// Artist or performer.
    Artist = 0x08,
    /// Conductor.
    Conductor = 0x09,
    /// Band or orchestra.
    BandOrchestra = 0x0A,
    /// Composer.
    Composer = 0x0B,
    /// Lyricist or text writer.
    Lyricist = 0x0C,
    /// Recording location.
    RecordingLocation = 0x0D,
    /// Photo taken during the recording.
    DuringRecording = 0x0E,
    /// Photo taken during a performance.
    DuringPerformance = 0x0F,
    /// Movie or video screen capture.
    MovieScreenCapture = 0x10,
    /// "A bright coloured fish" — verbatim spec-assigned role.
    ABrightColouredFish = 0x11,
    /// Illustration.
    Illustration = 0x12,
    /// Band or artist logotype.
    BandLogo = 0x13,
    /// Publisher or studio logotype.
    PublisherLogo = 0x14,
    /// Catch-all for unrecognised or out-of-range codes.
    Unknown = 0xFF,
}

impl PictureType {
    /// Convert a raw ID3v2/FLAC picture-type byte into a `PictureType`.
    /// Unknown or reserved codes (> 0x14) collapse to `Unknown`.
    pub fn from_u8(b: u8) -> Self {
        match b {
            0x00 => Self::Other,
            0x01 => Self::FileIcon32x32,
            0x02 => Self::FileIcon,
            0x03 => Self::FrontCover,
            0x04 => Self::BackCover,
            0x05 => Self::LeafletPage,
            0x06 => Self::Media,
            0x07 => Self::LeadArtist,
            0x08 => Self::Artist,
            0x09 => Self::Conductor,
            0x0A => Self::BandOrchestra,
            0x0B => Self::Composer,
            0x0C => Self::Lyricist,
            0x0D => Self::RecordingLocation,
            0x0E => Self::DuringRecording,
            0x0F => Self::DuringPerformance,
            0x10 => Self::MovieScreenCapture,
            0x11 => Self::ABrightColouredFish,
            0x12 => Self::Illustration,
            0x13 => Self::BandLogo,
            0x14 => Self::PublisherLogo,
            _ => Self::Unknown,
        }
    }

    /// Convert this `PictureType` back into its raw ID3v2/FLAC byte —
    /// the structural inverse of [`from_u8`](Self::from_u8). Because the
    /// enum is `#[repr(u8)]` and every variant has a stable
    /// discriminant matching the spec, this is equivalent to a `self as
    /// u8` cast; the named method documents the round-trip contract and
    /// gives consumer crates (ID3 writers, FLAC tag emitters) a
    /// reviewable call site that doesn't paper over the
    /// [`Unknown`](Self::Unknown) caveat.
    ///
    /// Round-tripping `Unknown` re-emits the sentinel `0xFF` value,
    /// which is itself outside the assigned ID3v2 picture-type range
    /// (max assigned is `0x14`). Callers writing strict output should
    /// gate on [`is_known`](Self::is_known) and decide on a fallback
    /// (skip the frame, substitute `Other`) before serialising — the
    /// raw `0xFF` round-trip stays bit-stable but reserved-byte-aware
    /// muxers will refuse it.
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// `true` when this picture type came from a spec-assigned code
    /// (`Other` through `PublisherLogo`); `false` for
    /// [`Unknown`](Self::Unknown) — the sentinel produced when
    /// [`from_u8`](Self::from_u8) sees a reserved or future-spec byte.
    ///
    /// Consumer-side gate before emitting a strict ID3v2 / FLAC byte:
    /// an `Unknown` round-trips as `0xFF`, which is itself outside the
    /// assigned range and will be rejected by strict parsers.
    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_type_known_values() {
        assert_eq!(PictureType::from_u8(0x03), PictureType::FrontCover);
        assert_eq!(PictureType::from_u8(0x07), PictureType::LeadArtist);
        assert_eq!(PictureType::from_u8(0x14), PictureType::PublisherLogo);
    }

    #[test]
    fn picture_type_unknown_collapses() {
        assert_eq!(PictureType::from_u8(0x15), PictureType::Unknown);
        assert_eq!(PictureType::from_u8(0xAB), PictureType::Unknown);
    }

    #[test]
    fn to_u8_inverts_from_u8_on_every_assigned_code() {
        // Spec-assigned range 0x00..=0x14 round-trips byte-for-byte
        // through from_u8 → to_u8.
        for b in 0x00u8..=0x14 {
            assert_eq!(PictureType::from_u8(b).to_u8(), b);
        }
    }

    #[test]
    fn to_u8_unknown_emits_sentinel_byte() {
        // Unknown variant deliberately carries discriminant 0xFF so
        // to_u8 stays a pure `as u8` cast. Reserved-byte audit: 0xFF
        // is outside the assigned 0x00..=0x14 range.
        assert_eq!(PictureType::Unknown.to_u8(), 0xFF);
        // A byte that collapses to Unknown does NOT round-trip
        // structurally — the original code is lost to the catch-all,
        // which is what is_known() exists to flag.
        let collapsed = PictureType::from_u8(0xAB);
        assert_eq!(collapsed, PictureType::Unknown);
        assert_eq!(collapsed.to_u8(), 0xFF);
        assert_ne!(collapsed.to_u8(), 0xAB);
    }

    #[test]
    fn is_known_separates_assigned_from_sentinel() {
        assert!(PictureType::Other.is_known());
        assert!(PictureType::FrontCover.is_known());
        assert!(PictureType::PublisherLogo.is_known());
        assert!(!PictureType::Unknown.is_known());
    }

    #[test]
    fn attached_picture_new_defaults_then_builders_fill() {
        let p = AttachedPicture::new("image/png", PictureType::FrontCover);
        assert_eq!(p.mime_type, "image/png");
        assert_eq!(p.picture_type, PictureType::FrontCover);
        assert!(p.description.is_empty());
        assert!(p.data.is_empty());
        assert!(!p.is_external_link());

        let filled = p
            .with_description("Album art")
            .with_data(vec![0x89, b'P', b'N', b'G']);
        assert_eq!(filled.description, "Album art");
        assert_eq!(filled.data, vec![0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn attached_picture_with_picture_type_replaces() {
        let p = AttachedPicture::new("image/jpeg", PictureType::Other)
            .with_picture_type(PictureType::BackCover);
        assert_eq!(p.picture_type, PictureType::BackCover);
    }

    #[test]
    fn attached_picture_external_link_detected_by_sentinel_mime() {
        // ID3v2 "-->" sentinel means data is a URL, not inline bytes.
        let p = AttachedPicture::new("-->", PictureType::FrontCover)
            .with_data(b"https://example.invalid/cover.jpg".to_vec());
        assert!(p.is_external_link());

        let inline = AttachedPicture::new("image/jpeg", PictureType::FrontCover);
        assert!(!inline.is_external_link());
    }
}
