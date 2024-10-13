import argparse
from minim import spotify
import re

def get_spotify_track_info(track_url):
    # Initialize Spotify Web API client
    client_spotify = spotify.WebAPI(client_id='83f950693ddf4e6196c43d92db6f700f', 
                                    client_secret='41854f6fd822421283b5df1facbc0925')

    # Extract track ID from URL
    track_id_match = re.search(r'track/(\w+)', track_url)
    if not track_id_match:
        return "Invalid track URL"
    
    track_id = track_id_match.group(1)
    
    # Get track details by ID
    track_spotify = client_spotify.get_track(track_id)
    
    track_info = {
        "cover": track_spotify['album']['images'][0]['url'],
        "title": track_spotify['name'],
        "album_name": track_spotify['album']['name'],
        "artist": ", ".join([artist['name'] for artist in track_spotify['artists']])
    }
    
    return track_info

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Get Spotify track info.')
    parser.add_argument('--track', type=str, required=True, help='Spotify track URL')
    args = parser.parse_args()

    track_info = get_spotify_track_info(args.track)
    print(track_info)
