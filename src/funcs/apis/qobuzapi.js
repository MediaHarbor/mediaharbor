const appId = "950096963";
const appSecret = "10b251c286cfbf64d6b7105f253d9a2e";
const authToken = "u6lHtzb1Vv_TbNYYL_PrIzVZfkMpxUJ4Y4AkpdrfFRaj5o1sbLP7ENCKVD-wQEmkMbQIN-G6vcgzPvwaZdEvPA";

async function _call(fn) { try { return await fn(); } catch (e) { return {}; } }

async function searchQobuz(query, searchType) {
    return _call(async () => {
        const url = `https://www.qobuz.com/api.json/0.2/${searchType}/search?app_id=${appId}&query=${encodeURIComponent(query)}&limit=10`;
        const response = await fetch(url, { headers: { "X-User-Auth-Token": authToken } });
        return await response.json();
    });
}

async function getTrackDetails(trackId) {
    return _call(async () => {
        const url = `https://www.qobuz.com/api.json/0.2/track/get?app_id=${appId}&track_id=${trackId}`;
        const response = await fetch(url, { headers: { "X-User-Auth-Token": authToken } });
        return await response.json();
    });
}

async function getTrackStream(trackId, formatId = 27) {
    return _call(async () => {
        const url = `https://www.qobuz.com/api.json/0.2/track/getFileUrl?app_id=${appId}&track_id=${trackId}&format_id=${formatId}`;
        const response = await fetch(url, { headers: { "X-User-Auth-Token": authToken } });
        return await response.json();
    });
}

async function getAlbumList(artistId) {
    return _call(async () => {
        const url = `https://www.qobuz.com/api.json/0.2/artist/get?app_id=${appId}&artist_id=${artistId}&extra=albums`;
        const response = await fetch(url, { headers: { "X-User-Auth-Token": authToken } });
        const artistData = await response.json();
        if (artistData.albums) return artistData.albums;
        return { status: "error", message: "No albums found for this artist." };
    });
}

async function getTrackList(entityId, entityType) {
    return _call(async () => {
        let url;
        if (entityType === "album") {
            url = `https://www.qobuz.com/api.json/0.2/album/get?app_id=${appId}&album_id=${entityId}`;
        } else if (entityType === "playlist") {
            url = `https://www.qobuz.com/api.json/0.2/playlist/get?app_id=${appId}&playlist_id=${entityId}&extra=tracks`;
        } else if (entityType === "artist") {
            url = `https://www.qobuz.com/api.json/0.2/artist/get?app_id=${appId}&artist_id=${entityId}&extra=albums`;
        } else {
            return { status: "error", message: `Unknown entity type: ${entityType}` };
        }

        const response = await fetch(url, { headers: { "X-User-Auth-Token": authToken } });
        const entityData = await response.json();

        if (entityType === "playlist" && entityData.tracks) return entityData;
        if (entityType === "album" && entityData.tracks) return entityData;
        if (entityType === "artist") return entityData;

        return { status: "error", message: `No tracks found for this ${entityType}.` };
    });
}

module.exports = { searchQobuz, getTrackDetails, getTrackStream, getAlbumList, getTrackList };
