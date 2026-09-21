//! Media-type and sample/pixel format enumerations.
//!
//! Audio channel ordering follows SMPTE 2036-2 / ITU-R BS.775 conventions
//! for surround layouts; per-channel positions are named with the
//! WAVEFORMATEXTENSIBLE "front-left, front-right, …" vocabulary.

/// Broad category of a stream's payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaType {
    /// Audio samples.
    Audio,
    /// Video pictures.
    Video,
    /// Timed-text / bitmap subtitle cues.
    Subtitle,
    /// Opaque non-media payload (timecodes, klv, chapters, …).
    Data,
    /// Category not (yet) determined.
    Unknown,
}

/// A single speaker position within a multi-channel audio layout.
///
/// Names follow the WAVEFORMATEXTENSIBLE / SMPTE convention.
/// `Side*` and `Back*` are kept distinct (mirroring 7.1's
/// L/R + Ls/Rs + Lb/Rb separation) so codecs that surface the
/// distinction don't collapse it. `Lr`/`Rr` (rear / back-rear) are aliases
/// for `BackLeft`/`BackRight` in this taxonomy — the rear pair sits behind
/// the listener on the room's centreline-extension, the side pair is at
/// roughly ±90° from front. The enum is `#[non_exhaustive]` so additional
/// positions (height channels for Atmos / Auro-3D, etc.) can be added
/// without breaking downstream match arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChannelPosition {
    /// Front-left (L). 30° left of centre in BS.775 listening geometry.
    FrontLeft,
    /// Front-right (R). 30° right of centre.
    FrontRight,
    /// Front-centre (C). Direct centre, 0°.
    FrontCenter,
    /// Low-frequency effects (LFE). Sub-bass, no positional meaning.
    LowFrequency,
    /// Back-left (Lb / Lr). Behind the listener, ±150° in 7.1.
    BackLeft,
    /// Back-right (Rb / Rr). Behind the listener, mirror of `BackLeft`.
    BackRight,
    /// Front left-of-centre (Lc). Used in cinema 7.1 SDDS layouts.
    FrontLeftOfCenter,
    /// Front right-of-centre (Rc). Mirror of `FrontLeftOfCenter`.
    FrontRightOfCenter,
    /// Back-centre (Cs). Single rear channel for 6.1 / BS.775 4.0.
    BackCenter,
    /// Side-left (Ls). ±90° on the listener's left in 5.1 / 7.1.
    SideLeft,
    /// Side-right (Rs). Mirror of `SideLeft`.
    SideRight,
    /// Top front-left. Atmos / Auro-3D height layer (placeholder).
    TopFrontLeft,
    /// Top front-right. Atmos / Auro-3D height layer (placeholder).
    TopFrontRight,
    /// Top back-left. Atmos / Auro-3D ceiling layer (placeholder).
    TopBackLeft,
    /// Top back-right. Atmos / Auro-3D ceiling layer (placeholder).
    TopBackRight,
}

/// Audio channel layout — names a fixed ordered tuple of speaker
/// positions, OR carries a discrete fallback count when the layout is
/// unknown / non-standard.
///
/// Channel orderings are taken from ITU-R BS.775 (5.1 / 7.1 surround
/// reference) and SMPTE ST 2036-2 (audio channel ordering for UHDTV).
/// For 5.1 the canonical order this crate adopts is
/// `L, R, C, LFE, Ls, Rs` (the WAVEFORMATEXTENSIBLE / Vorbis / Opus
/// convention). 7.1 extends that with `Lb, Rb` (back-rear pair).
///
/// The `Stereo` variant covers both regular two-channel stereo and the
/// AC-3 / AC-4 matrix-encoded downmix carriers `Lo/Ro` ("two of",
/// downmix-compatible) and `Lt/Rt` ("matrix-encoded for Pro Logic
/// extraction"); the dedicated [`LoRo`](ChannelLayout::LoRo) /
/// [`LtRt`](ChannelLayout::LtRt) variants surface the distinction
/// explicitly when a downstream filter or muxer needs it.
///
/// `DiscreteN(n)` is the catch-all for "we know there are `n` channels
/// but no recognised layout" — used when a codec produces an unusual
/// channel count (>8) or when the container failed to surface a layout
/// flag. It is the only variant whose `position()` returns `None`.
///
/// Marked `#[non_exhaustive]` so additional standard layouts (Atmos
/// 7.1.4, Auro-3D 9.1, …) can be added without breaking match-exhaustive
/// downstream consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChannelLayout {
    /// Mono (1ch): C.
    Mono,
    /// Stereo (2ch): L, R.
    Stereo,
    /// 2.1 (3ch): L, R, LFE.
    Stereo21,
    /// 3.0 surround (3ch): L, R, C.
    Surround30,
    /// Quadraphonic (4ch): L, R, Ls, Rs — no centre, side surrounds.
    Quad,
    /// 4.0 surround per BS.775 (4ch): L, R, C, Cs — centre + back surround.
    Surround40,
    /// 4.1 surround (5ch): L, R, C, Cs, LFE.
    Surround41,
    /// 5.0 surround (5ch): L, R, C, Ls, Rs.
    Surround50,
    /// 5.1 surround (6ch): L, R, C, LFE, Ls, Rs.
    Surround51,
    /// 6.0 surround (6ch): L, R, C, Cs, Ls, Rs.
    Surround60,
    /// 6.1 surround (7ch): L, R, C, LFE, Cs, Ls, Rs.
    Surround61,
    /// 7.0 surround (7ch): L, R, C, Ls, Rs, Lb, Rb.
    Surround70,
    /// 7.1 surround (8ch): L, R, C, LFE, Ls, Rs, Lb, Rb.
    Surround71,
    /// AC-3 / AC-4 Lo/Ro stereo downmix (2ch). Two-channel mix preserving
    /// downmix-compatibility coefficients; not matrix-encoded.
    LoRo,
    /// AC-3 / AC-4 Lt/Rt stereo downmix (2ch). Two-channel matrix-encoded
    /// downmix carrying surround information for Dolby Pro Logic decoding.
    LtRt,
    /// Discrete fallback: `n` channels with no recognised layout. Used for
    /// unusual / >8ch / unknown layouts surfaced by exotic codecs or
    /// containers that drop layout flags.
    DiscreteN(u16),
}

impl ChannelLayout {
    /// Number of channels in this layout.
    pub fn channel_count(&self) -> u16 {
        match self {
            Self::Mono => 1,
            Self::Stereo | Self::LoRo | Self::LtRt => 2,
            Self::Stereo21 | Self::Surround30 => 3,
            Self::Quad | Self::Surround40 => 4,
            Self::Surround41 | Self::Surround50 => 5,
            Self::Surround51 | Self::Surround60 => 6,
            Self::Surround61 | Self::Surround70 => 7,
            Self::Surround71 => 8,
            Self::DiscreteN(n) => *n,
        }
    }

