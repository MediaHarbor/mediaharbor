//! `MediaSource` adapter over the local streaming server.
//!
//! The streaming server already owns every service-specific quirk (Deezer
//! chunk alignment, Apple range windows, Tidal remux, tempfile tail-follow),
//! so this only has to speak ranged HTTP and present a blocking `Read + Seek`
//! to symphonia.

use std::io::{self, Read, Seek, SeekFrom};

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::{Client, Response, StatusCode};
use symphonia::core::io::MediaSource;
use tokio::runtime::Handle;

/// How much to pull per upstream request. The server caps its own window for
/// some services (Apple clamps to 2 MiB after the first request), so this is an
/// upper bound rather than a guarantee.
const CHUNK: u64 = 512 * 1024;

/// Bytes a stream the source holds open must deliver before a read returns.
///
/// Such a stream arrives in real time, so requiring a full `CHUNK` costs
/// `CHUNK / bitrate` seconds before symphonia can even probe it: at the 64 kbps
/// where HE-AAC radio lives that is 65 s, well past the 30 s `NativePlayer::load`
/// waits before reporting a timeout. Every station under ~140 kbps failed to open
/// for this reason, which looked like a codec fault because AAC+ is exactly what
/// low-bitrate stations use.
///
/// `CHUNK` remains the upper bound, so a stream that already has more buffered
/// still hands over up to 512 KiB in one call — a fast progressive source is not
/// slowed down, it just stops being able to stall the open.
const LIVE_CHUNK: usize = 16 * 1024;

/// What the first response told us about the stream.
#[derive(Debug, Default, Clone, Copy)]
struct Capabilities {
    seekable: bool,
    total: Option<u64>,
}

/// Boxed body stream of a response we keep open across reads.
type BodyStream = std::pin::Pin<
    Box<dyn futures_util::Stream<Item = reqwest::Result<Bytes>> + Send + Sync + 'static>,
>;

pub struct MhMediaSource {
    client: Client,
    handle: Handle,
    url: String,
    /// Absolute byte offset of the next `read`.
    pos: u64,
    /// Buffered bytes and the absolute offset `buf[0]` sits at.
    buf: Bytes,
    buf_start: u64,
    /// Populated from the first response; `None` until then.
    caps: Option<Capabilities>,
    /// Set once the server reports the end of the stream.
    eof: bool,
    /// Held open for non-seekable streams, which the server serves exactly
    /// once — re-requesting them would 404 as already consumed.
    body: Option<BodyStream>,
    /// Bytes read past the end of the last fill on `body`, kept for the next.
    carry: BytesMut,
}

impl MhMediaSource {
    /// Open `url` and fetch its first chunk.
    ///
    /// Symphonia queries `is_seekable()`/`byte_len()` before reading a byte and
    /// takes a very different path for an unseekable source, so capabilities
    /// have to be known up front. They are learned from this first real request
    /// rather than a throwaway preflight, because progressive streams are
    /// consume-once server-side and a probe would destroy them. The fetched
    /// bytes are kept, so nothing is wasted.
    pub fn open(client: Client, handle: Handle, url: impl Into<String>) -> io::Result<Self> {
        let mut src = Self {
            client,
            handle,
            url: url.into(),
            pos: 0,
            buf: Bytes::new(),
            buf_start: 0,
            caps: None,
            eof: false,
            body: None,
            carry: BytesMut::new(),
        };
        src.fill(0)?;
        Ok(src)
    }

    /// Capabilities are populated by `open`, so this never probes.
    fn capabilities(&self) -> Capabilities {
        self.caps.unwrap_or_default()
    }

    /// Merge freshly parsed capabilities into what is already known.
    ///
    /// A tempfile stream that is still downloading reports only what has landed
    /// so far, so its total grows between requests and must never shrink.
    fn learn(&mut self, fresh: Capabilities) -> Capabilities {
        match self.caps.as_mut() {
            Some(caps) => {
                if let Some(total) = fresh.total {
                    if caps.total.is_none_or(|known| total > known) {
                        caps.total = Some(total);
                    }
                }
                *caps
            }
            None => *self.caps.insert(fresh),
        }
    }

    fn buffer_covers(&self, pos: u64) -> bool {
        !self.buf.is_empty()
            && pos >= self.buf_start
            && pos < self.buf_start + self.buf.len() as u64
    }

