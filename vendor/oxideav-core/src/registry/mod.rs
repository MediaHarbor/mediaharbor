//! Framework registries.
//!
//! Codec / container / source / filter implementations register
//! themselves into one of the per-kind registries here. Most consumers
//! interact with the bundle via [`RuntimeContext`].

pub mod codec;
pub mod container;
pub mod context;
pub mod filter;
pub mod slice;
pub mod source;

pub use codec::{
    CodecImplementation, CodecInfo, CodecRegistry, Decoder, DecoderFactory, Encoder, EncoderFactory,
};
pub use container::{
    ContainerProbeFn, ContainerRegistry, Demuxer, Muxer, OpenDemuxerFn, OpenMuxerFn, ProbeData,
    ProbeScore, ReadSeek, WriteSeek, MAX_PROBE_SCORE, PROBE_SCORE_EXTENSION,
};
pub use context::RuntimeContext;
pub use filter::{FilterFactory, FilterRegistry};
pub use source::{
    BytesSource, FrameSource, MultiTitleSource, OpenBytesFn, OpenFramesFn, OpenMultiTitleFn,
    OpenPacketsFn, PacketSource, SourceOutput, SourceRegistry,
};
