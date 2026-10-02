use crate::services::deezer::client::DeezerClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::LazyLock;

/// Inserted as a lyric line when the instrumental gap between two sung lines is at
/// least this long, so LRC players show something during the break.
const GAP_MIN_MS: u64 = 1_500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTiming {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordSyncedLine {
    pub start_time: f64,
    pub end_time: f64,
    pub text: String,
    #[serde(default)]
    pub words: Vec<WordTiming>,
}

/// Line-level lyrics that may additionally carry per-word timings. This is the one
/// shape every service's lyrics get normalised into, so LRC, TTML and the
/// word-synced JSON the player consumes all come from a single source.
#[derive(Debug, Clone, Default)]
pub struct WordLyrics {
    pub lines: Vec<WordSyncedLine>,
}

impl WordLyrics {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn has_word_timings(&self) -> bool {
        self.lines.iter().any(|l| !l.words.is_empty())
    }

    /// The JSON the frontend player parses into `WordSyncedLine[]`.
    pub fn to_json(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        serde_json::to_string(&self.lines).ok()
    }

    pub fn to_plain(&self) -> Option<String> {
        let text = self
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        (!text.trim().is_empty()).then_some(text)
    }

    pub fn to_lrc(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut out: Vec<String> = Vec::with_capacity(self.lines.len());
        let mut prev_end_ms: Option<u64> = None;
        for line in &self.lines {
            let start_ms = secs_to_ms(line.start_time);
            if let Some(prev) = prev_end_ms {
                if start_ms.saturating_sub(prev) >= GAP_MIN_MS {
                    out.push(format!("{}♪", lrc_stamp(prev)));
                }
            }
            out.push(format!("{}{}", lrc_stamp(start_ms), line.text));
            prev_end_ms = Some(secs_to_ms(line.end_time).max(start_ms));
        }
        let lrc = out.join("\n");
        (!lrc.trim().is_empty()).then_some(lrc)
    }

    /// Word-synced TTML in the shape Apple Music publishes, so the same sidecar
    /// works for every service that exposes syllable timings.
    pub fn to_ttml(&self, title: Option<&str>, artist: Option<&str>) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let timing = if self.has_word_timings() {
            "Word"
        } else {
            "Line"
        };
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<tt xmlns=\"http://www.w3.org/ns/ttml\" \
             xmlns:ttm=\"http://www.w3.org/ns/ttml#metadata\" \
             xmlns:itunes=\"http://music.apple.com/lyric-ttml-internal\" \
             itunes:timing=\"{}\">\n",
            timing
        ));
        out.push_str("  <head>\n    <metadata>\n");
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            out.push_str(&format!("      <ttm:title>{}</ttm:title>\n", escape_xml(t)));
        }
        if let Some(a) = artist.filter(|a| !a.is_empty()) {
            out.push_str(&format!("      <ttm:agent>{}</ttm:agent>\n", escape_xml(a)));
        }
        out.push_str("    </metadata>\n  </head>\n  <body>\n    <div>\n");
        for line in &self.lines {
            let begin = ttml_stamp(line.start_time);
            let end = ttml_stamp(line.end_time.max(line.start_time));
            if line.words.is_empty() {
                out.push_str(&format!(
                    "      <p begin=\"{}\" end=\"{}\">{}</p>\n",
                    begin,
                    end,
                    escape_xml(&line.text)
                ));
            } else {
                out.push_str(&format!("      <p begin=\"{}\" end=\"{}\">", begin, end));
                for (i, w) in line.words.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    out.push_str(&format!(
                        "<span begin=\"{}\" end=\"{}\">{}</span>",
                        ttml_stamp(w.start),
                        ttml_stamp(w.end.max(w.start)),
                        escape_xml(&w.text)
                    ));
                }
                out.push_str("</p>\n");
            }
        }
        out.push_str("    </div>\n  </body>\n</tt>\n");
        Some(out)
    }
}

