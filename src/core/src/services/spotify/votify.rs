//! votify, the Spotify CLI downloader, as a [`CliTool`].

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::services::common::cli::{base_args, CliTool};
use crate::settings;

/// votify takes web URLs, but a track dragged out of the desktop client is a
/// `spotify:kind:id` URI. Rewrite it rather than letting votify reject it.
fn normalize_spotify_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("spotify:") {
        let mut parts = rest.splitn(2, ':');
        if let (Some(kind), Some(id)) = (parts.next(), parts.next()) {
            if !kind.is_empty() && !id.is_empty() {
                return format!("https://open.spotify.com/{kind}/{id}");
            }
        }
    }
    url.to_string()
}

pub struct Votify;

#[async_trait]
impl CliTool for Votify {
    fn program(&self) -> &'static str {
        "votify"
    }

    fn log_title(&self) -> &'static str {
        "Votify"
    }

    fn config_path(&self, user_data: &Path) -> PathBuf {
        settings::spotify_config_path(user_data)
    }

    async fn write_config(&self, settings: &Settings, config_path: &Path) -> MhResult<()> {
        settings::save_service_config(config_path, settings, "spotify").await
    }

    fn build_args(
        &self,
        settings: &Settings,
        url: &str,
        config_path: &Path,
        quality: Option<&str>,
    ) -> Vec<String> {
        let mut args = base_args(settings, config_path, "Spotify");

        if let Some(q) = quality {
            args.push("--audio-quality".into());
            args.push(q.to_string());
        }

        if !url.is_empty() {
            args.push(normalize_spotify_url(url));
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_for(url: &str) -> Vec<String> {
        let settings = Settings::default();
        let config = std::env::temp_dir().join("votify_config.ini");
        Votify.build_args(&settings, url, &config, None)
    }

    #[test]
    fn args_have_config_and_url() {
        let args = args_for("https://open.spotify.com/track/abc");
        assert!(args.contains(&"--config-path".to_string()));
        assert_eq!(args.last().unwrap(), "https://open.spotify.com/track/abc");
    }

    #[test]
    fn quality_becomes_an_audio_quality_flag() {
        let settings = Settings::default();
        let config = std::env::temp_dir().join("votify_config.ini");
        let args = Votify.build_args(
            &settings,
            "https://open.spotify.com/track/abc",
            &config,
            Some("OGG_VORBIS_320"),
        );
        let idx = args
            .iter()
            .position(|a| a == "--audio-quality")
            .expect("quality flag present");
        assert_eq!(args[idx + 1], "OGG_VORBIS_320");
    }

    #[test]
    fn normalizes_spotify_uri_to_url() {
        assert_eq!(
            normalize_spotify_url("spotify:album:c3x0lwc13rnty"),
            "https://open.spotify.com/album/c3x0lwc13rnty"
        );
        assert_eq!(
            normalize_spotify_url("spotify:playlist:37i9dQZF1DX"),
            "https://open.spotify.com/playlist/37i9dQZF1DX"
        );
        assert_eq!(
            normalize_spotify_url("https://open.spotify.com/track/abc"),
            "https://open.spotify.com/track/abc"
        );

        let args = args_for("spotify:album:c3x0lwc13rnty");
        assert_eq!(
            args.last().unwrap(),
            "https://open.spotify.com/album/c3x0lwc13rnty"
        );
    }
}
