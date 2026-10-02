//! `sbr_grid()` / `sbr_dtdf()` / `sbr_invf()` — ISO/IEC 14496-3
//! §4.4.2.8, Tables 4.69–4.71.
//!
//! The SBR time-frequency grid describes how a frame's QMF time slots
//! are partitioned into SBR *envelopes* and *noise floors*, and which
//! frequency resolution (high / low) each envelope uses. It is the
//! variable-length heart of an SBR data element: `sbr_envelope()` and
//! `sbr_noise()` are sized entirely by the grid (`bs_num_env` envelopes
//! and `bs_num_noise` noise floors).
//!
//! Four frame classes (Table 4.69) describe the slot layout:
//!
//! * `FIXFIX` (0) — a fixed number of equal-length envelopes
//!   (`bs_num_env = 2^bs_num_env_raw`, the raw value being a 2-bit
//!   field). A single envelope forces `bs_amp_res = 0`. All envelopes
//!   share one transmitted frequency resolution.
//! * `FIXVAR` (1) — a fixed leading border plus a variable trailing
//!   border list; envelopes are counted by `bs_num_rel_1 + 1` and the
//!   frequency-resolution flags are transmitted in reverse order.
//! * `VARFIX` (2) — a variable leading border plus a fixed trailing
//!   border; envelopes counted by `bs_num_rel_0 + 1`, freq-res in
//!   forward order.
//! * `VARVAR` (3) — both borders variable; envelopes counted by
//!   `bs_num_rel_0 + bs_num_rel_1 + 1`.
//!
//! For the variable classes the *envelope-count pointer* `bs_pointer`
//! is read as `ptr_bits = ceil(log2(bs_num_env + 1))` bits (Table 4.69
//! Note 2: a true float log, not a truncated one).
//!
//! After the class-specific body, `bs_num_noise = (bs_num_env > 1) ? 2
//! : 1`.
//!
//! `sbr_dtdf()` (Table 4.70) reads one delta-direction flag per
//! envelope (`bs_df_env`) and per noise floor (`bs_df_noise`):
//! `false` = delta in frequency (the first band is an absolute start
//! value), `true` = delta in time.
//!
//! `sbr_invf()` (Table 4.71) reads a 2-bit inverse-filtering mode per
//! noise band (`NQ`, taken from the derived noise band table).
//!
//! All three are fixed-/variable-width *syntax* only — no Huffman — so
//! they are fully recoverable from the spec tables. The actual border
//! reconstruction, envelope dequantization, and QMF synthesis are
//! downstream of this parse.

use crate::{Error, Result};
use oxideav_core::bits::BitReader;

/// SBR frame class (`bs_frame_class`, Table 4.69 switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameClass {
    /// `FIXFIX` (0) — fixed start, fixed stop; equal-length envelopes.
    FixFix,
    /// `FIXVAR` (1) — fixed start, variable stop.
    FixVar,
    /// `VARFIX` (2) — variable start, fixed stop.
    VarFix,
    /// `VARVAR` (3) — variable start, variable stop.
    VarVar,
}

impl FrameClass {
    fn from_bits(v: u32) -> Self {
        match v & 0b11 {
            0 => FrameClass::FixFix,
            1 => FrameClass::FixVar,
            2 => FrameClass::VarFix,
            _ => FrameClass::VarVar,
        }
    }

    /// The 2-bit `bs_frame_class` wire value.
    pub fn to_bits(self) -> u32 {
        match self {
            FrameClass::FixFix => 0,
            FrameClass::FixVar => 1,
            FrameClass::VarFix => 2,
            FrameClass::VarVar => 3,
        }
    }
}

/// The maximum number of SBR envelopes per frame (§4.6.18.3.6). Used to
/// bound the variable border lists so a corrupt grid cannot allocate
/// without limit.
pub const SBR_MAX_NUM_ENV: usize = 5;

/// A parsed `sbr_grid()` (Table 4.69) for one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbrGrid {
    /// `bs_frame_class`.
    pub frame_class: FrameClass,
    /// `bs_num_env[ch]` — number of envelopes in this frame.
    pub num_env: usize,
    /// `bs_num_noise[ch]` — number of noise floors (`1` or `2`).
    pub num_noise: usize,
    /// `bs_freq_res[ch][env]` — per-envelope frequency-resolution flag
    /// (`true` = high resolution). Length is [`Self::num_env`].
    pub freq_res: Vec<bool>,
    /// `bs_var_bord_0[ch]` — variable leading border (VARFIX / VARVAR),
    /// else `0`.
    pub var_bord_0: u8,
    /// `bs_var_bord_1[ch]` — variable trailing border (FIXVAR /
    /// VARVAR), else `0`.
    pub var_bord_1: u8,
    /// `bs_rel_bord_0[ch][..]` — relative leading borders (VARFIX /
    /// VARVAR). Each element is the *raw* 2-bit value; the reconstructed
    /// border is `2·raw + 2`.
    pub rel_bord_0: Vec<u8>,
    /// `bs_rel_bord_1[ch][..]` — relative trailing borders (FIXVAR /
    /// VARVAR). Raw 2-bit values; reconstructed `2·raw + 2`.
    pub rel_bord_1: Vec<u8>,
    /// `bs_pointer[ch]` — the envelope-count pointer for the variable
    /// classes (`0` for FIXFIX).
    pub pointer: u32,
    /// Whether this grid forced `bs_amp_res = 0` (single-envelope
    /// FIXFIX). The caller applies this override to the element-level
    /// `bs_amp_res`.
    pub amp_res_override: bool,
}

