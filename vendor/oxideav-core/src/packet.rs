//! Compressed-data packet passed between demuxer → decoder and encoder → muxer.

use crate::time::TimeBase;

/// Metadata flags on a packet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PacketFlags {
    /// Packet is (or starts) a keyframe / random-access point.
    pub keyframe: bool,
    /// Packet holds codec-level headers rather than media data.
    pub header: bool,
    /// Packet's data may be corrupt but decode should still be attempted.
    pub corrupt: bool,
    /// Packet should be discarded (e.g., decoder delay padding).
    pub discard: bool,
    /// Packet is the last in its source container's natural framing unit
    /// (Ogg page, MP4 chunk, MKV cluster, …). Container muxers may use this
    /// signal to recreate similar boundaries in their output. Decoders
    /// should ignore it.
    pub unit_boundary: bool,
}

/// A chunk of compressed (encoded) data belonging to one stream.
#[derive(Clone, Debug)]
pub struct Packet {
    /// Stream index this packet belongs to.
    pub stream_index: u32,
    /// Time base in which `pts` and `dts` are expressed.
    pub time_base: TimeBase,
    /// Presentation timestamp (display order). `None` if unknown.
    pub pts: Option<i64>,
    /// Decode timestamp (decode order). Often equal to `pts` for intra-only codecs.
    pub dts: Option<i64>,
    /// Packet duration in `time_base` units, or `None` if unknown.
    pub duration: Option<i64>,
    /// Flags describing this packet.
    pub flags: PacketFlags,
    /// Compressed payload.
    pub data: Vec<u8>,
}

impl Packet {
    /// Construct a packet with the given payload and no timing
    /// information (all timestamps `None`, default flags).
    pub fn new(stream_index: u32, time_base: TimeBase, data: Vec<u8>) -> Self {
        Self {
            stream_index,
            time_base,
            pts: None,
            dts: None,
            duration: None,
            flags: PacketFlags::default(),
            data,
        }
    }

    /// Builder: set the presentation timestamp (in `time_base` units).
    pub fn with_pts(mut self, pts: i64) -> Self {
        self.pts = Some(pts);
        self
    }

    /// Builder: set the decode timestamp (in `time_base` units).
    pub fn with_dts(mut self, dts: i64) -> Self {
        self.dts = Some(dts);
        self
    }

    /// Builder: set the packet duration (in `time_base` units).
    pub fn with_duration(mut self, d: i64) -> Self {
        self.duration = Some(d);
        self
    }

    /// Builder: mark (or unmark) the packet as a keyframe /
    /// random-access point.
    pub fn with_keyframe(mut self, kf: bool) -> Self {
        self.flags.keyframe = kf;
        self
    }

    /// Mark this packet as carrying codec-level headers rather than
    /// media data (extradata, parameter sets, codec-private blobs).
    pub fn with_header(mut self, header: bool) -> Self {
        self.flags.header = header;
        self
    }

    /// Mark this packet's payload as possibly corrupt. Decoders should
    /// still attempt to decode it but may produce best-effort output.
    pub fn with_corrupt(mut self, corrupt: bool) -> Self {
        self.flags.corrupt = corrupt;
        self
    }

    /// Mark this packet for downstream discard (e.g. decoder delay
    /// padding, encoder priming samples, ASS dialogue tags shipped only
    /// for muxer round-trip).
    pub fn with_discard(mut self, discard: bool) -> Self {
        self.flags.discard = discard;
        self
    }

    /// Mark this packet as the last entry inside its source container's
    /// natural framing unit (Ogg page, MP4 chunk, MKV cluster). Decoders
    /// ignore the flag; muxers may use it to recreate similar
    /// boundaries in their output.
    pub fn with_unit_boundary(mut self, boundary: bool) -> Self {
        self.flags.unit_boundary = boundary;
        self
    }

