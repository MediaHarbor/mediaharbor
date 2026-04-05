#[allow(unused_imports)]
use std::path::{Path, PathBuf};
use tempfile::TempDir;
#[cfg(target_os = "linux")]
use tokio::fs;
use tokio::process::Command;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_mozilla_client, download_to_file};

#[cfg(target_os = "macos")]
const FFMPEG_RSS: &str = "https://evermeet.cx/ffmpeg/rss.xml";
#[cfg(target_os = "macos")]
const FFPROBE_RSS: &str = "https://evermeet.cx/ffmpeg/ffprobe-rss.xml";

#[cfg(target_os = "windows")]
const WIN_X64_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

#[cfg(target_os = "linux")]
const LINUX_X64_URL: &str =
    "https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-amd64-static.tar.xz";
#[cfg(target_os = "linux")]
const LINUX_ARM64_URL: &str =
    "https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-arm64-static.tar.xz";
#[cfg(target_os = "linux")]
const LINUX_ARM_URL: &str =
    "https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-armhf-static.tar.xz";

#[cfg(target_os = "macos")]
struct RssItem {
    version: String,
    url: String,
}

#[cfg(target_os = "macos")]
async fn fetch_latest_from_rss(feed_url: &str, label: &str) -> MhResult<RssItem> {
    let client = build_mozilla_client()?;
    let xml = client
        .get(feed_url)
        .send()
        .await?
        .text()
        .await?;

    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut in_item = false;
    let mut in_title = false;
    let mut in_link = false;
    let mut title = String::new();
    let mut link = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name().as_ref() {
                    b"item" => in_item = true,
                    b"title" if in_item => in_title = true,
                    b"link" if in_item => in_link = true,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_title && in_item {
                    title = text;
                    in_title = false;
                } else if in_link && in_item {
                    link = text;
                    in_link = false;
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"item" {
                    break; // first item processed
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
        buf.clear();
    }

    if title.is_empty() || link.is_empty() {
        return Err(MhError::Other(format!(
            "Malformed RSS response for {}",
            label
        )));
    }

    Ok(RssItem {
        version: title.trim_end_matches(".zip").to_string(),
        url: link,
    })
}

#[allow(unused_variables)]
pub async fn update_system_path<F: Fn(u8, &str)>(
    bin_dir: &Path,
    on_progress: &F,
) -> MhResult<()> {
    let bin_dir_str = bin_dir.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        let ps_cmd = format!(
            r#"
$current = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($current -notlike '*{dir}*') {{
    [Environment]::SetEnvironmentVariable('Path', '{dir};' + $current, 'User')
}}
"#,
            dir = bin_dir_str.replace('\'', "''")
        );

        let status = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
            .status()
            .await
            .map_err(|e| MhError::Subprocess(e.to_string()))?;

        if !status.success() {
            on_progress(85, "PATH update failed; FFmpeg will work after restart.");
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        let shell = std::env::var("SHELL").unwrap_or_default();

        let rc_path: PathBuf = if shell.contains("zsh") {
            home.join(".zshrc")
        } else if shell.contains("bash") {
            #[cfg(target_os = "macos")]
            {
                let bp = home.join(".bash_profile");
                if bp.exists() { bp } else { home.join(".bashrc") }
            }
            #[cfg(not(target_os = "macos"))]
            home.join(".bashrc")
        } else if shell.contains("fish") {
            let fish_dir = home.join(".config").join("fish");
            let _ = std::fs::create_dir_all(&fish_dir);
            let fish_rc = fish_dir.join("config.fish");
            let existing = std::fs::read_to_string(&fish_rc).unwrap_or_default();
            if !existing.contains(&bin_dir_str) {
                let line = format!("\nset -gx PATH \"{}\" $PATH\n", bin_dir_str);
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&fish_rc)?;
                f.write_all(line.as_bytes())?;
            }
            return Ok(());
        } else {
            home.join(".profile")
        };

        let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();
        if !existing.contains(&bin_dir_str) {
            let line = format!("\nexport PATH=\"{}:$PATH\"\n", bin_dir_str);
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&rc_path)?;
            f.write_all(line.as_bytes())?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
async fn install_macos<F: Fn(u8, &str)>(ffmpeg_dir: &Path, on_progress: &F) -> MhResult<()> {
    let client = build_mozilla_client()?;

    on_progress(0, "Fetching FFmpeg RSS…");
    let ff = fetch_latest_from_rss(FFMPEG_RSS, "FFmpeg").await?;

    let tmp_ff = TempDir::new()?;
    let ff_zip = tmp_ff.path().join(
        Path::new(&ff.url)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("ffmpeg.zip")),
    );

    download_to_file(&client, &ff.url, &ff_zip, |dl, total| {
        let pct = total.map(|t| (dl * 25 / t) as u8).unwrap_or(0);
        on_progress(pct, &format!("Downloading FFmpeg… {}%", pct * 4));
    })
    .await?;

    on_progress(25, "Extracting FFmpeg…");
    extract_zip_single_binary(&ff_zip, ffmpeg_dir, "ffmpeg").await?;

    let ffmpeg_bin = ffmpeg_dir.join("ffmpeg");
    set_executable(&ffmpeg_bin)?;

    on_progress(25, "Fetching FFprobe RSS…");
    let fp = fetch_latest_from_rss(FFPROBE_RSS, "FFprobe").await?;

    let tmp_fp = TempDir::new()?;
    let fp_zip = tmp_fp.path().join(
        Path::new(&fp.url)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("ffprobe.zip")),
    );

    download_to_file(&client, &fp.url, &fp_zip, |dl, total| {
        let pct = 25 + total.map(|t| (dl * 25 / t) as u8).unwrap_or(0);
        on_progress(pct, &format!("Downloading FFprobe… {}%", (pct - 25) * 4));
    })
    .await?;

    on_progress(50, "Extracting FFprobe…");
    extract_zip_single_binary(&fp_zip, ffmpeg_dir, "ffprobe").await?;

    let ffprobe_bin = ffmpeg_dir.join("ffprobe");
    set_executable(&ffprobe_bin)?;

    Ok(())
}

#[cfg(target_os = "macos")]
async fn extract_zip_single_binary(
    zip_path: &Path,
    dest_dir: &Path,
    binary_name: &str,
) -> MhResult<()> {
    let zip_bytes = std::fs::read(zip_path)?;
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| MhError::Other(format!("zip open: {}", e)))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| MhError::Other(format!("zip entry: {}", e)))?;
        let name = entry.name().to_string();
        let file_name = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if file_name == binary_name || file_name.is_empty() {
            continue; // skip directories
        }
        let out_path = dest_dir.join(&file_name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
async fn install_windows<F: Fn(u8, &str)>(
    download_url: &str,
    ffmpeg_dir: &Path,
    on_progress: &F,
) -> MhResult<()> {
    let client = build_mozilla_client()?;
    let tmp = TempDir::new()?;
    let zip_path = tmp.path().join("ffmpeg.zip");

    download_to_file(&client, download_url, &zip_path, |dl, total| {
        let pct = total.map(|t| (dl * 50 / t) as u8).unwrap_or(0);
        on_progress(pct, &format!("Downloading FFmpeg… {}%", pct * 2));
    })
    .await?;

    on_progress(50, "Extracting FFmpeg…");

    let zip_bytes = std::fs::read(&zip_path)?;
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| MhError::Other(format!("zip open: {}", e)))?;

    let mut bin_entries: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| MhError::Other(format!("zip entry: {}", e)))?;
        let name = entry.name().to_string();
        if name.contains("/bin/") || name.contains("\\bin\\") {
            bin_entries.push(name);
        }
    }

    let zip_bytes2 = std::fs::read(&zip_path)?;
    let reader2 = std::io::Cursor::new(zip_bytes2);
    let mut archive2 = zip::ZipArchive::new(reader2)
        .map_err(|e| MhError::Other(format!("zip open 2: {}", e)))?;

    let mut found_bin = false;
    for i in 0..archive2.len() {
        let mut entry = archive2
            .by_index(i)
            .map_err(|e| MhError::Other(format!("zip entry 2: {}", e)))?;
        let name = entry.name().to_string();
        let path = Path::new(&name);

        let components: Vec<_> = path.components().collect();
        let is_bin_entry = components.iter().any(|c| {
            c.as_os_str().to_string_lossy().to_lowercase() == "bin"
        });

        if !is_bin_entry {
            continue;
        }

        let file_name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        if file_name.is_empty() {
            continue;
        }

        let out_path = ffmpeg_dir.join(&file_name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        found_bin = true;
    }

    if !found_bin {
        return Err(MhError::Other(
            "Could not find bin directory in extracted FFmpeg archive".to_string(),
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
async fn install_linux<F: Fn(u8, &str)>(
    download_url: &str,
    ffmpeg_dir: &Path,
    on_progress: &F,
) -> MhResult<()> {
    let client = build_mozilla_client()?;
    let tmp = TempDir::new()?;
    let tar_path = tmp.path().join("ffmpeg.tar.xz");

    download_to_file(&client, download_url, &tar_path, |dl, total| {
        let pct = total.map(|t| (dl * 50 / t) as u8).unwrap_or(0);
        on_progress(pct, &format!("Downloading FFmpeg… {}%", pct * 2));
    })
    .await?;

    on_progress(50, "Extracting FFmpeg…");

    let status = Command::new("tar")
        .args([
            "-xJf",
            tar_path.to_str().unwrap_or(""),
            "-C",
            tmp.path().to_str().unwrap_or(""),
        ])
        .status()
        .await
        .map_err(|e| MhError::Subprocess(format!("Failed to spawn tar: {}", e)))?;

    if !status.success() {
        return Err(MhError::Subprocess("tar extraction failed".to_string()));
    }

    let tar_file_name = tar_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let mut extracted_dir: Option<PathBuf> = None;
    let mut rd = fs::read_dir(tmp.path()).await?;
    while let Some(entry) = rd.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("ffmpeg") && name != tar_file_name {
            extracted_dir = Some(entry.path());
            break;
        }
    }

    let extracted_dir = extracted_dir
        .ok_or_else(|| MhError::Other("FFmpeg folder not found after extraction".to_string()))?;

    for binary in &["ffmpeg", "ffprobe"] {
        let src = extracted_dir.join(binary);
        let dest = ffmpeg_dir.join(binary);
        if !src.exists() {
            return Err(MhError::Other(format!(
                "Expected binary \"{}\" not found in archive",
                binary
            )));
        }
        fs::copy(&src, &dest).await?;
        set_executable(&dest)?;
    }

    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> MhResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn resolve_arch() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    return "x64";
    #[cfg(target_arch = "aarch64")]
    return "arm64";
    #[cfg(target_arch = "arm")]
    return "arm";
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "arm")))]
    return "x64";
}

