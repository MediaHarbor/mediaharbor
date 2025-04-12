import argparse
import json
from ytmusicapi import YTMusic

# Initialize the YTMusic API
ytmusic = YTMusic()

def get_track_list(content_id, content_type):
    """
    Get track list for an album or playlist.

    Parameters:
        content_id (str): The ID of the album or playlist
        content_type (str): Either 'album' or 'playlist'

    Returns:
        dict: Album information and track list in a format matching the app structure
    """
    try:
        if content_type == 'album':
            content = ytmusic.get_album(content_id)

            total_duration = sum(
                sum(int(t) * (60 ** i) for i, t in enumerate(reversed(track.get('duration', '0:00').split(':'))))
                for track in content.get('tracks', [])
            )

            album_info = {
                "title": content.get('title', 'Unknown Album'),
                "artist": content.get('artists', [{'name': 'Unknown Artist'}])[0]['name'],
                "coverUrl": content.get('thumbnails', [])[-1]['url'] if content.get('thumbnails') else '',
                "description": content.get('description', ''),
                "duration": total_duration
            }

            tracks = [{
                "id": track.get('videoId', ''),
                "number": index + 1,
                "title": track.get('title', 'Unknown Title'),
                "duration": sum(int(t) * (60 ** i) for i, t in enumerate(reversed(track.get('duration', '0:00').split(':')))),
                "quality": '256Kbps',
                "playUrl": f"https://music.youtube.com/watch?v={track.get('videoId')}" if track.get('videoId') else None
            } for index, track in enumerate(content.get('tracks', []))]

            return {
                "album": album_info,
                "tracks": tracks
            }

        elif content_type == 'playlist':
            content = ytmusic.get_playlist(content_id)

            total_duration = sum(
                sum(int(t) * (60 ** i) for i, t in enumerate(reversed(track.get('duration', '0:00').split(':'))))
                for track in content.get('tracks', [])
            )

            playlist_info = {
                "title": content.get('title', 'Unknown Playlist'),
                "artist": content.get('author', {}).get('name', 'Unknown Creator'),
                "releaseDate": '',
                "coverUrl": content.get('thumbnails', [])[-1]['url'] if content.get('thumbnails') else '',
                "description": content.get('description', ''),
                "duration": total_duration
            }

            tracks = [{
                "id": track.get('videoId', ''),
                "number": index + 1,
                "title": track.get('title', 'Unknown Title'),
                "duration": sum(int(t) * (60 ** i) for i, t in enumerate(reversed(track.get('duration', '0:00').split(':')))),
                "quality": 'HIGH',
                "playUrl": f"https://music.youtube.com/watch?v={track.get('videoId')}" if track.get('videoId') else None
            } for index, track in enumerate(content.get('tracks', []))]

            return {
                "album": playlist_info,
                "tracks": tracks
            }

        else:
            raise ValueError("Invalid content type. Must be 'album' or 'playlist'")
    except Exception as e:
        raise Exception(f"Error fetching {content_type} tracks: {str(e)}")

