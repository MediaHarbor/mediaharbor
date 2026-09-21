//! Single source of truth for per-service API result-page sizes.
//!
//! Each `*_PAGE_MAX` is the largest `limit`/`nb`/`maxResults` a single request to
//! that endpoint accepts before the API rejects or silently clamps it. Callers that
//! need more than one page must paginate (loop `offset`/`index`/cursor) — the page
//! max is not the total-results cap.
//!
//! Values marked `probed` were measured with `examples/search_limits_probe.rs`.
//! Values marked `documented` could not be probed live (no credentials / no key) and
//! come from the provider's published API limits.

/// Default number of search results a caller asks for when it has no explicit limit.
pub const DEFAULT_SEARCH_LIMIT: u32 = 50;

/// `album/{id}/tracks` — Web API hard page cap. (documented: 50)
pub const SPOTIFY_ALBUM_TRACKS_PAGE_MAX: u32 = 50;
/// `playlist/{id}/tracks` — Web API hard page cap. (documented: 100)
pub const SPOTIFY_PLAYLIST_TRACKS_PAGE_MAX: u32 = 100;
/// `artist/{id}/albums`, `search`, `show/{id}/episodes`, `audiobook chapters`. (documented: 50)
pub const SPOTIFY_PAGE_MAX: u32 = 50;
/// pathfinder `getAlbum`/`fetchPlaylist` `limit` (probed: 200+ accepted).
pub const SPOTIFY_PATHFINDER_PAGE_MAX: u32 = 200;

/// Public REST `/search/*`, `/artist/{id}/albums`, list `nb`. Deezer hard-clamps
/// the index endpoint at 100 regardless of a larger requested limit. (probed: 100)
pub const DEEZER_PAGE_MAX: u32 = 100;
/// Private gateway page `nb` (pagePlaylist / pageShow) tolerates large pulls.
pub const DEEZER_GATEWAY_PAGE_MAX: u32 = 500;

/// `search_v1` / `artists/{id}/albums` `limit`. v1 honours a large limit; v2's
/// `searchResults` returns a fixed ~20-item page and must be paginated by offset
/// through v1 when more is needed. (probed: v2 fixed page = 20; v1 honours 100)
pub const TIDAL_PAGE_MAX: u32 = 100;

/// `{type}/search` and album/playlist `limit`. Qobuz honours the requested limit
/// up to 500, then hard-clamps anything larger back to 500. (probed: cap = 500)
pub const QOBUZ_PAGE_MAX: u32 = 500;

/// YouTube Data API v3 `maxResults` — hard cap. (documented: 50)
pub const YOUTUBE_DATA_PAGE_MAX: u32 = 50;
/// YT Music internal `browse`/`search` continuation page size.
pub const YTMUSIC_PAGE_MAX: u32 = 100;

/// iTunes `lookup`/`search` `limit` — hard cap. (documented: 200)
pub const APPLE_MUSIC_PAGE_MAX: u32 = 200;

/// One page of internet-radio stations, as the radio page asks for it. Not an
/// upstream cap — the local directories have no page concept at all, so this is
/// simply the window a request without an explicit `limit` gets.
pub const RADIO_PAGE_DEFAULT: u32 = 60;
/// radio-browser `/json/stations/search` `limit`. (documented: 500)
pub const RADIO_BROWSER_PAGE_MAX: u32 = 500;

/// Number of autocomplete suggestions surfaced to the search box.
pub const SUGGESTIONS_LIMIT: usize = 8;