/// `ptr_bits = ceil(log2(num_env + 1))` (Table 4.69 Note 2: a true
/// float division / log, not a truncated one). For `num_env + 1` a
/// power of two this is exactly `log2`; otherwise it rounds up.
fn ptr_bits(num_env: usize) -> u32 {
    let n = (num_env + 1) as u32;
    // ceil(log2(n)): the position of the highest set bit, plus one if n
    // is not itself a power of two.
    if n <= 1 {
        0
    } else {
        let floor_log2 = 31 - n.leading_zeros();
        if n.is_power_of_two() {
            floor_log2
        } else {
            floor_log2 + 1
        }
    }
}

impl SbrGrid {
    /// Parse `sbr_grid()` (Table 4.69) for channel `ch` from `reader`.
    ///
    /// `num_env` is bounded by [`SBR_MAX_NUM_ENV`]; a value beyond it
    /// (only reachable for a corrupt VARVAR grid) yields
    /// [`Error::SbrGridInvalid`].
    pub fn parse(reader: &mut BitReader<'_>) -> Result<Self> {
        let frame_class = FrameClass::from_bits(read(reader, 2)?);
        let mut var_bord_0 = 0u8;
        let mut var_bord_1 = 0u8;
        let mut rel_bord_0: Vec<u8> = Vec::new();
        let mut rel_bord_1: Vec<u8> = Vec::new();
        let mut pointer = 0u32;
        let mut amp_res_override = false;

        let (num_env, freq_res) = match frame_class {
            FrameClass::FixFix => {
                let raw = read(reader, 2)?;
                let num_env = 1usize << raw; // bs_num_env = 2^tmp.
                check_num_env(num_env)?;
                if num_env == 1 {
                    amp_res_override = true; // bs_amp_res = 0.
                }
                let fr0 = read_flag(reader)?;
                // All envelopes share bs_freq_res[ch][0].
                let freq_res = vec![fr0; num_env];
                (num_env, freq_res)
            }
            FrameClass::FixVar => {
                var_bord_1 = read(reader, 2)? as u8;
                let num_rel_1 = read(reader, 2)? as usize;
                let num_env = num_rel_1 + 1;
                check_num_env(num_env)?;
                for _ in 0..num_env - 1 {
                    rel_bord_1.push(read(reader, 2)? as u8);
                }
                pointer = read(reader, ptr_bits(num_env))?;
                // Frequency-resolution flags transmitted in reverse:
                // bs_freq_res[ch][num_env - 1 - env].
                let mut freq_res = vec![false; num_env];
                for env in 0..num_env {
                    freq_res[num_env - 1 - env] = read_flag(reader)?;
                }
                (num_env, freq_res)
            }
            FrameClass::VarFix => {
                var_bord_0 = read(reader, 2)? as u8;
                let num_rel_0 = read(reader, 2)? as usize;
                let num_env = num_rel_0 + 1;
                check_num_env(num_env)?;
                for _ in 0..num_env - 1 {
                    rel_bord_0.push(read(reader, 2)? as u8);
                }
                pointer = read(reader, ptr_bits(num_env))?;
                // Forward order.
                let mut freq_res = Vec::with_capacity(num_env);
                for _ in 0..num_env {
                    freq_res.push(read_flag(reader)?);
                }
                (num_env, freq_res)
            }
            FrameClass::VarVar => {
                var_bord_0 = read(reader, 2)? as u8;
                var_bord_1 = read(reader, 2)? as u8;
                let num_rel_0 = read(reader, 2)? as usize;
                let num_rel_1 = read(reader, 2)? as usize;
                let num_env = num_rel_0 + num_rel_1 + 1;
                check_num_env(num_env)?;
                for _ in 0..num_rel_0 {
                    rel_bord_0.push(read(reader, 2)? as u8);
                }
                for _ in 0..num_rel_1 {
                    rel_bord_1.push(read(reader, 2)? as u8);
                }
                pointer = read(reader, ptr_bits(num_env))?;
                let mut freq_res = Vec::with_capacity(num_env);
                for _ in 0..num_env {
                    freq_res.push(read_flag(reader)?);
                }
                (num_env, freq_res)
            }
        };

        let num_noise = if num_env > 1 { 2 } else { 1 };

        Ok(SbrGrid {
            frame_class,
            num_env,
            num_noise,
            freq_res,
            var_bord_0,
            var_bord_1,
            rel_bord_0,
            rel_bord_1,
            pointer,
            amp_res_override,
        })
    }
}