    /// Fetch the chunk containing `pos` into `buf`.
    fn fill(&mut self, pos: u64) -> io::Result<()> {
        if let Some(body) = self.body.as_mut() {
            let carry = &mut self.carry;
            let chunk = self
                .handle
                .block_on(take_from_stream(body, carry, CHUNK as usize, LIVE_CHUNK))
                .map_err(|e| io::Error::other(format!("stream body failed: {e}")))?;
            if chunk.is_empty() {
                self.eof = true;
            }
            self.buf = chunk;
            self.buf_start = pos;
            return Ok(());
        }

        let req = self
            .client
            .get(&self.url)
            .header(RANGE, format!("bytes={}-{}", pos, pos + CHUNK - 1));

        let (status, headers, mut stream) = self
            .handle
            .block_on(async {
                let resp = req.send().await?;
                let status = resp.status();
                let headers = resp.headers().clone();
                Ok::<_, reqwest::Error>((status, headers, into_body(resp)))
            })
            .map_err(|e| io::Error::other(format!("stream request failed: {e}")))?;

        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            self.eof = true;
            self.buf = Bytes::new();
            return Ok(());
        }
        if !status.is_success() {
            return Err(io::Error::other(format!(
                "stream request returned HTTP {status}"
            )));
        }

        let caps = self.learn(read_capabilities(&headers, status));

        let assumed = if status == StatusCode::PARTIAL_CONTENT {
            pos
        } else {
            0
        };
        let body_start = content_range_start(&headers).unwrap_or(assumed);
        if body_start != pos {
            return Err(io::Error::other(format!(
                "stream ignored the requested range: asked for byte {pos}, got byte {body_start}"
            )));
        }

        self.carry.clear();
        // A source that cannot be re-requested by range is served in real time, so
        // it must not be allowed to hold this first read open for a full block.
        let min = if caps.seekable {
            CHUNK as usize
        } else {
            LIVE_CHUNK
        };
        let carry = &mut self.carry;
        let chunk = self
            .handle
            .block_on(take_from_stream(&mut stream, carry, CHUNK as usize, min))
            .map_err(|e| io::Error::other(format!("stream body failed: {e}")))?;

        if !caps.seekable {
            self.body = Some(stream);
        } else {
            self.carry.clear();
        }

        if chunk.is_empty() {
            self.eof = true;
        }
        self.buf = chunk;
        self.buf_start = pos;
        Ok(())
    }
}

fn into_body(resp: Response) -> BodyStream {
    Box::pin(resp.bytes_stream())
}

/// Read at most `limit` bytes from a body stream.
///
/// Several server branches ignore the range *end* and stream to EOF (the
/// local-file and tempfile readers seek to the start offset and then loop), so
/// draining the whole body would pull an entire track into memory on every
/// seek. Reading only what was asked for bounds the buffer.
///
/// A network chunk rarely ends exactly on `limit`, so the overshoot is kept in
/// `carry` for the next call. On a stream the source holds open — one it cannot
/// re-request by range — discarding it instead would punch a hole in the middle
/// of the audio every time the buffer refilled.
///
/// `min` is the point at which enough has arrived to return; `limit` is still the
/// ceiling. Passing `min == limit` reads a full block, which is what a seekable
/// source wants. A held-open stream passes `LIVE_CHUNK` so a slow station cannot
/// hold the first read — and therefore playback itself — open for a minute.
async fn take_from_stream(
    stream: &mut BodyStream,
    carry: &mut BytesMut,
    limit: usize,
    min: usize,
) -> Result<Bytes, reqwest::Error> {
    let mut out = BytesMut::with_capacity(limit.min(64 * 1024));

    if !carry.is_empty() {
        let take = carry.len().min(limit);
        out.extend_from_slice(&carry.split_to(take));
    }

    while out.len() < min {
        match stream.next().await {
            Some(chunk) => {
                let chunk = chunk?;
                let want = limit - out.len();
                if chunk.len() > want {
                    out.extend_from_slice(&chunk[..want]);
                    carry.extend_from_slice(&chunk[want..]);
                    break;
                }
                out.extend_from_slice(&chunk);
            }
            None => break,
        }
    }
    Ok(out.freeze())
}

/// First byte offset a `Content-Range` response body covers.
fn content_range_start(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())?
        .trim()
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Derive seekability and total length from response headers.
///
/// The streaming server omits `Accept-Ranges`/`Content-Length` precisely for
/// the variants that cannot seek (progressive channels, Tidal remux), so the
/// headers are authoritative and the player needs no per-service knowledge.
fn read_capabilities(headers: &HeaderMap, status: StatusCode) -> Capabilities {
    let num = |v: &str| v.trim().parse::<u64>().ok();

    let total = headers
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next().map(str::to_owned))
        .and_then(|v| num(&v))
        .or_else(|| {
            if status == StatusCode::PARTIAL_CONTENT {
                None
            } else {
                headers
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(num)
            }
        });

    let advertises_ranges = headers
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| !v.eq_ignore_ascii_case("none"))
        .unwrap_or(false);

    Capabilities {
        seekable: (advertises_ranges || status == StatusCode::PARTIAL_CONTENT) && total.is_some(),
        total,
    }
}