    /// Speaker positions in canonical order. Returns an empty slice for
    /// `DiscreteN` since the layout is unknown — call [`positions_owned`]
    /// to get a `Vec` if you need to enumerate slots regardless of
    /// known/unknown status.
    ///
    /// [`positions_owned`]: Self::positions_owned
    pub fn positions(&self) -> &'static [ChannelPosition] {
        use ChannelPosition::*;
        match self {
            Self::Mono => &[FrontCenter],
            Self::Stereo | Self::LoRo | Self::LtRt => &[FrontLeft, FrontRight],
            Self::Stereo21 => &[FrontLeft, FrontRight, LowFrequency],
            Self::Surround30 => &[FrontLeft, FrontRight, FrontCenter],
            Self::Quad => &[FrontLeft, FrontRight, SideLeft, SideRight],
            Self::Surround40 => &[FrontLeft, FrontRight, FrontCenter, BackCenter],
            Self::Surround41 => &[FrontLeft, FrontRight, FrontCenter, BackCenter, LowFrequency],
            Self::Surround50 => &[FrontLeft, FrontRight, FrontCenter, SideLeft, SideRight],
            Self::Surround51 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                LowFrequency,
                SideLeft,
                SideRight,
            ],
            Self::Surround60 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                BackCenter,
                SideLeft,
                SideRight,
            ],
            Self::Surround61 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                LowFrequency,
                BackCenter,
                SideLeft,
                SideRight,
            ],
            Self::Surround70 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                SideLeft,
                SideRight,
                BackLeft,
                BackRight,
            ],
            Self::Surround71 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                LowFrequency,
                SideLeft,
                SideRight,
                BackLeft,
                BackRight,
            ],
            Self::DiscreteN(_) => &[],
        }
    }

    /// Owned position list. For known layouts this clones [`positions`];
    /// for `DiscreteN(n)` it returns an empty `Vec` (positions remain
    /// unknown). Provided so callers that just want "give me positions
    /// for any layout" don't have to special-case the discrete arm.
    ///
    /// [`positions`]: Self::positions
    pub fn positions_owned(&self) -> Vec<ChannelPosition> {
        self.positions().to_vec()
    }

    /// Speaker position at slot `idx` in canonical order, or `None` for
    /// out-of-range slots and for `DiscreteN` (where the layout is
    /// unknown).
    pub fn position(&self, idx: usize) -> Option<ChannelPosition> {
        self.positions().get(idx).copied()
    }

    /// True when this layout carries a low-frequency-effects (LFE) channel.
    pub fn has_lfe(&self) -> bool {
        self.positions()
            .iter()
            .any(|p| matches!(p, ChannelPosition::LowFrequency))
    }

    /// True when this layout carries surround information (more than two
    /// channels OR an LFE). `Stereo` / `Mono` return false; `LoRo` /
    /// `LtRt` are 2-channel downmixes and also return false even though
    /// they encode surround content (that's the whole point of a
    /// downmix).
    pub fn is_surround(&self) -> bool {
        self.channel_count() > 2 || self.has_lfe()
    }

    /// Back-compat bridge: infer a layout from a bare channel count.
    ///
    /// This mapping is what lets codecs that haven't been updated to set
    /// a layout explicitly continue to work: they keep producing a count
    /// and we infer the most-common layout for that count. The choices
    /// follow industry defaults — 5.1 wins for 6ch (more common than
    /// 6.0), 7.1 wins for 8ch, and so on.
    ///
    /// | count | layout       |
    /// |-------|--------------|
    /// | 1     | `Mono`       |
    /// | 2     | `Stereo`     |
    /// | 3     | `Surround30` |
    /// | 4     | `Quad`       |
    /// | 5     | `Surround50` |
    /// | 6     | `Surround51` |
    /// | 7     | `Surround61` |
    /// | 8     | `Surround71` |
    /// | other | `DiscreteN`  |
    pub fn from_count(n: u16) -> ChannelLayout {
        match n {
            1 => Self::Mono,
            2 => Self::Stereo,
            3 => Self::Surround30,
            4 => Self::Quad,
            5 => Self::Surround50,
            6 => Self::Surround51,
            7 => Self::Surround61,
            8 => Self::Surround71,
            other => Self::DiscreteN(other),
        }
    }
}

impl std::fmt::Display for ChannelLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Mono => "mono",
            Self::Stereo => "stereo",
            Self::Stereo21 => "2.1",
            Self::Surround30 => "3.0",
            Self::Quad => "quad",
            Self::Surround40 => "4.0",
            Self::Surround41 => "4.1",
            Self::Surround50 => "5.0",
            Self::Surround51 => "5.1",
            Self::Surround60 => "6.0",
            Self::Surround61 => "6.1",
            Self::Surround70 => "7.0",
            Self::Surround71 => "7.1",
            Self::LoRo => "loro",
            Self::LtRt => "ltrt",
            Self::DiscreteN(n) => return write!(f, "discrete{n}"),
        };
        f.write_str(s)
    }
}

/// Error returned by the [`ChannelLayout`] `FromStr` impl when the input
/// doesn't match any recognised layout name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseChannelLayoutError(pub String);

impl std::fmt::Display for ParseChannelLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unrecognised channel layout: {:?}", self.0)
    }
}

impl std::error::Error for ParseChannelLayoutError {}

impl std::str::FromStr for ChannelLayout {
    type Err = ParseChannelLayoutError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.trim().to_ascii_lowercase();
        let layout = match lower.as_str() {
            "mono" | "1.0" => Self::Mono,
            "stereo" | "2.0" => Self::Stereo,
            "2.1" => Self::Stereo21,
            "3.0" | "surround3" | "surround30" => Self::Surround30,
            "quad" => Self::Quad,
            "4.0" | "surround4" | "surround40" => Self::Surround40,
            "4.1" | "surround41" => Self::Surround41,
            "5.0" | "surround5" | "surround50" => Self::Surround50,
            "5.1" | "surround51" => Self::Surround51,
            "6.0" | "surround6" | "surround60" => Self::Surround60,
            "6.1" | "surround61" => Self::Surround61,
            "7.0" | "surround7" | "surround70" => Self::Surround70,
            "7.1" | "surround71" => Self::Surround71,
            "loro" | "lo/ro" => Self::LoRo,
            "ltrt" | "lt/rt" => Self::LtRt,
            other => {
                if let Some(rest) = other.strip_prefix("discrete") {
                    if let Ok(n) = rest.parse::<u16>() {
                        return Ok(Self::DiscreteN(n));
                    }
                }
                return Err(ParseChannelLayoutError(s.to_owned()));
            }
        };
        Ok(layout)
    }
}

/// Audio sample format.
///
/// Variants carry **stable explicit discriminants** — the integer value
/// of `SampleFormat::S16 as u8` is part of the public ABI. Add new
/// variants only at the end with a fresh number; never reorder, renumber,
/// or remove. `#[non_exhaustive]` lets the enum grow without breaking
/// downstream `match` statements; pinned discriminants additionally let
/// the format round-trip through any byte-stable serialization
/// (config files, capability blobs, IPC) without losing meaning across
/// crate versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u8)]
pub enum SampleFormat {
    /// Unsigned 8-bit, interleaved.
    U8 = 0,
    /// Signed 8-bit, interleaved. Native format of Amiga 8SVX and MOD samples.
    S8 = 1,
    /// Signed 16-bit little-endian, interleaved.
    S16 = 2,
    /// Signed 24-bit packed (3 bytes/sample) little-endian, interleaved.
    S24 = 3,
    /// Signed 32-bit little-endian, interleaved.
    S32 = 4,
    /// 32-bit IEEE float, interleaved.
    F32 = 5,
    /// 64-bit IEEE float, interleaved.
    F64 = 6,
    /// Unsigned 8-bit, planar (one plane per channel).
    U8P = 7,
    /// Signed 16-bit little-endian, planar (one plane per channel).
    S16P = 8,
    /// Signed 32-bit little-endian, planar (one plane per channel).
    S32P = 9,
    /// 32-bit IEEE float, planar (one plane per channel).
    F32P = 10,
    /// 64-bit IEEE float, planar (one plane per channel).
    F64P = 11,
}

impl SampleFormat {
    /// `true` for the planar (one-plane-per-channel) variants.
    pub fn is_planar(&self) -> bool {
        matches!(
            self,
            Self::U8P | Self::S16P | Self::S32P | Self::F32P | Self::F64P
        )
    }

    /// Bytes per sample *per channel*.
    pub fn bytes_per_sample(&self) -> usize {
        match self {
            Self::U8 | Self::U8P | Self::S8 => 1,
            Self::S16 | Self::S16P => 2,
            Self::S24 => 3,
            Self::S32 | Self::S32P | Self::F32 | Self::F32P => 4,
            Self::F64 | Self::F64P => 8,
        }
    }

    /// `true` for the IEEE-float variants (32- or 64-bit, either layout).
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64 | Self::F32P | Self::F64P)
    }

    /// Number of `Vec<u8>` planes an [`AudioFrame`](crate::AudioFrame)
    /// of this format carries for `channels` channels: planar formats
    /// use one plane per channel, interleaved formats use one plane
    /// total.
    pub fn plane_count(&self, channels: u16) -> usize {
        if self.is_planar() {
            channels as usize
        } else {
            1
        }
    }
}

/// Video pixel format.
///
/// Variants carry **stable explicit discriminants** — the integer value
/// of `PixelFormat::Yuv420P as u16` is part of the public ABI. Add new
/// variants only at the end with a fresh number; never reorder, renumber,
/// or remove. `#[non_exhaustive]` lets the enum grow without breaking
/// downstream `match` statements; pinned discriminants additionally let
/// the format round-trip through any byte-stable serialization
/// (config files, capability blobs, IPC, on-disk caches) without losing
/// meaning across crate versions, and prevent inserts in the middle of
/// the enum from shifting every later variant's number (which
/// cargo-semver-checks rightly flags as a breaking change).
///
/// The first six variants (`Yuv420P` through `Gray8`) are the original
/// formats produced by the early codec crates. Everything beyond that
/// is additional surface handled by `oxideav-pixfmt` and the still-image
/// codecs (PNG, GIF, still-JPEG).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(u16)]
pub enum PixelFormat {
    /// 8-bit YUV 4:2:0, planar (Y, U, V).
    Yuv420P = 0,
    /// 8-bit YUV 4:2:2, planar.
    Yuv422P = 1,
    /// 8-bit YUV 4:4:4, planar.
    Yuv444P = 2,
    /// Packed 8-bit RGB, 3 bytes/pixel.
    Rgb24 = 3,
    /// Packed 8-bit RGBA, 4 bytes/pixel.
    Rgba = 4,
    /// Packed 8-bit grayscale.
    Gray8 = 5,

    // --- Palette ---
    /// 8-bit palette indices — companion palette carried out of band.
    Pal8 = 6,

