//! In-process codec registry.
//!
//! Every codec crate declares itself with one [`CodecInfo`] value —
//! capabilities, factory functions, the container tags it claims, and
//! (optionally) a probe function used to disambiguate genuine tag
//! collisions. The registry stores those registrations and exposes
//! three orthogonal lookups:
//!
//! - **id-keyed** — `make_decoder(params)` / `make_encoder(params)` walk
//!   the implementations registered under `params.codec_id`, filter by
//!   capability restrictions, and try them in priority order with init-
//!   time fallback.
//! - **tag-keyed** — `resolve_tag(&ProbeContext)` walks every
//!   registration whose `tags` contains `ctx.tag`, calls each probe
//!   (treating `None` as "returns 1.0"), and returns the id with the
//!   highest resulting confidence. First-registered wins on ties.
//! - **payload-magic-keyed** — `resolve_payload_magic(first_bytes)`
//!   prefix-matches the claimed payload magic prefixes against a
//!   stream's leading bytes; longest matching magic wins, then
//!   registration order. For containers that identify a codec by the
//!   payload itself rather than a tag (e.g. an Ogg logical stream's
//!   first packet, or raw elementary streams).
//! - **diagnostic** — `all_implementations`, `all_tag_registrations`,
//!   `all_payload_magic_registrations`.
//!
//! The tag path explicitly DOES NOT short-circuit on "first claim with
//! no probe" — every claimant is asked, so a lower-priority probed
//! claim can out-rank a higher-priority unprobed one when the content
//! is actually ambiguous (DIV3 XVID-with-real-MSMPEG4 payload etc.).

use std::collections::HashMap;

use crate::arena;
use crate::{
    CodecCapabilities, CodecId, CodecOptionsStruct, CodecParameters, CodecResolver, CodecTag,
    Error, ExecutionContext, Frame, OptionField, Packet, PixelFormat, ProbeContext, ProbeFn,
    Result,
};

// ───────────────────────── codec traits ─────────────────────────

/// A packet-to-frame decoder.
pub trait Decoder: Send {
    /// Identifier of the codec this decoder handles.
    fn codec_id(&self) -> &CodecId;

    /// Feed one compressed packet. May or may not produce a frame immediately —
    /// call `receive_frame` in a loop afterwards.
    fn send_packet(&mut self, packet: &Packet) -> Result<()>;

    /// Pull the next decoded frame, if any. Returns `Error::NeedMore` when the
    /// decoder needs another packet.
    fn receive_frame(&mut self) -> Result<Frame>;

    /// Pull the next decoded frame as an arena-backed [`arena::sync::Frame`].
    ///
    /// Decoders that build their output through an
    /// [`arena::sync::ArenaPool`] override this to return the pooled
    /// [`arena::sync::Frame`] **directly**, with no per-plane memcpy
    /// out — the caller gets true zero-copy plane access via
    /// [`arena::sync::FrameInner::plane`].
    ///
    /// The default implementation delegates to [`Self::receive_frame`]
    /// and copies the video planes into a freshly-leased one-shot
    /// `arena::sync::ArenaPool`. This makes the method an additive
    /// change for every existing [`Decoder`] impl: callers using the
    /// new API still work, but pay one memcpy per plane.
    ///
    /// **Audio / subtitle frames:** the [`arena::sync::Frame`] body is
    /// video-only (planes + [`arena::sync::FrameHeader`] with
    /// width/height/pixel format). The default implementation returns
    /// [`Error::Unsupported`] for non-video frames; an audio decoder
    /// that wants to expose `receive_arena_frame()` must override it
    /// with its own arena-backed audio-frame type once the framework
    /// gains one. Until then, audio decoders should keep using
    /// [`Self::receive_frame`].
    fn receive_arena_frame(&mut self) -> Result<arena::sync::Frame> {
        let frame = self.receive_frame()?;
        match frame {
            Frame::Video(v) => video_frame_to_arena_sync_frame(&v),
            Frame::Audio(_) => Err(Error::unsupported(
                "receive_arena_frame: audio frames not yet supported by default impl",
            )),
            Frame::Subtitle(_) => Err(Error::unsupported(
                "receive_arena_frame: subtitle frames have no arena-backed representation",
            )),
            Frame::Vector(_) => Err(Error::unsupported(
                "receive_arena_frame: vector frames have no arena-backed representation",
            )),
        }
    }

    /// Signal end-of-stream. After this, `receive_frame` will drain buffered
    /// frames and eventually return `Error::Eof`.
    fn flush(&mut self) -> Result<()>;

