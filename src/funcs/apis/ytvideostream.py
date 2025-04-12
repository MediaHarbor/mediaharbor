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
            'format': '22/bestvideo+bestaudio/best',  # Prioritize format 22, then fallback to best combined
            'quiet': True,  # Enable output for debugging
            'verbose': False,  # More verbose output to see what's happening
            'no_warnings': False,  # Show warnings
            'youtube_include_dash_manifest': True,  # Include DASH manifests
            'extractor_args': {
                'youtube': {
                    'player_client': ['android', 'web'],  # Try different clients
                    'skip': ['hls', 'dash']  # Skip HLS and DASH formats if causing issues
                }
            }
        }
        
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info_dict = ydl.extract_info(youtube_url, download=False)
            
            # Try to get a direct playable URL from the formats
            if 'formats' in info_dict:
                # First try to find format 22 (usually 720p with audio)
                for format in info_dict['formats']:
                    if format.get('format_id') == '22' and 'url' in format:
                        return format['url']
                
                # If format 22 not found, try any format with both video and audio
                for format in info_dict['formats']:
                    if ('vcodec' in format and format['vcodec'] != 'none' and 
                        'acodec' in format and format['acodec'] != 'none' and
                        'url' in format):
                        return format['url']
                        
                # As a last resort, try any format with a URL
                for format in info_dict['formats']:
                    if 'url' in format:
                        return format['url']
            
            # If direct URL is in the info_dict
            if 'url' in info_dict:
                return info_dict['url']
                
            # If no valid URL is found
            return "No valid stream URL found"
    except Exception as e:
        print(f"Error fetching combined stream URL: {e}")
        return f"Error: {str(e)}"

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