    // --- Packed RGB/BGR swizzles ---
    /// Packed 8-bit BGR, 3 bytes/pixel.
    Bgr24 = 7,
    /// Packed 8-bit BGRA, 4 bytes/pixel.
    Bgra = 8,
    /// Packed 8-bit ARGB, 4 bytes/pixel (alpha first).
    Argb = 9,
    /// Packed 8-bit ABGR, 4 bytes/pixel.
    Abgr = 10,

    // --- Deeper packed RGB ---
    /// Packed 16-bit-per-channel RGB, little-endian, 6 bytes/pixel.
    Rgb48Le = 11,
    /// Packed 16-bit-per-channel RGBA, little-endian, 8 bytes/pixel.
    Rgba64Le = 12,

    // --- Grayscale deeper / partial bit depths ---
    /// 16-bit little-endian grayscale.
    Gray16Le = 13,
    /// 10-bit grayscale in a 16-bit little-endian word.
    Gray10Le = 14,
    /// 12-bit grayscale in a 16-bit little-endian word.
    Gray12Le = 15,

    // --- Higher-precision YUV ---
    /// 10-bit YUV 4:2:0 planar, little-endian 16-bit storage.
    Yuv420P10Le = 16,
    /// 10-bit YUV 4:2:2 planar, little-endian 16-bit storage.
    Yuv422P10Le = 17,
    /// 10-bit YUV 4:4:4 planar, little-endian 16-bit storage.
    Yuv444P10Le = 18,
    /// 12-bit YUV 4:2:0 planar, little-endian 16-bit storage.
    Yuv420P12Le = 19,
    /// 12-bit YUV 4:2:2 planar, little-endian 16-bit storage.
    Yuv422P12Le = 20,
    /// 12-bit YUV 4:4:4 planar, little-endian 16-bit storage.
    Yuv444P12Le = 21,

    // --- Full-range ("J") YUV ---
    /// JPEG/full-range YUV 4:2:0 planar.
    YuvJ420P = 22,
    /// JPEG/full-range YUV 4:2:2 planar.
    YuvJ422P = 23,
    /// JPEG/full-range YUV 4:4:4 planar.
    YuvJ444P = 24,

    // --- Semi-planar YUV ---
    /// YUV 4:2:0, planar Y + interleaved UV (NV12).
    Nv12 = 25,
    /// YUV 4:2:0, planar Y + interleaved VU (NV21).
    Nv21 = 26,

    // --- Gray + alpha / YUV + alpha ---
    /// Packed grayscale + alpha, 2 bytes/pixel (Y, A).
    Ya8 = 27,
    /// Yuv420P with an additional full-resolution alpha plane.
    Yuva420P = 28,

    // --- Mono (1 bit per pixel) ---
    /// 1 bit per pixel, packed MSB-first, 0 = black.
    MonoBlack = 29,
    /// 1 bit per pixel, packed MSB-first, 0 = white.
    MonoWhite = 30,

    // --- Interleaved YUV 4:2:2 ---
    /// Packed 4:2:2, byte order Y0 U0 Y1 V0.
    Yuyv422 = 31,
    /// Packed 4:2:2, byte order U0 Y0 V0 Y1.
    Uyvy422 = 32,

    // --- Print / prepress ---
    /// Packed 8-bit CMYK, 4 bytes/pixel in byte order C, M, Y, K.
    /// "Regular" convention: C=0 means no cyan ink (white), C=255 means
    /// full cyan. Used by JPEG 4-component scans from non-Adobe encoders
    /// and by many print-side image toolchains. Adobe Photoshop's
    /// inverted CMYK (where 0 = full ink) is the separate
    /// [`CmykInverted`](Self::CmykInverted) variant.
    Cmyk = 33,

    // --- Wide-horizontal subsampled YUV ---
    /// 8-bit YUV 4:1:1, planar (Y, U, V). Luma at full resolution; chroma
    /// horizontally subsampled by 4 (each chroma sample covers a 4×1
    /// luma block), no vertical subsampling. Native sampling of
    /// NTSC DV-25 and a legal JPEG sampling layout (luma H=4, V=1;
    /// chroma H=V=1) emitted by some real-world JPEG corpora.
    Yuv411P = 34,

    // --- Planar GBR / GBRA (RGB stored as planes in G,B,R order) ---
    //
    // High-bit-depth GBR(A) layouts used by MagicYUV, JPEG 2000, OpenEXR,
    // TIFF and similar workflows that need lossless RGB at 10/12/14 bits
    // per channel. Planes are ordered G, B, R (and A for the `Gbrap*`
    // variants) — and
    // each sample is stored as a 16-bit little-endian word with the
    // top bits zero. The native 8-bit ([`Gbrp8`](Self::Gbrp8)) and
    // full-width 16-bit ([`Gbrp16Le`](Self::Gbrp16Le) /
    // [`Gbrap16Le`](Self::Gbrap16Le)) companions arrived later and
    // therefore live at fresh appended discriminants (52-54), per the
    // append-only rule.
    /// 10-bit planar GBR, little-endian 16-bit storage. 3 planes ordered
    /// G, B, R; each sample uses the low 10 bits of a 16-bit word.
    Gbrp10Le = 35,
    /// 10-bit planar GBR + alpha, little-endian 16-bit storage. 4 planes
    /// ordered G, B, R, A; each sample uses the low 10 bits of a 16-bit
    /// word.
    Gbrap10Le = 36,
    /// 12-bit planar GBR, little-endian 16-bit storage. 3 planes ordered
    /// G, B, R; each sample uses the low 12 bits of a 16-bit word.
    Gbrp12Le = 37,
    /// 12-bit planar GBR + alpha, little-endian 16-bit storage. 4 planes
    /// ordered G, B, R, A; each sample uses the low 12 bits of a 16-bit
    /// word.
    Gbrap12Le = 38,
    /// 14-bit planar GBR, little-endian 16-bit storage. 3 planes ordered
    /// G, B, R; each sample uses the low 14 bits of a 16-bit word.
    Gbrp14Le = 39,
    /// 14-bit planar GBR + alpha, little-endian 16-bit storage. 4 planes
    /// ordered G, B, R, A; each sample uses the low 14 bits of a 16-bit
    /// word.
    Gbrap14Le = 40,

    // --- 16-bit YUV planar ---
    //
    // Full-width companions to the 10/12-bit planar YUV variants above:
    // same three-plane layout and little-endian 16-bit words, but ALL 16
    // bits of every word are significant (there are no zero top bits and
    // no separate "valid bits" count — full-scale is 65535). Needed by
    // wavelet codecs whose signal-range presets go to 16 bits per
    // component (SMPTE VC-2 / Dirac video-format presets 7 and 8).
    /// 16-bit YUV 4:2:0 planar, little-endian 16-bit storage. All 16
    /// bits of each sample word are significant.
    Yuv420P16Le = 41,
    /// 16-bit YUV 4:2:2 planar, little-endian 16-bit storage. All 16
    /// bits of each sample word are significant.
    Yuv422P16Le = 42,
    /// 16-bit YUV 4:4:4 planar, little-endian 16-bit storage. All 16
    /// bits of each sample word are significant.
    Yuv444P16Le = 43,

    // --- 8-bit YUV + alpha at the remaining chroma samplings ---
    //
    // Companions to `Yuva420P`: the alpha plane is always full
    // resolution (one 8-bit sample per pixel, never chroma-subsampled),
    // appended after the V plane as plane index 3. Intermediate/mezzanine
    // codecs carry alpha at 4:2:2 and 4:4:4 samplings.
    /// Yuv422P with an additional full-resolution alpha plane.
    Yuva422P = 44,
    /// Yuv444P with an additional full-resolution alpha plane.
    Yuva444P = 45,

    // --- Deep YUV + alpha (10/12/16-bit words with full-resolution A) ---
    //
    // Alpha-carrying companions to the 10/12/16-bit planar YUV variants
    // above, completing the Yuva family for mezzanine codecs that carry
    // deep colour together with an alpha channel. Same conventions as
    // the 8-bit `Yuva*` trio: 4 planes ordered Y, U, V, A with the
    // alpha plane always at full resolution (one sample per pixel,
    // never chroma-subsampled) as plane index 3. Every sample — alpha
    // included — is stored as a little-endian 16-bit word; for the
    // 10/12-bit variants each sample uses the low bits of the word with
    // the top bits zero, and for the 16-bit variants all 16 bits of
    // every word are significant (full-scale is 65535), matching
    // `Yuv420P16Le`/`Yuv422P16Le`/`Yuv444P16Le`.
    /// 10-bit YUV 4:2:2 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; each sample uses
    /// the low 10 bits of a 16-bit word.
    Yuva422P10Le = 46,
    /// 12-bit YUV 4:2:2 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; each sample uses
    /// the low 12 bits of a 16-bit word.
    Yuva422P12Le = 47,
    /// 10-bit YUV 4:4:4 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; each sample uses
    /// the low 10 bits of a 16-bit word.
    Yuva444P10Le = 48,
    /// 12-bit YUV 4:4:4 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; each sample uses
    /// the low 12 bits of a 16-bit word.
    Yuva444P12Le = 49,
    /// 16-bit YUV 4:2:2 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; all 16 bits of
    /// each sample word are significant.
    Yuva422P16Le = 50,
    /// 16-bit YUV 4:4:4 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; all 16 bits of
    /// each sample word are significant.
    Yuva444P16Le = 51,

