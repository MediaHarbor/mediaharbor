import argparse
import json
from pyapplemusicapi import search_album, search_artist, search_track

def search_apple_music(term, media_type):
    try:
        if media_type == "track":
            results = search_track(term)
        elif media_type == "album":
            results = search_album(term)
        elif media_type == "artist":
            results = search_artist(term)
        else:
            print(json.dumps({"error": f"Media type '{media_type}' is not supported."}, indent=4))
            return

        # Create a list to hold all results
        all_results = []

        for item in results:
            json_data = getattr(item, 'json', {})
            if json_data:  # Only add non-empty results
                all_results.append(json_data)

        # Print all results as a single JSON array
        print(json.dumps(all_results, indent=4))

    except json.JSONDecodeError as e:
        print(json.dumps({
            "error": "JSON parsing error",
            "details": str(e)
        }, indent=4))
    except Exception as e:
        print(json.dumps({
            "error": "An error occurred",
            "details": str(e)
        }, indent=4))

def main():
    parser = argparse.ArgumentParser(description="Search Apple Music for songs, albums, and artists.")
    parser.add_argument("term", type=str, help="The search term (e.g., song name, artist name)")
    parser.add_argument(
        "--media_type",
        type=str,
        choices=["track", "album", "artist"],
        default="track",
        help="Type of media to search for (default: track)"
    )

    args = parser.parse_args()
    search_apple_music(args.term, args.media_type)

if __name__ == "__main__":
    main()