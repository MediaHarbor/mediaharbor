//! `coupling_channel_element()` — ISO/IEC 14496-3 §4.6.8.3 / Table 4.8.
//!
//! The coupling channel element (CCE, `id_syn_ele == 0b010`) carries an
//! embedded `single_channel_element()` whose decoded spectrum is scaled
//! by a list of *gain elements* and added onto one or more target
//! channels (SCE / CPE) signalled by the coupling header. This module
//! owns the **coupling header + gain-list** half of Table 4.8:
//!
//! ```text
//! coupling_channel_element() {
//!     element_instance_tag;                4  uimsbf   // consumed by the walker
//!     ind_sw_cce_flag;                     1  uimsbf
//!     num_coupled_elements;                3  uimsbf
//!     num_gain_element_lists = 0;
//!     for (c = 0; c < num_coupled_elements+1; c++) {
//!         num_gain_element_lists++;
//!         cc_target_is_cpe[c];             1  uimsbf
//!         cc_target_tag_select[c];         4  uimsbf
//!         if (cc_target_is_cpe[c]) {
//!             cc_l[c];                     1  uimsbf
//!             cc_r[c];                     1  uimsbf
//!             if (cc_l[c] && cc_r[c])  num_gain_element_lists++;
//!         }
//!     }
//!     cc_domain;                           1  uimsbf
//!     gain_element_sign;                   1  uimsbf
//!     gain_element_scale;                  2  uimsbf
//!     individual_channel_stream(0,0);      // the embedded SCE body
//!     for (c=1; c<num_gain_element_lists; c++) {
//!         if (ind_sw_cce_flag) cge = 1;
//!         else { common_gain_element_present[c]; 1 uimsbf; cge = ...; }
//!         if (cge)  hcod_sf[common_gain_element[c]];   1..19 vlclbf
//!         else for (g) for (sfb) if (sfb_cb[g][sfb] != ZERO_HCB)
//!                          hcod_sf[dpcm_gain_element[c][g][sfb]]; 1..19 vlclbf
//!     }
//! }
//! ```
//!
//! ## Split of responsibility
//!
//! Like the CPE walk in [`crate::decode`], the embedded
//! `individual_channel_stream(0,0)` (Table 4.50) is parsed by the
//! caller through [`crate::ics_body::IcsBody`] +
//! [`crate::spectral_data::SpectralData`]; this module parses the
//! coupling header that *precedes* it ([`CouplingHeader::parse`]) and the
//! per-target gain lists that *follow* it
//! ([`CouplingGains::parse`]) — the caller threads the embedded SCE's
//! `IcsInfo` / `SectionData` between the two halves so the trailing gain
//! loop can walk the `num_window_groups × max_sfb` non-`ZERO_HCB` band
//! grid (the §4.6.8.3.3 `Note`: `sfb_cb` is the CCE's *embedded* SCE
//! codebooks, not the coupled target's).
//!
//! ## Reconstruction
//!
//! [`CouplingGains::cc_gain`] applies the §4.6.8.3.3 `couple_channel()`
//! scaling: `cc_gain = cc_sign · cc_scale^gain`, with `cc_scale` from
//! Table 4.154 ([`CC_SCALE_TABLE`]) and the §4.6.8.3.3 `gain_element_sign`
//! in-phase / out-of-phase split (`cc_sign = 1 − 2·(g & 1)`, `gain =
//! g >> 1`). The first coupled target (`list_index == 0`) is not
//! transmitted: its gains are all `0`, i.e. the CCE is added in its
//! natural scaling (`cc_gain == 1`).
//!
//! ## Provenance
//!
//! Table 4.8 syntax, the §4.6.8.3.3 `decode_coupling_channel()` /
//! `couple_channel()` pseudocode, the Table 4.153 shared-gain-list table,
//! and the Table 4.154 `cc_scale_table` are all from ISO/IEC 14496-3
//! staged under `docs/audio/aac/`. The gain elements reuse the
//! §4.A.1 scalefactor Huffman codebook (codebook 12) via
//! [`crate::scale_factor_data::hcod_sf_decode`] /
//! [`crate::scale_factor_data::hcod_sf_encode`], exactly as the spec
//! directs ("gain_element values are differentially encoded using the
//! Huffman table for scalefactors").

use oxideav_core::bits::{BitReader, BitWriter};

use crate::ics_body::IcsBody;
use crate::ics_info::IcsInfo;
use crate::scale_factor_data::{hcod_sf_decode, hcod_sf_encode};
use crate::section_data::ZERO_HCB;
use crate::spectral_data::SpectralData;
use crate::{Error, Result};

/// Field width of `ind_sw_cce_flag` (Table 4.8).
pub const IND_SW_CCE_FLAG_BITS: u32 = 1;
/// Field width of `num_coupled_elements` (Table 4.8).
pub const NUM_COUPLED_ELEMENTS_BITS: u32 = 3;
/// Field width of `cc_target_tag_select` (Table 4.8).
pub const CC_TARGET_TAG_SELECT_BITS: u32 = 4;
/// Field width of `gain_element_scale` (Table 4.8).
pub const GAIN_ELEMENT_SCALE_BITS: u32 = 2;

