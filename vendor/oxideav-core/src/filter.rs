//! Multi-stream filter model.
//!
//! A [`StreamFilter`] is a node in the pipeline that consumes N input streams
//! and produces M output streams, where input and output media kinds may
//! differ. This generalises the single-kind `AudioFilter` /
//! `ImageFilter` traits and is the substrate used by
//! `oxideav-pipeline`'s filter registry.
//!
//! # Ports
//!
//! Every filter declares its [`PortSpec`]s at construction time. The pipeline
//! introspects ports to:
//!
//! - wire each input port to the correct upstream stream,
//! - synthesise [`StreamInfo`](crate::StreamInfo) entries for each output
//!   port so sinks see the forthcoming streams in their `start()` call
//!   (before the first frame arrives),
//! - size the per-port back-pressure channels.
//!
//! Output [`PortParams`] carry the concrete stream parameters (sample rate,
//! resolution, etc.) — not placeholders. A spectrogram filter that renders
//! 800×256 RGB at 30 fps declares those numbers in its `Video` port params
//! the moment it's built.
//!
//! # Back-pressure and frame emission
//!
//! `push` and `flush` take a [`FilterContext`] whose [`emit`](FilterContext::emit)
//! method is a per-port bounded-channel send. Filters call `emit` as many
//! times as they like per `push`, interleaving ports freely (`emit(0, a);
//! emit(0, a); emit(1, v); emit(0, a)` is fine). A slow consumer on one
//! port blocks only that port's `emit`, not the whole filter, so a 30 Hz
//! video output cannot stall a 48 kHz audio passthrough.
//!
//! # PTS responsibility
//!
//! Filters own their output frames' timestamps. For rate-changing or
//! kind-changing filters (e.g. an audio → video visualiser), the filter
//! is the only thing that knows how to map source pts onto output pts.
//! The pipeline does not rewrite pts on emitted frames. Downstream stages
//! expect constant-frame-rate outputs to have integer-multiple pts
//! spacing; vary from that at your peril.

use crate::{Error, Frame, MediaType, PixelFormat, Result, SampleFormat, TimeBase};

/// Description of one port (input or output) exposed by a filter.
#[derive(Clone, Debug)]
pub struct PortSpec {
    /// Port name, unique per filter-port-direction (e.g. `"audio"` /
    /// `"video"` / `"left"` / `"right"`). Used by the schema to address
    /// a specific output when the default "route by kind" isn't enough.
    pub name: String,
    /// Media type this port carries.
    pub kind: MediaType,
    /// Concrete stream parameters. For output ports these are
    /// authoritative; for input ports they describe what the filter
    /// *expects* and are used as a shape hint at wiring time.
    pub params: PortParams,
}

impl PortSpec {
    /// Convenience: audio port with the given params.
    pub fn audio(
        name: impl Into<String>,
        sample_rate: u32,
        channels: u16,
        format: SampleFormat,
    ) -> Self {
        Self {
            name: name.into(),
            kind: MediaType::Audio,
            params: PortParams::Audio {
                sample_rate,
                channels,
                format,
            },
        }
    }

    /// Convenience: video port with the given params.
    pub fn video(
        name: impl Into<String>,
        width: u32,
        height: u32,
        format: PixelFormat,
        time_base: TimeBase,
    ) -> Self {
        Self {
            name: name.into(),
            kind: MediaType::Video,
            params: PortParams::Video {
                width,
                height,
                format,
                time_base,
            },
        }
    }
}

/// Concrete stream parameters for a port.
///
/// Subtitle/Metadata are placeholders — they exist so the pipeline
/// can route those kinds once filters that emit them land, but no
/// current filter consumes them.
#[derive(Clone, Debug)]
pub enum PortParams {
    /// An audio port.
    Audio {
        /// Samples per second per channel.
        sample_rate: u32,
        /// Channel count.
        channels: u16,
        /// Sample format flowing through the port.
        format: SampleFormat,
    },
    /// A video port.
    Video {
        /// Picture width in pixels.
        width: u32,
        /// Picture height in pixels.
        height: u32,
        /// Pixel format flowing through the port.
        format: PixelFormat,
        /// Time base the port's frame timestamps are expressed in.
        time_base: TimeBase,
    },
    /// A subtitle-cue port (placeholder — no current filter emits it).
    Subtitle,
    /// A metadata/data port (placeholder — no current filter emits it).
    Metadata,
}

impl PortParams {
    /// Media type implied by the variant.
    pub fn kind(&self) -> MediaType {
        match self {
            PortParams::Audio { .. } => MediaType::Audio,
            PortParams::Video { .. } => MediaType::Video,
            PortParams::Subtitle => MediaType::Subtitle,
            PortParams::Metadata => MediaType::Data,
        }
    }
}

