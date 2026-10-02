//! Container traits (demuxer + muxer) and a registry.
//!
//! This module defines the abstract [`Demuxer`] / [`Muxer`] traits that
//! every container implementation (oxideav-mp4, oxideav-mkv,
//! oxideav-flac, oxideav-ogg, …) fulfils, plus a
//! [`ContainerRegistry`] that consumers of the framework use to pick a
//! demuxer by probe bytes or filename hint.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::{CodecResolver, Error, Packet, Result, StreamInfo};

// ───────────────────────── traits ─────────────────────────

/// Reads a container and emits packets per stream.
pub trait Demuxer: Send {
    /// Name of the container format (e.g., `"wav"`).
    fn format_name(&self) -> &str;

    /// Streams in this container. Stable across the lifetime of the demuxer.
    fn streams(&self) -> &[StreamInfo];

    /// Read the next packet from any stream. Returns `Error::Eof` at end.
    fn next_packet(&mut self) -> Result<Packet>;

    /// Hint that only the listed stream indices will be consumed by the
    /// pipeline. Demuxers that can efficiently skip inactive streams at
    /// the container level (e.g., MKV cluster-aware, MP4 trak-aware)
    /// should override this. The default is a no-op — the pipeline
    /// drops unwanted packets on the floor.
    fn set_active_streams(&mut self, _indices: &[u32]) {}

    /// Seek to the nearest keyframe at or before `pts` (in the given
    /// stream's time base). Returns the actual timestamp seeked to, or
    /// `Error::Unsupported` if this demuxer can't seek.
    fn seek_to(&mut self, _stream_index: u32, _pts: i64) -> Result<i64> {
        Err(Error::unsupported("this demuxer does not support seeking"))
    }

    /// Container-level metadata as ordered (key, value) pairs.
    /// Keys follow a loose convention borrowed from Vorbis comments:
    /// `title`, `artist`, `album`, `comment`, `date`, `sample_name:<n>`,
    /// `channels`, `n_patterns`, etc. Demuxers that carry no metadata
    /// return an empty slice (the default).
    fn metadata(&self) -> &[(String, String)] {
        &[]
    }
    /// Container-level duration, if known. Default is `None` — callers
    /// may fall back to the longest per-stream duration. Expressed as
    /// microseconds for portability; convert to seconds at the edge.
    fn duration_micros(&self) -> Option<i64> {
        None
    }

    /// Attached pictures (cover art, artist photos, ...) embedded in
    /// the container. Returns an empty slice (the default) when the
    /// container carries none or doesn't support them. Containers that
    /// do — ID3v2 on MP3, `METADATA_BLOCK_PICTURE` on FLAC, `covr`
    /// atoms on MP4, etc. — override this to expose the images.
    fn attached_pictures(&self) -> &[crate::AttachedPicture] {
        &[]
    }

    /// Structured chapter / cue list. Default returns an empty slice
    /// for back-compat; demuxers that carry chapters (MKV `Chapters`,
    /// MP4 chapter track, Ogg `CHAPTERnn=` Vorbis comments, …) should
    /// override and return [`Chapter`](crate::Chapter) records in
    /// presentation order. Coexists with the legacy `chapter:N:*`
    /// flat-metadata keys; new consumers should prefer this.
    fn chapters(&self) -> &[crate::Chapter] {
        &[]
    }

    /// Structured attachment list. Default returns an empty slice for
    /// back-compat; demuxers that carry attachments (MKV `Attachments`,
    /// …) should override and return [`Attachment`](crate::Attachment)
    /// records in container order. Coexists with the legacy
    /// `attachment:N:*` flat-metadata keys; new consumers should prefer
    /// this.
    fn attachments(&self) -> &[crate::Attachment] {
        &[]
    }
}

/// Writes packets into a container.
pub trait Muxer: Send {
    /// Registered name of the container format being written.
    fn format_name(&self) -> &str;

    /// Write the container header. Must be called after stream configuration
    /// and before the first `write_packet`.
    fn write_header(&mut self) -> Result<()>;

    /// Write one compressed packet into the container.
    fn write_packet(&mut self, packet: &Packet) -> Result<()>;

    /// Finalize the file (write index, patch in total sizes, etc.).
    fn write_trailer(&mut self) -> Result<()>;
}

