//! Uncompressed audio and video frames.

use crate::subtitle::SubtitleCue;
use crate::vector::VectorFrame;

/// A decoded chunk of uncompressed data: either audio samples, a video
/// picture, or (for subtitle streams) a single styled cue.
///
/// Marked `#[non_exhaustive]` — consumers that match on variants must
/// include a wildcard arm. This lets the crate add new frame kinds (data
/// tracks, hap rops, …) without breaking downstream code.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Frame {
    /// Uncompressed audio samples.
    Audio(AudioFrame),
    /// One uncompressed video picture.
    Video(VideoFrame),
    /// A single subtitle cue. Timing is carried inside the cue itself
    /// (`start_us`/`end_us`) so it's independent of container time bases,
    /// but the enclosing pipeline/muxer can still rescale via `pts` at
    /// the packet layer.
    Subtitle(SubtitleCue),
    /// A resolution-independent vector-graphics frame. Produced by
    /// vector-format decoders (`oxideav-svg`, the vector path of
    /// `oxideav-pdf`) and consumed by vector renderers / writers.
    /// See [`crate::vector`] for the full primitive set.
    Vector(VectorFrame),
}

impl Frame {
    /// Presentation timestamp of the frame in its stream's time base
    /// (a subtitle cue reports its `start_us`); `None` if unknown.
    pub fn pts(&self) -> Option<i64> {
        match self {
            Self::Audio(a) => a.pts,
            Self::Video(v) => v.pts,
            Self::Subtitle(s) => Some(s.start_us),
            Self::Vector(v) => v.pts,
        }
    }
}

/// Uncompressed audio frame.
///
/// Stream-level properties (sample format, channel count, sample rate,
/// time base) are NOT carried per-frame — read them from the stream's
/// [`CodecParameters`](crate::CodecParameters). Frames stay lightweight
/// because real-time playback moves thousands per second per stream.
///
/// Sample layout is determined by the stream's `SampleFormat`:
/// - Interleaved formats: `data` has one plane; samples are stored as
///   `ch0 ch1 ... chN ch0 ch1 ... chN ...`.
/// - Planar formats: `data` has one plane per channel.
///
/// Use [`SampleFormat::plane_count`](crate::SampleFormat::plane_count)
/// with the stream's channel count to compute the expected `data.len()`.
#[derive(Clone, Debug)]
pub struct AudioFrame {
    /// Number of samples *per channel* in this frame. Variable per-frame
    /// for VBR codecs and on partial flushes.
    pub samples: u32,
    /// Presentation timestamp in the stream's time base; `None` if unknown.
    pub pts: Option<i64>,
    /// Raw sample bytes. Length matches `format.plane_count(channels)`
    /// from the stream's `CodecParameters`.
    pub data: Vec<Vec<u8>>,
}

/// Uncompressed video frame.
///
/// Stream-level properties (pixel format, width, height, time base) are
/// NOT carried per-frame — read them from the stream's
/// [`CodecParameters`](crate::CodecParameters). Frames stay lightweight
/// because real-time playback moves thousands per second per stream.
///
/// # Side-channels
///
/// `VideoFrame` (like [`VideoPlane`]) is a fully-public struct built by
/// struct literal throughout the codec crates, so per-frame metadata
/// cannot be added as new fields without breaking every constructor.
/// Instead, optional metadata rides in-band as *side-channel* entries at
/// the tail of `planes`: [`VideoPlane`] values whose shape is impossible
/// for an image plane, which makes them unambiguous. Two side-channel
/// record kinds exist, distinguished by their `stride` tag:
///
/// - **Palette** — `stride == 0`, non-empty `data`. Impossible for an
///   image plane because an image plane's `data` is `stride × rows`
///   long, so a zero stride forces empty data. Carries the color table
///   for palette-indexed content
///   ([`PixelFormat::Pal8`](crate::PixelFormat::Pal8)); see
///   [`palette`](Self::palette) / [`set_palette`](Self::set_palette).
/// - **Per-plane significant bits** — `stride == usize::MAX`, non-empty
///   `data`. Impossible for an image plane because `stride × rows`
///   bytes with any non-zero row count would exceed what a `Vec` can
///   hold. Carries mixed per-plane bit depths (e.g. 12-bit luma with
///   10-bit chroma from a wavelet codec's custom signal range); see
///   [`significant_bits`](Self::significant_bits) /
///   [`set_significant_bits`](Self::set_significant_bits).
///
/// The two records compose: a frame can carry both at once, in either
/// order, within the trailing run of side-channel-shaped entries. The
/// typed accessors find each record by its `stride` tag regardless of
/// order, and [`image_planes`](Self::image_planes) /
/// [`image_plane_count`](Self::image_plane_count) exclude the whole
/// trailing run. Frames without any attached side-channel are
/// byte-for-byte identical to what they always were.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    /// Presentation timestamp in the stream's time base; `None` if unknown.
    pub pts: Option<i64>,
    /// One entry per plane (e.g., 3 for Yuv420P). Each entry is `(stride, bytes)`.
    ///
    /// May additionally end with side-channel entries (palette,
    /// per-plane significant bits — see the type-level docs). Code that
    /// wants only pixel planes should iterate
    /// [`image_planes`](Self::image_planes) instead of this field.
    pub planes: Vec<VideoPlane>,
}

