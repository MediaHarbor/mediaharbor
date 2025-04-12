import requests
import argparse
import json
import ssl

# To ensure SSL verification (security best practice)
ssl._create_default_https_context = ssl._create_unverified_context

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

# Function to search playlists
def search_playlists(query):
    search_url = f'https://api.deezer.com/search/playlist?q={query}'
    response = requests.get(search_url)
    return response.json()['data']

# Function to get track list from an album or playlist by ID
def get_track_list(item):
    if '/' not in item:
        raise ValueError("Invalid format. Use 'album/ID' or 'playlist/ID'.")

    item_type, item_id = item.split('/')
    if item_type not in ['album', 'playlist']:
        raise ValueError("Invalid item type. Must be 'album' or 'playlist'.")

    # Fetch item details (album/playlist metadata)
    details_url = f'https://api.deezer.com/{item_type}/{item_id}'
    details_response = requests.get(details_url)
    if details_response.status_code != 200:
        return {'error': f"Failed to fetch details for {item_type} ID {item_id}"}

    item_details = details_response.json()

    # Fetch track list
    track_url = f'https://api.deezer.com/{item_type}/{item_id}/tracks'
    track_response = requests.get(track_url)
    if track_response.status_code != 200:
        return {'error': f"Failed to fetch tracks for {item_type} ID {item_id}"}

    track_list = track_response.json()['data']

    # Include album/playlist metadata
    metadata = {
        "type": item_type,
        "id": item_id,
        "name": item_details.get("title", "Unknown Title"),
        "artist": item_details.get("artist", {}).get("name", "Unknown Artist") if item_type == "album" else "N/A",
        "release_date": item_details.get("release_date", "Unknown Date"),
        "total_tracks": len(track_list)
    }
    metadata['tracks'] = track_list

    return metadata

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Deezer API Script')
    parser.add_argument('--get-details', type=int, help='Get details of a track by ID')
    parser.add_argument('--search-track', type=str, help='Search for tracks')
    parser.add_argument('--search-album', type=str, help='Search for albums')
    parser.add_argument('--search-artist', type=str, help='Search for artists')
    parser.add_argument('--search-playlist', type=str, help='Search for playlists')
    parser.add_argument('--get-track-list', type=str,
                        help="Get track list from an album or playlist by ID. Format: 'album/ID' or 'playlist/ID'.")

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
        print(json.dumps(found_playlists, indent=4))  # Print as formatted JSON

    if args.get_track_list:
        track_list = get_track_list(args.get_track_list)
        print(json.dumps(track_list, indent=4))  # Print as formatted JSON