    /// Replace this packet's full flag set in one call. Useful for
    /// demuxers that compute flags up front and want a single setter
    /// rather than four chained builder calls.
    pub fn with_flags(mut self, flags: PacketFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Override the packet's stream index. Builder-style chainable
    /// counterpart to the public field, for cases where the demuxer
    /// builds packets with a placeholder stream index and remaps them
    /// to the final index downstream.
    pub fn with_stream_index(mut self, stream_index: u32) -> Self {
        self.stream_index = stream_index;
        self
    }

    /// Override the packet's time base. Builder-style chainable
    /// counterpart to the public field, for cases where the time base
    /// isn't known at construction time (e.g. a remuxer rescaling all
    /// packets onto a unified output base).
    pub fn with_time_base(mut self, time_base: TimeBase) -> Self {
        self.time_base = time_base;
        self
    }

    /// Compute the packet's end PTS (`pts + duration`) when both are
    /// known. Returns `None` if either is missing, or if the sum would
    /// overflow `i64`. Useful for muxers that need to derive a per-
    /// packet end timestamp without recomputing it at every call site.
    pub fn end_pts(&self) -> Option<i64> {
        self.pts
            .zip(self.duration)
            .and_then(|(p, d)| p.checked_add(d))
    }

    /// Convenience accessor: `true` when [`PacketFlags::keyframe`] is
    /// set. Mirrors the builder pair `with_keyframe(true)`.
    pub fn is_keyframe(&self) -> bool {
        self.flags.keyframe
    }

    /// Convenience accessor: `true` when [`PacketFlags::header`] is set
    /// (the packet carries codec-level headers rather than media data).
    pub fn is_header(&self) -> bool {
        self.flags.header
    }

    /// Convenience accessor: `true` when [`PacketFlags::discard`] is
    /// set (downstream consumers should drop the packet).
    pub fn is_discard(&self) -> bool {
        self.flags.discard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tb() -> TimeBase {
        TimeBase::new(1, 1000)
    }

    #[test]
    fn new_packet_has_default_flags_and_no_timing() {
        let p = Packet::new(3, tb(), vec![1, 2, 3]);
        assert_eq!(p.stream_index, 3);
        assert_eq!(p.time_base, tb());
        assert!(p.pts.is_none());
        assert!(p.dts.is_none());
        assert!(p.duration.is_none());
        assert_eq!(p.flags, PacketFlags::default());
        assert_eq!(p.data, vec![1, 2, 3]);
        // All accessor convenience helpers default to false.
        assert!(!p.is_keyframe());
        assert!(!p.is_header());
        assert!(!p.is_discard());
    }

    #[test]
    fn builder_chain_sets_every_flag_field() {
        let p = Packet::new(0, tb(), vec![])
            .with_keyframe(true)
            .with_header(true)
            .with_corrupt(true)
            .with_discard(true)
            .with_unit_boundary(true);
        assert!(p.flags.keyframe);
        assert!(p.flags.header);
        assert!(p.flags.corrupt);
        assert!(p.flags.discard);
        assert!(p.flags.unit_boundary);
        assert!(p.is_keyframe());
        assert!(p.is_header());
        assert!(p.is_discard());
    }

    #[test]
    fn with_flags_replaces_full_flag_set() {
        let flags = PacketFlags {
            keyframe: true,
            header: false,
            corrupt: true,
            discard: false,
            unit_boundary: true,
        };
        let p = Packet::new(0, tb(), vec![]).with_flags(flags);
        assert_eq!(p.flags, flags);
        // A second with_flags wipes the prior set rather than OR-ing.
        let cleared = p.with_flags(PacketFlags::default());
        assert_eq!(cleared.flags, PacketFlags::default());
    }

    #[test]
    fn with_stream_index_and_time_base_override() {
        let original = TimeBase::new(1, 1);
        let replacement = TimeBase::new(1, 90_000);
        let p = Packet::new(0, original, vec![])
            .with_stream_index(7)
            .with_time_base(replacement);
        assert_eq!(p.stream_index, 7);
        assert_eq!(p.time_base, replacement);
    }

    #[test]
    fn end_pts_requires_both_pts_and_duration() {
        // Neither set.
        assert_eq!(Packet::new(0, tb(), vec![]).end_pts(), None);
        // pts only.
        assert_eq!(Packet::new(0, tb(), vec![]).with_pts(100).end_pts(), None);
        // duration only.
        assert_eq!(
            Packet::new(0, tb(), vec![]).with_duration(50).end_pts(),
            None
        );
        // Both: returns pts + duration.
        assert_eq!(
            Packet::new(0, tb(), vec![])
                .with_pts(100)
                .with_duration(50)
                .end_pts(),
            Some(150)
        );
    }

    #[test]
    fn end_pts_saturates_on_overflow() {
        // pts + duration would overflow i64::MAX; checked_add returns
        // None so end_pts surfaces that instead of wrapping.
        let p = Packet::new(0, tb(), vec![])
            .with_pts(i64::MAX - 1)
            .with_duration(10);
        assert_eq!(p.end_pts(), None);
    }

    #[test]
    fn end_pts_handles_negative_pts() {
        // Negative pts is legal (B-frames pre-roll); ensure the sum
        // still works through zero.
        let p = Packet::new(0, tb(), vec![]).with_pts(-25).with_duration(40);
        assert_eq!(p.end_pts(), Some(15));
    }
}