/// Table 4.154 — the four `cc_scale` amplitude resolutions selected by
/// the 2-bit `gain_element_scale`. `cc_scale = 2^(1/8 · 2^scale)`:
/// `2^(1/8)`, `2^(1/4)`, `2^(1/2)`, `2^1` (step sizes 0.75 / 1.5 / 3.0 /
/// 6.0 dB).
pub const CC_SCALE_TABLE: [f64; 4] = [
    1.090_507_732_665_257_7,  // 2^(1/8)
    1.189_207_115_002_721,    // 2^(1/4)
    std::f64::consts::SQRT_2, // 2^(1/2)
    2.0,                      // 2^1
];

/// One coupled target of a CCE (Table 4.8 inner loop, one `c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoupledTarget {
    /// `cc_target_is_cpe[c]` — the coupled target is a CPE (`true`) or a
    /// SCE (`false`).
    pub is_cpe: bool,
    /// `cc_target_tag_select[c]` — the `element_instance_tag` of the
    /// coupled SCE / CPE.
    pub tag_select: u8,
    /// `cc_l[c]` — a gain list applies to the CPE's left channel. Always
    /// `false` for a SCE target.
    pub cc_l: bool,
    /// `cc_r[c]` — a gain list applies to the CPE's right channel. Always
    /// `false` for a SCE target.
    pub cc_r: bool,
}

impl CoupledTarget {
    /// The number of `num_gain_element_lists` slots this target
    /// contributes (Table 4.8): one per target, plus a *second* slot for
    /// a CPE target whose `cc_l && cc_r` (the shared-vs-split gain-list
    /// distinction, Table 4.153).
    fn gain_list_increment(&self) -> u32 {
        if self.is_cpe && self.cc_l && self.cc_r {
            2
        } else {
            1
        }
    }
}

/// Parsed `coupling_channel_element()` header (everything before the
/// embedded `individual_channel_stream(0,0)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouplingHeader {
    /// `ind_sw_cce_flag` — independently switched (`true`) vs dependently
    /// switched (`false`). An independently switched CCE must only use
    /// `common_gain_element` and is decoded to the time domain before
    /// coupling (§4.6.8.3.3); a dependently switched CCE shares the
    /// target window state and couples in the frequency domain.
    pub ind_sw_cce_flag: bool,
    /// `num_coupled_elements` — the number of coupled targets is
    /// `num_coupled_elements + 1` (minimum value `0` ⇒ one target).
    pub num_coupled_elements: u8,
    /// The `num_coupled_elements + 1` coupled targets.
    pub targets: Vec<CoupledTarget>,
    /// `cc_domain` — coupling performed before (`false`) or after
    /// (`true`) TNS decoding of the coupled target channels.
    pub cc_domain: bool,
    /// `gain_element_sign` — the transmitted gain elements carry
    /// in-phase / out-of-phase coupling information (`true`) or not
    /// (`false`).
    pub gain_element_sign: bool,
    /// `gain_element_scale` — 2-bit index into [`CC_SCALE_TABLE`].
    pub gain_element_scale: u8,
    /// `num_gain_element_lists` derived by the Table 4.8 loop. This is
    /// the number of transmitted gain lists; the trailing gain loop runs
    /// over `1 ..= num_gain_element_lists - 1` (list 0 is the implicit
    /// natural-scaling target).
    pub num_gain_element_lists: u32,
}

impl CouplingHeader {
    /// Parse the Table 4.8 coupling header. `reader` is positioned at
    /// `ind_sw_cce_flag` (i.e. the caller — typically the
    /// [`crate::raw_data_block`] walker — already consumed the 4-bit
    /// `element_instance_tag`).
    pub fn parse(reader: &mut BitReader<'_>) -> Result<Self> {
        let ind_sw_cce_flag = read_bit(reader)?;
        let num_coupled_elements = read_u8(reader, NUM_COUPLED_ELEMENTS_BITS)?;

        let mut num_gain_element_lists: u32 = 0;
        let mut targets = Vec::with_capacity(usize::from(num_coupled_elements) + 1);
        for _c in 0..(u32::from(num_coupled_elements) + 1) {
            num_gain_element_lists += 1;
            let is_cpe = read_bit(reader)?;
            let tag_select = read_u8(reader, CC_TARGET_TAG_SELECT_BITS)?;
            let (cc_l, cc_r) = if is_cpe {
                let cc_l = read_bit(reader)?;
                let cc_r = read_bit(reader)?;
                if cc_l && cc_r {
                    num_gain_element_lists += 1;
                }
                (cc_l, cc_r)
            } else {
                (false, false)
            };
            targets.push(CoupledTarget {
                is_cpe,
                tag_select,
                cc_l,
                cc_r,
            });
        }

        let cc_domain = read_bit(reader)?;
        let gain_element_sign = read_bit(reader)?;
        let gain_element_scale = read_u8(reader, GAIN_ELEMENT_SCALE_BITS)?;

        Ok(CouplingHeader {
            ind_sw_cce_flag,
            num_coupled_elements,
            targets,
            cc_domain,
            gain_element_sign,
            gain_element_scale,
            num_gain_element_lists,
        })
    }

