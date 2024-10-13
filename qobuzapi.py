import requests
import argparse
import json

# Set your Qobuz credentials
app_id = "950096963"
app_secret = "10b251c286cfbf64d6b7105f253d9a2e"
auth_token = "u6lHtzb1Vv_TbNYYL_PrIzVZfkMpxUJ4Y4AkpdrfFRaj5o1sbLP7ENCKVD-wQEmkMbQIN-G6vcgzPvwaZdEvPA"

# Define a function to search and return track lists
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

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Search Qobuz or get track details.')
    parser.add_argument('--search-track', help='Search for a track by release name')
    parser.add_argument('--search-artist', help='Search for an artist')
    parser.add_argument('--search-album', help='Search for an album by label')
    parser.add_argument('--get-details', help='Get track details by track ID')
    parser.add_argument('--get-stream', help='Get track stream by track ID')
    parser.add_argument('--format-id', type=int, default=27, help='Audio format ID for track stream (default: 27)')
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
    elif args.get_details:
        details = get_track_details(args.get_details)
        print(json.dumps(details, indent=4))
    elif args.get_stream:
        stream = get_track_stream(args.get_stream, format_id=args.format_id)
        print(json.dumps(stream, indent=4))
    else:
        print("Please provide a valid argument.")
