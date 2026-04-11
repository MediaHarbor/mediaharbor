use std::{
    process::Stdio,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};

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

pub async fn spawn_with_output(
    program: &str,
    args: &[&str],
    env: Option<Vec<(String, String)>>,
    cwd: Option<&std::path::Path>,
    tx: LineSender,
) -> MhResult<SubprocessHandle> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    if let Some(env_vars) = env {
        cmd.envs(env_vars);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| {
        MhError::Subprocess(format!("Failed to spawn `{}`: {}", program, e))
    })?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdin = child.stdin.take();

    let tx_out = tx.clone();
    let tx_err = tx.clone();

    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if tx_out.send((LineSource::Stdout, line)).await.is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if tx_err.send((LineSource::Stderr, line)).await.is_err() {
                break;
            }
        }
    });

    Ok(SubprocessHandle {
        child,
        cancelled: Arc::new(AtomicBool::new(false)),
        stdin,
    })
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

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    if let Some(vars) = env {
        cmd.envs(vars);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().await.map_err(|e| {
        MhError::Subprocess(format!("Failed to run `{}`: {}", program, e))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    Ok((stdout, stderr, code))
}

pub async fn probe_version(program: &str) -> Option<String> {
    let (stdout, stderr, code) =
        run_to_completion(program, &["--version"], None, None).await.ok()?;
    if code == 0 {
        let out = if stdout.is_empty() { &stderr } else { &stdout };
        Some(out.lines().next().unwrap_or("").to_string())
    } else {
        None
    }
}