/// `stride` tag of the per-plane significant-bits side-channel record.
/// (The palette record's tag is `0`; see the [`VideoFrame`] docs.)
const SIGNIFICANT_BITS_STRIDE: usize = usize::MAX;

impl VideoFrame {
    /// `true` when `plane` has a side-channel record shape: one of the
    /// two impossible-for-an-image-plane sentinels described in the
    /// type-level docs.
    fn is_side_channel_entry(plane: &VideoPlane) -> bool {
        (plane.stride == 0 || plane.stride == SIGNIFICANT_BITS_STRIDE) && !plane.data.is_empty()
    }

    /// Index of the first entry of the trailing side-channel run — equal
    /// to the number of image planes. Scans backwards from the tail
    /// while entries have a side-channel shape.
    fn side_channel_run_start(&self) -> usize {
        let mut start = self.planes.len();
        while start > 0 && Self::is_side_channel_entry(&self.planes[start - 1]) {
            start -= 1;
        }
        start
    }

    /// Index in `planes` of the side-channel record tagged with
    /// `stride_tag`, searching the trailing side-channel run only (the
    /// last match wins if a malformed frame carries duplicates).
    fn side_channel_index(&self, stride_tag: usize) -> Option<usize> {
        let start = self.side_channel_run_start();
        self.planes[start..]
            .iter()
            .rposition(|p| p.stride == stride_tag)
            .map(|i| start + i)
    }

    /// Remove every record tagged `stride_tag` from the trailing
    /// side-channel run, returning the data of the record the readers
    /// would have reported (the last match — consistent with
    /// [`side_channel_index`](Self::side_channel_index)).
    fn remove_side_channel(&mut self, stride_tag: usize) -> Option<Vec<u8>> {
        let reported = self
            .side_channel_index(stride_tag)
            .map(|i| self.planes.remove(i).data);
        while let Some(i) = self.side_channel_index(stride_tag) {
            self.planes.remove(i);
        }
        reported
    }

    /// The frame's attached palette, if any.
    ///
    /// Returns the raw bytes of the palette side-channel (see the
    /// type-level docs): packed 3-byte RGB entries, entry `i` at bytes
    /// `3*i .. 3*i + 3` in R, G, B order. A full
    /// [`Pal8`](crate::PixelFormat::Pal8) table is 256 entries
    /// (768 bytes), but producers may attach fewer when the source
    /// image declares a shorter table; indices at or beyond
    /// `len / 3` are undefined by this frame and up to the consumer's
    /// missing-entry policy (typically black).
    pub fn palette(&self) -> Option<&[u8]> {
        self.side_channel_index(0)
            .map(|i| self.planes[i].data.as_slice())
    }

    /// The RGB triplet for palette entry `index`, or `None` when no
    /// palette is attached or the attached table is too short to cover
    /// `index`. Sugar over [`palette`](Self::palette) for per-pixel
    /// lookups.
    pub fn palette_rgb(&self, index: u8) -> Option<[u8; 3]> {
        let pal = self.palette()?;
        let at = usize::from(index) * 3;
        let entry = pal.get(at..at + 3)?;
        Some([entry[0], entry[1], entry[2]])
    }

