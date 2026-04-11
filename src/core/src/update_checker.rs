use serde::Deserialize;
use std::cmp::Ordering;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    pub body: Option<String>,
    pub asset_url: Option<String>,
}

pub struct UpdateChecker {
    pub owner: String,
    pub repo: String,
    pub current_version: String,
}

impl UpdateChecker {
    pub fn new(owner: impl Into<String>, repo: impl Into<String>, current_version: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
            current_version: current_version.into(),
        }
    }

    pub async fn check_for_updates(&self) -> MhResult<Option<ReleaseInfo>> {
        let api_url = format!(
            "https://api.github.com/repos/{}/{}/releases",
            self.owner, self.repo
        );

        let client = build_mozilla_client()?;

        let resp = client
            .get(&api_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| MhError::Network(e))?;

        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "GitHub API returned {}",
                resp.status()
            )));
        }

        let releases: Vec<GhRelease> = resp.json().await.map_err(|e| MhError::Network(e))?;

        let latest = match releases.into_iter().find(|r| !r.prerelease) {
            Some(r) => r,
            None => return Ok(None),
        };

        let latest_ver = latest.tag_name.trim_start_matches('v');
        let current_ver = self.current_version.trim_start_matches('v');

        if compare_versions(latest_ver, current_ver) != Ordering::Greater {
            return Ok(None);
        }

        let asset_url = find_platform_asset(&latest.assets);

        Ok(Some(ReleaseInfo {
            tag_name: latest.tag_name,
            html_url: latest.html_url,
            body: latest.body,
            asset_url,
        }))
    }
}

pub fn compare_versions(v1: &str, v2: &str) -> Ordering {
    let parts1: Vec<u64> = v1.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let parts2: Vec<u64> = v2.split('.').map(|p| p.parse().unwrap_or(0)).collect();

    let len = parts1.len().max(parts2.len());
    for i in 0..len {
        let a = parts1.get(i).copied().unwrap_or(0);
        let b = parts2.get(i).copied().unwrap_or(0);
        match a.cmp(&b) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

fn find_platform_asset(assets: &[GhAsset]) -> Option<String> {
    #[cfg(target_os = "windows")]
    let suffix = ".exe";
    #[cfg(target_os = "macos")]
    let suffix = ".dmg";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let suffix = ".AppImage";

    assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .map(|a| a.browser_download_url.clone())
}
