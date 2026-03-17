const fs   = require('fs');
const path = require('path');

function loadBundledYouTubeKey() {
    try {
        const p = path.join(__dirname, 'apis.json');
        return JSON.parse(fs.readFileSync(p, 'utf8')).YOUTUBE_API_KEY || '';
    } catch (_) { return ''; }
}

class YouTubeSearch {
    constructor(apiKey) {
        this.BASE_URL = 'https://www.googleapis.com/youtube/v3';
        const key = apiKey || loadBundledYouTubeKey();
        if (!key) {
            throw new Error("YouTube API key not configured. Add it in Settings → API Keys.");
        }
        this.api_key = key;
    }

    async makeRequest(endpoint, params) {
        params.key = this.api_key;
        const query = new URLSearchParams(params).toString();
        const url = `${this.BASE_URL}/${endpoint}?${query}`;

        try {
            const response = await fetch(url);
            if (!response.ok) {
                throw new Error(`Request failed: ${response.statusText}`);
            }
            return await response.json();
        } catch (error) {
            throw new Error(`YouTube API request failed: ${error.message}`);
        }
    }

    async _search(type, query, maxResults) {
        const response = await this.makeRequest('search', { q: query, part: 'snippet', type, maxResults });
        return response.items || [];
    }

    async searchVideos(query, maxResults = 10) {
        const items = await this._search('video', query, maxResults);
        return items.filter(item => item.id?.videoId).map(item => ({
            id: item.id.videoId,
            title: item.snippet.title,
            channel: item.snippet.channelTitle,
            thumbnail: item.snippet.thumbnails?.medium?.url || item.snippet.thumbnails?.default?.url,
            url: `https://www.youtube.com/watch?v=${item.id.videoId}`,
            view_count: item.statistics?.viewCount ? parseInt(item.statistics.viewCount) : undefined,
        }));
    }

    async searchPlaylists(query, maxResults = 10) {
        const items = await this._search('playlist', query, maxResults);
        return items.map(item => ({
            id: item.id.playlistId,
            title: item.snippet.title,
            channel: item.snippet.channelTitle,
            thumbnail: item.snippet.thumbnails?.medium?.url || item.snippet.thumbnails?.default?.url,
            url: `https://www.youtube.com/playlist?list=${item.id.playlistId}`,
        }));
    }

    async searchChannels(query, maxResults = 10) {
        const items = await this._search('channel', query, maxResults);
        return items.map(item => ({
            id: item.id.channelId,
            title: item.snippet.title,
            channel: item.snippet.title,
            thumbnail: item.snippet.thumbnails?.medium?.url || item.snippet.thumbnails?.default?.url,
            url: `https://www.youtube.com/channel/${item.id.channelId}`,
        }));
    }

    async getChannelPlaylists(channelId, maxResults = 50) {
        const response = await this.makeRequest('playlists', {
            part: 'snippet,contentDetails',
            channelId,
            maxResults,
        });
        return (response.items || []).map(item => ({
            id: item.id,
            title: item.snippet.title,
            thumbnail: item.snippet.thumbnails?.medium?.url || item.snippet.thumbnails?.default?.url,
            url: `https://www.youtube.com/playlist?list=${item.id}`,
            trackCount: item.contentDetails?.itemCount,
        }));
    }
}

async function searchYouTube(query, searchType = 'video', apiKey) {
    const ytSearch = new YouTubeSearch(apiKey);

    switch (searchType.toLowerCase()) {
        case 'video':    return await ytSearch.searchVideos(query);
        case 'playlist': return await ytSearch.searchPlaylists(query);
        case 'channel':  return await ytSearch.searchChannels(query);
        default:         throw new Error(`Invalid search type: ${searchType}`);
    }
}

module.exports = { YouTubeSearch, searchYouTube };