impl Read for MhMediaSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if !self.buffer_covers(self.pos) {
            if self.eof {
                return Ok(0);
            }
            self.fill(self.pos)?;
            if !self.buffer_covers(self.pos) {
                return Ok(0);
            }
        }

        let offset = (self.pos - self.buf_start) as usize;
        let n = out.len().min(self.buf.len() - offset);
        out[..n].copy_from_slice(&self.buf[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for MhMediaSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let caps = self.capabilities();
        let target = match from {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
            SeekFrom::End(d) => {
                let total = caps.total.ok_or_else(|| {
                    io::Error::other("cannot seek from end: stream length is unknown")
                })?;
                total.saturating_add_signed(d)
            }
        };

        if target != self.pos && !caps.seekable {
            return Err(io::Error::other("stream is not seekable"));
        }

        self.pos = target;
        if !self.buffer_covers(target) {
            self.eof = false;
        }
        Ok(target)
    }
}

impl MediaSource for MhMediaSource {
    fn is_seekable(&self) -> bool {
        self.capabilities().seekable
    }

    fn byte_len(&self) -> Option<u64> {
        self.capabilities().total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn ranged_response_is_seekable_with_total() {
        let caps = read_capabilities(
            &headers(&[
                ("accept-ranges", "bytes"),
                ("content-range", "bytes 0-1023/8192"),
            ]),
            StatusCode::PARTIAL_CONTENT,
        );
        assert!(caps.seekable);
        assert_eq!(caps.total, Some(8192));
    }

    #[test]
    fn progressive_response_is_not_seekable() {
        let caps = read_capabilities(&headers(&[("content-type", "audio/flac")]), StatusCode::OK);
        assert!(!caps.seekable);
        assert_eq!(caps.total, None);
    }

    #[test]
    fn plain_ok_with_length_reports_total_but_not_seekable() {
        let caps = read_capabilities(&headers(&[("content-length", "4096")]), StatusCode::OK);
        assert_eq!(caps.total, Some(4096));
        assert!(!caps.seekable, "no Accept-Ranges means no seeking");
    }

    #[test]
    fn partial_content_length_is_not_mistaken_for_total() {
        let caps = read_capabilities(
            &headers(&[("accept-ranges", "bytes"), ("content-length", "1024")]),
            StatusCode::PARTIAL_CONTENT,
        );
        assert_eq!(caps.total, None);
    }

    #[test]
    fn seeking_a_non_seekable_stream_is_refused() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut src = MhMediaSource {
            client: Client::new(),
            handle: rt.handle().clone(),
            url: "http://127.0.0.1:1/never-contacted".to_string(),
            pos: 0,
            buf: Bytes::new(),
            buf_start: 0,
            caps: Some(Capabilities {
                seekable: false,
                total: None,
            }),
            eof: false,
            body: None,
            carry: BytesMut::new(),
        };

        assert!(src.seek(SeekFrom::Start(1024)).is_err());
        assert_eq!(src.seek(SeekFrom::Start(0)).unwrap(), 0);
    }

    /// The url is unreachable, so a source that tries to refill errors instead
    /// of returning 0 — proving the end came from `eof` and not a guess.
    #[test]
    fn reading_past_the_end_returns_zero() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut src = MhMediaSource {
            client: Client::new(),
            handle: rt.handle().clone(),
            url: "http://127.0.0.1:1/never-contacted".to_string(),
            pos: 8,
            buf: Bytes::new(),
            buf_start: 0,
            caps: Some(Capabilities {
                seekable: true,
                total: Some(8),
            }),
            eof: true,
            body: None,
            carry: BytesMut::new(),
        };