    /// Write the Table 4.8 coupling header (mirror of [`Self::parse`]),
    /// **not** including the leading `element_instance_tag` (the caller /
    /// frame assembler owns that, exactly as the walker consumes it on
    /// the parse side).
    ///
    /// Rejects an inconsistent record: a `targets` count that disagrees
    /// with `num_coupled_elements + 1`, a `gain_element_scale > 3`, or a
    /// SCE target carrying a `cc_l` / `cc_r` flag.
    pub fn write(&self, writer: &mut BitWriter) -> Result<()> {
        if self.targets.len() != usize::from(self.num_coupled_elements) + 1 {
            return Err(Error::CceInvalid);
        }
        if self.gain_element_scale > 3 {
            return Err(Error::CceInvalid);
        }
        let mut derived_lists: u32 = 0;
        for t in &self.targets {
            if !t.is_cpe && (t.cc_l || t.cc_r) {
                return Err(Error::CceInvalid);
            }
            derived_lists += t.gain_list_increment();
        }
        if derived_lists != self.num_gain_element_lists {
            return Err(Error::CceInvalid);
        }

        writer.write_bit(self.ind_sw_cce_flag);
        writer.write_u32(
            u32::from(self.num_coupled_elements),
            NUM_COUPLED_ELEMENTS_BITS,
        );
        for t in &self.targets {
            writer.write_bit(t.is_cpe);
            writer.write_u32(u32::from(t.tag_select), CC_TARGET_TAG_SELECT_BITS);
            if t.is_cpe {
                writer.write_bit(t.cc_l);
                writer.write_bit(t.cc_r);
            }
        }
        writer.write_bit(self.cc_domain);
        writer.write_bit(self.gain_element_sign);
        writer.write_u32(u32::from(self.gain_element_scale), GAIN_ELEMENT_SCALE_BITS);
        Ok(())
    }
}

/// The decoded gain list for one coupled target (Table 4.8 trailing
/// loop, one `c`). Either a single `common_gain_element` applied to
/// every band, or a per-`(g, sfb)` `dpcm_gain_element` list whose forward
/// running sum (`a += dpcm`) yields the absolute `gain_element[g][sfb]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GainList {
    /// `cge == 1`: one `common_gain_element` reused over every window
    /// group and scalefactor band (§4.6.8.3.3).
    Common(i32),
    /// `cge == 0`: the per-band absolute gain grid, indexed
    /// `gains[g][sfb]`. Only the non-`ZERO_HCB` bands are transmitted;
    /// `ZERO_HCB` bands hold the running accumulator value (no add).
    Dpcm(Vec<Vec<i32>>),
}

/// The whole trailing gain-list block of a CCE (Table 4.8), one
/// [`GainList`] per transmitted list (`1 ..= num_gain_element_lists`).
///
/// The implicit list 0 (natural scaling) is **not** stored — the
/// [`Self::cc_gain`] helper returns `1.0` for it.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingGains {
    /// The `gain_element_scale`-selected `cc_scale` from Table 4.154.
    pub cc_scale: f64,
    /// `gain_element_sign` (carried so [`Self::cc_gain`] can apply the
    /// in-phase / out-of-phase split).
    pub gain_element_sign: bool,
    /// The transmitted gain lists, in `c = 1 ..= num_gain_element_lists`
    /// order (`lists[0]` is the `c == 1` list).
    pub lists: Vec<GainList>,
}

impl CouplingGains {
    /// Parse the Table 4.8 trailing gain-list loop. `reader` is
    /// positioned immediately after the embedded
    /// `individual_channel_stream(0,0)`.
    ///
    /// * `header` — the already-parsed [`CouplingHeader`].
    /// * `num_window_groups` / `max_sfb` — from the embedded SCE's
    ///   `ics_info()`.
    /// * `sfb_cb` — the embedded SCE's per-`(g, sfb)` section codebooks
    ///   ([`crate::section_data::SectionData::sfb_cb`]); the §4.6.8.3.3
    ///   `Note` requires the CCE's *own* codebooks here, not the coupled
    ///   target's.
    pub fn parse(
        reader: &mut BitReader<'_>,
        header: &CouplingHeader,
        num_window_groups: usize,
        max_sfb: usize,
        sfb_cb: &[Vec<u8>],
    ) -> Result<Self> {
        let cc_scale = CC_SCALE_TABLE[usize::from(header.gain_element_scale & 0x3)];
        let mut lists = Vec::new();
        for _c in 1..header.num_gain_element_lists {
            let cge = if header.ind_sw_cce_flag {
                true
            } else {
                read_bit(reader)?
            };
            if cge {
                let common = i32::from(hcod_sf_decode(reader)?);
                lists.push(GainList::Common(common));
            } else {
                // An independently switched CCE must only use the common
                // gain element (§4.6.8.3.3); a per-band list here is
                // ill-formed. `cge` is already forced true above for that
                // case, so reaching the else branch with ind_sw set is
                // impossible, but guard against a hand-built record.
                if header.ind_sw_cce_flag {
                    return Err(Error::CceInvalid);
                }
                let mut acc: i32 = 0;
                let mut grid = vec![vec![0i32; max_sfb]; num_window_groups];
                for (g, row) in grid.iter_mut().enumerate() {
                    let cb_row = sfb_cb.get(g).ok_or(Error::CceInvalid)?;
                    for (sfb, cell) in row.iter_mut().enumerate() {
                        let cb = *cb_row.get(sfb).ok_or(Error::CceInvalid)?;
                        if cb != ZERO_HCB {
                            acc += i32::from(hcod_sf_decode(reader)?);
                            *cell = acc;
                        } else {
                            // ZERO_HCB band carries the running value but
                            // contributes no coupling (cc_gain unused).
                            *cell = acc;
                        }
                    }
                }
                lists.push(GainList::Dpcm(grid));
            }
        }
        Ok(CouplingGains {
            cc_scale,
            gain_element_sign: header.gain_element_sign,
            lists,
        })
    }

