import requests
import argparse
import json

# Set your Qobuz credentials
app_id = "950096963"
app_secret = "10b251c286cfbf64d6b7105f253d9a2e"
auth_token = "u6lHtzb1Vv_TbNYYL_PrIzVZfkMpxUJ4Y4AkpdrfFRaj5o1sbLP7ENCKVD-wQEmkMbQIN-G6vcgzPvwaZdEvPA"

# Define a function to search and return data
def search_qobuz(query, search_type):
    try:
        url = f"https://www.qobuz.com/api.json/0.2/{search_type}/search?app_id={app_id}&query={query}&limit=10"
        response = requests.get(url, headers={"X-User-Auth-Token": auth_token})
        return response.json()
    except Exception as e:
        print(f"An error occurred: {e}")
        return {}

# Define a function to get track details
def get_track_details(track_id):
    try:
        url = f"https://www.qobuz.com/api.json/0.2/track/get?app_id={app_id}&track_id={track_id}"
        response = requests.get(url, headers={"X-User-Auth-Token": auth_token})
        return response.json()
    except Exception as e:
        print(f"An error occurred: {e}")
        return {}

# Define a function to get the track stream
def get_track_stream(track_id, format_id=27):
    try:
        url = f"https://www.qobuz.com/api.json/0.2/track/getFileUrl?app_id={app_id}&track_id={track_id}&format_id={format_id}"
        response = requests.get(url, headers={"X-User-Auth-Token": auth_token})
        return response.json()
    except Exception as e:
        print(f"An error occurred: {e}")
        return {}

# Define a function to get album list for an artist by artist ID
def get_album_list(artist_id):
    try:
        url = f"https://www.qobuz.com/api.json/0.2/artist/get?app_id={app_id}&artist_id={artist_id}"
        response = requests.get(url, headers={"X-User-Auth-Token": auth_token})
        artist_data = response.json()
        
        if "albums" in artist_data:
            return artist_data["albums"]
        else:
            return {"status": "error", "message": "No albums found for this artist."}
    except Exception as e:
        print(f"An error occurred: {e}")
        return {}

def get_track_list(entity_id, entity_type):
    try:
        if entity_type == "album":
            url = f"https://www.qobuz.com/api.json/0.2/album/get?app_id={app_id}&album_id={entity_id}"
        elif entity_type == "playlist":
            url = f"https://www.qobuz.com/api.json/0.2/playlist/get?app_id={app_id}&playlist_id={entity_id}&extra=tracks"
        elif entity_type == "artist":
            url = f"https://www.qobuz.com/api.json/0.2/artist/get?app_id={app_id}&artist_id={entity_id}&extra=albums"
        else:
            return {"status": "error", "message": f"Unknown entity type: {entity_type}"}

        response = requests.get(url, headers={"X-User-Auth-Token": auth_token})
        entity_data = response.json()

        # Handle playlist-specific track retrieval
        if entity_type == "playlist" and "tracks" in entity_data:
            return entity_data["tracks"]["items"]  # Tracks are nested under 'tracks' > 'items'

        # Handle album-specific track retrieval
        if entity_type == "album" and "tracks" in entity_data:
            return entity_data["tracks"]["items"]

        # Handle artist data retrieval
        if entity_type == "artist":
            return entity_data

        return {"status": "error", "message": f"No tracks found for this {entity_type}."}
    except Exception as e:
        print(f"An error occurred: {e}")
        return {}

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Search Qobuz or get details.')
    parser.add_argument('--search-track', help='Search for a track by release name')
    parser.add_argument('--search-artist', help='Search for an artist')
    parser.add_argument('--search-album', help='Search for an album by label')
    parser.add_argument('--search-playlist', help='Search for a playlist by query')
    parser.add_argument('--get-details', help='Get track details by track ID')
    parser.add_argument('--get-stream', help='Get track stream by track ID')
    parser.add_argument('--format-id', type=int, default=27, help='Audio format ID for track stream (default: 27)')
    parser.add_argument('--get-album-list', help='Get album list by artist ID')
    parser.add_argument('--get-track-list', help='Get track list by entity_type/entity_id (e.g., album/123, playlist/456, artist/789)')
    args = parser.parse_args()

    if args.search_track:
        results = search_qobuz(args.search_track, "track")
        print(json.dumps(results, indent=4))
    elif args.search_artist:
        results = search_qobuz(args.search_artist, "artist")
        print(json.dumps(results, indent=4))
    elif args.search_album:
        results = search_qobuz(args.search_album, "album")
        print(json.dumps(results, indent=4))
    elif args.search_playlist:
        results = search_qobuz(args.search_playlist, "playlist")
        print(json.dumps(results, indent=4))
    elif args.get_details:
        details = get_track_details(args.get_details)
        print(json.dumps(details, indent=4))
    elif args.get_stream:
        stream = get_track_stream(args.get_stream, format_id=args.format_id)
        print(json.dumps(stream, indent=4))
    elif args.get_album_list:
        albums = get_album_list(args.get_album_list)
        print(json.dumps(albums, indent=4))
    elif args.get_track_list:
        entity_type, entity_id = args.get_track_list.split("/")
        tracks = get_track_list(entity_id, entity_type)
        print(json.dumps(tracks, indent=4))
    else:
        print("Please provide a valid argument.")