#[cfg(target_os = "linux")]
fn get_linux_url() -> Option<&'static str> {
    match resolve_arch() {
        "x64" => Some(LINUX_X64_URL),
        "arm64" => Some(LINUX_ARM64_URL),
        "arm" => Some(LINUX_ARM_URL),
        _ => None,
    }
}

pub async fn download_and_install_ffmpeg<F>(on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    on_progress(0, "Starting FFmpeg download…");

    let home = dirs::home_dir()
        .ok_or_else(|| MhError::Other("Cannot determine home directory".to_string()))?;

    #[cfg(target_os = "windows")]
    let ffmpeg_dir = home.join("ffmpeg").join("bin");
    #[cfg(not(target_os = "windows"))]
    let ffmpeg_dir = home.join(".local").join("bin");

    std::fs::create_dir_all(&ffmpeg_dir)?;

    #[cfg(target_os = "macos")]
    install_macos(&ffmpeg_dir, &on_progress).await?;

    #[cfg(target_os = "windows")]
    install_windows(WIN_X64_URL, &ffmpeg_dir, &on_progress).await?;

    #[cfg(target_os = "linux")]
    {
        let url = get_linux_url()
            .ok_or_else(|| MhError::Unsupported("Unsupported Linux architecture".to_string()))?;
        install_linux(url, &ffmpeg_dir, &on_progress).await?;
    }

    on_progress(80, "Updating system PATH…");
    update_system_path(&ffmpeg_dir, &on_progress).await?;

    on_progress(
        100,
        "FFmpeg installed successfully! Please restart your terminal if FFmpeg commands are not working.",
    );

    Ok(())
}
