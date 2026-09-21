//! `sbr_envelope()` / `sbr_noise()` raw decode — ISO/IEC 14496-3
//! §4.4.2.8, Tables 4.72–4.73.
//!
//! These two elements carry the SBR spectral-envelope scalefactors and
//! noise-floor scalefactors as **delta values** (`bs_data_env` /
//! `bs_data_noise`). For each envelope (resp. noise floor) the delta
//! direction comes from `sbr_dtdf()` ([`crate::sbr_grid::SbrDtdf`]):
//!
//! * delta-in-**frequency** (`bs_df_* == 0`): the first band carries an
//!   absolute *start value* read as a fixed-width field, and the
//!   remaining bands are frequency-direction Huffman deltas (`f_huff`).
//! * delta-in-**time** (`bs_df_* == 1`): every band is a
//!   time-direction Huffman delta (`t_huff`) relative to the
//!   corresponding band of the previous envelope / noise floor.
//!
//! The start-value field widths (Table 4.72 / 4.73) depend on the
//! coupling / channel / amplitude-resolution context:
//!
//! | element | context | width |
//! |---------|---------|-------|
//! | envelope | coupling && ch, amp_res   | 5 |
//! | envelope | coupling && ch, !amp_res  | 6 |
//! | envelope | level, amp_res            | 6 |
//! | envelope | level, !amp_res           | 7 |
//! | noise    | (any)                     | 5 |
//!
//! The per-envelope band count is `num_env_bands[bs_freq_res]` — the
//! high-resolution band count `NHigh` when the envelope's freq-res flag
//! is set, otherwise the low-resolution count `NLow`
//! ([`crate::sbr_freq_bands::HiLoTables`]). The noise band count is
//! `NQ` for every noise floor.
//!
//! This module produces the **raw** delta arrays exactly as written on
//! the wire; the §4.6.18.3.5 DPCM accumulation across bands / time and
//! the §4.6.18 dequantization to linear energies are downstream.

use crate::sbr_grid::{SbrDtdf, SbrGrid};
use crate::sbr_huffman::{env_tables, noise_tables, sbr_huff_dec, SbrHuffContext};
use crate::{Error, Result};
use oxideav_core::bits::BitReader;

/// Raw `bs_data_env` for one channel: one delta vector per envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbrEnvelopeData {
    /// `bs_data_env[env][band]` — the raw delta (or, at band 0 of a
    /// frequency-coded envelope, the absolute start value). One inner
    /// vector per envelope; its length is the envelope's band count.
    pub data: Vec<Vec<i32>>,
}

/// Raw `bs_data_noise` for one channel: one delta vector per noise
/// floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbrNoiseData {
    /// `bs_data_noise[noise][band]` — raw delta / start value. One
    /// inner vector per noise floor; each has `NQ` entries.
    pub data: Vec<Vec<i32>>,
}

/// The number of envelope bands for a given freq-resolution flag:
/// `NHigh` (high res) or `NLow` (low res).
fn num_env_bands(bands: &crate::sbr_freq_bands::HiLoTables, high_res: bool) -> usize {
    if high_res {
        bands.n_high()
    } else {
        bands.n_low()
    }
}

impl SbrEnvelopeData {
    /// Parse `sbr_envelope()` (Table 4.72) for one channel.
    ///
    /// * `grid` / `dtdf` are this channel's already-parsed grid and
    ///   delta-direction flags.
    /// * `bands` supplies `NHigh` / `NLow` for the per-envelope band
    ///   counts.
    /// * `coupling` is the element `bs_coupling`; `ch` is the channel
    ///   index within the element; `amp_res` is the *effective*
    ///   amplitude resolution (after any single-envelope FIXFIX
    ///   override).
    pub fn parse(
        reader: &mut BitReader<'_>,
        grid: &SbrGrid,
        dtdf: &SbrDtdf,
        bands: &crate::sbr_freq_bands::HiLoTables,
        coupling: bool,
        ch: bool,
        amp_res: bool,
    ) -> Result<Self> {
        let ctx = SbrHuffContext {
            coupling,
            ch,
            amp_res,
        };
        let ((t_huff, t_lav), (f_huff, f_lav)) = env_tables(ctx);

        // Start-value width per Table 4.72.
        let start_bits = if coupling && ch {
            if amp_res {
                5
            } else {
                6
            }
        } else if amp_res {
            6
        } else {
            7
        };

        let mut data = Vec::with_capacity(grid.num_env);
        for env in 0..grid.num_env {
            let n = num_env_bands(bands, grid.freq_res[env]);
            let mut row = Vec::with_capacity(n);
            if !dtdf.df_env[env] {
                // Delta in frequency: band 0 is the absolute start
                // value, bands 1.. are f_huff deltas.
                let start = read(reader, start_bits)? as i32;
                row.push(start);
                for _ in 1..n {
                    row.push(sbr_huff_dec(reader, f_huff, f_lav)?);
                }
            } else {
                // Delta in time: every band is a t_huff delta.
                for _ in 0..n {
                    row.push(sbr_huff_dec(reader, t_huff, t_lav)?);
                }
            }
            data.push(row);
        }
        Ok(SbrEnvelopeData { data })
    }
}