    // --- Native 8-bit and full-width 16-bit planar GBR(A) ---
    //
    // Companions to the 10/12/14-bit `Gbrp*`/`Gbrap*` family above,
    // closing the planar-RGB depth ladder at both ends for lossless
    // RGB codecs whose native coding space is per-plane G, B, R.
    // Plane order is identical to the rest of the family: G, B, R
    // (and A as plane index 3 for `Gbrap16Le`, always at full
    // resolution — RGB has no chroma subsampling). `Gbrp8` stores one
    // byte per sample with all 8 bits significant; the 16-bit variants
    // store little-endian 16-bit words with ALL 16 bits significant
    // (full-scale is 65535, matching the `Yuv*P16Le` convention — no
    // zero top bits, no separate valid-bits count). Odd in-between
    // depths on these storage formats (e.g. 9- or 15-bit RGB) are
    // expressed via the per-plane significant-bits side-channel on
    // `VideoFrame`, not by new enum variants.
    /// 8-bit planar GBR. 3 planes ordered G, B, R; one byte per
    /// sample, all 8 bits significant.
    Gbrp8 = 52,
    /// 16-bit planar GBR, little-endian 16-bit storage. 3 planes
    /// ordered G, B, R; all 16 bits of each sample word are
    /// significant.
    Gbrp16Le = 53,
    /// 16-bit planar GBR + alpha, little-endian 16-bit storage. 4
    /// planes ordered G, B, R, A; all 16 bits of each sample word are
    /// significant.
    Gbrap16Le = 54,

    // --- Deep YUV + alpha at 4:2:0 ---
    //
    // Completes the deep Yuva family begun by the 4:2:2/4:4:4 variants
    // above (46-51) at the remaining chroma sampling. Same conventions:
    // 4 planes ordered Y, U, V, A with the alpha plane always at full
    // resolution (one sample per pixel, never chroma-subsampled) as
    // plane index 3. Every sample — alpha included — is stored as a
    // little-endian 16-bit word; the 10/12-bit variants keep values in
    // the low bits of the word with the top bits zero, and the 16-bit
    // variant has all 16 bits of every word significant (full-scale is
    // 65535), matching `Yuv420P16Le`.
    /// 10-bit YUV 4:2:0 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; each sample uses
    /// the low 10 bits of a 16-bit word.
    Yuva420P10Le = 55,
    /// 12-bit YUV 4:2:0 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; each sample uses
    /// the low 12 bits of a 16-bit word.
    Yuva420P12Le = 56,
    /// 16-bit YUV 4:2:0 planar + full-resolution alpha, little-endian
    /// 16-bit storage. 4 planes ordered Y, U, V, A; all 16 bits of
    /// each sample word are significant.
    Yuva420P16Le = 57,

    // --- 8-bit planar GBR + alpha ---
    //
    // Alpha-carrying companion to `Gbrp8`, filling the last hole in
    // the planar GBR(A) family: with this variant every depth on the
    // ladder (8/10/12/14/16) exists in both alpha-less and
    // alpha-carrying form. Same conventions as the rest of the family:
    // planes ordered G, B, R, A with the alpha plane at full
    // resolution (RGB has no chroma subsampling) as plane index 3,
    // one byte per sample, all 8 bits significant. Lossless RGB codecs
    // whose native coding space is per-plane G, B, R carry 8-bit RGBA
    // in exactly this shape.
    /// 8-bit planar GBR + alpha. 4 planes ordered G, B, R, A; one
    /// byte per sample, all 8 bits significant.
    Gbrap8 = 58,

    // --- Deep gray + alpha ---
    //
    // 16-bit companion to `Ya8`, ending the gray+alpha ladder at the
    // same depth the plain gray ladder already reaches (`Gray16Le`).
    // Still-image wire formats carry 16-bit greyscale-with-alpha
    // natively (PNG colour type 4 at bit depth 16); without this
    // variant that content must detour through `Rgba64Le`, tripling
    // the gray payload and losing the "single luminance component"
    // semantics. Same packed shape as `Ya8` — interleaved Y then A —
    // with each sample widened to a little-endian 16-bit word, all 16
    // bits significant (full-scale is 65535, the `Gray16Le`
    // convention). In-between gray+alpha depths stay the job of the
    // per-plane significant-bits side-channel.
    /// Packed 16-bit grayscale + alpha, little-endian, 4 bytes/pixel
    /// (Y, A). All 16 bits of each sample word are significant.
    Ya16Le = 59,

    // --- Print / prepress, inverted-ink convention ---
    //
    // The companion `Cmyk` (33) reserved this name when it was added:
    // Adobe-authored 4-component scans store ink coverage inverted on
    // the wire (0 = full ink, 255 = no ink), and decoders that want to
    // hand the wire values through losslessly need a format that says
    // so rather than silently re-using the regular-convention `Cmyk`.
    /// Packed 8-bit inverted CMYK, 4 bytes/pixel in byte order C, M,
    /// Y, K. Inverted-ink convention: C=0 means full cyan ink, C=255
    /// means no cyan (white) — the complement of [`Cmyk`](Self::Cmyk).
    CmykInverted = 60,
}

impl PixelFormat {
    /// True if this format stores its components in separate planes.
    pub fn is_planar(&self) -> bool {
        matches!(
            self,
            Self::Yuv420P
                | Self::Yuv422P
                | Self::Yuv444P
                | Self::Yuv411P
                | Self::Yuv420P10Le
                | Self::Yuv422P10Le
                | Self::Yuv444P10Le
                | Self::Yuv420P12Le
                | Self::Yuv422P12Le
                | Self::Yuv444P12Le
                | Self::Yuv420P16Le
                | Self::Yuv422P16Le
                | Self::Yuv444P16Le
                | Self::YuvJ420P
                | Self::YuvJ422P
                | Self::YuvJ444P
                | Self::Nv12
                | Self::Nv21
                | Self::Yuva420P
                | Self::Yuva422P
                | Self::Yuva444P
                | Self::Yuva422P10Le
                | Self::Yuva422P12Le
                | Self::Yuva444P10Le
                | Self::Yuva444P12Le
                | Self::Yuva422P16Le
                | Self::Yuva444P16Le
                | Self::Yuva420P10Le
                | Self::Yuva420P12Le
                | Self::Yuva420P16Le
                | Self::Gbrp8
                | Self::Gbrap8
                | Self::Gbrp10Le
                | Self::Gbrap10Le
                | Self::Gbrp12Le
                | Self::Gbrap12Le
                | Self::Gbrp14Le
                | Self::Gbrap14Le
                | Self::Gbrp16Le
                | Self::Gbrap16Le
        )
    }

    /// True if the format is a palette index format (`Pal8`).
    pub fn is_palette(&self) -> bool {
        matches!(self, Self::Pal8)
    }

    /// True if this format carries an alpha channel.
    pub fn has_alpha(&self) -> bool {
        matches!(
            self,
            Self::Rgba
                | Self::Bgra
                | Self::Argb
                | Self::Abgr
                | Self::Rgba64Le
                | Self::Ya8
                | Self::Ya16Le
                | Self::Yuva420P
                | Self::Yuva422P
                | Self::Yuva444P
                | Self::Yuva422P10Le
                | Self::Yuva422P12Le
                | Self::Yuva444P10Le
                | Self::Yuva444P12Le
                | Self::Yuva422P16Le
                | Self::Yuva444P16Le
                | Self::Yuva420P10Le
                | Self::Yuva420P12Le
                | Self::Yuva420P16Le
                | Self::Gbrap8
                | Self::Gbrap10Le
                | Self::Gbrap12Le
                | Self::Gbrap14Le
                | Self::Gbrap16Le
        )
    }

    /// Number of planes in the stored layout. Packed and palette formats
    /// return 1; NV12/NV21 return 2; planar YUV without alpha and the
    /// `Gbrp*` variants return 3; YuvA and `Gbrap*` variants return 4.
    pub fn plane_count(&self) -> usize {
        match self {
            Self::Nv12 | Self::Nv21 => 2,
            Self::Yuv420P
            | Self::Yuv422P
            | Self::Yuv444P
            | Self::Yuv411P
            | Self::Yuv420P10Le
            | Self::Yuv422P10Le
            | Self::Yuv444P10Le
            | Self::Yuv420P12Le
            | Self::Yuv422P12Le
            | Self::Yuv444P12Le
            | Self::Yuv420P16Le
            | Self::Yuv422P16Le
            | Self::Yuv444P16Le
            | Self::YuvJ420P
            | Self::YuvJ422P
            | Self::YuvJ444P
            | Self::Gbrp8
            | Self::Gbrp10Le
            | Self::Gbrp12Le
            | Self::Gbrp14Le
            | Self::Gbrp16Le => 3,
            Self::Yuva420P
            | Self::Yuva422P
            | Self::Yuva444P
            | Self::Yuva422P10Le
            | Self::Yuva422P12Le
            | Self::Yuva444P10Le
            | Self::Yuva444P12Le
            | Self::Yuva422P16Le
            | Self::Yuva444P16Le
            | Self::Yuva420P10Le
            | Self::Yuva420P12Le
            | Self::Yuva420P16Le
            | Self::Gbrap8
            | Self::Gbrap10Le
            | Self::Gbrap12Le
            | Self::Gbrap14Le
            | Self::Gbrap16Le => 4,
            _ => 1,
        }
    }

