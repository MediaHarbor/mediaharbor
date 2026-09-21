//! Maximum TNS filter order and bandwidth lookup tables —
//! ISO/IEC 14496-3 §4.6.9.4 Tables 4.102 / 4.103 (general AAC)
//! and §4.6.17.2.5 Tables 4.119 / 4.120 (AAC LD).
//!
//! ## What this module covers
//!
//! TNS (Temporal Noise Shaping) places caps on two per-filter wire
//! quantities that the decoder must clamp during reconstruction
//! (the dispatching `tns_data()` parser, [`crate::tns_data`],
//! surfaces wire values *literally* — clamping happens here):
//!
//! * `TNS_MAX_ORDER` (Table 4.102) — the upper bound for
//!   `order[w][filt]` as a function of audio-object type, window
//!   sequence, and whether the surrounding stream's sampling rate
//!   exceeds 32 kHz.
//! * `TNS_MAX_BANDS` (Table 4.103) — the upper bound for the
//!   `bottom` and `top` band indices a TNS filter touches, as a
//!   function of audio-object type and `samplingFrequencyIndex`.
//!   Two AOT families dispatch differently: AOT 3 (AAC SSR) uses the
//!   polyphase-quadrature-filterbank columns; every other GA AOT
//!   uses the non-PQF columns.
//! * `TNS_MAX_BANDS` for AAC LD (Tables 4.119 / 4.120) — a separate
//!   pair of tables keyed by the AAC LD frame size (480 vs 512
//!   samples) and sampling rate, used by AOT 23 (ER AAC LD).
//!
//! ## How the spec applies these caps
//!
//! Per §4.6.9.3 the TNS reconstruction loop clamps the wire `order`
//! and the per-filter band range with:
//!
//! ```text
//! tns_order = min(order[w][f], TNS_MAX_ORDER);
//! start     = swb_offset[min(bottom, TNS_MAX_BANDS, max_sfb)];
//! end       = swb_offset[min(top,    TNS_MAX_BANDS, max_sfb)];
//! ```
//!
//! [`tns_max_order`] and [`tns_max_bands`] return those caps; the
//! [`clamp_tns_order`] / [`clamp_tns_band`] helpers fold the
//! `min` chain into one call so the eventual reconstruction layer
//! consumes them without re-deriving the dispatch from the AOT.
//!
//! ## AOT dispatch
//!
//! The Table 4.102 row map:
//!
//! | AOT       | name                  | row                |
//! |-----------|-----------------------|--------------------|
//! | 1         | AAC Main              | first row          |
//! | 2         | AAC LC                | second row         |
//! | 3         | AAC SSR               | third row          |
//! | 4, 17, 19, 20, 21, 22, 23 | other GA + ER variants using TNS | fourth row ("other AOT using TNS") |
//!
//! AOT 6 (AAC Scalable) and AOT 7 (TwinVQ) are GA dispatch targets
//! per [`crate::asc::GA_AOTS`] but do not use the AAC TNS surface
//! verbatim — AOT 6 wraps an inner AAC layer (which picks its own
//! row), and AOT 7 is a different frequency-domain codec entirely.
//! The accessor surfaces them as the "other" row when invoked, since
//! the field-width dispatch in [`crate::tns_data`] does not gate on
//! AOT in any case.
//!
//! The Table 4.103 column map:
//!
//! | AOT  | columns used                          |
//! |------|---------------------------------------|
//! | 1, 2, 4, 6, 7, 17, 19, 20, 21, 22 | columns 1 (long) / 2 (short) — "without PQF filterbank" |
//! | 3    | columns 3 (long) / 4 (short) — "with PQF filterbank" |
//!
//! AOT 23 (ER AAC LD) does **not** use Table 4.103 at all; its
//! `TNS_MAX_BANDS` cap comes from the §4.6.17.2.5 LD-specific tables
//! [`TNS_MAX_BANDS_LD_480`] / [`TNS_MAX_BANDS_LD_512`] keyed by the
//! AAC LD frame size (480 vs 512 samples). The crate's frame-size
//! tracking is the responsibility of the dispatching
//! `individual_channel_stream()` layer (not landed yet); the
//! accessor here exposes both tables as a stand-alone surface so
//! the eventual LD reconstruction loop can pick the right one.
//!
//! ## What this module does *not* cover
//!
//! * No wire-format I/O. The clamps are decoder-side reconstruction
//!   constraints; the literal `length` / `order` values are still
//!   written and read by [`crate::tns_data`] without clamping.
//! * No actual TNS LPC reconstruction. That belongs in the
//!   per-AOT IMDCT back-end (not yet present in this crate).
//! * No ER AAC ELD (AOT 39) TNS cap. ELD uses its own MDCT length
//!   (480 / 512 like AAC LD) and an ELD-specific reconstruction
//!   path; the spec subclause for that cap lives in §4.6.20 and is
//!   deferred until ELD-specific machinery lands.
//! * No xHE-AAC / USAC (AOT 42) caps. USAC's TNS is governed by
//!   ISO/IEC 23003-3 which is out of scope for this crate.

