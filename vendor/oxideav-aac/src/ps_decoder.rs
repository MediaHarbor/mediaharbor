//! PS frame driver — ISO/IEC 14496-3:2009 Annex 8.A (combination of
//! the SBR tool with the parametric stereo tool).
//!
//! Composes the whole §8.6.4 chain per stereo frame: `ps_data()`
//! parse (with the persistent header configuration), differential
//! index resolution, the hybrid analysis of the Annex 8.A.3 `Xinput`
//! matrix (32 SBR slots + 6 look-ahead slots from `XLow`),
//! de-correlation with the per-frame partial reset above the
//! SBR-generated spectrum (`kmax = k_x + M + 7` hybrid channels for
//! 10/20 stereo bands, `+ 27` for 34 — the split-region offsets), the
//! §8.6.4.6 stereo mixing, and the hybrid synthesis back to two
//! 64-band QMF matrices ready for the final synthesis filterbanks.
//!
//! Per §8.6.5.1 the decoder stays *inactive* (mono output duplicated
//! by the caller) until the first `ps_data()` that carries
//! `enable_ps_header == 1` arrives; per Annex 8.A.3 a frame with no
//! `ps_data()` after activation holds the previous parameters, and a
//! *missing previous* `ps_data()` forces a full de-correlator reset.
//! Table 8.44 picks the stereo band count from the IID/ICC modes
//! (either at 34 bands → 34, else 20); a switch re-maps the retained
//! mixing coefficients (Table 8.47) and resets the hybrid /
//! de-correlator state.
//!
//! All truth from ISO/IEC 14496-3:2009 subpart 8 + Annex 8.A staged
//! under `docs/audio/aac/`.

use oxideav_core::bits::BitReader;

use crate::ps_data::{PsConfig, PsData, PsIndexState};
use crate::ps_decorr::PsDecorr;
use crate::ps_hybrid::{synthesize, HybridConfig, PsHybrid};
use crate::ps_stereo::PsStereo;
use crate::sbr_qmf::Complex;
use crate::Result;

/// A stereo pair of 64-band QMF matrices (`NUM_QMF_SLOTS` slots).
pub type QmfPair = (Vec<[Complex; 64]>, Vec<[Complex; 64]>);

/// The Annex 8.A PS decoder: one instance per SBR channel element.
#[derive(Debug)]
pub struct PsDecoder {
    /// Persistent `enable_ps_header` configuration (§8.5.2).
    config: Option<PsConfig>,
    /// Cross-frame differential-index state.
    idx_state: PsIndexState,
    hybrid: PsHybrid,
    decorr: PsDecorr,
    stereo: PsStereo,
    /// Whether the previous frame carried a `ps_data()` element
    /// (Annex 8.A.3 full-reset rule).
    prev_frame_had_ps: bool,
    /// Whether a decodable (header-carrying) `ps_data()` has arrived.
    active: bool,
}

impl Default for PsDecoder {
    fn default() -> Self {
        PsDecoder::new()
    }
}

impl PsDecoder {
    /// A fresh, inactive PS decoder (20-band configuration until the
    /// first header says otherwise).
    #[must_use]
    pub fn new() -> Self {
        PsDecoder {
            config: None,
            idx_state: PsIndexState::default(),
            hybrid: PsHybrid::new(HybridConfig::Bands1020),
            decorr: PsDecorr::new(HybridConfig::Bands1020),
            stereo: PsStereo::new(20),
            prev_frame_had_ps: false,
            active: false,
        }
    }

    /// Whether a decodable `ps_data()` has been received — before
    /// this, the caller outputs the mono signal on both channels.
    #[must_use]
    pub fn active(&self) -> bool {
        self.active
    }