/// Runtime plumbing handed to [`StreamFilter::push`] / [`StreamFilter::flush`].
///
/// The executor implements this to route emitted frames to the correct
/// downstream stage. `emit` is a bounded-channel send and may block if a
/// consumer is slow, which provides natural per-port back-pressure.
pub trait FilterContext {
    /// Emit a frame on the named output port. Blocks if the downstream
    /// channel is full.
    fn emit(&mut self, output_port: usize, frame: Frame) -> Result<()>;
}

/// Multi-stream filter.
///
/// See the [module docs](self) for the overall model.
pub trait StreamFilter: Send {
    /// Input ports, ordered by port id. The returned slice is lifetime-
    /// tied to `self`; implementors typically hold the spec as an
    /// owned field and return a borrow.
    fn input_ports(&self) -> &[PortSpec];

    /// Output ports, ordered by port id.
    fn output_ports(&self) -> &[PortSpec];

    /// Process one input frame on `port`. The filter may call
    /// [`FilterContext::emit`] zero or more times on any of its
    /// output ports before returning.
    fn push(&mut self, ctx: &mut dyn FilterContext, port: usize, frame: &Frame) -> Result<()>;

    /// Drain any internally buffered state at end-of-stream. Filters that
    /// hold rolling windows (spectrogram) or temporal buffers (resample)
    /// emit their remaining output here.
    fn flush(&mut self, _ctx: &mut dyn FilterContext) -> Result<()> {
        Ok(())
    }

    /// Reset internal state on a flow barrier (seek). Drops any buffered
    /// frames silently — unlike `flush`, no frames are emitted, because
    /// the upstream pipeline is going to start delivering frames from a
    /// new wall-clock position. Filters with rolling windows
    /// (spectrogram) or temporal smoothing should restart from empty so
    /// the user sees a clean cut over the seek.
    ///
    /// Default no-op so stateless / freshly-restartable filters need no
    /// boilerplate.
    fn reset(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Helper used by pipeline registries when a named filter isn't known.
pub fn unknown_filter_error(name: &str) -> Error {
    Error::unsupported(format!("unknown filter '{name}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioFrame, VideoFrame, VideoPlane};

    /// A trivial filter that doubles audio samples on port 0 (pass-through
    /// with gain) and emits nothing on port 1 (video). Exists only to
    /// prove the trait shape compiles and `FilterContext::emit` wires.
    struct Fake {
        inp: Vec<PortSpec>,
        outp: Vec<PortSpec>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                inp: vec![PortSpec::audio("in", 48_000, 2, SampleFormat::S16)],
                outp: vec![
                    PortSpec::audio("audio", 48_000, 2, SampleFormat::S16),
                    PortSpec::video("video", 16, 8, PixelFormat::Rgb24, TimeBase::new(1, 30)),
                ],
            }
        }
    }

    impl StreamFilter for Fake {
        fn input_ports(&self) -> &[PortSpec] {
            &self.inp
        }
        fn output_ports(&self) -> &[PortSpec] {
            &self.outp
        }
        fn push(&mut self, ctx: &mut dyn FilterContext, port: usize, frame: &Frame) -> Result<()> {
            assert_eq!(port, 0);
            if let Frame::Audio(a) = frame {
                ctx.emit(0, Frame::Audio(a.clone()))?;
            }
            Ok(())
        }
    }

    struct CollectCtx {
        out: Vec<(usize, Frame)>,
    }
    impl FilterContext for CollectCtx {
        fn emit(&mut self, port: usize, frame: Frame) -> Result<()> {
            self.out.push((port, frame));
            Ok(())
        }
    }

    #[test]
    fn trait_compiles_and_ports_round_trip() {
        let mut f = Fake::new();
        assert_eq!(f.input_ports().len(), 1);
        assert_eq!(f.output_ports().len(), 2);
        assert_eq!(f.output_ports()[0].kind, MediaType::Audio);
        assert_eq!(f.output_ports()[1].kind, MediaType::Video);

        let audio = AudioFrame {
            samples: 0,
            pts: None,
            data: vec![vec![]; 2],
        };
        let mut ctx = CollectCtx { out: Vec::new() };
        f.push(&mut ctx, 0, &Frame::Audio(audio)).unwrap();
        assert_eq!(ctx.out.len(), 1);
        matches!(&ctx.out[0].1, Frame::Video(_));
    }

    #[test]
    fn video_frame_shape_is_unchanged() {
        // Guard: this trait module does not change the VideoFrame /
        // AudioFrame types. Anything consuming `Frame` via the trait
        // treats them as opaque carriers. Stream-level properties
        // (format/dimensions/time_base) live on the stream's
        // `CodecParameters`, not the frame.
        let vf = VideoFrame {
            pts: None,
            planes: vec![VideoPlane {
                stride: 12,
                data: vec![0u8; 24],
            }],
        };
        assert_eq!(vf.planes.len(), 1);
    }
}
