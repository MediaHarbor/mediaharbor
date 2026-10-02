//! ADTS — *Audio Data Transport Stream* — fixed-header parser.
//!
//! ISO/IEC 13818-7 §1.A.2.2.1 defines the ADTS fixed-header (28 bits)
//! and §1.A.2.2.2 the variable-header (28 bits), followed by an
//! optional 16-bit CRC and one or more `raw_data_block()` payloads.
//! The header layout, MSB first:
//!
//! | bits | field                                          |
//! |------|------------------------------------------------|
//! | 12   | `syncword` — required `0xFFF`                  |
//! | 1    | `ID` — MPEG-4 (`0`) vs MPEG-2 (`1`)            |
//! | 2    | `layer` — required `0b00`                      |
//! | 1    | `protection_absent` — `1` ⇒ no CRC follows     |
//! | 2    | `profile_ObjectType` — ADTS profile field      |
//! | 4    | `sampling_frequency_index`                     |
//! | 1    | `private_bit`                                  |
//! | 3    | `channel_configuration`                        |
//! | 1    | `original_copy`                                |
//! | 1    | `home`                                         |
//! | 1    | `copyright_identification_bit`                 |
//! | 1    | `copyright_identification_start`               |
//! | 13   | `aac_frame_length` — total frame bytes         |
//! | 11   | `adts_buffer_fullness`                         |
//! | 2    | `number_of_raw_data_blocks_in_frame` (N − 1)   |
//!
//! followed by either:
//!
//! * `protection_absent == 1` ⇒ no CRC; payload starts at byte 7.
//! * `protection_absent == 0` ⇒ 16-bit CRC, payload starts at byte 9.
//!
//! Note the field ADTS calls `profile_ObjectType` is **one less** than
//! the `audioObjectType` defined in ISO/IEC 14496-3 Table 1.16. ADTS
//! `profile_ObjectType == 1` is therefore AAC LC (AOT 2).
//!
//! The `sampling_frequency_index` mapping follows ISO/IEC 14496-3
//! Table 1.18; this module exposes [`AdtsHeader::sample_rate`] which
//! resolves the index to a frequency in Hz.

use oxideav_core::bits::BitReader;

use crate::{Error, Result};

/// Fixed sync pattern at the start of every ADTS frame.
pub const ADTS_SYNCWORD: u16 = 0x0FFF;

/// Header byte count when `protection_absent == 1` (no CRC).
pub const ADTS_HEADER_BYTES_NO_CRC: usize = 7;

/// Header byte count when `protection_absent == 0` (16-bit CRC after
/// the fixed/variable-header pair).
pub const ADTS_HEADER_BYTES_WITH_CRC: usize = 9;

/// ISO/IEC 14496-3 Table 1.18 — `samplingFrequencyIndex`.
///
/// Indices 13 and 14 are reserved. Index 15 signals an explicit
/// 24-bit rate in `AudioSpecificConfig` but is *not* legal in an
/// ADTS header (the ADTS field is 4 bits).
pub const ADTS_SAMPLE_RATES_HZ: [u32; 13] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
    7_350,
];

/// Resolved ADTS fixed + variable header. See module docs for the
/// per-field bit layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdtsHeader {
    /// MPEG version indicator. `false` ⇒ MPEG-4 (`ID = 0`), `true` ⇒
    /// MPEG-2 (`ID = 1`). The decoder behaviour is otherwise
    /// identical; the bit only affects which extensions are legal in
    /// downstream `raw_data_block()` payloads (PNS / LTP are MPEG-4
    /// only).
    pub mpeg_version_mpeg2: bool,

    /// `true` ⇒ no 16-bit CRC follows the variable header. The frame
    /// payload starts at byte 7 instead of byte 9.
    pub protection_absent: bool,

    /// 2-bit ADTS `profile_ObjectType` field as read from the wire.
    /// This is `audioObjectType − 1` per ISO/IEC 13818-7 §1.A.2 (so
    /// `0` = Main, `1` = LC, `2` = SSR, `3` = (LTP) reserved in
    /// 13818-7 but used by 14496-3).
    pub profile: u8,

    /// 4-bit `sampling_frequency_index`. Use [`AdtsHeader::sample_rate`]
    /// for the resolved Hz value.
    pub sampling_frequency_index: u8,

    /// 3-bit `channel_configuration`. ISO/IEC 14496-3 Table 1.19:
    /// `0` ⇒ defined by an inline PCE in the payload, `1` ⇒ mono,
    /// `2` ⇒ stereo, …, `7` ⇒ 7.1 surround.
    pub channel_configuration: u8,

    /// 13-bit `aac_frame_length` — total frame size in bytes,
    /// including the header itself and (if present) the CRC.
    pub aac_frame_length: u16,

    /// 11-bit `adts_buffer_fullness`. `0x7FF` is the spec-mandated
    /// "VBR / unknown" sentinel. Phase 1 does not enforce buffer
    /// modelling.
    pub adts_buffer_fullness: u16,

    /// Number of `raw_data_block()` payloads contained in this frame.
    /// The wire field is `N − 1`; this is the resolved count (≥ 1).
    pub number_of_raw_data_blocks_in_frame: u8,
}