def search_youtube_music(query, search_type="songs", raw_response=False):
    """
    Search YouTube Music for tracks, albums, playlists, podcasts, or artists.

    Parameters:
        query (str): The search query.
        search_type (str): The type of content to search for.
        raw_response (bool): If True, returns the raw API response for podcasts and episodes.

    Returns:
        list: A list of search results based on the search_type.
    """
    search_type_map = {
        'song': 'songs',
        'album': 'albums',
        'playlist': 'playlists',
        'artist': 'artists',
        'podcast': 'podcasts',
        'episode': 'episodes'
    }

    if search_type not in search_type_map:
        raise ValueError(f"Invalid search type: {search_type}. Valid types are {list(search_type_map.keys())}.")

    search_filter = search_type_map[search_type]
    search_results = ytmusic.search(query, filter=search_filter)

    # Return raw response for podcasts and episodes if requested
    if raw_response and search_type in ['podcast', 'episode']:
        return search_results

    formatted_results = []

    if search_type == 'song':
        for result in search_results:
            if 'album' in result:
                formatted_results.append({
                    "TrackTitle": result.get('title', 'Unknown Title'),
                    "AlbumTitle": result['album'].get('name', 'Unknown Album'),
                    "AlbumCover": result['thumbnails'][-1]['url'],
                    "ArtistName": result['artists'][0].get('name', 'Unknown Artist') if result.get('artists') else 'Unknown Artist',
                    "TrackURL": f"https://music.youtube.com/watch?v={result['videoId']}" if 'videoId' in result else 'N/A',
                    "Explicit": result.get('isExplicit', False)
                })
    elif search_type == 'album':
        for result in search_results:
            formatted_results.append({
                "AlbumTitle": result.get('title', 'Unknown Album'),
                "AlbumCover": result['thumbnails'][-1]['url'] if result.get('thumbnails') else 'N/A',
                "ArtistName": result['artists'][0].get('name', 'Unknown Artist') if result.get('artists') else 'Unknown Artist',
                "AlbumURL": f"https://music.youtube.com/browse/{result['browseId']}" if 'browseId' in result else 'N/A',
                "Explicit": result.get('isExplicit', False),
                "BrowseId": result.get('browseId', 'N/A')
            })
    elif search_type == 'playlist':
        for result in search_results:
            formatted_results.append({
                "PlaylistTitle": result.get('title', 'Unknown Playlist'),
                "PlaylistCover": result['thumbnails'][-1]['url'] if result.get('thumbnails') else 'N/A',
                "Author": result.get('author', 'Unknown Author'),
                "PlaylistURL": f"https://music.youtube.com/browse/{result['browseId']}" if 'browseId' in result else 'N/A',
                "BrowseId": result.get('browseId', 'N/A')
            })
    elif search_type == 'artist':
        for result in search_results:
            formatted_results.append({
                "ArtistName": result.get('artist', result.get('title', 'Unknown Artist')),
                "ArtistCover": result['thumbnails'][-1]['url'] if result.get('thumbnails') else 'N/A',
                "ArtistURL": f"https://music.youtube.com/browse/{result['browseId']}" if 'browseId' in result else 'N/A'
            })
    elif search_type == 'podcast':
        for result in search_results:
            formatted_results.append({
                "PodcastTitle": result.get('title', 'Unknown Podcast'),
                "PodcastCover": result['thumbnails'][-1]['url'] if result.get('thumbnails') else 'N/A',
                "PodcastURL": f"https://music.youtube.com/browse/{result['browseId']}" if 'browseId' in result else 'N/A'
            })
    elif search_type == 'episode':
        for result in search_results:
            formatted_results.append({
                "EpisodeTitle": result.get('title', 'Unknown Title'),
                "EpisodeCover": result['thumbnails'][-1]['url'] if result.get('thumbnails') else 'N/A',
                "Podcast": result['podcast']['name'],
                "EpisodeURL": f"https://music.youtube.com/watch?v={result['videoId']}" if 'videoId' in result else 'N/A',
            })
    return formatted_results

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Search YouTube Music for songs, albums, playlists, podcasts, or artists.")
    parser.add_argument('-q', '--query', help="The search query")
    parser.add_argument('-t', '--type', choices=['song', 'album', 'playlist', 'artist', 'podcast', 'episode'],
                        default='song', help="The type of content to search for")
    parser.add_argument('-r', '--raw', action='store_true',
                        help="Return raw API response for podcasts and episodes")
    parser.add_argument('--get-track-list', help="Get track list for album/ID or playlist/ID (format: 'album/ID' or 'playlist/ID')")

    args = parser.parse_args()

    try:
        if args.get_track_list:
            content_type, content_id = args.get_track_list.split('/')
            if content_type not in ['album', 'playlist']:
                raise ValueError("Track list can only be retrieved for albums or playlists")
            results = get_track_list(content_id, content_type)
        elif args.query:
            results = search_youtube_music(args.query, args.type, args.raw)
        else:
            parser.error("Either --query or --get-track-list must be specified")

        if results:
            print(json.dumps(results, indent=4))
        else:
            print("No results found.")
    except Exception as e:
        print(f"Error: {str(e)}")