    /// Attach (or replace) the frame's palette side-channel.
    ///
    /// `rgb` is packed 3-byte RGB entries — see
    /// [`palette`](Self::palette) for the exact layout; pass a length
    /// that is a multiple of 3 (up to 768 bytes for a full 256-entry
    /// [`Pal8`](crate::PixelFormat::Pal8) table). The bytes are stored
    /// verbatim. An empty `rgb` removes any attached palette instead
    /// (the sentinel requires non-empty data), leaving the frame
    /// exactly as it was before any palette was attached.
    pub fn set_palette(&mut self, rgb: Vec<u8>) {
        self.remove_side_channel(0);
        if !rgb.is_empty() {
            self.planes.push(VideoPlane {
                stride: 0,
                data: rgb,
            });
        }
    }

    /// Builder-style counterpart to [`set_palette`](Self::set_palette)
    /// for construction chains:
    /// `VideoFrame { pts, planes }.with_palette(rgb)`.
    pub fn with_palette(mut self, rgb: Vec<u8>) -> Self {
        self.set_palette(rgb);
        self
    }

    /// Detach and return the frame's palette side-channel, if any.
    /// Afterwards the frame carries no palette (any other side-channel
    /// record is left in place).
    pub fn take_palette(&mut self) -> Option<Vec<u8>> {
        self.remove_side_channel(0)
    }

    /// The frame's attached per-plane significant-bits record, if any.
    ///
    /// Returns the raw bytes of the significant-bits side-channel (see
    /// the type-level docs): byte `k` is the number of significant bits
    /// in the samples of image plane `k`, in plane order. This lets a
    /// producer express **mixed** per-plane depths that no single
    /// [`PixelFormat`](crate::PixelFormat) variant can name — e.g. a
    /// wavelet codec's custom signal range with 12-bit luma and 10-bit
    /// chroma, stored on a `Yuv444P12Le` surface with an attached
    /// record of `[12, 10, 10]`.
    ///
    /// # Semantics
    ///
    /// - Values are **LSB-anchored**: a plane with `b` significant bits
    ///   keeps its sample values in the low `b` bits of each storage
    ///   word, with the upper bits zero — the same convention as this
    ///   crate's partial-depth formats (`Gray10Le`, `Yuv420P10Le`,
    ///   `Gbrp12Le`, …, each documented as "uses the low N bits of a
    ///   16-bit word"). Full-scale for `b` significant bits is
    ///   `(1 << b) - 1`.
    /// - Each value must satisfy `1 ≤ b ≤ 8 × storage-word-bytes` of
    ///   the frame's pixel format (so at most 8 for byte-sized planes,
    ///   16 for LE-16-bit-word planes). The record refines the storage
    ///   format's *significant* depth; it never changes the storage
    ///   word size or plane geometry.
    /// - A record shorter than the image-plane count (or a missing
    ///   record) leaves the uncovered planes at the pixel format's own
    ///   documented depth. Bytes are stored verbatim; out-of-range
    ///   values are a producer bug and consumers may clamp or reject
    ///   them.
    pub fn significant_bits(&self) -> Option<&[u8]> {
        self.side_channel_index(SIGNIFICANT_BITS_STRIDE)
            .map(|i| self.planes[i].data.as_slice())
    }

    /// The significant-bit count for image plane `plane`, or `None`
    /// when no record is attached or the attached record is too short
    /// to cover `plane` (fall back to the pixel format's own depth).
    /// Sugar over [`significant_bits`](Self::significant_bits) for
    /// per-plane lookups.
    pub fn plane_significant_bits(&self, plane: usize) -> Option<u8> {
        self.significant_bits()?.get(plane).copied()
    }

    /// Attach (or replace) the frame's per-plane significant-bits
    /// side-channel.
    ///
    /// `bits` holds one byte per image plane, in plane order — see
    /// [`significant_bits`](Self::significant_bits) for the exact
    /// semantics (LSB-anchored values, `1 ≤ b ≤ storage word bits`).
    /// The bytes are stored verbatim. An empty `bits` removes any
    /// attached record instead (the sentinel requires non-empty data),
    /// leaving the frame exactly as it was before any record was
    /// attached. Any attached palette is unaffected.
    pub fn set_significant_bits(&mut self, bits: Vec<u8>) {
        self.remove_side_channel(SIGNIFICANT_BITS_STRIDE);
        if !bits.is_empty() {
            self.planes.push(VideoPlane {
                stride: SIGNIFICANT_BITS_STRIDE,
                data: bits,
            });
        }
    }

    /// Builder-style counterpart to
    /// [`set_significant_bits`](Self::set_significant_bits) for
    /// construction chains:
    /// `VideoFrame { pts, planes }.with_significant_bits(bits)`.
    pub fn with_significant_bits(mut self, bits: Vec<u8>) -> Self {
        self.set_significant_bits(bits);
        self
    }

