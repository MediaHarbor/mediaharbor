//! Canonical multichannel output ordering — ISO/IEC 14496-3 Table 1.19.
//!
//! A `raw_data_block()` lists its channel elements (SCE / CPE / LFE) in
//! **bitstream order**, and [`crate::decode::StreamDecoder`] decodes each
//! element's time signal into that same element order. For the default
//! `channelConfiguration` values 1–7 (Table 1.19) the spec fixes which
//! loudspeaker each element feeds, but the loudspeaker order is *not* the
//! order a downstream interleaved-PCM sink expects: a 5.1 decoder emits
//! its elements as `SCE(C), CPE(L,R), CPE(Ls,Rs), LFE` — speaker order
//! `[C, L, R, Ls, Rs, LFE]` — whereas the canonical interleaved layout
//! is `[L, R, C, LFE, Ls, Rs]` (the WAVE_FORMAT_EXTENSIBLE / BS.775
//! convention that [`oxideav_core::ChannelLayout::Surround51`] adopts).
//!
//! This module owns the mapping from a `channelConfiguration` to:
//!
//! * the canonical [`ChannelLayout`] it denotes ([`layout_for_config`]),
//!   and
//! * the **permutation** that reorders the element-order channel buffers
//!   into that layout's canonical order ([`reorder_permutation`]).
//!
//! ## Element → speaker mapping (Table 1.19)
//!
//! Table 1.19's "channel to speaker mapping" column, read against the
//! "audio syntactic elements, listed in order received" column, gives the
//! per-element speaker assignment used here:
//!
//! | cfg | elements (in order)            | element speaker order            |
//! |-----|--------------------------------|----------------------------------|
//! | 1   | SCE                            | `[C]`                            |
//! | 2   | CPE                            | `[L, R]`                         |
//! | 3   | SCE, CPE                       | `[C, L, R]`                      |
//! | 4   | SCE, CPE, SCE                  | `[C, L, R, Cs]`                  |
//! | 5   | SCE, CPE, CPE                  | `[C, L, R, Ls, Rs]`              |
//! | 6   | SCE, CPE, CPE, LFE             | `[C, L, R, Ls, Rs, LFE]`         |
//!
//! Each `ChannelPosition` in that element order is then matched to its
//! slot in the canonical layout (`ChannelLayout::positions()`), producing
//! the index permutation. The reorder is applied by the decode driver
//! before interleaving (see [`crate::decode`]).
//!
//! `channelConfiguration == 0` (custom layout, carried by a
//! `program_config_element`) and `channelConfiguration == 7` (the 7.1
//! layout, whose element→speaker mapping differs between ISO/IEC
//! 14496-3 amendments and the encoders that emit it) are **not** handled
//! here: [`reorder_permutation`] returns `None`, and the driver keeps the
//! bitstream element order unchanged for those.
//!
//! ## Clean-room provenance
//!
//! The element list and speaker mapping are transcribed from ISO/IEC
//! 14496-3:2009 §1.6.3.5 Table 1.19. The canonical interleaved order is
//! the WAVE_FORMAT_EXTENSIBLE / ITU-R BS.775 convention already encoded
//! in [`oxideav_core::ChannelLayout`].

use oxideav_core::{ChannelLayout, ChannelPosition};

/// The canonical [`ChannelLayout`] denoted by a Table 1.19
/// `channelConfiguration`, for the default values this crate reorders
/// (1–6). Returns `None` for `0` (PCE-defined), `7` (amendment-specific
/// 7.1), and any reserved value `≥ 8`.
#[must_use]
pub fn layout_for_config(channel_configuration: u8) -> Option<ChannelLayout> {
    Some(match channel_configuration {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        3 => ChannelLayout::Surround30,
        4 => ChannelLayout::Surround40,
        5 => ChannelLayout::Surround50,
        6 => ChannelLayout::Surround51,
        _ => return None,
    })
}