use crate::ics_info::WindowSequence;
use crate::{Error, Result};

/// AOT 1 — AAC Main.
pub const AOT_AAC_MAIN: u8 = 1;
/// AOT 2 — AAC LC (Low Complexity).
pub const AOT_AAC_LC: u8 = 2;
/// AOT 3 — AAC SSR (Scalable Sampling Rate). Uses the PQF-filterbank
/// columns of Table 4.103.
pub const AOT_AAC_SSR: u8 = 3;
/// AOT 4 — AAC LTP (Long-Term Prediction).
pub const AOT_AAC_LTP: u8 = 4;
/// AOT 23 — ER AAC LD (Low Delay). Uses the §4.6.17.2.5 LD-specific
/// `TNS_MAX_BANDS` tables, not Table 4.103.
pub const AOT_ER_AAC_LD: u8 = 23;

/// Sample-rate index threshold for the "short window / long window
/// >32 kHz / long window ≤32 kHz" partition in Table 4.102.
///
/// `samplingFrequencyIndex` 0..=4 cover 96000 / 88200 / 64000 /
/// 48000 / 44100 Hz — all > 32 kHz. Index 5 is exactly 32 kHz which
/// the table's `<= 32kHz` column also covers. Indices 6..=11 cover
/// 24000 / 22050 / 16000 / 12000 / 11025 / 8000 Hz — all ≤ 32 kHz.
/// Index 12 (7350 Hz) is also ≤ 32 kHz.
const FS_INDEX_FIRST_LE_32K: u8 = 5;

/// `TNS_MAX_BANDS` lookup for AOTs that use the "without PQF
/// filterbank" columns of Table 4.103 with **long** windows. Indexed
/// by `samplingFrequencyIndex` 0..=11 — slot 12 (7350 Hz) is not
/// covered by the table.
const TNS_MAX_BANDS_LONG_NON_PQF: [u8; 12] = [31, 31, 34, 40, 42, 51, 46, 46, 42, 42, 42, 39];

/// `TNS_MAX_BANDS` lookup for AOTs that use the "without PQF
/// filterbank" columns of Table 4.103 with **short** windows.
const TNS_MAX_BANDS_SHORT_NON_PQF: [u8; 12] = [9, 9, 10, 14, 14, 14, 14, 14, 14, 14, 14, 14];

/// `TNS_MAX_BANDS` lookup for AOT 3 (AAC SSR) — the "with PQF
/// filterbank" columns of Table 4.103 — with **long** windows.
const TNS_MAX_BANDS_LONG_PQF: [u8; 12] = [28, 28, 27, 26, 26, 26, 29, 29, 23, 23, 23, 19];

/// `TNS_MAX_BANDS` lookup for AOT 3 (AAC SSR) — the "with PQF
/// filterbank" columns of Table 4.103 — with **short** windows.
const TNS_MAX_BANDS_SHORT_PQF: [u8; 12] = [7, 7, 7, 6, 6, 6, 7, 7, 8, 8, 8, 7];

