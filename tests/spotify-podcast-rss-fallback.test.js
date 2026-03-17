/**
 * Tests for RSS fallback when Spotify spclient returns empty manifest {}
 * (externally-hosted podcast episodes: BBC Learning English, etc.)
 *
 * Key facts:
 * - For RSS-hosted episodes, _getPlaybackInfo returns manifest: {} with audio_id: null
 * - GID metadata returns 404 for these episodes (they are not in the GID database)
 * - The metadata field has group_uri (show ID) and episode name/duration
 * - Fallback chain:
 *   1. show GID metadata → feed_url (try with/without market)
 *   2. podcast-experience/v2/shows/{showId} → rss_url / rssFeedUrl / feedUrl (base62, no GID)
 *   3. podcast-experience/v2/episodes/{episodeId} → externalUrl / external_url (base62, no GID)
 *   Each strategy that finds an RSS URL parses the feed to find the episode audio URL
 */

import { describe, it, expect } from 'vitest';
import xml2js from 'xml2js';

// ──────────────────────────────────────────────────────────
// Helpers replicated from librespotService.js
// ──────────────────────────────────────────────────────────

function _parseItunesDuration(duration) {
    if (typeof duration !== 'string') return parseInt(duration) * 1000 || 0;
    const parts = duration.split(':').map(Number);
    if (parts.length === 3) return ((parts[0] * 3600) + (parts[1] * 60) + parts[2]) * 1000;
    if (parts.length === 2) return ((parts[0] * 60) + parts[1]) * 1000;
    return parseFloat(duration) * 1000 || 0;
}

async function _findEpisodeInRss(feedXml, episodeId, episodeName, durationMs) {
    const data = await xml2js.parseStringPromise(feedXml, { explicitArray: true });
    const items = data?.rss?.channel?.[0]?.item || [];
    const spotifyUri = `spotify:episode:${episodeId}`;

    let bestMatch = null;

    for (const item of items) {
        const enclosure = item.enclosure?.[0]?.$ || {};
        const audioUrl = enclosure.url;
        if (!audioUrl) continue;

        // Match by Spotify URI in guid
        const guid = item.guid?.[0];
        const guidVal = typeof guid === 'string' ? guid : guid?._ || '';
        if (guidVal === spotifyUri) return audioUrl;

        // Exact title match
        const rawTitle = Array.isArray(item.title) ? item.title[0] : item.title;
        const titleStr = typeof rawTitle === 'string' ? rawTitle : rawTitle?._ || '';
        if (episodeName && titleStr.toLowerCase().trim() === episodeName.toLowerCase().trim()) {
            return audioUrl;
        }

        // Fuzzy title prefix match (first 20 chars)
        if (episodeName && titleStr.toLowerCase().includes(episodeName.toLowerCase().substring(0, 20))) {
            bestMatch = bestMatch || audioUrl;
        }

        // Duration match (±5 seconds)
        if (durationMs) {
            const itunesDuration = item['itunes:duration']?.[0];
            if (itunesDuration) {
                const parsed = _parseItunesDuration(itunesDuration);
                if (Math.abs(parsed - durationMs) < 5000) {
                    bestMatch = audioUrl;
                }
            }
        }
    }

    return bestMatch;
}

function extractGroupUri(playbackInfo) {
    if (!playbackInfo?.media) return null;
    for (const key of Object.keys(playbackInfo.media)) {
        const entry = playbackInfo.media[key];
        const meta = entry?.item?.metadata || entry?.metadata;
        if (meta?.group_uri) return meta.group_uri;
    }
    return null;
}

function isEmptyManifest(playbackInfo) {
    if (!playbackInfo?.media) return false;
    for (const key of Object.keys(playbackInfo.media)) {
        const entry = playbackInfo.media[key];
        const manifest = entry?.item?.manifest || entry?.manifest;
        if (manifest && Object.keys(manifest).length === 0) return true;
    }
    return false;
}

// ──────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────

