use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use crate::{
    defaults::Settings,
    errors::{MhError, MhResult},
    ipc_contract::{BackendLogEvent, DownloadProgressEvent, ProcessStdinPromptEvent},
    services::common::cli::driver::{
        drive, IdleAction, LineEffect, ProcessDriver, Prompts, Running,
    },
    subprocess::{run_to_completion, spawn_with_output_opts, strip_ansi, LineMode, LineSource},
    venv_manager, EventEmitter,
};

pub const ORPHEUS_GIT_URL: &str = "https://github.com/OrfiTeam/OrpheusDL";

pub const KNOWN_MODULES: &[(&str, &str, &str)] = &[
    (
        "tidal",
        "Tidal",
        "https://github.com/Dniel97/orpheusdl-tidal",
    ),
    (
        "qobuz",
        "Qobuz",
        "https://github.com/OrfiDev/orpheusdl-qobuz",
    ),
    (
        "deezer",
        "Deezer",
        "https://github.com/uhwot/orpheusdl-deezer",
    ),
    (
        "soundcloud",
        "SoundCloud",
        "https://github.com/OrfiDev/orpheusdl-soundcloud",
    ),
    (
        "napster",
        "Napster",
        "https://github.com/OrfiDev/orpheusdl-napster",
    ),
    (
        "beatport",
        "Beatport",
        "https://github.com/Dniel97/orpheusdl-beatport",
    ),
    (
        "nugs",
        "Nugs.net",
        "https://github.com/Dniel97/orpheusdl-nugs",
    ),
    ("kkbox", "KKBox", "https://github.com/uhwot/orpheusdl-kkbox"),
    (
        "bugs",
        "Bugs! Music",
        "https://github.com/Dniel97/orpheusdl-bugsmusic",
    ),
    (
        "idagio",
        "Idagio",
        "https://github.com/Dniel97/orpheusdl-idagio",
    ),
    (
        "jiosaavn",
        "JioSaavn",
        "https://github.com/bunnykek/orpheusdl-jiosaavn",
    ),
];

pub fn get_orpheus_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mediaharbor")
        .join("orpheusdl")
}

pub fn get_orpheus_venv_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mediaharbor")
        .join("orpheus_venv")
}