    /// Rough bits-per-pixel estimate, useful for buffer sizing. Not exact
    /// for chroma-subsampled YUV — intended for worst-case preallocation
    /// rather than wire-accurate accounting.
    pub fn bits_per_pixel_approx(&self) -> u32 {
        match self {
            Self::MonoBlack | Self::MonoWhite => 1,
            Self::Gray8 | Self::Pal8 => 8,
            Self::Ya8 => 16,
            // 16-bit gray + alpha: two LE 16-bit words per pixel, all
            // bits significant — packed bits equal storage bits.
            Self::Ya16Le => 32,
            Self::Gray16Le | Self::Gray10Le | Self::Gray12Le => 16,
            Self::Rgb24 | Self::Bgr24 => 24,
            Self::Rgba | Self::Bgra | Self::Argb | Self::Abgr => 32,
            Self::Rgb48Le => 48,
            Self::Rgba64Le => 64,
            Self::Yuyv422 | Self::Uyvy422 => 16,
            Self::Cmyk | Self::CmykInverted => 32,
            // Planar YUV: 4:2:0 ≈ 12, 4:2:2 ≈ 16, 4:4:4 ≈ 24
            // 10/12-bit variants double the byte count but we report the
            // packed-bits-per-pixel estimate for a uniform heuristic.
            Self::Yuv420P | Self::YuvJ420P | Self::Nv12 | Self::Nv21 => 12,
            // 4:1:1 has the same packed bits-per-pixel as 4:2:0 (luma at
            // full res + 2 chroma planes each subsampled by 4).
            Self::Yuv411P => 12,
            Self::Yuv422P | Self::YuvJ422P => 16,
            Self::Yuv444P | Self::YuvJ444P => 24,
            Self::Yuv420P10Le | Self::Yuv420P12Le | Self::Yuv420P16Le => 24,
            Self::Yuv422P10Le | Self::Yuv422P12Le | Self::Yuv422P16Le => 32,
            Self::Yuv444P10Le | Self::Yuv444P12Le | Self::Yuv444P16Le => 48,
            Self::Yuva420P => 20,
            // 4:2:2 + full-res alpha: 8 (Y) + 4 (U) + 4 (V) + 8 (A).
            Self::Yuva422P => 24,
            // 4:4:4 + full-res alpha: four full-resolution 8-bit planes.
            Self::Yuva444P => 32,
            // Deep 4:2:2 + full-res alpha in 16-bit words: the estimator
            // reports the 16-bit-word cost like the alpha-less deep YUV
            // arms above — 3 sample words per pixel (Y + U/2 + V/2 + A).
            Self::Yuva422P10Le | Self::Yuva422P12Le | Self::Yuva422P16Le => 48,
            // Deep 4:4:4 + full-res alpha: 4 sample words per pixel.
            Self::Yuva444P10Le | Self::Yuva444P12Le | Self::Yuva444P16Le => 64,
            // Deep 4:2:0 + full-res alpha in 16-bit words: 16-bit-word
            // storage cost of the alpha-less 4:2:0 arms above (24) plus
            // one full-resolution 16-bit alpha word per pixel.
            Self::Yuva420P10Le | Self::Yuva420P12Le | Self::Yuva420P16Le => 40,
            // Planar GBR(A) at 10/12/14 bits stored in 16-bit words: we
            // report the packed bits-per-pixel density (samples × bits)
            // rather than the 16-bit storage cost, matching how the
            // 10/12-bit YUV variants are reported above.
            Self::Gbrp10Le => 30,
            Self::Gbrap10Le => 40,
            Self::Gbrp12Le => 36,
            Self::Gbrap12Le => 48,
            Self::Gbrp14Le => 42,
            Self::Gbrap14Le => 56,
            // Native 8-bit GBR: three bytes per pixel, like Rgb24 but
            // planar. 16-bit GBR(A): packed bits == storage bits (every
            // bit of each 16-bit word is significant), so the density
            // and storage numbers coincide.
            Self::Gbrp8 => 24,
            Self::Gbrp16Le => 48,
            Self::Gbrap16Le => 64,
            // 8-bit GBR + alpha: four bytes per pixel, like Rgba but
            // planar.
            Self::Gbrap8 => 32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin every `PixelFormat` and `SampleFormat` discriminant. This is the
    /// stability commitment — the integer value of each variant is part of
    /// the public ABI. Any reorder, renumber, or removal will fail this test
    /// and the change MUST be a major version bump (or a fresh variant
    /// appended at a new number, leaving the existing ones untouched).
    #[test]
    fn pixel_format_discriminants_pinned() {
        assert_eq!(PixelFormat::Yuv420P as u16, 0);
        assert_eq!(PixelFormat::Yuv422P as u16, 1);
        assert_eq!(PixelFormat::Yuv444P as u16, 2);
        assert_eq!(PixelFormat::Rgb24 as u16, 3);
        assert_eq!(PixelFormat::Rgba as u16, 4);
        assert_eq!(PixelFormat::Gray8 as u16, 5);
        assert_eq!(PixelFormat::Pal8 as u16, 6);
        assert_eq!(PixelFormat::Bgr24 as u16, 7);
        assert_eq!(PixelFormat::Bgra as u16, 8);
        assert_eq!(PixelFormat::Argb as u16, 9);
        assert_eq!(PixelFormat::Abgr as u16, 10);
        assert_eq!(PixelFormat::Rgb48Le as u16, 11);
        assert_eq!(PixelFormat::Rgba64Le as u16, 12);
        assert_eq!(PixelFormat::Gray16Le as u16, 13);
        assert_eq!(PixelFormat::Gray10Le as u16, 14);
        assert_eq!(PixelFormat::Gray12Le as u16, 15);
        assert_eq!(PixelFormat::Yuv420P10Le as u16, 16);
        assert_eq!(PixelFormat::Yuv422P10Le as u16, 17);
        assert_eq!(PixelFormat::Yuv444P10Le as u16, 18);
        assert_eq!(PixelFormat::Yuv420P12Le as u16, 19);
        assert_eq!(PixelFormat::Yuv422P12Le as u16, 20);
        assert_eq!(PixelFormat::Yuv444P12Le as u16, 21);
        assert_eq!(PixelFormat::YuvJ420P as u16, 22);
        assert_eq!(PixelFormat::YuvJ422P as u16, 23);
        assert_eq!(PixelFormat::YuvJ444P as u16, 24);
        assert_eq!(PixelFormat::Nv12 as u16, 25);
        assert_eq!(PixelFormat::Nv21 as u16, 26);
        assert_eq!(PixelFormat::Ya8 as u16, 27);
        assert_eq!(PixelFormat::Yuva420P as u16, 28);
        assert_eq!(PixelFormat::MonoBlack as u16, 29);
        assert_eq!(PixelFormat::MonoWhite as u16, 30);
        assert_eq!(PixelFormat::Yuyv422 as u16, 31);
        assert_eq!(PixelFormat::Uyvy422 as u16, 32);
        assert_eq!(PixelFormat::Cmyk as u16, 33);
        assert_eq!(PixelFormat::Yuv411P as u16, 34);
        assert_eq!(PixelFormat::Gbrp10Le as u16, 35);
        assert_eq!(PixelFormat::Gbrap10Le as u16, 36);
        assert_eq!(PixelFormat::Gbrp12Le as u16, 37);
        assert_eq!(PixelFormat::Gbrap12Le as u16, 38);
        assert_eq!(PixelFormat::Gbrp14Le as u16, 39);
        assert_eq!(PixelFormat::Gbrap14Le as u16, 40);
        assert_eq!(PixelFormat::Yuv420P16Le as u16, 41);
        assert_eq!(PixelFormat::Yuv422P16Le as u16, 42);
        assert_eq!(PixelFormat::Yuv444P16Le as u16, 43);
        assert_eq!(PixelFormat::Yuva422P as u16, 44);
        assert_eq!(PixelFormat::Yuva444P as u16, 45);
        assert_eq!(PixelFormat::Yuva422P10Le as u16, 46);
        assert_eq!(PixelFormat::Yuva422P12Le as u16, 47);
        assert_eq!(PixelFormat::Yuva444P10Le as u16, 48);
        assert_eq!(PixelFormat::Yuva444P12Le as u16, 49);
        assert_eq!(PixelFormat::Yuva422P16Le as u16, 50);
        assert_eq!(PixelFormat::Yuva444P16Le as u16, 51);
        assert_eq!(PixelFormat::Gbrp8 as u16, 52);
        assert_eq!(PixelFormat::Gbrp16Le as u16, 53);
        assert_eq!(PixelFormat::Gbrap16Le as u16, 54);
        assert_eq!(PixelFormat::Yuva420P10Le as u16, 55);
        assert_eq!(PixelFormat::Yuva420P12Le as u16, 56);
        assert_eq!(PixelFormat::Yuva420P16Le as u16, 57);
        assert_eq!(PixelFormat::Gbrap8 as u16, 58);
        assert_eq!(PixelFormat::Ya16Le as u16, 59);
        assert_eq!(PixelFormat::CmykInverted as u16, 60);
    }

    #[test]
    fn sample_format_discriminants_pinned() {
        assert_eq!(SampleFormat::U8 as u8, 0);
        assert_eq!(SampleFormat::S8 as u8, 1);
        assert_eq!(SampleFormat::S16 as u8, 2);
        assert_eq!(SampleFormat::S24 as u8, 3);
        assert_eq!(SampleFormat::S32 as u8, 4);
        assert_eq!(SampleFormat::F32 as u8, 5);
        assert_eq!(SampleFormat::F64 as u8, 6);
        assert_eq!(SampleFormat::U8P as u8, 7);
        assert_eq!(SampleFormat::S16P as u8, 8);
        assert_eq!(SampleFormat::S32P as u8, 9);
        assert_eq!(SampleFormat::F32P as u8, 10);
        assert_eq!(SampleFormat::F64P as u8, 11);
    }

    #[test]
    fn high_bit_yuv_planar_metadata() {
        // 10-bit reference variants are planar with three planes.
        assert!(PixelFormat::Yuv420P10Le.is_planar());
        assert!(PixelFormat::Yuv422P10Le.is_planar());
        assert!(PixelFormat::Yuv444P10Le.is_planar());

        // 12-bit variants must follow the same shape.
        assert!(PixelFormat::Yuv420P12Le.is_planar());
        assert!(PixelFormat::Yuv422P12Le.is_planar());
        assert!(PixelFormat::Yuv444P12Le.is_planar());

        assert_eq!(PixelFormat::Yuv420P12Le.plane_count(), 3);
        assert_eq!(PixelFormat::Yuv422P12Le.plane_count(), 3);
        assert_eq!(PixelFormat::Yuv444P12Le.plane_count(), 3);

        // 16-bit variants must follow the same shape.
        for fmt in [
            PixelFormat::Yuv420P16Le,
            PixelFormat::Yuv422P16Le,
            PixelFormat::Yuv444P16Le,
        ] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 3, "{fmt:?} must have 3 planes");
        }

        // None of the high-bit YUV variants carry alpha or palette.
        assert!(!PixelFormat::Yuv422P12Le.has_alpha());
        assert!(!PixelFormat::Yuv444P12Le.has_alpha());
        assert!(!PixelFormat::Yuv422P12Le.is_palette());
        assert!(!PixelFormat::Yuv444P12Le.is_palette());
        assert!(!PixelFormat::Yuv420P16Le.has_alpha());
        assert!(!PixelFormat::Yuv422P16Le.has_alpha());
        assert!(!PixelFormat::Yuv444P16Le.has_alpha());
        assert!(!PixelFormat::Yuv420P16Le.is_palette());
        assert!(!PixelFormat::Yuv422P16Le.is_palette());
        assert!(!PixelFormat::Yuv444P16Le.is_palette());
    }