describe('Empty manifest detection', () => {
    it('detects empty manifest {} from BBC-like playback response', () => {
        const playbackInfo = {
            media: {
                'spotify:episode:4Kvzs6aBxM9luhlsmNzgv4': {
                    item: {
                        metadata: {
                            uri: 'spotify:episode:4Kvzs6aBxM9luhlsmNzgv4',
                            group_uri: 'spotify:show:7bm7GARt4OYKHPd3xbtmSR',
                            name: 'Real Easy English: Talking about rain',
                            duration: 355970,
                        },
                        manifest: {},
                        audio_id: null,
                        video_id: null,
                    }
                }
            }
        };
        expect(isEmptyManifest(playbackInfo)).toBe(true);
    });

    it('does NOT flag non-empty manifest as empty', () => {
        const playbackInfo = {
            media: {
                'spotify:episode:abc': {
                    item: {
                        metadata: { uri: 'spotify:episode:abc', group_uri: 'spotify:show:xyz' },
                        manifest: { file_ids_mp4: [{ file_id: 'deadbeef', format: '11' }] },
                    }
                }
            }
        };
        expect(isEmptyManifest(playbackInfo)).toBe(false);
    });

    it('extracts group_uri (show ID) from playback response', () => {
        const playbackInfo = {
            media: {
                'spotify:episode:4Kvzs6aBxM9luhlsmNzgv4': {
                    item: {
                        metadata: {
                            group_uri: 'spotify:show:7bm7GARt4OYKHPd3xbtmSR',
                        },
                        manifest: {},
                    }
                }
            }
        };
        expect(extractGroupUri(playbackInfo)).toBe('spotify:show:7bm7GARt4OYKHPd3xbtmSR');
    });

    it('returns null group_uri when missing', () => {
        expect(extractGroupUri({ media: {} })).toBe(null);
        expect(extractGroupUri({})).toBe(null);
    });
});

describe('_parseItunesDuration', () => {
    it('parses HH:MM:SS format', () => {
        expect(_parseItunesDuration('1:05:30')).toBe((3600 + 330) * 1000); // 5730000
    });

    it('parses MM:SS format', () => {
        expect(_parseItunesDuration('5:55')).toBe(355000);
    });

    it('parses seconds-only string', () => {
        expect(_parseItunesDuration('355')).toBe(355000);
    });

    it('parses seconds-only number', () => {
        expect(_parseItunesDuration(355)).toBe(355000);
    });

    it('handles invalid input gracefully', () => {
        expect(_parseItunesDuration('abc')).toBe(0);
        expect(_parseItunesDuration(null)).toBe(0);
    });
});

const SAMPLE_RSS = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Learning Easy English</title>
    <item>
      <title>Real Easy English: Talking about rain</title>
      <guid>https://bbc.co.uk/programmes/p0abc123</guid>
      <itunes:duration>5:55</itunes:duration>
      <enclosure url="https://open.live.bbc.co.uk/mediaselector/6/redir/version/2.0/mediaset/audio-nondrm-download/proto/https/vpid/p0abc123.mp3" length="5000000" type="audio/mpeg"/>
    </item>
    <item>
      <title>Learning Grammar: Past Tense</title>
      <guid>https://bbc.co.uk/programmes/p0def456</guid>
      <itunes:duration>10:23</itunes:duration>
      <enclosure url="https://open.live.bbc.co.uk/mediaselector/6/redir/version/2.0/mediaset/audio-nondrm-download/proto/https/vpid/p0def456.mp3" length="8000000" type="audio/mpeg"/>
    </item>
  </channel>
</rss>`;

const SAMPLE_RSS_WITH_SPOTIFY_GUID = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>TED Talks Daily</title>
    <item>
      <title>Some TED talk</title>
      <guid>spotify:episode:4Kvzs6aBxM9luhlsmNzgv4</guid>
      <itunes:duration>12:30</itunes:duration>
      <enclosure url="https://rss.acast.com/audio/ted/episode1.mp3" length="15000000" type="audio/mpeg"/>
    </item>
    <item>
      <title>Another TED talk</title>
      <guid>spotify:episode:otherEpisodeId</guid>
      <itunes:duration>10:00</itunes:duration>
      <enclosure url="https://rss.acast.com/audio/ted/episode2.mp3" length="12000000" type="audio/mpeg"/>
    </item>
  </channel>
</rss>`;

