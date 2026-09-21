//! `ps_data()` — Parametric Stereo bitstream element, ISO/IEC
//! 14496-3:2009 §8.4.2 Tables 8.9–8.14 (+ §8.5.2 semantics).
//!
//! PS conveys the stereo image of an HE-AAC v2 stream as per-band
//! Inter-channel Intensity Differences (IID), Inter-channel
//! Coherences (ICC) and optional Inter-channel / Overall Phase
//! Differences (IPD/OPD), carried inside the SBR `sbr_extension()`
//! container (`bs_extension_id == EXTENSION_ID_PS`, Annex 8.A).
//!
//! ## Header persistence
//!
//! The one-bit `enable_ps_header` gates the configuration block
//! (`enable_iid` / `iid_mode` / `enable_icc` / `icc_mode` /
//! `enable_ext`); when clear, **the latest transmitted configuration
//! persists** (§8.5.2). [`PsData::parse`] therefore takes the previous
//! frame's [`PsConfig`] and returns `Ok(None)` for a headerless
//! element with no prior configuration — per §8.6.5.1 the decoder
//! outputs the mono signal in both channels until a decodable
//! `ps_data()` arrives.
//!
//! ## Differential decode
//!
//! IID/ICC/IPD/OPD parameters are DPCM-coded per envelope, either over
//! frequency (`*_dt[e] == 0`, band `b` relative to band `b-1`, the
//! first band relative to index 0) or over time (`*_dt[e] == 1`,
//! relative to the same band of envelope `e-1`, envelope 0 relative to
//! the previous frame's last envelope). [`PsData::resolve`] applies
//! the accumulation against a caller-threaded [`PsIndexState`] and
//! range-checks the result against the Table 8.24 / 8.27 index ranges
//! (IPD/OPD indices accumulate modulo 8 on the Table 8.31 phase
//! ladder, so they cannot leave their range). `num_env == 0` signals
//! that the previous parameters are held (§8.5.2 / Table 8.50–8.52);
//! `resolve` then produces no envelopes and leaves the state
//! untouched.
//!
//! All truth from ISO/IEC 14496-3:2009 subpart 8 staged under
//! `docs/audio/aac/`.

use oxideav_core::bits::BitReader;

use crate::ps_huffman::{
    ps_huff_dec, HUFF_ICC_DF, HUFF_ICC_DT, HUFF_IID_DF, HUFF_IID_DT, HUFF_IID_FINE_DF,
    HUFF_IID_FINE_DT, HUFF_IPD_DF, HUFF_IPD_DT, HUFF_OPD_DF, HUFF_OPD_DT,
};
use crate::{Error, Result};

/// `nr_iid_par_tab[iid_mode]` / `nr_icc_par_tab[icc_mode]` — Tables
/// 8.24 / 8.27 (modes 6 and 7 are reserved).
const NR_PAR_TAB: [usize; 6] = [10, 20, 34, 10, 20, 34];

/// `nr_ipdopd_par_tab[iid_mode]` — Table 8.24.
const NR_IPDOPD_PAR_TAB: [usize; 6] = [5, 11, 17, 5, 11, 17];

/// `num_env_tab[frame_class][num_env_idx]` — Table 8.29.
const NUM_ENV_TAB: [[usize; 4]; 2] = [[0, 1, 2, 4], [1, 2, 3, 4]];

/// The persistent `ps_data()` configuration (the `enable_ps_header`
/// block of Table 8.9): which parameters are transmitted and on which
/// band/quantization grid (Tables 8.24 / 8.27).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsConfig {
    /// `enable_iid`.
    pub enable_iid: bool,
    /// `iid_mode` (0..=5; 6/7 reserved). Meaningful when `enable_iid`.
    pub iid_mode: u8,
    /// `enable_icc`.
    pub enable_icc: bool,
    /// `icc_mode` (0..=5; 6/7 reserved). Meaningful when `enable_icc`.
    pub icc_mode: u8,
    /// `enable_ext` — whether the extension layer (IPD/OPD) may be
    /// present.
    pub enable_ext: bool,
}