    /// Detach and return the frame's per-plane significant-bits
    /// side-channel, if any. Afterwards the frame carries no
    /// significant-bits record (any attached palette is left in place).
    pub fn take_significant_bits(&mut self) -> Option<Vec<u8>> {
        self.remove_side_channel(SIGNIFICANT_BITS_STRIDE)
    }

    /// The frame's image planes — `planes` with the trailing
    /// side-channel entries (palette, significant bits) excluded.
    /// Prefer this over indexing `planes` directly in code that
    /// handles side-channel-capable frames.
    pub fn image_planes(&self) -> &[VideoPlane] {
        &self.planes[..self.image_plane_count()]
    }

    /// Number of image planes (excludes every side-channel entry).
    /// Matches the stream pixel format's
    /// [`plane_count`](crate::PixelFormat::plane_count) for well-formed
    /// frames.
    pub fn image_plane_count(&self) -> usize {
        self.side_channel_run_start()
    }
}

/// One plane of a [`VideoFrame`]: row-major sample bytes plus the
/// stride between rows.
///
/// An entry with non-empty `data` and a `stride` of `0` or `usize::MAX`
/// is not an image plane: it is a side-channel record (palette and
/// per-plane significant bits respectively) described on [`VideoFrame`]
/// — only meaningful within the trailing run of `VideoFrame::planes`.
#[derive(Clone, Debug)]
pub struct VideoPlane {
    /// Bytes per row in `data`.
    pub stride: usize,
    /// Raw plane bytes, `stride × rows` long (rows may carry padding
    /// beyond the visible width).
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_frame() -> VideoFrame {
        // 4×2 Gray8 image plane.
        VideoFrame {
            pts: Some(7),
            planes: vec![VideoPlane {
                stride: 4,
                data: vec![0u8; 8],
            }],
        }
    }

    /// A full 256-entry table where entry i is (i, !i, i^0x55).
    fn full_palette() -> Vec<u8> {
        (0u16..256)
            .flat_map(|i| {
                let i = i as u8;
                [i, !i, i ^ 0x55]
            })
            .collect()
    }

    #[test]
    fn frame_without_palette_reports_none_and_full_image_planes() {
        let f = gray_frame();
        assert_eq!(f.palette(), None);
        assert_eq!(f.palette_rgb(0), None);
        assert_eq!(f.image_plane_count(), 1);
        assert_eq!(f.image_planes().len(), 1);
        assert_eq!(f.image_planes()[0].stride, 4);
    }

    #[test]
    fn set_palette_round_trips_and_keeps_image_planes_intact() {
        let mut f = gray_frame();
        let pal = full_palette();
        f.set_palette(pal.clone());

        assert_eq!(f.palette(), Some(pal.as_slice()));
        // Image-plane view is unchanged by the side-channel.
        assert_eq!(f.image_plane_count(), 1);
        assert_eq!(f.image_planes()[0].data.len(), 8);
        // The raw field sees the sentinel entry at the tail.
        assert_eq!(f.planes.len(), 2);
        assert_eq!(f.planes[1].stride, 0);

        // Entry lookup: entry i is (i, !i, i ^ 0x55) by construction.
        assert_eq!(f.palette_rgb(0), Some([0x00, 0xFF, 0x55]));
        assert_eq!(f.palette_rgb(0xAB), Some([0xAB, 0x54, 0xFE]));
        assert_eq!(f.palette_rgb(255), Some([0xFF, 0x00, 0xAA]));
    }

    #[test]
    fn set_palette_replaces_existing_table() {
        let mut f = gray_frame();
        f.set_palette(vec![1, 2, 3]);
        f.set_palette(vec![9, 8, 7, 6, 5, 4]);
        // Replacement, not stacking: one image plane + one sentinel.
        assert_eq!(f.planes.len(), 2);
        assert_eq!(f.palette(), Some(&[9, 8, 7, 6, 5, 4][..]));
        assert_eq!(f.palette_rgb(1), Some([6, 5, 4]));
    }

