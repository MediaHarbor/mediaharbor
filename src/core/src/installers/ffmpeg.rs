#[cfg(target_os = "macos")]
use std::path::Path;
#[cfg(target_os = "macos")]
use tempfile::TempDir;

#[cfg(target_os = "macos")]
use crate::errors::MhError;
use crate::errors::MhResult;
#[cfg(target_os = "macos")]
use crate::http_client::build_mozilla_client;
#[cfg(target_os = "macos")]
use crate::http_client::{build_download_client, download_to_file};

use super::{ArchiveKind, BinarySpec, SandboxPolicy, Target};

#[cfg(target_os = "macos")]
const OSXEXPERTS_URL: &str = "https://www.osxexperts.net/";
#[cfg(target_os = "macos")]
const FFMPEG_RSS: &str = "https://evermeet.cx/ffmpeg/rss.xml";
#[cfg(target_os = "macos")]
const FFPROBE_RSS: &str = "https://evermeet.cx/ffmpeg/ffprobe-rss.xml";

const MIN_ARCHIVE_BYTES: u64 = 100_000;

/// The two binaries every FFmpeg archive is expected to yield.
const FFMPEG_BINS: &[&str] = &["ffmpeg", "ffprobe"];
const FFMPEG_BINS_WIN: &[&str] = &["ffmpeg.exe", "ffprobe.exe"];

pub(crate) fn detect() -> Option<&'static str> {
    (crate::venv_manager::find_ffmpeg().is_some() && crate::venv_manager::find_ffprobe().is_some())
        .then_some("FFmpeg")
}

/// The managed FFmpeg builds. macOS is absent; it keeps the bespoke path below.
pub(crate) const SPEC: BinarySpec = BinarySpec {
    id: "ffmpeg",
    display_name: "FFmpeg",
    targets: &[
        Target {
            os: "windows",
            arch: "x86_64",
            urls: &["https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"],
            archive: ArchiveKind::Zip,
            binary_names: FFMPEG_BINS_WIN,
        },
        Target {
            os: "windows",
            arch: "aarch64",
            urls: &["https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-winarm64-gpl.zip"],
            archive: ArchiveKind::Zip,
            binary_names: FFMPEG_BINS_WIN,
        },
        Target {
            os: "linux",
            arch: "x86_64",
            urls: &[
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
                "https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-amd64-static.tar.xz",
            ],
            archive: ArchiveKind::TarXz,
            binary_names: FFMPEG_BINS,
        },
        Target {
            os: "linux",
            arch: "aarch64",
            urls: &[
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                "https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-arm64-static.tar.xz",
            ],
            archive: ArchiveKind::TarXz,
            binary_names: FFMPEG_BINS,
        },
        Target {
            os: "linux",
            arch: "arm",
            urls: &["https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-armhf-static.tar.xz"],
            archive: ArchiveKind::TarXz,
            binary_names: FFMPEG_BINS,
        },
    ],
    min_bytes: MIN_ARCHIVE_BYTES,
    sandbox: SandboxPolicy::RefuseEach {
        snap: "FFmpeg should be bundled in the Snap but was not found on PATH. \
               Reinstall the Snap, or report this as a packaging bug.",
        flatpak: "FFmpeg was not found inside the Flatpak sandbox. The GNOME runtime does \
                  not ship the ffmpeg binary, so it must be bundled with MediaHarbor — \
                  this build is missing it. Please report this as a packaging bug.",
    },
    detect,
    install_hint: "Please install ffmpeg via your system package manager instead.",
    unsupported_hint: "Install ffmpeg via your system package manager.",
};

#[cfg(target_os = "macos")]
struct RssItem {
    url: String,
}