    #[test]
    fn channel_layout_round_trip_count_for_known_layouts() {
        // For every `n` that `from_count` maps to a named layout, the
        // resulting layout's `channel_count()` must equal `n` again.
        for n in 1..=8u16 {
            let layout = ChannelLayout::from_count(n);
            assert_eq!(layout.channel_count(), n, "round-trip failed for n={n}");
            // None of these defaults should fall through to DiscreteN.
            assert!(
                !matches!(layout, ChannelLayout::DiscreteN(_)),
                "from_count({n}) unexpectedly produced DiscreteN"
            );
        }
    }

    #[test]
    fn channel_layout_from_count_default_table() {
        // The exact mapping documented on `from_count` — pin it so
        // future refactors don't silently change the inferred layout.
        assert_eq!(ChannelLayout::from_count(1), ChannelLayout::Mono);
        assert_eq!(ChannelLayout::from_count(2), ChannelLayout::Stereo);
        assert_eq!(ChannelLayout::from_count(3), ChannelLayout::Surround30);
        assert_eq!(ChannelLayout::from_count(4), ChannelLayout::Quad);
        assert_eq!(ChannelLayout::from_count(5), ChannelLayout::Surround50);
        assert_eq!(ChannelLayout::from_count(6), ChannelLayout::Surround51);
        assert_eq!(ChannelLayout::from_count(7), ChannelLayout::Surround61);
        assert_eq!(ChannelLayout::from_count(8), ChannelLayout::Surround71);
    }

    #[test]
    fn channel_layout_unknown_count_falls_through_to_discrete() {
        assert_eq!(ChannelLayout::from_count(0), ChannelLayout::DiscreteN(0));
        assert_eq!(ChannelLayout::from_count(13), ChannelLayout::DiscreteN(13));
        assert_eq!(
            ChannelLayout::from_count(64).channel_count(),
            64,
            "DiscreteN must report the count it was constructed with"
        );
    }

    #[test]
    fn channel_layout_position_lookup() {
        assert_eq!(
            ChannelLayout::Stereo.position(0),
            Some(ChannelPosition::FrontLeft)
        );
        assert_eq!(
            ChannelLayout::Stereo.position(1),
            Some(ChannelPosition::FrontRight)
        );
        assert_eq!(ChannelLayout::Stereo.position(2), None);

        // 5.1 canonical: L, R, C, LFE, Ls, Rs.
        let s51 = ChannelLayout::Surround51;
        assert_eq!(s51.position(0), Some(ChannelPosition::FrontLeft));
        assert_eq!(s51.position(1), Some(ChannelPosition::FrontRight));
        assert_eq!(s51.position(2), Some(ChannelPosition::FrontCenter));
        assert_eq!(s51.position(3), Some(ChannelPosition::LowFrequency));
        assert_eq!(s51.position(4), Some(ChannelPosition::SideLeft));
        assert_eq!(s51.position(5), Some(ChannelPosition::SideRight));
        assert_eq!(s51.position(6), None);

        // DiscreteN never reveals a position.
        assert_eq!(ChannelLayout::DiscreteN(13).position(0), None);
    }

    #[test]
    fn channel_layout_lfe_and_surround_predicates() {
        assert!(ChannelLayout::Surround51.has_lfe());
        assert!(ChannelLayout::Surround71.has_lfe());
        assert!(ChannelLayout::Stereo21.has_lfe());
        assert!(!ChannelLayout::Quad.has_lfe());
        assert!(!ChannelLayout::Surround50.has_lfe());
        assert!(!ChannelLayout::Stereo.has_lfe());

        assert!(!ChannelLayout::Mono.is_surround());
        assert!(!ChannelLayout::Stereo.is_surround());
        // Downmix carriers are still 2ch / no-LFE → not "surround" by
        // the layout-shape definition; the surround info lives in the
        // sample matrix itself.
        assert!(!ChannelLayout::LoRo.is_surround());
        assert!(!ChannelLayout::LtRt.is_surround());
        assert!(ChannelLayout::Stereo21.is_surround());
        assert!(ChannelLayout::Surround51.is_surround());
        assert!(ChannelLayout::Surround71.is_surround());
    }

    #[test]
    fn channel_layout_display_and_fromstr_round_trip() {
        use std::str::FromStr;
        let cases = [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Stereo21,
            ChannelLayout::Surround30,
            ChannelLayout::Quad,
            ChannelLayout::Surround40,
            ChannelLayout::Surround41,
            ChannelLayout::Surround50,
            ChannelLayout::Surround51,
            ChannelLayout::Surround60,
            ChannelLayout::Surround61,
            ChannelLayout::Surround70,
            ChannelLayout::Surround71,
            ChannelLayout::LoRo,
            ChannelLayout::LtRt,
            ChannelLayout::DiscreteN(13),
        ];
        for layout in cases {
            let s = layout.to_string();
            let parsed = ChannelLayout::from_str(&s).expect("display output must parse back");
            assert_eq!(parsed, layout, "round-trip failed via {s:?}");
        }
    }

