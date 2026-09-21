//! Decoder DoS-protection limits.
//!
//! [`DecoderLimits`] is a small `Copy + Default` configuration struct
//! threaded through [`CodecParameters`](crate::CodecParameters) so every
//! decoder constructed from a stream sees the same caps. Each cap is a
//! conservative default chosen to be generous enough that no real-world
//! file trips it but tight enough that a malicious input (huge declared
//! dimensions in a tiny container, decompression bombs, etc.) returns
//! [`Error::ResourceExhausted`](crate::Error::ResourceExhausted) instead
//! of OOM-ing the process.
//!
//! Two layers consume these caps:
//!
//! 1. **Header-parse layer.** Every decoder, immediately after parsing
//!    a stream/sequence header that declares dimensions, channel/group
//!    counts, or sample-rate × duration products, must check those
//!    declared values against [`DecoderLimits::max_pixels_per_frame`] /
//!    [`DecoderLimits::max_decoded_audio_seconds_per_packet`] *before*
//!    any allocation. A 1 GiB declared frame in a 4 KiB file should
//!    error here without ever calling `Vec::with_capacity`.
//!
//! 2. **Arena layer.** [`ArenaPool`](crate::arena::ArenaPool) honours
//!    [`DecoderLimits::max_arenas_in_flight`] (pool size) and
//!    [`DecoderLimits::max_alloc_bytes_per_frame`] (arena capacity).
//!    [`DecoderLimits::max_alloc_count_per_frame`] catches small-alloc
//!    DoS where each individual allocation is tiny but the count grows
//!    unbounded (e.g. one alloc per macroblock × millions of macroblocks).
//!
//! The struct is `Copy` so threading it through call chains never
//! involves clones or refcounts. It is also `#[non_exhaustive]` so
//! additional caps can be added without a semver break — construct
//! defaults with [`DecoderLimits::default`] and use the builder methods
//! to tighten individual fields.

/// Caps that bound a single decoder's peak resource use.
///
/// Defaults are intentionally **generous** (32 k × 32 k pixels, 1 GiB
/// per arena, 60 s of decoded audio per packet, …) so existing
/// real-world media decodes unchanged. Callers wanting tighter bounds
/// (e.g. a server processing untrusted uploads) should construct
/// `DecoderLimits` explicitly with the builder methods.
///
/// `Copy` and `Default` so the struct travels through hot paths
/// without indirection. `#[non_exhaustive]` so future caps can be
/// added without breaking semver — use [`DecoderLimits::default`] and
/// the `with_*` builder methods rather than struct-literal syntax.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DecoderLimits {
    /// Hard cap on `width × height` for a single decoded video frame.
    /// Header-parse code computes this product (using `u64` to avoid
    /// `u32::MAX × u32::MAX` overflow) and compares against this cap
    /// before allocating any plane. Default: `32_768 × 32_768` =
    /// `1_073_741_824` pixels (4 GiB at 32-bpp / 1 GiB at 8-bpp).
    pub max_pixels_per_frame: u64,

    /// Hard cap on the total bytes any single decoded frame may
    /// consume across all of its plane allocations. Also defines the
    /// per-arena capacity — see
    /// [`crate::arena::ArenaPool::new`]. Default: `1 GiB`. Tighter
    /// than `max_pixels_per_frame × bytes_per_pixel` for catching
    /// pathological pixel formats (e.g. a 16-bit-per-channel RGBA
    /// surface at near-cap dimensions).
    pub max_alloc_bytes_per_frame: u64,

    /// Hard cap on the *count* of allocations performed inside a
    /// single arena, regardless of total bytes. Catches small-alloc
    /// DoS (e.g. one alloc per macroblock × millions of macroblocks
    /// where the bytes-per-frame check would be too loose to fire).
    /// Default: `1_000_000` allocations.
    pub max_alloc_count_per_frame: u32,

    /// Hard cap on how many arenas a single decoder may have in
    /// flight at once — i.e. the size of the per-decoder
    /// [`ArenaPool`](crate::arena::ArenaPool). When all arenas are
    /// checked out the next `lease()` returns
    /// [`Error::ResourceExhausted`](crate::Error::ResourceExhausted),
    /// providing automatic backpressure: a slow downstream consumer
    /// stalls the decoder rather than letting it grow memory
    /// unboundedly. Default: `8` arenas.
    pub max_arenas_in_flight: u8,

    /// Audio-only cap on the wall-clock duration (in seconds) of
    /// decoded samples a single packet may produce. Header-parse
    /// code computes `(samples_per_frame × frames_per_packet) /
    /// sample_rate` and rejects packets whose declared output
    /// exceeds this. Default: `60` seconds — far more than any
    /// real-world AAC/Opus/etc. packet would ever produce, but
    /// finite enough to refuse a malformed packet that claims
    /// hours of output.
    pub max_decoded_audio_seconds_per_packet: u32,
}