impl SbrNoiseData {
    /// Parse `sbr_noise()` (Table 4.73) for one channel.
    ///
    /// `num_noise_bands` is `NQ`
    /// ([`crate::sbr_freq_bands::HiLoTables::n_q`]). The other
    /// arguments mirror [`SbrEnvelopeData::parse`]; the noise start
    /// value is always a 5-bit field (Table 4.73), regardless of
    /// `amp_res`.
    pub fn parse(
        reader: &mut BitReader<'_>,
        grid: &SbrGrid,
        dtdf: &SbrDtdf,
        num_noise_bands: usize,
        coupling: bool,
        ch: bool,
        amp_res: bool,
    ) -> Result<Self> {
        let ctx = SbrHuffContext {
            coupling,
            ch,
            amp_res,
        };
        let ((t_huff, t_lav), (f_huff, f_lav)) = noise_tables(ctx);

        let mut data = Vec::with_capacity(grid.num_noise);
        for noise in 0..grid.num_noise {
            let mut row = Vec::with_capacity(num_noise_bands);
            if !dtdf.df_noise[noise] {
                // Delta in frequency: band 0 is a 5-bit absolute start
                // value, bands 1.. are f_huff deltas.
                let start = read(reader, 5)? as i32;
                row.push(start);
                for _ in 1..num_noise_bands {
                    row.push(sbr_huff_dec(reader, f_huff, f_lav)?);
                }
            } else {
                for _ in 0..num_noise_bands {
                    row.push(sbr_huff_dec(reader, t_huff, t_lav)?);
                }
            }
            data.push(row);
        }
        Ok(SbrNoiseData { data })
    }
}

