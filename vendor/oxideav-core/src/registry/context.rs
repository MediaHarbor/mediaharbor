//! Unified runtime registration context.
//!
//! [`RuntimeContext`] bundles every registry the framework needs into a
//! single value that consumers pass around (codec/container/source/
//! filter). Sibling crates expose a uniform `register(&mut
//! RuntimeContext)` entry point that installs themselves into the
//! relevant sub-registry.

use super::codec::CodecRegistry;
use super::container::ContainerRegistry;
use super::filter::FilterRegistry;
use super::source::SourceRegistry;

/// Aggregate of every registry the framework consumes.
///
/// Every sibling crate that contributes implementations exposes
/// `pub fn register(ctx: &mut RuntimeContext)` to install itself.
/// Construct with [`RuntimeContext::new`] for an empty context, then
/// call each sibling's `register` to fill it in.
#[derive(Default)]
pub struct RuntimeContext {
    /// Codec registry (decoder/encoder factories, tags, probes).
    pub codecs: CodecRegistry,
    /// Container registry (demuxer/muxer factories, extensions, probes).
    pub containers: ContainerRegistry,
    /// Source registry (URL-scheme / device input drivers).
    pub sources: SourceRegistry,
    /// Stream-filter registry (frame-to-frame transforms).
    pub filters: FilterRegistry,
}

impl RuntimeContext {
    /// Empty context — no codecs, no containers, no source schemes, no
    /// filters. Sibling crates fill in the four sub-registries via
    /// their `register(&mut RuntimeContext)` entry points.
    pub fn new() -> Self {
        Self::default()
    }
}