#[cfg(target_os = "macos")]
async fn fetch_latest_from_rss(feed_url: &str, label: &str) -> MhResult<RssItem> {
    let client = build_mozilla_client()?;
    let xml = client.get(feed_url).send().await?.text().await?;

    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut in_item = false;
    let mut in_title = false;
    let mut in_link = false;
    let mut title = String::new();
    let mut link = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().into_inner() {
                "item" => in_item = true,
                "title" if in_item => in_title = true,
                "link" if in_item => in_link = true,
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                let text = e.xml10_content().into_owned();
                if in_title && in_item {
                    title = text;
                    in_title = false;
                } else if in_link && in_item {
                    link = text;
                    in_link = false;
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().into_inner() == "item" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(MhError::Parse(format!(
                    "RSS parse error for {}: {}",
                    label, e
                )));
            }
            _ => {}
        }
    }

    if title.is_empty() || link.is_empty() {
        return Err(MhError::Other(format!(
            "Malformed RSS response for {}",
            label
        )));
    }

    Ok(RssItem { url: link })
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn fetch_osxexperts_arm_url(bin: &str) -> MhResult<String> {
    let client = build_mozilla_client()?;
    let html = client.get(OSXEXPERTS_URL).send().await?.text().await?;

    let pattern = format!(r"(?i){}(\d+)arm\.zip", regex::escape(bin));
    let re = regex::Regex::new(&pattern)?;

    re.captures_iter(&html)
        .filter_map(|c| {
            let version: u32 = c.get(1)?.as_str().parse().ok()?;
            Some((version, c.get(0)?.as_str().to_string()))
        })
        .max_by_key(|(v, _)| *v)
        .map(|(_, file)| format!("{}{}", OSXEXPERTS_URL, file))
        .ok_or_else(|| MhError::Other(format!("osxexperts.net has no arm64 build for {}", bin)))
}

/// Ordered download URLs for a single macOS binary (native arm64 first on Apple
/// Silicon, then evermeet x86_64).
#[cfg(target_os = "macos")]
async fn mac_binary_urls(bin: &str) -> Vec<String> {
    let mut urls = Vec::new();

    #[cfg(target_arch = "aarch64")]
    if let Ok(url) = fetch_osxexperts_arm_url(bin).await {
        urls.push(url);
    }

    let rss = if bin == "ffprobe" {
        FFPROBE_RSS
    } else {
        FFMPEG_RSS
    };
    if let Ok(item) = fetch_latest_from_rss(rss, bin).await {
        urls.push(item.url);
    }

    urls
}