impl PsConfig {
    /// Number of IID parameters per envelope (Table 8.24).
    #[must_use]
    pub fn nr_iid_par(&self) -> usize {
        if self.enable_iid {
            NR_PAR_TAB[usize::from(self.iid_mode)]
        } else {
            0
        }
    }

    /// Number of ICC parameters per envelope (Table 8.27).
    #[must_use]
    pub fn nr_icc_par(&self) -> usize {
        if self.enable_icc {
            NR_PAR_TAB[usize::from(self.icc_mode)]
        } else {
            0
        }
    }

    /// Number of IPD/OPD parameters per envelope (Table 8.24 — coupled
    /// to the IID configuration).
    #[must_use]
    pub fn nr_ipdopd_par(&self) -> usize {
        if self.enable_iid {
            NR_IPDOPD_PAR_TAB[usize::from(self.iid_mode)]
        } else {
            0
        }
    }

    /// `iid_quant` — Table 8.24: modes 3..=5 use the fine (±15,
    /// Table 8.26) grid, modes 0..=2 the default (±7, Table 8.25).
    #[must_use]
    pub fn iid_quant_fine(&self) -> bool {
        self.iid_mode >= 3
    }

    /// The Table 8.24 IID index bound: 7 (default grid) or 15 (fine).
    #[must_use]
    pub fn iid_bound(&self) -> i32 {
        if self.iid_quant_fine() {
            15
        } else {
            7
        }
    }
}

/// One parsed `ps_data()` element: the effective configuration plus
/// the raw (still differential) parameter deltas of each envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsData {
    /// `enable_ps_header` — whether this element carried a fresh
    /// configuration block.
    pub header_present: bool,
    /// The effective configuration (fresh or inherited).
    pub config: PsConfig,
    /// `frame_class` — `false` = FIX_BORDERS, `true` = VAR_BORDERS.
    pub frame_class: bool,
    /// `num_env` (Table 8.29). `0` = hold the previous parameters.
    pub num_env: usize,
    /// `border_position[e]` (5 bits each) when VAR_BORDERS.
    pub border_position: Vec<u8>,
    /// `iid_dt[e]` — time (`true`) vs frequency differential.
    pub iid_dt: Vec<bool>,
    /// Raw IID deltas per envelope (`nr_iid_par` each).
    pub iid_deltas: Vec<Vec<i32>>,
    /// `icc_dt[e]`.
    pub icc_dt: Vec<bool>,
    /// Raw ICC deltas per envelope (`nr_icc_par` each).
    pub icc_deltas: Vec<Vec<i32>>,
    /// `enable_ipdopd` (extension layer, Table 8.10); `false` when no
    /// extension was present.
    pub enable_ipdopd: bool,
    /// `ipd_dt[e]`.
    pub ipd_dt: Vec<bool>,
    /// Raw IPD deltas per envelope (`nr_ipdopd_par` each).
    pub ipd_deltas: Vec<Vec<i32>>,
    /// `opd_dt[e]`.
    pub opd_dt: Vec<bool>,
    /// Raw OPD deltas per envelope.
    pub opd_deltas: Vec<Vec<i32>>,
}