    /// Process one stereo frame.
    ///
    /// * `payload` — the raw `sbr_extension()` body bytes carrying
    ///   `ps_data()` (already stripped of the 2-bit extension id), or
    ///   `None` when this frame transmitted no PS data (parameters
    ///   hold).
    /// * `x_input` — the Annex 8.A.3 `Xinput` matrix:
    ///   `NUM_QMF_SLOTS + LOOKAHEAD` slots of 64 QMF bands (the
    ///   look-ahead tail needs only the split bands populated).
    /// * `kx_plus_m` — `k_x + M` (§4.6.18.3.2.2): the first QMF band
    ///   above the SBR-generated spectrum, for the per-frame partial
    ///   de-correlator reset (pass 32 for a pure-upsampled frame).
    ///
    /// Returns `Ok(None)` while inactive (§8.6.5.1 — the caller
    /// duplicates the mono synthesis), otherwise the left/right QMF
    /// matrices for two independent §4.6.18.4.2 synthesis banks.
    pub fn process(
        &mut self,
        payload: Option<&[u8]>,
        x_input: &[[Complex; 64]],
        kx_plus_m: usize,
    ) -> Result<Option<QmfPair>> {
        // Parse (and activate on the first header'd element).
        let parsed: Option<PsData> = match payload {
            Some(bytes) => {
                let mut reader = BitReader::new(bytes);
                PsData::parse(&mut reader, self.config.as_ref())?
            }
            None => None,
        };
        if let Some(ps) = &parsed {
            self.config = Some(ps.config);
            self.active = true;
        }
        let Some(config) = self.config else {
            // Not yet decodable: mono until a header arrives.
            self.prev_frame_had_ps = payload.is_some();
            return Ok(None);
        };
        if !self.active {
            self.prev_frame_had_ps = payload.is_some();
            return Ok(None);
        }

        // Table 8.44: 34 stereo bands iff either parameter kind runs
        // on the 34-band grid; disabled kinds count as 20.
        let bands34 = (config.enable_iid && config.iid_mode % 3 == 2)
            || (config.enable_icc && config.icc_mode % 3 == 2);
        let hcfg = if bands34 {
            HybridConfig::Bands34
        } else {
            HybridConfig::Bands1020
        };
        if hcfg != self.hybrid.config() {
            // Table 8.47: instantaneous filterbank switch, coefficient
            // re-map, de-correlator reset.
            self.hybrid.reset(hcfg);
            self.decorr = PsDecorr::new(hcfg);
            self.stereo.switch_bands(if bands34 { 34 } else { 20 });
        }

        // Annex 8.A.3 resets: full when the previous frame had no
        // ps_data(); otherwise partial above the SBR spectrum.
        if !self.prev_frame_had_ps {
            self.decorr.reset_bands(0);
        } else {
            let off = if bands34 { 27 } else { 7 };
            let kmax = (kx_plus_m + off).min(hcfg.nr_bands());
            self.decorr.reset_bands(kmax);
        }

        // The hold element for a frame with no (new) parameters.
        let ps = parsed.unwrap_or_else(|| hold_element(config));
        let idx = ps.resolve(&mut self.idx_state)?;

        // Hybrid analysis → de-correlation → stereo mixing →
        // hybrid synthesis.
        let s = self.hybrid.analyze(x_input)?;
        let d = self.decorr.process(&s)?;
        let (l, r) = self.stereo.process(&ps, &idx, hcfg, &s, &d)?;
        let l_qmf = synthesize(hcfg, &l);
        let r_qmf = synthesize(hcfg, &r);

        self.prev_frame_had_ps = payload.is_some();
        Ok(Some((l_qmf, r_qmf)))
    }
}

