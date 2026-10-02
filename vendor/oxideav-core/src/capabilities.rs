//! Codec capability description.
//!
//! Each codec implementation registered with the codec registry attaches one
//! of these structs to declare what it can do, what its constraints are, and
//! how the registry should rank it against alternative implementations of
//! the same codec id.
//!
//! The flag layout is a 6-column capability string (one letter per
//! capability, `.` when absent):
//!
//! ```text
//!  D..... = Decoding supported
//!  .E.... = Encoding supported
//!  ..V... = Video codec       ..A... = Audio       ..S... = Subtitle
//!  ..D... = Data              ..T... = Attachment
//!  ...I.. = Intra-frame-only codec
//!  ....L. = Lossy compression
//!  .....S = Lossless compression
//! ```

use std::fmt;

use crate::format::{MediaType, PixelFormat};

/// Default priority for software implementations. Lower numbers are preferred
/// at resolution time, so register hardware impls with a smaller value (e.g.
/// `10`) and software fallbacks with the default `100`.
pub const DEFAULT_PRIORITY: i32 = 100;

/// What an implementation can do plus how it ranks vs alternatives.
#[derive(Clone, Debug)]
pub struct CodecCapabilities {
    /// Decoding supported by this implementation.
    pub decode: bool,
    /// Encoding supported by this implementation.
    pub encode: bool,
    /// Media type this implementation handles (audio, video, ...).
    pub media_type: MediaType,
    /// Every coded unit is independently decodable (no inter-frame
    /// prediction).
    pub intra_only: bool,
    /// Supports lossy compression.
    pub lossy: bool,
    /// Supports lossless compression. `lossy` and `lossless` may both
    /// be set for codecs that offer both modes.
    pub lossless: bool,
    /// Hardware-accelerated implementation (VAAPI/NVENC/QSV/VideoToolbox/...).
    pub hardware_accelerated: bool,
    /// Short identifier for this implementation, e.g. "flac_sw", "h264_qsv".
    pub implementation: String,
    /// Restrictions — `None` means "no constraint".
    pub max_width: Option<u32>,
    /// Maximum supported frame height in pixels; `None` = unconstrained.
    pub max_height: Option<u32>,
    /// Maximum supported bit rate in bits per second; `None` =
    /// unconstrained.
    pub max_bitrate: Option<u64>,
    /// Maximum supported audio sample rate in Hz; `None` = unconstrained.
    pub max_sample_rate: Option<u32>,
    /// Maximum supported audio channel count; `None` = unconstrained.
    pub max_channels: Option<u16>,
    /// Lower numbers are preferred. HW impls should be ~10, SW impls ~100.
    pub priority: i32,
    /// Pixel formats this implementation accepts (video only). An empty
    /// `Vec` means "any format" — resolution won't filter on it. When
    /// populated, the registry can skip impls whose accepted set does not
    /// include the format requested by the caller.
    pub accepted_pixel_formats: Vec<PixelFormat>,
}

impl CodecCapabilities {
    /// Construct a software audio decoder/encoder capability set with sensible
    /// defaults — adjust fields after creation.
    pub fn audio(implementation: impl Into<String>) -> Self {
        Self {
            decode: false,
            encode: false,
            media_type: MediaType::Audio,
            intra_only: true, // audio packets are independently decodable in most codecs
            lossy: false,
            lossless: false,
            hardware_accelerated: false,
            implementation: implementation.into(),
            max_width: None,
            max_height: None,
            max_bitrate: None,
            max_sample_rate: None,
            max_channels: None,
            priority: DEFAULT_PRIORITY,
            accepted_pixel_formats: Vec::new(),
        }
    }

    /// Construct a software video decoder/encoder capability set with
    /// sensible defaults — adjust fields after creation.
    pub fn video(implementation: impl Into<String>) -> Self {
        Self {
            decode: false,
            encode: false,
            media_type: MediaType::Video,
            intra_only: false,
            lossy: false,
            lossless: false,
            hardware_accelerated: false,
            implementation: implementation.into(),
            max_width: None,
            max_height: None,
            max_bitrate: None,
            max_sample_rate: None,
            max_channels: None,
            priority: DEFAULT_PRIORITY,
            accepted_pixel_formats: Vec::new(),
        }
    }