pub fn get_orpheus_python() -> PathBuf {
    let venv = get_orpheus_venv_dir();
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

pub fn is_orpheus_installed() -> bool {
    get_orpheus_dir().join("orpheus.py").exists() && get_orpheus_python().exists()
}

pub fn is_module_installed(id: &str) -> bool {
    get_orpheus_dir().join("modules").join(id).exists()
}

pub async fn install_orpheus<F: Fn(u8, &str) + Send>(progress: F) -> MhResult<()> {
    if crate::sandbox::is_sandboxed() {
        return Err(MhError::Unsupported(
            "OrpheusDL cannot be installed inside the Flatpak/Snap sandbox — it clones a git \
             repository and installs into a separate Python environment, which the sandbox \
             blocks. Use MediaHarbor's native backends, or install the desktop build outside \
             the sandbox."
                .to_string(),
        ));
    }

    let system_python = venv_manager::find_system_python().await?;
    let venv_dir = get_orpheus_venv_dir();
    let orpheus_dir = get_orpheus_dir();

    let venv_str = venv_dir
        .to_str()
        .ok_or_else(|| {
            MhError::Other(
                "OrpheusDL's data path contains invalid (non-UTF-8) characters.".to_string(),
            )
        })?
        .to_string();

    progress(5, "Creating virtual environment");
    run_to_completion(&system_python, &["-m", "venv", &venv_str], None, None).await?;

    let python = get_orpheus_python();
    let python_str = python.to_str().unwrap_or("python").to_string();

    // This venv gets its own certifi: the python.org build MediaHarbor installs on
    // macOS ships no CA store, so cloning and pip-installing below would otherwise
    // fail TLS verification. Best effort — the OS bundle is still a fallback.
    run_to_completion(
        &python_str,
        &["-m", "pip", "install", "--upgrade", "certifi"],
        None,
        None,
    )
    .await
    .ok();
    crate::venv_manager::invalidate_ca_bundle_cache();

    progress(15, "Cloning OrpheusDL");
    if orpheus_dir.join(".git").exists() {
        git_pull(&orpheus_dir).await?;
    } else {
        if orpheus_dir.exists() {
            tokio::fs::remove_dir_all(&orpheus_dir).await.ok();
        }
        git_clone(ORPHEUS_GIT_URL, &orpheus_dir).await?;
    }

    progress(50, "Installing requirements");
    let req_path = orpheus_dir.join("requirements.txt");
    if req_path.exists() {
        let req_str = req_path.to_str().unwrap_or("").to_string();
        run_to_completion(
            &python_str,
            &["-m", "pip", "install", "-r", &req_str],
            None,
            None,
        )
        .await?;
    }

    progress(90, "Creating directories");
    tokio::fs::create_dir_all(orpheus_dir.join("modules")).await?;
    tokio::fs::create_dir_all(orpheus_dir.join("config")).await?;

    progress(95, "Refreshing settings");
    run_to_completion(
        &python_str,
        &["orpheus.py", "settings", "refresh"],
        None,
        Some(&orpheus_dir),
    )
    .await
    .ok();

    progress(100, "Done");
    Ok(())
}

pub async fn install_module<F: Fn(u8, &str)>(
    module_id: &str,
    git_url: &str,
    progress: F,
) -> MhResult<()> {
    let orpheus_dir = get_orpheus_dir();
    let module_dir = orpheus_dir.join("modules").join(module_id);
    let python = get_orpheus_python();
    let python_str = python.to_str().unwrap_or("python").to_string();

    if module_dir.join(".git").exists() {
        progress(10, "Updating module");
        git_pull(&module_dir).await?;
    } else {
        if module_dir.exists() {
            tokio::fs::remove_dir_all(&module_dir).await.ok();
        }
        progress(10, "Cloning module");
        if module_id == "tidal" || module_id == "nugs" {
            git_clone_recursive(git_url, &module_dir).await?;
        } else {
            git_clone(git_url, &module_dir).await?;
        }
    }

    progress(70, "Installing module requirements");
    let req_path = module_dir.join("requirements.txt");
    if req_path.exists() {
        let req_str = req_path.to_str().unwrap_or("").to_string();
        run_to_completion(
            &python_str,
            &["-m", "pip", "install", "-r", &req_str],
            None,
            None,
        )
        .await?;
    }

    progress(90, "Refreshing settings");
    run_to_completion(
        &python_str,
        &["orpheus.py", "settings", "refresh"],
        None,
        Some(&orpheus_dir),
    )
    .await
    .ok();

    progress(100, "Done");
    Ok(())
}

pub fn map_quality(platform: &str, settings: &Settings) -> &'static str {
    match platform {
        "tidal" => match settings.tidal_quality {
            0 => "minimum",
            1 => "high",
            2 => "lossless",
            _ => "hifi",
        },
        "qobuz" => match settings.qobuz_quality {
            5 => "high",
            6 => "lossless",
            _ => "hifi",
        },
        "deezer" => {
            if settings.deezer_quality == "FLAC" {
                "lossless"
            } else {
                "high"
            }
        }
        _ => "hifi",
    }
}

/// Default `settings.json` entries per OrpheusDL module. Seeded only when the key
/// is absent, so a value the user edited by hand is never overwritten.
#[rustfmt::skip]
const MODULE_DEFAULTS: &[(&str, &[(&str, &str)])] = &[
    ("deezer", &[
        ("client_id", "447462"), ("client_secret", "a83bf7f38ad2f137e444727cfc3775cf"),
        ("bf_secret", ""), ("email", ""), ("password", ""),
    ]),
    ("qobuz", &[
        ("app_id", ""), ("app_secret", ""), ("quality_format", "{sample_rate}"),
        ("username", ""), ("password", ""),
    ]),
    ("soundcloud", &[("web_access_token", "")]),
    ("napster", &[
        ("api_key", ""), ("customer_secret", ""), ("requested_netloc", ""),
        ("username", ""), ("password", ""),
    ]),
    ("beatport", &[("username", ""), ("password", "")]),
    ("nugs", &[
        ("username", ""), ("password", ""),
        ("client_id", "Eg7HuH873H65r5rt325UytR5429"), ("dev_key", "x7f54tgbdyc64y656thy47er4"),
    ]),
    ("kkbox", &[
        ("kc1_key", ""), ("secret_key", ""), ("email", ""), ("password", ""),
    ]),
    ("bugs", &[("username", ""), ("password", "")]),
    ("idagio", &[("username", ""), ("password", "")]),
];

