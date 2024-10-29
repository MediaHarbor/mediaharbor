import argparse
import json
from googleapiclient.discovery import build

# Set up the YouTube Data API client
API_KEY = 'AIzaSyAa8RX-ZL8XbYco39ymM4q3alDx2lqRXTY'
YOUTUBE_API_SERVICE_NAME = 'youtube'
YOUTUBE_API_VERSION = 'v3'

def youtube_search(query, search_type="video"):
    """
    Search YouTube for videos, playlists, or channels.

    Parameters:
        query (str): The search query.
        search_type (str): The type of content to search for.
                           Can be 'video', 'playlist', or 'channel'.

    Returns:
        list: A list of search results based on the search_type.
    """
    youtube = build(YOUTUBE_API_SERVICE_NAME, YOUTUBE_API_VERSION, developerKey=API_KEY)

    # Map search types to YouTube Data API 'type' parameter
    valid_search_types = ['video', 'playlist', 'channel']

    if search_type not in valid_search_types:
        raise ValueError(f"Invalid search type: {search_type}. Valid types are {valid_search_types}.")

    # Perform the search
    search_response = youtube.search().list(
        q=query,
        type=search_type,
        part='snippet',
        maxResults=10  # You can change this to a higher value if needed
    ).execute()

    results = []

    # Process search results based on search type
    for item in search_response.get('items', []):
        if search_type == 'video':
            results.append({
                "Video Title": item['snippet']['title'],
                "Video ID": item['id']['videoId'],
                "Video URL": f"https://www.youtube.com/watch?v={item['id']['videoId']}",
                "Channel Title": item['snippet']['channelTitle'],
                "Thumbnail": item['snippet']['thumbnails']['high']['url']
            })
        elif search_type == 'playlist':
            results.append({
                "Playlist Title": item['snippet']['title'],
                "Playlist ID": item['id']['playlistId'],
                "Playlist URL": f"https://www.youtube.com/playlist?list={item['id']['playlistId']}",
                "Channel Title": item['snippet']['channelTitle'],
                "Thumbnail": item['snippet']['thumbnails']['high']['url']
            })
        elif search_type == 'channel':
            results.append({
                "Channel Title": item['snippet']['title'],
                "Channel ID": item['id']['channelId'],
                "Channel URL": f"https://www.youtube.com/channel/{item['id']['channelId']}",
                "Thumbnail": item['snippet']['thumbnails']['high']['url']
            })

    return results


if __name__ == "__main__":
    # Set up argument parser
    parser = argparse.ArgumentParser(description="Search YouTube for videos, playlists, or channels.")
    parser.add_argument('-q', '--query', required=True, help="The search query")
    parser.add_argument('-t', '--type', choices=['video', 'playlist', 'channel'], default='video',
                        help="The type of content to search for: video, playlist, or channel (default: video)")

    # Parse the arguments
    args = parser.parse_args()

    search_query = args.query
    search_type = args.type

    try:
        # Perform the search with the given arguments
        results = youtube_search(search_query, search_type)

        if results:
            # Output results as formatted JSON
            print(json.dumps(results, indent=4))
        else:
            print("No results found.")
    except ValueError as e:
        print(e)
