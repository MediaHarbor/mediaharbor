import requests
import argparse
import json

# Function to get track details
def get_track(track_id):
    track_url = f'https://api.deezer.com/track/{track_id}'
    response = requests.get(track_url)
    return response.json()

# Function to search tracks
def search_tracks(query):
    search_url = f'https://api.deezer.com/search/track?q={query}'
    response = requests.get(search_url)
    return response.json()['data']

# Function to search albums
def search_albums(query):
    search_url = f'https://api.deezer.com/search/album?q={query}'
    response = requests.get(search_url)
    return response.json()['data']

# Function to search artists
def search_artists(query):
    search_url = f'https://api.deezer.com/search/artist?q={query}'
    response = requests.get(search_url)
    return response.json()['data']

def search_playlists(query):
    search_url = f'https://api.deezer.com/search/playlist?q={query}'
    response = requests.get(search_url)
    return response.json()['data']

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Deezer API Script')
    parser.add_argument('--get-details', type=int, help='Get details of a track by ID')
    parser.add_argument('--search-track', type=str, help='Search for tracks')
    parser.add_argument('--search-album', type=str, help='Search for albums')
    parser.add_argument('--search-artist', type=str, help='Search for artists')
    parser.add_argument('--search-playlist', type=str, help='Search for playlists')

    args = parser.parse_args()

    if args.get_details:
        track_details = get_track(args.get_details)
        print(json.dumps(track_details, indent=4))  # Print as formatted JSON

    if args.search_track:
        found_tracks = search_tracks(args.search_track)
        print(json.dumps(found_tracks, indent=4))  # Print as formatted JSON

    if args.search_album:
        found_albums = search_albums(args.search_album)
        print(json.dumps(found_albums, indent=4))  # Print as formatted JSON

    if args.search_artist:
        found_artists = search_artists(args.search_artist)
        print(json.dumps(found_artists, indent=4))  # Print as formatted JSON

    if args.search_playlist:
        found_playlists = search_playlists(args.search_playlist)
        print(json.dumps(found_playlists, indent=4))