fn apply_credentials(root: &mut serde_json::Value, settings: &Settings) {
    if root["modules"].as_object().is_none() {
        root["modules"] = serde_json::json!({});
    }

    let mut set = |module: &str, key: &str, val: &str, only_if_absent: bool| {
        let modules = &mut root["modules"];
        if modules[module].as_object().is_none() {
            modules[module] = serde_json::json!({});
        }
        if !only_if_absent || modules[module][key].is_null() {
            modules[module][key] = serde_json::Value::String(val.to_string());
        }
    };

    for (module, defaults) in MODULE_DEFAULTS {
        for (key, val) in *defaults {
            set(module, key, val, true);
        }
    }

    if !settings.qobuz_email_or_userid.is_empty() {
        set("qobuz", "username", &settings.qobuz_email_or_userid, false);
        set(
            "qobuz",
            "password",
            &settings.qobuz_password_or_token,
            false,
        );
        let qobuz_pair = crate::services::qobuz::app_credentials::configured_pair(settings);
        let (qobuz_app_id, qobuz_app_secret) =
            qobuz_pair.map(|p| (p.app_id, p.secret)).unwrap_or_default();
        set("qobuz", "app_id", &qobuz_app_id, false);
        set("qobuz", "app_secret", &qobuz_app_secret, false);
    }
}

pub async fn write_settings_json(
    settings: &Settings,
    output_dir: &str,
    quality: &str,
) -> MhResult<()> {
    let orpheus_dir = get_orpheus_dir();
    let config_dir = orpheus_dir.join("config");
    tokio::fs::create_dir_all(&config_dir).await?;
    let settings_path = config_dir.join("settings.json");

    let mut root = if settings_path.exists() {
        let raw = tokio::fs::read_to_string(&settings_path)
            .await
            .map_err(|e| MhError::Other(format!("Failed to read settings.json: {}", e)))?;
        serde_json::from_str(&raw).unwrap_or_else(
            |_| serde_json::json!({"global": {"general": {}}, "extensions": {}, "modules": {}}),
        )
    } else {
        serde_json::json!({"global": {"general": {}}, "extensions": {}, "modules": {}})
    };

    root["global"]["general"]["download_path"] = serde_json::Value::String(output_dir.to_string());
    root["global"]["general"]["download_quality"] = serde_json::Value::String(quality.to_string());

    apply_credentials(&mut root, settings);

    let json_str = serde_json::to_string_pretty(&root)
        .map_err(|e| MhError::Other(format!("Failed to serialize settings.json: {}", e)))?;
    tokio::fs::write(&settings_path, json_str).await?;
    Ok(())
}

fn emit_log(emitter: &Arc<dyn EventEmitter>, level: &str, message: &str) {
    emitter.emit_log(&BackendLogEvent::new(
        level,
        "orpheusdl",
        "OrpheusDL",
        message.to_string(),
    ));
}

fn emit_consolidated_log(
    emitter: &Arc<dyn EventEmitter>,
    level: &str,
    title: &str,
    lines: &[String],
) {
    emitter.emit_log(&BackendLogEvent::new(
        level,
        "orpheusdl",
        title,
        lines.join("\n"),
    ));
}

fn emit_error_progress(emitter: &Arc<dyn EventEmitter>, download_id: u64, msg: &str) {
    emitter.emit_progress(&DownloadProgressEvent {
        download_id,
        percent: 0.0,
        speed: None,
        eta: None,
        status: format!("error: {}", msg),
        item_index: None,
        item_total: None,
        quality: None,
    });
}

fn is_prompt_indicator(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("choose a login method")
        || lower.contains("login method:")
        || lower.contains("enter your username")
        || lower.contains("enter your password")
        || lower.contains("enter the code")
        || lower.contains("choose a method")
        || (lower.contains("choose") && lower.ends_with(':'))
}