impl PsData {
    /// Parse one `ps_data()` element (Table 8.9).
    ///
    /// `prev_config` is the configuration in force from the last
    /// element that carried `enable_ps_header == 1`. Returns
    /// `Ok(None)` when the element carries no header and no previous
    /// configuration exists (§8.6.5.1: output mono until then) —
    /// the payload bits are consumed either way.
    pub fn parse(
        reader: &mut BitReader<'_>,
        prev_config: Option<&PsConfig>,
    ) -> Result<Option<PsData>> {
        let header_present = read_flag(reader)?;
        let config = if header_present {
            let enable_iid = read_flag(reader)?;
            let mut iid_mode = 0u8;
            if enable_iid {
                iid_mode = read(reader, 3)? as u8;
                if iid_mode > 5 {
                    return Err(Error::PsDataInvalid);
                }
            }
            let enable_icc = read_flag(reader)?;
            let mut icc_mode = 0u8;
            if enable_icc {
                icc_mode = read(reader, 3)? as u8;
                if icc_mode > 5 {
                    return Err(Error::PsDataInvalid);
                }
            }
            let enable_ext = read_flag(reader)?;
            PsConfig {
                enable_iid,
                iid_mode,
                enable_icc,
                icc_mode,
                enable_ext,
            }
        } else {
            match prev_config {
                Some(c) => *c,
                // §8.6.5.1: not yet decodable — a conformant stream
                // starts with a header'd element; consume nothing more
                // and signal "mono until a header arrives".
                None => return Ok(None),
            }
        };

        let frame_class = read_flag(reader)?;
        let num_env_idx = read(reader, 2)? as usize;
        let num_env = NUM_ENV_TAB[usize::from(frame_class)][num_env_idx];

        let mut border_position = Vec::new();
        if frame_class {
            for _ in 0..num_env {
                border_position.push(read(reader, 5)? as u8);
            }
        }

        let nr_iid = config.nr_iid_par();
        let mut iid_dt = Vec::with_capacity(num_env);
        let mut iid_deltas = Vec::with_capacity(num_env);
        if config.enable_iid {
            let fine = config.iid_quant_fine();
            for _ in 0..num_env {
                let dt = read_flag(reader)?;
                iid_dt.push(dt);
                let table: &[(u8, u32)] = match (fine, dt) {
                    (false, false) => &HUFF_IID_DF,
                    (false, true) => &HUFF_IID_DT,
                    (true, false) => &HUFF_IID_FINE_DF,
                    (true, true) => &HUFF_IID_FINE_DT,
                };
                let lav = if fine { 30 } else { 14 };
                let mut row = Vec::with_capacity(nr_iid);
                for _ in 0..nr_iid {
                    row.push(ps_huff_dec(reader, table, lav)?);
                }
                iid_deltas.push(row);
            }
        }

        let nr_icc = config.nr_icc_par();
        let mut icc_dt = Vec::with_capacity(num_env);
        let mut icc_deltas = Vec::with_capacity(num_env);
        if config.enable_icc {
            for _ in 0..num_env {
                let dt = read_flag(reader)?;
                icc_dt.push(dt);
                let table: &[(u8, u32)] = if dt { &HUFF_ICC_DT } else { &HUFF_ICC_DF };
                let mut row = Vec::with_capacity(nr_icc);
                for _ in 0..nr_icc {
                    row.push(ps_huff_dec(reader, table, 7)?);
                }
                icc_deltas.push(row);
            }
        }

        // Extension layer (Tables 8.9/8.10): byte-counted, id-tagged.
        let mut enable_ipdopd = false;
        let mut ipd_dt = Vec::new();
        let mut ipd_deltas = Vec::new();
        let mut opd_dt = Vec::new();
        let mut opd_deltas = Vec::new();
        if config.enable_ext {
            let mut cnt = read(reader, 4)?;
            if cnt == 15 {
                cnt += read(reader, 8)?;
            }
            let mut num_bits_left = i64::from(8 * cnt);
            let nr_ipdopd = config.nr_ipdopd_par();
            while num_bits_left > 7 {
                let id = read(reader, 2)?;
                num_bits_left -= 2;
                if id == 0 {
                    // ps_extension(0): optional IPD/OPD + reserved bit.
                    let start = reader.bit_position();
                    enable_ipdopd = read_flag(reader)?;
                    if enable_ipdopd {
                        for _ in 0..num_env {
                            let dt_i = read_flag(reader)?;
                            ipd_dt.push(dt_i);
                            let t: &[(u8, u32)] = if dt_i { &HUFF_IPD_DT } else { &HUFF_IPD_DF };
                            let mut row = Vec::with_capacity(nr_ipdopd);
                            for _ in 0..nr_ipdopd {
                                row.push(ps_huff_dec(reader, t, 0)?);
                            }
                            ipd_deltas.push(row);
                            let dt_o = read_flag(reader)?;
                            opd_dt.push(dt_o);
                            let t: &[(u8, u32)] = if dt_o { &HUFF_OPD_DT } else { &HUFF_OPD_DF };
                            let mut row = Vec::with_capacity(nr_ipdopd);
                            for _ in 0..nr_ipdopd {
                                row.push(ps_huff_dec(reader, t, 0)?);
                            }
                            opd_deltas.push(row);
                        }
                    }
                    let _reserved_ps = read_flag(reader)?;
                    num_bits_left -= (reader.bit_position() - start) as i64;
                } else {
                    // Unknown extension id: the remaining block is fill.
                    skip_bits(reader, num_bits_left)?;
                    num_bits_left = 0;
                }
            }
            if num_bits_left < 0 {
                return Err(Error::PsDataInvalid);
            }
            // fill_bits.
            skip_bits(reader, num_bits_left)?;
        }

        Ok(Some(PsData {
            header_present,
            config,
            frame_class,
            num_env,
            border_position,
            iid_dt,
            iid_deltas,
            icc_dt,
            icc_deltas,
            enable_ipdopd,
            ipd_dt,
            ipd_deltas,
            opd_dt,
            opd_deltas,
        }))
    }
}