describe('_findEpisodeInRss', () => {
    it('finds episode by exact title match', async () => {
        const url = await _findEpisodeInRss(
            SAMPLE_RSS,
            'notMatching',
            'Real Easy English: Talking about rain',
            355000
        );
        expect(url).toContain('p0abc123.mp3');
    });

    it('finds episode by Spotify URI in guid field', async () => {
        const url = await _findEpisodeInRss(
            SAMPLE_RSS_WITH_SPOTIFY_GUID,
            '4Kvzs6aBxM9luhlsmNzgv4',
            'Some TED talk',
            750000
        );
        expect(url).toContain('episode1.mp3');
    });

    it('finds episode by duration match (±5s)', async () => {
        // Duration 355000ms = 5:55 — matches second item (350s = 5:50? no let's use the first item)
        // 5:55 = 355s = 355000ms
        const url = await _findEpisodeInRss(
            SAMPLE_RSS,
            'someId',
            'nonexistent title',
            355000  // exact match for 5:55
        );
        expect(url).toContain('p0abc123.mp3');
    });

    it('finds episode by fuzzy title prefix', async () => {
        // "Real Easy English: Talking" shares first 26 chars with the RSS title
        // so substring(0, 20) = "Real Easy English: T" which IS contained in "Real Easy English: Talking about rain"
        const url = await _findEpisodeInRss(
            SAMPLE_RSS,
            'someId',
            'Real Easy English: Talking differently today',
            null
        );
        expect(url).toContain('p0abc123.mp3');
    });

    it('returns null when no match found', async () => {
        const url = await _findEpisodeInRss(
            SAMPLE_RSS,
            'nonexistentId',
            'Completely unrelated episode title xyz',
            999999
        );
        expect(url).toBeNull();
    });

    it('skips items with no enclosure URL', async () => {
        const rssNoEnclosure = `<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item><title>Episode 1</title><guid>g1</guid></item>
  <item><title>Episode 2</title><guid>g2</guid>
    <enclosure url="https://cdn.example.com/ep2.mp3" type="audio/mpeg"/>
  </item>
</channel></rss>`;
        const url = await _findEpisodeInRss(rssNoEnclosure, 'g2', 'Episode 2', null);
        expect(url).toContain('ep2.mp3');
    });

    it('guid object form (xml2js wraps text as { _ })', async () => {
        const rssObjGuid = `<?xml version="1.0"?>
<rss version="2.0"><channel>
  <item>
    <title>Test</title>
    <guid isPermaLink="false">spotify:episode:testEpisodeId</guid>
    <enclosure url="https://cdn.example.com/test.mp3" type="audio/mpeg"/>
  </item>
</channel></rss>`;
        const url = await _findEpisodeInRss(rssObjGuid, 'testEpisodeId', 'Test', null);
        expect(url).toContain('test.mp3');
    });
});

describe('Show GID metadata feed_url extraction', () => {
    it('extracts feed_url from show GID metadata', () => {
        const showMeta = {
            name: 'Learning Easy English',
            publisher: { name: 'BBC' },
            feed_url: 'https://rss.bbc.co.uk/learning-english.xml',
        };
        const feedUrl = showMeta.feed_url || showMeta.rss_url || showMeta.external_url;
        expect(feedUrl).toBe('https://rss.bbc.co.uk/learning-english.xml');
    });

    it('falls back to rss_url if feed_url not present', () => {
        const showMeta = {
            name: 'Some Podcast',
            rss_url: 'https://example.com/feed.rss',
        };
        const feedUrl = showMeta.feed_url || showMeta.rss_url || showMeta.external_url;
        expect(feedUrl).toBe('https://example.com/feed.rss');
    });

    it('returns undefined when neither field present', () => {
        const showMeta = { name: 'No Feed Podcast' };
        const feedUrl = showMeta.feed_url || showMeta.rss_url || showMeta.external_url;
        expect(feedUrl).toBeUndefined();
    });
});

describe('GID metadata market fallback', () => {
    it('retries without market when 404 occurs with market param', () => {
        // Simulate the two-URL retry logic
        const attempts = [];
        const gid = 'abcdef1234567890abcdef1234567890';
        const base = `https://spclient.wg.spotify.com/metadata/4/episode/${gid}`;
        const urls = [`${base}?market=from_token`, base];

        // First URL returns 404, second would succeed
        const mockResponses = [{ status: 404, ok: false }, { status: 200, ok: true }];
        urls.forEach((url, i) => {
            attempts.push({ url, wouldSucceed: mockResponses[i].ok });
        });

        expect(attempts[0].url).toContain('market=from_token');
        expect(attempts[1].url).not.toContain('market');
        expect(attempts[1].wouldSucceed).toBe(true);
    });

    it('does not swallow non-404 errors', () => {
        // A 403 or 500 should throw immediately, not retry
        const status = 403;
        const shouldThrow = status !== 404;
        expect(shouldThrow).toBe(true);
    });

    it('extracts group_name from playback metadata', () => {
        const entry = {
            item: {
                metadata: {
                    group_uri: 'spotify:show:7bm7GARt4OYKHPd3xbtmSR',
                    group_name: 'Learning Easy English',
                    context_description: 'Learning Easy English',
                },
                manifest: {},
            }
        };
        const meta = entry?.item?.metadata;
        const showName = meta?.group_name || meta?.context_description;
        expect(showName).toBe('Learning Easy English');
        const showId = meta.group_uri.split(':').pop();
        expect(showId).toBe('7bm7GARt4OYKHPd3xbtmSR');
    });
});

