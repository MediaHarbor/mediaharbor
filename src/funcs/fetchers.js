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

        // Fetch the title from the <title> tag only
        let title = $('title').text().trim();

        // Trim the title if it's too long
        title = title.length > 50 ? title.slice(0, 50)+'…' : title;

        return title;
    } catch (error) {
        console.error('Error fetching title:', error);
        return 'Unknown Title';  // Fallback if there's an error
    }
}
function extractDomain(url) {
    try {
        const domain = new URL(url).hostname;
        return domain.startsWith('www.') ? domain.slice(4) : domain;  // Remove 'www.' if present
    } catch (error) {
        console.error('Invalid URL:', error);
        return url;  // Fallback to the full URL if invalid
    }
}
async function fetchHighResImageOrFavicon(url) {
    try {
        const response = await axios.get(url);
        const html = response.data;
        const $ = cheerio.load(html);

        // Look for Open Graph image first
        let ogImage = $('meta[property="og:image"]').attr('content');

        // If no OG image, fallback to high-res favicons or Apple touch icons
        let favicon = $('link[rel="apple-touch-icon"]').attr('href') ||
            $('link[rel="icon"][sizes]').attr('href') ||  // Look for any icon with sizes attribute
            $('link[rel="icon"]').attr('href') ||  // Fallback to normal favicon
            '/favicon.ico';  // Fallback to default favicon

        // If we found an OG image, return that
        let image = ogImage || favicon;

        // If the image URL is relative, make it absolute by combining with base URL
        if (!image.startsWith('http')) {
            const baseUrl = new URL(url).origin;
            image = `${baseUrl}${image}`;
        }

        return image;
    } catch (error) {
        console.error('Error fetching image:', error);
        return '/favicon.ico';  // Fallback to generic favicon
    }
}

module.exports = {fetchWebsiteTitle, extractDomain, fetchHighResImageOrFavicon};