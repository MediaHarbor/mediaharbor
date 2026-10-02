//! `raw_data_block()` syntactic walker.
//!
//! ISO/IEC 14496-3 §4.4.2.1 defines `raw_data_block()` as a sequence
//! of *syntactic elements*, each prefixed by a 3-bit `id_syn_ele`
//! identifier (Table 4.71). The element types are:
//!
//! | id (binary) | id (decimal) | name | role                                       |
//! |-------------|--------------|------|--------------------------------------------|
//! | `0b000`     | 0            | SCE  | single-channel element (mono)              |
//! | `0b001`     | 1            | CPE  | channel-pair element                       |
//! | `0b010`     | 2            | CCE  | coupling channel element                   |
//! | `0b011`     | 3            | LFE  | low-frequency-effects element              |
//! | `0b100`     | 4            | DSE  | data stream element                        |
//! | `0b101`     | 5            | PCE  | program config element                     |
//! | `0b110`     | 6            | FIL  | fill element (padding / extension payload) |
//! | `0b111`     | 7            | END  | block terminator                           |
//!
//! After the terminating `END`, ISO/IEC 14496-3 §4.4.2.1 requires the
//! decoder to byte-align the bit-reader before the next
//! `raw_data_block()` begins. The walker performs that alignment so
//! the next call after `END` resumes on a fresh byte boundary.
//!
//! ## Phase 1 scope
//!
//! This module is the **syntactic skeleton** — the walker emits an
//! [`Element`] per `id_syn_ele` it encounters and stops at `END`.
//! Per-element bodies are handled as follows:
//!
//! * **SCE / CPE / CCE / LFE**: the walker reads the mandatory 4-bit
//!   `element_instance_tag` and then *stops body parsing*. The
//!   consumer must advance the [`BitReader`](oxideav_core::bits::BitReader)
//!   past the channel-element body itself; Phase 2 will absorb that
//!   logic. The emitted [`Element::ChannelElement`] carries the
//!   element kind and its tag.
//! * **FIL**: parsed as ISO/IEC 14496-3 §4.4.2.7 — 4-bit
//!   `count`, optional 8-bit `esc_count` escape (when `count == 15`,
//!   the real byte count is `count + esc_count − 1`), then *count*
//!   bytes of `extension_payload` which are skipped without
//!   interpretation. The emitted [`Element::Fill`] reports the byte
//!   length skipped.
//! * **DSE**: parsed as ISO/IEC 14496-3 §4.4.2.5 — 4-bit
//!   `element_instance_tag`, 1-bit `data_byte_align_flag`, 8-bit
//!   `count`, optional 8-bit `esc_count`, byte-align (if flag set),
//!   then *count* bytes of `data_stream_byte[]`.
//! * **PCE**: parsed via [`crate::pce::Pce::parse`] with an
//!   `origin_bit_offset` of `0` (the standalone-in-`raw_data_block`
//!   form has no enclosing ASC, so the Table 4.2 `byte_alignment()`
//!   resolves to the absolute byte boundary). The walker emits
//!   [`Element::ProgramConfig`] carrying the resolved
//!   [`crate::pce::Pce`].
//! * **END**: emits [`Element::End`] and byte-aligns the reader.
//!   Subsequent calls return `None`.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::pce::Pce;
use crate::{Error, Result};

/// Syntactic element identifier — the 3-bit `id_syn_ele` field
/// defined in ISO/IEC 14496-3 Table 4.71.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IdSynEle {
    /// `0b000` — single-channel element.
    Sce = 0,
    /// `0b001` — channel-pair element.
    Cpe = 1,
    /// `0b010` — coupling channel element.
    Cce = 2,
    /// `0b011` — low-frequency-effects element.
    Lfe = 3,
    /// `0b100` — data stream element.
    Dse = 4,
    /// `0b101` — program config element.
    Pce = 5,
    /// `0b110` — fill element.
    Fil = 6,
    /// `0b111` — raw-data-block terminator.
    End = 7,
}

