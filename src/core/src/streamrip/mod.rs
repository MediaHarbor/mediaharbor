pub mod downloader;
pub mod converter;
pub mod tagger;
pub mod deezer_client;
pub mod qobuz_client;
pub mod tidal_client;
pub mod orchestrator;

pub fn safe_name(s: &str) -> String {
    s.chars().map(|c| match c {
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
        other => other,
    }).collect()
}

pub fn build_album_folder(
    folder_format: &str,
    albumartist: &str,
    album: &str,
    year: &str,
    genre: &str,
    label: &str,
) -> String {
    let template = if folder_format.is_empty() {
        "{albumartist} - {album} ({year})"
    } else {
        folder_format
    };

    let result = regex::Regex::new(r"\{(\w+)(?::[^}]*)?\}")
        .map(|re| re.replace_all(template, |caps: &regex::Captures| -> String {
            match caps[1].as_ref() {
                "album"       => safe_name(album),
                "albumartist" => safe_name(albumartist),
                "artist"      => safe_name(albumartist),
                "year"        => year.to_string(),
                "genre"       => safe_name(genre),
                "label"       => safe_name(label),
                _             => String::new(),
            }
        }).to_string())
        .unwrap_or_else(|_| template.to_string());

    let folder = result.chars().map(|c| match c {
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
        other => other,
    }).collect::<String>()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .trim()
    .chars()
    .take(150)
    .collect::<String>();

    if folder.is_empty() { safe_name(album) } else { folder }
}
