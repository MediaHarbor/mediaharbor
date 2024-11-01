import argparse
import yt_dlp

def get_stream_url(youtube_url):
    """
    Get the best quality stream URL for a YouTube video.

    Parameters:
        youtube_url (str): The full YouTube video URL.

    Returns:
        str: The best quality stream URL.
    """
    try:
        ydl_opts = {
            'format': 'bestaudio',  # Get the best quality audio
            'quiet': True  # Suppress output
        }
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info_dict = ydl.extract_info(youtube_url, download=False)
            for format in info_dict['formats']:
                if format['ext'] == 'm4a':  # Prefer m4a for audio
                    return format['url']
            return info_dict['formats'][0]['url'] if info_dict['formats'] else "N/A"
    except Exception as e:
        print(f"Error getting stream URL: {e}")
        return "N/A"

if __name__ == "__main__":
    # Set up argument parser
    parser = argparse.ArgumentParser(description="Get the best quality stream URL from a YouTube video URL.")
    parser.add_argument('--url', required=True, help="The full YouTube video URL")

    # Parse the arguments
    args = parser.parse_args()

    youtube_url = args.url

    # Fetch the stream URL
    stream_url = get_stream_url(youtube_url)

    # Print the result
    print(stream_url)
