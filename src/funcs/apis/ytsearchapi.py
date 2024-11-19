import argparse
import json
import sys
import os
import requests

class YouTubeSearch:
    BASE_URL = 'https://www.googleapis.com/youtube/v3/search'

    def __init__(self):
        self.api_key = self._load_api_key()

    def _load_api_key(self):
        try:
            # First check if apis.json exists in the current directory
            if os.path.exists('apis.json'):
                config_path = 'apis.json'
            else:
                # Try to find it in the script's directory
                script_dir = os.path.dirname(os.path.abspath(__file__))
                config_path = os.path.join(script_dir, 'apis.json')

                if not os.path.exists(config_path):
                    raise FileNotFoundError("apis.json not found")

            with open(config_path, 'r') as f:
                config = json.load(f)

            if 'YOUTUBE_API_KEY' not in config:
                raise KeyError("YOUTUBE_API_KEY not found in apis.json")

            return config['YOUTUBE_API_KEY']

        except Exception as e:
            print(json.dumps({'error': f"Failed to load API key: {str(e)}"}, indent=2))
            sys.exit(1)

    def _make_request(self, params):
        params['key'] = self.api_key
        try:
            response = requests.get(self.BASE_URL, params=params)
            response.raise_for_status()
            return response.json()
        except requests.exceptions.RequestException as e:
            return {'error': str(e)}

    def search_videos(self, query, max_results=5):
        params = {
            'q': query,
            'part': 'snippet',
            'type': 'video',
            'maxResults': max_results
        }
        response = self._make_request(params)
        if 'error' in response:
            return {'error': response['error']}

        results = []
        for item in response.get('items', []):
            result = {
                'Thumbnail': item['snippet']['thumbnails']['medium']['url'],
                'Video Title': item['snippet']['title'],
                'Channel Title': item['snippet']['channelTitle'],
                'Video URL': f"https://www.youtube.com/watch?v={item['id']['videoId']}"
            }
            results.append(result)
        return results

    def search_playlists(self, query, max_results=5):
        params = {
            'q': query,
            'part': 'snippet',
            'type': 'playlist',
            'maxResults': max_results
        }
        response = self._make_request(params)
        if 'error' in response:
            return {'error': response['error']}

        results = []
        for item in response.get('items', []):
            result = {
                'Thumbnail': item['snippet']['thumbnails']['medium']['url'],
                'Playlist Title': item['snippet']['title'],
                'Channel Title': item['snippet']['channelTitle'],
                'Playlist URL': f"https://www.youtube.com/playlist?list={item['id']['playlistId']}"
            }
            results.append(result)
        return results

    def search_channels(self, query, max_results=5):
        params = {
            'q': query,
            'part': 'snippet',
            'type': 'channel',
            'maxResults': max_results
        }
        response = self._make_request(params)
        if 'error' in response:
            return {'error': response['error']}

        results = []
        for item in response.get('items', []):
            result = {
                'Thumbnail': item['snippet']['thumbnails']['medium']['url'],
                'Channel Title': item['snippet']['title'],
                'Channel ID': item['snippet']['channelId'],
                'Channel URL': f"https://www.youtube.com/channel/{item['snippet']['channelId']}"
            }
            results.append(result)
        return results

def main():
    parser = argparse.ArgumentParser(description='YouTube Search API')
    parser.add_argument('-q', '--query', required=True, help='Search query')
    parser.add_argument('-t', '--type', choices=['video', 'playlist', 'channel'],
                        default='video', help='Type of search')
    parser.add_argument('-m', '--max-results', type=int, default=10,
                        help='Maximum number of results')

    args = parser.parse_args()

    yt_search = YouTubeSearch()

    try:
        if args.type == 'video':
            results = yt_search.search_videos(args.query, args.max_results)
        elif args.type == 'playlist':
            results = yt_search.search_playlists(args.query, args.max_results)
        elif args.type == 'channel':
            results = yt_search.search_channels(args.query, args.max_results)

        print(json.dumps(results, indent=2))

    except Exception as e:
        print(json.dumps({'error': str(e)}, indent=2))
        sys.exit(1)

if __name__ == "__main__":
    main()
