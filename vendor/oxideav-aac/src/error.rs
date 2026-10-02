//! Crate-local error type.

/// Errors returned by `oxideav-aac` Phase 1 surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Decode / encode body is not implemented yet (Phase 1 skeleton).
    NotImplemented,

    /// ADTS sync pattern (`syncword = 0xFFF`, 12 bits) not found at the
    /// expected position. ISO/IEC 13818-7 §1.A.2.2.1.
    AdtsSyncNotFound,

    /// ADTS layer field must be `00` per ISO/IEC 13818-7 §1.A.2.2.1
    /// (the *Layer* field is reserved for MPEG-1/2 layer signalling
    /// and is required zero in ADTS). Decoder rejects non-zero.
    AdtsLayerNonZero,

    /// Reserved `sampling_frequency_index` value (13 or 14). ISO/IEC
    /// 14496-3 Table 1.18 marks indices 13 and 14 as reserved; index
    /// 15 signals an explicit 24-bit rate (only present in
    /// `AudioSpecificConfig`, never in an ADTS header — the ADTS
    /// field is 4 bits so the legal range is 0..=12).
    AdtsReservedSampleRateIndex,

    /// ADTS `aac_frame_length` is smaller than the fixed header
    /// itself (7 bytes without CRC, 9 bytes with CRC). Such a frame
    /// is malformed and cannot wrap any payload.
    AdtsFrameLengthTooSmall,

    /// The bit-reader hit end-of-stream while parsing.
    UnexpectedEnd,

    /// Encountered an `id_syn_ele` value the walker cannot advance
    /// past in Phase 1. Carries the raw 3-bit value (0..=7) — the
    /// caller can map it back to ISO/IEC 14496-3 Table 4.71 names.
    /// Phase 1 can step past FIL (`0b110`), DSE (`0b100`), and PCE
    /// (`0b101`); the channel elements (SCE/CPE/CCE/LFE) still
    /// require body parsing that is deferred.
    UnsupportedElementSkip(u8),

    /// `AudioSpecificConfig` carried an `audioObjectType` whose
    /// body Phase 1 does not parse. The General Audio AOTs handled
    /// by Phase 1 are 1 (Main), 2 (LC), 3 (SSR), 4 (LTP), 6
    /// (scalable), 7 (TwinVQ), 17 (ER AAC LC), 19 (ER AAC LTP), 20
    /// (ER AAC scalable), 21 (ER TwinVQ), 22 (ER BSAC), 23 (ER AAC
    /// LD); SBR (5) and PS (29) hierarchical wrappers are
    /// unwrapped before this check. Any other AOT — CELP, HVXC,
    /// SSC, USAC, ELD, ALS, SLS, …  — currently surfaces here.
    UnsupportedAot(u8),

    /// [`crate::ics_info::IcsInfo::parse`] was called with a
    /// `sampling_frequency_index` outside the standard 0..=11
    /// range covered by the `NUM_SWB_{LONG,SHORT}_WINDOW` tables.
    /// The 24-bit explicit-rate escape (`samplingFrequencyIndex
    /// == 0xf`) does not select an SWB table directly — the caller
    /// must resolve the explicit rate to the nearest standard
    /// index before invoking the ics_info parser.
    IcsInfoUnsupportedSampleRateIndex(u8),

    /// [`crate::ics_info::IcsInfo::write`] was handed an in-memory
    /// [`crate::ics_info::IcsInfo`] whose field combination cannot
    /// be represented on the wire under ISO/IEC 14496-3 Table 4.6 /
    /// Table 4.55. Examples: `max_sfb` exceeds its field width
    /// (`> 15` for `EIGHT_SHORT_SEQUENCE`, `> 63` otherwise);
    /// `scale_factor_grouping == None` for `EIGHT_SHORT_SEQUENCE` or
    /// `Some(_)` for any other window sequence; a predictor / LTP
    /// body slot is populated while the dispatching
    /// `predictor_data_present` bit is zero, or vice versa; a
    /// non-Main AOT has `predictor_data` set instead of `ltp_data`;
    /// the paired-channel `ltp_data_present_pair` slot is populated
    /// while `common_window == false`; a `prediction_used[]` /
    /// `long_used[]` length differs from the spec-cap
    /// (`min(max_sfb, PRED_SFB_MAX[fs_index])` or
    /// `min(max_sfb, MAX_LTP_LONG_SFB)`); or a numeric field
    /// (`ltp_coef`, `ltp_lag`, `reset_group_number`) exceeds the
    /// width of its wire slot. A conforming AAC encoder never builds
    /// such a structure; this surfaces caller bugs at the boundary
    /// between psychoacoustic / windowing-decision code and bitstream
    /// emission.
    IcsInfoEncodeInvalid,

    /// [`crate::section_data::SectionData::parse`] read a section
    /// run-length (`sect_len`) that would extend a section past
    /// `max_sfb`. ISO/IEC 13818-7 §6.3 Table 17 terminates the
    /// per-group loop at `k < max_sfb`; a conforming encoder never
    /// emits a `sect_len` that overshoots, so this signals a
    /// malformed `section_data()`.
    SectionDataOverrun,

    /// [`crate::section_data::SectionData::write`] was handed an
    /// in-memory [`crate::section_data::SectionData`] whose
    /// per-group section list violates an invariant the encoder
    /// cannot represent on the wire — non-contiguous bands
    /// (`start != 0`, `end[i] != start[i+1]`, or last `end !=
    /// max_sfb`), a `sect_cb` greater than the 4-bit field, or a
    /// zero-length section that the §6.3 escape cannot terminate
    /// while preserving parser round-trip. A conforming AAC encoder
    /// never builds such a structure; this surfaces caller bugs at
    /// the boundary between scalefactor-grouping and section
    /// emission.
    SectionDataEncodeInvalid,

    /// [`crate::pulse_data::PulseData::write`] was handed an
    /// in-memory [`crate::pulse_data::PulseData`] whose field set
    /// cannot be represented on the wire under ISO/IEC 14496-3
    /// §4.4.6.3 Table 4.7. Examples: `pulses` is empty (the loop
    /// bound is `number_pulse + 1 >= 1`) or exceeds the 2-bit
    /// `number_pulse` field cap (`pulses.len() > 4`);
    /// `pulse_start_sfb > 0x3f` (6-bit overflow); a `Pulse::offset >
    /// 0x1f` (5-bit overflow) or `Pulse::amp > 0x0f` (4-bit
    /// overflow). A conforming AAC encoder never builds such a
    /// structure; this surfaces caller bugs at the boundary between
    /// the pulse-selection psychoacoustic stage and bitstream
    /// emission.
    PulseDataEncodeInvalid,

    /// [`crate::tns_data::TnsData::write`] was handed an in-memory
    /// [`crate::tns_data::TnsData`] whose field combination cannot
    /// be represented on the wire under ISO/IEC 14496-3 §4.4.6 /
    /// Table 4.54 (with the §4.6.9.2 Table 4.155 size switch).
    /// Examples: `windows.len()` differs from `num_windows` for the
    /// surrounding `window_sequence` (1 for long sequences, 8 for
    /// `EIGHT_SHORT_SEQUENCE`); per-window `filters.len()` exceeds
    /// the `n_filt` field cap (1 on `EIGHT_SHORT_SEQUENCE`, 3
    /// otherwise); a filter's `length` exceeds the `length` field
    /// cap (15 / 63); a filter's `order` exceeds the `order` field
    /// cap (7 / 31); the `coef[]` length differs from `order`; a
    /// coefficient magnitude exceeds the `(1 << coef_bits) - 1`
    /// cap (where `coef_bits = (3 + coef_res) - coef_compress`); a
    /// zero-`order` filter carries a non-default `direction` /
    /// `coef_compress` that would silently be dropped on the wire
    /// (those fields are not transmitted when `order == 0`). A
    /// conforming AAC encoder never builds such a structure; this
    /// surfaces caller bugs at the boundary between the TNS
    /// psychoacoustic-decision stage and bitstream emission.
    TnsDataEncodeInvalid,

    /// [`crate::scale_factor_data::ScaleFactorData::write`] was
    /// handed an in-memory record set whose shape cannot be
    /// represented on the wire under ISO/IEC 14496-3 §4.4.6 /
    /// Table 4.53 (non-resilient branch). Examples: the outer
    /// `entries.len()` does not match the supplied `sfb_cb.len()`;
    /// a group's entry count differs from the non-`ZERO_HCB` band
    /// count of the matching `sfb_cb` group; an entry variant
    /// does not match its band's codebook classification
    /// (e.g. [`crate::scale_factor_data::ScaleFactorEntry::Intensity`]
    /// paired with a spectrum band, or
    /// [`crate::scale_factor_data::ScaleFactorEntry::NoisePcm`] re-used
    /// after the §4.4.6 frame-scope `noise_pcm_flag` has already
    /// cleared, or
    /// [`crate::scale_factor_data::ScaleFactorEntry::NoiseDpcm`] used
    /// on the first PNS band of the frame); a DPCM delta falls
    /// outside `-60..=+60` (Table 4.150); or a `NoisePcm` magnitude
    /// exceeds the 9-bit field cap (`> 0x1ff`). A conforming AAC
    /// encoder never builds such a structure; this surfaces caller
    /// bugs at the boundary between the rate-allocation /
    /// scalefactor-quantisation stage and bitstream emission.
    ScaleFactorDataEncodeInvalid,

    /// An RVLC encode primitive ([`crate::rvlc::rvlc_encode`] /
    /// [`crate::rvlc::rvlc_esc_encode`]) was handed a value outside
    /// its codebook domain: a Table 4.166 RVLC delta outside
    /// `-7..=+7`, or a Table 4.168 escape magnitude index outside
    /// `0..=53` (ISO/IEC 14496-3 §4.6.16.2). A conforming
    /// error-resilient encoder never builds such a value; this
    /// surfaces caller bugs at the scalefactor-quantisation /
    /// emission boundary.
    RvlcEncodeInvalid,

    /// [`crate::rvlc::rvlc_decode`] read a Table 4.167 *asymmetric*
    /// (forbidden) codeword from the error-resilient
    /// `scale_factor_data()` RVLC part (ISO/IEC 14496-3 §4.6.16.2.1).
    /// Because the RVLC code tree leaves some nodes unused, hitting
    /// one is an in-band *error-detection* event — the stream's RVLC
    /// scalefactor data is corrupt.
    RvlcForbiddenCodeword,

    /// [`crate::rvlc::rvlc_esc_decode`] walked the full 20-bit
    /// Table 4.168 RVLC-ESC depth without matching any codeword
    /// (ISO/IEC 14496-3 §4.6.16.2). The escape part of the
    /// error-resilient `scale_factor_data()` is corrupt.
    RvlcEscInvalid,

    /// The error-resilient `scale_factor_data()` RVLC branch
    /// (ISO/IEC 14496-3 Table 4.53 / §4.6.16.2) violated a
    /// structural invariant: the decoded RVLC part did not consume
    /// exactly `length_of_rvlc_sf` bits, the escape part did not
    /// consume exactly `length_of_rvlc_escapes` bits, an escape was
    /// signalled for a non-`ESC_FLAG` band, or an in-memory record
    /// set handed to the writer cannot be represented (variant /
    /// codebook mismatch, escape magnitude out of range, or the
    /// `rev_global_gain` / DPCM-last seeds out of their field caps).
    RvlcScaleFactorDataInvalid,

    /// [`crate::pce::Pce::write`] was handed an in-memory
    /// [`crate::pce::Pce`] whose field combination cannot be
    /// represented on the wire under ISO/IEC 14496-3 §4.4.1.1 /
    /// Table 4.2. Examples: `element_instance_tag > 0x0f` (4-bit
    /// field cap); `object_type > 0x03` (2-bit field cap);
    /// `sampling_frequency_index > 0x0f` (4-bit field cap);
    /// `front_elements.len() > 0x0f`, `side_elements.len() > 0x0f`,
    /// or `back_elements.len() > 0x0f` (4-bit `num_*` field caps);
    /// `lfe_element_tag_selects.len() > 0x03` (2-bit `num_lfe`
    /// field cap); `assoc_data_tag_selects.len() > 0x07` (3-bit
    /// `num_assoc` field cap); `valid_cc_elements.len() > 0x0f`
    /// (4-bit `num_valid_cc` field cap); a `tag_select` inside any
    /// per-element list exceeds the 4-bit cap; a `matrix_mixdown`
    /// `idx > 0x03` (2-bit field cap); `mono_mixdown_element_number`
    /// or `stereo_mixdown_element_number` `> 0x0f` (4-bit caps); or
    /// `comment_field.len() > 0xff` (8-bit `comment_field_bytes`
    /// length prefix). A conforming AAC encoder never builds such a
    /// structure; this surfaces caller bugs at the boundary between
    /// channel-layout selection and bitstream emission.
    PceEncodeInvalid,

    /// [`crate::raw_data_block::FrameAssembler`] was handed an
    /// element whose field combination cannot be represented on the
    /// wire under ISO/IEC 14496-3 §4.4.2.1. Examples:
    /// [`crate::raw_data_block::FrameAssembler::push_channel_header`]
    /// was called with an `IdSynEle` other than `SCE` / `CPE` / `CCE`
    /// / `LFE` (those have their own dedicated `push_*` entry points
    /// because each carries a bespoke wire layout — FIL goes through
    /// [`crate::raw_data_block::FrameAssembler::push_fill`], DSE
    /// through
    /// [`crate::raw_data_block::FrameAssembler::push_data`], END
    /// through
    /// [`crate::raw_data_block::FrameAssembler::push_end`], and PCE
    /// has no writer yet); a channel-element `element_instance_tag`
    /// or DSE `element_instance_tag` exceeds the 4-bit field cap
    /// (`> 0x0f`); a FIL payload exceeds the 269-byte ceiling
    /// (`15 + 255 − 1`) imposed by the §4.4.2.7 8-bit `esc_count`
    /// field; a DSE payload exceeds the 510-byte ceiling
    /// (`255 + 255`) imposed by the §4.4.2.5 8-bit `esc_count`
    /// field; or
    /// [`crate::raw_data_block::FrameAssembler::push_channel_body_bits`]
    /// was called with `bit_count > bits.len() * 8`. Long fill /
    /// data payloads (above the per-element ceilings) split
    /// naturally across multiple back-to-back FIL / DSE elements
    /// with the same `tag`; that splitting is the caller's
    /// responsibility, not the assembler's.
    RawDataBlockEncodeInvalid,

    /// `epConfig` (from the Table 1.15 outer `switch (audioObjectType)`
    /// for the ER object types) selected value `2` or `3`, which
    /// mandates parsing the trailing `ErrorProtectionSpecificConfig()`
    /// body. Phase 1 does not parse the error-protection
    /// configuration; the carried `u8` is the literal 2-bit
    /// `epConfig` field value as read from the wire. `epConfig == 0`
    /// (no EP) and `epConfig == 1` (EP defined by EP class mapping
    /// table only — no trailing body) are accepted and surfaced via
    /// [`crate::asc::AudioSpecificConfig::ep_config`].
    UnsupportedEpConfig(u8),

    /// `extensionFlag3` was set to `1` inside the `GASpecificConfig`
    /// `extensionFlag` body (Table 4.1). ISO/IEC 14496-3:2009 reserves
    /// the body behind this flag with the comment "tbd in version 3";
    /// since the body bit-layout is not defined, Phase 1 cannot
    /// advance the bit-reader and rejects the ASC.
    UnsupportedAscExtensionFlag3,

    /// The Table 1.15 trailing `syncExtensionType == 0x2b7` probe
    /// resolved an `extensionAudioObjectType` whose body bit-layout
    /// is not specified by ISO/IEC 14496-3:2009 §1.6.2.1. The carrier
    /// only spells out two values: `5` (HE-AAC SBR with the optional
    /// `0x548` PS sub-probe) and `22` (ER BSAC with mandatory
    /// `extensionChannelConfiguration`); any other extension AOT
    /// resolved by `GetAudioObjectType()` inside the probe surfaces
    /// here. The carried `u8` is the resolved extension AOT.
    UnsupportedTrailingExtensionAot(u8),

    /// `extension_payload()` dispatched on an `extension_type` value
    /// whose body needs the SBR back-end this crate does not yet
    /// provide. The carried `u8` is the literal 4-bit
    /// `extension_type` value as read from the wire — one of
    /// `0b1101` (`EXT_SBR_DATA`) or `0b1110` (`EXT_SBR_DATA_CRC`)
    /// per ISO/IEC 13818-7 Table 40.
    UnsupportedExtensionSbr(u8),

    /// `extension_payload()` dispatched on a reserved
    /// `extension_type` value (any 4-bit value not in
    /// `{0b0000, 0b0001, 0b1011, 0b1101, 0b1110}`). ISO/IEC
    /// 14496-3 Table 4.59 and ISO/IEC 13818-7 Table 40 list these
    /// values as "reserved"; this crate has no body layout to
    /// advance the bit-reader by.
    UnsupportedExtensionType(u8),

    /// [`crate::extension_payload::ExtensionPayload`] parse / write
    /// hit a structural invariant violation:
    ///
    /// * The dispatching FIL `cnt` is 0 (no room for the 4-bit
    ///   `extension_type` field).
    /// * For `EXT_FILL` (parser / writer): an `other_bits` byte
    ///   buffer whose length does not match the
    ///   `8 * (cnt - 1) + 4` body-bits ceiling.
    /// * For `EXT_FILL_DATA` (parser): a `fill_nibble` that is not
    ///   normatively `0b0000`, or a `fill_byte` that is not
    ///   normatively `0b10100101`.
    /// * For `EXT_DYNAMIC_RANGE` (parser): the Table 4.52 derived
    ///   byte count `n` disagrees with the dispatching FIL `cnt`.
    /// * For `EXT_DYNAMIC_RANGE` (writer): a numeric field
    ///   overflows its Table 4.52 cap (`pce_instance_tag > 0x0f`,
    ///   `drc_tag_reserved_bits > 0x0f`, `drc_band_incr > 0x0f`,
    ///   `drc_bands_reserved_bits > 0x0f`, `prog_ref_level >
    ///   0x7f`, `dyn_rng_ctl > 0x7f`), an internal
    ///   shape-mismatch (`band_top.len() != 1 + band_incr`,
    ///   `bands.len() != drc_num_bands`), or an
    ///   `excluded_channels.exclude_mask.len()` that is not a
    ///   positive multiple of 7 (Table 4.53 emits exclusion bits
    ///   in fixed groups of 7).
    ExtensionPayloadInvalid,

    /// [`crate::gain_control_data::GainControlData::write`] was
    /// handed an in-memory
    /// [`crate::gain_control_data::GainControlData`] whose field
    /// combination cannot be represented on the wire under ISO/IEC
    /// 14496-3 §4.4.6.5 / Table 4.12. Examples: `max_band > 0x03`
    /// (2-bit field cap); `bands.len() != max_band` (the outer
    /// band-loop count must match the dispatched wire value);
    /// `band.windows.len()` differs from the per-`window_sequence`
    /// count (1 for `OnlyLong`, 2 for `LongStart` / `LongStop`, 8 for
    /// `EightShort`); a per-`(bd, wd)` `adjustments.len() > 7`
    /// (3-bit `adjust_num` field cap); a `GainAdjust::alevcode >
    /// 0x0f` (4-bit field cap); or a `GainAdjust::aloccode` exceeds
    /// the per-slot width-derived cap (5 bits for `OnlyLong wd=0`,
    /// 4 bits for `LongStart / LongStop wd=0`, 2 bits for
    /// `EightShort` and the `wd=1` slot of `LongStart`, 5 bits for
    /// the `wd=1` slot of `LongStop`). A conforming AAC SSR encoder
    /// never builds such a structure; this surfaces caller bugs at
    /// the boundary between the SSR PQF gain-control psychoacoustic
    /// stage and bitstream emission.
    GainControlDataEncodeInvalid,

    /// [`crate::scale_factor_data::differentiate`] was handed an
    /// [`crate::scale_factor_data::AbsoluteScaleFactors`] whose
    /// shape or numeric values cannot be encoded back to a
    /// well-formed `scale_factor_data()` block. Examples: outer
    /// length differs from `sfb_cb.len()`; a group's
    /// per-band-classification list differs from the matching
    /// `sfb_cb` group; the spectrum-track delta `sf - last_sf`
    /// falls outside Table 4.150's `-60..=+60`; the intensity-track
    /// delta `is_pos - last_is` falls outside `-60..=+60`; the
    /// PNS-track delta `nrg - last_nrg` (for PNS bands after the
    /// first) falls outside `-60..=+60`; or the first PNS band's
    /// initial seed magnitude (`first_nrg - (global_gain -
    /// NOISE_OFFSET - 256)`) does not fit the 9-bit Table 4.53
    /// `dpcm_noise_nrg` uimsbf field (`0..=511`). A conforming AAC
    /// rate-allocation stage never produces such a structure; this
    /// surfaces caller bugs at the boundary between absolute
    /// scalefactor quantisation and DPCM differential coding.
    ScaleFactorAccumulatorInvalid,

    /// [`crate::spectral_codebook::table_4_95`] (or any other
    /// public accessor in that module) was called with a `codebook`
    /// value `> 31`. ISO/IEC 14496-3 Table 4.95 only defines rows
    /// `0..=31`.
    SpectralCodebookOutOfRange(u8),

    /// [`crate::spectral_codebook::decode_index_to_tuple`] /
    /// [`crate::spectral_codebook::encode_tuple_to_index`] /
    /// [`crate::spectral_codebook::apply_sign_bits`] /
    /// [`crate::spectral_codebook::derive_sign_bits`] was called
    /// with a codebook whose Table 4.95 row carries no
    /// `unsigned_cb` / `dimension` / `lav` (`0`, `12`, `13`, `14`,
    /// `15`). Those are non-spectral books (`ZERO_HCB`, reserved,
    /// PNS, intensity stereo); §4.6.3.3 does not translate any
    /// codeword index for them.
    SpectralCodebookHasNoTuple(u8),

    /// [`crate::spectral_codebook::decode_index_to_tuple`] was
    /// called with a codeword index `idx >= mod^dim` where `mod =
    /// lav + 1` (unsigned) or `2 * lav + 1` (signed). A conforming
    /// Huffman decoder never produces such an index; this surfaces
    /// an incoherence between the Huffman tree and Table 4.95.
    SpectralCodebookIndexOutOfRange(u8),

    /// [`crate::spectral_codebook::encode_tuple_to_index`] /
    /// [`crate::spectral_codebook::derive_sign_bits`] was called
    /// with a tuple shorter than the codebook's dimension, or with
    /// an entry outside the codebook's representable range
    /// (`0..=lav` unsigned, `-lav..=+lav` signed). A conforming AAC
    /// encoder never produces such a tuple.
    SpectralCodebookTupleOutOfRange(u8),

    /// [`crate::spectral_codebook::apply_sign_bits`] was called
    /// with a `signs` slice whose length disagrees with the count
    /// of non-zero coefficients in the unsigned-codebook tuple, or
    /// with a non-empty `signs` slice on a signed codebook.
    SpectralCodebookSignBitsMismatch(u8),

    /// [`crate::spectral_codebook::decode_esc_value`] /
    /// [`crate::spectral_codebook::encode_esc_value`] was called
    /// with arguments outside the §4.6.3.3 ESC range: `prefix_len >
    /// 9`, `escape_word` not fitting `(prefix_len + 4)` bits, a
    /// decoded value exceeding `MAX_QUANT` (`8191`), or an encoder
    /// value `< 16` (which is in-band, not ESC-encoded).
    SpectralCodebookEscOutOfRange,

    /// [`crate::tns_coef::tns_decode_coef`] /
    /// [`crate::tns_coef::tns_encode_coef`] /
    /// [`crate::tns_coef::iqfac`] / [`crate::tns_coef::iqfac_m`] /
    /// [`crate::tns_coef::sign_extend_coef`] /
    /// [`crate::tns_coef::pack_coef`] was called with an argument
    /// outside the §4.6.9.3 / §C.6 legal range. Examples:
    /// `coef_res_bits` not in `{3, 4}` (the spec's `coef_res[w] + 3`
    /// envelope); `coef_compress > 1` (a 1-bit wire flag); a wire
    /// `coef[i]` value that does not fit in `coef_res2 =
    /// coef_res_bits - coef_compress` bits; a `pack_coef` `value`
    /// outside `-(1 << (coef_res2-1))..=(1 << (coef_res2-1)) - 1`;
    /// or an encode-side PARCOR coefficient `|r| > 1.0` (or NaN /
    /// ±∞) — `arcsin` is undefined outside `[-1, 1]`.
    TnsCoefOutOfRange,

    /// [`crate::tns_frame::tns_decode_frame`] was called with a
    /// frame-level argument combination that violates the §4.6.9.3
    /// `tns_decode_frame()` preconditions: the `spec` buffer length
    /// differs from `num_windows × window_len` (8 × 128 for
    /// `EIGHT_SHORT_SEQUENCE`, 1 × 1024 otherwise); the
    /// [`crate::tns_data::TnsData`] window count disagrees with the
    /// `window_sequence`; or a filter's `coef` vector is shorter than
    /// the `TNS_MAX_ORDER`-clamped `tns_order` it must supply. A
    /// [`crate::tns_data::TnsData`] produced by
    /// [`crate::tns_data::TnsData::parse`] under the same
    /// `window_sequence` never trips the structural checks — this
    /// surfaces caller-fabricated structures.
    TnsFrameInvalid,

    /// [`crate::spectral_data::SpectralData::parse`] (or the
    /// [`crate::spectral_data::sect_sfb_offset`] helper) found a
    /// structural violation of Table 4.56 / §4.5.2.3.4: `max_sfb`
    /// exceeding `num_swb` for the active window sequence, a
    /// [`crate::section_data::SectionData`] whose group count
    /// disagrees with the [`crate::ics_info::IcsInfo`], a section
    /// carrying the reserved codebook 12 into `spectral_data()`,
    /// or a section span that is not a whole number of
    /// `QUAD_LEN` / `PAIR_LEN` n-tuples.
    SpectralDataInvalid,

    /// [`crate::spectral_data::SpectralData::write`] was handed a
    /// coefficient buffer that cannot be represented on the wire:
    /// per-group buffer lengths disagreeing with
    /// `window_group_length[g] × window_len`, a non-zero coefficient
    /// inside a `ZERO_HCB` / `NOISE_HCB` / intensity section (or
    /// above `max_sfb`), or a magnitude exceeding the section
    /// codebook's LAV (`MAX_QUANT` = 8191 for the ESC book).
    SpectralDataEncodeInvalid,

    /// [`crate::dequant::rescale_spectrum`] found a structural
    /// mismatch between its inputs: group counts disagreeing with
    /// `num_window_groups`, a per-group `x_quant` buffer length
    /// disagreeing with the `ics_info` grouping, or an
    /// [`crate::scale_factor_data::AbsoluteScaleFactorEntry`]
    /// sequence that does not match the non-`ZERO_HCB` codebook
    /// classification of `sfb_cb` (including the reserved codebook
    /// 12, which has no spectrum semantics to rescale). Inputs
    /// produced by the wire parsers plus
    /// [`crate::scale_factor_data::accumulate`] under one shared
    /// `ics_info` / `section_data` never trip this — it surfaces
    /// caller-fabricated structures.
    DequantInvalid,

    /// [`crate::decoded_spectrum::quant_to_spec`] was handed a group
    /// buffer set whose shape disagrees with the `ics_info`
    /// grouping: wrong group count, a group buffer length that is
    /// not `window_group_length[g] × window_len`, or a
    /// `window_group_length[]` whose sum is not `num_windows`.
    QuantToSpecInvalid,

    /// [`crate::filterbank::Filterbank::synthesize`] was handed a
    /// window-major spectrum whose length disagrees with the
    /// [`crate::ics_info::IcsInfo`] `window_sequence`: a long
    /// sequence (`ONLY_LONG` / `LONG_START` / `LONG_STOP`) requires
    /// exactly [`crate::swb_offset::LONG_WINDOW_LEN`] (1024)
    /// coefficients, an `EIGHT_SHORT` sequence requires `8 ×`
    /// [`crate::swb_offset::SHORT_WINDOW_LEN`] (1024 total). The
    /// §4.6.11.3.1 IMDCT cannot run against any other length.
    FilterbankInvalid,

    /// [`crate::ms_stereo::apply_ms_stereo`] was handed a channel
    /// pair whose shapes disagree with the shared
    /// [`crate::ics_info::IcsInfo`]: the two window-major spectra
    /// have different lengths, a length that is not
    /// `num_windows × window_len`, an `ms_used` mask whose group
    /// count is not `num_window_groups` (or a per-group row shorter
    /// than `max_sfb`), or a per-channel `sfb_cb` whose group/band
    /// extents do not cover `max_sfb`. The §4.6.8.1.3 de-matrix is
    /// undefined without a consistent group/band geometry across
    /// both channels.
    MsStereoInvalid,
    /// [`crate::intensity_stereo::apply_intensity_stereo`] was handed a
    /// channel pair whose shapes disagree with the shared
    /// [`crate::ics_info::IcsInfo`]: the two window-major spectra have
    /// different lengths, a length that is not
    /// `num_windows × window_len`, an `ms_used` mask whose group count
    /// is not `num_window_groups` (or a per-group row shorter than
    /// `max_sfb`), a right-channel `sfb_cb` that does not cover
    /// `max_sfb`, or an `is_pos[g][sfb]` table whose group/band extents
    /// do not cover every intensity-coded band. The §4.6.8.2.3 scale
    /// `is_intensity · invert_intensity · 0.5^(0.25·is_pos)` is
    /// undefined without a consistent group/band geometry and an
    /// intensity-stereo position for every intensity band.
    IntensityStereoInvalid,
    /// [`crate::pns::apply_pns`] / [`crate::pns::apply_pns_pair`] was
    /// handed a channel (or pair) whose shapes disagree with the
    /// [`crate::ics_info::IcsInfo`]: a window-major spectrum whose
    /// length is not `num_windows × window_len`, a
    /// `window_group_length` whose sum is not `num_windows`, a
    /// `max_sfb` beyond the active window's band count, a `sfb_cb` or
    /// `noise_nrg` table whose group/band extents do not cover
    /// `max_sfb`, two paired channels with differing window geometry,
    /// or (for the pair) an `ms_used` mask whose group count is not
    /// `num_window_groups` (or a per-group row shorter than `max_sfb`).
    /// The §4.6.13.3 noise synthesis is undefined without a consistent
    /// group/band geometry and a `noise_nrg` for every noise band.
    PnsInvalid,
    /// [`crate::ltp::LtpState::apply_long`] was handed §4.6.7 Long-Term
    /// Prediction inputs that are mutually inconsistent: an `ltp_coef`
    /// index outside the Table 4.98 codebook (`> 7`), an active
    /// `ltp_long_used` mask with no transmitted `ltp_lag`, or a channel
    /// spectrum whose length is not `LONG_WINDOW_LEN` (1024). The
    /// §4.6.7.3 `X_rec = X_est + Y_rec` combination is undefined without
    /// a valid predictor coefficient, lag, and long-window spectrum.
    LtpInvalid,
    /// [`crate::element_decode`] was asked to decode a channel element
    /// whose component shapes are mutually inconsistent: a channel-pair
    /// element (`CPE`) whose two channels disagree on `window_sequence`
    /// (so the shared `common_window` geometry the §4.6.8 joint-stereo
    /// tools require is violated), an `ms_used` row that does not cover
    /// `num_window_groups × max_sfb`, or a per-channel
    /// `AbsoluteScaleFactors` whose wire-order record count does not
    /// match its `sfb_cb` non-`ZERO_HCB` band count when expanded to the
    /// band-indexed `is_pos[g][sfb]` / `noise_nrg[g][sfb]` layout the
    /// §4.6.8.2 / §4.6.13 synthesis passes consume. The element-level
    /// §4.6 block-order chain (de-quantise → M/S → intensity → PNS →
    /// TNS → filterbank) cannot run without a consistent geometry across
    /// the composed stages.
    ElementDecodeInvalid,
    /// [`crate::predictor::PredictorBank`] was handed §4.6.6
    /// frequency-domain-prediction inputs that are mutually inconsistent:
    /// a long-window scalefactor-band offset table too short to cover
    /// `PRED_SFB_MAX` for the sampling rate, a reconstructed spectrum
    /// shorter than the per-line predictor bank, or a
    /// `predictor_reset_group_number` outside the Table 4.97 range
    /// (`1 ..= 30`; the values `0` and `31` are reserved). The
    /// §4.6.6.3.2.1 `x_rec = x_est + y_rec` reconstruction and the
    /// §4.6.6.3.3 reset are undefined without a full predictor bank and a
    /// valid reset group.
    PredictorInvalid,
    /// SBR frequency-band-table derivation
    /// ([`crate::sbr_freq_bands`], §4.6.18.3.2) was handed parameters
    /// that violate a normative constraint:
    ///
    /// * `bs_start_freq` / `bs_stop_freq` outside their 4-bit ranges
    ///   (`0 ..= 15` each), an unsupported `FsSBR` (no offset /
    ///   `startMin` / `stopMin` row in §4.6.18.3.2.1), or
    ///   `bs_freq_scale` / `bs_alter_scale` / `bs_noise_bands`
    ///   outside their signalled ranges.
    /// * A derived geometry that breaks a §4.6.18.3.6 requirement:
    ///   `k2 <= k0` (`fMaster` undefined), `numBands <= 0`,
    ///   `k2 - k0` over the per-rate subband-count cap, `k_x > 32`,
    ///   `k_x + M > 64`, or `bs_xover_band >= NMaster`.
    ///
    /// The §4.6.18.3.2.1 master table and the §4.6.18.3.2.2 derived
    /// high / low / noise tables are undefined for such inputs.
    SbrFreqBandInvalid,
    /// SBR envelope / noise Huffman decode ([`crate::sbr_huffman`],
    /// §4.A.6.1 `sbr_huff_dec()`) could not match a codeword: either no
    /// table entry matched within the maximum SBR codeword length, or
    /// the bitstream ran out before a codeword completed. Both signal a
    /// corrupt or truncated SBR extension payload.
    SbrHuffInvalid,
    /// Parametric Stereo `ps_data()` parse ([`crate::ps_data`] /
    /// [`crate::ps_huffman`], ISO/IEC 14496-3:2009 §8.4.2 Table 8.9):
    /// a PS Huffman codeword failed to match within the Annex 8.B
    /// maximum length, the bitstream ran out mid-element, a reserved
    /// `iid_mode` / `icc_mode` was signalled, or a differentially
    /// decoded IID/ICC index left its Table 8.24/8.27 range. All
    /// signal a corrupt or truncated PS extension payload.
    PsDataInvalid,
    /// SBR time-frequency grid parse ([`crate::sbr_grid`], §4.4.2.8
    /// Tables 4.69–4.71) failed: the bitstream ran out mid-grid, or a
    /// frame class signalled an envelope count outside the
    /// §4.6.18.3.6 limit ([`crate::sbr_grid::SBR_MAX_NUM_ENV`]). Both
    /// signal a corrupt SBR data element.
    SbrGridInvalid,
    /// SBR QMF filterbank ([`crate::sbr_qmf`], §4.6.18.4) was handed a
    /// slot buffer of the wrong length: the analysis bank consumes
    /// exactly 32 time samples per slot, the synthesis bank exactly 64
    /// complex subband samples (32 for the downsampled variant).
    SbrQmfInvalid,
    /// Integer-PCM rendering ([`crate::pcm`], §4.6.11 output →
    /// §1.3 `NINT()`-rounded 16-bit word) was handed per-channel time
    /// signals of disagreeing length. [`crate::pcm::interleave_s16`]
    /// requires every channel buffer to carry the same per-frame sample
    /// count (the §4.6.11 transform length) so the interleave is
    /// well-defined.
    PcmInvalid,
    /// LATM `StreamMuxConfig()` ([`crate::latm`], ISO/IEC 14496-3
    /// §1.7.3 Table 1.42) signalled `audioMuxVersion == 1` with
    /// `audioMuxVersionA == 1`, which the spec marks reserved-for-
    /// future-extensions (`/* tbd */`). No syntax is defined for that
    /// branch, so the multiplex cannot be parsed.
    LatmAudioMuxVersionAReserved,
    /// LATM `StreamMuxConfig()` ([`crate::latm`], §1.7.3 Table 1.42)
    /// signalled a per-layer `frameLengthType` this decoder does not
    /// carry payload framing for. Only `0` (variable-length, byte
    /// count in `PayloadLengthInfo()`) and `1` (fixed `frameLength`
    /// bits) are supported; the CELP (`3`/`4`/`5`) and HVXC
    /// (`6`/`7`) types index frame-length tables this AAC-focused
    /// decoder does not implement. Carries the offending value.
    LatmUnsupportedFrameLengthType(u8),
    /// LATM multiplex configuration ([`crate::latm`], §1.7.3) exceeded
    /// one of the spec signalling caps: `numProgram > 15`,
    /// `numLayer > 7`, `numChunk > 15`, `streamCnt > 15`, or
    /// `numSubFrames` produced more PayloadMux frames than the bound.
    /// The fields are bit-limited on the wire so this only fires on a
    /// derived-count overflow or an internal inconsistency.
    LatmConfigOutOfRange,
    /// LATM `AudioMuxElement()` ([`crate::latm`], §1.7.3 Table 1.41)
    /// with `muxConfigPresent == 1` set `useSameStreamMux == 1` (apply
    /// previous configuration) but no `StreamMuxConfig()` had been
    /// decoded yet on this stream. The first in-band element must
    /// carry the configuration.
    LatmNoPreviousMuxConfig,
    /// LATM transport ([`crate::latm`], §1.7.3 Table 1.42) carried a
    /// `crcCheckSum` whose recomputed §1.8.4.5 `CRC8` value did not
    /// match the transmitted byte, indicating a corrupt
    /// `StreamMuxConfig()`.
    LatmCrcMismatch,
    /// LOAS `AudioSyncStream()` / `EPAudioSyncStream()`
    /// ([`crate::latm`], §1.7.2 Tables 1.36 / 1.37) sync search failed:
    /// the `0x2B7` / `0x4DE1` syncword was not found, or the
    /// `audioMuxLengthBytes` payload ran past the end of the buffer.
    LoasSyncInvalid,
    /// **No longer emitted.** A LATM/LOAS `AudioSpecificConfig` that
    /// signals SBR ([`crate::latm::LoasDecoder`]) now decodes through
    /// the shared §4.6.18 SBR back-end instead of being pre-rejected
    /// (a PS-signalling stream decodes its HE-AAC v1 layer). The
    /// variant is kept so existing `match` arms stay valid.
    LatmSbrUnsupported,
    /// `coupling_channel_element()` parse / reconstruction
    /// ([`crate::cce`], ISO/IEC 14496-3 §4.6.8.3 / Table 4.8) was handed
    /// a structurally inconsistent CCE:
    ///
    /// * a `num_coupled_elements` / `cc_target_is_cpe` / `cc_l` / `cc_r`
    ///   combination that derives a `num_gain_element_lists` other than
    ///   the count of transmitted gain lists,
    /// * an `ind_sw_cce_flag == 1` (independently switched) element that
    ///   carries a per-band `dpcm_gain_element` list instead of the
    ///   §4.6.8.3.3-required single `common_gain_element` per target, or
    /// * a coupled-target geometry (`num_window_groups` / `max_sfb` /
    ///   `swb_offset`) whose gain list does not cover the embedded
    ///   `single_channel_element()`'s band layout.
    ///
    /// The §4.6.8.3.3 `couple_channel()` scaling-and-add is undefined for
    /// such inputs.
    CceInvalid,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotImplemented => {
                write!(f, "oxideav-aac: feature not implemented in Phase 1")
            }
            Error::AdtsSyncNotFound => {
                write!(f, "ADTS sync word (0xFFF) not found")
            }
            Error::AdtsLayerNonZero => {
                write!(f, "ADTS layer field must be 0")
            }
            Error::AdtsReservedSampleRateIndex => {
                write!(
                    f,
                    "ADTS sampling_frequency_index is reserved (13, 14, or 15)"
                )
            }
            Error::AdtsFrameLengthTooSmall => {
                write!(f, "ADTS aac_frame_length is smaller than the header")
            }
            Error::UnexpectedEnd => {
                write!(f, "unexpected end of bitstream")
            }
            Error::UnsupportedElementSkip(id) => {
                write!(
                    f,
                    "raw_data_block walker cannot advance past id_syn_ele {} in Phase 1",
                    id
                )
            }
            Error::UnsupportedAot(aot) => {
                write!(
                    f,
                    "AudioSpecificConfig audioObjectType {} is not handled in Phase 1",
                    aot
                )
            }
            Error::IcsInfoUnsupportedSampleRateIndex(idx) => {
                write!(
                    f,
                    "ics_info sampling_frequency_index {} is outside the 0..=11 SWB-table range",
                    idx
                )
            }
            Error::IcsInfoEncodeInvalid => {
                write!(
                    f,
                    "ics_info encode: in-memory IcsInfo violates a Table 4.6 / 4.55 wire-field invariant"
                )
            }
            Error::SectionDataOverrun => {
                write!(
                    f,
                    "section_data sect_len overruns max_sfb (malformed bitstream)"
                )
            }
            Error::SectionDataEncodeInvalid => {
                write!(
                    f,
                    "section_data encode: per-group sections must be contiguous [0, max_sfb), sect_cb < 16, sect_len > 0"
                )
            }
            Error::PulseDataEncodeInvalid => {
                write!(
                    f,
                    "pulse_data encode: pulses.len() in 1..=4, pulse_start_sfb < 64, pulse_offset < 32, pulse_amp < 16"
                )
            }
            Error::TnsDataEncodeInvalid => {
                write!(
                    f,
                    "tns_data encode: in-memory TnsData violates a Table 4.54 / 4.155 wire-field invariant"
                )
            }
            Error::ScaleFactorDataEncodeInvalid => {
                write!(
                    f,
                    "scale_factor_data encode: in-memory record set violates a Table 4.53 / 4.150 wire-field invariant"
                )
            }
            Error::RvlcEncodeInvalid => {
                write!(
                    f,
                    "rvlc encode: value outside the Table 4.166 (-7..=+7) / Table 4.168 (0..=53) codebook domain"
                )
            }
            Error::RvlcForbiddenCodeword => {
                write!(
                    f,
                    "rvlc decode: read a Table 4.167 asymmetric (forbidden) codeword — RVLC scalefactor data is corrupt (§4.6.16.2.1)"
                )
            }
            Error::RvlcEscInvalid => {
                write!(
                    f,
                    "rvlc-esc decode: 20-bit Table 4.168 walk matched no codeword — RVLC escape data is corrupt (§4.6.16.2)"
                )
            }
            Error::RvlcScaleFactorDataInvalid => {
                write!(
                    f,
                    "error-resilient scale_factor_data: RVLC branch violates a Table 4.53 / §4.6.16.2 structural invariant"
                )
            }
            Error::PceEncodeInvalid => {
                write!(
                    f,
                    "pce encode: in-memory Pce violates a Table 4.2 wire-field invariant"
                )
            }
            Error::RawDataBlockEncodeInvalid => {
                write!(
                    f,
                    "raw_data_block encode: element field violates a §4.4.2.1 / §4.4.2.5 / §4.4.2.7 wire-field invariant"
                )
            }
            Error::GainControlDataEncodeInvalid => {
                write!(
                    f,
                    "gain_control_data encode: in-memory GainControlData violates a Table 4.12 wire-field invariant"
                )
            }
            Error::ScaleFactorAccumulatorInvalid => {
                write!(
                    f,
                    "scale_factor accumulator: absolute-to-DPCM differentiation produced a delta outside Table 4.150 / Table 4.53 ranges"
                )
            }
            Error::UnsupportedEpConfig(value) => {
                write!(
                    f,
                    "AudioSpecificConfig epConfig {} requires ErrorProtectionSpecificConfig parsing (Phase 1 supports only epConfig 0 and 1)",
                    value
                )
            }
            Error::UnsupportedAscExtensionFlag3 => {
                write!(
                    f,
                    "GASpecificConfig extensionFlag3 body is reserved (\"tbd in version 3\") and cannot be parsed"
                )
            }
            Error::UnsupportedTrailingExtensionAot(aot) => {
                write!(
                    f,
                    "AudioSpecificConfig trailing syncExtensionType=0x2b7 probe resolved extensionAudioObjectType {} (only 5 and 22 have a Table 1.15 body)",
                    aot
                )
            }
            Error::UnsupportedExtensionSbr(value) => {
                write!(
                    f,
                    "extension_payload extension_type 0x{:x} selects EXT_SBR_DATA / EXT_SBR_DATA_CRC; SBR back-end is not implemented",
                    value
                )
            }
            Error::UnsupportedExtensionType(value) => {
                write!(
                    f,
                    "extension_payload extension_type 0x{:x} is reserved (no body layout defined)",
                    value
                )
            }
            Error::ExtensionPayloadInvalid => {
                write!(
                    f,
                    "extension_payload: Table 4.51 / 4.52 / 4.53 / 4.59 wire-field invariant violated"
                )
            }
            Error::SpectralCodebookOutOfRange(cb) => {
                write!(
                    f,
                    "spectral codebook {} is outside Table 4.95 (legal range 0..=31)",
                    cb
                )
            }
            Error::SpectralCodebookHasNoTuple(cb) => {
                write!(
                    f,
                    "spectral codebook {} is non-spectral (Table 4.95 row carries no dim / lav)",
                    cb
                )
            }
            Error::SpectralCodebookIndexOutOfRange(cb) => {
                write!(
                    f,
                    "spectral codebook {}: codeword index out of Table 4.95 range",
                    cb
                )
            }
            Error::SpectralCodebookTupleOutOfRange(cb) => {
                write!(
                    f,
                    "spectral codebook {}: tuple length or value outside Table 4.95 dimension / lav",
                    cb
                )
            }
            Error::SpectralCodebookSignBitsMismatch(cb) => {
                write!(
                    f,
                    "spectral codebook {}: sign-bit count disagrees with non-zero coefficients in tuple",
                    cb
                )
            }
            Error::SpectralCodebookEscOutOfRange => {
                write!(
                    f,
                    "spectral codebook 11/16..=31 ESC sequence: prefix_len, escape_word, or magnitude outside §4.6.3.3 range"
                )
            }
            Error::TnsCoefOutOfRange => {
                write!(
                    f,
                    "tns_coef: coef_res_bits / coef_compress / wire coef / PARCOR value outside §4.6.9.3 / §C.6 legal range"
                )
            }
            Error::TnsFrameInvalid => {
                write!(
                    f,
                    "tns_decode_frame: spec length, TnsData window count, or per-filter coef length violates a §4.6.9.3 precondition"
                )
            }
            Error::SpectralDataInvalid => {
                write!(
                    f,
                    "spectral_data: max_sfb / section layout / codebook violates a Table 4.56 or §4.5.2.3.4 structural constraint"
                )
            }
            Error::SpectralDataEncodeInvalid => {
                write!(
                    f,
                    "spectral_data encode: coefficient buffer shape, zero-section content, or magnitude range cannot be represented per Table 4.56"
                )
            }
            Error::DequantInvalid => {
                write!(
                    f,
                    "rescale_spectrum: x_quant / scalefactor-entry / sfb_cb layout violates a §4.6.1.3 / §4.6.2.3.3 precondition"
                )
            }
            Error::QuantToSpecInvalid => {
                write!(
                    f,
                    "quant_to_spec: group buffer shape disagrees with the §4.5.2.3.4 ics_info grouping"
                )
            }
            Error::FilterbankInvalid => {
                write!(
                    f,
                    "filterbank: window-major spectrum length disagrees with the §4.6.11 window_sequence"
                )
            }
            Error::MsStereoInvalid => {
                write!(
                    f,
                    "M/S stereo: channel-pair spectra / ms_used / sfb_cb shapes disagree with the §4.6.8.1 ics_info geometry"
                )
            }
            Error::IntensityStereoInvalid => {
                write!(
                    f,
                    "intensity stereo: channel-pair spectra / ms_used / right sfb_cb / is_pos shapes disagree with the §4.6.8.2 ics_info geometry"
                )
            }
            Error::PnsInvalid => {
                write!(
                    f,
                    "PNS: channel spectrum / sfb_cb / noise_nrg / ms_used shapes disagree with the §4.6.13 ics_info geometry"
                )
            }
            Error::LtpInvalid => {
                write!(
                    f,
                    "LTP: ltp_coef index, ltp_lag presence, or long-window spectrum length disagree with the §4.6.7 decoding process"
                )
            }
            Error::ElementDecodeInvalid => {
                write!(
                    f,
                    "element decode: channel-element component shapes (window_sequence pairing, ms_used extent, or scalefactor-record count) are mutually inconsistent for the §4.6 block-order chain"
                )
            }
            Error::PredictorInvalid => {
                write!(
                    f,
                    "predictor: long-window offset table, spectrum length, or reset-group number disagree with the §4.6.6 frequency-domain prediction process"
                )
            }
            Error::SbrFreqBandInvalid => {
                write!(
                    f,
                    "SBR frequency bands: bs_start_freq/bs_stop_freq/bs_freq_scale, FsSBR, or the derived k0/k2 geometry violate a §4.6.18.3.2 / §4.6.18.3.6 constraint"
                )
            }
            Error::SbrHuffInvalid => {
                write!(
                    f,
                    "SBR Huffman decode: no §4.A.6.1 codeword matched (corrupt or truncated SBR envelope/noise payload)"
                )
            }
            Error::PsDataInvalid => {
                write!(
                    f,
                    "PS ps_data(): §8.4.2 Table 8.9 parse failed (unmatched Annex 8.B codeword, truncated payload, reserved iid/icc mode, or out-of-range index)"
                )
            }
            Error::SbrGridInvalid => {
                write!(
                    f,
                    "SBR grid: §4.4.2.8 sbr_grid/sbr_dtdf/sbr_invf ran out of bits or signalled an out-of-range envelope count"
                )
            }
            Error::SbrQmfInvalid => {
                write!(
                    f,
                    "SBR QMF: §4.6.18.4 filterbank slot buffer has the wrong length (analysis takes 32 samples, synthesis 64 complex bands, downsampled 32)"
                )
            }
            Error::PcmInvalid => {
                write!(
                    f,
                    "PCM interleave: per-channel time signals disagree in length"
                )
            }
            Error::LatmAudioMuxVersionAReserved => {
                write!(
                    f,
                    "LATM StreamMuxConfig: audioMuxVersionA == 1 is reserved (§1.7.3 Table 1.42 /* tbd */ branch)"
                )
            }
            Error::LatmUnsupportedFrameLengthType(t) => {
                write!(
                    f,
                    "LATM StreamMuxConfig: frameLengthType {t} (CELP/HVXC table-indexed framing) is unsupported; only 0 and 1 are carried"
                )
            }
            Error::LatmConfigOutOfRange => {
                write!(
                    f,
                    "LATM StreamMuxConfig: a multiplex count (numProgram/numLayer/numChunk/streamCnt/numSubFrames) exceeded the §1.7.3 signalling cap"
                )
            }
            Error::LatmNoPreviousMuxConfig => {
                write!(
                    f,
                    "LATM AudioMuxElement: useSameStreamMux == 1 but no previous StreamMuxConfig() has been decoded"
                )
            }
            Error::LatmCrcMismatch => {
                write!(
                    f,
                    "LATM StreamMuxConfig: recomputed §1.8.4.5 CRC8 does not match the transmitted crcCheckSum"
                )
            }
            Error::LoasSyncInvalid => {
                write!(
                    f,
                    "LOAS AudioSyncStream: §1.7.2 0x2B7/0x4DE1 syncword not found or audioMuxLengthBytes overruns the buffer"
                )
            }
            Error::LatmSbrUnsupported => {
                write!(
                    f,
                    "LATM AudioSpecificConfig signalled SBR/PS, which the core LATM PCM driver does not decode"
                )
            }
            Error::CceInvalid => {
                write!(
                    f,
                    "coupling_channel_element() has an inconsistent gain-list / target geometry (§4.6.8.3)"
                )
            }
        }
    }
}

impl std::error::Error for Error {}