/// The Table 1.19 per-element speaker order for a default
/// `channelConfiguration` — the loudspeaker each decoded channel feeds,
/// in the order the elements appear in the `raw_data_block()`.
///
/// Returns `None` for the configurations this crate does not reorder
/// (`0`, `7`, and reserved values).
#[must_use]
pub fn element_speaker_order(channel_configuration: u8) -> Option<&'static [ChannelPosition]> {
    use ChannelPosition::*;
    Some(match channel_configuration {
        1 => &[FrontCenter],
        2 => &[FrontLeft, FrontRight],
        3 => &[FrontCenter, FrontLeft, FrontRight],
        4 => &[FrontCenter, FrontLeft, FrontRight, BackCenter],
        5 => &[FrontCenter, FrontLeft, FrontRight, SideLeft, SideRight],
        6 => &[
            FrontCenter,
            FrontLeft,
            FrontRight,
            SideLeft,
            SideRight,
            LowFrequency,
        ],
        _ => return None,
    })
}

/// The permutation that reorders element-order channel buffers into the
/// canonical [`ChannelLayout`] order for a default `channelConfiguration`.
///
/// The returned vector `perm` has one entry per output channel: output
/// slot `i` (in canonical layout order) is sourced from element-order
/// channel `perm[i]`. Applying it is `out[i] = channels[perm[i]]`.
///
/// Returns `None` when no reordering is defined for this configuration
/// (`0`, `7`, reserved) — the caller keeps the bitstream element order.
/// An identity permutation (configs 1 and 2, where element order already
/// matches the canonical order) is returned as `Some(vec![0, 1, …])` so
/// the caller can still validate the channel count.
#[must_use]
pub fn reorder_permutation(channel_configuration: u8) -> Option<Vec<usize>> {
    let element_order = element_speaker_order(channel_configuration)?;
    let layout = layout_for_config(channel_configuration)?;
    let canonical = layout.positions();
    // For every canonical output slot, find the element that feeds that
    // speaker. Each speaker appears exactly once in both lists, so the
    // search is total and unambiguous.
    let mut perm = Vec::with_capacity(canonical.len());
    for &want in canonical {
        let src = element_order.iter().position(|&p| p == want)?;
        perm.push(src);
    }
    Some(perm)
}