/// Cross-frame differential state: the absolute parameter indices of
/// the previous frame's last envelope, plus the band counts they were
/// decoded at (a mode change forces frequency-differential coding on
/// the first envelope, §8.5.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PsIndexState {
    /// Last-envelope absolute IID indices.
    pub iid: Vec<i32>,
    /// Last-envelope absolute ICC indices.
    pub icc: Vec<i32>,
    /// Last-envelope absolute IPD indices (0..8).
    pub ipd: Vec<i32>,
    /// Last-envelope absolute OPD indices (0..8).
    pub opd: Vec<i32>,
}

/// The resolved (absolute-index) parameters of one `ps_data()`
/// element: `num_env` rows per enabled parameter kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PsIndices {
    /// Absolute IID indices per envelope (Table 8.25/8.26 domain).
    pub iid: Vec<Vec<i32>>,
    /// Absolute ICC indices per envelope (Table 8.28 domain, 0..=7).
    pub icc: Vec<Vec<i32>>,
    /// Absolute IPD indices per envelope (Table 8.31 ladder, 0..8).
    pub ipd: Vec<Vec<i32>>,
    /// Absolute OPD indices per envelope.
    pub opd: Vec<Vec<i32>>,
}

impl PsData {
    /// Resolve the differential deltas to absolute indices against
    /// `state` (§8.5.2 `iid_par[e][b]` accumulation), updating `state`
    /// to this element's last envelope. Time-differential envelope 0
    /// references the previous frame's last envelope; when the
    /// previous state has a different parameter count (mode change —
    /// the spec forces `*_dt[0] == 0` there) a zero history is used
    /// for robustness. IID/ICC results are range-checked; IPD/OPD
    /// accumulate modulo 8.
    pub fn resolve(&self, state: &mut PsIndexState) -> Result<PsIndices> {
        let mut out = PsIndices::default();
        if self.num_env == 0 {
            // Parameters held (§8.6.4.6.5); state unchanged.
            return Ok(out);
        }
        let bound = self.config.iid_bound();
        out.iid = resolve_kind(
            &self.iid_deltas,
            &self.iid_dt,
            &mut state.iid,
            self.config.nr_iid_par(),
            Some((-bound, bound)),
        )?;
        out.icc = resolve_kind(
            &self.icc_deltas,
            &self.icc_dt,
            &mut state.icc,
            self.config.nr_icc_par(),
            Some((0, 7)),
        )?;
        if self.enable_ipdopd {
            out.ipd = resolve_kind(
                &self.ipd_deltas,
                &self.ipd_dt,
                &mut state.ipd,
                self.config.nr_ipdopd_par(),
                None,
            )?;
            out.opd = resolve_kind(
                &self.opd_deltas,
                &self.opd_dt,
                &mut state.opd,
                self.config.nr_ipdopd_par(),
                None,
            )?;
        } else {
            // §8.5.2: no IPD/OPD data → parameters are index 0.
            state.ipd.clear();
            state.opd.clear();
        }
        Ok(out)
    }
}