/// `TNS_MAX_BANDS` for the AAC LD coder when the frame is 480
/// samples per ISO/IEC 14496-3 Table 4.119. Indexed by
/// `samplingFrequencyIndex` 0..=11; entries marked `None` mean the
/// rate is not covered by the table (Table 4.119 only specifies
/// 48000, 44100, 32000, 24000, 22050 Hz — fs indices 3, 4, 5, 6, 7).
pub const TNS_MAX_BANDS_LD_480: [Option<u8>; 12] = [
    None,     // 0 = 96000
    None,     // 1 = 88200
    None,     // 2 = 64000
    Some(31), // 3 = 48000
    Some(32), // 4 = 44100
    Some(37), // 5 = 32000
    Some(30), // 6 = 24000
    Some(30), // 7 = 22050
    None,     // 8 = 16000
    None,     // 9 = 12000
    None,     // 10 = 11025
    None,     // 11 = 8000
];

/// `TNS_MAX_BANDS` for the AAC LD coder when the frame is 512
/// samples per ISO/IEC 14496-3 Table 4.120. Indexed by
/// `samplingFrequencyIndex` 0..=11.
pub const TNS_MAX_BANDS_LD_512: [Option<u8>; 12] = [
    None,     // 0 = 96000
    None,     // 1 = 88200
    None,     // 2 = 64000
    Some(31), // 3 = 48000
    Some(32), // 4 = 44100
    Some(37), // 5 = 32000
    Some(31), // 6 = 24000
    Some(31), // 7 = 22050
    None,     // 8 = 16000
    None,     // 9 = 12000
    None,     // 10 = 11025
    None,     // 11 = 8000
];

/// Look up `TNS_MAX_ORDER` per ISO/IEC 14496-3 Table 4.102.
///
/// `aot` is the `audioObjectType` value driving the stream
/// (1 = Main, 2 = LC, 3 = SSR; all other AOTs fall into the
/// "other AOT using TNS" row of the table). `window_sequence` is
/// the per-frame `ics_info()` value; `EIGHT_SHORT_SEQUENCE`
/// dispatches the `short windows` column, every other sequence
/// dispatches one of the two `long windows` columns. `fs_index` is
/// `samplingFrequencyIndex` (Table 1.18) and partitions the long-
/// window dispatch between `> 32 kHz` (fs 0..=4) and `<= 32 kHz`
/// (fs 5..=12) per the Table 4.102 header.
///
/// Returns [`Error::IcsInfoUnsupportedSampleRateIndex`] when
/// `fs_index >= 13`.
pub fn tns_max_order(aot: u8, window_sequence: WindowSequence, fs_index: u8) -> Result<u8> {
    if fs_index >= 13 {
        return Err(Error::IcsInfoUnsupportedSampleRateIndex(fs_index));
    }

    if window_sequence.is_eight_short() {
        // Every AOT row collapses to 7 for short windows.
        return Ok(7);
    }

    let above_32k = fs_index < FS_INDEX_FIRST_LE_32K;
    Ok(match aot {
        AOT_AAC_MAIN => 20,
        AOT_AAC_LC => 12,
        AOT_AAC_SSR => 12,
        _ => {
            if above_32k {
                20
            } else {
                12
            }
        }
    })
}

/// Look up `TNS_MAX_BANDS` per ISO/IEC 14496-3 Table 4.103.
///
/// `aot` selects the table column pair: AOT 3 (AAC SSR) uses the
/// "with PQF filterbank" columns; every other AOT uses the
/// "without PQF filterbank" columns. `window_sequence` distinguishes
/// the long-window column (every sequence except
/// `EIGHT_SHORT_SEQUENCE`) from the short-window column.
///
/// Returns [`Error::IcsInfoUnsupportedSampleRateIndex`] when
/// `fs_index >= 12` (Table 4.103 does not cover fs 12 = 7350 Hz).
///
/// For AOT 23 (ER AAC LD) this accessor returns the non-PQF Table
/// 4.103 entry as a syntactic fallback; callers in an LD stream
/// should use [`tns_max_bands_ld_480`] or [`tns_max_bands_ld_512`]
/// directly per the LD frame size in [`crate::asc`].
pub fn tns_max_bands(aot: u8, window_sequence: WindowSequence, fs_index: u8) -> Result<u8> {
    let idx = fs_index as usize;
    if idx >= TNS_MAX_BANDS_LONG_NON_PQF.len() {
        return Err(Error::IcsInfoUnsupportedSampleRateIndex(fs_index));
    }

    let table = match (aot, window_sequence.is_eight_short()) {
        (AOT_AAC_SSR, false) => &TNS_MAX_BANDS_LONG_PQF,
        (AOT_AAC_SSR, true) => &TNS_MAX_BANDS_SHORT_PQF,
        (_, false) => &TNS_MAX_BANDS_LONG_NON_PQF,
        (_, true) => &TNS_MAX_BANDS_SHORT_NON_PQF,
    };
    Ok(table[idx])
}