/// Apply [`reorder_permutation`] to a set of element-order channel
/// buffers, returning the reordered set. When no permutation is defined
/// for `channel_configuration`, or the channel count does not match the
/// permutation length, the input order is preserved (returned unchanged).
///
/// This is the entry point the decode driver calls once a frame's
/// element-order channels are assembled.
#[must_use]
pub fn reorder_channels<T>(channel_configuration: u8, channels: Vec<Vec<T>>) -> Vec<Vec<T>> {
    let Some(perm) = reorder_permutation(channel_configuration) else {
        return channels;
    };
    if perm.len() != channels.len() {
        // Element count disagrees with the signalled configuration (a
        // malformed or PCE-overridden stream); leave the order untouched
        // rather than drop or duplicate a channel.
        return channels;
    }
    // `perm[i]` is the source slot for output slot `i`. Move each source
    // buffer into its destination exactly once (the permutation is a
    // bijection over `0..len`).
    let mut slots: Vec<Option<Vec<T>>> = channels.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(perm.len());
    for &src in &perm {
        out.push(
            slots[src]
                .take()
                .expect("reorder permutation is a bijection over the channel slots"),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::ChannelPosition::*;

    #[test]
    fn mono_and_stereo_are_identity() {
        assert_eq!(reorder_permutation(1), Some(vec![0]));
        assert_eq!(reorder_permutation(2), Some(vec![0, 1]));
    }

    #[test]
    fn surround30_moves_center_to_third_slot() {
        // element order [C, L, R] -> canonical [L, R, C]
        assert_eq!(reorder_permutation(3), Some(vec![1, 2, 0]));
    }

    #[test]
    fn surround40_keeps_back_center_last() {
        // element order [C, L, R, Cs] -> canonical [L, R, C, Cs]
        assert_eq!(reorder_permutation(4), Some(vec![1, 2, 0, 3]));
    }

    #[test]
    fn surround50_orders_front_then_surround() {
        // element order [C, L, R, Ls, Rs] -> canonical [L, R, C, Ls, Rs]
        assert_eq!(reorder_permutation(5), Some(vec![1, 2, 0, 3, 4]));
    }

    #[test]
    fn surround51_interleaves_lfe_before_surround() {
        // element order [C, L, R, Ls, Rs, LFE] -> canonical
        // [L, R, C, LFE, Ls, Rs]
        assert_eq!(reorder_permutation(6), Some(vec![1, 2, 0, 5, 3, 4]));
    }

    #[test]
    fn config_zero_and_seven_and_reserved_are_unmapped() {
        assert_eq!(reorder_permutation(0), None);
        assert_eq!(reorder_permutation(7), None);
        assert_eq!(reorder_permutation(8), None);
        assert_eq!(reorder_permutation(15), None);
        assert_eq!(layout_for_config(0), None);
        assert_eq!(layout_for_config(7), None);
    }

    #[test]
    fn permutation_matches_layout_positions() {
        // The permutation must land each element on the layout slot whose
        // ChannelPosition equals the element's Table 1.19 speaker.
        for cfg in 1..=6u8 {
            let perm = reorder_permutation(cfg).unwrap();
            let elem = element_speaker_order(cfg).unwrap();
            let layout = layout_for_config(cfg).unwrap();
            let canonical = layout.positions();
            assert_eq!(perm.len(), canonical.len(), "cfg {cfg} length");
            assert_eq!(canonical.len(), elem.len(), "cfg {cfg} element count");
            for (out_slot, &src) in perm.iter().enumerate() {
                assert_eq!(
                    elem[src], canonical[out_slot],
                    "cfg {cfg}: output slot {out_slot} mismatched speaker"
                );
            }
        }
    }

    #[test]
    fn layout_channel_counts_agree_with_element_order() {
        for cfg in 1..=6u8 {
            let layout = layout_for_config(cfg).unwrap();
            let elem = element_speaker_order(cfg).unwrap();
            assert_eq!(
                usize::from(layout.channel_count()),
                elem.len(),
                "cfg {cfg} channel count"
            );
        }
    }

    #[test]
    fn reorder_channels_permutes_buffers() {
        // 5.1 element order [C, L, R, Ls, Rs, LFE] tagged by a sentinel
        // sample so we can see where each lands.
        let channels: Vec<Vec<i16>> = vec![
            vec![0], // C
            vec![1], // L
            vec![2], // R
            vec![3], // Ls
            vec![4], // Rs
            vec![5], // LFE
        ];
        let out = reorder_channels(6, channels);
        // canonical [L, R, C, LFE, Ls, Rs] = [1, 2, 0, 5, 3, 4]
        let got: Vec<i16> = out.iter().map(|c| c[0]).collect();
        assert_eq!(got, vec![1, 2, 0, 5, 3, 4]);
    }

    #[test]
    fn reorder_channels_passthrough_on_unmapped_config() {
        let channels: Vec<Vec<i16>> = vec![vec![9], vec![8]];
        let out = reorder_channels(0, channels.clone());
        assert_eq!(out, channels);
    }

    #[test]
    fn reorder_channels_passthrough_on_count_mismatch() {
        // cfg 6 expects 6 channels; a 4-channel input is left untouched.
        let channels: Vec<Vec<i16>> = vec![vec![0], vec![1], vec![2], vec![3]];
        let out = reorder_channels(6, channels.clone());
        assert_eq!(out, channels);
    }

    #[test]
    fn every_speaker_in_canonical_appears_in_element_order() {
        // Guards the bijection assumption reorder_channels relies on.
        for cfg in 1..=6u8 {
            let elem = element_speaker_order(cfg).unwrap();
            let layout = layout_for_config(cfg).unwrap();
            for &pos in layout.positions() {
                assert!(
                    elem.contains(&pos),
                    "cfg {cfg}: canonical speaker {pos:?} missing from element order"
                );
            }
        }
        // Sanity: a position only present in a higher layout is absent.
        let elem5 = element_speaker_order(5).unwrap();
        assert!(!elem5.contains(&LowFrequency));
    }
}
