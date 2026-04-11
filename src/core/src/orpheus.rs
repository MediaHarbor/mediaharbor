use std::{
    path::PathBuf,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};
use chrono::Utc;
use tokio::io::AsyncWriteExt;

use crate::{
    defaults::Settings,
    errors::{MhError, MhResult},
    ipc_contract::{BackendLogEvent, DownloadProgressEvent, ProcessStdinPromptEvent},
    subprocess::{run_to_completion, spawn_with_output, LineSource},
    venv_manager,
    EventEmitter,
};

pub const ORPHEUS_GIT_URL: &str = "https://github.com/OrfiTeam/OrpheusDL";

pub const KNOWN_MODULES: &[(&str, &str, &str)] = &[
    ("tidal",      "Tidal",       "https://github.com/Dniel97/orpheusdl-tidal"),
    ("qobuz",      "Qobuz",       "https://github.com/OrfiDev/orpheusdl-qobuz"),
    ("deezer",     "Deezer",      "https://github.com/uhwot/orpheusdl-deezer"),
    ("soundcloud", "SoundCloud",  "https://github.com/OrfiDev/orpheusdl-soundcloud"),
    ("napster",    "Napster",     "https://github.com/OrfiDev/orpheusdl-napster"),
    ("beatport",   "Beatport",    "https://github.com/Dniel97/orpheusdl-beatport"),
    ("nugs",       "Nugs.net",    "https://github.com/Dniel97/orpheusdl-nugs"),
    ("kkbox",      "KKBox",       "https://github.com/uhwot/orpheusdl-kkbox"),
    ("bugs",       "Bugs! Music", "https://github.com/Dniel97/orpheusdl-bugsmusic"),
    ("idagio",     "Idagio",      "https://github.com/Dniel97/orpheusdl-idagio"),
    ("jiosaavn",   "JioSaavn",    "https://github.com/bunnykek/orpheusdl-jiosaavn"),
];

pub fn get_orpheus_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mediaharbor")
        .join("orpheusdl")
}

pub fn get_orpheus_venv_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mediaharbor")
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
    let system_python = venv_manager::find_system_python().await?;
    let venv_dir = get_orpheus_venv_dir();
    let orpheus_dir = get_orpheus_dir();

    let venv_str = venv_dir.to_str().unwrap_or("").to_string();
    let orpheus_str = orpheus_dir.to_str().unwrap_or("").to_string();

    progress(5, "Creating virtual environment");
    run_to_completion(&system_python, &["-m", "venv", &venv_str], None, None).await?;

    let python = get_orpheus_python();
    let python_str = python.to_str().unwrap_or("python").to_string();

    progress(15, "Cloning OrpheusDL");
    if orpheus_dir.join(".git").exists() {
        run_to_completion("git", &["-C", &orpheus_str, "pull"], None, None).await?;
    } else {
        if orpheus_dir.exists() {
            tokio::fs::remove_dir_all(&orpheus_dir).await.ok();
        }
        run_to_completion("git", &["clone", ORPHEUS_GIT_URL, &orpheus_str], None, None).await?;
    }

    progress(50, "Installing requirements");
    let req_path = orpheus_dir.join("requirements.txt");
    if req_path.exists() {
        let req_str = req_path.to_str().unwrap_or("").to_string();
        run_to_completion(&python_str, &["-m", "pip", "install", "-r", &req_str], None, None).await?;
    }

    progress(90, "Creating directories");
    tokio::fs::create_dir_all(orpheus_dir.join("modules")).await?;
    tokio::fs::create_dir_all(orpheus_dir.join("config")).await?;

    progress(100, "Done");
    Ok(())
}

pub async fn install_module<F: Fn(u8, &str)>(module_id: &str, git_url: &str, progress: F) -> MhResult<()> {
    let orpheus_dir = get_orpheus_dir();
    let module_dir = orpheus_dir.join("modules").join(module_id);
    let python = get_orpheus_python();
    let python_str = python.to_str().unwrap_or("python").to_string();
    let module_str = module_dir.to_str().unwrap_or("").to_string();

    if module_dir.join(".git").exists() {
        progress(10, "Updating module");
        run_to_completion("git", &["-C", &module_str, "pull"], None, None).await?;
    } else {
        if module_dir.exists() {
            tokio::fs::remove_dir_all(&module_dir).await.ok();
        }
        progress(10, "Cloning module");
        if module_id == "tidal" || module_id == "nugs" {
            run_to_completion("git", &["clone", "--recurse-submodules", git_url, &module_str], None, None).await?;
        } else {
            run_to_completion("git", &["clone", git_url, &module_str], None, None).await?;
        }
    }

    progress(70, "Installing module requirements");
    let req_path = module_dir.join("requirements.txt");
    if req_path.exists() {
        let req_str = req_path.to_str().unwrap_or("").to_string();
        run_to_completion(&python_str, &["-m", "pip", "install", "-r", &req_str], None, None).await?;
    }

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
            if settings.deezer_quality == "FLAC" { "lossless" } else { "high" }
        },
        _ => "hifi",
    }
}

