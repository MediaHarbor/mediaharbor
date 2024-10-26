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

    def make_request(self, method, url, params=None):
        self.authenticate()
        headers = {
            "Authorization": f"Bearer {self.access_token}",
            "Accept": "application/vnd.api+json"
        }
        response = self.session.request(method, url, headers=headers, params=params)
        response.raise_for_status()
        return response.json()

    def get_track(self, track_id: str, country_code: str):
        params = {"countryCode": country_code, "include": "artists,albums"}
        return self.make_request("GET", f"{self.API_URL}/tracks/{track_id}", params=params)
    

    def get_album(self, album_id: str, country_code: str):
        return self.make_request("GET", f"{self.API_URL}/albums/{album_id}", params={"countryCode": country_code})

    def search_tracks(self, query: str, country_code: str, limit: int = 10):
        params = {
            "countryCode": country_code,
            "limit": limit,
            "include": "tracks"
        }
        encoded_query = quote(query)
        return self.make_request("GET", f"{self.API_URL}/searchresults/{encoded_query}/relationships/tracks", params=params)

    def search_albums(self, query: str, country_code: str, limit: int = 10):
        params = {
            "countryCode": country_code,
            "limit": limit,
            "include": "albums"
        }
        encoded_query = quote(query)
        return self.make_request("GET", f"{self.API_URL}/searchresults/{encoded_query}/relationships/albums", params=params)

def main():
    parser = argparse.ArgumentParser(description="Tidal API script")
    parser.add_argument('--country-code', default='US', help='Country code (default: US)')
    parser.add_argument('--get-track', help='Track ID to get details for')
    parser.add_argument('--get-album', help='Album ID to get details for')
    parser.add_argument('--search-track', help='Track name to search for')
    parser.add_argument('--search-album', help='Album name to search for')
    
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
            print(f"Searching for track: {args.search_track}")
            search_results = tidal_api.search_tracks(query=args.search_track, country_code=args.country_code)
            print("Track Search Results JSON:")
            print(json.dumps(search_results, indent=4))

        if args.search_album:
            print(f"Searching for album: {args.search_album}")
            search_results = tidal_api.search_albums(query=args.search_album, country_code=args.country_code)
            print("Album Search Results JSON:")
            print(json.dumps(search_results, indent=4))

    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        if hasattr(e, 'response') and e.response is not None:
            print(f"Response status code: {e.response.status_code}")
            print(f"Response content: {e.response.content}")

if __name__ == "__main__":
    main()