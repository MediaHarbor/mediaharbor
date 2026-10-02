use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::errors::{MhError, MhResult};

/// Kills the ffmpeg child if the future driving it is dropped. Download
/// cancellation works by dropping the task, so without this the transcode would
/// keep running (and keep writing) after the user pressed cancel.
struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.start_kill();
        }
    }
}

/// `1080p`, `1080`, `720P` → 1080 / 1080 / 720.
pub fn parse_height(spec: &str) -> Option<u32> {
    let cleaned: String = spec
        .trim()
        .to_ascii_lowercase()
        .trim_end_matches('p')
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    cleaned.parse().ok().filter(|h| *h > 0)
}

/// Reads `key=value` lines from ffmpeg's `-progress` stream and converts
/// `out_time_us` into a fraction of the known duration.
fn progress_fraction(line: &str, total_secs: u64) -> Option<(u64, u64)> {
    let (key, value) = line.split_once('=')?;
    let done_secs = match key.trim() {
        "out_time_us" | "out_time_ms" => {
            let raw: u64 = value.trim().parse().ok()?;
            raw / 1_000_000
        }
        _ => return None,
    };
    let total = total_secs.max(1);
    Some((done_secs.min(total), total))
}

/// Runs ffmpeg with real progress reporting instead of a single tick once the
/// whole file has already been written.
pub async fn run_ffmpeg_with_progress(
    input_url: &str,
    dest_path: &Path,
    total_secs: u64,
    on_progress: &impl Fn(u64, u64),
) -> MhResult<()> {
    let ffmpeg = crate::venv_manager::resolve_ffmpeg();
    let mut cmd = Command::new(&ffmpeg);
    cmd.args([
        "-y",
        "-loglevel",
        "error",
        "-user_agent",
        crate::http_client::UA_MOZILLA,
        "-i",
        input_url,
        "-c",
        "copy",
        "-progress",
        "pipe:1",
        "-nostats",
    ])
    .arg(dest_path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    crate::subprocess::apply_no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| MhError::Subprocess(format!("ffmpeg spawn failed: {e}")))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut guard = ChildGuard(Some(child));

    if let Some(stdout) = stdout {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            if let Some((done, total)) = progress_fraction(&line, total_secs) {
                on_progress(done, total);
            }
        }
    }

    let mut err_text = String::new();
    if let Some(stderr) = stderr {
        let mut lines = BufReader::new(stderr).lines();
        while let Some(line) = lines.next_line().await? {
            err_text.push_str(&line);
            err_text.push('\n');
        }
    }

    let status = guard
        .0
        .as_mut()
        .expect("child taken")
        .wait()
        .await
        .map_err(|e| MhError::Subprocess(format!("ffmpeg wait failed: {e}")))?;
    guard.0 = None;

    if !status.success() {
        return Err(MhError::Subprocess(format!(
            "ffmpeg failed (exit {}): {}",
            status.code().unwrap_or(-1),
            err_text.trim()
        )));
    }
    on_progress(total_secs.max(1), total_secs.max(1));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_parse_from_every_spelling() {
        assert_eq!(parse_height("1080p"), Some(1080));
        assert_eq!(parse_height("720"), Some(720));
        assert_eq!(parse_height(" 480P "), Some(480));
        assert_eq!(parse_height("best"), None);
        assert_eq!(parse_height(""), None);
    }

    #[test]
    fn out_time_becomes_a_fraction_of_the_duration() {
        assert_eq!(
            progress_fraction("out_time_us=30000000", 120),
            Some((30, 120))
        );
        assert_eq!(
            progress_fraction("out_time_ms=60000000", 120),
            Some((60, 120))
        );
        assert_eq!(progress_fraction("frame=12", 120), None);
    }

    #[test]
    fn progress_never_exceeds_the_total() {
        assert_eq!(
            progress_fraction("out_time_us=999000000", 10),
            Some((10, 10))
        );
    }

    #[test]
    fn an_unknown_duration_still_yields_a_usable_ratio() {
        assert_eq!(progress_fraction("out_time_us=5000000", 0), Some((1, 1)));
    }
}