#[cfg(target_os = "macos")]
async fn install_macos<F: Fn(u8, &str)>(ffmpeg_dir: &Path, on_progress: &F) -> MhResult<()> {
    install_macos_binary("ffmpeg", ffmpeg_dir, 0, 25, on_progress).await?;
    install_macos_binary("ffprobe", ffmpeg_dir, 25, 50, on_progress).await?;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn install_macos_binary<F: Fn(u8, &str)>(
    bin: &str,
    ffmpeg_dir: &Path,
    pct_start: u8,
    pct_end: u8,
    on_progress: &F,
) -> MhResult<()> {
    let urls = mac_binary_urls(bin).await;
    if urls.is_empty() {
        return Err(MhError::Other(format!(
            "No download source is currently reachable for {} on macOS. Please try again later.",
            bin
        )));
    }

    let client = build_download_client()?;
    let span = (pct_end.saturating_sub(pct_start)) as u64;
    let mut last_err: Option<MhError> = None;

    for url in urls {
        let tmp = TempDir::new()?;
        let zip = tmp.path().join(format!("{}.zip", bin));

        if let Err(e) = download_to_file(&client, &url, &zip, |dl, total| {
            let pct = pct_start as u64 + total.map(|t| dl * span / t).unwrap_or(0);
            on_progress(
                pct.min(pct_end as u64) as u8,
                &format!("Downloading {}…", bin),
            );
        })
        .await
        {
            last_err = Some(e);
            continue;
        }

        if let Err(e) =
            super::validate_download(&zip, MIN_ARCHIVE_BYTES, "FFmpeg", super::ZIP_MAGICS)
        {
            last_err = Some(e);
            continue;
        }

        match super::extract_zip_single_binary(&zip, ffmpeg_dir, bin) {
            Ok(()) => {
                super::set_executable(&ffmpeg_dir.join(bin))?;
                on_progress(pct_end, &format!("{} ready", bin));
                return Ok(());
            }
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| MhError::Other(format!("Failed to install {}.", bin))))
}

/// Verify the installed ffmpeg/ffprobe actually run; remove + error on failure.
#[cfg(target_os = "macos")]
async fn smoke_test_ffmpeg(dir: &Path) -> MhResult<()> {
    for bin in FFMPEG_BINS {
        let path = dir.join(bin);
        if !crate::venv_manager::verify_binary(&path).await {
            let _ = std::fs::remove_file(&path);
            return Err(MhError::Other(format!(
                "The downloaded {} does not run on this machine (wrong CPU architecture or corrupt download). \
                 Please install ffmpeg via your system package manager instead.",
                bin
            )));
        }
    }
    Ok(())
}

/// macOS is absent from the platform table on purpose. ffmpeg and ffprobe arrive as two
/// separate archives, from URLs that exist only once an RSS feed (evermeet) or a listing
/// page (osxexperts) has been read — so the table driver would need an async URL resolver
/// and a per-binary archive mode, about 25 lines of machinery to save 15.
#[cfg(target_os = "macos")]
pub async fn download_and_install_ffmpeg<F>(on_progress: F, force: bool) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    on_progress(0, "Starting FFmpeg download…");
    if !force && detect().is_some() {
        on_progress(100, "FFmpeg is already available — no download needed.");
        return Ok(());
    }

    let ffmpeg_dir = crate::venv_manager::managed_bin_dir()
        .ok_or_else(|| MhError::Other("Cannot determine data directory".to_string()))?;
    std::fs::create_dir_all(&ffmpeg_dir)?;

    install_macos(&ffmpeg_dir, &on_progress).await?;
    crate::venv_manager::invalidate_binary_cache();

    on_progress(90, "Verifying FFmpeg…");
    smoke_test_ffmpeg(&ffmpeg_dir).await?;
    on_progress(100, "FFmpeg installed successfully!");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub async fn download_and_install_ffmpeg<F>(on_progress: F, force: bool) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    super::install_single_binary(&SPEC, &on_progress, force).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_for(os: &str, arch: &str) -> Option<&'static Target> {
        SPEC.targets.iter().find(|t| t.os == os && t.arch == arch)
    }

    #[test]
    fn every_target_yields_both_binaries() {
        for t in SPEC.targets {
            assert_eq!(t.binary_names.len(), 2, "{}-{}", t.os, t.arch);
            assert!(!t.urls.is_empty(), "{}-{}", t.os, t.arch);
        }
    }

    #[test]
    fn linux_x64_and_arm64_have_a_mirror_each() {
        assert_eq!(target_for("linux", "x86_64").unwrap().urls.len(), 2);
        assert_eq!(target_for("linux", "aarch64").unwrap().urls.len(), 2);
    }

    #[test]
    fn macos_is_not_in_the_table() {
        assert!(target_for("macos", "aarch64").is_none());
        assert!(target_for("macos", "x86_64").is_none());
    }

    #[test]
    fn validate_rejects_html_body() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("x.tar.xz");
        let mut body = b"<!DOCTYPE html><html>mirror error</html>".to_vec();
        body.resize(200_000, b' ');
        std::fs::write(&p, &body).unwrap();
        assert!(super::super::validate_download(
            &p,
            MIN_ARCHIVE_BYTES,
            "FFmpeg",
            super::super::XZ_MAGICS
        )
        .is_err());
    }

    #[test]
    fn validate_accepts_valid_xz_magic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("x.tar.xz");
        let mut body = super::super::XZ_MAGICS[0].to_vec();
        body.resize(200_000, 0);
        std::fs::write(&p, &body).unwrap();
        assert!(super::super::validate_download(
            &p,
            MIN_ARCHIVE_BYTES,
            "FFmpeg",
            super::super::XZ_MAGICS
        )
        .is_ok());
    }

    #[test]
    fn validate_accepts_zip_magic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("x.zip");
        let mut body = b"PK\x03\x04".to_vec();
        body.resize(200_000, 0);
        std::fs::write(&p, &body).unwrap();
        assert!(super::super::validate_download(
            &p,
            MIN_ARCHIVE_BYTES,
            "FFmpeg",
            super::super::ZIP_MAGICS
        )
        .is_ok());
    }
}