/// `sbr_dtdf()` (Table 4.70) — the delta-coding direction flags for a
/// channel's envelopes and noise floors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbrDtdf {
    /// `bs_df_env[ch][env]` — `false` = delta in frequency (absolute
    /// start band), `true` = delta in time. Length = `num_env`.
    pub df_env: Vec<bool>,
    /// `bs_df_noise[ch][noise]` — same convention. Length = `num_noise`.
    pub df_noise: Vec<bool>,
}

impl SbrDtdf {
    /// Parse `sbr_dtdf()` (Table 4.70). `num_env` / `num_noise` come
    /// from the channel's already-parsed [`SbrGrid`].
    pub fn parse(reader: &mut BitReader<'_>, num_env: usize, num_noise: usize) -> Result<Self> {
        let mut df_env = Vec::with_capacity(num_env);
        for _ in 0..num_env {
            df_env.push(read_flag(reader)?);
        }
        let mut df_noise = Vec::with_capacity(num_noise);
        for _ in 0..num_noise {
            df_noise.push(read_flag(reader)?);
        }
        Ok(SbrDtdf { df_env, df_noise })
    }
}

/// `sbr_invf()` (Table 4.71) — the 2-bit inverse-filtering mode per
/// noise band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbrInvf {
    /// `bs_invf_mode[ch][n]` — one mode (0..=3) per noise band (`NQ`).
    pub invf_mode: Vec<u8>,
}

impl SbrInvf {
    /// Parse `sbr_invf()` (Table 4.71). `num_noise_bands` is `NQ` from
    /// the derived noise band table
    /// ([`crate::sbr_freq_bands::HiLoTables::n_q`]).
    pub fn parse(reader: &mut BitReader<'_>, num_noise_bands: usize) -> Result<Self> {
        let mut invf_mode = Vec::with_capacity(num_noise_bands);
        for _ in 0..num_noise_bands {
            invf_mode.push(read(reader, 2)? as u8);
        }
        Ok(SbrInvf { invf_mode })
    }
}

#[inline]
fn read(reader: &mut BitReader<'_>, n: u32) -> Result<u32> {
    reader.read_u32(n).map_err(|_| Error::SbrGridInvalid)
}

#[inline]
fn read_flag(reader: &mut BitReader<'_>) -> Result<bool> {
    reader.read_bit().map_err(|_| Error::SbrGridInvalid)
}

