async function getTrack(trackId) {
    const response = await fetch(`https://api.deezer.com/track/${trackId}`);
    return await response.json();
}

const DEEZER_TYPES = { tracks: 'track', albums: 'album', artists: 'artist', playlists: 'playlist', podcasts: 'podcast', episodes: 'episode' };

async function _search(type, query) {
    const res = await fetch(`https://api.deezer.com/search/${DEEZER_TYPES[type]}?q=${encodeURIComponent(query)}`);
    const data = await res.json();
    return Array.isArray(data) ? data : data.data ?? [];
}

async function searchTracks(query) { return _search('tracks', query); }
async function searchAlbums(query) { return _search('albums', query); }
async function searchArtists(query) { return _search('artists', query); }
async function searchPlaylists(query) { return _search('playlists', query); }
async function searchPodcasts(query) { return _search('podcasts', query); }
async function searchEpisodes(query) { return _search('episodes', query); }

async function getArtistAlbums(artistId) {
    const res = await fetch(`https://api.deezer.com/artist/${artistId}/albums?limit=50`);
    const data = await res.json();
    return Array.isArray(data) ? data : (data.data ?? []);
}

async function getTrackList(item) {
    if (!item.includes('/')) {
        throw new Error("Invalid format. Use 'album/ID' or 'playlist/ID'.");
    }

    const [itemType, itemId] = item.split('/');
    if (!['album', 'playlist'].includes(itemType)) {
        throw new Error("Invalid item type. Must be 'album' or 'playlist'.");
    }

    const detailsResponse = await fetch(`https://api.deezer.com/${itemType}/${itemId}`);
    if (!detailsResponse.ok) {
        return { error: `Failed to fetch details for ${itemType} ID ${itemId}` };
    }

    const itemDetails = await detailsResponse.json();

    const trackResponse = await fetch(`https://api.deezer.com/${itemType}/${itemId}/tracks`);
    if (!trackResponse.ok) {
        return { error: `Failed to fetch tracks for ${itemType} ID ${itemId}` };
    }

    const trackData = await trackResponse.json();
    const trackList = trackData.data ?? [];

    const metadata = {
        type: itemType,
        id: itemId,
        name: itemDetails.title || "Unknown Title",
        artist: itemType === "album" ? (itemDetails.artist?.name || "Unknown Artist") : "N/A",
        release_date: itemDetails.release_date || "Unknown Date",
        total_tracks: trackList.length,
        cover_xl: itemDetails.cover_xl || itemDetails.picture_xl || "",
        md5_image: itemDetails.md5_image || "",
    };
    metadata.tracks = trackList;

    return metadata;
}

module.exports = { getTrack, searchTracks, searchAlbums, searchArtists, searchPlaylists, searchPodcasts, searchEpisodes, getTrackList, getArtistAlbums };