impl IdSynEle {
    /// Map a 3-bit wire value (0..=7) to the corresponding variant.
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0b111 {
            0 => IdSynEle::Sce,
            1 => IdSynEle::Cpe,
            2 => IdSynEle::Cce,
            3 => IdSynEle::Lfe,
            4 => IdSynEle::Dse,
            5 => IdSynEle::Pce,
            6 => IdSynEle::Fil,
            _ => IdSynEle::End,
        }
    }

    /// Short upper-case name as used in the spec table and the
    /// AAC_TRACE fixture corpus (`SCE`, `CPE`, `CCE`, `LFE`, `DSE`,
    /// `PCE`, `FIL`, `END`).
    pub fn name(self) -> &'static str {
        match self {
            IdSynEle::Sce => "SCE",
            IdSynEle::Cpe => "CPE",
            IdSynEle::Cce => "CCE",
            IdSynEle::Lfe => "LFE",
            IdSynEle::Dse => "DSE",
            IdSynEle::Pce => "PCE",
            IdSynEle::Fil => "FIL",
            IdSynEle::End => "END",
        }
    }
}

/// An event emitted by [`Walker::next_element`].
///
/// The walker emits exactly one event per `id_syn_ele` it consumes
/// and stops at `END`. For non-`End` events the bit-reader position
/// after the call reflects the bytes the walker itself consumed
/// (header + any per-element bookkeeping it parses); see the
/// per-variant docs for which bytes have been skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    /// SCE / CPE / CCE / LFE — channel element. The walker has
    /// consumed the 3-bit `id_syn_ele` and the 4-bit
    /// `element_instance_tag`. The channel-element body (`ics_info`,
    /// section data, scale factors, spectral data, …) starts at the
    /// current bit-reader position and is **not** parsed in Phase 1.
    ChannelElement {
        /// The channel element variant (`Sce`, `Cpe`, `Cce`, or
        /// `Lfe`).
        kind: IdSynEle,
        /// The 4-bit `element_instance_tag` read from the wire.
        element_instance_tag: u8,
    },
    /// FIL — fill element. The walker has consumed the 3-bit
    /// `id_syn_ele`, the 4-bit `count`, the optional 8-bit
    /// `esc_count`, and the resulting *count* `extension_payload`
    /// bytes.
    Fill {
        /// Total `extension_payload` bytes skipped (`count` after
        /// optional escape expansion).
        payload_bytes: u32,
    },
    /// DSE — data stream element. The walker has consumed the
    /// header (3-bit `id_syn_ele`, 4-bit `element_instance_tag`,
    /// 1-bit `data_byte_align_flag`, 8-bit `count`, optional 8-bit
    /// `esc_count`, optional byte-align) and the resulting *count*
    /// `data_stream_byte[]` values.
    Data {
        /// The 4-bit `element_instance_tag` read from the wire.
        element_instance_tag: u8,
        /// `true` ⇔ a `data_byte_align_flag == 1` was processed and
        /// the bit-reader was byte-aligned before the payload.
        byte_align_flag: bool,
        /// Total `data_stream_byte[]` bytes skipped (`count` after
        /// optional escape expansion).
        payload_bytes: u32,
    },
    /// PCE — program config element. The walker has consumed the
    /// 3-bit `id_syn_ele` and the entire PCE body per
    /// [`Pce::parse`] (`origin_bit_offset = 0` — see
    /// [`crate::pce`] for the standalone vs ASC-embedded handling
    /// of the trailing `byte_alignment()`).
    ProgramConfig(Pce),
    /// END (`0b111`) — the raw-data-block terminator. The walker
    /// has consumed the 3-bit `id_syn_ele` and byte-aligned the
    /// bit-reader (ISO/IEC 14496-3 §4.4.2.1).
    End,
}

/// Walker over a `raw_data_block()` payload.
///
/// Drive the walker by calling [`Walker::next_element`] in a loop
/// until it returns either an [`Element::End`] event or `None`
/// (input exhausted before reaching `END`). See the [module
/// docs](self) for the per-element body-skipping rules and what
/// the walker currently does not parse.
pub struct Walker<'a, 'b> {
    reader: &'b mut BitReader<'a>,
    finished: bool,
}

impl<'a, 'b> core::fmt::Debug for Walker<'a, 'b> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Walker")
            .field("finished", &self.finished)
            .field("bit_position", &self.reader.bit_position())
            .finish()
    }
}