    /// Discard all carry-over state so the decoder can resume from a new
    /// bitstream position without producing stale output. Called by the
    /// player after a container seek.
    ///
    /// Unlike [`flush`](Self::flush) (which signals end-of-stream and
    /// drains buffered frames), `reset` is expected to:
    /// * drop every buffered input packet and pending output frame;
    /// * zero any per-stream filter / predictor / overlap memory so the
    ///   next `send_packet` decodes as if it were the first;
    /// * leave the codec id and stream parameters untouched.
    ///
    /// The default is a conservative "drain-then-forget": call
    /// [`flush`](Self::flush) and ignore any remaining frames. Stateful
    /// codecs (LPC predictors, backward-adaptive gain, IMDCT overlap,
    /// reference pictures, …) should override this to wipe their
    /// internal state explicitly — otherwise the first ~N output
    /// samples after a seek will be glitchy until the state re-adapts.
    fn reset(&mut self) -> Result<()> {
        self.flush()?;
        // Drain any remaining output frames so the next send_packet
        // starts clean. NeedMore / Eof both mean "no more frames"; any
        // other error is surfaced so the caller can see why.
        loop {
            match self.receive_frame() {
                Ok(_) => {}
                Err(Error::NeedMore) | Err(Error::Eof) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    /// Advisory: announce the runtime environment (today: a thread budget
    /// for codec-internal parallelism). Called at most once, before the
    /// first `send_packet`. Default no-op; codecs that want to run
    /// slice-/GOP-/tile-parallel override this to capture the budget.
    /// Ignoring the hint is always safe — callers must still work with
    /// a decoder that runs serial.
    fn set_execution_context(&mut self, _ctx: &ExecutionContext) {}
}

/// A frame-to-packet encoder.
pub trait Encoder: Send {
    /// Identifier of the codec this encoder produces.
    fn codec_id(&self) -> &CodecId;

    /// Parameters describing this encoder's output stream (to feed into a muxer).
    fn output_params(&self) -> &CodecParameters;

    /// Feed one uncompressed frame. May or may not produce a packet
    /// immediately — call `receive_packet` in a loop afterwards.
    fn send_frame(&mut self, frame: &Frame) -> Result<()>;

    /// Pull the next encoded packet, if any. Returns `Error::NeedMore`
    /// when the encoder needs another frame (or a `flush`).
    fn receive_packet(&mut self) -> Result<Packet>;

    /// Signal end of input: drain internal lookahead so the remaining
    /// packets become available via `receive_packet`.
    fn flush(&mut self) -> Result<()>;

    /// Advisory: announce the runtime environment. Same semantics as
    /// [`Decoder::set_execution_context`].
    fn set_execution_context(&mut self, _ctx: &ExecutionContext) {}
}

/// Default-impl helper for [`Decoder::receive_arena_frame`]: copy a
/// heap-backed [`crate::VideoFrame`] into a freshly-leased
/// [`arena::sync::Frame`].
///
/// Allocates a single-slot, single-arena `arena::sync::ArenaPool`
/// sized to fit the planes verbatim. The pool is dropped at the end of
/// this call; the returned `Frame` keeps its leased buffer alive via
/// `Arc<FrameInner>` (the `Arena`'s `Weak` handle to the dropped pool
/// just stops upgrading — the buffer drops normally when the last
/// `Frame` clone goes away).
///
/// Width / height / pixel-format on the returned `FrameHeader` are
/// derived from the plane shape: `width = plane[0].stride`,
/// `height = plane[0].data.len() / stride`. Pixel format is left as
/// [`PixelFormat::Yuv420P`] when there are 3 planes, else the first
/// per-plane sensible default — this is a best-effort label for the
/// generic conversion path; decoders that override
/// `receive_arena_frame` themselves should set the correct pixel
/// format.
fn video_frame_to_arena_sync_frame(v: &crate::VideoFrame) -> Result<arena::sync::Frame> {
    if v.planes.is_empty() {
        return Err(Error::invalid(
            "receive_arena_frame: video frame has no planes",
        ));
    }
    let total_bytes: usize = v.planes.iter().map(|p| p.data.len()).sum();
    if total_bytes == 0 {
        return Err(Error::invalid(
            "receive_arena_frame: video frame planes are empty",
        ));
    }
    // One-shot pool sized exactly to the frame. The pool drops at end
    // of scope; the leased Arena lives on inside the returned Frame
    // (its Weak<ArenaPool> handle just won't upgrade in Drop, so the
    // Box<[u8]> falls through to a normal heap free).
    let pool = arena::sync::ArenaPool::with_alloc_count_cap(
        1,
        total_bytes,
        // One alloc per plane, plus a generous safety margin.
        (v.planes.len() as u32).saturating_add(4),
    );
    let arena = pool.lease()?;
    let mut plane_offsets: Vec<(usize, usize)> = Vec::with_capacity(v.planes.len());
    let mut cursor = 0usize;
    for plane in &v.planes {
        let dst = arena.alloc::<u8>(plane.data.len())?;
        dst.copy_from_slice(&plane.data);
        plane_offsets.push((cursor, plane.data.len()));
        cursor += plane.data.len();
    }
    // Best-effort header: width = stride of plane 0, height inferred
    // from plane 0's data length. Pixel format defaults to Yuv420P for
    // the common 3-plane case, Gray8 for single-plane, otherwise
    // Yuv444P. Decoders that care about exact pixel-format / width /
    // height should override `receive_arena_frame` themselves so they
    // can emit a correct `FrameHeader` straight from their arena
    // build path.
    let stride0 = v.planes[0].stride.max(1);
    let width = stride0 as u32;
    let height = (v.planes[0].data.len() / stride0) as u32;
    // Count only image planes for the format guess — side-channel
    // entries (palette, significant bits; copied verbatim above like
    // any other plane) must not bump e.g. a single-plane palette frame
    // out of the Gray8 label.
    let pixel_format = match v.image_plane_count() {
        1 => PixelFormat::Gray8,
        3 => PixelFormat::Yuv420P,
        _ => PixelFormat::Yuv444P,
    };
    let header = arena::sync::FrameHeader::new(width, height, pixel_format, v.pts);
    arena::sync::FrameInner::new(arena, &plane_offsets, header)
}

/// Factory that builds a decoder for a given codec parameter set.
pub type DecoderFactory = fn(params: &CodecParameters) -> Result<Box<dyn Decoder>>;

/// Factory that builds an encoder for a given codec parameter set.
pub type EncoderFactory = fn(params: &CodecParameters) -> Result<Box<dyn Encoder>>;

// ───────────────────────── CodecInfo ─────────────────────────

/// A single registration: capabilities, decoder/encoder factories,
/// optional probe, and the container tags this codec claims.
///
/// Codec crates build one of these per codec id inside their
/// `register(reg)` function and hand it to
/// [`CodecRegistry::register`]. The struct is `#[non_exhaustive]` so
/// additional fields can be added without breaking existing codec
/// crates — construction is only possible through
/// [`CodecInfo::new`] plus the builder methods below.
#[non_exhaustive]
pub struct CodecInfo {
    /// Canonical codec identifier this entry registers.
    pub id: CodecId,
    /// Capability description (media kind, feature flags, priority).
    pub capabilities: CodecCapabilities,
    /// Factory producing a fresh decoder instance, if decode is supported.
    pub decoder_factory: Option<DecoderFactory>,
    /// Factory producing a fresh encoder instance, if encode is supported.
    pub encoder_factory: Option<EncoderFactory>,
    /// Probe function that returns a confidence in `0.0..=1.0` for a
    /// given [`ProbeContext`]. `None` means "confidence 1.0 for every
    /// claimed tag" — the correct default for codecs whose tag claims
    /// are unambiguous.
    pub probe: Option<ProbeFn>,
    /// Tags this codec is willing to be looked up under. One codec may
    /// claim many tags (an AAC decoder covers several WaveFormat ids,
    /// a FourCC, an MP4 OTI, and a Matroska CodecID string at once).
    pub tags: Vec<CodecTag>,
    /// Payload magic prefixes this codec answers to (`\x01vorbis`,
    /// `OpusHead`, …). Some carriage formats have no codec tag — the
    /// codec is announced by a magic byte prefix on the payload itself
    /// (an Ogg logical stream's first packet is the canonical case;
    /// raw elementary streams are another). Such claims are
    /// prefix-matched by
    /// [`CodecRegistry::resolve_payload_magic_ref`] instead of living in
    /// the exact-match [`CodecTag`] index. Attached with
    /// [`Self::payload_magic`] / [`Self::payload_magics`]. Empty prefixes are
    /// ignored at registration time (a zero-length prefix would match
    /// every stream while carrying no evidence).
    pub payload_magics: Vec<Vec<u8>>,
    /// Schema of the encoder's recognised option keys
    /// (`CodecParameters::options`). Attached with
    /// [`Self::encoder_options`]. Used for validation / `oxideav list`
    /// / pipeline JSON checks.
    pub encoder_options_schema: Option<&'static [OptionField]>,
    /// Schema of the decoder's recognised option keys.
    pub decoder_options_schema: Option<&'static [OptionField]>,
    /// HW backend identifier, e.g. `"nvidia"`, `"vaapi"`, `"vdpau"`,
    /// `"vulkan-video"`, `"videotoolbox"`. Set by HW siblings on every
    /// `CodecInfo` they register; SW codecs leave this `None`.
    /// Consumers (e.g. the CLI's `info` command) use it to group
    /// codec entries by backend and to dedupe probe calls — multiple
    /// `CodecInfo` entries with the same `engine_id` typically share
    /// an `engine_probe` function, and consumers should call the probe
    /// at most once per `engine_id` per pass. Attached via
    /// [`Self::with_engine_id`].
    pub engine_id: Option<&'static str>,
    /// Optional engine probe function. When `Some`, calling it returns
    /// one [`crate::engine::HwDeviceInfo`] entry per device the backend
    /// sees. Phase-2 HW siblings populate this on every `CodecInfo`
    /// they register; Phase-3 consumers (CLI) call it on demand.
    /// Attached via [`Self::with_engine_probe`].
    pub engine_probe: Option<crate::engine::EngineProbeFn>,
}

impl CodecInfo {
    /// Start a new registration for `id` with empty capabilities, no
    /// factories, no probe, and no tags. Chain the builder methods
    /// below to fill it in, then hand the result to
    /// [`CodecRegistry::register`].
    pub fn new(id: CodecId) -> Self {
        Self {
            capabilities: CodecCapabilities::audio(id.as_str()),
            id,
            decoder_factory: None,
            encoder_factory: None,
            probe: None,
            tags: Vec::new(),
            payload_magics: Vec::new(),
            encoder_options_schema: None,
            decoder_options_schema: None,
            engine_id: None,
            engine_probe: None,
        }
    }

    /// Replace the capability description. The default built by
    /// [`Self::new`] is a placeholder (audio-flavoured, no flags); every
    /// real registration should call this.
    pub fn capabilities(mut self, caps: CodecCapabilities) -> Self {
        self.capabilities = caps;
        self
    }

    /// Builder: attach the decoder factory.
    pub fn decoder(mut self, factory: DecoderFactory) -> Self {
        self.decoder_factory = Some(factory);
        self
    }

    /// Builder: attach the encoder factory.
    pub fn encoder(mut self, factory: EncoderFactory) -> Self {
        self.encoder_factory = Some(factory);
        self
    }

    /// Builder: attach a confidence probe (see [`CodecInfo::probe`]).
    pub fn probe(mut self, probe: ProbeFn) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Claim a single container tag for this codec. Equivalent to
    /// `.tags([tag])` but avoids the array ceremony for single-tag
    /// claims.
    pub fn tag(mut self, tag: CodecTag) -> Self {
        self.tags.push(tag);
        self
    }

    /// Claim a set of container tags for this codec. Takes any
    /// iterable (arrays, `Vec`, `Option`, …) so the common case of a
    /// codec with 3-6 tags reads as one clean block.
    pub fn tags(mut self, tags: impl IntoIterator<Item = CodecTag>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// Claim one payload magic prefix for this codec (see
    /// [`Self::payload_magics`]). Chain repeatedly for codecs that answer
    /// to more than one magic:
    ///
    /// ```
    /// # use oxideav_core::registry::CodecInfo;
    /// # use oxideav_core::CodecId;
    /// let info = CodecInfo::new(CodecId::new("vorbis")).payload_magic(b"\x01vorbis");
    /// # let _ = info;
    /// ```
    pub fn payload_magic(mut self, magic: impl Into<Vec<u8>>) -> Self {
        self.payload_magics.push(magic.into());
        self
    }

    /// Claim a set of payload magic prefixes for this codec — the
    /// iterable companion to [`Self::payload_magic`], mirroring the
    /// [`Self::tag`] / [`Self::tags`] pair.
    pub fn payload_magics<I>(mut self, magics: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Vec<u8>>,
    {
        self.payload_magics
            .extend(magics.into_iter().map(Into::into));
        self
    }

    /// Declare the options struct this codec's encoder factory expects.
    /// Attaches `T::SCHEMA` so the registry can enumerate recognised
    /// option keys (for `oxideav list`, pipeline JSON validation, etc.).
    /// The factory itself still has to call
    /// [`crate::parse_options::<T>()`] against
    /// `CodecParameters::options` at init time.
    pub fn encoder_options<T: CodecOptionsStruct>(mut self) -> Self {
        self.encoder_options_schema = Some(T::SCHEMA);
        self
    }

    /// Declare the options struct this codec's decoder factory expects.
    /// See [`Self::encoder_options`] for the encoder counterpart.
    pub fn decoder_options<T: CodecOptionsStruct>(mut self) -> Self {
        self.decoder_options_schema = Some(T::SCHEMA);
        self
    }

    /// Tag this codec as belonging to a HW backend identified by
    /// `engine_id`. Should match the `engine_id` of every other
    /// `CodecInfo` registered by the same backend, and the corresponding
    /// `engine_id` field used by the CLI for grouping. SW codecs leave
    /// this unset.
    pub fn with_engine_id(mut self, engine_id: &'static str) -> Self {
        self.engine_id = Some(engine_id);
        self
    }

    /// Attach a probe function. Consumers call it to enumerate the
    /// engines (devices) this backend can dispatch to. Probes are
    /// expected to be idempotent and side-effect free; consumers may
    /// call them more than once per process and should dedupe by
    /// [`Self::engine_id`].
    pub fn with_engine_probe(mut self, probe: crate::engine::EngineProbeFn) -> Self {
        self.engine_probe = Some(probe);
        self
    }
}

/// Internal per-impl record held inside the registry's id map. Kept
/// distinct from [`CodecInfo`] so the id map stays cheap to walk
/// during `make_decoder` / `make_encoder` lookups.
#[derive(Clone)]
pub struct CodecImplementation {
    /// Capability description copied from the originating [`CodecInfo`].
    pub caps: CodecCapabilities,
    /// Decoder factory, if this implementation can decode.
    pub make_decoder: Option<DecoderFactory>,
    /// Encoder factory, if this implementation can encode.
    pub make_encoder: Option<EncoderFactory>,
    /// Encoder options schema declared via
    /// [`CodecInfo::encoder_options`]. `None` means the encoder accepts
    /// no tuning knobs (any non-empty `CodecParameters::options` will
    /// still be rejected by the factory if the encoder calls
    /// `parse_options` — this is purely informational for discovery).
    pub encoder_options_schema: Option<&'static [OptionField]>,
    /// Decoder options schema declared via
    /// [`CodecInfo::decoder_options`]; same semantics as the encoder
    /// schema above.
    pub decoder_options_schema: Option<&'static [OptionField]>,
    /// HW backend identifier copied verbatim from the originating
    /// [`CodecInfo::engine_id`]. `Some("nvidia"/"vaapi"/...)` on HW
    /// backends; `None` on SW codecs. Consumers (CLI `info` command,
    /// pipeline dispatcher, bench loop) read this to group entries by
    /// backend without grepping `caps.implementation`.
    pub engine_id: Option<&'static str>,
    /// Engine probe function copied verbatim from the originating
    /// [`CodecInfo::engine_probe`]. `Some(fn)` on HW backends with a
    /// probe wired; `None` on SW codecs. Consumers call it on demand
    /// to enumerate per-device info ([`crate::engine::HwDeviceInfo`]).
    pub engine_probe: Option<crate::engine::EngineProbeFn>,
}

/// Registry mapping codec ids and container tags to their registered
/// implementations; the lookup point behind `make_decoder` /
/// `make_encoder` / `resolve_tag`.
#[derive(Default)]
pub struct CodecRegistry {
    /// id → list of implementations. Each registered codec appends one
    /// entry here. `make_decoder` / `make_encoder` walk this list in
    /// preference order.
    impls: HashMap<CodecId, Vec<CodecImplementation>>,
    /// Append-only list of every registration — the `tag_index` stores
    /// offsets into this vector.
    registrations: Vec<RegistrationRecord>,
    /// Tag → indices into `registrations`. Indices are stored in
    /// registration order so tie-breaking in `resolve_tag` is
    /// deterministic (first-registered wins).
    tag_index: HashMap<CodecTag, Vec<usize>>,
    /// Payload magic-prefix claims: `(magic, registration index)` in
    /// registration order. Kept as a flat list rather than a map
    /// because resolution is prefix matching (see
    /// [`Self::resolve_payload_magic_ref`]), not exact-key lookup.
    magic_index: Vec<(Vec<u8>, usize)>,
}

/// Internal registry record. Mirrors the subset of [`CodecInfo`]
/// needed at resolve time.
struct RegistrationRecord {
    id: CodecId,
    probe: Option<ProbeFn>,
}

impl CodecRegistry {
    /// An empty registry (same as `Default`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one codec. Expands into:
    ///   * an entry in the id → implementations map (for
    ///     `make_decoder` / `make_encoder`);
    ///   * an entry in the tag index for every claimed tag (for
    ///     `resolve_tag`).
    ///
    /// Calling `register` multiple times with the same id is allowed
    /// and how multi-implementation codecs (software-plus-hardware
    /// FLAC, for example) are expressed.
    pub fn register(&mut self, info: CodecInfo) {
        let CodecInfo {
            id,
            capabilities,
            decoder_factory,
            encoder_factory,
            probe,
            tags,
            payload_magics,
            encoder_options_schema,
            decoder_options_schema,
            // engine_id / engine_probe are metadata attached to a
            // CodecInfo for backends that want consumers (CLI `info`,
            // pipeline bench) to enumerate the underlying devices on
            // demand. They're surfaced verbatim on the resulting
            // CodecImplementation so consumers can read them without
            // grepping `caps.implementation`. Tag-only CodecInfo entries
            // (no factories) drop the values on the floor — there's no
            // CodecImplementation built in that branch.
            engine_id,
            engine_probe,
        } = info;

        let caps = {
            let mut c = capabilities;
            if decoder_factory.is_some() {
                c = c.with_decode();
            }
            if encoder_factory.is_some() {
                c = c.with_encode();
            }
            c
        };

        // Only record an implementation entry when at least one factory
        // is present. A "tag-only" CodecInfo — used to attach extra tag
        // claims to a codec that was already registered with factories —
        // shouldn't pollute the impl list.
        if decoder_factory.is_some() || encoder_factory.is_some() {
            self.impls
                .entry(id.clone())
                .or_default()
                .push(CodecImplementation {
                    caps,
                    make_decoder: decoder_factory,
                    make_encoder: encoder_factory,
                    encoder_options_schema,
                    decoder_options_schema,
                    engine_id,
                    engine_probe,
                });
        }

        let record_idx = self.registrations.len();
        self.registrations.push(RegistrationRecord {
            id: id.clone(),
            probe,
        });
        for tag in tags {
            self.tag_index.entry(tag).or_default().push(record_idx);
        }
        for magic in payload_magics {
            // A zero-length prefix would match every stream while
            // carrying no evidence — drop it here so resolution never
            // has to special-case it.
            if !magic.is_empty() {
                self.magic_index.push((magic, record_idx));
            }
        }
    }

    /// Whether at least one registered implementation of `id` can decode.
    pub fn has_decoder(&self, id: &CodecId) -> bool {
        self.impls
            .get(id)
            .map(|v| v.iter().any(|i| i.make_decoder.is_some()))
            .unwrap_or(false)
    }

    /// Whether at least one registered implementation of `id` can encode.
    pub fn has_encoder(&self, id: &CodecId) -> bool {
        self.impls
            .get(id)
            .map(|v| v.iter().any(|i| i.make_encoder.is_some()))
            .unwrap_or(false)
    }

    /// First registered decoder factory for `params.codec_id`, invoked
    /// with `params`. No priority walk, no preference filter, no
    /// init-time fallback to a lower-priority impl. Errors if no
    /// decoder is registered for the codec.
    ///
    /// Intended for single-impl scenarios — typically a codec crate's
    /// own self-tests, where exactly one impl has been registered into
    /// a freshly-constructed registry. Production callers selecting
    /// among multiple candidates (e.g. h264_sw vs h264_videotoolbox)
    /// should use `oxideav_pipeline::make_decoder_with` instead, which
    /// applies `CodecPreferences` and walks priorities.
    pub fn first_decoder(&self, params: &CodecParameters) -> Result<Box<dyn Decoder>> {
        let imp = self
            .implementations(&params.codec_id)
            .iter()
            .find(|i| i.make_decoder.is_some())
            .ok_or_else(|| {
                Error::CodecNotFound(format!("no decoder for codec {}", params.codec_id))
            })?;
        (imp.make_decoder.expect("checked above"))(params)
    }

    /// First registered encoder factory — see [`first_decoder`].
    ///
    /// [`first_decoder`]: Self::first_decoder
    pub fn first_encoder(&self, params: &CodecParameters) -> Result<Box<dyn Encoder>> {
        let imp = self
            .implementations(&params.codec_id)
            .iter()
            .find(|i| i.make_encoder.is_some())
            .ok_or_else(|| {
                Error::CodecNotFound(format!("no encoder for codec {}", params.codec_id))
            })?;
        (imp.make_encoder.expect("checked above"))(params)
    }

    /// Look up a decoder by exact implementation name
    /// (`"h264_sw"`, `"aac_audiotoolbox"`, ...). Errors if the impl
    /// isn't registered or if it has no decoder factory.
    pub fn decoder_by_impl(
        &self,
        impl_name: &str,
        params: &CodecParameters,
    ) -> Result<Box<dyn Decoder>> {
        let imp = self
            .implementations(&params.codec_id)
            .iter()
            .find(|i| i.caps.implementation == impl_name)
            .ok_or_else(|| {
                Error::CodecNotFound(format!(
                    "no implementation `{impl_name}` for codec {}",
                    params.codec_id
                ))
            })?;
        let factory = imp
            .make_decoder
            .ok_or_else(|| Error::CodecNotFound(format!("`{impl_name}` is encoder-only")))?;
        factory(params)
    }

    /// Look up an encoder by exact implementation name — see
    /// [`decoder_by_impl`].
    ///
    /// [`decoder_by_impl`]: Self::decoder_by_impl
    pub fn encoder_by_impl(
        &self,
        impl_name: &str,
        params: &CodecParameters,
    ) -> Result<Box<dyn Encoder>> {
        let imp = self
            .implementations(&params.codec_id)
            .iter()
            .find(|i| i.caps.implementation == impl_name)
            .ok_or_else(|| {
                Error::CodecNotFound(format!(
                    "no implementation `{impl_name}` for codec {}",
                    params.codec_id
                ))
            })?;
        let factory = imp
            .make_encoder
            .ok_or_else(|| Error::CodecNotFound(format!("`{impl_name}` is decoder-only")))?;
        factory(params)
    }

    /// Iterate codec ids that have at least one decoder implementation.
    pub fn decoder_ids(&self) -> impl Iterator<Item = &CodecId> {
        self.impls
            .iter()
            .filter(|(_, v)| v.iter().any(|i| i.make_decoder.is_some()))
            .map(|(id, _)| id)
    }

    /// Iterate codec ids that have at least one encoder implementation.
    pub fn encoder_ids(&self) -> impl Iterator<Item = &CodecId> {
        self.impls
            .iter()
            .filter(|(_, v)| v.iter().any(|i| i.make_encoder.is_some()))
            .map(|(id, _)| id)
    }

    /// All registered implementations of a given codec id.
    pub fn implementations(&self, id: &CodecId) -> &[CodecImplementation] {
        self.impls.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Lookup the encoder options schema for a registered codec. Walks
    /// implementations in registration order and returns the first
    /// schema found. `None` means either the codec isn't registered or
    /// no implementation declared an encoder schema.
    pub fn encoder_options_schema(&self, id: &CodecId) -> Option<&'static [OptionField]> {
        self.impls
            .get(id)?
            .iter()
            .find_map(|i| i.encoder_options_schema)
    }

    /// Lookup the decoder options schema — see
    /// [`encoder_options_schema`](Self::encoder_options_schema).
    pub fn decoder_options_schema(&self, id: &CodecId) -> Option<&'static [OptionField]> {
        self.impls
            .get(id)?
            .iter()
            .find_map(|i| i.decoder_options_schema)
    }

    /// Iterator over every (codec_id, impl) pair — useful for `oxideav list`
    /// to show capability flags per implementation.
    pub fn all_implementations(&self) -> impl Iterator<Item = (&CodecId, &CodecImplementation)> {
        self.impls
            .iter()
            .flat_map(|(id, v)| v.iter().map(move |i| (id, i)))
    }

    /// Iterator over every `(tag, codec_id)` pair currently registered —
    /// used by `oxideav tags` debug output and by tests that want to
    /// walk the tag surface.
    pub fn all_tag_registrations(&self) -> impl Iterator<Item = (&CodecTag, &CodecId)> {
        self.tag_index.iter().flat_map(move |(tag, idxs)| {
            idxs.iter().map(move |&i| (tag, &self.registrations[i].id))
        })
    }

    /// Inherent form of tag resolution that returns a reference.
    /// The owned-value form used by container code lives behind the
    /// [`CodecResolver`] trait impl below.
    ///
    /// Walks every registration that claimed `ctx.tag`, calls its
    /// probe with `ctx`, and returns the id of the registration that
    /// scored highest. Probes that return `0.0` are discarded; ties
    /// on confidence are broken by registration order (first wins).
    /// Registrations with no probe are treated as returning `1.0`.
    pub fn resolve_tag_ref(&self, ctx: &ProbeContext) -> Option<&CodecId> {
        let idxs = self.tag_index.get(ctx.tag)?;
        let mut best: Option<(f32, usize)> = None;
        for &i in idxs {
            let rec = &self.registrations[i];
            let conf = match rec.probe {
                Some(f) => f(ctx),
                None => 1.0,
            };
            if conf <= 0.0 {
                continue;
            }
            best = match best {
                None => Some((conf, i)),
                Some((bc, _)) if conf > bc => Some((conf, i)),
                other => other,
            };
        }
        best.map(|(_, i)| &self.registrations[i].id)
    }

    /// Inherent form of payload-magic resolution that returns a
    /// reference. The owned-value form used by container code lives
    /// behind the [`CodecResolver`] trait impl below.
    ///
    /// Walks every registered payload magic prefix (declared via
    /// [`CodecInfo::payload_magic`] / [`CodecInfo::payload_magics`]) and
    /// returns the codec whose magic is a prefix of `first_bytes` —
    /// however much of the stream's leading payload the caller has
    /// (an Ogg demuxer passes the first packet of a logical stream; a
    /// raw-stream prober passes the file head). The **longest**
    /// matching magic wins (most specific claim); remaining ties are
    /// broken by registration order (first wins). Unlike the tag path
    /// there is no probe step: a payload magic is itself the bitstream
    /// evidence a probe would look for, and specificity is expressed
    /// by prefix length instead of a confidence value.
    pub fn resolve_payload_magic_ref(&self, first_bytes: &[u8]) -> Option<&CodecId> {
        let mut best: Option<(usize, usize)> = None; // (magic_len, reg idx)
        for (magic, idx) in &self.magic_index {
            if !first_bytes.starts_with(magic) {
                continue;
            }
            // Strict `>` keeps the earlier registration on equal
            // lengths — `magic_index` is in registration order.
            best = match best {
                None => Some((magic.len(), *idx)),
                Some((len, _)) if magic.len() > len => Some((magic.len(), *idx)),
                other => other,
            };
        }
        best.map(|(_, i)| &self.registrations[i].id)
    }

    /// Iterator over every `(payload magic, codec_id)` pair currently
    /// registered, in registration order — the payload-magic companion
    /// to [`all_tag_registrations`](Self::all_tag_registrations).
    pub fn all_payload_magic_registrations(&self) -> impl Iterator<Item = (&[u8], &CodecId)> {
        self.magic_index
            .iter()
            .map(move |(magic, i)| (magic.as_slice(), &self.registrations[*i].id))
    }
}

/// Implement the shared [`CodecResolver`] interface so container
/// demuxers can accept `&dyn CodecResolver` without depending on
/// this crate directly — the trait lives in oxideav-core.
impl CodecResolver for CodecRegistry {
    fn resolve_tag(&self, ctx: &ProbeContext) -> Option<CodecId> {
        self.resolve_tag_ref(ctx).cloned()
    }

    fn resolve_payload_magic(&self, first_packet: &[u8]) -> Option<CodecId> {
        self.resolve_payload_magic_ref(first_packet).cloned()
    }
}

#[cfg(test)]
mod tag_tests {
    use super::*;
    use crate::CodecCapabilities;

    /// Probe: return 1.0 iff the peeked bytes look like MS-MPEG4 (no
    /// 0x000001 start code in the first few bytes).
    fn probe_msmpeg4(ctx: &ProbeContext) -> f32 {
        match ctx.packet {
            Some(d) if !d.windows(3).take(6).any(|w| w == [0x00, 0x00, 0x01]) => 1.0,
            Some(_) => 0.0,
            None => 0.5, // no data yet — weak evidence
        }
    }

    /// Probe: return 1.0 iff the peeked bytes look like MPEG-4 Part 2
    /// (starts with a 0x000001 start code in the first few bytes).
    fn probe_mpeg4_part2(ctx: &ProbeContext) -> f32 {
        match ctx.packet {
            Some(d) if d.windows(3).take(6).any(|w| w == [0x00, 0x00, 0x01]) => 1.0,
            Some(_) => 0.0,
            None => 0.5,
        }
    }

    fn info(id: &str) -> CodecInfo {
        CodecInfo::new(CodecId::new(id)).capabilities(CodecCapabilities::audio(id))
    }

    #[test]
    fn resolve_single_claim_no_probe() {
        let mut reg = CodecRegistry::new();
        reg.register(info("flac").tag(CodecTag::fourcc(b"FLAC")));
        let t = CodecTag::fourcc(b"FLAC");
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&t))
                .map(|c| c.as_str()),
            Some("flac"),
        );
    }

    #[test]
    fn resolve_missing_tag_returns_none() {
        let reg = CodecRegistry::new();
        let t = CodecTag::fourcc(b"????");
        assert!(reg.resolve_tag_ref(&ProbeContext::new(&t)).is_none());
    }

    #[test]
    fn unprobed_claims_tie_first_registered_wins() {
        // Two unprobed claims on the same tag: deterministic order.
        let mut reg = CodecRegistry::new();
        reg.register(info("first").tag(CodecTag::fourcc(b"TEST")));
        reg.register(info("second").tag(CodecTag::fourcc(b"TEST")));
        let t = CodecTag::fourcc(b"TEST");
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&t))
                .map(|c| c.as_str()),
            Some("first"),
        );
    }

    #[test]
    fn probe_picks_matching_bitstream() {
        // The core bug fix: every probe is asked and the highest
        // confidence wins regardless of registration order.
        let mut reg = CodecRegistry::new();
        reg.register(
            info("msmpeg4v3")
                .probe(probe_msmpeg4)
                .tag(CodecTag::fourcc(b"DIV3")),
        );
        reg.register(
            info("mpeg4video")
                .probe(probe_mpeg4_part2)
                .tag(CodecTag::fourcc(b"DIV3")),
        );

        let mpeg4_part2 = [0x00u8, 0x00, 0x01, 0xB0, 0x01, 0x00];
        let ms_mpeg4 = [0x85u8, 0x3F, 0xD4, 0x80, 0x00, 0xA2];
        let tag = CodecTag::fourcc(b"DIV3");

        let ctx_part2 = ProbeContext::new(&tag).packet(&mpeg4_part2);
        assert_eq!(
            reg.resolve_tag_ref(&ctx_part2).map(|c| c.as_str()),
            Some("mpeg4video"),
        );
        let ctx_ms = ProbeContext::new(&tag).packet(&ms_mpeg4);
        assert_eq!(
            reg.resolve_tag_ref(&ctx_ms).map(|c| c.as_str()),
            Some("msmpeg4v3"),
        );
    }

    #[test]
    fn unprobed_claim_wins_against_low_confidence_probe() {
        // One codec claims a tag without a probe (→ confidence 1.0)
        // and another claims it with a probe returning 0.3. The
        // unprobed one wins — a codec that knows it owns the tag
        // outright should not lose to a speculative probe.
        let mut reg = CodecRegistry::new();
        reg.register(info("owner").tag(CodecTag::fourcc(b"OWN_")));
        reg.register(
            info("speculative")
                .probe(|_| 0.3)
                .tag(CodecTag::fourcc(b"OWN_")),
        );
        let t = CodecTag::fourcc(b"OWN_");
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&t))
                .map(|c| c.as_str()),
            Some("owner"),
        );
    }

    #[test]
    fn probe_returning_zero_is_skipped() {
        let mut reg = CodecRegistry::new();
        reg.register(
            info("refuses")
                .probe(|_| 0.0)
                .tag(CodecTag::fourcc(b"MAYB")),
        );
        reg.register(info("fallback").tag(CodecTag::fourcc(b"MAYB")));
        let t = CodecTag::fourcc(b"MAYB");
        let ctx = ProbeContext::new(&t).packet(b"hello");
        assert_eq!(
            reg.resolve_tag_ref(&ctx).map(|c| c.as_str()),
            Some("fallback"),
        );
    }

    #[test]
    fn fourcc_case_insensitive_lookup() {
        let mut reg = CodecRegistry::new();
        reg.register(info("vid").tag(CodecTag::fourcc(b"div3")));
        // Registered as "DIV3" (uppercase via ctor); lookup using
        // lowercase / mixed case also hits.
        let upper = CodecTag::fourcc(b"DIV3");
        let lower = CodecTag::fourcc(b"div3");
        let mixed = CodecTag::fourcc(b"DiV3");
        assert!(reg.resolve_tag_ref(&ProbeContext::new(&upper)).is_some());
        assert!(reg.resolve_tag_ref(&ProbeContext::new(&lower)).is_some());
        assert!(reg.resolve_tag_ref(&ProbeContext::new(&mixed)).is_some());
    }

    #[test]
    fn wave_format_and_matroska_tags_work() {
        let mut reg = CodecRegistry::new();
        reg.register(info("mp3").tag(CodecTag::wave_format(0x0055)));
        reg.register(info("h264").tag(CodecTag::matroska("V_MPEG4/ISO/AVC")));
        let wf = CodecTag::wave_format(0x0055);
        let mk = CodecTag::matroska("V_MPEG4/ISO/AVC");
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&wf))
                .map(|c| c.as_str()),
            Some("mp3"),
        );
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&mk))
                .map(|c| c.as_str()),
            Some("h264"),
        );
    }

    #[test]
    fn mp4_object_type_tag_works() {
        let mut reg = CodecRegistry::new();
        reg.register(info("aac").tag(CodecTag::mp4_object_type(0x40)));
        let t = CodecTag::mp4_object_type(0x40);
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&t))
                .map(|c| c.as_str()),
            Some("aac"),
        );
    }

    #[test]
    fn multi_tag_claim_all_resolve() {
        let mut reg = CodecRegistry::new();
        reg.register(info("aac").tags([
            CodecTag::fourcc(b"MP4A"),
            CodecTag::wave_format(0x00FF),
            CodecTag::mp4_object_type(0x40),
            CodecTag::matroska("A_AAC"),
        ]));
        for t in [
            CodecTag::fourcc(b"MP4A"),
            CodecTag::wave_format(0x00FF),
            CodecTag::mp4_object_type(0x40),
            CodecTag::matroska("A_AAC"),
        ] {
            assert_eq!(
                reg.resolve_tag_ref(&ProbeContext::new(&t))
                    .map(|c| c.as_str()),
                Some("aac"),
                "tag {t:?} did not resolve",
            );
        }
    }
}