        let mut out = [0u8; 4];
        assert_eq!(src.read(&mut out).unwrap(), 0);
    }

    #[test]
    fn a_still_downloading_stream_is_not_capped_at_its_first_reported_size() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut src = MhMediaSource {
            client: Client::new(),
            handle: rt.handle().clone(),
            url: "http://127.0.0.1:1/never-contacted".to_string(),
            pos: 0,
            buf: Bytes::from_static(b"0123456789"),
            buf_start: 0,
            caps: Some(Capabilities {
                seekable: true,
                total: Some(4),
            }),
            eof: false,
            body: None,
            carry: BytesMut::new(),
        };

        src.seek(SeekFrom::Start(4)).unwrap();
        let mut out = [0u8; 4];
        assert_eq!(
            src.read(&mut out).unwrap(),
            4,
            "audio the download produced after the first response must still play"
        );
        assert_eq!(&out, b"4567");
    }

    #[test]
    fn a_growing_total_is_learned_and_never_shrinks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut src = MhMediaSource {
            client: Client::new(),
            handle: rt.handle().clone(),
            url: "http://127.0.0.1:1/never-contacted".to_string(),
            pos: 0,
            buf: Bytes::new(),
            buf_start: 0,
            caps: Some(Capabilities {
                seekable: true,
                total: Some(1024),
            }),
            eof: false,
            body: None,
            carry: BytesMut::new(),
        };

        src.learn(Capabilities {
            seekable: true,
            total: Some(8192),
        });
        assert_eq!(src.byte_len(), Some(8192));

        src.learn(Capabilities {
            seekable: true,
            total: Some(4096),
        });
        assert_eq!(src.byte_len(), Some(8192), "a total must never shrink");
    }

    fn body_of(chunks: Vec<Vec<u8>>) -> BodyStream {
        Box::pin(futures_util::stream::iter(
            chunks.into_iter().map(|c| Ok(Bytes::from(c))),
        ))
    }

    /// A held-open stream cannot be re-requested by range, so anything read
    /// past the fill limit and dropped is gone from the middle of the track.
    /// Network chunks almost never end on the limit, so this fires constantly.
    #[test]
    fn a_chunk_straddling_the_limit_is_not_lost() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut stream = body_of(vec![vec![1u8; 300], vec![2u8; 300]]);
        let mut carry = BytesMut::new();

        let first = rt
            .block_on(take_from_stream(&mut stream, &mut carry, 500, 500))
            .unwrap();
        let second = rt
            .block_on(take_from_stream(&mut stream, &mut carry, 500, 500))
            .unwrap();

        assert_eq!(first.len(), 500);
        assert_eq!(
            first.len() + second.len(),
            600,
            "bytes went missing between fills"
        );
        assert_eq!(
            second[..],
            [2u8; 100],
            "the carried bytes are the right ones"
        );
    }

    #[test]
    fn a_stream_read_in_odd_sizes_arrives_whole_and_in_order() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let payload: Vec<u8> = (0..5_000u32).map(|i| (i % 251) as u8).collect();
        let mut stream = body_of(payload.chunks(137).map(<[u8]>::to_vec).collect());
        let mut carry = BytesMut::new();

        let mut got = Vec::new();
        loop {
            let part = rt
                .block_on(take_from_stream(&mut stream, &mut carry, 512, 512))
                .unwrap();
            if part.is_empty() {
                break;
            }
            got.extend_from_slice(&part);
        }
        assert_eq!(got, payload);
    }

    /// The bug this guards: a station delivers in real time, so a read that
    /// insists on a full `CHUNK` blocks for `CHUNK / bitrate` seconds — 65 s at
    /// the 64 kbps HE-AAC radio uses — and `load` times out at 30 s. Returning
    /// once `min` has arrived is what lets a low-bitrate station open at all.
    #[test]
    fn a_live_stream_returns_without_waiting_for_a_full_block() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // One small chunk, then a stream that would never yield another.
        let mut stream = body_of(vec![vec![7u8; 4096]]);
        let mut carry = BytesMut::new();

        let got = rt
            .block_on(take_from_stream(&mut stream, &mut carry, 512 * 1024, 2048))
            .unwrap();

        assert_eq!(
            got.len(),
            4096,
            "a live read must return what arrived, not block for the ceiling"
        );
    }

    /// `min` is a floor, not a cap: a source with plenty buffered still hands
    /// over a full block, so progressive service streams are not slowed down.
    #[test]
    fn a_fast_stream_still_fills_up_to_the_ceiling() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut stream = body_of(vec![vec![1u8; 8192]; 8]);
        let mut carry = BytesMut::new();

        let got = rt
            .block_on(take_from_stream(&mut stream, &mut carry, 32 * 1024, 4096))
            .unwrap();

        assert!(
            got.len() >= 8192,
            "a ready stream should not be throttled to `min`, got {}",
            got.len()
        );
        assert!(got.len() <= 32 * 1024, "the ceiling still applies");
    }

    #[test]
    fn content_range_start_is_read_from_the_header() {
        assert_eq!(
            content_range_start(&headers(&[("content-range", "bytes 1024-2047/8192")])),
            Some(1024)
        );
        assert_eq!(content_range_start(&headers(&[])), None);
    }

    #[test]
    fn accept_ranges_none_is_honoured() {
        let caps = read_capabilities(
            &headers(&[("accept-ranges", "none"), ("content-length", "4096")]),
            StatusCode::OK,
        );
        assert!(!caps.seekable);
    }
}