impl<'a, 'b> Walker<'a, 'b> {
    /// Bind a walker to an existing [`BitReader`] positioned at the
    /// first byte of a `raw_data_block()` payload.
    pub fn new(reader: &'b mut BitReader<'a>) -> Self {
        Self {
            reader,
            finished: false,
        }
    }

    /// Read the next syntactic element. Returns `Ok(Some(_))` for
    /// every non-terminating element, `Ok(Some(Element::End))` once
    /// (and the walker becomes `finished`), and `Ok(None)` for any
    /// further calls after `End`.
    ///
    /// Errors out with [`Error::UnsupportedElementSkip`] when the
    /// next `id_syn_ele` would require body parsing Phase 1 has
    /// not landed yet. As of this round, PCE is fully parsed
    /// (round 126) and FIL / DSE are skipped (round 121); only the
    /// channel-element bodies (SCE/CPE/CCE/LFE) remain deferred,
    /// and even those return [`Element::ChannelElement`] for the
    /// header — the caller must advance the bit-reader past the
    /// body itself if more than one element is needed in a single
    /// `raw_data_block()`.
    pub fn next_element(&mut self) -> Result<Option<Element>> {
        self.next_element_impl(true)
    }

    /// [`Self::next_element`], except a FIL element's
    /// `extension_payload` body is **left unconsumed**: the returned
    /// [`Element::Fill`] reports the byte count and the bit-reader
    /// stays at the first extension-payload bit, so the caller can
    /// parse the Table 4.51 `extension_payload()` chain itself (e.g.
    /// to route an `EXT_SBR_DATA` payload into the SBR decoder). The
    /// caller **must** consume exactly `payload_bytes` bytes worth of
    /// bits before the next call.
    pub fn next_element_keep_fill(&mut self) -> Result<Option<Element>> {
        self.next_element_impl(false)
    }

    fn next_element_impl(&mut self, consume_fill: bool) -> Result<Option<Element>> {
        if self.finished {
            return Ok(None);
        }

        let id_bits = self.reader.read_u32(3).map_err(|_| Error::UnexpectedEnd)? as u8;
        let id = IdSynEle::from_bits(id_bits);

        match id {
            IdSynEle::Sce | IdSynEle::Cpe | IdSynEle::Cce | IdSynEle::Lfe => {
                let element_instance_tag =
                    self.reader.read_u32(4).map_err(|_| Error::UnexpectedEnd)? as u8;
                Ok(Some(Element::ChannelElement {
                    kind: id,
                    element_instance_tag,
                }))
            }
            IdSynEle::Fil => {
                let payload_bytes = self.read_fill_count()?;
                if consume_fill {
                    self.skip_bytes(payload_bytes)?;
                }
                Ok(Some(Element::Fill { payload_bytes }))
            }
            IdSynEle::Dse => {
                let element_instance_tag =
                    self.reader.read_u32(4).map_err(|_| Error::UnexpectedEnd)? as u8;
                let byte_align_flag = self.reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
                let payload_bytes = self.read_data_count()?;
                if byte_align_flag {
                    self.reader.align_to_byte();
                }
                self.skip_bytes(payload_bytes)?;
                Ok(Some(Element::Data {
                    element_instance_tag,
                    byte_align_flag,
                    payload_bytes,
                }))
            }
            IdSynEle::Pce => {
                // Standalone PCE inside a raw_data_block: align the
                // PCE's byte_alignment() to the absolute byte
                // boundary (origin_bit_offset == 0). The ASC-inline
                // variant uses the surrounding ASC origin instead.
                let pce = Pce::parse(self.reader, 0)?;
                Ok(Some(Element::ProgramConfig(pce)))
            }
            IdSynEle::End => {
                self.reader.align_to_byte();
                self.finished = true;
                Ok(Some(Element::End))
            }
        }
    }

    /// `true` once an [`Element::End`] event has been returned.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Fill-element byte-count read per ISO/IEC 14496-3 §4.4.2.7.
    /// 4-bit `count`; if `count == 15`, an 8-bit `esc_count` follows
    /// and the resulting count is `count + esc_count − 1`.
    fn read_fill_count(&mut self) -> Result<u32> {
        let count = self.reader.read_u32(4).map_err(|_| Error::UnexpectedEnd)?;
        if count == 15 {
            let esc = self.reader.read_u32(8).map_err(|_| Error::UnexpectedEnd)?;
            // §4.4.2.7: `cnt = esc_count + 15 - 1`.
            Ok(esc + 15 - 1)
        } else {
            Ok(count)
        }
    }