#[cfg(test)]
mod payload_magic_tests {
    use super::*;
    use crate::CodecCapabilities;

    fn info(id: &str) -> CodecInfo {
        CodecInfo::new(CodecId::new(id)).capabilities(CodecCapabilities::audio(id))
    }

    /// Registry with the classic Ogg family registered, each under its
    /// real BOS magic.
    fn ogg_family_registry() -> CodecRegistry {
        let mut reg = CodecRegistry::new();
        reg.register(info("vorbis").payload_magic(b"\x01vorbis"));
        reg.register(info("opus").payload_magic(b"OpusHead"));
        reg.register(info("theora").payload_magic(b"\x80theora"));
        reg.register(info("flac").payload_magic(b"\x7fFLAC"));
        reg
    }

    #[test]
    fn resolve_payload_magic_matches_first_packet_prefix() {
        let reg = ogg_family_registry();
        // A Vorbis identification header: magic + version + channels +
        // rate + ... — the resolver only needs the prefix to match.
        let vorbis_id_header = b"\x01vorbis\x00\x00\x00\x00\x02\x44\xac\x00\x00";
        assert_eq!(
            reg.resolve_payload_magic_ref(vorbis_id_header)
                .map(|c| c.as_str()),
            Some("vorbis"),
        );
        assert_eq!(
            reg.resolve_payload_magic_ref(b"OpusHead\x01\x02\x38\x01")
                .map(|c| c.as_str()),
            Some("opus"),
        );
        assert_eq!(
            reg.resolve_payload_magic_ref(b"\x80theora\x03\x02\x01")
                .map(|c| c.as_str()),
            Some("theora"),
        );
        assert_eq!(
            reg.resolve_payload_magic_ref(b"\x7fFLAC\x01\x00")
                .map(|c| c.as_str()),
            Some("flac"),
        );
    }