    /// Write the trailing gain-list loop (mirror of [`Self::parse`]).
    /// `sfb_cb` must be the same embedded-SCE codebook grid the parse
    /// consumed so the `ZERO_HCB` bands are skipped identically.
    pub fn write(
        &self,
        writer: &mut BitWriter,
        header: &CouplingHeader,
        sfb_cb: &[Vec<u8>],
    ) -> Result<()> {
        if self.lists.len() + 1 != header.num_gain_element_lists as usize {
            return Err(Error::CceInvalid);
        }
        for list in &self.lists {
            match list {
                GainList::Common(common) => {
                    if !header.ind_sw_cce_flag {
                        // common_gain_element_present[c] = 1
                        writer.write_bit(true);
                    }
                    let dpcm = i8::try_from(*common).map_err(|_| Error::CceInvalid)?;
                    let (len, cw) = hcod_sf_encode(dpcm)?;
                    writer.write_u32(cw, u32::from(len));
                }
                GainList::Dpcm(grid) => {
                    if header.ind_sw_cce_flag {
                        return Err(Error::CceInvalid);
                    }
                    // common_gain_element_present[c] = 0
                    writer.write_bit(false);
                    let mut prev: i32 = 0;
                    for (g, row) in grid.iter().enumerate() {
                        let cb_row = sfb_cb.get(g).ok_or(Error::CceInvalid)?;
                        for (sfb, &abs) in row.iter().enumerate() {
                            let cb = *cb_row.get(sfb).ok_or(Error::CceInvalid)?;
                            if cb != ZERO_HCB {
                                let dpcm =
                                    i8::try_from(abs - prev).map_err(|_| Error::CceInvalid)?;
                                let (len, cw) = hcod_sf_encode(dpcm)?;
                                writer.write_u32(cw, u32::from(len));
                            }
                            prev = abs;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The §4.6.8.3.3 `couple_channel()` per-band gain factor for a given
    /// transmitted gain list and `(g, sfb)`.
    ///
    /// `list_index` is the §4.6.8.3.3 `couple_channel()` `gain_list_index`
    /// (`0` = the implicit natural-scaling target → `cc_gain == 1.0`;
    /// `1 ..= num_gain_element_lists - 1` index [`Self::lists`]).
    ///
    /// Returns `cc_gain = cc_sign · cc_scale^gain`, where (per
    /// `gain_element_sign`):
    /// * sign set: `cc_sign = 1 − 2·(g & 1)`, `gain = g >> 1`;
    /// * sign clear: `cc_sign = 1`, `gain = g`.
    pub fn cc_gain(&self, list_index: usize, g: usize, sfb: usize) -> Result<f64> {
        if list_index == 0 {
            // The first coupled target's gains are not transmitted; the
            // CCE adds in its natural scaling (gain = 0 ⇒ cc_gain = 1).
            return Ok(1.0);
        }
        let list = self.lists.get(list_index - 1).ok_or(Error::CceInvalid)?;
        let raw = match list {
            GainList::Common(common) => *common,
            GainList::Dpcm(grid) => *grid
                .get(g)
                .and_then(|row| row.get(sfb))
                .ok_or(Error::CceInvalid)?,
        };
        let (cc_sign, gain) = if self.gain_element_sign {
            let sign = 1.0 - 2.0 * f64::from(raw & 0x1);
            (sign, raw >> 1)
        } else {
            (1.0, raw)
        };
        Ok(cc_sign * self.cc_scale.powi(gain))
    }

    /// §4.6.8.3.3 `couple_channel(source_spectrum, dest_spectrum,
    /// gain_list_index)` — scale the CCE's embedded-SCE spectrum by the
    /// `gain_list_index` gain list and **add** it onto one target
    /// channel's window-major spectrum in place.
    ///
    /// This is the per-band scale-and-add the spec pseudocode defines:
    ///
    /// ```text
    /// for (g = 0; g < num_window_groups; g++)
    ///   for (b = 0; b < window_group_length[g]; b++)
    ///     for (sfb = 0; sfb < max_sfb; sfb++)
    ///       if (sfb_cb[g][sfb] != ZERO_HCB)
    ///         for (i = swb_offset[sfb]; i < swb_offset[sfb+1]; i++)
    ///           dest[g][b][sfb][i] += cc_gain(idx,g,sfb) * source[g][b][sfb][i];
    /// ```
    ///
    /// `cc_gain` per band is [`Self::cc_gain`] (`cc_sign · cc_scale^gain`);
    /// the implicit list 0 (`list_index == 0`) couples in natural scaling
    /// (`cc_gain == 1`) onto every non-`ZERO_HCB` band.
    ///
    /// * `source` / `dest` — window-major spectra
    ///   (`num_windows × window_len`), identical geometry. `source` is the
    ///   decoded embedded-SCE spectrum; `dest` is the addressed SCE / CPE
    ///   channel's spectrum at the §4.6.8.3.3 `cc_domain` stage (before or
    ///   after TNS).
    /// * `list_index` — the §4.6.8.3.3 `couple_channel()` `gain_list_index`
    ///   the [`CouplingHeader`] walk assigns to this target.
    /// * `sfb_cb` — the **embedded SCE's** per-`(g, sfb)` section
    ///   codebooks, per the §4.6.8.3.3 Note (`sfb_cb` is the CCE's own
    ///   codebook data, not the coupled target's). Drives the `ZERO_HCB`
    ///   band skip and, for a `GainList::Dpcm` list, the gain lookup.
    /// * `window_group_length` / `max_sfb` — the embedded SCE's
    ///   `ics_info()` group geometry.
    /// * `offsets` — the `swb_offset` table for the embedded SCE's window
    ///   length (`window_len + 1` entries; `offsets[sfb]..offsets[sfb+1]`
    ///   is band `sfb`).
    ///
    /// Returns [`Error::CceInvalid`] on any geometry mismatch (source /
    /// dest length, group / band shapes) so a malformed coupling does not
    /// corrupt the target out of bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn couple_channel(
        &self,
        source: &[f64],
        dest: &mut [f64],
        list_index: usize,
        sfb_cb: &[Vec<u8>],
        window_group_length: &[u8],
        max_sfb: usize,
        offsets: &[u16],
    ) -> Result<()> {
        if source.len() != dest.len() {
            return Err(Error::CceInvalid);
        }
        if offsets.is_empty() {
            return Err(Error::CceInvalid);
        }
        // The last `swb_offset` entry is the window length (the first
        // coefficient past the last band). The window-major spectrum is
        // `num_windows * window_len` long.
        let window_len = usize::from(*offsets.last().expect("non-empty checked above"));
        if window_len == 0 || source.len() % window_len != 0 {
            return Err(Error::CceInvalid);
        }
        let num_swb = offsets.len() - 1;
        if max_sfb > num_swb {
            return Err(Error::CceInvalid);
        }
        if sfb_cb.len() != window_group_length.len() {
            return Err(Error::CceInvalid);
        }

        let mut window_base = 0usize;
        for (g, &wgl) in window_group_length.iter().enumerate() {
            let cb_row = sfb_cb.get(g).ok_or(Error::CceInvalid)?;
            if cb_row.len() < max_sfb {
                return Err(Error::CceInvalid);
            }
            let wgl = usize::from(wgl);
            for sfb in 0..max_sfb {
                if cb_row[sfb] == ZERO_HCB {
                    // §4.6.8.3.3: ZERO_HCB bands carry no coupling
                    // contribution (and, for a DPCM list, were not
                    // transmitted — the accumulator simply skipped them).
                    continue;
                }
                let start = usize::from(offsets[sfb]);
                let end = usize::from(offsets[sfb + 1]);
                let cc_gain = self.cc_gain(list_index, g, sfb)?;
                for b in 0..wgl {
                    let base = (window_base + b)
                        .checked_mul(window_len)
                        .ok_or(Error::CceInvalid)?;
                    let dst_end = base + end;
                    if dst_end > dest.len() {
                        return Err(Error::CceInvalid);
                    }
                    for i in start..end {
                        dest[base + i] += cc_gain * source[base + i];
                    }
                }
            }
            window_base += wgl;
        }
        Ok(())
    }
}

/// A fully-parsed `coupling_channel_element()` (Table 4.8): the coupling
/// header, the embedded `individual_channel_stream(0,0)` (body +
/// spectrum), and the trailing gain lists.
///
/// This is the single entry point a `raw_data_block()` walker uses to
/// **consume a whole CCE** from the bitstream (advancing the reader past
/// it). The decode loop can then either drop the element (a CCE
/// contributes no output channel of its own) or, once the cross-element
/// coupling is wired, scale [`Self::spectral`] by [`Self::gains`] and add
/// it onto the addressed target channels (§4.6.8.3.3 `couple_channel()`).
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingChannelElement {
    /// `element_instance_tag` (4 bits) — the CCE's own instance tag.
    pub element_instance_tag: u8,
    /// The Table 4.8 coupling header.
    pub header: CouplingHeader,
    /// The embedded `individual_channel_stream(0,0)` body (Table 4.50),
    /// up to but not including `spectral_data()`.
    pub body: IcsBody,
    /// `ics_info()` of the embedded SCE (cloned out of [`Self::body`] for
    /// convenience; the embedded body always reads its own `ics_info`).
    pub ics_info: IcsInfo,
    /// The embedded SCE's `spectral_data()` (Table 4.56).
    pub spectral: SpectralData,
    /// The Table 4.8 trailing gain lists.
    pub gains: CouplingGains,
}

impl CouplingChannelElement {
    /// Parse a whole `coupling_channel_element()` (Table 4.8). `reader`
    /// is positioned at `element_instance_tag` (i.e. immediately after
    /// the `raw_data_block()` walker read the 3-bit `id_syn_ele == CCE`).
    ///
    /// * `aot` — the surrounding ASC's effective `audioObjectType`.
    /// * `fs_index` — the `samplingFrequencyIndex`.
    ///
    /// Walks, in spec order: the 4-bit instance tag, the
    /// [`CouplingHeader`], the embedded `individual_channel_stream(0,0)`
    /// ([`IcsBody`] + [`SpectralData`]), and the [`CouplingGains`]
    /// gain-list loop keyed off the embedded SCE's `sfb_cb`.
    pub fn parse(reader: &mut BitReader<'_>, aot: u8, fs_index: u8) -> Result<Self> {
        let element_instance_tag = read_u8(reader, 4)?;
        Self::parse_after_tag(reader, element_instance_tag, aot, fs_index)
    }

    /// Parse a `coupling_channel_element()` whose 4-bit
    /// `element_instance_tag` was already consumed by the surrounding
    /// `raw_data_block()` walker (which returns the tag in its
    /// `ChannelElement` event). `reader` is positioned at
    /// `ind_sw_cce_flag`; `element_instance_tag` is the walker-supplied
    /// tag. Otherwise identical to [`Self::parse`].
    pub fn parse_after_tag(
        reader: &mut BitReader<'_>,
        element_instance_tag: u8,
        aot: u8,
        fs_index: u8,
    ) -> Result<Self> {
        let header = CouplingHeader::parse(reader)?;
        // Embedded individual_channel_stream(0,0): common_window = 0 and
        // scale_flag = 0 per Table 4.8.
        let body = IcsBody::parse(reader, aot, fs_index, false)?;
        let ics_info = body.ics_info.clone().ok_or(Error::CceInvalid)?;
        let spectral = SpectralData::parse(reader, &ics_info, &body.section_data, fs_index)?;
        let gains = CouplingGains::parse(
            reader,
            &header,
            usize::from(ics_info.num_window_groups),
            usize::from(ics_info.max_sfb),
            &body.section_data.sfb_cb,
        )?;
        Ok(CouplingChannelElement {
            element_instance_tag,
            header,
            body,
            ics_info,
            spectral,
            gains,
        })
    }
}

/// Helper: read a 1-bit flag, mapping underflow to [`Error::UnexpectedEnd`].
fn read_bit(reader: &mut BitReader<'_>) -> Result<bool> {
    reader.read_bit().map_err(|_| Error::UnexpectedEnd)
}

/// Helper: read an `n`-bit `uimsbf` field as a `u8`.
fn read_u8(reader: &mut BitReader<'_>, bits: u32) -> Result<u8> {
    Ok(reader.read_u32(bits).map_err(|_| Error::UnexpectedEnd)? as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table 4.154 values are `2^(2^scale / 8)`.
    #[test]
    fn cc_scale_table_matches_spec_resolutions() {
        for (scale, &v) in CC_SCALE_TABLE.iter().enumerate() {
            let expected = 2f64.powf((1u32 << scale) as f64 / 8.0);
            assert!(
                (v - expected).abs() < 1e-12,
                "cc_scale[{scale}] = {v} != {expected}"
            );
        }
    }

    /// A header with a single SCE target derives `num_gain_element_lists
    /// == 1` (only the implicit list 0 — no trailing gains).
    #[test]
    fn single_sce_target_has_one_gain_list() {
        // ind_sw=0, num_coupled=0, target0: is_cpe=0 tag=0,
        // cc_domain=0 sign=0 scale=0
        let mut writer = BitWriter::new();
        writer.write_bit(false); // ind_sw_cce_flag
        writer.write_u32(0, 3); // num_coupled_elements
        writer.write_bit(false); // cc_target_is_cpe[0]
        writer.write_u32(0, 4); // cc_target_tag_select[0]
        writer.write_bit(false); // cc_domain
        writer.write_bit(false); // gain_element_sign
        writer.write_u32(0, 2); // gain_element_scale
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let h = CouplingHeader::parse(&mut reader).unwrap();
        assert_eq!(h.num_gain_element_lists, 1);
        assert_eq!(h.targets.len(), 1);
        assert!(!h.targets[0].is_cpe);
    }

    /// A CPE target with `cc_l && cc_r` adds a second gain list slot
    /// (Table 4.153: split left/right lists).
    #[test]
    fn cpe_target_with_both_channels_adds_a_list() {
        let mut writer = BitWriter::new();
        writer.write_bit(false); // ind_sw_cce_flag
        writer.write_u32(0, 3); // num_coupled_elements (=> 1 target)
        writer.write_bit(true); // cc_target_is_cpe[0]
        writer.write_u32(3, 4); // cc_target_tag_select[0]
        writer.write_bit(true); // cc_l[0]
        writer.write_bit(true); // cc_r[0]
        writer.write_bit(false); // cc_domain
        writer.write_bit(false); // gain_element_sign
        writer.write_u32(1, 2); // gain_element_scale
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let h = CouplingHeader::parse(&mut reader).unwrap();
        // 1 (target) + 1 (cc_l && cc_r) = 2.
        assert_eq!(h.num_gain_element_lists, 2);
        assert!(h.targets[0].is_cpe);
        assert!(h.targets[0].cc_l && h.targets[0].cc_r);
        assert_eq!(h.targets[0].tag_select, 3);
    }

    /// The header round-trips through write → parse.
    #[test]
    fn header_round_trips() {
        let h = CouplingHeader {
            ind_sw_cce_flag: true,
            num_coupled_elements: 1,
            targets: vec![
                CoupledTarget {
                    is_cpe: false,
                    tag_select: 2,
                    cc_l: false,
                    cc_r: false,
                },
                CoupledTarget {
                    is_cpe: true,
                    tag_select: 5,
                    cc_l: true,
                    cc_r: false,
                },
            ],
            cc_domain: true,
            gain_element_sign: true,
            gain_element_scale: 2,
            num_gain_element_lists: 2,
        };
        let mut writer = BitWriter::new();
        h.write(&mut writer).unwrap();
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let parsed = CouplingHeader::parse(&mut reader).unwrap();
        assert_eq!(parsed, h);
    }

    /// `write` rejects a SCE target carrying a `cc_l` flag.
    #[test]
    fn write_rejects_sce_target_with_cc_flag() {
        let h = CouplingHeader {
            ind_sw_cce_flag: false,
            num_coupled_elements: 0,
            targets: vec![CoupledTarget {
                is_cpe: false,
                tag_select: 0,
                cc_l: true,
                cc_r: false,
            }],
            cc_domain: false,
            gain_element_sign: false,
            gain_element_scale: 0,
            num_gain_element_lists: 1,
        };
        let mut writer = BitWriter::new();
        assert_eq!(h.write(&mut writer), Err(Error::CceInvalid));
    }

    /// `cc_gain` for the implicit list 0 is the natural scaling 1.0.
    #[test]
    fn cc_gain_list_zero_is_unity() {
        let gains = CouplingGains {
            cc_scale: CC_SCALE_TABLE[3],
            gain_element_sign: false,
            lists: vec![],
        };
        assert_eq!(gains.cc_gain(0, 0, 0).unwrap(), 1.0);
    }

    /// `cc_gain` applies `cc_scale^gain` for a common-gain list with the
    /// sign bit clear.
    #[test]
    fn cc_gain_common_no_sign() {
        let gains = CouplingGains {
            cc_scale: 2.0, // scale index 3 => 2^1
            gain_element_sign: false,
            lists: vec![GainList::Common(3)],
        };
        // gain = 3, cc_sign = 1 => 2^3 = 8.
        assert!((gains.cc_gain(1, 0, 0).unwrap() - 8.0).abs() < 1e-12);
    }

    /// With the sign bit set, the LSB of the gain element selects the
    /// out-of-phase sign and the value is right-shifted.
    #[test]
    fn cc_gain_common_with_sign() {
        let gains = CouplingGains {
            cc_scale: 2.0,
            gain_element_sign: true,
            lists: vec![GainList::Common(7)],
        };
        // raw = 7 (0b111): cc_sign = 1 - 2*(1) = -1, gain = 3 => -8.
        assert!((gains.cc_gain(1, 0, 0).unwrap() + 8.0).abs() < 1e-12);
    }

    /// A dependently switched per-band DPCM list round-trips through
    /// write → parse against a fixed `sfb_cb` grid, and the forward
    /// accumulator reconstructs the absolute gains.
    #[test]
    fn dpcm_gain_list_round_trips() {
        // One window group, three bands; band 1 is ZERO_HCB (skipped).
        let sfb_cb = vec![vec![2u8, ZERO_HCB, 4u8]];
        let header = CouplingHeader {
            ind_sw_cce_flag: false,
            num_coupled_elements: 1,
            targets: vec![
                CoupledTarget {
                    is_cpe: false,
                    tag_select: 0,
                    cc_l: false,
                    cc_r: false,
                },
                CoupledTarget {
                    is_cpe: false,
                    tag_select: 1,
                    cc_l: false,
                    cc_r: false,
                },
            ],
            cc_domain: false,
            gain_element_sign: false,
            gain_element_scale: 0,
            num_gain_element_lists: 2,
        };
        // Absolute gains: band0 = +2 (dpcm +2), band1 carries acc (2,
        // not transmitted), band2 = +5 (dpcm +3).
        let grid = vec![vec![2i32, 2i32, 5i32]];
        let gains = CouplingGains {
            cc_scale: CC_SCALE_TABLE[0],
            gain_element_sign: false,
            lists: vec![GainList::Dpcm(grid.clone())],
        };
        let mut writer = BitWriter::new();
        gains.write(&mut writer, &header, &sfb_cb).unwrap();
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let parsed = CouplingGains::parse(&mut reader, &header, 1, 3, &sfb_cb).unwrap();
        assert_eq!(parsed.lists.len(), 1);
        match &parsed.lists[0] {
            GainList::Dpcm(g) => assert_eq!(g, &grid),
            other => panic!("expected Dpcm, got {other:?}"),
        }
    }

    /// `couple_channel` for the implicit list 0 (natural scaling) adds
    /// the source spectrum onto the target unchanged on every
    /// non-`ZERO_HCB` band, and skips the `ZERO_HCB` band entirely.
    #[test]
    fn couple_channel_list_zero_adds_natural_scaling() {
        // One window group, one window of length 8; two bands of width 4.
        // Band 0 is a spectrum book (couples), band 1 is ZERO_HCB (skip).
        let offsets = [0u16, 4, 8];
        let sfb_cb = vec![vec![2u8, ZERO_HCB]];
        let wgl = [1u8];
        let gains = CouplingGains {
            cc_scale: CC_SCALE_TABLE[3],
            gain_element_sign: false,
            lists: vec![],
        };
        let source = vec![1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut dest = vec![10.0f64; 8];
        gains
            .couple_channel(&source, &mut dest, 0, &sfb_cb, &wgl, 2, &offsets)
            .unwrap();
        // Band 0 (indices 0..4): dest += 1*source.
        assert_eq!(&dest[0..4], &[11.0, 12.0, 13.0, 14.0]);
        // Band 1 (indices 4..8) is ZERO_HCB → untouched.
        assert_eq!(&dest[4..8], &[10.0, 10.0, 10.0, 10.0]);
    }

    /// `couple_channel` applies a non-unity common gain (`cc_scale^gain`)
    /// onto every coupled band.
    #[test]
    fn couple_channel_common_gain_scales() {
        let offsets = [0u16, 4];
        let sfb_cb = vec![vec![2u8]];
        let wgl = [1u8];
        // gain element +1, sign clear, scale index 3 (cc_scale = 2) ⇒
        // cc_gain = 2^1 = 2.
        let gains = CouplingGains {
            cc_scale: 2.0,
            gain_element_sign: false,
            lists: vec![GainList::Common(1)],
        };
        let source = vec![1.0f64, 2.0, 3.0, 4.0];
        let mut dest = vec![0.0f64; 4];
        gains
            .couple_channel(&source, &mut dest, 1, &sfb_cb, &wgl, 1, &offsets)
            .unwrap();
        assert_eq!(dest, vec![2.0, 4.0, 6.0, 8.0]);
    }

    /// `couple_channel` walks the multi-window short-block grid: a window
    /// group of length 2 applies the same per-sfb gain to both windows.
    #[test]
    fn couple_channel_multi_window_group() {
        // num_windows = 2, window_len = 4, one group of length 2, one band.
        let offsets = [0u16, 4];
        let sfb_cb = vec![vec![2u8]];
        let wgl = [2u8];
        let gains = CouplingGains {
            cc_scale: 2.0,
            gain_element_sign: false,
            lists: vec![GainList::Common(0)], // cc_gain = 2^0 = 1
        };
        let source = vec![1.0f64, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0];
        let mut dest = vec![0.0f64; 8];
        gains
            .couple_channel(&source, &mut dest, 1, &sfb_cb, &wgl, 1, &offsets)
            .unwrap();
        // Both windows of the group are coupled at gain 1.
        assert_eq!(dest, source);
    }

    /// `couple_channel` per-band DPCM gains scale each band independently.
    #[test]
    fn couple_channel_dpcm_per_band_gains() {
        let offsets = [0u16, 2, 4];
        let sfb_cb = vec![vec![2u8, 2u8]];
        let wgl = [1u8];
        // Absolute gains: band0 = 0 (cc_gain 1), band1 = 1 (cc_gain 2).
        let gains = CouplingGains {
            cc_scale: 2.0,
            gain_element_sign: false,
            lists: vec![GainList::Dpcm(vec![vec![0i32, 1i32]])],
        };
        let source = vec![3.0f64, 3.0, 3.0, 3.0];
        let mut dest = vec![0.0f64; 4];
        gains
            .couple_channel(&source, &mut dest, 1, &sfb_cb, &wgl, 2, &offsets)
            .unwrap();
        // Band 0 (0..2): ×1; band 1 (2..4): ×2.
        assert_eq!(dest, vec![3.0, 3.0, 6.0, 6.0]);
    }

    /// `couple_channel` rejects a source / dest length mismatch.
    #[test]
    fn couple_channel_rejects_length_mismatch() {
        let offsets = [0u16, 4];
        let sfb_cb = vec![vec![2u8]];
        let gains = CouplingGains {
            cc_scale: 2.0,
            gain_element_sign: false,
            lists: vec![],
        };
        let source = vec![0.0f64; 4];
        let mut dest = vec![0.0f64; 8];
        assert_eq!(
            gains.couple_channel(&source, &mut dest, 0, &sfb_cb, &[1u8], 1, &offsets),
            Err(Error::CceInvalid)
        );
    }

    /// An independently switched CCE forces `cge == 1`: no
    /// `common_gain_element_present` bit is read, and the gain list is a
    /// single common element per target.
    #[test]
    fn ind_sw_cce_uses_common_gain_only() {
        let header = CouplingHeader {
            ind_sw_cce_flag: true,
            num_coupled_elements: 1,
            targets: vec![
                CoupledTarget {
                    is_cpe: false,
                    tag_select: 0,
                    cc_l: false,
                    cc_r: false,
                },
                CoupledTarget {
                    is_cpe: false,
                    tag_select: 1,
                    cc_l: false,
                    cc_r: false,
                },
            ],
            cc_domain: false,
            gain_element_sign: false,
            gain_element_scale: 0,
            num_gain_element_lists: 2,
        };
        let gains = CouplingGains {
            cc_scale: CC_SCALE_TABLE[0],
            gain_element_sign: false,
            lists: vec![GainList::Common(1)],
        };
        let mut writer = BitWriter::new();
        gains.write(&mut writer, &header, &[]).unwrap();
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        // No common_gain_element_present bit is present; parse must read
        // exactly one hcod_sf codeword for the single list.
        let parsed = CouplingGains::parse(&mut reader, &header, 1, 1, &[]).unwrap();
        assert_eq!(parsed.lists, vec![GainList::Common(1)]);
    }
}