    #[test]
    fn short_palette_covers_only_its_entries() {
        let f = gray_frame().with_palette(vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(f.palette_rgb(0), Some([10, 20, 30]));
        assert_eq!(f.palette_rgb(1), Some([40, 50, 60]));
        // Beyond the table: undefined by the frame → None.
        assert_eq!(f.palette_rgb(2), None);
        assert_eq!(f.palette_rgb(255), None);
    }

    #[test]
    fn empty_palette_clears_and_take_palette_detaches() {
        let mut f = gray_frame();
        f.set_palette(vec![1, 2, 3]);
        assert!(f.palette().is_some());

        // Empty input removes the side-channel entirely.
        f.set_palette(Vec::new());
        assert_eq!(f.palette(), None);
        assert_eq!(f.planes.len(), 1);

        // take_palette detaches and returns the bytes.
        f.set_palette(vec![4, 5, 6]);
        assert_eq!(f.take_palette(), Some(vec![4, 5, 6]));
        assert_eq!(f.palette(), None);
        assert_eq!(f.take_palette(), None);
        assert_eq!(f.planes.len(), 1);
    }

    #[test]
    fn zero_stride_empty_plane_is_not_mistaken_for_a_palette() {
        // stride == 0 with EMPTY data is the degenerate (but
        // contract-consistent) empty image plane, not the sentinel.
        let f = VideoFrame {
            pts: None,
            planes: vec![
                VideoPlane {
                    stride: 4,
                    data: vec![0u8; 8],
                },
                VideoPlane {
                    stride: 0,
                    data: Vec::new(),
                },
            ],
        };
        assert_eq!(f.palette(), None);
        assert_eq!(f.image_plane_count(), 2);
    }

    #[test]
    fn palette_on_frame_without_image_planes() {
        // A palette can be attached before pixel planes exist (encoder
        // scaffolding); the image-plane view is then empty.
        let f = VideoFrame {
            pts: None,
            planes: Vec::new(),
        }
        .with_palette(vec![1, 2, 3]);
        assert_eq!(f.palette(), Some(&[1, 2, 3][..]));
        assert_eq!(f.image_plane_count(), 0);
        assert!(f.image_planes().is_empty());
    }

    #[test]
    fn frame_without_significant_bits_reports_none() {
        let f = gray_frame();
        assert_eq!(f.significant_bits(), None);
        assert_eq!(f.plane_significant_bits(0), None);
        assert_eq!(f.image_plane_count(), 1);
    }

    #[test]
    fn set_significant_bits_round_trips_and_keeps_image_planes_intact() {
        // A 12-bit-luma / 10-bit-chroma mixed-depth frame (the VC-2
        // custom-signal-range shape that motivated the record).
        let mut f = VideoFrame {
            pts: Some(3),
            planes: vec![
                VideoPlane {
                    stride: 8,
                    data: vec![0u8; 16],
                },
                VideoPlane {
                    stride: 8,
                    data: vec![0u8; 16],
                },
                VideoPlane {
                    stride: 8,
                    data: vec![0u8; 16],
                },
            ],
        };
        f.set_significant_bits(vec![12, 10, 10]);

        assert_eq!(f.significant_bits(), Some(&[12, 10, 10][..]));
        assert_eq!(f.plane_significant_bits(0), Some(12));
        assert_eq!(f.plane_significant_bits(1), Some(10));
        assert_eq!(f.plane_significant_bits(2), Some(10));
        // Beyond the record: fall back to the format default → None.
        assert_eq!(f.plane_significant_bits(3), None);

        // Image-plane view is unchanged by the side-channel.
        assert_eq!(f.image_plane_count(), 3);
        assert_eq!(f.image_planes().len(), 3);
        // The raw field sees the sentinel entry at the tail.
        assert_eq!(f.planes.len(), 4);
        assert_eq!(f.planes[3].stride, usize::MAX);
    }

    #[test]
    fn set_significant_bits_replaces_and_empty_clears_and_take_detaches() {
        let mut f = gray_frame();
        f.set_significant_bits(vec![7]);
        f.set_significant_bits(vec![6]);
        // Replacement, not stacking.
        assert_eq!(f.planes.len(), 2);
        assert_eq!(f.significant_bits(), Some(&[6][..]));

        // Empty input removes the side-channel entirely.
        f.set_significant_bits(Vec::new());
        assert_eq!(f.significant_bits(), None);
        assert_eq!(f.planes.len(), 1);

        // take_significant_bits detaches and returns the bytes.
        f.set_significant_bits(vec![5]);
        assert_eq!(f.take_significant_bits(), Some(vec![5]));
        assert_eq!(f.significant_bits(), None);
        assert_eq!(f.take_significant_bits(), None);
        assert_eq!(f.planes.len(), 1);
    }

    #[test]
    fn palette_and_significant_bits_compose_in_either_order() {
        // Palette first, then depths.
        let mut f = gray_frame()
            .with_palette(vec![1, 2, 3])
            .with_significant_bits(vec![8]);
        assert_eq!(f.palette(), Some(&[1, 2, 3][..]));
        assert_eq!(f.significant_bits(), Some(&[8][..]));
        assert_eq!(f.image_plane_count(), 1);
        assert_eq!(f.planes.len(), 3);

        // Replacing one record must not disturb the other, regardless
        // of which currently sits at the tail.
        f.set_palette(vec![9, 8, 7]);
        assert_eq!(f.palette(), Some(&[9, 8, 7][..]));
        assert_eq!(f.significant_bits(), Some(&[8][..]));
        f.set_significant_bits(vec![7]);
        assert_eq!(f.palette(), Some(&[9, 8, 7][..]));
        assert_eq!(f.significant_bits(), Some(&[7][..]));
        assert_eq!(f.image_plane_count(), 1);

        // Depths first, then palette.
        let g = gray_frame()
            .with_significant_bits(vec![4])
            .with_palette(full_palette());
        assert_eq!(g.significant_bits(), Some(&[4][..]));
        assert_eq!(g.palette_rgb(0), Some([0x00, 0xFF, 0x55]));
        assert_eq!(g.image_plane_count(), 1);

        // Detaching one leaves the other attached.
        let mut h = g;
        assert_eq!(h.take_significant_bits(), Some(vec![4]));
        assert_eq!(h.significant_bits(), None);
        assert_eq!(h.palette().map(<[u8]>::len), Some(768));
        assert_eq!(h.take_palette().map(|p| p.len()), Some(768));
        assert_eq!(h.planes.len(), 1);
        assert_eq!(h.image_plane_count(), 1);
    }

    #[test]
    fn max_stride_empty_plane_is_not_mistaken_for_significant_bits() {
        // stride == usize::MAX with EMPTY data is not the sentinel
        // (mirroring the palette rule: sentinels require non-empty
        // data). Degenerate, but must not be misread as a record.
        let f = VideoFrame {
            pts: None,
            planes: vec![
                VideoPlane {
                    stride: 4,
                    data: vec![0u8; 8],
                },
                VideoPlane {
                    stride: usize::MAX,
                    data: Vec::new(),
                },
            ],
        };
        assert_eq!(f.significant_bits(), None);
        assert_eq!(f.image_plane_count(), 2);
    }

    #[test]
    fn significant_bits_on_frame_without_image_planes() {
        // Like the palette, the record can be attached before pixel
        // planes exist (encoder scaffolding).
        let f = VideoFrame {
            pts: None,
            planes: Vec::new(),
        }
        .with_significant_bits(vec![12, 10, 10]);
        assert_eq!(f.significant_bits(), Some(&[12, 10, 10][..]));
        assert_eq!(f.image_plane_count(), 0);
        assert!(f.image_planes().is_empty());
    }

    #[test]
    fn side_channels_survive_clone_and_frame_wrapping() {
        let f = gray_frame()
            .with_palette(vec![1, 2, 3])
            .with_significant_bits(vec![6]);
        let cloned = f.clone();
        assert_eq!(cloned.palette(), f.palette());
        assert_eq!(cloned.significant_bits(), f.significant_bits());

        let wrapped = Frame::Video(cloned);
        assert_eq!(wrapped.pts(), Some(7));
        if let Frame::Video(v) = wrapped {
            assert_eq!(v.palette(), Some(&[1, 2, 3][..]));
            assert_eq!(v.significant_bits(), Some(&[6][..]));
        } else {
            unreachable!("wrapped as Video above");
        }
    }

    #[test]
    fn palette_survives_clone_and_frame_wrapping() {
        let f = gray_frame().with_palette(full_palette());
        let cloned = f.clone();
        assert_eq!(cloned.palette(), f.palette());

        // Through the Frame enum, pts and palette both survive.
        let wrapped = Frame::Video(cloned);
        assert_eq!(wrapped.pts(), Some(7));
        if let Frame::Video(v) = wrapped {
            assert_eq!(v.palette().map(<[u8]>::len), Some(768));
        } else {
            unreachable!("wrapped as Video above");
        }
    }
}
