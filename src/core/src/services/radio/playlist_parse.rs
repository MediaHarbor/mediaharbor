use crate::errors::{MhError, MhResult};

/// What a fetched playlist body turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistKind {
    /// An HLS media/master manifest. It shares the `.m3u8` extension with an
    /// ordinary playlist but is not a list of streams — parsing it yields segment
    /// URLs, which play about two seconds of audio and stop. The original URL is
    /// what gets handed to the remuxer.
    Hls,
    Entries(Playlist),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Playlist {
    /// Every stream in the file, in the order it was listed. SomaFM publishes
    /// three servers per channel and any of them will do.
    pub urls: Vec<String>,
    /// The first human-readable title the file carried, if it carried one.
    pub title: Option<String>,
}

/// Classifies and parses a playlist body.
///
/// Discrimination comes first and is deliberate: an `#EXT-X-` tag anywhere in the
/// file means HLS, whatever the extension said.
pub fn parse(body: &str) -> MhResult<PlaylistKind> {
    if body.lines().any(|l| l.trim_start().starts_with("#EXT-X-")) {
        return Ok(PlaylistKind::Hls);
    }
    let lower = body.trim_start().to_ascii_lowercase();
    let playlist = if lower.starts_with("[playlist]") {
        parse_pls(body)
    } else {
        parse_m3u(body)
    };
    if playlist.urls.is_empty() {
        return Err(MhError::Parse(
            "playlist contained no stream URLs".to_string(),
        ));
    }
    Ok(PlaylistKind::Entries(playlist))
}

/// `.pls` is an INI file: `File1=`, `Title1=`, `Length1=`, repeated. The indices
/// are not guaranteed contiguous, so entries are read in file order rather than
/// counted off `numberofentries`.
fn parse_pls(body: &str) -> Playlist {
    let mut out = Playlist::default();
    for line in body.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if key.starts_with("file") {
            out.urls.push(value.to_string());
        } else if key.starts_with("title") && out.title.is_none() {
            out.title = Some(value.to_string());
        }
    }
    out
}

/// `.m3u` is one URL per line; `#EXTINF:` carries a title for the line that
/// follows it. Anything else beginning with `#` is a comment.
fn parse_m3u(body: &str) -> Playlist {
    let mut out = Playlist::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            if out.title.is_none() {
                let title = rest.split_once(',').map(|(_, t)| t.trim()).unwrap_or("");
                if !title.is_empty() {
                    out.title = Some(title.to_string());
                }
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        out.urls.push(line.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOMAFM_PLS: &str = "[playlist]\r\nnumberofentries=3\r\n\
File1=https://ice6.somafm.com/7soul-128-mp3\r\n\
Title1=SomaFM: Seven Inch Soul (#1): Vintage soul tracks.\r\n\
Length1=-1\r\n\
File2=https://ice2.somafm.com/7soul-128-mp3\r\n\
Title2=SomaFM: Seven Inch Soul (#2): Vintage soul tracks.\r\n\
Length2=-1\r\n\
File3=https://ice5.somafm.com/7soul-128-mp3\r\n\
Title3=SomaFM: Seven Inch Soul (#3): Vintage soul tracks.\r\n\
Length3=-1\r\nVersion=2\r\n";

    #[test]
    fn pls_keeps_every_mirror_in_order() {
        let PlaylistKind::Entries(p) = parse(SOMAFM_PLS).unwrap() else {
            panic!("a .pls is not HLS");
        };
        assert_eq!(
            p.urls,
            vec![
                "https://ice6.somafm.com/7soul-128-mp3",
                "https://ice2.somafm.com/7soul-128-mp3",
                "https://ice5.somafm.com/7soul-128-mp3",
            ]
        );
        assert!(p.title.unwrap().starts_with("SomaFM: Seven Inch Soul (#1)"));
    }

    #[test]
    fn m3u_skips_comments_and_reads_extinf_title() {
        let body = "#EXTM3U\n#EXTINF:-1,Radio Paradise Main Mix\n\
https://stream.radioparadise.com/mp3-192\n\
# a bare comment\n\nhttp://mirror.example/stream\n";
        let PlaylistKind::Entries(p) = parse(body).unwrap() else {
            panic!("a plain .m3u is not HLS");
        };
        assert_eq!(
            p.urls,
            vec![
                "https://stream.radioparadise.com/mp3-192",
                "http://mirror.example/stream",
            ]
        );
        assert_eq!(p.title.as_deref(), Some("Radio Paradise Main Mix"));
    }

    /// The case the discrimination exists for: without it these segment URLs
    /// parse as three stations and play two seconds of audio.
    #[test]
    fn hls_manifest_is_classified_not_parsed() {
        let body = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:4207\n#EXTINF:10.0,\nseg4207.aac\n\
#EXTINF:10.0,\nseg4208.aac\n#EXTINF:10.0,\nseg4209.aac\n";
        assert_eq!(parse(body).unwrap(), PlaylistKind::Hls);
    }

    #[test]
    fn master_manifest_is_also_hls() {
        let body = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000,CODECS=\"mp4a.40.2\"\n\
chunklist_b128000.m3u8\n";
        assert_eq!(parse(body).unwrap(), PlaylistKind::Hls);
    }

    #[test]
    fn an_empty_playlist_is_an_error_not_an_empty_station() {
        let err = parse("#EXTM3U\n# nothing here\n").unwrap_err();
        assert!(err.to_string().contains("no stream URLs"), "{err}");
    }
}