/// Accumulate one parameter kind's deltas to absolute indices.
/// `range = None` selects the modulo-8 phase accumulation (Table
/// 8.31); `Some((lo, hi))` the range-checked linear accumulation.
fn resolve_kind(
    deltas: &[Vec<i32>],
    dt: &[bool],
    state: &mut Vec<i32>,
    nr_par: usize,
    range: Option<(i32, i32)>,
) -> Result<Vec<Vec<i32>>> {
    if deltas.is_empty() {
        // Parameter kind disabled this frame; reset its history so a
        // later re-enable starts from the defaults (§8.5.2 index 0).
        state.clear();
        return Ok(Vec::new());
    }
    let mut rows: Vec<Vec<i32>> = Vec::with_capacity(deltas.len());
    for (e, row) in deltas.iter().enumerate() {
        let mut abs = Vec::with_capacity(nr_par);
        if dt[e] {
            // Time differential: reference envelope e-1 (or the
            // previous frame's last envelope; zeros on a mode change).
            let prev_row: &[i32] = if e > 0 {
                &rows[e - 1]
            } else if state.len() == nr_par {
                state
            } else {
                &[]
            };
            for (b, &d) in row.iter().enumerate().take(nr_par) {
                let prev = prev_row.get(b).copied().unwrap_or(0);
                abs.push(accumulate(prev, d, range)?);
            }
        } else {
            // Frequency differential: band b references band b-1,
            // band 0 references index 0.
            let mut prev = 0i32;
            for &d in row {
                prev = accumulate(prev, d, range)?;
                abs.push(prev);
            }
        }
        rows.push(abs);
    }
    *state = rows.last().cloned().unwrap_or_default();
    Ok(rows)
}

#[inline]
fn accumulate(prev: i32, delta: i32, range: Option<(i32, i32)>) -> Result<i32> {
    match range {
        Some((lo, hi)) => {
            let v = prev + delta;
            if v < lo || v > hi {
                return Err(Error::PsDataInvalid);
            }
            Ok(v)
        }
        None => Ok((prev + delta).rem_euclid(8)),
    }
}

#[inline]
fn read(reader: &mut BitReader<'_>, n: u32) -> Result<u32> {
    reader.read_u32(n).map_err(|_| Error::PsDataInvalid)
}

#[inline]
fn read_flag(reader: &mut BitReader<'_>) -> Result<bool> {
    reader.read_bit().map_err(|_| Error::PsDataInvalid)
}