/// Factory that tries to open a stream as a particular container format.
///
/// Implementations should read the minimum needed to confirm the format and
/// return `Error::InvalidData` if the stream is not in this format.
///
/// The `codecs` parameter carries a resolver that converts container-
/// level codec tags (FourCCs, WAVEFORMATEX wFormatTag, Matroska
/// CodecIDs, …) into [`CodecId`](crate::CodecId) values.
pub type OpenDemuxerFn =
    fn(input: Box<dyn ReadSeek>, codecs: &dyn CodecResolver) -> Result<Box<dyn Demuxer>>;

/// Factory that creates a muxer for a set of streams.
pub type OpenMuxerFn =
    fn(output: Box<dyn WriteSeek>, streams: &[StreamInfo]) -> Result<Box<dyn Muxer>>;

/// Information passed to a content-based [`ContainerProbeFn`].
///
/// `buf` holds the first few KB of the input — enough to recognise the
/// magic bytes of any container we know about. `ext` carries the file
/// extension as a hint (lowercase, no leading dot); some containers
/// (raw MP3 with no ID3v2, headerless tracker formats) need it to break
/// ties with otherwise weak signatures.
pub struct ProbeData<'a> {
    /// First few KB of the input, for magic-byte matching.
    pub buf: &'a [u8],
    /// File-extension hint (lowercase, no leading dot), when known.
    pub ext: Option<&'a str>,
}

/// Confidence score returned by a [`ContainerProbeFn`]. `0` means no match.
/// Higher means more certain. Conventional values:
///
/// * `100` – unambiguous magic bytes at a known offset
/// * `75`  – signature match corroborated by file extension
/// * `50`  – signature match without extension corroboration
/// * `25`  – extension match only (no content signature available)
pub type ProbeScore = u8;

/// Maximum probe score (alias for `100`).
pub const MAX_PROBE_SCORE: ProbeScore = 100;
/// Default score returned when only the file extension matches.
pub const PROBE_SCORE_EXTENSION: ProbeScore = 25;

/// Content-based format detection function.
///
/// Returns a [`ProbeScore`] in `0..=100`. Implementations should be
/// pure (no I/O, no allocation beyond the stack) and fast — they may
/// be invoked once per registered demuxer on every input file.
pub type ContainerProbeFn = fn(probe: &ProbeData) -> ProbeScore;

/// Convenience trait bundle for seekable readers.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// Convenience trait bundle for seekable writers.
pub trait WriteSeek: Write + Seek + Send {}
impl<T: Write + Seek + Send> WriteSeek for T {}

// ───────────────────────── ContainerRegistry ─────────────────────────

/// Registry of container formats: demuxer/muxer factories keyed by
/// format name, plus the extension map and content probes that back
/// input auto-detection.
#[derive(Default)]
pub struct ContainerRegistry {
    demuxers: HashMap<String, OpenDemuxerFn>,
    muxers: HashMap<String, OpenMuxerFn>,
    /// Lowercase file extension → container name (e.g. "wav" → "wav").
    extensions: HashMap<String, String>,
    /// Container name → content-probe function. Optional — containers
    /// without a probe still work but require an extension hint or an
    /// explicit format name.
    probes: HashMap<String, ContainerProbeFn>,
}

impl ContainerRegistry {
    /// An empty registry (same as `Default`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a demuxer factory under a container format name.
    pub fn register_demuxer(&mut self, name: &str, open: OpenDemuxerFn) {
        self.demuxers.insert(name.to_owned(), open);
    }

    /// Register a muxer factory under a container format name.
    pub fn register_muxer(&mut self, name: &str, open: OpenMuxerFn) {
        self.muxers.insert(name.to_owned(), open);
    }

    /// Map a file extension (case-insensitive) to a registered
    /// container name, for extension-hint lookups.
    pub fn register_extension(&mut self, ext: &str, container_name: &str) {
        self.extensions
            .insert(ext.to_lowercase(), container_name.to_owned());
    }

    /// Attach a content-based probe to a registered demuxer. Called by
    /// the registry's [`probe_input`](Self::probe_input) to detect the
    /// container format from the first few KB of an input stream.
    pub fn register_probe(&mut self, container_name: &str, probe: ContainerProbeFn) {
        self.probes.insert(container_name.to_owned(), probe);
    }

    /// Iterate the registered demuxer format names (arbitrary order).
    pub fn demuxer_names(&self) -> impl Iterator<Item = &str> {
        self.demuxers.keys().map(|s| s.as_str())
    }

    /// Iterate the registered muxer format names (arbitrary order).
    pub fn muxer_names(&self) -> impl Iterator<Item = &str> {
        self.muxers.keys().map(|s| s.as_str())
    }

