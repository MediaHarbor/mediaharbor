const https = require("https");

const APPLE_MUSIC_URL_RE = /https:\/\/(?:classical\.)?music\.apple\.com\/([a-z]{2})\/(artist|album|playlist|song|music-video|post)\/[^/]*(?:\/([^/?]*))?(?:\?i=)?([0-9a-z]*)?/;

function parseAppleMusicUrl(url) {
    const match = url.match(APPLE_MUSIC_URL_RE);
    if (!match) return null;

    const storefront = match[1];
    const type = match[2];
    const primaryId = match[3];
    const subId = match[4];

    const id = subId || primaryId;
    const resolvedType = subId ? "song" : type;

    return { storefront, type: resolvedType, id };
}

function itunesLookup(id, country = "us") {
    return new Promise((resolve, reject) => {
        const url = `https://itunes.apple.com/lookup?id=${encodeURIComponent(id)}&country=${encodeURIComponent(country)}&entity=song`;
        const req = https.get(url, { headers: { "User-Agent": "MediaHarbor/1.0" } }, (res) => {
            if ([301, 302, 307, 308].includes(res.statusCode)) {
                https.get(res.headers.location, (res2) => {
                    let data = "";
                    res2.on("data", (chunk) => data += chunk);
                    res2.on("end", () => {
                        try { resolve(JSON.parse(data)); } catch (e) { reject(e); }
                    });
                }).on("error", reject);
                return;
            }
            let data = "";
            res.on("data", (chunk) => data += chunk);
            res.on("end", () => {
                try { resolve(JSON.parse(data)); } catch (e) { reject(e); }
            });
        });
        req.on("error", reject);
        req.setTimeout(10000, () => { req.destroy(); reject(new Error("iTunes API timeout")); });
    });
}

function scaleArtworkUrl(artworkUrl, size = 1200) {
    if (!artworkUrl) return null;
    return artworkUrl.replace(/\d+x\d+(bb|cc|sr)/, `${size}x${size}$1`);
}

async function resolveMetadata(url) {
    try {
        const parsed = parseAppleMusicUrl(url);
        if (!parsed || !parsed.id || !parsed.id.match(/^\d+$/)) {
            return null;
        }

        const data = await itunesLookup(parsed.id, parsed.storefront);
        if (!data || !data.results || data.results.length === 0) return null;

        const results = data.results;

        if (parsed.type === "song") {
            const song = results.find(r => r.wrapperType === "track" && String(r.trackId) === String(parsed.id))
                || results.find(r => r.wrapperType === "track");
            if (song) {
                return {
                    title: song.trackName,
                    artist: song.artistName,
                    album: song.collectionName,
                    coverUrl: scaleArtworkUrl(song.artworkUrl100)
                };
            }
        }

        if (parsed.type === "album") {
            const album = results.find(r => r.wrapperType === "collection")
                || results[0];
            if (album) {
                return {
                    title: album.collectionName,
                    artist: album.artistName,
                    album: album.collectionName,
                    coverUrl: scaleArtworkUrl(album.artworkUrl100)
                };
            }
        }

        const first = results[0];
        if (first) {
            return {
                title: first.trackName || first.collectionName || first.artistName || null,
                artist: first.artistName || null,
                album: first.collectionName || null,
                coverUrl: scaleArtworkUrl(first.artworkUrl100)
            };
        }

        return null;
    } catch (err) {
        return null;
    }
}

module.exports = { parseAppleMusicUrl, resolveMetadata, scaleArtworkUrl };