    /// Data-stream-element byte-count read per ISO/IEC 14496-3
    /// §4.4.2.5. 8-bit `count`; if `count == 255`, an 8-bit
    /// `esc_count` follows and the resulting count is
    /// `count + esc_count`.
    fn read_data_count(&mut self) -> Result<u32> {
        let count = self.reader.read_u32(8).map_err(|_| Error::UnexpectedEnd)?;
        if count == 255 {
            let esc = self.reader.read_u32(8).map_err(|_| Error::UnexpectedEnd)?;
            Ok(count + esc)
        } else {
            Ok(count)
        }
    }

    /// Skip `n` whole bytes via the bit-reader.
    fn skip_bytes(&mut self, n: u32) -> Result<()> {
        // Multiplication is safe within u32 because §4.4.2.5 caps
        // `count` at 2 × 255 = 510 and §4.4.2.7 caps `cnt` at
        // 15 + 255 − 1 = 269, well below `u32::MAX / 8`.
        let bits = n.saturating_mul(8);
        self.reader.skip(bits).map_err(|_| Error::UnexpectedEnd)
    }
}

// ===================================================================
// raw_data_block() frame assembler — encoder primitive
// ===================================================================
//
// Round 160 lands the symmetric encoder side: a [`FrameAssembler`]
// that composes the existing typed writers into a complete
// `raw_data_block()` byte stream per ISO/IEC 14496-3 §4.4.2.1, the
// inverse of [`Walker`]. The assembler accepts:
//
// * [`FrameAssembler::push_channel_header`] — emits the 3-bit
//   `id_syn_ele` (`SCE` / `CPE` / `CCE` / `LFE`) + 4-bit
//   `element_instance_tag`. The channel-element *body*
//   (`ics_info` → `section_data` → `scale_factor_data` → optional
//   `pulse_data` / `tns_data` / `gain_control_data` → `spectral_data`)
//   is not internalised yet; the caller is responsible for serialising
//   it via the existing per-tool writers (`IcsInfo::write`,
//   `SectionData::write`, `ScaleFactorData::write`, `PulseData::write`,
//   `TnsData::write`, …). [`FrameAssembler::push_channel_body_bits`]
//   appends a pre-serialised body as a bit-slice immediately after a
//   channel header.
//
// * [`FrameAssembler::push_fill`] — emits a FIL element per §4.4.2.7,
//   including the 8-bit `esc_count` escape when `payload_bytes >= 15`
//   (resulting wire `count = 15` + `esc_count = payload_bytes - 15 + 1`
//   — the inverse of `read_fill_count`'s `cnt = esc_count + 15 - 1`).
//
// * [`FrameAssembler::push_data`] — emits a DSE element per §4.4.2.5,
//   honouring `data_byte_align_flag` (which, when set, byte-aligns
//   *before* the payload bytes per §4.4.2.5) and the 8-bit `esc_count`
//   escape when `payload_bytes >= 255` (resulting wire `count = 255` +
//   `esc_count = payload_bytes - 255` — the inverse of
//   `read_data_count`'s `cnt = count + esc_count`).
//
// * [`FrameAssembler::push_end`] — emits the 3-bit `END` terminator
//   and byte-aligns to the next byte boundary per §4.4.2.1.
//
// PCE encoding is deferred — [`Pce`] has no `write` primitive yet, and
// adding one is a separate round's worth of work (Tables 4.4 / 4.5
// front/side/back/lfe element selects, mono / stereo / matrix
// mix-down hints, comment field, plus the relative-origin
// `byte_alignment()` per Table 4.2 Note 1).
//
// The §4.4.2.1 normative constraint that exactly one `END` element
// terminates the block (and that no further elements may follow) is
// enforced by the type-state: [`FrameAssembler::push_end`] consumes
// `self` and returns the finished [`Vec<u8>`] (calling any other
// `push_*` after END is a compile-time error).