impl Default for DecoderLimits {
    fn default() -> Self {
        Self {
            max_pixels_per_frame: 32_768u64 * 32_768u64,
            max_alloc_bytes_per_frame: 1u64 << 30, // 1 GiB
            max_alloc_count_per_frame: 1_000_000,
            max_arenas_in_flight: 8,
            max_decoded_audio_seconds_per_packet: 60,
        }
    }
}

impl DecoderLimits {
    /// Tighten the per-frame pixel cap. See
    /// [`DecoderLimits::max_pixels_per_frame`].
    pub fn with_max_pixels_per_frame(mut self, n: u64) -> Self {
        self.max_pixels_per_frame = n;
        self
    }

    /// Tighten the per-frame allocation byte cap (also defines arena
    /// capacity). See [`DecoderLimits::max_alloc_bytes_per_frame`].
    pub fn with_max_alloc_bytes_per_frame(mut self, n: u64) -> Self {
        self.max_alloc_bytes_per_frame = n;
        self
    }

    /// Tighten the per-frame allocation count cap. See
    /// [`DecoderLimits::max_alloc_count_per_frame`].
    pub fn with_max_alloc_count_per_frame(mut self, n: u32) -> Self {
        self.max_alloc_count_per_frame = n;
        self
    }

    /// Tighten the per-decoder pool size. See
    /// [`DecoderLimits::max_arenas_in_flight`].
    pub fn with_max_arenas_in_flight(mut self, n: u8) -> Self {
        self.max_arenas_in_flight = n;
        self
    }

    /// Tighten the per-packet decoded-audio duration cap. See
    /// [`DecoderLimits::max_decoded_audio_seconds_per_packet`].
    pub fn with_max_decoded_audio_seconds_per_packet(mut self, n: u32) -> Self {
        self.max_decoded_audio_seconds_per_packet = n;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_conservative_but_finite() {
        let l = DecoderLimits::default();
        // 32k x 32k pixels.
        assert_eq!(l.max_pixels_per_frame, 1_073_741_824);
        // 1 GiB per arena.
        assert_eq!(l.max_alloc_bytes_per_frame, 1u64 << 30);
        // 1M allocations per frame.
        assert_eq!(l.max_alloc_count_per_frame, 1_000_000);
        // 8 arenas in flight.
        assert_eq!(l.max_arenas_in_flight, 8);
        // 60 s of decoded audio per packet.
        assert_eq!(l.max_decoded_audio_seconds_per_packet, 60);
    }

    #[test]
    fn builder_methods_compose() {
        let l = DecoderLimits::default()
            .with_max_pixels_per_frame(1024 * 1024)
            .with_max_alloc_bytes_per_frame(8 * 1024 * 1024)
            .with_max_alloc_count_per_frame(1024)
            .with_max_arenas_in_flight(2)
            .with_max_decoded_audio_seconds_per_packet(1);
        assert_eq!(l.max_pixels_per_frame, 1024 * 1024);
        assert_eq!(l.max_alloc_bytes_per_frame, 8 * 1024 * 1024);
        assert_eq!(l.max_alloc_count_per_frame, 1024);
        assert_eq!(l.max_arenas_in_flight, 2);
        assert_eq!(l.max_decoded_audio_seconds_per_packet, 1);
    }

    #[test]
    fn copy_semantics() {
        let a = DecoderLimits::default();
        let b = a; // would not compile if not Copy
        assert_eq!(a, b);
    }
}