#[inline]
fn skip_bits(reader: &mut BitReader<'_>, mut n: i64) -> Result<()> {
    while n > 0 {
        let step = n.min(32) as u32;
        read(reader, step)?;
        n -= i64::from(step);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::bits::BitWriter;

    /// Write the 1-bit codeword for delta 0 in the coarse IID (`0`),
    /// ICC (`0`) tables.
    fn write_zero_deltas(w: &mut BitWriter, n: usize) {
        for _ in 0..n {
            w.write_bit(false);
        }
    }

    /// Minimal header'd element: IID mode 0 (10 bands), ICC mode 0,
    /// no ext, FIX_BORDERS, 1 envelope, all-zero freq deltas.
    fn build_min() -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_bit(true); // enable_ps_header
        w.write_bit(true); // enable_iid
        w.write_u32(0, 3); // iid_mode = 0
        w.write_bit(true); // enable_icc
        w.write_u32(0, 3); // icc_mode = 0
        w.write_bit(false); // enable_ext
        w.write_bit(false); // frame_class = FIX
        w.write_u32(1, 2); // num_env_idx = 1 -> num_env = 1
        w.write_bit(false); // iid_dt[0] = freq
        write_zero_deltas(&mut w, 10);
        w.write_bit(false); // icc_dt[0] = freq
        write_zero_deltas(&mut w, 10);
        w.finish()
    }

    #[test]
    fn parses_minimal_headered_element() {
        let bytes = build_min();
        let mut r = BitReader::new(&bytes);
        let ps = PsData::parse(&mut r, None).unwrap().unwrap();
        assert!(ps.header_present);
        assert!(ps.config.enable_iid);
        assert_eq!(ps.config.nr_iid_par(), 10);
        assert_eq!(ps.config.nr_icc_par(), 10);
        assert!(!ps.config.iid_quant_fine());
        assert_eq!(ps.num_env, 1);
        assert_eq!(ps.iid_deltas[0], vec![0; 10]);
        assert_eq!(ps.icc_deltas[0], vec![0; 10]);

        let mut st = PsIndexState::default();
        let idx = ps.resolve(&mut st).unwrap();
        assert_eq!(idx.iid[0], vec![0; 10]);
        assert_eq!(idx.icc[0], vec![0; 10]);
        assert_eq!(st.iid, vec![0; 10]);
    }

    #[test]
    fn headerless_without_prior_config_is_mono_signal() {
        let mut w = BitWriter::new();
        w.write_bit(false); // enable_ps_header = 0
        w.write_bit(false);
        w.write_u32(0, 2);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        assert!(PsData::parse(&mut r, None).unwrap().is_none());
    }

    #[test]
    fn headerless_inherits_previous_config() {
        // First frame with header, then a headerless frame reusing it.
        let bytes = build_min();
        let mut r = BitReader::new(&bytes);
        let ps0 = PsData::parse(&mut r, None).unwrap().unwrap();

        let mut w = BitWriter::new();
        w.write_bit(false); // enable_ps_header = 0
        w.write_bit(false); // frame_class
        w.write_u32(1, 2); // num_env = 1
        w.write_bit(true); // iid_dt[0] = time
        for _ in 0..10 {
            w.write_bit(false); // coarse dt zero-delta codeword `0`
        }
        w.write_bit(true); // icc_dt[0] = time
        for _ in 0..10 {
            w.write_bit(false);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ps1 = PsData::parse(&mut r, Some(&ps0.config)).unwrap().unwrap();
        assert!(!ps1.header_present);
        assert_eq!(ps1.config, ps0.config);
        assert!(ps1.iid_dt[0]);
    }

    /// Frequency-differential accumulation: deltas +1 per band ramp
    /// the index; time-differential carries envelope-to-envelope.
    #[test]
    fn differential_accumulation_freq_then_time() {
        let mut w = BitWriter::new();
        w.write_bit(true); // header
        w.write_bit(true); // enable_iid
        w.write_u32(0, 3); // iid_mode 0
        w.write_bit(false); // enable_icc = 0
        w.write_bit(false); // enable_ext = 0
        w.write_bit(false); // FIX
        w.write_u32(2, 2); // num_env = 2
                           // env 0: freq deltas +1 ×7 then -1 ×3
                           // (coarse df: +1 = `100`, -1 = `101`).
        w.write_bit(false);
        for _ in 0..7 {
            w.write_u32(0b100, 3);
        }
        for _ in 0..3 {
            w.write_u32(0b101, 3);
        }
        // env 1: time deltas -1 ×10 (coarse dt: -1 = `10`).
        w.write_bit(true);
        for _ in 0..10 {
            w.write_u32(0b10, 2);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ps = PsData::parse(&mut r, None).unwrap().unwrap();
        let mut st = PsIndexState::default();
        let idx = ps.resolve(&mut st).unwrap();
        // env 0 freq ramp: +1 ×7 then -1 ×3 → 1..7 then 6,5,4.
        assert_eq!(idx.iid[0], vec![1, 2, 3, 4, 5, 6, 7, 6, 5, 4]);
        // env 1 subtracts 1 per band from env 0.
        assert_eq!(idx.iid[1], vec![0, 1, 2, 3, 4, 5, 6, 5, 4, 3]);
        // State carries env 1 forward.
        assert_eq!(st.iid, idx.iid[1]);
        // ICC disabled: no rows, history cleared.
        assert!(idx.icc.is_empty());
        assert!(st.icc.is_empty());
    }

    /// A frequency ramp that leaves the Table 8.24 index range is
    /// rejected.
    #[test]
    fn out_of_range_iid_rejected() {
        let mut w = BitWriter::new();
        w.write_bit(true); // header
        w.write_bit(true); // enable_iid
        w.write_u32(0, 3); // iid_mode 0 (bound ±7)
        w.write_bit(false); // enable_icc
        w.write_bit(false); // enable_ext
        w.write_bit(false); // FIX
        w.write_u32(1, 2); // num_env = 1
        w.write_bit(false); // freq
        for _ in 0..10 {
            w.write_u32(0b100, 3); // +1 each → crosses +7 at band 7
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ps = PsData::parse(&mut r, None).unwrap().unwrap();
        let mut st = PsIndexState::default();
        assert!(matches!(ps.resolve(&mut st), Err(Error::PsDataInvalid)));
    }

    /// VAR_BORDERS carries 5-bit border positions; the extension
    /// layer decodes IPD/OPD with modulo-8 accumulation.
    #[test]
    fn var_borders_and_ipdopd_extension() {
        let mut w = BitWriter::new();
        w.write_bit(true); // header
        w.write_bit(true); // enable_iid
        w.write_u32(0, 3); // iid_mode 0 → nr_ipdopd_par = 5
        w.write_bit(false); // enable_icc
        w.write_bit(true); // enable_ext
        w.write_bit(true); // frame_class = VAR
        w.write_u32(0, 2); // num_env_idx 0 → num_env = 1 (VAR column)
        w.write_u32(15, 5); // border_position[0]
        w.write_bit(false); // iid_dt[0] = freq
        for _ in 0..10 {
            w.write_bit(false); // zero deltas
        }
        // Extension: ps_extension_size counts whole bytes. Body:
        // id(2) + enable_ipdopd(1) + ipd_dt(1) + 5×ipd deltas +
        // opd_dt(1) + 5×opd deltas + reserved(1) then fill. Zero
        // phase deltas are the 1-bit codeword `1`.
        let mut body = BitWriter::new();
        body.write_u32(0, 2); // ps_extension_id = 0
        body.write_bit(true); // enable_ipdopd
        body.write_bit(false); // ipd_dt[0] = freq
        for _ in 0..5 {
            body.write_bit(true); // delta 0
        }
        body.write_bit(false); // opd_dt[0]
        for _ in 0..5 {
            body.write_bit(true);
        }
        body.write_bit(false); // reserved_ps
        let body_bytes = body.finish(); // padded to whole bytes = fill
        w.write_u32(body_bytes.len() as u32, 4); // ps_extension_size
        for &b in &body_bytes {
            w.write_u32(u32::from(b), 8);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ps = PsData::parse(&mut r, None).unwrap().unwrap();
        assert!(ps.frame_class);
        assert_eq!(ps.border_position, vec![15]);
        assert!(ps.enable_ipdopd);
        assert_eq!(ps.ipd_deltas[0], vec![0; 5]);
        let mut st = PsIndexState::default();
        let idx = ps.resolve(&mut st).unwrap();
        assert_eq!(idx.ipd[0], vec![0; 5]);
        assert_eq!(idx.opd[0], vec![0; 5]);
    }
}