fn apply_credentials(root: &mut serde_json::Value, settings: &Settings) {
    if root["modules"].as_object().is_none() {
        root["modules"] = serde_json::json!({});
    }

    macro_rules! ensure_field {
        ($module:expr, $key:expr, $val:expr) => {{
            if root["modules"][$module].as_object().is_none() {
                root["modules"][$module] = serde_json::json!({});
            }
            root["modules"][$module][$key] = serde_json::Value::String($val.to_string());
        }};
    }

    macro_rules! seed_field {
        ($module:expr, $key:expr, $val:expr) => {{
            if root["modules"][$module].as_object().is_none() {
                root["modules"][$module] = serde_json::json!({});
            }
            if root["modules"][$module][$key].is_null() {
                root["modules"][$module][$key] = serde_json::Value::String($val.to_string());
            }
        }};
        ($module:expr, $key:expr, bool $val:expr) => {{
            if root["modules"][$module].as_object().is_none() {
                root["modules"][$module] = serde_json::json!({});
            }
            if root["modules"][$module][$key].is_null() {
                root["modules"][$module][$key] = serde_json::Value::Bool($val);
            }
        }};
    }

    seed_field!("deezer", "client_id", "447462");
    seed_field!("deezer", "client_secret", "a83bf7f38ad2f137e444727cfc3775cf");
    seed_field!("deezer", "bf_secret", "");
    seed_field!("deezer", "email", "");
    seed_field!("deezer", "password", "");

    seed_field!("qobuz", "app_id", "");
    seed_field!("qobuz", "app_secret", "");
    seed_field!("qobuz", "quality_format", "{sample_rate}");
    seed_field!("qobuz", "username", "");
    seed_field!("qobuz", "password", "");

    seed_field!("soundcloud", "web_access_token", "");

    seed_field!("napster", "api_key", "");
    seed_field!("napster", "customer_secret", "");
    seed_field!("napster", "requested_netloc", "");
    seed_field!("napster", "username", "");
    seed_field!("napster", "password", "");

    seed_field!("beatport", "username", "");
    seed_field!("beatport", "password", "");

    seed_field!("nugs", "username", "");
    seed_field!("nugs", "password", "");
    seed_field!("nugs", "client_id", "Eg7HuH873H65r5rt325UytR5429");
    seed_field!("nugs", "dev_key", "x7f54tgbdyc64y656thy47er4");

    seed_field!("kkbox", "kc1_key", "");
    seed_field!("kkbox", "secret_key", "");
    seed_field!("kkbox", "email", "");
    seed_field!("kkbox", "password", "");

    seed_field!("bugs", "username", "");
    seed_field!("bugs", "password", "");

    seed_field!("idagio", "username", "");
    seed_field!("idagio", "password", "");

    if !settings.qobuz_email_or_userid.is_empty() {
        ensure_field!("qobuz", "username", settings.qobuz_email_or_userid);
        ensure_field!("qobuz", "password", settings.qobuz_password_or_token);
        ensure_field!("qobuz", "app_id", settings.qobuz_app_id);
        ensure_field!("qobuz", "app_secret", settings.qobuz_app_secret);
    }
}

