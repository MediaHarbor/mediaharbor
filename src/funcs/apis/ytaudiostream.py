import argparse
import yt_dlp

def get_stream_url(youtube_url):
    """
    Get the best quality audio stream URL for a YouTube video.

    Parameters:
        youtube_url (str): The full YouTube video URL.

    Returns:
        str: The best quality stream URL.
    """
    try:
        ydl_opts = {
            'format': 'bestaudio[ext=m4a]',  # Limit to the best audio in m4a format
            'quiet': True,  # Suppress output
            'noplaylist': True,  # Avoid fetching playlist information
            'extractor_args': {'youtube': {'skip': 'hls'}}  # Skip HLS formats
        }

        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info_dict = ydl.extract_info(youtube_url, download=False)
            return info_dict['url'] if 'url' in info_dict else "N/A"

    except Exception as e:
        print(f"Error getting stream URL: {e}")
        return "N/A"

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Get the best quality stream URL from a YouTube video URL.")
    parser.add_argument('--url', required=True, help="The full YouTube video URL")

    args = parser.parse_args()
    youtube_url = args.url

    # Fetch the stream URL
    stream_url = get_stream_url(youtube_url)
    print(stream_url)