/// Encoder-side frame assembler for `raw_data_block()` per ISO/IEC
/// 14496-3 §4.4.2.1 — the bit-exact inverse of [`Walker`].
///
/// Construct via [`FrameAssembler::new`] or
/// [`FrameAssembler::with_capacity`], push elements with the
/// `push_*` family in wire order, then finish with
/// [`FrameAssembler::push_end`] which consumes the assembler and
/// returns the byte-aligned frame. END is mandatory — dropping a
/// non-finished assembler discards the in-progress frame.
///
/// ## Composition with the existing typed writers
///
/// Channel-element *headers* are emitted by
/// [`FrameAssembler::push_channel_header`]. The channel-element
/// *body* — `ics_info` → `section_data` → `scale_factor_data` →
/// optional `pulse_data` / `tns_data` / `gain_control_data` →
/// `spectral_data` — has no single round-160 writer. Callers
/// serialise the body separately via the existing tool writers
/// ([`crate::ics_info::IcsInfo::write`],
/// [`crate::section_data::SectionData::write`],
/// [`crate::scale_factor_data::ScaleFactorData::write`],
/// [`crate::pulse_data::PulseData::write`],
/// [`crate::tns_data::TnsData::write`]) into an auxiliary
/// [`BitWriter`] and append the resulting bits to the frame via
/// [`FrameAssembler::push_channel_body_bits`]. This keeps the
/// frame-level concern (element ordering + sync + alignment +
/// fill/data escapes + END) separate from the channel-element-level
/// concern (per-tool bit layouts), which round 160 already covers
/// for everything except `gain_control_data` / `spectral_data`.
///
/// ## Why this is `Phase 2`, not `Phase 1`
///
/// The Phase 1 [`Walker`] *consumes* a `raw_data_block()` byte slice
/// produced by an external encoder (typically extracted from an ADTS
/// frame or an MP4 audio sample). Phase 2 adds the inverse — the
/// assembler that *produces* the byte slice that the Phase 1 walker
/// can read back. Together they form a complete §4.4.2.1
/// parse / write cycle for every element type with a bit-exact
/// inverse already in the crate (channel headers, FIL, DSE, END).
pub struct FrameAssembler {
    writer: BitWriter,
}

impl core::fmt::Debug for FrameAssembler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrameAssembler")
            .field("bit_position", &self.writer.bit_position())
            .finish()
    }
}

impl Default for FrameAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAssembler {
    /// Start a new, empty `raw_data_block()` assembler.
    pub fn new() -> Self {
        Self {
            writer: BitWriter::new(),
        }
    }

