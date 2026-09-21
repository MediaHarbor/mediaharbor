use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tokio::{
    process::{Child, Command},
    sync::mpsc,
};

use std::sync::LazyLock;

use regex::Regex;

use crate::errors::{MhError, MhResult};

pub struct SubprocessHandle {
    pub child: Child,
    pub cancelled: Arc<AtomicBool>,
    pub stdin: Option<tokio::process::ChildStdin>,
}

impl SubprocessHandle {
    pub async fn kill(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let _ = self.child.kill().await;
    }
}

pub type LineSender = mpsc::Sender<(LineSource, String)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineSource {
    Stdout,
    Stderr,
}

/// How a child's byte stream is cut into lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMode {
    /// One line per `\n`, delivered as printed. Matches `BufReader::lines()`.
    Newline,
    /// Also cut on `\r`, then trim and drop blanks. The only way to see progress from
    /// a CLI that redraws its bar in place instead of printing a new line each tick.
    NewlineOrCarriageReturn,
}

fn pump_lines<R>(reader: R, source: LineSource, tx: LineSender, mode: LineMode)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = reader;
        let mut pending: Vec<u8> = Vec::new();
        // Bytes of `pending` already searched. Without it a line arriving in many
        // small reads is rescanned from the front on every one of them.
        let mut scanned = 0usize;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let mut start = 0;
                    // Terminators are ASCII, which never occurs inside a multi-byte
                    // UTF-8 sequence, so scanning bytes cannot split a character.
                    for i in scanned..pending.len() {
                        let b = pending[i];
                        if b != b'\n' && !(mode == LineMode::NewlineOrCarriageReturn && b == b'\r')
                        {
                            continue;
                        }
                        if let Some(line) = cut_line(&pending[start..i], mode) {
                            if tx.send((source, line)).await.is_err() {
                                return;
                            }
                        }
                        start = i + 1;
                    }
                    pending.drain(..start);
                    scanned = pending.len();
                }
            }
        }
        // A last line with no trailing terminator still matters: an error printed as
        // the process dies arrives that way. A trailing terminator leaves nothing here,
        // so unlike mid-stream a blank remainder is absence, not a printed blank line.
        if let Some(line) = cut_line(&pending, mode) {
            if !line.is_empty() {
                let _ = tx.send((source, line)).await;
            }
        }
    });
}

fn cut_line(raw: &[u8], mode: LineMode) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    match mode {
        // A blank line here was really printed, and `BufReader::lines()` yields it.
        LineMode::Newline => Some(text.strip_suffix('\r').unwrap_or(&text).to_string()),
        // In `\r` mode a blank segment is not a line at all, just the cursor being
        // parked back at column zero before the bar is redrawn.
        LineMode::NewlineOrCarriageReturn => {
            let line = text.trim();
            (!line.is_empty()).then(|| line.to_string())
        }
    }
}

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());

/// Drop terminal colour and cursor escapes so a line can be matched on or shown to the
/// user. Borrows when there is nothing to strip, which is the common case.
pub fn strip_ansi(line: &str) -> std::borrow::Cow<'_, str> {
    ANSI_RE.replace_all(line, "")
}

pub async fn spawn_with_output(
    program: &str,
    args: &[&str],
    env: Option<Vec<(String, String)>>,
    cwd: Option<&std::path::Path>,
    tx: LineSender,
) -> MhResult<SubprocessHandle> {
    spawn_with_output_opts(program, args, env, cwd, tx, LineMode::Newline).await
}

pub async fn spawn_with_output_opts(
    program: &str,
    args: &[&str],
    env: Option<Vec<(String, String)>>,
    cwd: Option<&std::path::Path>,
    tx: LineSender,
    mode: LineMode,
) -> MhResult<SubprocessHandle> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());

    apply_no_window(&mut cmd);

    // Applied first so a caller's own vars override it; harmless for the non-Python
    // programs (ffmpeg and friends) that also come through here.
    cmd.envs(crate::venv_manager::python_env());
    if let Some(env_vars) = env {
        cmd.envs(env_vars);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| MhError::Subprocess(format!("Failed to spawn `{}`: {}", program, e)))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdin = child.stdin.take();

    pump_lines(stdout, LineSource::Stdout, tx.clone(), mode);
    pump_lines(stderr, LineSource::Stderr, tx, mode);

    Ok(SubprocessHandle {
        child,
        cancelled: Arc::new(AtomicBool::new(false)),
        stdin,
    })
}

