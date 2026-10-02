//! Shared error type for oxideav.
//!
//! # Taxonomy
//!
//! Pick the variant by what the *caller* should do about it:
//!
//! * [`Error::InvalidData`] — the input violates its format's rules;
//!   retrying or feeding more bytes won't help. Skip the packet / abort
//!   the stream.
//! * [`Error::Unsupported`] — the input is (as far as we can tell)
//!   valid, but exercises a feature this implementation doesn't cover.
//!   A different implementation might succeed.
//! * [`Error::Eof`] — the logical end of the stream was reached. Not a
//!   failure when it happens between packets; drain and stop.
//! * [`Error::NeedMore`] — a push-style parser stopped mid-unit; feed
//!   more bytes and call again. Unlike `Eof`, progress resumes.
//! * [`Error::FormatNotFound`] / [`Error::CodecNotFound`] — registry
//!   probe/lookup misses.
//! * [`Error::ResourceExhausted`] — a configured cap or pool limit
//!   fired; hard-reject the input or back off, never retry blindly.
//! * [`Error::Io`] / [`Error::Other`] — transport problems and
//!   everything else.

use thiserror::Error;

/// Convenience alias: `std::result::Result` pinned to [`enum@Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// The error type shared by every oxideav crate. See the
/// [module docs](self) for which variant to pick.
#[derive(Debug, Error)]
pub enum Error {
    /// An underlying transport / filesystem operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Valid input exercising a feature this implementation lacks.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// The input violates its format's rules; not retryable.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// Logical end of stream (clean between packets; short otherwise).
    #[error("end of stream")]
    Eof,

    /// Push-parser starvation: feed more bytes and call again.
    #[error("need more data")]
    NeedMore,

    /// No registered container format matched the probe subject.
    #[error("format not found: {0}")]
    FormatNotFound(String),

    /// No registered codec matched the requested name or tag.
    #[error("codec not found: {0}")]
    CodecNotFound(String),

    /// A decoder (or arena pool) refused to allocate or proceed because
    /// doing so would exceed a configured [`DecoderLimits`](crate::DecoderLimits)
    /// cap, or because a pool has no free slot. This is the canonical
    /// "DoS protection fired" error — callers should treat it as a hard
    /// rejection of the input or a transient backpressure signal, never
    /// retry blindly.
    #[error("resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Anything that doesn't fit the other variants; the message
    /// carries the whole story.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Construct an [`Error::Unsupported`] with the given message.
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }

    /// Construct an [`Error::InvalidData`] with the given message.
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidData(msg.into())
    }

    /// Construct an [`Error::Other`] with the given message.
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    /// Construct a [`Error::ResourceExhausted`] with the given message.
    /// Use this from any decoder that has just hit a `DecoderLimits` cap
    /// or an arena-pool exhaustion.
    pub fn resource_exhausted(msg: impl Into<String>) -> Self {
        Self::ResourceExhausted(msg.into())
    }

    /// Construct a [`Error::FormatNotFound`] with the given probe
    /// subject (file name, extension, or magic description).
    pub fn format_not_found(msg: impl Into<String>) -> Self {
        Self::FormatNotFound(msg.into())
    }

    /// Construct a [`Error::CodecNotFound`] with the codec name or tag
    /// that missed the registry.
    pub fn codec_not_found(msg: impl Into<String>) -> Self {
        Self::CodecNotFound(msg.into())
    }

    /// `true` for [`Error::Eof`]. `Error` cannot implement `PartialEq`
    /// (the `Io` variant wraps `std::io::Error`), so drain loops that
    /// need "stop cleanly on end-of-stream" branch on this instead of
    /// a `matches!` at every call site.
    pub fn is_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }

    /// `true` for [`Error::NeedMore`] — the push-parser "feed me more
    /// bytes and retry" signal.
    pub fn is_need_more(&self) -> bool {
        matches!(self, Self::NeedMore)
    }

    /// `true` for [`Error::ResourceExhausted`] — the "DoS cap fired"
    /// signal that must not be blindly retried.
    pub fn is_resource_exhausted(&self) -> bool {
        matches!(self, Self::ResourceExhausted(_))
    }

    /// `true` when the error only says the stream stopped short —
    /// [`Error::Eof`] or [`Error::NeedMore`] — rather than reporting
    /// malformed or unsupported content. Useful for probe loops that
    /// try successive parsers on a growing prefix: starvation means
    /// "inconclusive, buffer more", anything else means "this parser
    /// has a verdict".
    pub fn is_starved(&self) -> bool {
        matches!(self, Self::Eof | Self::NeedMore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_produce_matching_variants() {
        assert!(matches!(
            Error::format_not_found("mkv"),
            Error::FormatNotFound(s) if s == "mkv"
        ));
        assert!(matches!(
            Error::codec_not_found("vp8"),
            Error::CodecNotFound(s) if s == "vp8"
        ));
        assert!(matches!(
            Error::resource_exhausted("pool"),
            Error::ResourceExhausted(s) if s == "pool"
        ));
    }

    #[test]
    fn predicates_partition_correctly() {
        assert!(Error::Eof.is_eof());
        assert!(!Error::Eof.is_need_more());
        assert!(Error::NeedMore.is_need_more());
        assert!(!Error::NeedMore.is_eof());
        assert!(Error::Eof.is_starved());
        assert!(Error::NeedMore.is_starved());
        assert!(Error::resource_exhausted("x").is_resource_exhausted());
        for e in [
            Error::invalid("bad"),
            Error::unsupported("feature"),
            Error::other("misc"),
            Error::format_not_found("f"),
            Error::codec_not_found("c"),
        ] {
            assert!(!e.is_eof());
            assert!(!e.is_need_more());
            assert!(!e.is_starved());
            assert!(!e.is_resource_exhausted());
        }
    }

    #[test]
    fn display_messages_are_stable() {
        assert_eq!(Error::Eof.to_string(), "end of stream");
        assert_eq!(Error::NeedMore.to_string(), "need more data");
        assert_eq!(Error::invalid("x").to_string(), "invalid data: x");
        assert_eq!(Error::unsupported("y").to_string(), "unsupported: y");
        assert_eq!(
            Error::format_not_found("z").to_string(),
            "format not found: z"
        );
        assert_eq!(
            Error::codec_not_found("w").to_string(),
            "codec not found: w"
        );
        assert_eq!(
            Error::resource_exhausted("v").to_string(),
            "resource exhausted: v"
        );
        assert_eq!(Error::other("u").to_string(), "u");
    }
}