/// Look up `TNS_MAX_BANDS` for an AAC LD stream with a 480-sample
/// frame, per ISO/IEC 14496-3 Table 4.119.
///
/// Returns [`Error::IcsInfoUnsupportedSampleRateIndex`] when
/// `fs_index >= 12`, and the same error when `fs_index` lies in the
/// table's covered range (0..=11) but the entry is `None` (i.e. the
/// sampling rate is not one of the five LD rates 48 / 44.1 / 32 / 24 /
/// 22.05 kHz).
pub fn tns_max_bands_ld_480(fs_index: u8) -> Result<u8> {
    let idx = fs_index as usize;
    if idx >= TNS_MAX_BANDS_LD_480.len() {
        return Err(Error::IcsInfoUnsupportedSampleRateIndex(fs_index));
    }
    TNS_MAX_BANDS_LD_480[idx].ok_or(Error::IcsInfoUnsupportedSampleRateIndex(fs_index))
}

/// Look up `TNS_MAX_BANDS` for an AAC LD stream with a 512-sample
/// frame, per ISO/IEC 14496-3 Table 4.120.
///
/// Returns [`Error::IcsInfoUnsupportedSampleRateIndex`] when
/// `fs_index >= 12`, and the same error when the table entry is
/// `None` (Table 4.120 only covers fs indices 3, 4, 5, 6, 7).
pub fn tns_max_bands_ld_512(fs_index: u8) -> Result<u8> {
    let idx = fs_index as usize;
    if idx >= TNS_MAX_BANDS_LD_512.len() {
        return Err(Error::IcsInfoUnsupportedSampleRateIndex(fs_index));
    }
    TNS_MAX_BANDS_LD_512[idx].ok_or(Error::IcsInfoUnsupportedSampleRateIndex(fs_index))
}

/// Clamp a raw `order[w][filt]` wire value by `TNS_MAX_ORDER` per
/// §4.6.9.3:
///
/// ```text
/// tns_order = min(order[w][f], TNS_MAX_ORDER);
/// ```
///
/// Returns the clamped order, or
/// [`Error::IcsInfoUnsupportedSampleRateIndex`] when the cap lookup
/// rejects `fs_index`.
pub fn clamp_tns_order(
    order: u8,
    aot: u8,
    window_sequence: WindowSequence,
    fs_index: u8,
) -> Result<u8> {
    let cap = tns_max_order(aot, window_sequence, fs_index)?;
    Ok(order.min(cap))
}

