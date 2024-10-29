import datetime
import json
import argparse
import requests
import os
from base64 import b64encode
from urllib.parse import quote

class TidalAPI:
    API_URL = "https://openapi.tidal.com/v2"
    TOKEN_URL = "https://auth.tidal.com/v1/oauth2/token"
    STREAM_API_URL = "https://api.tidal.com/v1"  # Different base URL for stream endpoint

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
            self.client_id = credentials.get('TIDAL_CLIENT_ID')
            self.client_secret = credentials.get('TIDAL_CLIENT_SECRET')
        except FileNotFoundError:
            self.client_id = os.environ.get('TIDAL_CLIENT_ID')
            self.client_secret = os.environ.get('TIDAL_CLIENT_SECRET')

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

    def make_request(self, method, url, params=None, base_url=None):
        self.authenticate()
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Accept": "application/vnd.api+json"
        }
        full_url = f"{base_url or self.API_URL}/{url}"
        response = self.session.request(method, full_url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def get_stream_url(self, track_id: str, country_code: str, user_id: str = None, user_token: str = None):
        """Get the stream URL for a track using either client credentials or user token."""
        headers = {
            "Authorization": f"Bearer {user_token if user_token else self.access_token}"
        }

        params = {'countryCode': country_code}
        url = f"{self.STREAM_API_URL}/tracks/{track_id}/streamUrl"

        response = self.session.get(url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def get_track(self, track_id: str, country_code: str):
        params = {"countryCode": country_code, "include": "artists,albums"}
        return self.make_request("GET", f"tracks/{track_id}", params=params)

    def get_album(self, album_id: str, country_code: str):
        return self.make_request("GET", f"albums/{album_id}", params={"countryCode": country_code})

    def search_tracks(self, query: str, country_code: str, limit: int = 30):
        self.authenticate()
        url = "https://openapi.tidal.com/search"
        params = {
            "query": query,
            "type": "TRACKS",
            "offset": 0,
            "limit": limit,
            "countryCode": country_code,
            "popularity": "WORLDWIDE"
        }
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Content-Type": "application/vnd.tidal.v1+json",
            "Accept": "application/vnd.tidal.v1+json"
        }

        response = self.session.get(url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def search_albums(self, query: str, country_code: str, limit: int = 30):
        self.authenticate()
        url = "https://openapi.tidal.com/search"
        params = {
            "query": query,
            "type": "ALBUMS",
            "offset": 0,
            "limit": limit,
            "countryCode": country_code,
            "popularity": "WORLDWIDE"
        }
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Content-Type": "application/vnd.tidal.v1+json",
            "Accept": "application/vnd.tidal.v1+json"
        }

        response = self.session.get(url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def search_artists(self, query: str, country_code: str, limit: int = 30):
        self.authenticate()
        url = "https://openapi.tidal.com/search"
        params = {
            "query": query,
            "type": "ARTISTS",
            "offset": 0,
            "limit": limit,
            "countryCode": country_code,
            "popularity": "WORLDWIDE"
        }
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Content-Type": "application/vnd.tidal.v1+json",
            "Accept": "application/vnd.tidal.v1+json"
        }

        response = self.session.get(url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def search_playlists(self, query: str, country_code: str, limit: int = 30):
        self.authenticate()
        url = "https://openapi.tidal.com/search"
        params = {
            "query": query,
            "type": "PLAYLISTS",
            "offset": 0,
            "limit": limit,
            "countryCode": country_code,
            "popularity": "WORLDWIDE"
        }
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Content-Type": "application/vnd.tidal.v1+json",
            "Accept": "application/vnd.tidal.v1+json"
        }

        response = self.session.get(url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def search_videos(self, query: str, country_code: str, limit: int = 30):
        self.authenticate()
        url = "https://openapi.tidal.com/search"
        params = {
            "query": query,
            "type": "VIDEOS",
            "offset": 0,
            "limit": limit,
            "countryCode": country_code,
            "popularity": "WORLDWIDE"
        }
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Content-Type": "application/vnd.tidal.v1+json",
            "Accept": "application/vnd.tidal.v1+json"
        }

        response = self.session.get(url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

def main():
    parser = argparse.ArgumentParser(description="Tidal API script")
    parser.add_argument('--country-code', default='US', help='Country code (default: US)')
    parser.add_argument('--get-track', help='Track ID to get details for')
    parser.add_argument('--get-album', help='Album ID to get details for')
    parser.add_argument('--search-track', help='Track name to search for')
    parser.add_argument('--search-album', help='Album name to search for')
    parser.add_argument('--get-stream', help='Track ID to get stream URL for')
    parser.add_argument('--user-id', help='User ID for stream URL request')
    parser.add_argument('--user-token', help='User access token for stream URL request')
    parser.add_argument('--search-video', help='Video name to search for')
    parser.add_argument('--search-playlist', help='Playlist name to search for')
    parser.add_argument('--search-artist', help='Artist name to search for')

    args = parser.parse_args()

    tidal_api = TidalAPI()

    try:
        if args.get_track:
            track_details = tidal_api.get_track(track_id=args.get_track, country_code=args.country_code)
            print(json.dumps(track_details, indent=4))

        if args.get_album:
            album_details = tidal_api.get_album(album_id=args.get_album, country_code=args.country_code)
            print(json.dumps(album_details, indent=4))

        if args.search_track:
            search_results = tidal_api.search_tracks(query=args.search_track, country_code=args.country_code)
            print(json.dumps(search_results, indent=4))

        if args.search_album:
            search_results = tidal_api.search_albums(query=args.search_album, country_code=args.country_code)
            print(json.dumps(search_results, indent=4))

        if args.search_video:
            search_results = tidal_api.search_videos(query=args.search_video, country_code=args.country_code)
            print(json.dumps(search_results, indent=4))

        if args.search_playlist:
            search_results = tidal_api.search_playlists(query=args.search_playlist, country_code=args.country_code)
            print(json.dumps(search_results, indent=4))

        if args.search_artist:
            search_results = tidal_api.search_artists(query=args.search_artist, country_code=args.country_code)
            print(json.dumps(search_results, indent=4))

        if args.get_stream:
            stream_url_data = tidal_api.get_stream_url(
                track_id=args.get_stream,
                country_code=args.country_code,
                user_id=args.user_id,
                user_token=args.user_token
            )
            print("Stream URL Data:")
            print(json.dumps(stream_url_data, indent=4))


    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        if hasattr(e, 'response') and e.response is not None:
            print(f"Response status code: {e.response.status_code}")
            print(f"Response content: {e.response.content}")

if __name__ == "__main__":
    main()