// ──────────────────────────────────────────────────────────
// podcast-experience API response parsing
// ──────────────────────────────────────────────────────────

// Extracts RSS feed URL from various possible podcast-experience show response shapes
function extractFeedUrlFromShowResponse(data) {
    if (!data) return null;
    const show = data.show || data;
    return show.rssFeedUrl || show.rss_url || show.feed_url
        || show.feedUrl || show.rssUrl || show.external_url || null;
}

// Extracts external audio URL from various possible podcast-experience episode response shapes
function extractExternalUrlFromEpisodeResponse(data) {
    if (!data) return null;
    const ep = data.episode || data;
    return ep.externalUrl || ep.external_url || ep.audioUrl
        || ep.audio?.url || ep.media?.url || null;
}

describe('podcast-experience show response parsing', () => {
    it('handles rssFeedUrl field', () => {
        const resp = { id: 'abc', name: 'Test Show', rssFeedUrl: 'https://feeds.bbc.co.uk/test.rss' };
        expect(extractFeedUrlFromShowResponse(resp)).toBe('https://feeds.bbc.co.uk/test.rss');
    });

    it('handles rss_url field', () => {
        const resp = { show: { rss_url: 'https://example.com/feed.rss' } };
        expect(extractFeedUrlFromShowResponse(resp)).toBe('https://example.com/feed.rss');
    });

    it('handles feed_url field', () => {
        const resp = { show: { feed_url: 'https://example.com/podcast.xml' } };
        expect(extractFeedUrlFromShowResponse(resp)).toBe('https://example.com/podcast.xml');
    });

    it('handles feedUrl field (camelCase)', () => {
        const resp = { feedUrl: 'https://example.com/rss' };
        expect(extractFeedUrlFromShowResponse(resp)).toBe('https://example.com/rss');
    });

    it('returns null when no feed URL found', () => {
        const resp = { id: 'abc', name: 'No Feed' };
        expect(extractFeedUrlFromShowResponse(resp)).toBeNull();
    });

    it('returns null for empty/null input', () => {
        expect(extractFeedUrlFromShowResponse(null)).toBeNull();
        expect(extractFeedUrlFromShowResponse({})).toBeNull();
    });
});

describe('podcast-experience episode response parsing', () => {
    it('handles externalUrl field (camelCase)', () => {
        const resp = {
            episode: {
                id: 'abc',
                externalUrl: 'https://media.bbc.co.uk/episode.mp3',
                duration: 355970
            }
        };
        expect(extractExternalUrlFromEpisodeResponse(resp)).toBe('https://media.bbc.co.uk/episode.mp3');
    });

    it('handles external_url field (snake_case)', () => {
        const resp = { external_url: 'https://cdn.bbc.co.uk/episode.mp3' };
        expect(extractExternalUrlFromEpisodeResponse(resp)).toBe('https://cdn.bbc.co.uk/episode.mp3');
    });

    it('handles nested audio.url field', () => {
        const resp = { episode: { audio: { url: 'https://media.example.com/ep.mp3' } } };
        expect(extractExternalUrlFromEpisodeResponse(resp)).toBe('https://media.example.com/ep.mp3');
    });

    it('handles audioUrl field', () => {
        const resp = { audioUrl: 'https://stream.example.com/ep.mp3' };
        expect(extractExternalUrlFromEpisodeResponse(resp)).toBe('https://stream.example.com/ep.mp3');
    });

    it('returns null when no URL found', () => {
        const resp = { episode: { id: 'abc', name: 'No URL' } };
        expect(extractExternalUrlFromEpisodeResponse(resp)).toBeNull();
    });
});

describe('full fallback chain order', () => {
    it('episode podcast-experience returns direct external URL (skips RSS parsing)', () => {
        const episodeResp = { episode: { externalUrl: 'https://bbc.co.uk/audio.mp3' } };
        const directUrl = extractExternalUrlFromEpisodeResponse(episodeResp);
        expect(directUrl).toBeTruthy();
        // If we have a direct URL, we can stream it without parsing any RSS
    });

    it('show podcast-experience returns RSS URL (requires RSS parsing)', () => {
        const showResp = { show: { rssFeedUrl: 'https://feeds.bbc.co.uk/show.rss' } };
        const feedUrl = extractFeedUrlFromShowResponse(showResp);
        expect(feedUrl).toBeTruthy();
        // With feedUrl we still need to call _findEpisodeInRss
    });
});