/// Clamp a TNS filter band-index (the `bottom` or `top` operand of
/// the swb_offset lookup) by `min(band, TNS_MAX_BANDS, max_sfb)` per
/// §4.6.9.3:
///
/// ```text
/// start = swb_offset[min(bottom, TNS_MAX_BANDS, max_sfb)];
/// end   = swb_offset[min(top,    TNS_MAX_BANDS, max_sfb)];
/// ```
///
/// `max_sfb` is the surrounding `ics_info()` field. Returns the
/// three-way `min`. Errors mirror [`tns_max_bands`].
pub fn clamp_tns_band(
    band: u8,
    max_sfb: u8,
    aot: u8,
    window_sequence: WindowSequence,
    fs_index: u8,
) -> Result<u8> {
    let cap = tns_max_bands(aot, window_sequence, fs_index)?;
    Ok(band.min(cap).min(max_sfb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::WindowSequence;

    // ===== Table 4.102 — TNS_MAX_ORDER =====

    #[test]
    fn order_short_window_is_7_for_every_aot() {
        // Every row of Table 4.102 collapses to 7 in the short-window
        // column. Cover the four AOTs the table calls out by name
        // plus a representative ER AOT (17 = ER AAC LC).
        for aot in [AOT_AAC_MAIN, AOT_AAC_LC, AOT_AAC_SSR, AOT_AAC_LTP, 17] {
            for fs in 0..=12_u8 {
                assert_eq!(
                    tns_max_order(aot, WindowSequence::EightShort, fs).unwrap(),
                    7,
                    "AOT {aot} fs {fs} short windows",
                );
            }
        }
    }

    #[test]
    fn order_aac_main_long_window_is_20_for_all_rates() {
        for fs in 0..=12_u8 {
            for ws in [
                WindowSequence::OnlyLong,
                WindowSequence::LongStart,
                WindowSequence::LongStop,
            ] {
                assert_eq!(tns_max_order(AOT_AAC_MAIN, ws, fs).unwrap(), 20);
            }
        }
    }

    #[test]
    fn order_aac_lc_long_window_is_12_for_all_rates() {
        for fs in 0..=12_u8 {
            assert_eq!(
                tns_max_order(AOT_AAC_LC, WindowSequence::OnlyLong, fs).unwrap(),
                12,
            );
        }
    }

    #[test]
    fn order_aac_ssr_long_window_is_12_for_all_rates() {
        for fs in 0..=12_u8 {
            assert_eq!(
                tns_max_order(AOT_AAC_SSR, WindowSequence::OnlyLong, fs).unwrap(),
                12,
            );
        }
    }

    #[test]
    fn order_other_aot_long_window_splits_at_32k_threshold() {
        // "other AOT using TNS": > 32 kHz → 20, ≤ 32 kHz → 12.
        // fs indices 0..=4 (96/88.2/64/48/44.1 kHz) take the high
        // column; 5..=12 (32 / 24 / 22.05 / 16 / 12 / 11.025 / 8 /
        // 7.35 kHz) take the low column.
        for aot in [AOT_AAC_LTP, 17, 19, 20, 21, 22, 23] {
            for fs in 0..=4_u8 {
                assert_eq!(
                    tns_max_order(aot, WindowSequence::OnlyLong, fs).unwrap(),
                    20,
                    "AOT {aot} fs {fs} long > 32 kHz",
                );
            }
            for fs in 5..=12_u8 {
                assert_eq!(
                    tns_max_order(aot, WindowSequence::OnlyLong, fs).unwrap(),
                    12,
                    "AOT {aot} fs {fs} long <= 32 kHz",
                );
            }
        }
    }

    #[test]
    fn order_rejects_out_of_range_fs_index() {
        assert!(matches!(
            tns_max_order(AOT_AAC_LC, WindowSequence::OnlyLong, 13),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(13))
        ));
        assert!(matches!(
            tns_max_order(AOT_AAC_LC, WindowSequence::OnlyLong, 15),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(15))
        ));
    }

    // ===== Table 4.103 — TNS_MAX_BANDS =====

    #[test]
    fn bands_long_non_pqf_matches_table_row_by_row() {
        // Each row of Table 4.103, column 1 ("without PQF filterbank,
        // long windows"), per the Table 1.18 fs-index ordering.
        let expected: [(u8, u8); 12] = [
            (0, 31),  // 96000
            (1, 31),  // 88200
            (2, 34),  // 64000
            (3, 40),  // 48000
            (4, 42),  // 44100
            (5, 51),  // 32000
            (6, 46),  // 24000
            (7, 46),  // 22050
            (8, 42),  // 16000
            (9, 42),  // 12000
            (10, 42), // 11025
            (11, 39), // 8000
        ];
        for (fs, expected_bands) in expected {
            assert_eq!(
                tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, fs).unwrap(),
                expected_bands,
                "fs {fs} long non-PQF",
            );
        }
    }

    #[test]
    fn bands_short_non_pqf_matches_table_row_by_row() {
        let expected: [(u8, u8); 12] = [
            (0, 9),
            (1, 9),
            (2, 10),
            (3, 14),
            (4, 14),
            (5, 14),
            (6, 14),
            (7, 14),
            (8, 14),
            (9, 14),
            (10, 14),
            (11, 14),
        ];
        for (fs, expected_bands) in expected {
            assert_eq!(
                tns_max_bands(AOT_AAC_LC, WindowSequence::EightShort, fs).unwrap(),
                expected_bands,
                "fs {fs} short non-PQF",
            );
        }
    }

    #[test]
    fn bands_long_pqf_aac_ssr_matches_table_row_by_row() {
        let expected: [(u8, u8); 12] = [
            (0, 28),
            (1, 28),
            (2, 27),
            (3, 26),
            (4, 26),
            (5, 26),
            (6, 29),
            (7, 29),
            (8, 23),
            (9, 23),
            (10, 23),
            (11, 19),
        ];
        for (fs, expected_bands) in expected {
            assert_eq!(
                tns_max_bands(AOT_AAC_SSR, WindowSequence::OnlyLong, fs).unwrap(),
                expected_bands,
                "fs {fs} long PQF",
            );
        }
    }

    #[test]
    fn bands_short_pqf_aac_ssr_matches_table_row_by_row() {
        let expected: [(u8, u8); 12] = [
            (0, 7),
            (1, 7),
            (2, 7),
            (3, 6),
            (4, 6),
            (5, 6),
            (6, 7),
            (7, 7),
            (8, 8),
            (9, 8),
            (10, 8),
            (11, 7),
        ];
        for (fs, expected_bands) in expected {
            assert_eq!(
                tns_max_bands(AOT_AAC_SSR, WindowSequence::EightShort, fs).unwrap(),
                expected_bands,
                "fs {fs} short PQF",
            );
        }
    }

    #[test]
    fn bands_dispatches_long_start_and_stop_to_long_column() {
        // §4.6.9.4 contrast is short vs long; LongStart and LongStop
        // are long-window sequences (the analysis transform produces a
        // 1024-line spectrum just like OnlyLong), so they must use the
        // long-windows column.
        for ws in [WindowSequence::LongStart, WindowSequence::LongStop] {
            assert_eq!(tns_max_bands(AOT_AAC_LC, ws, 4).unwrap(), 42);
            assert_eq!(tns_max_bands(AOT_AAC_SSR, ws, 4).unwrap(), 26);
        }
    }

    #[test]
    fn bands_rejects_fs_12_and_above() {
        // Table 4.103 does not list 7350 Hz (fs 12) as a row.
        assert!(matches!(
            tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, 12),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(12))
        ));
        assert!(matches!(
            tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, 13),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(13))
        ));
        assert!(matches!(
            tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, 15),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(15))
        ));
    }

    #[test]
    fn bands_aot_other_treats_as_non_pqf() {
        // AOT 17 / 19 / 20 / 21 / 22 / 23 (ER variants) are *not*
        // SSR, so they take the non-PQF columns identical to AOT 2.
        for aot in [AOT_AAC_LTP, 17, 19, 20, 21, 22, 23] {
            assert_eq!(
                tns_max_bands(aot, WindowSequence::OnlyLong, 4).unwrap(),
                42,
                "AOT {aot} long non-PQF",
            );
            assert_eq!(
                tns_max_bands(aot, WindowSequence::EightShort, 4).unwrap(),
                14,
                "AOT {aot} short non-PQF",
            );
        }
    }

    // ===== Tables 4.119 / 4.120 — AAC LD =====

    #[test]
    fn ld_480_matches_table_4_119_row_by_row() {
        assert_eq!(tns_max_bands_ld_480(3).unwrap(), 31); // 48000
        assert_eq!(tns_max_bands_ld_480(4).unwrap(), 32); // 44100
        assert_eq!(tns_max_bands_ld_480(5).unwrap(), 37); // 32000
        assert_eq!(tns_max_bands_ld_480(6).unwrap(), 30); // 24000
        assert_eq!(tns_max_bands_ld_480(7).unwrap(), 30); // 22050
    }

    #[test]
    fn ld_512_matches_table_4_120_row_by_row() {
        assert_eq!(tns_max_bands_ld_512(3).unwrap(), 31); // 48000
        assert_eq!(tns_max_bands_ld_512(4).unwrap(), 32); // 44100
        assert_eq!(tns_max_bands_ld_512(5).unwrap(), 37); // 32000
                                                          // The 512-sample row for 24 kHz / 22.05 kHz is 31 (one
                                                          // higher than the 480 row); this is the row-by-row
                                                          // contrast that justifies the two tables existing.
        assert_eq!(tns_max_bands_ld_512(6).unwrap(), 31); // 24000
        assert_eq!(tns_max_bands_ld_512(7).unwrap(), 31); // 22050
    }

    #[test]
    fn ld_480_rejects_uncovered_rates() {
        // Table 4.119 covers fs 3..=7 only. Every other slot is None.
        for fs in [0_u8, 1, 2, 8, 9, 10, 11] {
            assert!(matches!(
                tns_max_bands_ld_480(fs),
                Err(Error::IcsInfoUnsupportedSampleRateIndex(_))
            ));
        }
    }

    #[test]
    fn ld_512_rejects_uncovered_rates() {
        for fs in [0_u8, 1, 2, 8, 9, 10, 11] {
            assert!(matches!(
                tns_max_bands_ld_512(fs),
                Err(Error::IcsInfoUnsupportedSampleRateIndex(_))
            ));
        }
    }

    #[test]
    fn ld_accessors_reject_out_of_range_fs_index() {
        assert!(matches!(
            tns_max_bands_ld_480(12),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(12))
        ));
        assert!(matches!(
            tns_max_bands_ld_480(15),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(15))
        ));
        assert!(matches!(
            tns_max_bands_ld_512(13),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(13))
        ));
    }

    // ===== Clamp helpers =====

    #[test]
    fn clamp_order_floors_to_cap() {
        // AAC LC at 48 kHz long: cap is 12. A wire order of 20
        // (decoder MUST clamp per §4.6.9.3) becomes 12.
        assert_eq!(
            clamp_tns_order(20, AOT_AAC_LC, WindowSequence::OnlyLong, 3).unwrap(),
            12,
        );
        // Wire order under the cap is returned unchanged.
        assert_eq!(
            clamp_tns_order(5, AOT_AAC_LC, WindowSequence::OnlyLong, 3).unwrap(),
            5,
        );
        // Equal-to-cap order is preserved (not clamped to one less).
        assert_eq!(
            clamp_tns_order(12, AOT_AAC_LC, WindowSequence::OnlyLong, 3).unwrap(),
            12,
        );
        // Short windows always cap at 7 regardless of AOT.
        assert_eq!(
            clamp_tns_order(31, AOT_AAC_MAIN, WindowSequence::EightShort, 3).unwrap(),
            7,
        );
    }

    #[test]
    fn clamp_order_propagates_fs_error() {
        assert!(matches!(
            clamp_tns_order(5, AOT_AAC_LC, WindowSequence::OnlyLong, 13),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(13))
        ));
    }

    #[test]
    fn clamp_band_takes_three_way_min() {
        // AAC LC at 44.1 kHz long: TNS_MAX_BANDS = 42. With
        // max_sfb = 49 (per Table 4.129) and a wire band of 50, the
        // three-way min is 42 (the TNS_MAX_BANDS cap wins).
        assert_eq!(
            clamp_tns_band(50, 49, AOT_AAC_LC, WindowSequence::OnlyLong, 4).unwrap(),
            42,
        );
        // With max_sfb = 30 the second min collapses to 30 (the
        // ics_info `max_sfb` cap wins).
        assert_eq!(
            clamp_tns_band(50, 30, AOT_AAC_LC, WindowSequence::OnlyLong, 4).unwrap(),
            30,
        );
        // With a wire band under both caps, the band itself wins.
        assert_eq!(
            clamp_tns_band(10, 49, AOT_AAC_LC, WindowSequence::OnlyLong, 4).unwrap(),
            10,
        );
    }

    #[test]
    fn clamp_band_propagates_fs_error() {
        assert!(matches!(
            clamp_tns_band(5, 49, AOT_AAC_LC, WindowSequence::OnlyLong, 12),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(12))
        ));
    }

    // ===== Sanity: table lengths cover fs 0..=11 =====

    #[test]
    fn every_non_ld_table_has_12_entries() {
        assert_eq!(TNS_MAX_BANDS_LONG_NON_PQF.len(), 12);
        assert_eq!(TNS_MAX_BANDS_SHORT_NON_PQF.len(), 12);
        assert_eq!(TNS_MAX_BANDS_LONG_PQF.len(), 12);
        assert_eq!(TNS_MAX_BANDS_SHORT_PQF.len(), 12);
    }

    #[test]
    fn every_ld_table_has_12_entries() {
        assert_eq!(TNS_MAX_BANDS_LD_480.len(), 12);
        assert_eq!(TNS_MAX_BANDS_LD_512.len(), 12);
    }
}