    #[test]
    fn resolve_payload_magic_exact_length_packet_matches() {
        // A packet that is exactly the magic (nothing after it) still
        // resolves — starts_with is inclusive of equality.
        let reg = ogg_family_registry();
        assert_eq!(
            reg.resolve_payload_magic_ref(b"OpusHead")
                .map(|c| c.as_str()),
            Some("opus"),
        );
    }

    #[test]
    fn resolve_payload_magic_unknown_or_short_packet_is_none() {
        let reg = ogg_family_registry();
        // Unknown magic.
        assert!(reg.resolve_payload_magic_ref(b"Speex   1.2.0").is_none());
        // Packet shorter than every registered magic.
        assert!(reg.resolve_payload_magic_ref(b"Opus").is_none());
        // Empty packet.
        assert!(reg.resolve_payload_magic_ref(b"").is_none());
    }

    #[test]
    fn resolve_payload_magic_longest_prefix_wins_regardless_of_order() {
        // A shorter magic that is itself a prefix of a longer one must
        // lose to the more specific claim, whichever registered first.
        let mut reg = CodecRegistry::new();
        reg.register(info("generic").payload_magic(b"Opus"));
        reg.register(info("opus").payload_magic(b"OpusHead"));
        assert_eq!(
            reg.resolve_payload_magic_ref(b"OpusHead\x01")
                .map(|c| c.as_str()),
            Some("opus"),
        );
        // ...but the shorter claim still wins packets only it matches.
        assert_eq!(
            reg.resolve_payload_magic_ref(b"OpusTags")
                .map(|c| c.as_str()),
            Some("generic"),
        );

        // Same result with the registration order flipped.
        let mut reg = CodecRegistry::new();
        reg.register(info("opus").payload_magic(b"OpusHead"));
        reg.register(info("generic").payload_magic(b"Opus"));
        assert_eq!(
            reg.resolve_payload_magic_ref(b"OpusHead\x01")
                .map(|c| c.as_str()),
            Some("opus"),
        );
    }

