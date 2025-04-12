import argparse
import isodate
import json
import sys
import os
import requests

class YouTubeSearch:
    BASE_URL = 'https://www.googleapis.com/youtube/v3'

    def __init__(self):
        self.api_key = self._load_api_key()

    def _load_api_key(self):
        try:
            if os.path.exists('apis.json'):
                config_path = 'apis.json'
            else:
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

    def _make_request(self, endpoint, params):
        params['key'] = self.api_key
        try:
            response = requests.get(f"{self.BASE_URL}/{endpoint}", params=params)
            response.raise_for_status()
            return response.json()
        except requests.exceptions.RequestException as e:
            return {'error': str(e)}

    def search_videos(self, query, max_results=5):
        params = {
            'q': query,
            'part': 'snippet',
            'type': 'video',  # Ensure we're searching for videos
            'maxResults': max_results
        }
        response = self._make_request('search', params)
        if 'error' in response:
            return {'error': response['error']}

        results = []
        for item in response.get('items', []):
            # Check if the item is a video (should have 'videoId' in 'id')
            if 'videoId' not in item['id']:
                # If it's not a video, skip it or handle it differently
                continue

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
        response = self._make_request('search', params)
        if 'error' in response:
            return {'error': response['error']}

        results = []
        for item in response.get('items', []):
            result = {
                'Thumbnail': item['snippet']['thumbnails']['medium']['url'],
                'Playlist Title': item['snippet']['title'],
                'Channel Title': item['snippet']['channelTitle'],
                'Playlist URL': f"https://www.youtube.com/playlist?list={item['id']['playlistId']}",
                'BrowseId': item['id']['playlistId']  # Adding BrowseId for playlists
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
        response = self._make_request('search', params)
        if 'error' in response:
            return {'error': response['error']}

        results = []
        for item in response.get('items', []):
            result = {
                'Thumbnail': item['snippet']['thumbnails']['medium']['url'],
                'Channel Title': item['snippet']['title'],
                'Channel ID': item['snippet']['channelId'],
                'Channel URL': f"https://www.youtube.com/channel/{item['snippet']['channelId']}",
                'BrowseId': item['snippet']['channelId']  # Adding BrowseId for channels
            }
            results.append(result)
        return results


    def get_playlist_details(self, playlist_id):
        params = {
            'part': 'snippet,contentDetails',
            'id': playlist_id
        }
        response = self._make_request('playlists', params)
        if 'error' in response:
            return {'error': response['error']}

        if not response.get('items'):
            return {'error': 'Playlist not found'}

        item = response['items'][0]
        playlist_details = {
            'title': item['snippet']['title'],
            'artist': item['snippet']['channelTitle'],
            'releaseDate': item['snippet']['publishedAt'],
            'coverUrl': item['snippet']['thumbnails']['medium']['url'],
            'description': item['snippet']['description'],
            'duration': item['contentDetails']['itemCount']
        }
        return playlist_details

    def _get_duration_seconds(self, iso_duration):
        try:
            duration = isodate.parse_duration(iso_duration)
            return int(duration.total_seconds())
        except Exception:
            return 0

    def get_playlist_tracks(self, playlist_id):
        playlist_params = {
            'part': 'snippet,contentDetails',
            'playlistId': playlist_id,
            'maxResults': 50
        }
        video_ids = []
        tracks = []

        while True:
            response = self._make_request('playlistItems', playlist_params)
            if 'error' in response:
                return {'error': response['error']}

            for item in response.get('items', []):
                video_ids.append(item['contentDetails']['videoId'])
                snippet = item.get('snippet', {})
                thumbnails = snippet.get('thumbnails', {})

                cover_url = (
                    thumbnails.get('medium', {}).get('url') or
                    thumbnails.get('default', {}).get('url') or
                    None
                )

                tracks.append({
                    'id': item['contentDetails']['videoId'],
                    'number': len(tracks) + 1,
                    'title': snippet.get('title', 'Unknown'),
                    'artist': snippet.get('videoOwnerChannelTitle', 'Unknown'),
                    'playUrl': f"https://www.youtube.com/watch?v={item['contentDetails']['videoId']}",
                    'coverUrl': cover_url
                })

            if 'nextPageToken' in response:
                playlist_params['pageToken'] = response['nextPageToken']
            else:
                break

        for i in range(0, len(video_ids), 50):
            video_params = {
                'part': 'contentDetails',
                'id': ','.join(video_ids[i:i+50])
            }
            video_response = self._make_request('videos', video_params)
            if 'error' in video_response:
                return {'error': video_response['error']}

            for idx, video_item in enumerate(video_response.get('items', [])):
                duration_str = video_item.get('contentDetails', {}).get('duration', 'PT0S')
                duration_seconds = self._get_duration_seconds(duration_str)
                tracks[i + idx]['duration'] = duration_seconds

        return tracks

def main():
    parser = argparse.ArgumentParser(description='YouTube Search API with Playlist Details')
    parser.add_argument('-q', '--query', help='Search query')
    parser.add_argument('-t', '--type', choices=['video', 'playlist', 'channel'],
                        default='video', help='Type of search')
    parser.add_argument('-m', '--max-results', type=int, default=10,
                        help='Maximum number of results')
    parser.add_argument('--get-track-list', metavar='playlist/ID',
                        help='Get track list and details of a playlist by ID')

    args = parser.parse_args()
    yt_search = YouTubeSearch()

    try:
        if args.get_track_list:
            playlist_id = args.get_track_list.split('/')[-1]
            playlist_details = yt_search.get_playlist_details(playlist_id)
            if 'error' in playlist_details:
                print(json.dumps(playlist_details, indent=2))
                sys.exit(1)
            tracks = yt_search.get_playlist_tracks(playlist_id)
            if 'error' in tracks:
                print(json.dumps(tracks, indent=2))
                sys.exit(1)
            result = {
                'Playlist': playlist_details,
                'Tracks': tracks
            }
            print(json.dumps(result, indent=2))
        else:
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