    /// 6-character capability flag string (see the module docs for the
    /// column layout). Useful for `oxideav list`-style
    /// output.
    pub fn flag_string(&self) -> String {
        let mut s = String::with_capacity(6);
        s.push(if self.decode { 'D' } else { '.' });
        s.push(if self.encode { 'E' } else { '.' });
        s.push(match self.media_type {
            MediaType::Video => 'V',
            MediaType::Audio => 'A',
            MediaType::Subtitle => 'S',
            MediaType::Data => 'D',
            MediaType::Unknown => '.',
        });
        s.push(if self.intra_only { 'I' } else { '.' });
        s.push(if self.lossy { 'L' } else { '.' });
        s.push(if self.lossless { 'S' } else { '.' });
        s
    }

    // Builder-style helpers so registrations stay compact.

    /// Mark this implementation as supporting decode.
    pub fn with_decode(mut self) -> Self {
        self.decode = true;
        self
    }
    /// Mark this implementation as supporting encode.
    pub fn with_encode(mut self) -> Self {
        self.encode = true;
        self
    }
    /// Set the intra-frame-only flag.
    pub fn with_intra_only(mut self, v: bool) -> Self {
        self.intra_only = v;
        self
    }
    /// Set the lossy-compression flag.
    pub fn with_lossy(mut self, v: bool) -> Self {
        self.lossy = v;
        self
    }
    /// Set the lossless-compression flag.
    pub fn with_lossless(mut self, v: bool) -> Self {
        self.lossless = v;
        self
    }
    /// Set the hardware-accelerated flag.
    pub fn with_hardware(mut self, v: bool) -> Self {
        self.hardware_accelerated = v;
        self
    }
    /// Set the registry ranking priority (lower is preferred; HW ~10,
    /// SW ~100).
    pub fn with_priority(mut self, p: i32) -> Self {
        self.priority = p;
        self
    }
    /// Constrain the maximum frame size to `w` × `h` pixels.
    pub fn with_max_size(mut self, w: u32, h: u32) -> Self {
        self.max_width = Some(w);
        self.max_height = Some(h);
        self
    }
    /// Constrain the maximum bit rate (bits per second).
    pub fn with_max_bitrate(mut self, br: u64) -> Self {
        self.max_bitrate = Some(br);
        self
    }
    /// Constrain the maximum audio sample rate (Hz).
    pub fn with_max_sample_rate(mut self, sr: u32) -> Self {
        self.max_sample_rate = Some(sr);
        self
    }
    /// Constrain the maximum audio channel count.
    pub fn with_max_channels(mut self, ch: u16) -> Self {
        self.max_channels = Some(ch);
        self
    }

    /// Add one accepted pixel format. Appends — call multiple times to
    /// list several.
    pub fn with_pixel_format(mut self, fmt: PixelFormat) -> Self {
        self.accepted_pixel_formats.push(fmt);
        self
    }

    /// Replace the accepted pixel-format set wholesale.
    pub fn with_pixel_formats(mut self, fmts: Vec<PixelFormat>) -> Self {
        self.accepted_pixel_formats = fmts;
        self
    }
}

impl fmt::Display for CodecCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.flag_string(), self.implementation)
    }
}

impl CodecCapabilities {
    /// Whether this implementation's max-* restrictions are compatible
    /// with the requested codec parameters. `for_encode` is reserved
    /// for restrictions that apply asymmetrically. Used by the
    /// registry's `make_decoder` / `make_encoder` walker and by
    /// out-of-tree selection layers (e.g. `oxideav-pipeline`'s
    /// `CodecPreferences` filter).
    pub fn fits_params(&self, p: &crate::CodecParameters, for_encode: bool) -> bool {
        let _ = for_encode;
        if let (Some(max), Some(w)) = (self.max_width, p.width) {
            if w > max {
                return false;
            }
        }
        if let (Some(max), Some(h)) = (self.max_height, p.height) {
            if h > max {
                return false;
            }
        }
        if let (Some(max), Some(br)) = (self.max_bitrate, p.bit_rate) {
            if br > max {
                return false;
            }
        }
        if let (Some(max), Some(sr)) = (self.max_sample_rate, p.sample_rate) {
            if sr > max {
                return false;
            }
        }
        if let (Some(max), Some(ch)) = (self.max_channels, p.channels) {
            if ch > max {
                return false;
            }
        }
        true
    }
}
