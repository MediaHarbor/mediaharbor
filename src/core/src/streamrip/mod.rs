pub mod downloader;
pub mod converter;
pub mod tagger;
pub mod deezer_client;
pub mod qobuz_client;
pub mod tidal_client;
pub mod orchestrator;

pub fn tidal_quality_label(quality: u8) -> &'static str {
    match quality {
        0 => "Low",
        1 => "High",
        2 => "16-bit ⁄ 44.1kHz",
        _ => "MQA",
    }
}

pub fn tidal_format(quality: u8) -> &'static str {
    match quality { 0 | 1 => "AAC", _ => "FLAC" }
}

pub fn deezer_quality_label(quality: &str) -> &'static str {
    match quality.to_uppercase().as_str() {
        "MP3_128" | "128" => "128kbps",
        "MP3_320" | "320" => "320kbps",
        _ => "16-bit ⁄ 44.1kHz",
    }
}

pub fn deezer_format(quality: &str) -> &'static str {
    match quality.to_uppercase().as_str() {
        "MP3_128" | "128" | "MP3_320" | "320" => "MP3",
        _ => "FLAC",
    }
}

/// Fallback when actual audio metadata is unavailable — based on requested format_id only.
pub fn qobuz_quality_label(format_id: u32) -> &'static str {
    match format_id {
        5  => "320kbps",
        6  => "16-bit ⁄ 44.1kHz",
        7  => "24-bit ⁄ 96kHz",
        27 => "24-bit ⁄ 192kHz",
        _  => "",
    }
}

/// Converts settings quality u8 to Qobuz format_id u32.
pub fn qobuz_settings_to_format_id(quality: u8) -> u32 {
    match quality {
        5 | 6 | 7 | 27 => quality as u32,
        1 => 5, 2 => 6, 3 => 7, _ => 27,
    }
}

/// Accurate quality spec (no format prefix): caps at user's requested format_id, then takes min with API-reported max.
pub fn qobuz_audio_quality_label(ext: &str, format_id: u32, api_max_bit_depth: u32, api_max_sampling_rate: f64) -> String {
    if ext != "flac" {
        return "320kbps".to_string();
    }
    let (req_bd, req_sr): (u32, f64) = match format_id {
        6  => (16, 44.1),
        7  => (24, 96.0),
        27 => (24, 192.0),
        _  => (16, 44.1),
    };
    let eff_bd = req_bd.min(api_max_bit_depth);
    let eff_sr = req_sr.min(api_max_sampling_rate);
    let sr_str = if eff_sr.fract() == 0.0 {
        format!("{}kHz", eff_sr as u32)
    } else {
        format!("{:.1}kHz", eff_sr)
    };
    format!("{}-bit ⁄ {}", eff_bd, sr_str)
}

pub fn safe_name(s: &str) -> String {
    s.chars().map(|c| match c {
        '<'  => '＜',
        '>'  => '＞',
        ':'  => '：',
        '"'  => '＂',
        '/'  => '⁄',
        '\\' => '＼',
        '|'  => '｜',
        '?'  => '？',
        '*'  => '＊',
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
    quality: &str,
    format: &str,
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
                "quality"     => safe_name(quality),
                "format"      => format.to_string(),
                _             => String::new(),
            }
        }).to_string())
        .unwrap_or_else(|_| template.to_string());

    let folder = result.chars().map(|c| match c {
        '<'  => '＜',
        '>'  => '＞',
        ':'  => '：',
        '"'  => '＂',
        '/'  => '⁄',
        '\\' => '＼',
        '|'  => '｜',
        '?'  => '？',
        '*'  => '＊',
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