impl WordLyrics {
    /// Recovers line timings from an LRC so services that only publish LRC can still
    /// emit a (line-level) TTML sidecar.
    pub fn from_lrc(lrc: &str) -> Option<Self> {
        let mut lines: Vec<WordSyncedLine> = Vec::new();
        for raw in lrc.lines() {
            let mut rest = raw.trim_start();
            let mut stamps: Vec<f64> = Vec::new();
            while let Some(body) = rest.strip_prefix('[') {
                let Some(end) = body.find(']') else { break };
                let (stamp, tail) = body.split_at(end);
                if let Some(secs) = parse_lrc_stamp(stamp) {
                    stamps.push(secs);
                    rest = &tail[1..];
                } else {
                    break;
                }
            }
            if stamps.is_empty() {
                continue;
            }
            let text = rest.trim().to_string();
            for start in stamps {
                lines.push(WordSyncedLine {
                    start_time: start,
                    end_time: start,
                    text: text.clone(),
                    words: Vec::new(),
                });
            }
        }
        if lines.is_empty() {
            return None;
        }
        lines.sort_by(|a, b| a.start_time.total_cmp(&b.start_time));
        for i in 0..lines.len().saturating_sub(1) {
            lines[i].end_time = lines[i + 1].start_time;
        }
        Some(Self { lines })
    }
}

fn parse_lrc_stamp(stamp: &str) -> Option<f64> {
    let (mins, rest) = stamp.split_once(':')?;
    let mins: f64 = mins.trim().parse().ok()?;
    let secs: f64 = rest.trim().replace(',', ".").parse().ok()?;
    Some(mins * 60.0 + secs)
}

fn secs_to_ms(secs: f64) -> u64 {
    (secs.max(0.0) * 1000.0).round() as u64
}

fn lrc_stamp(ms: u64) -> String {
    let total_cs = ms / 10;
    format!(
        "[{:02}:{:02}.{:02}]",
        total_cs / 6000,
        (total_cs % 6000) / 100,
        total_cs % 100
    )
}

fn ttml_stamp(secs: f64) -> String {
    let ms = secs_to_ms(secs);
    let total_s = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total_s / 3600,
        (total_s % 3600) / 60,
        total_s % 60,
        ms % 1000
    )
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct LrcLibLyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
}