pub async fn run_orpheus_download(
    url: &str,
    output_dir: &str,
    platform: &str,
    download_id: u64,
    settings: &Settings,
    cancelled: Arc<AtomicBool>,
    emitter: Arc<dyn EventEmitter>,
    mut stdin_rx: tokio::sync::mpsc::Receiver<String>,
) -> MhResult<()> {
    let quality = map_quality(platform, settings);
    let mut log_buf: Vec<String> = Vec::new();
    let log_title = format!(
        "OrpheusDL: {}",
        crate::services::common::pipeline::truncate_str_bytes(url, 60)
    );

    log_buf.push(format!(
        "Starting OrpheusDL download: url={} platform={} output_dir={} quality={}",
        url, platform, output_dir, quality
    ));
    log_buf.push(format!(
        "orpheus_installed={} module_installed={}",
        is_orpheus_installed(),
        is_module_installed(platform)
    ));

    let python = get_orpheus_python();
    let orpheus_dir = get_orpheus_dir();
    log_buf.push(format!(
        "python={} cwd={}",
        python.display(),
        orpheus_dir.display()
    ));

    if let Err(e) = write_settings_json(settings, output_dir, quality).await {
        let msg = format!("Failed to write settings.json: {}", e);
        emit_log(&emitter, "error", &msg);
        emit_error_progress(&emitter, download_id, &msg);
        return Err(e);
    }
    log_buf.push("settings.json written successfully".to_string());

    let python_str = python.to_str().unwrap_or("python").to_string();
    let url_owned = url.to_string();
    let output_owned = output_dir.to_string();
    let orpheus_dir_clone = orpheus_dir.clone();

    log_buf.push(format!(
        "Spawning: {} -u orpheus.py {} -o {}",
        python_str, url_owned, output_owned
    ));

    let (tx, mut rx) = tokio::sync::mpsc::channel::<(LineSource, String)>(256);
    let mut handle = match spawn_with_output_opts(
        &python_str,
        &["-u", "orpheus.py", &url_owned, "-o", &output_owned],
        None,
        Some(&orpheus_dir_clone),
        tx,
        LineMode::NewlineOrCarriageReturn,
    )
    .await
    {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("Failed to spawn orpheus.py: {}", e);
            log_buf.push(msg.clone());
            emit_consolidated_log(&emitter, "error", &log_title, &log_buf);
            emit_error_progress(&emitter, download_id, &msg);
            return Err(e);
        }
    };

    log_buf.push("Process spawned, reading output...".to_string());

    let ask = {
        let emitter = emitter.clone();
        move |prompt_lines: Vec<String>| {
            emitter.emit_stdin_prompt(&ProcessStdinPromptEvent {
                download_id,
                prompt_lines,
            });
        }
    };
    let mut driver = OrpheusDriver::default();
    let result = drive(
        &mut driver,
        Running::new(&mut handle, &mut rx, PROMPT_DEBOUNCE).answered_by(Prompts {
            replies: &mut stdin_rx,
            ask: &ask,
        }),
        &cancelled,
        |percent| {
            emitter.emit_progress(&DownloadProgressEvent {
                download_id,
                percent,
                speed: None,
                eta: None,
                status: "downloading".into(),
                item_index: None,
                item_total: None,
                quality: None,
            });
        },
        |_| {},
    )
    .await;

    log_buf.extend(driver.log);

    if let Err(e) = result {
        let level = if matches!(e, MhError::Cancelled) {
            "info"
        } else {
            emit_error_progress(&emitter, download_id, &e.to_string());
            "error"
        };
        log_buf.push(e.to_string());
        emit_consolidated_log(&emitter, level, &log_title, &log_buf);
        return Err(e);
    }

    emit_consolidated_log(&emitter, "info", &log_title, &log_buf);
    emitter.emit_progress(&DownloadProgressEvent {
        download_id,
        percent: 100.0,
        speed: None,
        eta: None,
        status: "completed".into(),
        item_index: None,
        item_total: None,
        quality: None,
    });
    Ok(())
}

/// How long OrpheusDL must be silent before a half-printed question is taken as a
/// prompt it is now blocked on. It prints the question across several lines and then
/// simply waits, with no newline to mark the end.
const PROMPT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Silence that means the run is wedged rather than thinking.
const ORPHEUS_STALL: Duration = Duration::from_secs(300);

/// What OrpheusDL's output means. The loop feeding it is shared with every other
/// spawned downloader.
#[derive(Default)]
struct OrpheusDriver {
    /// OrpheusDL's log is emitted once at the end rather than streamed, so the lines
    /// are collected here instead of going out through `on_log`.
    log: Vec<String>,
    prompt_lines: Vec<String>,
    new_settings_detected: bool,
}

impl ProcessDriver for OrpheusDriver {
    type Progress = f32;

