import argparse
import yt_dlp

def get_combined_stream_url(youtube_url):
    """
    Get a combined audio and video stream URL for a YouTube video.

    Parameters:
        youtube_url (str): The full YouTube video URL.

    Returns:
        str: The URL of the best combined (audio + video) stream.
    """
    try:
        ydl_opts = {
            'format': '22/bestvideo+bestaudio/best',  # Prioritize format 22, then fallback to best combined video/audio
            'quiet': True  # Suppress output
        }
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info_dict = ydl.extract_info(youtube_url, download=False)

            # Try to extract the URL from the info_dict
            if 'url' in info_dict:
                return info_dict['url']  # Return the direct combined stream URL
            elif 'formats' in info_dict:
                # If formats are available, pick the first one that has both video and audio
                for format in info_dict['formats']:
                    if 'url' in format and format.get('vcodec') != 'none' and format.get('acodec') != 'none':
                        return format['url']

            # If no valid URL is found
            return "N/A"
    except Exception as e:
        print(f"Error fetching combined stream URL: {e}")
        return "N/A"

if __name__ == "__main__":
    # Set up argument parser
    parser = argparse.ArgumentParser(description="Get a combined audio + video stream URL from a YouTube video URL.")
    parser.add_argument('--url', required=True, help="The full YouTube video URL")

    # Parse the arguments
    args = parser.parse_args()

    youtube_url = args.url

    # Fetch the combined stream URL
    combined_url = get_combined_stream_url(youtube_url)

    # Print the result
    print(combined_url)