/// Community lyrics keyed by title/artist/duration. Needs no account of any kind, so
/// it is the fallback for services that publish no lyrics of their own.
pub async fn fetch_lrclib(
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> Option<LrcLibLyrics> {
    if title.trim().is_empty() {
        return None;
    }
    let http = crate::http_client::ua_client(crate::http_client::UA_MOZILLA).ok()?;
    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        url::form_urlencoded::byte_serialize(title.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(artist.as_bytes()).collect::<String>(),
    );
    if let Some(dur) = duration_secs {
        url.push_str(&format!("&duration={}", dur.round() as u64));
    }
    let resp = http
        .get(&url)
        .header("User-Agent", "MediaHarbor/1.0")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let out = LrcLibLyrics {
        synced: body["syncedLyrics"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty()),
        plain: body["plainLyrics"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty()),
    };
    (out.synced.is_some() || out.plain.is_some()).then_some(out)
}

/// Drops the `[mm:ss.xx]` stamps from an LRC so a service that only published
/// timed lyrics can still fill the plain-text tag.
pub fn strip_lrc_timestamps(lrc: &str) -> String {
    let mut out = String::with_capacity(lrc.len());
    for raw in lrc.lines() {
        let mut s = raw;
        loop {
            let t = s.trim_start();
            if let Some(rest) = t.strip_prefix('[') {
                if let Some(end) = rest.find(']') {
                    s = &rest[end + 1..];
                    continue;
                }
            }
            break;
        }
        let line = s.trim();
        if !line.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// One lyrics lookup's result, whatever published it.
#[derive(Debug, Clone, Default)]
pub struct FoundLyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub word: Option<WordLyrics>,
}

impl FoundLyrics {
    pub fn is_empty(&self) -> bool {
        self.synced.is_none() && self.plain.is_none() && self.word.is_none()
    }

    /// Derives the forms the source left out: an LRC from word timings, and plain
    /// text from either. A service that publishes only timings still ends up with
    /// something to write into the lyrics tag.
    pub fn filled(mut self) -> Self {
        if self.synced.is_none() {
            self.synced = self.word.as_ref().and_then(|w| w.to_lrc());
        }
        if self.plain.is_none() {
            self.plain = self.word.as_ref().and_then(|w| w.to_plain()).or_else(|| {
                self.synced
                    .as_deref()
                    .map(strip_lrc_timestamps)
                    .filter(|p| !p.is_empty())
            });
        }
        self
    }

    /// Whether anything with timings was found. A service that publishes only a
    /// plain lyric sheet leaves nothing an `.lrc` can carry, so the anonymous
    /// fallbacks still have work to do — treating "some lyrics" as "done" is what
    /// made "Save synced lyrics" produce no file for those tracks.
    pub fn has_timed(&self) -> bool {
        self.synced.is_some() || self.word.is_some()
    }

    /// Keeps whichever source supplied each form, so a service's own plain sheet
    /// survives a fallback that only carried timings, and vice versa.
    pub fn merge(mut self, other: FoundLyrics) -> Self {
        if self.synced.is_none() {
            self.synced = other.synced;
        }
        if self.word.is_none() {
            self.word = other.word;
        }
        if self.plain.is_none() {
            self.plain = other.plain;
        }
        self.filled()
    }

    pub fn sidecars(&self, title: Option<&str>, artist: Option<&str>) -> LyricsSidecars {
        match self.word.as_ref() {
            Some(w) => {
                let mut sc = LyricsSidecars::from_word_lyrics(w, title, artist);
                if self.synced.is_some() {
                    sc.lrc = self.synced.clone();
                }
                sc
            }
            None => self
                .synced
                .as_deref()
                .map(|lrc| LyricsSidecars::from_lrc(lrc, title, artist))
                .unwrap_or_default(),
        }
    }
}

/// Deezer stamps a sync line either as a ready-made `[mm:ss.xx]` string or as a
/// bare millisecond offset, and which one it uses varies by track.
fn deezer_sync_line_to_lrc(l: &serde_json::Value) -> Option<String> {
    let line = l["line"].as_str().unwrap_or("");
    if let Some(ts) = l["lrc_timestamp"].as_str() {
        if !ts.is_empty() {
            return Some(format!("{}{}", ts, line));
        }
    }
    let ms = l["milliseconds"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| l["milliseconds"].as_u64())
        .or_else(|| l["milliseconds"].as_f64().map(|f| f as u64))
        .or_else(|| l["lrc_timestamp"].as_u64())
        .or_else(|| l["lrc_timestamp"].as_f64().map(|f| f as u64))?;
    Some(format!("{}{}", lrc_stamp(ms), line))
}

/// Reads the `LYRICS_TEXT` / `LYRICS_SYNC_JSON` pair out of a gw-light
/// `lyrics.getLyrics` payload.
pub fn parse_deezer_gw_lyrics(payload: &serde_json::Value) -> FoundLyrics {
    let has_error = payload["error"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if has_error {
        return FoundLyrics::default();
    }
    let results = &payload["results"];
    let synced = results["LYRICS_SYNC_JSON"].as_array().and_then(|arr| {
        let lines: Vec<String> = arr.iter().filter_map(deezer_sync_line_to_lrc).collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    });
    FoundLyrics {
        synced,
        plain: results["LYRICS_TEXT"]
            .as_str()
            .map(str::to_string)
            .filter(|t| !t.trim().is_empty()),
        word: None,
    }
}

/// Finds the track in Deezer's catalogue by name, then reads its lyrics. The
/// word-by-word endpoint authenticates anonymously, so this still returns synced
/// lyrics for an account that has none of its own; an ARL, when present, also
/// unlocks the gw-light text.
pub async fn fetch_deezer_lyrics_by_search(
    title: &str,
    artist: &str,
    arl: &str,
) -> Option<FoundLyrics> {
    if title.trim().is_empty() {
        return None;
    }
    let http = crate::http_client::ua_client(crate::http_client::UA_MOZILLA).ok()?;
    let query = format!("{} {}", title, artist);
    let search_url = format!(
        "https://api.deezer.com/search?q={}&limit=5",
        url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>(),
    );
    let body: serde_json::Value = http.get(&search_url).send().await.ok()?.json().await.ok()?;
    let track_id = body["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|t| t["id"].as_u64())
        .map(|id| id.to_string())?;

    let client = lyric_deezer_client(arl).await?;

    let found = if arl.is_empty() {
        FoundLyrics {
            word: client.fetch_word_lyrics(&track_id).await,
            ..FoundLyrics::default()
        }
    } else {
        let (word, gw) = tokio::join!(
            client.fetch_word_lyrics(&track_id),
            client.get_lyrics(&track_id)
        );
        let mut found = gw
            .ok()
            .map(|p| parse_deezer_gw_lyrics(&p))
            .unwrap_or_default();
        found.word = word;
        found
    };

    let found = found.filled();
    (!found.is_empty()).then_some(found)
}

/// The authenticated Deezer client the lyric fallback reuses for the whole run.
///
/// Every track whose own service published nothing reaches this path, and building a
/// client plus a full `getUserData` handshake per track meant a 50-track playlist paid
/// fifty handshakes for lyrics alone, with no connection reuse between them. Keyed on
/// the ARL so changing it in Settings still takes effect, and the lock is held across
/// the handshake so a burst of tracks produces one, not one each.
/// The ARL the cached client was built for, beside the client itself.
type CachedDeezer = tokio::sync::Mutex<Option<(String, Arc<DeezerClient>)>>;

static LYRIC_DEEZER: LazyLock<CachedDeezer> = LazyLock::new(|| tokio::sync::Mutex::new(None));

pub(crate) async fn lyric_deezer_client(arl: &str) -> Option<Arc<DeezerClient>> {
    let mut slot = LYRIC_DEEZER.lock().await;
    if let Some((cached_arl, client)) = slot.as_ref() {
        if cached_arl == arl {
            return Some(Arc::clone(client));
        }
    }
    let client = Arc::new(DeezerClient::new(arl).ok()?);
    if !arl.is_empty() {
        client.authenticate().await.ok()?;
    }
    *slot = Some((arl.to_string(), Arc::clone(&client)));
    Some(client)
}

/// Whether anything at all will be done with lyrics for this download. Guards the
/// lookups so a user with both toggles off never pays for the requests.
pub fn lyrics_wanted(settings: &crate::defaults::Settings) -> bool {
    settings.save_lrc_files || settings.embed_lyrics
}

/// Where the player looks for lyrics, in order. Not configurable, and it does not
/// need to be: whether each source exists at all is already decided by the settings
/// that write it — "Save synced lyrics alongside each track", "Embed lyrics in the
/// audio file", and Deezer's public-fallback switch. A source that was never written
/// simply misses. Local first is always right: it is faster, works offline, and is
/// exactly what the user asked to be saved.
pub const SOURCE_ORDER: &[&str] = &["sidecar", "tags", "service", "community"];

/// The `.lrc`/`.ttml` MediaHarbor wrote next to the track. Reading it back is what
/// makes a downloaded track show its lyrics offline — before this, the player
/// discarded the sidecar and searched the internet by title and artist instead.
pub async fn sidecar_lyrics(audio_path: &std::path::Path) -> Option<FoundLyrics> {
    for ext in ["lrc", "ttml"] {
        let path = audio_path.with_extension(ext);
        let Ok(body) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        if body.trim().is_empty() {
            continue;
        }
        let found = if ext == "ttml" {
            let (synced, plain, _) =
                crate::services::apple_music::playback::parse_ttml_to_lrc(&body);
            FoundLyrics {
                word: synced.as_deref().and_then(WordLyrics::from_lrc),
                synced,
                plain,
            }
        } else {
            FoundLyrics {
                word: WordLyrics::from_lrc(&body),
                synced: Some(body),
                plain: None,
            }
        }
        .filled();
        if !found.is_empty() {
            return Some(found);
        }
    }
    None
}

/// The lyrics tag inside the file itself, which "Embed lyrics in the audio file"
/// wrote at download time.
pub async fn embedded_lyrics(audio_path: &std::path::Path) -> Option<FoundLyrics> {
    let path = audio_path.to_path_buf();
    let read = move || -> Option<String> {
        use lofty::file::TaggedFileExt;
        use lofty::tag::ItemKey;
        let tagged = lofty::read_from_path(&path).ok()?;
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
        tag.get_string(ItemKey::Lyrics)
            .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
            .map(str::to_string)
            .filter(|t| !t.trim().is_empty())
    };
    let text = tokio::task::spawn_blocking(read).await.ok().flatten()?;

    let looks_synced = text.lines().any(|l| l.trim_start().starts_with('['));
    let found = if looks_synced {
        FoundLyrics {
            word: WordLyrics::from_lrc(&text),
            synced: Some(text),
            plain: None,
        }
    } else {
        FoundLyrics {
            plain: Some(text),
            ..Default::default()
        }
    }
    .filled();
    (!found.is_empty()).then_some(found)
}

/// Sources that belong to no account, tried when the service a track came from
/// published nothing for it — or publishes lyrics only to paying subscribers.
/// Which account-less lyric sources the user has left switched on. Each one applies to
/// every service, not just the one it is named after.
#[derive(Debug, Clone, Copy)]
pub struct FallbackSources {
    pub deezer: bool,
    pub lrclib: bool,
}

impl FallbackSources {
    pub fn from_settings(settings: &crate::defaults::Settings) -> Self {
        Self {
            deezer: settings.deezer_lrc_public_fallback,
            lrclib: settings.lyrics_fallback_lrclib,
        }
    }

    pub fn any(self) -> bool {
        self.deezer || self.lrclib
    }
}

pub async fn fetch_fallback_lyrics(
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
    arl: &str,
    sources: FallbackSources,
) -> Option<FoundLyrics> {
    if sources.deezer {
        if let Some(found) = fetch_deezer_lyrics_by_search(title, artist, arl).await {
            return Some(found);
        }
    }
    if !sources.lrclib {
        return None;
    }
    let lrclib = fetch_lrclib(title, artist, duration_secs).await?;
    let found = FoundLyrics {
        synced: lrclib.synced,
        plain: lrclib.plain,
        word: None,
    }
    .filled();
    (!found.is_empty()).then_some(found)
}

/// Resolves the lyrics for one track: the service's own source first, then the
/// anonymous fallbacks whenever that source gave nothing an `.lrc` could carry.
///
/// Every service routes through here so the rule cannot drift between them, and so
/// a track that ends up with no sidecar always says why instead of failing silently.
pub async fn resolve_track_lyrics(
    native: FoundLyrics,
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
    settings: &crate::defaults::Settings,
    on_log: impl Fn(String),
) -> FoundLyrics {
    if !lyrics_wanted(settings) {
        return native;
    }

    let wants_word = settings.save_lrc_files && prefers_ttml(settings) && native.word.is_none();
    if native.has_timed() && !wants_word {
        return native;
    }

    let sources = FallbackSources::from_settings(settings);
    let fallback = if sources.any() {
        fetch_fallback_lyrics(title, artist, duration_secs, &settings.deezer_arl, sources)
            .await
            .unwrap_or_default()
    } else {
        FoundLyrics::default()
    };

    let had_timed = native.has_timed();
    let merged = native.merge(fallback);

    if settings.save_lrc_files && !merged.has_timed() {
        on_log(if merged.plain.is_some() {
            "  · lyrics for this track are unsynced everywhere we looked, so there is \
             nothing an .lrc can carry"
                .to_string()
        } else if sources.any() {
            "  · no lyrics published for this track".to_string()
        } else {
            "  · this service published no lyrics, and every fallback source is \
             switched off under General → Lyrics"
                .to_string()
        });
    } else if !had_timed && merged.has_timed() {
        on_log("  · service lyrics were unsynced; used community timings".to_string());
    }

    merged
}

/// The lyric files written next to a downloaded track.
#[derive(Debug, Clone, Default)]
pub struct LyricsSidecars {
    pub lrc: Option<String>,
    pub ttml: Option<String>,
    /// Whether the TTML carries per-word timings. A TTML derived from an LRC only
    /// has line timings, so it holds nothing the `.lrc` does not — which is what
    /// decides whether "Prefer TTML" is worth honouring for this track.
    pub word_timed: bool,
}

impl LyricsSidecars {
    pub fn is_empty(&self) -> bool {
        self.lrc.is_none() && self.ttml.is_none()
    }

    /// Renders both sidecars from one timed source. TTML keeps word timings when the
    /// service published them and falls back to line timings when it didn't.
    pub fn from_word_lyrics(
        lyrics: &WordLyrics,
        title: Option<&str>,
        artist: Option<&str>,
    ) -> Self {
        Self {
            lrc: lyrics.to_lrc(),
            ttml: lyrics.to_ttml(title, artist),
            word_timed: lyrics.has_word_timings(),
        }
    }

    /// For services that only publish a finished LRC; TTML is derived from it.
    pub fn from_lrc(lrc: &str, title: Option<&str>, artist: Option<&str>) -> Self {
        Self {
            lrc: (!lrc.trim().is_empty()).then(|| lrc.to_string()),
            ttml: WordLyrics::from_lrc(lrc).and_then(|w| w.to_ttml(title, artist)),
            word_timed: false,
        }
    }
}

/// Picks the one sidecar to write. TTML is only worth choosing when the service
/// published per-word timings, since that is the only thing an LRC cannot express;
/// otherwise a "Prefer TTML" request falls back to the format every player reads.
pub fn chosen_sidecar(lyrics: &LyricsSidecars, prefer_ttml: bool) -> Option<(&'static str, &str)> {
    if prefer_ttml && lyrics.word_timed {
        if let Some(ttml) = lyrics.ttml.as_deref() {
            return Some(("ttml", ttml));
        }
    }
    if let Some(lrc) = lyrics.lrc.as_deref() {
        return Some(("lrc", lrc));
    }
    lyrics.ttml.as_deref().map(|t| ("ttml", t))
}

/// Writes the sidecar the shared native settings ask for — exactly one file per
/// track, never a matching pair.
pub async fn write_sidecars(
    dest_path: &std::path::Path,
    lyrics: &LyricsSidecars,
    settings: &crate::defaults::Settings,
    on_log: impl Fn(String),
) {
    if !settings.save_lrc_files {
        return;
    }
    write_chosen_sidecar(dest_path, lyrics, prefers_ttml(settings), on_log).await;
}

/// `ttml` no longer means "write TTML instead of LRC" — it means "TTML is worth it
/// when the service gave word timings", which is the only case it carries anything
/// an LRC cannot.
pub fn prefers_ttml(settings: &crate::defaults::Settings) -> bool {
    settings.native_synced_lyrics_format.trim() == "ttml"
}

/// Writes regardless of `save_lrc_files`, for the lyrics-only download modes where
/// the sidecar is the entire deliverable.
pub async fn write_chosen_sidecar(
    dest_path: &std::path::Path,
    lyrics: &LyricsSidecars,
    prefer_ttml: bool,
    on_log: impl Fn(String),
) {
    let Some((ext, body)) = chosen_sidecar(lyrics, prefer_ttml) else {
        return;
    };
    let path = dest_path.with_extension(ext);
    if let Err(e) = tokio::fs::write(&path, body.as_bytes()).await {
        on_log(format!("  ⚠ could not write {}: {e}", path.display()));
    }
}

/// A service's lyrics lookup.
///
/// The fourth of the provider registries in [`crate::services`], alongside
/// `SearchProvider`, `DownloadProvider` and `PlaybackProvider`. Takes `&BackendState`
/// because the arms need very different things from it — a live librespot token, the
/// Apple Music session, an authenticated Tidal client — and none of that is worth
/// flattening into a context struct for one call.
#[async_trait::async_trait]
pub trait LyricsProvider: Send + Sync {
    async fn fetch(
        &self,
        req: &crate::ipc_contract::GetLyricsRequest,
        settings: &crate::defaults::Settings,
        state: &crate::BackendState,
    ) -> crate::ipc_contract::GetLyricsResponse;
}

/// Nothing found. Every provider returns this rather than an error: a track simply
/// having no lyrics is the common case, not a failure.
pub fn empty_response() -> crate::ipc_contract::GetLyricsResponse {
    crate::ipc_contract::GetLyricsResponse {
        synced: None,
        plain: None,
        word_synced: None,
    }
}

pub fn found_to_response(found: FoundLyrics) -> crate::ipc_contract::GetLyricsResponse {
    crate::ipc_contract::GetLyricsResponse {
        word_synced: found.word.as_ref().and_then(|l| l.to_json()),
        synced: found.synced,
        plain: found.plain,
    }
}

/// Deezer's word-timed lyrics without an ARL, for the public-fallback path.
pub async fn fetch_public_word_lyrics(track_id: &str) -> Option<WordLyrics> {
    let client = DeezerClient::new("").ok()?;
    client.fetch_word_lyrics(track_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug that made "Save synced lyrics" produce nothing: a service that
    /// publishes an unsynced sheet made `found` non-empty, the fallback chain was
    /// skipped, and `sidecars()` then had no timings to write — silently.
    #[test]
    fn a_plain_only_source_is_not_treated_as_a_finished_lookup() {
        let plain_only = FoundLyrics {
            plain: Some("just the words".into()),
            ..Default::default()
        };
        assert!(!plain_only.is_empty());
        assert!(!plain_only.has_timed());
        assert!(plain_only.sidecars(None, None).is_empty());
    }

    /// Merging keeps the service's own words and takes the timings from wherever
    /// they were found, rather than letting one source discard the other.
    #[test]
    fn merging_keeps_the_words_and_gains_the_timings() {
        let native = FoundLyrics {
            plain: Some("service words".into()),
            ..Default::default()
        };
        let fallback = FoundLyrics {
            synced: Some("[00:01.00]community line".into()),
            plain: Some("community words".into()),
            ..Default::default()
        };
        let merged = native.merge(fallback);
        assert!(merged.has_timed());
        assert_eq!(merged.plain.as_deref(), Some("service words"));
        assert!(!merged.sidecars(None, None).is_empty());
    }

    #[test]
    fn prefer_ttml_only_wins_when_the_ttml_carries_word_timings() {
        let word_timed = LyricsSidecars {
            lrc: Some("[00:01.00]Line".into()),
            ttml: Some("<tt/>".into()),
            word_timed: true,
        };
        assert_eq!(chosen_sidecar(&word_timed, true).unwrap().0, "ttml");
        assert_eq!(chosen_sidecar(&word_timed, false).unwrap().0, "lrc");

        let line_only = LyricsSidecars {
            word_timed: false,
            ..word_timed.clone()
        };
        assert_eq!(chosen_sidecar(&line_only, true).unwrap().0, "lrc");
    }

    #[test]
    fn exactly_one_sidecar_is_ever_chosen() {
        let both = LyricsSidecars {
            lrc: Some("[00:01.00]Line".into()),
            ttml: Some("<tt/>".into()),
            word_timed: true,
        };
        assert!(chosen_sidecar(&both, true).is_some());
        assert!(chosen_sidecar(&LyricsSidecars::default(), true).is_none());
    }

    #[test]
    fn a_ttml_only_source_still_produces_a_file() {
        let ttml_only = LyricsSidecars {
            lrc: None,
            ttml: Some("<tt/>".into()),
            word_timed: false,
        };
        assert_eq!(chosen_sidecar(&ttml_only, false).unwrap().0, "ttml");
    }

    #[tokio::test]
    async fn a_sidecar_next_to_the_track_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("01. Artist - Title.m4a");
        std::fs::write(&audio, b"not really audio").unwrap();
        std::fs::write(
            audio.with_extension("lrc"),
            "[00:01.50]Hello world\n[00:03.00]Second line",
        )
        .unwrap();

        let found = sidecar_lyrics(&audio).await.expect("sidecar not read");
        assert!(found.synced.unwrap().contains("Hello world"));
        assert_eq!(found.plain.as_deref(), Some("Hello world\nSecond line"));
    }

    #[tokio::test]
    async fn a_track_with_no_sidecar_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("bare.m4a");
        std::fs::write(&audio, b"nope").unwrap();
        assert!(sidecar_lyrics(&audio).await.is_none());
    }

    #[test]
    fn deezer_sync_lines_parse_from_a_stamp_or_from_milliseconds() {
        let payload = serde_json::json!({
            "results": {
                "LYRICS_TEXT": "Hello world\nSecond line",
                "LYRICS_SYNC_JSON": [
                    { "lrc_timestamp": "[00:01.50]", "line": "Hello world" },
                    { "milliseconds": "3000", "line": "Second line" },
                ]
            }
        });
        let found = parse_deezer_gw_lyrics(&payload);
        assert_eq!(
            found.synced.as_deref(),
            Some("[00:01.50]Hello world\n[00:03.00]Second line")
        );
        assert_eq!(found.plain.as_deref(), Some("Hello world\nSecond line"));
    }

    #[test]
    fn a_deezer_error_payload_yields_nothing() {
        let payload = serde_json::json!({ "error": ["VALID_TOKEN_REQUIRED"], "results": {} });
        assert!(parse_deezer_gw_lyrics(&payload).is_empty());
    }

    #[test]
    fn plain_text_is_derived_when_only_timings_were_published() {
        let found = FoundLyrics {
            synced: Some("[00:01.50]Hello world\n[00:03.00]Second line".into()),
            plain: None,
            word: None,
        }
        .filled();
        assert_eq!(found.plain.as_deref(), Some("Hello world\nSecond line"));
    }

    #[test]
    fn word_timings_fill_in_both_other_forms() {
        let found = FoundLyrics {
            word: Some(sample()),
            ..Default::default()
        }
        .filled();
        assert!(found.synced.is_some(), "no LRC derived from word timings");
        assert!(
            found.plain.is_some(),
            "no plain text derived from word timings"
        );

        let sidecars = found.sidecars(Some("Title"), Some("Artist"));
        assert!(sidecars.lrc.is_some());
        assert!(sidecars.ttml.is_some());
    }

    #[test]
    fn a_service_lrc_wins_over_the_one_derived_from_word_timings() {
        let found = FoundLyrics {
            synced: Some("[00:09.99]Authoritative".into()),
            plain: None,
            word: Some(sample()),
        };
        let sidecars = found.sidecars(None, None);
        assert_eq!(sidecars.lrc.as_deref(), Some("[00:09.99]Authoritative"));
        assert!(sidecars.ttml.is_some(), "word timings still drive the TTML");
    }

    #[test]
    fn an_empty_result_stays_empty_after_filling() {
        assert!(FoundLyrics::default().filled().is_empty());
    }

    fn sample() -> WordLyrics {
        WordLyrics {
            lines: vec![
                WordSyncedLine {
                    start_time: 1.5,
                    end_time: 3.0,
                    text: "Hello world".into(),
                    words: vec![
                        WordTiming {
                            start: 1.5,
                            end: 2.0,
                            text: "Hello".into(),
                        },
                        WordTiming {
                            start: 2.1,
                            end: 3.0,
                            text: "world".into(),
                        },
                    ],
                },
                WordSyncedLine {
                    start_time: 10.0,
                    end_time: 12.0,
                    text: "Rock & roll <live>".into(),
                    words: vec![],
                },
            ],
        }
    }

    #[test]
    fn lrc_stamps_and_gap_marker() {
        let lrc = sample().to_lrc().unwrap();
        let lines: Vec<&str> = lrc.lines().collect();
        assert_eq!(lines[0], "[00:01.50]Hello world");
        assert_eq!(lines[1], "[00:03.00]♪");
        assert_eq!(lines[2], "[00:10.00]Rock & roll <live>");
    }

    #[test]
    fn ttml_escapes_and_marks_word_timing() {
        let ttml = sample().to_ttml(Some("A & B"), Some("Artist")).unwrap();
        assert!(ttml.contains("itunes:timing=\"Word\""));
        assert!(ttml.contains("<ttm:title>A &amp; B</ttm:title>"));
        assert!(ttml.contains("<span begin=\"00:00:01.500\" end=\"00:00:02.000\">Hello</span>"));
        assert!(ttml.contains("Rock &amp; roll &lt;live&gt;"));
    }

    /// The generated TTML used `ttm:` without declaring the prefix, so every file
    /// was rejected by a real parser. The old assertions were substring checks,
    /// which is exactly why nothing noticed.
    #[test]
    fn ttml_resolves_every_namespace_prefix_it_uses() {
        use quick_xml::events::Event;
        use quick_xml::name::ResolveResult;

        let ttml = sample().to_ttml(Some("A & B"), Some("Artist")).unwrap();
        let mut reader = quick_xml::NsReader::from_str(&ttml);
        let mut saw_ttm = false;
        loop {
            match reader.read_resolved_event() {
                Ok((_, Event::Eof)) => break,
                Ok((ns, Event::Start(e))) | Ok((ns, Event::Empty(e))) => {
                    assert!(
                        !matches!(ns, ResolveResult::Unknown(_)),
                        "undeclared prefix on <{}>",
                        e.name().into_inner()
                    );
                    if e.name().into_inner().starts_with("ttm:") {
                        saw_ttm = true;
                    }
                }
                Ok(_) => {}
                Err(e) => panic!("generated TTML is not well-formed XML: {e}"),
            }
        }
        assert!(
            saw_ttm,
            "expected the metadata elements that need the prefix"
        );
    }

    #[test]
    fn ttml_falls_back_to_line_timing() {
        let lyrics = WordLyrics {
            lines: vec![WordSyncedLine {
                start_time: 0.0,
                end_time: 1.0,
                text: "Only a line".into(),
                words: vec![],
            }],
        };
        let ttml = lyrics.to_ttml(None, None).unwrap();
        assert!(ttml.contains("itunes:timing=\"Line\""));
        assert!(!ttml.contains("<span"));
    }

    #[test]
    fn json_matches_player_shape() {
        let json = sample().to_json().unwrap();
        assert!(json.contains("\"startTime\":1.5"));
        assert!(json.contains("\"endTime\":3.0"));
        assert!(json.contains("\"words\":[{\"start\":1.5,\"end\":2.0,\"text\":\"Hello\"}"));
    }

    #[test]
    fn lrc_round_trips_into_line_timed_ttml() {
        let lrc = "[00:01.50]First line\n[00:10.00]Second line";
        let recovered = WordLyrics::from_lrc(lrc).expect("parsed");
        assert_eq!(recovered.lines.len(), 2);
        assert_eq!(recovered.lines[0].start_time, 1.5);
        assert_eq!(recovered.lines[0].end_time, 10.0);
        assert_eq!(recovered.to_lrc().unwrap(), lrc);

        let sidecars = LyricsSidecars::from_lrc(lrc, Some("T"), Some("A"));
        assert!(sidecars.ttml.unwrap().contains("itunes:timing=\"Line\""));
    }

    #[test]
    fn lrc_with_repeated_stamps_expands_each_one() {
        let recovered = WordLyrics::from_lrc("[00:01.00][00:30.00]Chorus").expect("parsed");
        assert_eq!(recovered.lines.len(), 2);
        assert_eq!(recovered.lines[1].start_time, 30.0);
    }

    #[test]
    fn lrc_metadata_headers_are_ignored() {
        assert!(WordLyrics::from_lrc("[ar:Someone]\n[ti:Song]").is_none());
    }

    #[test]
    fn empty_renders_nothing() {
        let empty = WordLyrics::default();
        assert!(empty.to_json().is_none());
        assert!(empty.to_lrc().is_none());
        assert!(empty.to_ttml(None, None).is_none());
        assert!(empty.to_plain().is_none());
    }
}