    fn feed(&mut self, source: LineSource, line: &str) -> LineEffect<f32> {
        let label = match source {
            LineSource::Stdout => "stdout",
            LineSource::Stderr => "stderr",
        };
        let clean = strip_ansi(line);
        let clean = clean.trim();
        self.log.push(format!("[{}] {}", label, clean));

        if clean.contains("New settings detected") {
            self.new_settings_detected = true;
        }

        // A question starts a block; the lines after it are its continuation, until the
        // output goes quiet and `on_idle` hands the whole block over.
        if is_prompt_indicator(clean) {
            self.prompt_lines.clear();
            self.prompt_lines.push(clean.to_string());
        } else if !self.prompt_lines.is_empty() && !clean.is_empty() {
            self.prompt_lines.push(clean.to_string());
        }

        let percent = parse_orpheus_progress(clean);
        if percent > 0.0 {
            LineEffect::important(percent)
        } else {
            LineEffect::None
        }
    }

    fn on_idle(&mut self, idle: Duration) -> IdleAction {
        if !self.prompt_lines.is_empty() {
            return IdleAction::Prompt(std::mem::take(&mut self.prompt_lines));
        }
        if idle > ORPHEUS_STALL {
            IdleAction::Stall(
                "OrpheusDL stopped responding (no output for 5 minutes) and was stopped.".into(),
            )
        } else {
            IdleAction::Wait
        }
    }

    fn finish(&mut self, exit_code: i32) -> MhResult<Option<f32>> {
        if self.new_settings_detected {
            return Err(MhError::Other(
                "OrpheusDL reset its settings.json — a newly installed module needs its credentials filled in via Settings → OrpheusDL, then retry the download".into(),
            ));
        }
        if exit_code != 0 {
            return Err(MhError::Subprocess(format!(
                "orpheus.py exited with code {}",
                exit_code
            )));
        }
        // The caller emits the completed event, which carries a status this cannot.
        Ok(None)
    }
}

pub async fn read_settings_json() -> MhResult<String> {
    let path = get_orpheus_dir().join("config").join("settings.json");
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| MhError::Other(format!("Failed to read settings.json: {}", e)))
}

pub async fn write_raw_settings_json(content: &str) -> MhResult<()> {
    let _: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| MhError::Other(format!("Invalid JSON: {}", e)))?;
    let config_dir = get_orpheus_dir().join("config");
    tokio::fs::create_dir_all(&config_dir).await?;
    tokio::fs::write(config_dir.join("settings.json"), content).await?;
    Ok(())
}

/// A depth-1 clone builder. These checkouts are only ever read as a working
/// tree — nothing inspects their history — so the rest of the log is download
/// and disk nobody spends.
fn shallow() -> git2::build::RepoBuilder<'static> {
    let mut fetch = git2::FetchOptions::new();
    fetch.depth(1);
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch);
    builder
}

async fn git_clone(url: &str, dest: &std::path::Path) -> MhResult<()> {
    let url = url.to_string();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || shallow().clone(&url, &dest).map(|_| ())).await??;
    Ok(())
}

async fn git_clone_recursive(url: &str, dest: &std::path::Path) -> MhResult<()> {
    let url = url.to_string();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), git2::Error> {
        let repo = shallow().clone(&url, &dest)?;
        for sub in repo.submodules()?.iter_mut() {
            sub.update(true, None)?;
        }
        Ok(())
    })
    .await??;
    Ok(())
}

async fn git_pull(repo_path: &std::path::Path) -> MhResult<()> {
    let repo_path = repo_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), git2::Error> {
        let repo = git2::Repository::open(&repo_path)?;
        repo.find_remote("origin")?
            .fetch(&[] as &[&str], None, None)?;
        let fetch_head = repo.find_reference("FETCH_HEAD")?;
        let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;
        let (analysis, _) = repo.merge_analysis(&[&fetch_commit])?;
        if analysis.is_fast_forward() {
            let head = repo.head()?;
            let refname = head.name().unwrap_or("refs/heads/main").to_string();
            repo.find_reference(&refname)?
                .set_target(fetch_commit.id(), "fast-forward")?;
            repo.set_head(&refname)?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
        }
        Ok(())
    })
    .await??;
    Ok(())
}

fn parse_orpheus_progress(line: &str) -> f32 {
    if let Some(pos) = line.find("Downloading track ") {
        let rest = &line[pos + "Downloading track ".len()..];
        if let Some(slash) = rest.find('/') {
            let current_str = rest[..slash].trim();
            let after_slash = &rest[slash + 1..];
            let total_str = after_slash
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("")
                .trim();
            if let (Ok(cur), Ok(tot)) = (current_str.parse::<f32>(), total_str.parse::<f32>()) {
                if tot > 0.0 {
                    return (cur / tot) * 100.0;
                }
            }
        }
    }
    0.0
}
