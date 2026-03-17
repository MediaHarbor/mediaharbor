const https = require("https");

function fetchSpotifyOEmbed(url) {
    return new Promise((resolve, reject) => {
        const oembedUrl = `https://open.spotify.com/oembed?url=${encodeURIComponent(url)}`;
        const req = https.get(oembedUrl, { headers: { "User-Agent": "MediaHarbor/1.0" } }, (res) => {
            if (res.statusCode !== 200) {
                reject(new Error(`Spotify oEmbed returned ${res.statusCode}`));
                res.resume();
                return;
            }
            let data = "";
            res.on("data", (chunk) => data += chunk);
            res.on("end", () => {
                try { resolve(JSON.parse(data)); } catch (e) { reject(e); }
            });
        });
        req.on("error", reject);
        req.setTimeout(10000, () => { req.destroy(); reject(new Error("Spotify oEmbed timeout")); });
    });
}

function fetchSpotifyPageMeta(url) {
    return new Promise((resolve, reject) => {
        const req = https.get(url, { headers: { "User-Agent": "Mozilla/5.0 (compatible; MediaHarbor/1.0)" } }, (res) => {
            if (res.statusCode !== 200) {
                reject(new Error(`Spotify page returned ${res.statusCode}`));
                res.resume();
                return;
            }
            let html = "";
            res.on("data", (chunk) => html += chunk);
            res.on("end", () => {
                const titleMatch = html.match(/<meta\s+property="og:title"\s+content="([^"]+)"/i)
                    || html.match(/<meta\s+content="([^"]+)"\s+property="og:title"/i);
                const descMatch = html.match(/<meta\s+property="og:description"\s+content="([^"]+)"/i)
                    || html.match(/<meta\s+content="([^"]+)"\s+property="og:description"/i);
                const ogTitle = titleMatch ? titleMatch[1] : null;
                const ogDesc = descMatch ? descMatch[1] : null;
                resolve({ ogTitle, ogDesc });
            });
        });
        req.on("error", reject);
        req.setTimeout(10000, () => { req.destroy(); reject(new Error("Spotify page fetch timeout")); });
    });
}

async function resolveSpotifyMetadata(url) {
    try {
        const [oembedResult, pageResult] = await Promise.allSettled([
            fetchSpotifyOEmbed(url),
            fetchSpotifyPageMeta(url),
        ]);

        const oembedData = oembedResult.status === "fulfilled" ? oembedResult.value : null;
        const pageMeta = pageResult.status === "fulfilled" ? pageResult.value : null;

        const coverUrl = oembedData?.thumbnail_url || null;

        let title = oembedData?.title || pageMeta?.ogTitle || null;
        let artist = null;
        let album = null;

        // og:description format: "Song · Artist · Album · Year"
        if (pageMeta?.ogDesc) {
            const parts = pageMeta.ogDesc.split(" \u00b7 ");
            if (parts.length >= 3) {
                artist = parts[1].trim() || null;
                album = parts[2].trim() || null;
            }
        }

        // Fallback: try parsing "Artist - Title" from oEmbed title
        if (!artist && oembedData?.type === "rich" && title && title.includes(" - ")) {
            const parts = title.split(" - ");
            if (parts.length === 2) {
                artist = parts[0].trim();
                title = parts[1].trim();
            }
        }

        return { title, artist, album, coverUrl };
    } catch (e) {
        return null;
    }
}

module.exports = { resolveSpotifyMetadata };