/// A `num_env == 0` element holding the previous parameters
/// (§8.6.4.6.5 / Table 8.50–8.52).
fn hold_element(config: PsConfig) -> PsData {
    PsData {
        header_present: false,
        config,
        frame_class: false,
        num_env: 0,
        border_position: Vec::new(),
        iid_dt: Vec::new(),
        iid_deltas: Vec::new(),
        icc_dt: Vec::new(),
        icc_deltas: Vec::new(),
        enable_ipdopd: false,
        ipd_dt: Vec::new(),
        ipd_deltas: Vec::new(),
        opd_dt: Vec::new(),
        opd_deltas: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ps_hybrid::{LOOKAHEAD, NUM_QMF_SLOTS};
    use oxideav_core::bits::BitWriter;

    /// Build a header'd one-envelope ps_data payload: coarse IID with
    /// a uniform index, ICC index 0 everywhere (freq differential).
    fn payload(iid_idx: i32) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_bit(true); // enable_ps_header
        w.write_bit(true); // enable_iid
        w.write_u32(0, 3); // iid_mode 0
        w.write_bit(true); // enable_icc
        w.write_u32(0, 3); // icc_mode 0
        w.write_bit(false); // enable_ext
        w.write_bit(false); // FIX
        w.write_u32(1, 2); // num_env = 1
        w.write_bit(false); // iid_dt = freq
        let (len, code) = crate::ps_huffman::HUFF_IID_DF[(iid_idx + 14) as usize];
        w.write_u32(code, u32::from(len));
        let (l0, c0) = crate::ps_huffman::HUFF_IID_DF[14];
        for _ in 1..10 {
            w.write_u32(c0, u32::from(l0));
        }
        w.write_bit(false); // icc_dt = freq
        let (li, ci) = crate::ps_huffman::HUFF_ICC_DF[7];
        for _ in 0..10 {
            w.write_u32(ci, u32::from(li));
        }
        w.finish()
    }

    fn x_input_ones() -> Vec<[Complex; 64]> {
        (0..NUM_QMF_SLOTS + LOOKAHEAD)
            .map(|_| [Complex::new(1.0, 0.0); 64])
            .collect()
    }

    /// Inactive until a header'd element arrives; then the stereo
    /// output appears and a hold frame keeps producing it.
    #[test]
    fn activation_and_hold() {
        let mut dec = PsDecoder::new();
        let x = x_input_ones();
        // No payload → inactive.
        assert!(dec.process(None, &x, 32).unwrap().is_none());
        // Headerless payload with no prior config → still inactive.
        let mut w = BitWriter::new();
        w.write_bit(false); // enable_ps_header = 0
        w.write_bit(false); // frame_class
        w.write_u32(0, 2); // num_env_idx → num_env = 0
        let headerless = w.finish();
        assert!(dec.process(Some(&headerless), &x, 32).unwrap().is_none());
        // Header'd element → active, stereo out.
        let p = payload(7); // +25 dB left
        let out = dec.process(Some(&p), &x, 32).unwrap();
        let (l, r) = out.expect("active after header");
        assert_eq!(l.len(), NUM_QMF_SLOTS);
        assert_eq!(r.len(), NUM_QMF_SLOTS);
        // Hold frame (no payload) keeps producing stereo.
        assert!(dec.process(None, &x, 32).unwrap().is_some());
    }

    /// A large positive IID tilts the energy to the left channel
    /// (steady state, after a couple of frames of interpolation).
    #[test]
    fn iid_tilts_energy_left() {
        let mut dec = PsDecoder::new();
        let x = x_input_ones();
        let p = payload(7);
        let mut l_e = 0.0f64;
        let mut r_e = 0.0f64;
        for f in 0..4 {
            let out = dec.process(Some(&p), &x, 32).unwrap().unwrap();
            if f >= 2 {
                for n in 0..NUM_QMF_SLOTS {
                    for k in 0..64 {
                        l_e += out.0[n][k].norm_sqr();
                        r_e += out.1[n][k].norm_sqr();
                    }
                }
            }
        }
        // 25 dB IID → power ratio 10^2.5 ≈ 316; allow generous slack
        // for the decorrelated component and filter transients.
        assert!(l_e > 50.0 * r_e, "left {l_e} not dominant over right {r_e}");
    }

    /// IID 0 + ICC 1 reproduces the mono signal identically on both
    /// channels in steady state (h11 = h12 = 1, h21 = h22 = 0).
    #[test]
    fn neutral_cues_give_dual_mono() {
        let mut dec = PsDecoder::new();
        let x = x_input_ones();
        let p = payload(0);
        let mut last = None;
        for _ in 0..3 {
            last = dec.process(Some(&p), &x, 32).unwrap();
        }
        let (l, r) = last.unwrap();
        for n in 0..NUM_QMF_SLOTS {
            for k in 0..64 {
                let d = l[n][k] - r[n][k];
                assert!(d.norm_sqr() < 1e-20, "slot {n} band {k}");
                // And the mono signal passes through: DC input in
                // every QMF band re-appears (the hybrid partition is
                // exact).
            }
        }
        let d = l[16][10] - Complex::new(1.0, 0.0);
        assert!(d.norm_sqr() < 1e-18, "mono pass-through broken: {d:?}");
    }
}
