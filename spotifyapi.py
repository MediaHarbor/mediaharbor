import datetime
import json
import argparse
import requests
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
            with open('apis.json', 'r') as f:
                credentials = json.load(f)
            self.client_id = credentials.get('SPOTIFY_CLIENT_ID')
            self.client_secret = credentials.get('SPOTIFY_CLIENT_SECRET')
        except FileNotFoundError:
            self.client_id = os.environ.get('SPOTIFY_CLIENT_ID')
            self.client_secret = os.environ.get('SPOTIFY_CLIENT_SECRET')

        if not self.client_id or not self.client_secret:
            raise ValueError("Client ID and Client Secret must be provided in apis.json or as environment variables.")

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
        return self.make_request("GET", f"{self.API_URL}/tracks/{track_id}")

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
            "type": "show",  # Spotify uses 'show' type for podcasts
            "limit": limit,
            "market": "US"  # Adding market parameter
        }
        return self.make_request("GET", f"{self.API_URL}/search", params=params)



def main():
    parser = argparse.ArgumentParser(description="Spotify API script")
    parser.add_argument('--get-track', help='Track ID to get details for')
    parser.add_argument('--get-album', help='Album ID to get details for')
    parser.add_argument('--search-track', help='Track name to search for')
    parser.add_argument('--search-album', help='Album name to search for')
    parser.add_argument('--search-playlist', help='Playlist name to search for')
    parser.add_argument('--search-episode', help='Episode name to search for')
    parser.add_argument('--search-artist', help='Artist name to search for')
    parser.add_argument('--search-podcast', help='Podcast name to search for')

    args = parser.parse_args()

    spotify_api = SpotifyAPI()

    try:
        if args.get_track:
            track_details = spotify_api.get_track(track_id=args.get_track)
            print(json.dumps(track_details, indent=4))

        if args.get_album:
            album_details = spotify_api.get_album(album_id=args.get_album)
            print(json.dumps(album_details, indent=4))

        if args.search_track:
            search_results = spotify_api.search_tracks(query=args.search_track)
            print(json.dumps(search_results, indent=4))

        if args.search_album:
            search_results = spotify_api.search_albums(query=args.search_album)
            print(json.dumps(search_results, indent=4))

        if args.search_playlist:
            search_results = spotify_api.search_playlists(query=args.search_playlist)
            print(json.dumps(search_results, indent=4))

        if args.search_episode:
            search_results = spotify_api.search_episodes(query=args.search_episode)
            print(json.dumps(search_results, indent=4))

        if args.search_artist:
            search_results = spotify_api.search_artists(query=args.search_artist)
            print(json.dumps(search_results, indent=4))

        if args.search_podcast:
            search_results = spotify_api.search_podcasts(query=args.search_podcast)
            print(json.dumps(search_results, indent=4))

    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        if hasattr(e, 'response') and e.response is not None:
            print(f"Response status code: {e.response.status_code}")
            print(f"Response content: {e.response.content}")

if __name__ == "__main__":
    main()