    #[test]
    fn resolve_payload_magic_equal_length_tie_first_registered_wins() {
        let mut reg = CodecRegistry::new();
        reg.register(info("first").payload_magic(b"SameMagic"));
        reg.register(info("second").payload_magic(b"SameMagic"));
        assert_eq!(
            reg.resolve_payload_magic_ref(b"SameMagic\x00")
                .map(|c| c.as_str()),
            Some("first"),
        );
    }

    #[test]
    fn empty_payload_magic_is_ignored_at_registration() {
        let mut reg = CodecRegistry::new();
        reg.register(info("greedy").payload_magic(b""));
        assert!(reg.resolve_payload_magic_ref(b"anything at all").is_none());
        assert_eq!(reg.all_payload_magic_registrations().count(), 0);
    }

    #[test]
    fn payload_magics_plural_builder_and_diagnostics() {
        // One codec answering to several magics via the iterable
        // builder; the diagnostic iterator surfaces each claim in
        // registration order.
        let mut reg = CodecRegistry::new();
        reg.register(info("speex").payload_magics([b"Speex   ".to_vec(), b"speex-alt".to_vec()]));
        assert_eq!(
            reg.resolve_payload_magic_ref(b"Speex   1.2")
                .map(|c| c.as_str()),
            Some("speex"),
        );
        assert_eq!(
            reg.resolve_payload_magic_ref(b"speex-alt\x00")
                .map(|c| c.as_str()),
            Some("speex"),
        );
        let all: Vec<(&[u8], &str)> = reg
            .all_payload_magic_registrations()
            .map(|(m, id)| (m, id.as_str()))
            .collect();
        assert_eq!(
            all,
            vec![
                (b"Speex   ".as_slice(), "speex"),
                (b"speex-alt".as_slice(), "speex"),
            ],
        );
    }