pub async fn write_settings_json(settings: &Settings, output_dir: &str, quality: &str) -> MhResult<()> {
    let orpheus_dir = get_orpheus_dir();
    let config_dir = orpheus_dir.join("config");
    tokio::fs::create_dir_all(&config_dir).await?;
    let settings_path = config_dir.join("settings.json");

    let mut root = if settings_path.exists() {
        let raw = tokio::fs::read_to_string(&settings_path).await
            .map_err(|e| MhError::Other(format!("Failed to read settings.json: {}", e)))?;
        serde_json::from_str(&raw)
            .unwrap_or_else(|_| serde_json::json!({"global": {"general": {}}, "extensions": {}, "modules": {}}))
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
    emitter.emit_log(&BackendLogEvent {
        level: level.to_string(),
        source: "orpheusdl".to_string(),
        title: "OrpheusDL".to_string(),
        message: message.to_string(),
        timestamp: Utc::now().to_rfc3339(),
    });
}

fn emit_consolidated_log(emitter: &Arc<dyn EventEmitter>, level: &str, title: &str, lines: &[String]) {
    emitter.emit_log(&BackendLogEvent {
        level: level.to_string(),
        source: "orpheusdl".to_string(),
        title: title.to_string(),
        message: lines.join("\n"),
        timestamp: Utc::now().to_rfc3339(),
    });
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
    let log_title = format!("OrpheusDL: {}", if url.len() > 60 { &url[..60] } else { url });

    log_buf.push(format!(
        "Starting OrpheusDL download: url={} platform={} output_dir={} quality={}",
        url, platform, output_dir, quality
    ));
    log_buf.push(format!(
        "orpheus_installed={} module_installed={}",
        is_orpheus_installed(), is_module_installed(platform)
    ));

    let python = get_orpheus_python();
    let orpheus_dir = get_orpheus_dir();
    log_buf.push(format!(
        "python={} cwd={}",
        python.display(), orpheus_dir.display()
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
    let mut handle = match spawn_with_output(
        &python_str,
        &["-u", "orpheus.py", &url_owned, "-o", &output_owned],
        None,
        Some(&orpheus_dir_clone),
        tx,
    ).await {
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

    let mut stdin_writer = handle.stdin.take().map(tokio::io::BufWriter::new);

    let emitter_clone = emitter.clone();
    let mut new_settings_detected = false;
    let mut prompt_lines: Vec<String> = Vec::new();

    loop {
        if cancelled.load(Ordering::Relaxed) {
            handle.kill().await;
            log_buf.push("Download cancelled".to_string());
            emit_consolidated_log(&emitter_clone, "info", &log_title, &log_buf);
            return Err(MhError::Cancelled);
        }

        let prompt_pending = !prompt_lines.is_empty();
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some((src, line)) => {
                        let src_label = match src { LineSource::Stdout => "stdout", LineSource::Stderr => "stderr" };
                        let clean = strip_ansi_cr(&line);
                        log_buf.push(format!("[{}] {}", src_label, clean));
                        if clean.contains("New settings detected") {
                            new_settings_detected = true;
                        }
                        if is_prompt_indicator(&clean) {
                            prompt_lines.clear();
                            prompt_lines.push(clean.clone());
                        } else if !prompt_lines.is_empty() {
                            if !clean.trim().is_empty() {
                                prompt_lines.push(clean.clone());
                            }
                        }
                        let percent = parse_orpheus_progress(&clean);
                        if percent > 0.0 {
                            emitter_clone.emit_progress(&DownloadProgressEvent {
                                download_id,
                                percent,
                                speed: None,
                                eta: None,
                                status: "downloading".into(),
                                item_index: None,
                                item_total: None,
                            });
                        }
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(300)), if prompt_pending => {
                emitter_clone.emit_stdin_prompt(&ProcessStdinPromptEvent {
                    download_id,
                    prompt_lines: prompt_lines.clone(),
                });
                prompt_lines.clear();
                if let Some(response) = stdin_rx.recv().await {
                    if let Some(ref mut w) = stdin_writer {
                        let _ = w.write_all(response.as_bytes()).await;
                        let _ = w.write_all(b"\n").await;
                        let _ = w.flush().await;
                    }
                }
            }
        }
    }

    if new_settings_detected {
        let msg = "OrpheusDL reset its settings.json — a newly installed module needs its credentials filled in via Settings → OrpheusDL, then retry the download";
        log_buf.push(msg.to_string());
        emit_consolidated_log(&emitter, "error", &log_title, &log_buf);
        emit_error_progress(&emitter, download_id, msg);
        return Err(MhError::Other(msg.to_string()));
    }

    let status = match handle.child.wait().await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("Failed to wait for orpheus.py: {}", e);
            log_buf.push(msg.clone());
            emit_consolidated_log(&emitter, "error", &log_title, &log_buf);
            emit_error_progress(&emitter, download_id, &msg);
            return Err(MhError::Subprocess(e.to_string()));
        }
    };

    log_buf.push(format!("orpheus.py exited with code {:?}", status.code()));

    if !status.success() {
        let msg = format!("orpheus.py exited with code {:?}", status.code());
        emit_consolidated_log(&emitter, "error", &log_title, &log_buf);
        emit_error_progress(&emitter, download_id, &msg);
        return Err(MhError::Subprocess(msg));
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
    });
    Ok(())
}

pub async fn read_settings_json() -> MhResult<String> {
    let path = get_orpheus_dir().join("config").join("settings.json");
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path).await
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

fn strip_ansi_cr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() { break; }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
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