    #[test]
    fn channel_layout_fromstr_accepts_aliases_and_case() {
        use std::str::FromStr;
        assert_eq!(
            ChannelLayout::from_str("STEREO").unwrap(),
            ChannelLayout::Stereo
        );
        assert_eq!(
            ChannelLayout::from_str("2.0").unwrap(),
            ChannelLayout::Stereo
        );
        assert_eq!(
            ChannelLayout::from_str("5.1").unwrap(),
            ChannelLayout::Surround51
        );
        assert_eq!(
            ChannelLayout::from_str("Lo/Ro").unwrap(),
            ChannelLayout::LoRo
        );
        assert_eq!(
            ChannelLayout::from_str("lt/rt").unwrap(),
            ChannelLayout::LtRt
        );
        assert!(ChannelLayout::from_str("absurd_layout").is_err());
    }

    #[test]
    fn channel_layout_positions_owned_matches_static_slice() {
        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Surround51,
            ChannelLayout::Surround71,
        ] {
            assert_eq!(layout.positions_owned(), layout.positions());
        }
        // DiscreteN returns an empty owned vec — positions are unknown.
        assert!(ChannelLayout::DiscreteN(7).positions_owned().is_empty());
    }

    #[test]
    fn sample_format_plane_count_interleaved_is_one() {
        // Interleaved formats always pack into a single plane, regardless
        // of channel count.
        for ch in [1u16, 2, 6, 8, 64, 0] {
            assert_eq!(SampleFormat::S16.plane_count(ch), 1);
            assert_eq!(SampleFormat::F32.plane_count(ch), 1);
            assert_eq!(SampleFormat::U8.plane_count(ch), 1);
            assert_eq!(SampleFormat::S24.plane_count(ch), 1);
        }
    }

    #[test]
    fn sample_format_plane_count_planar_matches_channels() {
        // Planar formats use one plane per channel.
        assert_eq!(SampleFormat::S16P.plane_count(1), 1);
        assert_eq!(SampleFormat::S16P.plane_count(2), 2);
        assert_eq!(SampleFormat::F32P.plane_count(6), 6);
        assert_eq!(SampleFormat::F64P.plane_count(8), 8);

        // Edge case: zero channels in a planar format yields zero planes.
        assert_eq!(SampleFormat::S32P.plane_count(0), 0);
    }

    #[test]
    fn high_bit_yuv_bits_per_pixel_approx() {
        // 4:2:2 and 4:4:4 12-bit match their 10-bit siblings on the
        // packed-bits estimator (the approximation reports samples-per-pixel
        // density, not the 16-bit storage width).
        assert_eq!(PixelFormat::Yuv422P10Le.bits_per_pixel_approx(), 32);
        assert_eq!(PixelFormat::Yuv422P12Le.bits_per_pixel_approx(), 32);
        assert_eq!(PixelFormat::Yuv444P10Le.bits_per_pixel_approx(), 48);
        assert_eq!(PixelFormat::Yuv444P12Le.bits_per_pixel_approx(), 48);
        assert_eq!(PixelFormat::Yuv420P12Le.bits_per_pixel_approx(), 24);

        // 16-bit: packed bits == storage bits (every bit of the 16-bit
        // word is significant), so the estimator lands on the same
        // numbers as the 10/12-bit siblings.
        assert_eq!(PixelFormat::Yuv420P16Le.bits_per_pixel_approx(), 24);
        assert_eq!(PixelFormat::Yuv422P16Le.bits_per_pixel_approx(), 32);
        assert_eq!(PixelFormat::Yuv444P16Le.bits_per_pixel_approx(), 48);
    }

    #[test]
    fn yuva_planar_metadata() {
        // All three alpha-carrying planar YUV samplings share one shape:
        // planar, 4 planes (Y, U, V, full-resolution A), alpha set, not
        // a palette format.
        for fmt in [
            PixelFormat::Yuva420P,
            PixelFormat::Yuva422P,
            PixelFormat::Yuva444P,
        ] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 4, "{fmt:?} must have 4 planes");
            assert!(fmt.has_alpha(), "{fmt:?} must carry alpha");
            assert!(!fmt.is_palette(), "{fmt:?} must not be palette");
        }

        // Packed-bits estimator: the alpha plane adds a full 8 bits per
        // pixel on top of the alpha-less sampling's density.
        assert_eq!(
            PixelFormat::Yuva420P.bits_per_pixel_approx(),
            PixelFormat::Yuv420P.bits_per_pixel_approx() + 8
        );
        assert_eq!(
            PixelFormat::Yuva422P.bits_per_pixel_approx(),
            PixelFormat::Yuv422P.bits_per_pixel_approx() + 8
        );
        assert_eq!(
            PixelFormat::Yuva444P.bits_per_pixel_approx(),
            PixelFormat::Yuv444P.bits_per_pixel_approx() + 8
        );
        assert_eq!(PixelFormat::Yuva422P.bits_per_pixel_approx(), 24);
        assert_eq!(PixelFormat::Yuva444P.bits_per_pixel_approx(), 32);
    }

    #[test]
    fn deep_yuva_planar_metadata() {
        // All six deep alpha-carrying variants share one shape: planar,
        // 4 planes (Y, U, V, full-resolution A), alpha set, no palette.
        for fmt in [
            PixelFormat::Yuva422P10Le,
            PixelFormat::Yuva422P12Le,
            PixelFormat::Yuva444P10Le,
            PixelFormat::Yuva444P12Le,
            PixelFormat::Yuva422P16Le,
            PixelFormat::Yuva444P16Le,
        ] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 4, "{fmt:?} must have 4 planes");
            assert!(fmt.has_alpha(), "{fmt:?} must carry alpha");
            assert!(!fmt.is_palette(), "{fmt:?} must not be palette");
        }
    }

    #[test]
    fn deep_yuva_bits_per_pixel_approx() {
        // Estimator reports 16-bit-word storage cost, matching the
        // alpha-less deep YUV trio: the full-resolution alpha word adds
        // 16 on top of the alpha-less sampling's number.
        for fmt in [
            PixelFormat::Yuva422P10Le,
            PixelFormat::Yuva422P12Le,
            PixelFormat::Yuva422P16Le,
        ] {
            assert_eq!(fmt.bits_per_pixel_approx(), 48, "{fmt:?}");
        }
        for fmt in [
            PixelFormat::Yuva444P10Le,
            PixelFormat::Yuva444P12Le,
            PixelFormat::Yuva444P16Le,
        ] {
            assert_eq!(fmt.bits_per_pixel_approx(), 64, "{fmt:?}");
        }
        assert_eq!(
            PixelFormat::Yuva422P16Le.bits_per_pixel_approx(),
            PixelFormat::Yuv422P16Le.bits_per_pixel_approx() + 16
        );
        assert_eq!(
            PixelFormat::Yuva444P16Le.bits_per_pixel_approx(),
            PixelFormat::Yuv444P16Le.bits_per_pixel_approx() + 16
        );
        assert_eq!(
            PixelFormat::Yuva422P10Le.bits_per_pixel_approx(),
            PixelFormat::Yuv422P10Le.bits_per_pixel_approx() + 16
        );
        assert_eq!(
            PixelFormat::Yuva444P12Le.bits_per_pixel_approx(),
            PixelFormat::Yuv444P12Le.bits_per_pixel_approx() + 16
        );
    }

    #[test]
    fn high_bit_gbr_planar_metadata() {
        // All six new variants are planar with the right plane count.
        for fmt in [
            PixelFormat::Gbrp10Le,
            PixelFormat::Gbrp12Le,
            PixelFormat::Gbrp14Le,
        ] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 3, "{fmt:?} must have 3 planes");
            assert!(!fmt.has_alpha(), "{fmt:?} must not have alpha");
            assert!(!fmt.is_palette(), "{fmt:?} must not be palette");
        }
        for fmt in [
            PixelFormat::Gbrap10Le,
            PixelFormat::Gbrap12Le,
            PixelFormat::Gbrap14Le,
        ] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 4, "{fmt:?} must have 4 planes");
            assert!(fmt.has_alpha(), "{fmt:?} must carry alpha");
            assert!(!fmt.is_palette(), "{fmt:?} must not be palette");
        }
    }

    #[test]
    fn high_bit_gbr_bits_per_pixel_approx() {
        // Packed bits-per-pixel = samples × bits (consistent with how
        // the 10/12-bit YUV variants are reported above).
        assert_eq!(PixelFormat::Gbrp10Le.bits_per_pixel_approx(), 30);
        assert_eq!(PixelFormat::Gbrap10Le.bits_per_pixel_approx(), 40);
        assert_eq!(PixelFormat::Gbrp12Le.bits_per_pixel_approx(), 36);
        assert_eq!(PixelFormat::Gbrap12Le.bits_per_pixel_approx(), 48);
        assert_eq!(PixelFormat::Gbrp14Le.bits_per_pixel_approx(), 42);
        assert_eq!(PixelFormat::Gbrap14Le.bits_per_pixel_approx(), 56);
    }

    #[test]
    fn high_bit_gbr_constructible_and_distinct() {
        // Round-trip the discriminant through `as u16` and back via the
        // pinning test's reverse mapping — every variant must be unique.
        let all = [
            PixelFormat::Gbrp10Le,
            PixelFormat::Gbrap10Le,
            PixelFormat::Gbrp12Le,
            PixelFormat::Gbrap12Le,
            PixelFormat::Gbrp14Le,
            PixelFormat::Gbrap14Le,
            PixelFormat::Gbrp8,
            PixelFormat::Gbrap8,
            PixelFormat::Gbrp16Le,
            PixelFormat::Gbrap16Le,
        ];
        let mut seen = std::collections::HashSet::new();
        for fmt in all {
            assert!(seen.insert(fmt as u16), "duplicate discriminant: {fmt:?}");
        }
    }

    #[test]
    fn gbr_depth_ladder_ends_metadata() {
        // Gbrp8 and the 16-bit pair share the family shape: planar,
        // G/B/R plane order (3 planes), alpha only on Gbrap16Le, never
        // palette.
        for fmt in [PixelFormat::Gbrp8, PixelFormat::Gbrp16Le] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 3, "{fmt:?} must have 3 planes");
            assert!(!fmt.has_alpha(), "{fmt:?} must not have alpha");
            assert!(!fmt.is_palette(), "{fmt:?} must not be palette");
        }
        assert!(PixelFormat::Gbrap16Le.is_planar());
        assert_eq!(PixelFormat::Gbrap16Le.plane_count(), 4);
        assert!(PixelFormat::Gbrap16Le.has_alpha());
        assert!(!PixelFormat::Gbrap16Le.is_palette());
    }

    #[test]
    fn gbr_depth_ladder_ends_bits_per_pixel_approx() {
        // Gbrp8 matches the packed 8-bit RGB density (planar layout
        // doesn't change bits-per-pixel), and the 16-bit pair matches
        // the packed 16-bit RGB(A) densities — for 16-bit words packed
        // bits equal storage bits.
        assert_eq!(
            PixelFormat::Gbrp8.bits_per_pixel_approx(),
            PixelFormat::Rgb24.bits_per_pixel_approx()
        );
        assert_eq!(
            PixelFormat::Gbrp16Le.bits_per_pixel_approx(),
            PixelFormat::Rgb48Le.bits_per_pixel_approx()
        );
        assert_eq!(
            PixelFormat::Gbrap16Le.bits_per_pixel_approx(),
            PixelFormat::Rgba64Le.bits_per_pixel_approx()
        );
        assert_eq!(PixelFormat::Gbrp8.bits_per_pixel_approx(), 24);
        assert_eq!(PixelFormat::Gbrp16Le.bits_per_pixel_approx(), 48);
        assert_eq!(PixelFormat::Gbrap16Le.bits_per_pixel_approx(), 64);
    }

    #[test]
    fn gbrap8_metadata() {
        // Gbrap8 completes the GBR(A) family: every depth on the
        // 8/10/12/14/16 ladder now has both an alpha-less and an
        // alpha-carrying variant. Shape matches the rest of the
        // alpha-carrying family: planar, 4 planes (G, B, R,
        // full-resolution A), alpha set, never palette.
        let fmt = PixelFormat::Gbrap8;
        assert!(fmt.is_planar());
        assert_eq!(fmt.plane_count(), 4);
        assert!(fmt.has_alpha());
        assert!(!fmt.is_palette());
    }

    #[test]
    fn gbrap8_bits_per_pixel_approx() {
        // Four bytes per pixel: the packed Rgba density (planar layout
        // doesn't change bits-per-pixel), i.e. the alpha plane adds a
        // full 8 bits on top of Gbrp8.
        assert_eq!(PixelFormat::Gbrap8.bits_per_pixel_approx(), 32);
        assert_eq!(
            PixelFormat::Gbrap8.bits_per_pixel_approx(),
            PixelFormat::Rgba.bits_per_pixel_approx()
        );
        assert_eq!(
            PixelFormat::Gbrap8.bits_per_pixel_approx(),
            PixelFormat::Gbrp8.bits_per_pixel_approx() + 8
        );
    }

    #[test]
    fn gbr_family_alpha_ladder_complete() {
        // Every GBR depth has an alpha companion with exactly one more
        // plane and the same planarity — the asymmetry Gbrap8 closed.
        let pairs = [
            (PixelFormat::Gbrp8, PixelFormat::Gbrap8),
            (PixelFormat::Gbrp10Le, PixelFormat::Gbrap10Le),
            (PixelFormat::Gbrp12Le, PixelFormat::Gbrap12Le),
            (PixelFormat::Gbrp14Le, PixelFormat::Gbrap14Le),
            (PixelFormat::Gbrp16Le, PixelFormat::Gbrap16Le),
        ];
        for (gbr, gbra) in pairs {
            assert!(gbr.is_planar() && gbra.is_planar());
            assert_eq!(gbr.plane_count(), 3, "{gbr:?}");
            assert_eq!(gbra.plane_count(), 4, "{gbra:?}");
            assert!(!gbr.has_alpha(), "{gbr:?}");
            assert!(gbra.has_alpha(), "{gbra:?}");
        }
    }

    #[test]
    fn ya16le_metadata() {
        // Same packed shape as Ya8 (interleaved Y, A in one plane),
        // widened to 16-bit LE words: not planar, single plane, alpha
        // set, never palette. Density is exactly double Ya8's and
        // matches half of Rgba64Le (two components instead of four).
        let fmt = PixelFormat::Ya16Le;
        assert!(!fmt.is_planar());
        assert_eq!(fmt.plane_count(), 1);
        assert!(fmt.has_alpha());
        assert!(!fmt.is_palette());
        assert_eq!(fmt.bits_per_pixel_approx(), 32);
        assert_eq!(
            fmt.bits_per_pixel_approx(),
            PixelFormat::Ya8.bits_per_pixel_approx() * 2
        );
        assert_eq!(
            fmt.bits_per_pixel_approx(),
            PixelFormat::Rgba64Le.bits_per_pixel_approx() / 2
        );
        // The alpha word adds a full 16 bits on top of Gray16Le.
        assert_eq!(
            fmt.bits_per_pixel_approx(),
            PixelFormat::Gray16Le.bits_per_pixel_approx() + 16
        );
    }

    #[test]
    fn cmyk_inverted_metadata() {
        // The inverted-ink convention changes sample semantics, not
        // layout: CmykInverted must be metadata-identical to Cmyk on
        // every shape predicate.
        let (reg, inv) = (PixelFormat::Cmyk, PixelFormat::CmykInverted);
        for fmt in [reg, inv] {
            assert!(!fmt.is_planar(), "{fmt:?}");
            assert_eq!(fmt.plane_count(), 1, "{fmt:?}");
            assert!(!fmt.has_alpha(), "{fmt:?}");
            assert!(!fmt.is_palette(), "{fmt:?}");
        }
        assert_eq!(reg.bits_per_pixel_approx(), inv.bits_per_pixel_approx());
        assert_eq!(inv.bits_per_pixel_approx(), 32);
        // They remain distinct formats on the wire-stable axis.
        assert_ne!(reg as u16, inv as u16);
    }

    #[test]
    fn deep_yuva420_planar_metadata() {
        // The 4:2:0 completions share the deep-Yuva shape: planar, 4
        // planes (Y, U, V, full-resolution A), alpha set, no palette.
        for fmt in [
            PixelFormat::Yuva420P10Le,
            PixelFormat::Yuva420P12Le,
            PixelFormat::Yuva420P16Le,
        ] {
            assert!(fmt.is_planar(), "{fmt:?} must be planar");
            assert_eq!(fmt.plane_count(), 4, "{fmt:?} must have 4 planes");
            assert!(fmt.has_alpha(), "{fmt:?} must carry alpha");
            assert!(!fmt.is_palette(), "{fmt:?} must not be palette");
        }
    }

    #[test]
    fn deep_yuva420_bits_per_pixel_approx() {
        // Same estimator convention as the 4:2:2/4:4:4 deep Yuva arms:
        // 16-bit-word storage cost, with the full-resolution alpha word
        // adding 16 on top of the alpha-less sampling's number.
        for fmt in [
            PixelFormat::Yuva420P10Le,
            PixelFormat::Yuva420P12Le,
            PixelFormat::Yuva420P16Le,
        ] {
            assert_eq!(fmt.bits_per_pixel_approx(), 40, "{fmt:?}");
        }
        assert_eq!(
            PixelFormat::Yuva420P10Le.bits_per_pixel_approx(),
            PixelFormat::Yuv420P10Le.bits_per_pixel_approx() + 16
        );
        assert_eq!(
            PixelFormat::Yuva420P12Le.bits_per_pixel_approx(),
            PixelFormat::Yuv420P12Le.bits_per_pixel_approx() + 16
        );
        assert_eq!(
            PixelFormat::Yuva420P16Le.bits_per_pixel_approx(),
            PixelFormat::Yuv420P16Le.bits_per_pixel_approx() + 16
        );
    }
}