#[inline]
fn check_num_env(num_env: usize) -> Result<()> {
    if num_env == 0 || num_env > SBR_MAX_NUM_ENV {
        Err(Error::SbrGridInvalid)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::bits::BitWriter;

    #[test]
    fn ptr_bits_matches_ceil_log2() {
        // ceil(log2(n+1)) for n = num_env.
        assert_eq!(ptr_bits(1), 1); // ceil(log2 2) = 1
        assert_eq!(ptr_bits(2), 2); // ceil(log2 3) = 2
        assert_eq!(ptr_bits(3), 2); // ceil(log2 4) = 2
        assert_eq!(ptr_bits(4), 3); // ceil(log2 5) = 3
        assert_eq!(ptr_bits(5), 3); // ceil(log2 6) = 3
    }

    #[test]
    fn fixfix_single_env_forces_amp_res() {
        let mut w = BitWriter::new();
        w.write_u32(FrameClass::FixFix.to_bits(), 2);
        w.write_u32(0, 2); // 2^0 = 1 envelope
        w.write_bit(true); // freq_res[0]
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let g = SbrGrid::parse(&mut r).unwrap();
        assert_eq!(g.frame_class, FrameClass::FixFix);
        assert_eq!(g.num_env, 1);
        assert_eq!(g.num_noise, 1);
        assert_eq!(g.freq_res, vec![true]);
        assert!(g.amp_res_override);
    }

    #[test]
    fn fixfix_four_env_shares_freq_res() {
        let mut w = BitWriter::new();
        w.write_u32(FrameClass::FixFix.to_bits(), 2);
        w.write_u32(2, 2); // 2^2 = 4 envelopes
        w.write_bit(false); // freq_res[0] shared by all
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let g = SbrGrid::parse(&mut r).unwrap();
        assert_eq!(g.num_env, 4);
        assert_eq!(g.num_noise, 2);
        assert_eq!(g.freq_res, vec![false; 4]);
        assert!(!g.amp_res_override);
    }

    #[test]
    fn fixvar_reverses_freq_res() {
        // num_rel_1 = 2 → num_env = 3. Frequency-resolution flags are
        // transmitted as bs_freq_res[num_env-1-env].
        let mut w = BitWriter::new();
        w.write_u32(FrameClass::FixVar.to_bits(), 2);
        w.write_u32(1, 2); // var_bord_1
        w.write_u32(2, 2); // num_rel_1 = 2 → num_env = 3
        w.write_u32(0, 2); // rel_bord_1[0]
        w.write_u32(3, 2); // rel_bord_1[1]
                           // ptr_bits(3) = 2.
        w.write_u32(1, 2); // pointer
                           // freq_res transmitted reversed: index 2, then 1, then 0.
        w.write_bit(true); // -> freq_res[2]
        w.write_bit(false); // -> freq_res[1]
        w.write_bit(true); // -> freq_res[0]
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let g = SbrGrid::parse(&mut r).unwrap();
        assert_eq!(g.frame_class, FrameClass::FixVar);
        assert_eq!(g.num_env, 3);
        assert_eq!(g.var_bord_1, 1);
        assert_eq!(g.rel_bord_1, vec![0, 3]);
        assert_eq!(g.pointer, 1);
        assert_eq!(g.freq_res, vec![true, false, true]);
    }

    #[test]
    fn varfix_forward_freq_res() {
        let mut w = BitWriter::new();
        w.write_u32(FrameClass::VarFix.to_bits(), 2);
        w.write_u32(2, 2); // var_bord_0
        w.write_u32(1, 2); // num_rel_0 = 1 → num_env = 2
        w.write_u32(3, 2); // rel_bord_0[0]
                           // ptr_bits(2) = 2.
        w.write_u32(0, 2); // pointer
        w.write_bit(false); // freq_res[0]
        w.write_bit(true); // freq_res[1]
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let g = SbrGrid::parse(&mut r).unwrap();
        assert_eq!(g.frame_class, FrameClass::VarFix);
        assert_eq!(g.num_env, 2);
        assert_eq!(g.var_bord_0, 2);
        assert_eq!(g.rel_bord_0, vec![3]);
        assert_eq!(g.freq_res, vec![false, true]);
    }

    #[test]
    fn varvar_both_borders() {
        let mut w = BitWriter::new();
        w.write_u32(FrameClass::VarVar.to_bits(), 2);
        w.write_u32(1, 2); // var_bord_0
        w.write_u32(2, 2); // var_bord_1
        w.write_u32(1, 2); // num_rel_0 = 1
        w.write_u32(1, 2); // num_rel_1 = 1 → num_env = 3
        w.write_u32(0, 2); // rel_bord_0[0]
        w.write_u32(3, 2); // rel_bord_1[0]
                           // ptr_bits(3) = 2.
        w.write_u32(2, 2); // pointer
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let g = SbrGrid::parse(&mut r).unwrap();
        assert_eq!(g.frame_class, FrameClass::VarVar);
        assert_eq!(g.num_env, 3);
        assert_eq!(g.num_noise, 2);
        assert_eq!(g.var_bord_0, 1);
        assert_eq!(g.var_bord_1, 2);
        assert_eq!(g.rel_bord_0, vec![0]);
        assert_eq!(g.rel_bord_1, vec![3]);
        assert_eq!(g.pointer, 2);
        assert_eq!(g.freq_res, vec![true, false, true]);
    }

    #[test]
    fn dtdf_reads_per_env_and_noise() {
        let mut w = BitWriter::new();
        w.write_bit(true); // df_env[0]
        w.write_bit(false); // df_env[1]
        w.write_bit(true); // df_noise[0]
        w.write_bit(false); // df_noise[1]
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let d = SbrDtdf::parse(&mut r, 2, 2).unwrap();
        assert_eq!(d.df_env, vec![true, false]);
        assert_eq!(d.df_noise, vec![true, false]);
    }

    #[test]
    fn invf_reads_two_bits_per_band() {
        let mut w = BitWriter::new();
        w.write_u32(0, 2);
        w.write_u32(1, 2);
        w.write_u32(2, 2);
        w.write_u32(3, 2);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let inv = SbrInvf::parse(&mut r, 4).unwrap();
        assert_eq!(inv.invf_mode, vec![0, 1, 2, 3]);
    }

    #[test]
    fn truncated_grid_errors() {
        let bytes = [0u8; 0];
        let mut r = BitReader::new(&bytes);
        assert!(matches!(SbrGrid::parse(&mut r), Err(Error::SbrGridInvalid)));
    }
}