#[inline]
fn read(reader: &mut BitReader<'_>, n: u32) -> Result<u32> {
    reader.read_u32(n).map_err(|_| Error::SbrHuffInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbr_freq_bands::{k0, k2, master_table, HiLoTables};
    use crate::sbr_grid::FrameClass;
    use oxideav_core::bits::{BitReader, BitWriter};

    fn bands_44100() -> HiLoTables {
        // The known-good 44.1 kHz linear geometry from sbr_freq_bands.
        let k0v = k0(88_200, 5).unwrap();
        let k2v = k2(88_200, 5, k0v).unwrap();
        let fm = master_table(k0v, k2v, 0, false).unwrap();
        HiLoTables::derive(&fm, 1, 2).unwrap()
    }

    /// Push the MSB-first codeword for one table entry into a writer.
    fn push_code(w: &mut BitWriter, table: &[(u8, u32)], idx: usize) {
        let (len, code) = table[idx];
        w.write_u32(code, len as u32);
    }

    #[test]
    fn envelope_freq_direction_one_env() {
        let bands = bands_44100();
        // FIXFIX, single env, high-res freq, delta-in-frequency.
        let grid = SbrGrid {
            frame_class: FrameClass::FixFix,
            num_env: 1,
            num_noise: 1,
            freq_res: vec![true],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: true,
        };
        let dtdf = SbrDtdf {
            df_env: vec![false], // frequency direction
            df_noise: vec![false],
        };
        // amp_res = false → level start width 7 bits.
        let n = bands.n_high();
        let ((_t, _tl), (f_huff, f_lav)) = env_tables(SbrHuffContext {
            coupling: false,
            ch: false,
            amp_res: false,
        });
        let mut w = BitWriter::new();
        w.write_u32(40, 7); // start value
                            // remaining n-1 bands: pick a few known indices.
        let chosen: Vec<usize> = (1..n)
            .map(|i| (i + f_lav as usize) % f_huff.len())
            .collect();
        for &idx in &chosen {
            push_code(&mut w, f_huff, idx);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ev = SbrEnvelopeData::parse(&mut r, &grid, &dtdf, &bands, false, false, false).unwrap();
        assert_eq!(ev.data.len(), 1);
        assert_eq!(ev.data[0].len(), n);
        assert_eq!(ev.data[0][0], 40);
        for (band, &idx) in chosen.iter().enumerate() {
            assert_eq!(ev.data[0][band + 1], idx as i32 - f_lav);
        }
    }

    #[test]
    fn envelope_time_direction() {
        let bands = bands_44100();
        let grid = SbrGrid {
            frame_class: FrameClass::FixFix,
            num_env: 1,
            num_noise: 1,
            freq_res: vec![false], // low res
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: false,
        };
        let dtdf = SbrDtdf {
            df_env: vec![true], // time direction → no start value
            df_noise: vec![false],
        };
        let n = bands.n_low();
        let ((t_huff, t_lav), (_f, _fl)) = env_tables(SbrHuffContext {
            coupling: false,
            ch: false,
            amp_res: false,
        });
        let mut w = BitWriter::new();
        let chosen: Vec<usize> = (0..n).map(|i| (i * 2 + 1) % t_huff.len()).collect();
        for &idx in &chosen {
            push_code(&mut w, t_huff, idx);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ev = SbrEnvelopeData::parse(&mut r, &grid, &dtdf, &bands, false, false, false).unwrap();
        assert_eq!(ev.data[0].len(), n);
        for (band, &idx) in chosen.iter().enumerate() {
            assert_eq!(ev.data[0][band], idx as i32 - t_lav);
        }
    }

    #[test]
    fn noise_freq_and_time() {
        let bands = bands_44100();
        let nq = bands.n_q();
        let grid = SbrGrid {
            frame_class: FrameClass::FixVar,
            num_env: 2,
            num_noise: 2,
            freq_res: vec![true, true],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: false,
        };
        let dtdf = SbrDtdf {
            df_env: vec![false, false],
            df_noise: vec![false, true], // floor 0 freq, floor 1 time
        };
        let ((t_huff, t_lav), (f_huff, f_lav)) = noise_tables(SbrHuffContext {
            coupling: false,
            ch: false,
            amp_res: false,
        });
        let mut w = BitWriter::new();
        // Floor 0: 5-bit start + (nq-1) f deltas.
        w.write_u32(12, 5);
        let f_chosen: Vec<usize> = (1..nq)
            .map(|i| (i + f_lav as usize) % f_huff.len())
            .collect();
        for &idx in &f_chosen {
            push_code(&mut w, f_huff, idx);
        }
        // Floor 1: nq t deltas.
        let t_chosen: Vec<usize> = (0..nq)
            .map(|i| (i + t_lav as usize) % t_huff.len())
            .collect();
        for &idx in &t_chosen {
            push_code(&mut w, t_huff, idx);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let nd = SbrNoiseData::parse(&mut r, &grid, &dtdf, nq, false, false, false).unwrap();
        assert_eq!(nd.data.len(), 2);
        assert_eq!(nd.data[0].len(), nq);
        assert_eq!(nd.data[0][0], 12);
        for (band, &idx) in f_chosen.iter().enumerate() {
            assert_eq!(nd.data[0][band + 1], idx as i32 - f_lav);
        }
        for (band, &idx) in t_chosen.iter().enumerate() {
            assert_eq!(nd.data[1][band], idx as i32 - t_lav);
        }
    }

    #[test]
    fn coupling_balance_start_width() {
        // Coupled second channel at 3.0 dB → balance start width 5.
        let bands = bands_44100();
        let grid = SbrGrid {
            frame_class: FrameClass::FixFix,
            num_env: 1,
            num_noise: 1,
            freq_res: vec![true],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: false,
        };
        let dtdf = SbrDtdf {
            df_env: vec![false],
            df_noise: vec![false],
        };
        let n = bands.n_high();
        let ((_t, _tl), (f_huff, f_lav)) = env_tables(SbrHuffContext {
            coupling: true,
            ch: true,
            amp_res: true,
        });
        let mut w = BitWriter::new();
        w.write_u32(7, 5); // 5-bit balance start
        for i in 1..n {
            push_code(&mut w, f_huff, (i + f_lav as usize) % f_huff.len());
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ev = SbrEnvelopeData::parse(&mut r, &grid, &dtdf, &bands, true, true, true).unwrap();
        assert_eq!(ev.data[0][0], 7);
    }

    #[test]
    fn truncated_envelope_errors() {
        let bands = bands_44100();
        let grid = SbrGrid {
            frame_class: FrameClass::FixFix,
            num_env: 1,
            num_noise: 1,
            freq_res: vec![true],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: false,
        };
        let dtdf = SbrDtdf {
            df_env: vec![false],
            df_noise: vec![false],
        };
        let bytes = [0u8; 0];
        let mut r = BitReader::new(&bytes);
        assert!(matches!(
            SbrEnvelopeData::parse(&mut r, &grid, &dtdf, &bands, false, false, false),
            Err(Error::SbrHuffInvalid)
        ));
    }
}