impl AdtsHeader {
    /// Parse an ADTS fixed + variable header from the start of `data`.
    /// On success returns the [`AdtsHeader`] and the byte offset where
    /// the first `raw_data_block()` payload begins (7 if
    /// `protection_absent == 1`, 9 otherwise).
    ///
    /// CRC validation is **deferred**: when `protection_absent == 0`
    /// this routine confirms the CRC bytes are present (i.e. the
    /// input is at least 9 bytes long) but does not verify the CRC
    /// value itself.
    pub fn parse(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < ADTS_HEADER_BYTES_NO_CRC {
            return Err(Error::UnexpectedEnd);
        }

        let mut br = BitReader::new(data);

        // 12-bit syncword
        let sync = br.read_u32(12).map_err(|_| Error::UnexpectedEnd)? as u16;
        if sync != ADTS_SYNCWORD {
            return Err(Error::AdtsSyncNotFound);
        }

        // 1-bit ID, 2-bit layer, 1-bit protection_absent
        let mpeg_version_mpeg2 = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;
        let layer = br.read_u32(2).map_err(|_| Error::UnexpectedEnd)?;
        if layer != 0 {
            return Err(Error::AdtsLayerNonZero);
        }
        let protection_absent = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;

        // 2-bit profile, 4-bit sampling_frequency_index, 1-bit private_bit
        let profile = br.read_u32(2).map_err(|_| Error::UnexpectedEnd)? as u8;
        let sampling_frequency_index = br.read_u32(4).map_err(|_| Error::UnexpectedEnd)? as u8;
        if sampling_frequency_index >= 13 {
            return Err(Error::AdtsReservedSampleRateIndex);
        }
        let _private_bit = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;

        // 3-bit channel_configuration, 1-bit original_copy, 1-bit home
        let channel_configuration = br.read_u32(3).map_err(|_| Error::UnexpectedEnd)? as u8;
        let _original_copy = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;
        let _home = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;

        // 1-bit copyright_identification_bit, 1-bit copyright_identification_start
        let _copyright_identification_bit = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;
        let _copyright_identification_start = br.read_bit().map_err(|_| Error::UnexpectedEnd)?;

        // 13-bit aac_frame_length, 11-bit adts_buffer_fullness,
        // 2-bit number_of_raw_data_blocks_in_frame
        let aac_frame_length = br.read_u32(13).map_err(|_| Error::UnexpectedEnd)? as u16;
        let adts_buffer_fullness = br.read_u32(11).map_err(|_| Error::UnexpectedEnd)? as u16;
        let raw_blocks_minus_one = br.read_u32(2).map_err(|_| Error::UnexpectedEnd)? as u8;
        let number_of_raw_data_blocks_in_frame = raw_blocks_minus_one + 1;

        let payload_offset = if protection_absent {
            ADTS_HEADER_BYTES_NO_CRC
        } else {
            ADTS_HEADER_BYTES_WITH_CRC
        };

        if (aac_frame_length as usize) < payload_offset {
            return Err(Error::AdtsFrameLengthTooSmall);
        }

        // When a CRC is present, confirm the trailing two bytes fit
        // in `data`. We do not validate the CRC value in Phase 1.
        if !protection_absent && data.len() < ADTS_HEADER_BYTES_WITH_CRC {
            return Err(Error::UnexpectedEnd);
        }

        Ok((
            AdtsHeader {
                mpeg_version_mpeg2,
                protection_absent,
                profile,
                sampling_frequency_index,
                channel_configuration,
                aac_frame_length,
                adts_buffer_fullness,
                number_of_raw_data_blocks_in_frame,
            },
            payload_offset,
        ))
    }

    /// Resolved sample rate in Hz, from the
    /// `sampling_frequency_index` via ISO/IEC 14496-3 Table 1.18.
    pub fn sample_rate(&self) -> u32 {
        // `parse` already rejects reserved indices, so the cast
        // cannot index out of bounds for any successfully-parsed
        // header. Defensive check kept regardless.
        ADTS_SAMPLE_RATES_HZ
            .get(self.sampling_frequency_index as usize)
            .copied()
            .unwrap_or(0)
    }

    /// `audioObjectType` for the carried payload — the ADTS wire
    /// field is one less than the `audioObjectType` defined by
    /// ISO/IEC 14496-3 Table 1.16, so this returns `profile + 1`.
    pub fn audio_object_type(&self) -> u8 {
        self.profile + 1
    }

    /// Length in bytes of the `raw_data_block()` region that follows
    /// the header (and CRC, if present): `aac_frame_length` minus
    /// header overhead.
    pub fn payload_len(&self) -> usize {
        let header = if self.protection_absent {
            ADTS_HEADER_BYTES_NO_CRC
        } else {
            ADTS_HEADER_BYTES_WITH_CRC
        };
        (self.aac_frame_length as usize).saturating_sub(header)
    }
}