    /// Open a demuxer explicitly by format name. The `codecs` resolver
    /// is passed through to the demuxer so it can translate the
    /// container's in-stream codec tags (FourCCs / wFormatTag /
    /// Matroska CodecIDs) into [`CodecId`](crate::CodecId)
    /// values. Demuxers that don't need tag resolution can ignore it.
    pub fn open_demuxer(
        &self,
        name: &str,
        input: Box<dyn ReadSeek>,
        codecs: &dyn CodecResolver,
    ) -> Result<Box<dyn Demuxer>> {
        let open = self
            .demuxers
            .get(name)
            .ok_or_else(|| Error::FormatNotFound(name.to_owned()))?;
        open(input, codecs)
    }

    /// Open a muxer by format name.
    pub fn open_muxer(
        &self,
        name: &str,
        output: Box<dyn WriteSeek>,
        streams: &[StreamInfo],
    ) -> Result<Box<dyn Muxer>> {
        let open = self
            .muxers
            .get(name)
            .ok_or_else(|| Error::FormatNotFound(name.to_owned()))?;
        open(output, streams)
    }

    /// Look up a container name from a file extension (no leading dot).
    pub fn container_for_extension(&self, ext: &str) -> Option<&str> {
        self.extensions.get(&ext.to_lowercase()).map(|s| s.as_str())
    }

    /// Detect the container format by reading the first ~256 KB of the
    /// input, scoring each registered probe, and returning the highest-
    /// scoring container's name. The extension is passed to probes as a
    /// hint — they may use it to break ties when their signature is weak.
    ///
    /// Falls back to the extension table if no probe scores above zero.
    /// The input cursor is restored to its starting position on success
    /// and on the I/O failure paths that allow it.
    pub fn probe_input(&self, input: &mut dyn ReadSeek, ext_hint: Option<&str>) -> Result<String> {
        const PROBE_BUF_SIZE: usize = 256 * 1024;

        let saved_pos = input.stream_position()?;
        input.seek(SeekFrom::Start(0))?;
        let mut buf = vec![0u8; PROBE_BUF_SIZE];
        let mut got = 0;
        while got < buf.len() {
            match input.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let _ = input.seek(SeekFrom::Start(saved_pos));
                    return Err(e.into());
                }
            }
        }
        buf.truncate(got);
        input.seek(SeekFrom::Start(saved_pos))?;

        let ext_lower = ext_hint.map(|s| s.to_ascii_lowercase());
        let probe_data = ProbeData {
            buf: &buf,
            ext: ext_lower.as_deref(),
        };

        let mut best: Option<(&str, ProbeScore)> = None;
        for (name, probe) in &self.probes {
            let score = probe(&probe_data);
            if score == 0 {
                continue;
            }
            match best {
                Some((_, prev)) if score <= prev => {}
                _ => best = Some((name.as_str(), score)),
            }
        }
        if let Some((name, _)) = best {
            return Ok(name.to_owned());
        }

        // Fall back to extension lookup with the conventional weak score.
        if let Some(ext) = ext_hint {
            if let Some(name) = self.container_for_extension(ext) {
                let _ = PROBE_SCORE_EXTENSION; // export retained for symmetry
                return Ok(name.to_owned());
            }
        }

        Err(Error::FormatNotFound(
            "no registered demuxer recognises this input".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyDemuxer;

    impl Demuxer for DummyDemuxer {
        fn format_name(&self) -> &str {
            "dummy"
        }
        fn streams(&self) -> &[StreamInfo] {
            &[]
        }
        fn next_packet(&mut self) -> Result<Packet> {
            Err(Error::Eof)
        }
    }

    #[test]
    fn default_seek_to_is_unsupported() {
        let mut d = DummyDemuxer;
        match d.seek_to(0, 0) {
            Err(Error::Unsupported(_)) => {}
            other => panic!(
                "expected default seek_to to return Unsupported, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn default_chapters_and_attachments_are_empty() {
        // A demuxer that overrides nothing must compile and return
        // empty slices for both structured accessors. This is the
        // back-compat contract that lets every existing demuxer pick
        // up the new API without source changes.
        let d = DummyDemuxer;
        assert!(d.chapters().is_empty());
        assert!(d.attachments().is_empty());
        assert!(d.attached_pictures().is_empty());
        assert!(d.metadata().is_empty());
        assert_eq!(d.duration_micros(), None);
    }
}