/// Spawn one of the managed Python downloaders out of MediaHarbor's venv.
///
/// The venv's `bin`/`Scripts` is prepended to `PATH` and `PYTHONPATH` is cleared so the
/// tool finds its own helpers rather than a system install, and output is cut on `\r`
/// too because these CLIs redraw their progress bar in place.
pub async fn spawn_venv_cli(
    command: &str,
    args: &[String],
    tx: LineSender,
) -> MhResult<SubprocessHandle> {
    // The same fallback chain yt-dlp uses: the venv console script, then `python -m
    // <tool>` out of that venv, then whatever is on PATH. A partial install can leave
    // the console script missing while the module itself still imports.
    let tool = crate::venv_manager::resolve_tool(command, command);

    let venv_dir = crate::venv_manager::get_venv_dir();
    let venv_bin = crate::venv_manager::venv_bin_dir();
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let new_path = if existing_path.is_empty() {
        venv_bin.to_string_lossy().to_string()
    } else {
        format!(
            "{}{}{}",
            venv_bin.to_string_lossy(),
            if cfg!(windows) { ";" } else { ":" },
            existing_path
        )
    };

    let env = vec![
        ("LANG".to_string(), "C.UTF-8".to_string()),
        ("LC_ALL".to_string(), "C.UTF-8".to_string()),
        (
            "VIRTUAL_ENV".to_string(),
            venv_dir.to_string_lossy().to_string(),
        ),
        ("PYTHONPATH".to_string(), String::new()),
        ("PATH".to_string(), new_path),
    ];

    let extra: Vec<&str> = args.iter().map(String::as_str).collect();
    spawn_with_output_opts(
        &tool.program,
        &tool.argv(&extra),
        Some(env),
        None,
        tx,
        LineMode::NewlineOrCarriageReturn,
    )
    .await
}

pub async fn run_to_completion(
    program: &str,
    args: &[&str],
    env: Option<Vec<(String, String)>>,
    cwd: Option<&std::path::Path>,
) -> MhResult<(String, String, i32)> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    apply_no_window(&mut cmd);

    cmd.envs(crate::venv_manager::python_env());
    if let Some(vars) = env {
        cmd.envs(vars);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("Failed to run `{}`: {}", program, e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    Ok((stdout, stderr, code))
}

#[cfg(target_os = "windows")]
pub fn apply_no_window(cmd: &mut Command) {
    cmd.creation_flags(0x08000000);
}

#[cfg(not(target_os = "windows"))]
pub fn apply_no_window(_cmd: &mut Command) {}

#[cfg(target_os = "windows")]
pub fn apply_no_window_std(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt as _;
    cmd.creation_flags(0x08000000);
}

#[cfg(not(target_os = "windows"))]
pub fn apply_no_window_std(_cmd: &mut std::process::Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(raw: &str, mode: LineMode) -> Option<String> {
        cut_line(raw.as_bytes(), mode)
    }

    #[test]
    fn newline_mode_matches_what_a_line_reader_yields() {
        // Printed blank lines are lines; a CRLF loses only its carriage return; nothing
        // else is trimmed, because yt-dlp's progress template is position-sensitive.
        assert_eq!(cut("", LineMode::Newline).as_deref(), Some(""));
        assert_eq!(cut("hello\r", LineMode::Newline).as_deref(), Some("hello"));
        assert_eq!(
            cut("  indented  ", LineMode::Newline).as_deref(),
            Some("  indented  ")
        );
    }

    #[test]
    fn carriage_return_mode_drops_the_cursor_parking_between_redraws() {
        assert_eq!(cut("", LineMode::NewlineOrCarriageReturn), None);
        assert_eq!(cut("   ", LineMode::NewlineOrCarriageReturn), None);
        assert_eq!(
            cut("  [download] 12%  ", LineMode::NewlineOrCarriageReturn).as_deref(),
            Some("[download] 12%")
        );
    }

    #[test]
    fn strip_ansi_borrows_when_there_is_nothing_to_strip() {
        assert!(matches!(
            strip_ansi("plain output"),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(
            strip_ansi("\x1b[1;32m[Track 1/4]\x1b[0m ok"),
            "[Track 1/4] ok"
        );
    }
}
