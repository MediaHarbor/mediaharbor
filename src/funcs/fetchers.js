const axios = require("axios");
const cheerio = require("cheerio");

async function fetchWebsiteTitle(url) {
    try {
        const response = await axios.get(url, {
            headers: {
                'Cache-Control': 'no-cache',
                'Pragma': 'no-cache',
                'Expires': '0'
            }
        });
        const html = response.data;
        const $ = cheerio.load(html);

        let title = $('title').text().trim();

        title = title.length > 50 ? title.slice(0, 50)+'…' : title;

        return title;
    } catch (error) {
        return 'Unknown Title';
    }
}
function extractDomain(url) {
    try {
        const domain = new URL(url).hostname;
        return domain.startsWith('www.') ? domain.slice(4) : domain;
    } catch (error) {
        return url;
    }
}
async function fetchHighResImageOrFavicon(url) {
    try {
        const response = await axios.get(url);
        const html = response.data;
        const $ = cheerio.load(html);

        let ogImage = $('meta[property="og:image"]').attr('content');

        let favicon = $('link[rel="apple-touch-icon"]').attr('href') ||
            $('link[rel="icon"][sizes]').attr('href') ||
            $('link[rel="icon"]').attr('href') ||
            '/favicon.ico';

        let image = ogImage || favicon;

        if (!image.startsWith('http')) {
            const baseUrl = new URL(url).origin;
            image = `${baseUrl}${image}`;
        }

        return image;
    } catch (error) {
        return '/favicon.ico';
    }
}

module.exports = {fetchWebsiteTitle, extractDomain, fetchHighResImageOrFavicon};