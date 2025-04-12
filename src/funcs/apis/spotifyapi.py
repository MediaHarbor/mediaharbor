import sys
import datetime
import json
import argparse
import requests
import sys
import os
from base64 import b64encode
from urllib.parse import quote

class SpotifyAPI:
    API_URL = "https://api.spotify.com/v1"
    TOKEN_URL = "https://accounts.spotify.com/api/token"

    def __init__(self):
        self.session = requests.Session()
        self.access_token = None
        self.token_expiry = None
        self.load_credentials()
        self.authenticate()

    def load_credentials(self):
        try:
            if os.path.exists('apis.json'):
                config_path = 'apis.json'
            else:
                script_dir = os.path.dirname(os.path.abspath(__file__))
                config_path = os.path.join(script_dir, 'apis.json')
                if not os.path.exists(config_path):
                    raise FileNotFoundError("apis.json not found")

            with open(config_path, 'r') as f:
                credentials = json.load(f)

            if 'SPOTIFY_CLIENT_ID' not in credentials or 'SPOTIFY_CLIENT_SECRET' not in credentials:
                raise KeyError("SPOTIFY_CLIENT_ID or SPOTIFY_CLIENT_SECRET not found in apis.json")

            self.client_id = credentials['SPOTIFY_CLIENT_ID']
            self.client_secret = credentials['SPOTIFY_CLIENT_SECRET']

        except Exception as e:
            print(json.dumps({'error': f"Failed to load credentials: {str(e)}"}, indent=2))
            sys.exit(1)

    def authenticate(self):
        if self.access_token and self.token_expiry and datetime.datetime.now() < self.token_expiry:
            return
        auth_header = b64encode(f"{self.client_id}:{self.client_secret}".encode()).decode()
        headers = {
            "Authorization": f"Basic {auth_header}",
            "Content-Type": "application/x-www-form-urlencoded"
        }
        data = {"grant_type": "client_credentials"}

        response = self.session.post(self.TOKEN_URL, headers=headers, data=data)
        response.raise_for_status()

        token_data = response.json()
        self.access_token = token_data['access_token']
        self.token_expiry = datetime.datetime.now() + datetime.timedelta(seconds=token_data['expires_in'])

    def make_request(self, method, url, params=None):
        self.authenticate()
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Accept": "application/json"
        }
        response = self.session.request(method, url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def get_track(self, track_id: str):
        """
        Get details of a specific track by its ID.
        
        Args:
            track_id (str): The Spotify ID of the track
            
        Returns:
            dict: Dictionary containing track title, album art, and artist name
        """
        track_data = self.make_request("GET", f"{self.API_URL}/tracks/{track_id}")
        
        track_info = {
            "title": track_data["name"],
            "album_art": track_data["album"]["images"][0]["url"] if track_data["album"].get("images") else None,
            "artist_name": track_data["artists"][0]["name"]
        }
        
        return track_info
    def get_album_info(self, album_id: str):
        """
        Get essential details of a specific album by its ID.
        
        Args:
            album_id (str): The Spotify ID of the album
            
        Returns:
            dict: Dictionary containing album name, release date, artist name, and cover URL
        """
        album_data = self.make_request("GET", f"{self.API_URL}/albums/{album_id}")
        
        album_info = {
            "album_name": album_data["name"],
            "release_date": album_data["release_date"],
            "artist_name": album_data["artists"][0]["name"],
            "cover_url": album_data["images"][0]["url"] if album_data.get("images") else None
        }
        
        return album_info

    def get_playlist_info(self, playlist_id: str):
        """
        Get essential details of a specific playlist by its ID.
        
        Args:
            playlist_id (str): The Spotify ID of the playlist
            
        Returns:
            dict: Dictionary containing playlist name, owner name, description, and cover URL
        """
        playlist_data = self.make_request("GET", f"{self.API_URL}/playlists/{playlist_id}")
        
        playlist_info = {
            "playlist_name": playlist_data["name"],
            "owner_name": playlist_data["owner"]["display_name"],
            "description": playlist_data.get("description", ""),
            "cover_url": playlist_data["images"][0]["url"] if playlist_data.get("images") else None,
            "total_tracks": playlist_data["tracks"]["total"]
        }
        
        return playlist_info
    def get_album_tracks(self, album_id: str):
        album_data = self.get_album(album_id)
        tracks_data = self.make_request("GET", f"{self.API_URL}/albums/{album_id}/tracks")
        
        album_info = {
            "album_name": album_data["name"],
            "release_date": album_data["release_date"],
            "artist_name": album_data["artists"][0]["name"],
            "cover_url": album_data["images"][0]["url"] if album_data.get("images") else None,
            "tracks": tracks_data["items"]
        }
        
        return album_info

    def get_playlist_tracks(self, playlist_id: str):
        playlist_data = self.make_request("GET", f"{self.API_URL}/playlists/{playlist_id}")
        tracks_data = self.make_request("GET", f"{self.API_URL}/playlists/{playlist_id}/tracks")
        
        playlist_info = {
            "playlist_name": playlist_data["name"],
            "owner_name": playlist_data["owner"]["display_name"],
            "cover_url": playlist_data["images"][0]["url"] if playlist_data.get("images") else None,
            "tracks": tracks_data["items"]
        }
        
        return playlist_info

    def get_album(self, album_id: str):
        return self.make_request("GET", f"{self.API_URL}/albums/{album_id}")

    def search_tracks(self, query: str, limit: int = 10):
        params = {
            "q": query,
            "type": "track",
            "limit": limit
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)

    def search_albums(self, query: str, limit: int = 10):
        params = {
            "q": query,
            "type": "album",
            "limit": limit
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)

    def search_playlists(self, query: str, limit: int = 10):
        params = {
            "q": query,
            "type": "playlist",
            "limit": limit
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)

    def search_episodes(self, query: str, limit: int = 10):
        params = {
            "q": query,
            "type": "episode",
            "limit": limit,
            "market": "US"
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)

    def search_artists(self, query: str, limit: int = 10):
        params = {
            "q": query,
            "type": "artist",
            "limit": limit
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)

    def search_podcasts(self, query: str, limit: int = 10):
        params = {
            "q": query,
            "type": "show",
            "limit": limit,
            "market": "US"
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)

def parse_resource_id(resource_string: str):
    try:
        resource_type, resource_id = resource_string.split('/')
        if resource_type not in ['album', 'playlist']:
            raise ValueError
        return resource_type, resource_id
    except ValueError:
        raise ValueError("Format must be 'album/ID' or 'playlist/ID'")

def main():
    parser = argparse.ArgumentParser(description="Spotify API script")
    parser.add_argument('--search-track', help='Track name to search for')
    parser.add_argument('--search-album', help='Album name to search for')
    parser.add_argument('--search-playlist', help='Playlist name to search for')
    parser.add_argument('--search-episode', help='Episode name to search for')
    parser.add_argument('--search-artist', help='Artist name to search for')
    parser.add_argument('--search-podcast', help='Podcast name to search for')
    parser.add_argument('--get-track-list', help='Get tracks from album or playlist (format: album/ID or playlist/ID)')
    parser.add_argument('--get-track', help='Get track details by ID')
    parser.add_argument('--get-album-info', help='Get album details by ID')
    parser.add_argument('--get-playlist-info', help='Get playlist details by ID')
    args = parser.parse_args()

    try:
        spotify_api = SpotifyAPI()

        if args.get_track_list:
            resource_type, resource_id = parse_resource_id(args.get_track_list)
            if resource_type == 'album':
                results = spotify_api.get_album_tracks(resource_id)
            else:  # playlist
                results = spotify_api.get_playlist_tracks(resource_id)
            print(json.dumps(results, indent=2))
        elif args.search_track:
            results = spotify_api.search_tracks(query=args.search_track)
            print(json.dumps(results, indent=2))
        elif args.search_album:
            results = spotify_api.search_albums(query=args.search_album)
            print(json.dumps(results, indent=2))
        elif args.search_playlist:
            results = spotify_api.search_playlists(query=args.search_playlist)
            print(json.dumps(results, indent=2))
        elif args.search_episode:
            results = spotify_api.search_episodes(query=args.search_episode)
            print(json.dumps(results, indent=2))
        elif args.search_artist:
            results = spotify_api.search_artists(query=args.search_artist)
            print(json.dumps(results, indent=2))
        elif args.search_podcast:
            results = spotify_api.search_podcasts(query=args.search_podcast)
            print(json.dumps(results, indent=2))
        elif args.get_track:
            results = spotify_api.get_track(args.get_track)
            print(json.dumps(results, indent=2))
        elif args.get_album_info:
            results = spotify_api.get_album_info(args.get_album_info)
            print(json.dumps(results, indent=2))
        elif args.get_playlist_info:
            results = spotify_api.get_playlist_info(args.get_playlist_info)
            print(json.dumps(results, indent=2))
    except Exception as e:
        print(json.dumps({'error': str(e)}, indent=2))
        sys.exit(1)

if __name__ == "__main__":
    main()