    #[test]
    fn magic_claims_compose_with_tag_claims_on_one_registration() {
        // A codec that lives in both Ogg and Matroska declares both
        // claim kinds on one CodecInfo; each resolution path finds it.
        let mut reg = CodecRegistry::new();
        reg.register(
            info("vorbis")
                .tag(CodecTag::matroska("A_VORBIS"))
                .payload_magic(b"\x01vorbis"),
        );
        let mk = CodecTag::matroska("A_VORBIS");
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&mk))
                .map(|c| c.as_str()),
            Some("vorbis"),
        );
        assert_eq!(
            reg.resolve_payload_magic_ref(b"\x01vorbis\x00")
                .map(|c| c.as_str()),
            Some("vorbis"),
        );
    }

    #[test]
    fn resolver_trait_payload_magic_surface() {
        // The owned-value trait form mirrors the inherent form, and
        // the default implementation (NullCodecResolver) resolves
        // nothing.
        let reg = ogg_family_registry();
        let resolver: &dyn CodecResolver = &reg;
        assert_eq!(
            resolver
                .resolve_payload_magic(b"OpusHead\x01")
                .map(|c| c.0.clone()),
            Some("opus".to_owned()),
        );
        assert!(resolver.resolve_payload_magic(b"unknown").is_none());

        let null = crate::NullCodecResolver;
        assert!(null.resolve_payload_magic(b"OpusHead\x01").is_none());
    }

    /// The surface is container-agnostic: a raw elementary stream
    /// identified by a file-head magic resolves through the same path
    /// as the Ogg family — nothing about the mechanism is Ogg-shaped.
    #[test]
    fn payload_magic_serves_non_ogg_carriage() {
        let mut reg = CodecRegistry::new();
        reg.register(info("flac").payload_magic(b"fLaC"));
        reg.register(info("shorten").payload_magic(b"ajkg"));

        assert_eq!(
            reg.resolve_payload_magic_ref(b"fLaC\x00\x00\x00\x22"),
            Some(&CodecId::new("flac"))
        );
        assert_eq!(
            reg.resolve_payload_magic_ref(b"ajkg\x02"),
            Some(&CodecId::new("shorten"))
        );
        assert_eq!(reg.resolve_payload_magic_ref(b"RIFF"), None);
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::engine::HwDeviceInfo;

    #[test]
    fn codec_info_engine_id_and_probe_default_to_none() {
        let ci = CodecInfo::new(CodecId::new("h264"));
        assert!(ci.engine_id.is_none());
        assert!(ci.engine_probe.is_none());
    }

    #[test]
    fn codec_info_engine_builder_methods_set_fields() {
        fn dummy_probe() -> Vec<HwDeviceInfo> {
            vec![]
        }
        let ci = CodecInfo::new(CodecId::new("h264"))
            .with_engine_id("nvidia")
            .with_engine_probe(dummy_probe);
        assert_eq!(ci.engine_id, Some("nvidia"));
        assert!(ci.engine_probe.is_some());
        let probe = ci.engine_probe.unwrap();
        let result = probe();
        assert!(result.is_empty());
    }

    #[test]
    fn registering_codec_with_engine_metadata_does_not_panic() {
        // The new fields are passthrough metadata — register() should
        // accept them without affecting existing id/tag bookkeeping.
        fn dummy_probe() -> Vec<HwDeviceInfo> {
            vec![]
        }
        let mut reg = CodecRegistry::new();
        reg.register(
            CodecInfo::new(CodecId::new("h264"))
                .capabilities(CodecCapabilities::audio("h264_nvdec"))
                .tag(CodecTag::fourcc(b"H264"))
                .with_engine_id("nvidia")
                .with_engine_probe(dummy_probe),
        );
        let t = CodecTag::fourcc(b"H264");
        assert_eq!(
            reg.resolve_tag_ref(&ProbeContext::new(&t))
                .map(|c| c.as_str()),
            Some("h264"),
        );
    }

    /// No-op decoder factory so the registration produces a real
    /// CodecImplementation (the registry skips tag-only entries —
    /// without a factory there'd be nothing in `implementations()`
    /// to assert against).
    fn dummy_decoder_factory(
        _params: &crate::CodecParameters,
    ) -> crate::Result<Box<dyn super::Decoder>> {
        Err(crate::Error::unsupported("dummy decoder"))
    }

    #[test]
    fn engine_metadata_propagates_through_register() {
        fn dummy_probe() -> Vec<HwDeviceInfo> {
            vec![]
        }
        let mut reg = CodecRegistry::default();
        reg.register(
            CodecInfo::new(CodecId::new("h264"))
                .capabilities(CodecCapabilities::video("h264_test"))
                .decoder(dummy_decoder_factory)
                .with_engine_id("test-backend")
                .with_engine_probe(dummy_probe),
        );
        let impls = reg.implementations(&CodecId::new("h264"));
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].engine_id, Some("test-backend"));
        assert!(impls[0].engine_probe.is_some());
    }

    #[test]
    fn engine_metadata_absent_for_sw_codecs() {
        // SW codecs don't call the engine builders — both fields
        // should land as None on the resulting CodecImplementation.
        let mut reg = CodecRegistry::default();
        reg.register(
            CodecInfo::new(CodecId::new("flac"))
                .capabilities(CodecCapabilities::audio("flac_sw"))
                .decoder(dummy_decoder_factory),
        );
        let impls = reg.implementations(&CodecId::new("flac"));
        assert_eq!(impls.len(), 1);
        assert!(impls[0].engine_id.is_none());
        assert!(impls[0].engine_probe.is_none());
    }
}
