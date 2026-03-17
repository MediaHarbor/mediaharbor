async function searchAppleMusic(term, mediaType) {
    try {
        let entity;
        if (mediaType === "track") {
            entity = "song";
        } else if (mediaType === "album") {
            entity = "album";
        } else if (mediaType === "artist") {
            entity = "musicArtist";
        } else if (mediaType === "playlist") {
            entity = "musicPlaylist";
        } else if (mediaType === "musicvideo") {
            entity = "musicVideo";
        } else {
            throw new Error(`Media type '${mediaType}' is not supported.`);
        }

        const url = `https://itunes.apple.com/search?term=${encodeURIComponent(term)}&media=music&entity=${entity}&limit=50`;
        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`Request failed: ${response.statusText}`);
        }
        const data = await response.json();
        let results = data.results || [];

        if (mediaType === "artist") {
            results = await Promise.all(results.map(async (artist) => {
                if (!artist.artistLinkUrl) return artist;
                try {
                    const resp = await fetch(artist.artistLinkUrl, {
                        headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36' }
                    });
                    const html = await resp.text();
                    const match = html.match(/<meta property="og:image"\s+content="([^"]+)"/);
                    if (match?.[1]) artist.artworkUrl100 = match[1];
                } catch (_) {}
                return artist;
            }));
        }

        return results;
    } catch (e) {
        throw new Error(`An error occurred: ${e.message}`);
    }
}

async function getArtistAlbums(artistId) {
    try {
        const url = `https://itunes.apple.com/lookup?id=${artistId}&entity=album&limit=50`;
        const response = await fetch(url);
        const data = await response.json();
        return (data.results || []).filter(item => item.wrapperType === 'collection');
    } catch (e) {
        return [];
    }
}

async function getAlbumTracks(albumId) {
    try {
        const url = `https://itunes.apple.com/lookup?id=${albumId}&entity=song`;
        const response = await fetch(url);
        const data = await response.json();
        const results = data.results || [];
        const album = results.find(item => item.wrapperType === 'collection') || {};
        const tracks = results.filter(item => item.wrapperType === 'track' && item.kind === 'song');
        return { album, tracks };
    } catch (e) {
        return { album: {}, tracks: [] };
    }
}

module.exports = { searchAppleMusic, getArtistAlbums, getAlbumTracks };