    /// Start a new assembler whose underlying byte buffer is
    /// pre-reserved for at least `cap` bytes.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            writer: BitWriter::with_capacity(cap),
        }
    }

    /// Current bit position (relative to the start of the frame).
    /// Useful for sizing channel-element bodies.
    pub fn bit_position(&self) -> u64 {
        self.writer.bit_position()
    }

    /// Emit a channel-element header — the 3-bit `id_syn_ele` (one of
    /// `SCE` / `CPE` / `CCE` / `LFE`) followed by the 4-bit
    /// `element_instance_tag` per ISO/IEC 14496-3 §4.4.2.1.
    ///
    /// The channel-element body itself is the caller's responsibility
    /// — see [`FrameAssembler::push_channel_body_bits`] for the
    /// post-header append.
    ///
    /// Returns [`Error::RawDataBlockEncodeInvalid`] when:
    ///
    /// * `kind` is not one of `SCE` / `CPE` / `CCE` / `LFE` (this
    ///   helper is for channel elements only — use
    ///   [`FrameAssembler::push_fill`] / [`FrameAssembler::push_data`]
    ///   / [`FrameAssembler::push_end`] for the other element types,
    ///   each of which has its own bespoke wire layout).
    /// * `element_instance_tag > 0x0f` (4-bit field overflow).
    pub fn push_channel_header(&mut self, kind: IdSynEle, element_instance_tag: u8) -> Result<()> {
        match kind {
            IdSynEle::Sce | IdSynEle::Cpe | IdSynEle::Cce | IdSynEle::Lfe => {}
            _ => return Err(Error::RawDataBlockEncodeInvalid),
        }
        if element_instance_tag > 0x0f {
            return Err(Error::RawDataBlockEncodeInvalid);
        }
        self.writer.write_u32(kind as u32, 3);
        self.writer.write_u32(element_instance_tag as u32, 4);
        Ok(())
    }

    /// Append `bit_count` raw bits from `bits` (read MSB-first) to
    /// the frame — the channel-element body that follows a
    /// [`FrameAssembler::push_channel_header`].
    ///
    /// `bits` is interpreted as an MSB-first packed bit-buffer (the
    /// same byte layout [`BitWriter::finish`] / [`BitReader::new`]
    /// already use throughout the crate). The low `(8 - bit_count %
    /// 8) % 8` bits of the last byte are not consumed and may carry
    /// arbitrary content.
    ///
    /// Returns [`Error::RawDataBlockEncodeInvalid`] when `bit_count`
    /// exceeds `bits.len() * 8`.
    pub fn push_channel_body_bits(&mut self, bits: &[u8], bit_count: u64) -> Result<()> {
        if bit_count > (bits.len() as u64).saturating_mul(8) {
            return Err(Error::RawDataBlockEncodeInvalid);
        }
        let mut remaining = bit_count;
        let mut byte_idx = 0usize;
        // Whole bytes first.
        while remaining >= 8 {
            self.writer.write_byte(bits[byte_idx]);
            byte_idx += 1;
            remaining -= 8;
        }
        // Trailing partial byte: take the high `remaining` bits of
        // the next source byte.
        if remaining > 0 {
            let last = bits[byte_idx];
            let high = (last as u32) >> (8 - remaining);
            self.writer.write_u32(high, remaining as u32);
        }
        Ok(())
    }

    /// Emit a FIL element per ISO/IEC 14496-3 §4.4.2.7 — the 3-bit
    /// `id_syn_ele` (`0b110`), the 4-bit `count`, the optional 8-bit
    /// `esc_count` escape (when `payload_bytes >= 15`), then the
    /// `payload_bytes` of `extension_payload`.
    ///
    /// Escape arithmetic: the parser's `cnt = esc_count + 15 - 1`
    /// (see [`Walker::read_fill_count`]) inverts to `esc_count =
    /// payload_bytes - 15 + 1 = payload_bytes - 14`, so the largest
    /// representable payload is `15 + 255 - 1 = 269` bytes. Larger
    /// fill payloads must be split across multiple FIL elements (as
    /// AAC's bit-reservoir code path does in practice for long fill
    /// runs).
    ///
    /// Returns [`Error::RawDataBlockEncodeInvalid`] when:
    ///
    /// * `payload.len() > 269` (Table 4.57 + escape arithmetic
    ///   ceiling), or
    /// * `payload.len()` exceeds the `bit_count` capacity of the
    ///   surrounding writer (in practice `u32::MAX`).
    pub fn push_fill(&mut self, payload: &[u8]) -> Result<()> {
        let n = payload.len();
        if n > 269 {
            return Err(Error::RawDataBlockEncodeInvalid);
        }
        self.writer.write_u32(IdSynEle::Fil as u32, 3);
        if n < 15 {
            self.writer.write_u32(n as u32, 4);
        } else {
            self.writer.write_u32(15, 4);
            // §4.4.2.7: parser reconstructs `cnt = esc_count + 15 -
            // 1`. The inverse, given `cnt == n`, is
            // `esc_count = n - 15 + 1 = n - 14`. The 8-bit
            // `esc_count` field caps `n` at `15 + 255 - 1 = 269`,
            // which we already rejected above when violated.
            let esc = (n as u32) - 14;
            self.writer.write_u32(esc, 8);
        }
        // Per §4.4.2.7 the payload is *not* required to be
        // byte-aligned — `extension_payload()` is itself a
        // bit-level item — but the walker treats it as `count`
        // whole bytes, mirroring how every conforming encoder we
        // care about ever emits it. The assembler therefore writes
        // the payload as bytes too.
        for &b in payload {
            self.writer.write_byte(b);
        }
        Ok(())
    }

    /// Emit a DSE element per ISO/IEC 14496-3 §4.4.2.5 — the 3-bit
    /// `id_syn_ele` (`0b100`), the 4-bit `element_instance_tag`, the
    /// 1-bit `data_byte_align_flag`, the 8-bit `count`, the optional
    /// 8-bit `esc_count` escape (when `payload_bytes >= 255`),
    /// optionally byte-align (if the flag was set), then the
    /// `payload_bytes` of `data_stream_byte[]`.
    ///
    /// Escape arithmetic: the parser's `cnt = count + esc_count` (see
    /// [`Walker::read_data_count`]) inverts to `esc_count =
    /// payload_bytes - 255`, so the largest representable payload is
    /// `255 + 255 = 510` bytes. Larger data payloads must be split
    /// across multiple DSE elements with the same `tag`.
    ///
    /// Returns [`Error::RawDataBlockEncodeInvalid`] when:
    ///
    /// * `element_instance_tag > 0x0f` (4-bit field overflow), or
    /// * `payload.len() > 510` (the escape arithmetic ceiling above).
    pub fn push_data(
        &mut self,
        element_instance_tag: u8,
        byte_align_flag: bool,
        payload: &[u8],
    ) -> Result<()> {
        if element_instance_tag > 0x0f {
            return Err(Error::RawDataBlockEncodeInvalid);
        }
        let n = payload.len();
        if n > 510 {
            return Err(Error::RawDataBlockEncodeInvalid);
        }
        self.writer.write_u32(IdSynEle::Dse as u32, 3);
        self.writer.write_u32(element_instance_tag as u32, 4);
        self.writer.write_bit(byte_align_flag);
        if n < 255 {
            self.writer.write_u32(n as u32, 8);
        } else {
            // §4.4.2.5: parser reconstructs `cnt = count +
            // esc_count`. The inverse, given `cnt == n` and the
            // escape trigger `count == 255`, is `esc_count = n -
            // 255`. The 8-bit `esc_count` field caps `n` at
            // `255 + 255 = 510`, which we already rejected above
            // when violated.
            self.writer.write_u32(255, 8);
            let esc = (n as u32) - 255;
            self.writer.write_u32(esc, 8);
        }
        if byte_align_flag {
            self.writer.align_to_byte();
        }
        for &b in payload {
            self.writer.write_byte(b);
        }
        Ok(())
    }

    /// Emit a PCE element per ISO/IEC 14496-3 §4.4.1.1 / Table 4.2
    /// — the 3-bit `id_syn_ele` (`0b101`) followed by the full
    /// `program_config_element()` body produced by [`Pce::write`].
    ///
    /// The Table 4.2 Note 1 `byte_alignment()` call inside the PCE
    /// body is *relative to the start of the PCE body* (i.e. the bit
    /// position immediately after the 3-bit `id_syn_ele`). For the
    /// standalone-in-`raw_data_block()` form the PCE-relative origin
    /// is the parser's `origin_bit_offset = 0` (see [`Pce::parse`])
    /// — since [`Pce::write`] reproduces that exact arithmetic, this
    /// helper simply passes `0` and the writer's own
    /// `bit_position` becomes the alignment reference. Bit-exact
    /// inverse of [`Walker::next_element`]'s
    /// [`Element::ProgramConfig`] branch.
    ///
    /// Returns [`Error::PceEncodeInvalid`] propagated from
    /// [`Pce::write`] when any wire field overflows its bit-width.
    pub fn push_pce(&mut self, pce: &Pce) -> Result<()> {
        self.writer.write_u32(IdSynEle::Pce as u32, 3);
        // §4.4.1.1 Note 1: the Table 4.2 byte_alignment() is
        // measured from the start of the PCE body, which is the
        // current writer position *after* the id_syn_ele prefix.
        // The Phase 1 standalone-in-raw_data_block parser hands
        // origin_bit_offset = 0 to `Pce::parse`, which collapses to
        // absolute byte alignment of the reader. The writer mirrors
        // that exact collapse by passing 0 here — the alignment
        // pad inside `Pce::write` will then align to the next
        // absolute byte boundary of the underlying BitWriter.
        pce.write(&mut self.writer, 0)
    }

    /// Emit the terminating `END` element per ISO/IEC 14496-3
    /// §4.4.2.1 — the 3-bit `id_syn_ele` (`0b111`), then a pad-to-
    /// byte-boundary that the [`Walker`] mirrors via
    /// [`BitReader::align_to_byte`]. Consumes the assembler and
    /// returns the finished byte buffer; the final byte is always
    /// fully populated.
    pub fn push_end(mut self) -> Vec<u8> {
        self.writer.write_u32(IdSynEle::End as u32, 3);
        self.writer.align_to_byte();
        self.writer.finish()
    }
}
