use std::path::{Path, PathBuf};

use crate::{
    errors::{MhError, MhResult},
    subprocess::run_to_completion,
};

pub fn get_venv_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mediaharbor")
        .join("venv")
}

pub fn get_venv_python() -> PathBuf {
    venv_bin_dir().join(if cfg!(windows) {
        "python.exe"
    } else {
        "python"
    })
}

pub fn is_venv_ready() -> bool {
    get_venv_python().exists()
}

/// Where the venv keeps its executables. Named differently per platform, so every
/// caller that needs it goes through here rather than re-deriving the split.
pub fn venv_bin_dir() -> PathBuf {
    get_venv_dir().join(if cfg!(windows) { "Scripts" } else { "bin" })
}

pub fn get_venv_bin(name: &str) -> PathBuf {
    let bin_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    venv_bin_dir().join(bin_name)
}

pub fn resolve_command(name: &str, fallback_paths: &[&Path]) -> PathBuf {
    let venv_bin = get_venv_bin(name);
    if venv_bin.exists() {
        return venv_bin;
    }
    for &p in fallback_paths {
        if p.exists() {
            return p.to_path_buf();
        }
    }
    PathBuf::from(name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTool {
    pub program: String,
    pub prefix_args: Vec<String>,
}

impl ResolvedTool {
    pub fn bare(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            prefix_args: Vec::new(),
        }
    }

    pub fn argv<'a>(&'a self, extra: &[&'a str]) -> Vec<&'a str> {
        let mut v: Vec<&str> = self.prefix_args.iter().map(String::as_str).collect();
        v.extend_from_slice(extra);
        v
    }
}

pub fn system_python_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["py", "python3", "python"]
    } else {
        &["python3", "python"]
    }
}

pub fn resolve_tool_candidates(name: &str, module: &str) -> Vec<ResolvedTool> {
    let mut out: Vec<ResolvedTool> = Vec::new();

    let venv_script = get_venv_bin(name);
    if venv_script.exists() {
        out.push(ResolvedTool::bare(
            venv_script.to_string_lossy().into_owned(),
        ));
    }

    if is_venv_ready() {
        out.push(ResolvedTool {
            program: get_venv_python().to_string_lossy().into_owned(),
            prefix_args: vec!["-m".into(), module.into()],
        });
    }

    if cfg!(windows) {
        out.push(ResolvedTool::bare(format!("{}.exe", name)));
    }
    out.push(ResolvedTool::bare(name.to_string()));

    for py in system_python_names() {
        out.push(ResolvedTool {
            program: (*py).to_string(),
            prefix_args: vec!["-m".into(), module.into()],
        });
    }

    out
}

pub fn resolve_tool(name: &str, module: &str) -> ResolvedTool {
    resolve_tool_candidates(name, module)
        .into_iter()
        .next()
        .unwrap_or_else(|| ResolvedTool::bare(name.to_string()))
}

pub async fn verify_binary(path: &Path) -> bool {
    matches!(
        run_to_completion(&path.to_string_lossy(), &["-version"], None, None).await,
        Ok((_, _, 0))
    ) || matches!(
        run_to_completion(&path.to_string_lossy(), &["--version"], None, None).await,
        Ok((_, _, 0))
    )
}

pub fn managed_bin_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("mediaharbor").join("bin"))
}

pub fn ensure_managed_bin_dir() {
    if let Some(dir) = managed_bin_dir() {
        let _ = std::fs::create_dir_all(dir);
    }
}

/// Single source of truth for the managed/known directories that may contain
/// ffmpeg/ffprobe. Shared by the finder ([`find_ffmpeg`]) and the startup PATH
/// augmenter ([`augment_process_path`]) so detection, execution, and the
/// inherited child PATH all agree on where binaries can live. Higher-priority
/// (MediaHarbor-managed) directories come first.
pub fn ffmpeg_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    dirs.push(venv_bin_dir());
    if let Some(dir) = managed_bin_dir() {
        dirs.push(dir);
    }
    dirs.push(crate::installers::bento4::get_bento4_bin_dir());

    #[cfg(target_os = "windows")]
    {
        if let Some(data) = dirs::data_local_dir() {
            dirs.push(data.join("Microsoft").join("WinGet").join("Links"));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(snap) = std::env::var("SNAP") {
            dirs.push(PathBuf::from(snap).join("usr").join("bin"));
        }
        for p in [
            "/app/bin",          // Flatpak prefix
            "/app/lib/ffmpeg",   // Flatpak ffmpeg-full extension mount
            "/opt/homebrew/bin", // macOS Apple-silicon Homebrew
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/bin",
            "/snap/bin",
        ] {
            dirs.push(PathBuf::from(p));
        }

        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".nix-profile").join("bin"));
        }
        if let Some(user) = std::env::var_os("USER") {
            dirs.push(
                PathBuf::from("/etc/profiles/per-user")
                    .join(user)
                    .join("bin"),
            );
        }
        dirs.push(PathBuf::from("/run/wrappers/bin"));
        dirs.push(PathBuf::from("/run/current-system/sw/bin"));
    }

    dirs
}

fn binary_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Look for an executable named `name` inside `dirs`. Split out (rather than
/// reading `$PATH` inline) so it can be unit-tested without mutating the global
/// environment.
fn find_binary_in(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let file = binary_file_name(name);
    for dir in dirs {
        let candidate = dir.join(&file);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn probe_binary(name: &str) -> Option<PathBuf> {
    if let Some(p) = find_binary_in(&ffmpeg_search_dirs(), name) {
        return Some(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        if let Some(p) = find_binary_in(&dirs, name) {
            return Some(p);
        }
    }
    None
}

/// Memoised *successful* lookups only. Typing the value as a bare `PathBuf`
/// rather than `Option<PathBuf>` makes a remembered miss unrepresentable instead
/// of merely unwritten — see [`cached_lookup`].
static BINARY_CACHE: std::sync::OnceLock<dashmap::DashMap<String, PathBuf>> =
    std::sync::OnceLock::new();

fn binary_cache() -> &'static dashmap::DashMap<String, PathBuf> {
    BINARY_CACHE.get_or_init(dashmap::DashMap::new)
}

/// Drop every memoised lookup.
///
/// A binary that disappeared now heals itself on the next lookup, so the case
/// that still needs this is *ordering*: a tool installed into a directory that
/// outranks the one already cached — the venv's bin dir, the managed bin dir and
/// Bento4's prefix all sort above `$PATH` in [`ffmpeg_search_dirs`] — would
/// otherwise keep resolving to the lower-priority copy. Call it after anything
/// that installs a managed binary.
pub fn invalidate_binary_cache() {
    binary_cache().clear();
}

/// Look `name` up, remembering only hits.
///
/// Remembering a miss is what let the dependency installers loop: `check_deps`
/// probes a binary moments before it exists, the miss sticks for the life of the
/// process, and every later "is it installed?" question — including the guard
/// that decides whether to download — keeps answering "no" about a file now
/// sitting on disk. Re-probing costs one `stat` per entry in
/// [`ffmpeg_search_dirs`] plus `$PATH`, and only happens on paths that already do
/// far more work: once per download, per settings save, per `check_deps`.
fn cached_lookup(name: &str, probe: impl FnOnce(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    // Clone the value out rather than holding the `Ref`: DashMap's `get` keeps a
    // shard read lock alive, and the `remove`/`insert` below want to write it.
    if let Some(path) = binary_cache().get(name).map(|hit| hit.value().clone()) {
        if is_executable(&path) {
            return Some(path);
        }
        binary_cache().remove(name);
    }
    let found = probe(name);
    if let Some(path) = &found {
        binary_cache().insert(name.to_string(), path.clone());
    }
    found
}

fn find_binary(name: &str) -> Option<PathBuf> {
    cached_lookup(name, probe_binary)
}

pub fn find_managed_or_path(name: &str) -> Option<PathBuf> {
    find_binary(name)
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    find_binary("ffmpeg").or_else(|| dynamic_find("ffmpeg"))
}

pub fn find_ffprobe() -> Option<PathBuf> {
    find_binary("ffprobe").or_else(|| dynamic_find("ffprobe"))
}

fn dynamic_find(name: &str) -> Option<PathBuf> {
    if crate::sandbox::is_sandboxed() {
        return None;
    }
    let cache: &'static std::sync::OnceLock<Option<PathBuf>> = match name {
        "ffprobe" => {
            static FFPROBE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
            &FFPROBE
        }
        _ => {
            static FFMPEG: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
            &FFMPEG
        }
    };
    cache.get_or_init(|| probe_os_for_binary(name)).clone()
}

#[cfg(unix)]
fn probe_os_for_binary(name: &str) -> Option<PathBuf> {
    use std::process::{Command, Stdio};

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut cmd = Command::new(&shell);
    cmd.args(["-l", "-c", &format!("sh -c 'command -v {}'", name)]);
    cmd.stdin(Stdio::null());

    let stdout = spawn_with_timeout(cmd, std::time::Duration::from_secs(3))?;
    parse_command_v_output(&stdout)
}

#[cfg(unix)]
fn parse_command_v_output(stdout: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(stdout);
    let last = text.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let candidate = PathBuf::from(last);
    is_executable(&candidate).then_some(candidate)
}

#[cfg(unix)]
fn spawn_with_timeout(
    mut cmd: std::process::Command,
    timeout: std::time::Duration,
) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::Stdio;

    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let result = rx.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(windows)]
fn probe_os_for_binary(name: &str) -> Option<PathBuf> {
    use std::process::Command;

    let ps = "[Environment]::GetEnvironmentVariable('Path','User') + ';' + \
              [Environment]::GetEnvironmentVariable('Path','Machine')";
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", ps]);
    crate::subprocess::apply_no_window_std(&mut cmd);

    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let joined = String::from_utf8_lossy(&out.stdout);
    let dirs: Vec<PathBuf> = joined
        .trim()
        .split(';')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    find_binary_in(&dirs, name)
}

/// Resolve the ffmpeg binary path to run, falling back to the bare name so a
/// PATH-resolved ffmpeg still works if discovery somehow misses it.
pub fn resolve_ffmpeg() -> String {
    find_ffmpeg()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ffmpeg".to_string())
}

/// Locate `certifi`'s bundled CA file inside `venv_dir`, if it is installed there.
fn certifi_in_venv(venv_dir: &Path) -> Option<PathBuf> {
    let rel = Path::new("certifi").join("cacert.pem");

    if cfg!(windows) {
        let p = venv_dir.join("Lib").join("site-packages").join(&rel);
        return p.is_file().then_some(p);
    }

    // Unix venvs nest site-packages under a version-stamped directory
    // (`lib/python3.14/site-packages`), so the minor version has to be discovered
    // rather than assumed. Newest first, to match the interpreter the venv exposes.
    let mut versioned: Vec<PathBuf> = std::fs::read_dir(venv_dir.join("lib"))
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("python"))
        })
        .collect();
    versioned.sort_by_key(|p| python_dir_version(p));
    versioned
        .into_iter()
        .rev()
        .map(|p| p.join("site-packages").join(&rel))
        .find(|p| p.is_file())
}

/// Version of a `lib/python3.14`-style directory, for ordering. Sorting these
/// names as text would rank `python3.9` above `python3.14`, so compare numerically.
/// A free-threaded suffix (`python3.14t`) is ignored.
fn python_dir_version(dir: &Path) -> (u32, u32) {
    dir.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("python"))
        .and_then(|v| v.split_once('.'))
        .map(|(major, minor)| {
            let digits = |s: &str| {
                s.trim_end_matches(|c: char| !c.is_ascii_digit())
                    .parse()
                    .unwrap_or(0)
            };
            (digits(major), digits(minor))
        })
        .unwrap_or((0, 0))
}

/// OS-provided CA bundles, in the order they are worth trying. Windows is
/// deliberately empty: CPython there loads the Windows certificate store
/// directly, so there is nothing to point it at.
fn system_ca_bundles() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["/etc/ssl/cert.pem"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/ssl/ca-bundle.pem",
            "/etc/ssl/cert.pem",
        ]
    }
    #[cfg(not(unix))]
    {
        &[]
    }
}

static CA_BUNDLE_CACHE: std::sync::OnceLock<std::sync::RwLock<Option<Option<PathBuf>>>> =
    std::sync::OnceLock::new();

fn ca_bundle_cache() -> &'static std::sync::RwLock<Option<Option<PathBuf>>> {
    CA_BUNDLE_CACHE.get_or_init(|| std::sync::RwLock::new(None))
}

/// Drop the memoised CA bundle. Call after anything that pip-installs into a
/// venv, so a freshly installed `certifi` is found without a restart.
pub fn invalidate_ca_bundle_cache() {
    if let Ok(mut cache) = ca_bundle_cache().write() {
        *cache = None;
    }
}

/// Resolve a CA bundle for spawned Python processes.
///
/// The python.org framework build MediaHarbor installs on macOS ignores the
/// system trust store and looks for `<framework>/etc/openssl/cert.pem`, which its
/// installer ships empty — that file only appears once the user manually runs
/// `Install Certificates.command`. Until something points Python at a real bundle,
/// every HTTPS request from the venv fails with `CERTIFICATE_VERIFY_FAILED`.
pub fn find_ca_bundle() -> Option<PathBuf> {
    if let Ok(cache) = ca_bundle_cache().read() {
        if let Some(hit) = cache.as_ref() {
            return hit.clone();
        }
    }
    let found = probe_ca_bundle();
    if let Ok(mut cache) = ca_bundle_cache().write() {
        *cache = Some(found.clone());
    }
    found
}

fn probe_ca_bundle() -> Option<PathBuf> {
    // A bundle the user (or their distro) chose deliberately wins over ours.
    if let Some(chosen) = std::env::var_os("SSL_CERT_FILE") {
        let p = PathBuf::from(chosen);
        if p.is_file() {
            return Some(p);
        }
    }

    certifi_in_venv(&get_venv_dir())
        .or_else(|| certifi_in_venv(&crate::orpheus::get_orpheus_venv_dir()))
        .or_else(|| {
            system_ca_bundles()
                .iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
        })
}

/// Base environment for every spawned Python process: UTF-8 I/O so output parses
/// cleanly on every platform, plus a CA bundle so TLS verification works at all.
///
/// Callers that need more (a venv `PATH`, `VIRTUAL_ENV`) should apply this first
/// and then their own vars, so theirs win.
pub fn python_env() -> Vec<(String, String)> {
    let mut env = vec![
        ("PYTHONUNBUFFERED".to_string(), "1".to_string()),
        ("PYTHONIOENCODING".to_string(), "utf-8".to_string()),
        ("PYTHONUTF8".to_string(), "1".to_string()),
    ];
    if let Some(bundle) = find_ca_bundle() {
        let path = bundle.to_string_lossy().into_owned();
        // The stdlib `ssl` module (and so yt-dlp) reads SSL_CERT_FILE; `requests`-based
        // tools such as gamdl and votify read REQUESTS_CA_BUNDLE. Set both.
        env.push(("SSL_CERT_FILE".to_string(), path.clone()));
        env.push(("REQUESTS_CA_BUNDLE".to_string(), path));
    }
    env
}

/// Prepend MediaHarbor-managed and common system bin directories to the process
/// `PATH` so bare-name spawns of ffmpeg/ffprobe — and downstream tools like
/// yt-dlp/gamdl and friends that invoke ffmpeg by bare name — resolve even when a
/// GUI launcher handed us a trimmed session PATH. Must be called before any
/// threads are spawned, since it mutates the process environment.
pub fn augment_process_path() {
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let existing_dirs: Vec<PathBuf> = std::env::split_paths(&existing).collect();

    let mut candidates: Vec<PathBuf> = ffmpeg_search_dirs();

    if find_binary_in(&candidates, "ffmpeg").is_none()
        && find_binary_in(&existing_dirs, "ffmpeg").is_none()
    {
        if let Some(dir) = dynamic_find("ffmpeg")
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
        {
            candidates.push(dir);
        }
    }

    let mut prepend: Vec<PathBuf> = Vec::new();
    for dir in candidates {
        if dir.is_dir() && !existing_dirs.contains(&dir) && !prepend.contains(&dir) {
            prepend.push(dir);
        }
    }

    if prepend.is_empty() {
        return;
    }

    prepend.extend(existing_dirs);
    if let Ok(joined) = std::env::join_paths(prepend) {
        std::env::set_var("PATH", joined);
    }
}

pub async fn find_system_python() -> MhResult<String> {
    #[cfg(not(target_os = "windows"))]
    {
        let mut prefixes: Vec<PathBuf> = vec![
            PathBuf::from("/app/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/run/current-system/sw/bin"),
        ];
        if let Ok(snap) = std::env::var("SNAP") {
            prefixes.insert(0, PathBuf::from(snap).join("usr").join("bin"));
        }
        if let Some(home) = dirs::home_dir() {
            prefixes.push(home.join(".pyenv").join("shims"));
            prefixes.push(home.join(".asdf").join("shims"));
            prefixes.push(home.join(".nix-profile").join("bin"));
        }

        for minor in (10u32..=20).rev() {
            for prefix in &prefixes {
                let path = prefix.join(format!("python3.{}", minor));
                if let Some(p) = check_python_path(&path).await {
                    return Ok(p);
                }
            }
        }

        for prefix in &prefixes {
            for name in ["python3", "python"] {
                if let Some(p) = check_python_path(&prefix.join(name)).await {
                    return Ok(p);
                }
            }
        }
    }

    for &cmd in system_python_names() {
        let (stdout, stderr, code) = run_to_completion(cmd, &["--version"], None, None)
            .await
            .unwrap_or_default();

        if code != 0 {
            continue;
        }

        let output = if stdout.is_empty() { &stderr } else { &stdout };
        if python_version_ok(output.trim()) {
            return Ok(cmd.to_string());
        }
    }

    #[cfg(unix)]
    if let Some(p) = login_shell_python().await {
        return Ok(p);
    }

    if let Some(path) = scan_filesystem_python().await {
        return Ok(path);
    }

    Err(MhError::Subprocess(
        "No suitable Python 3.10+ installation found. Please install Python 3.10 or newer.".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
async fn check_python_path(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let path_str = path.to_string_lossy().to_string();
    let (stdout, stderr, code) = run_to_completion(&path_str, &["--version"], None, None)
        .await
        .ok()?;
    if code != 0 {
        return None;
    }
    let output = if stdout.is_empty() { &stderr } else { &stdout };
    python_version_ok(output.trim()).then_some(path_str)
}

#[cfg(unix)]
async fn login_shell_python() -> Option<String> {
    if crate::sandbox::is_sandboxed() {
        return None;
    }
    for name in ["python3", "python"] {
        if let Some(path) = probe_os_for_binary(name) {
            if let Some(p) = check_python_path(&path).await {
                return Some(p);
            }
        }
    }
    None
}

fn python_version_ok(output: &str) -> bool {
    output
        .strip_prefix("Python ")
        .and_then(|v| {
            let mut parts = v.split('.');
            let major: u32 = parts.next()?.parse().ok()?;
            let minor: u32 = parts.next()?.parse().ok()?;
            Some(major == 3 && minor >= 10)
        })
        .unwrap_or(false)
}

fn read_venv_version() -> Option<(u32, u32)> {
    let content = std::fs::read_to_string(get_venv_dir().join("pyvenv.cfg")).ok()?;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("version = ") {
            let mut parts = val.trim().split('.');
            let major: u32 = parts.next()?.parse().ok()?;
            let minor: u32 = parts.next()?.parse().ok()?;
            return Some((major, minor));
        }
    }
    None
}

async fn get_python_minor_version(python: &str) -> Option<(u32, u32)> {
    let (stdout, stderr, code) = run_to_completion(python, &["--version"], None, None)
        .await
        .ok()?;
    if code != 0 {
        return None;
    }
    let output = if stdout.is_empty() { &stderr } else { &stdout };
    output.trim().strip_prefix("Python ").and_then(|v| {
        let mut parts = v.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        Some((major, minor))
    })
}

async fn scan_filesystem_python() -> Option<String> {
    #[cfg_attr(
        not(any(target_os = "windows", target_os = "macos")),
        allow(unused_mut)
    )]
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = dirs::data_local_dir() {
            let python_root = local_app_data.join("Programs").join("Python");
            if let Ok(entries) = std::fs::read_dir(&python_root) {
                let mut dir_entries: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_dir()
                            && p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    if let Some(digits) = n.strip_prefix("Python") {
                                        if digits.len() >= 3 {
                                            let mut chars = digits.chars();
                                            let major: u32 = chars
                                                .next()
                                                .and_then(|c| c.to_digit(10))
                                                .unwrap_or(0);
                                            let minor: u32 = chars.as_str().parse().unwrap_or(0);
                                            return major == 3 && minor >= 10;
                                        }
                                    }
                                    false
                                })
                                .unwrap_or(false)
                    })
                    .collect();
                dir_entries.sort_by(|a, b| b.cmp(a));
                for dir in dir_entries {
                    candidates.push(dir.join("python.exe"));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let fw_root = PathBuf::from("/Library/Frameworks/Python.framework/Versions");
        if let Ok(entries) = std::fs::read_dir(&fw_root) {
            let mut ver_dirs: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| {
                                let parts: Vec<&str> = n.split('.').collect();
                                if parts.len() == 2 {
                                    let major: u32 = parts[0].parse().unwrap_or(0);
                                    let minor: u32 = parts[1].parse().unwrap_or(0);
                                    return major == 3 && minor >= 10;
                                }
                                false
                            })
                            .unwrap_or(false)
                })
                .collect();
            ver_dirs.sort_by(|a, b| b.cmp(a));
            for dir in ver_dirs {
                candidates.push(dir.join("bin").join("python3"));
            }
        }
    }

    for path in candidates {
        if !path.exists() {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let Ok((stdout, stderr, code)) =
            run_to_completion(&path_str, &["--version"], None, None).await
        else {
            continue;
        };
        if code != 0 {
            continue;
        }
        let output = if stdout.is_empty() { &stderr } else { &stdout };
        if python_version_ok(output.trim()) {
            return Some(path_str);
        }
    }

    None
}

pub async fn venv_pip_works() -> bool {
    let py = get_venv_python();
    if !py.exists() {
        return false;
    }
    let py_str = py.to_string_lossy().to_string();
    matches!(
        run_to_completion(&py_str, &["-m", "pip", "--version"], None, None).await,
        Ok((_, _, 0))
    )
}

const MANAGED_TOOLS: &[(&str, &str)] = &[
    ("certifi", "certifi"),
    ("yt-dlp", "yt-dlp"),
    ("isodate", "isodate"),
    ("gamdl", "gamdl"),
    ("votify", "votify[librespot]"),
    ("pywidevine", "pywidevine"),
];

fn venv_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn snapshot_installed_tools() -> Vec<String> {
    let py = get_venv_python();
    if !py.exists() {
        return Vec::new();
    }
    let Ok((stdout, _, 0)) = run_to_completion(
        &py.to_string_lossy(),
        &["-m", "pip", "list", "--format=freeze"],
        None,
        None,
    )
    .await
    else {
        return Vec::new();
    };
    let low = stdout.to_lowercase();
    MANAGED_TOOLS
        .iter()
        .filter(|(dist, _)| {
            let needle = format!("{}==", dist.to_lowercase());
            low.lines().any(|l| l.trim().starts_with(&needle))
        })
        .map(|(_, spec)| (*spec).to_string())
        .collect()
}

async fn reinstall_tools<F: Fn(u8, &str)>(specs: &[String], on_progress: &F) -> MhResult<()> {
    on_progress(20, "Restoring downloader tools…");
    let py = get_venv_python();
    let mut args: Vec<String> = vec![
        "-m".into(),
        "pip".into(),
        "install".into(),
        "--upgrade".into(),
    ];
    args.extend(specs.iter().cloned());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (_, stderr, code) = run_to_completion(&py.to_string_lossy(), &arg_refs, None, None).await?;
    if code != 0 {
        return Err(MhError::Subprocess(format!(
            "Python environment was rebuilt but reinstalling downloader tools failed (exit {code}): {}",
            stderr.trim()
        )));
    }
    invalidate_ca_bundle_cache();
    on_progress(60, "Downloader tools restored");
    Ok(())
}

/// Install `certifi` into the freshly created venv.
///
/// This is infrastructure rather than a per-service tool: the python.org build
/// MediaHarbor installs on macOS carries no CA store of its own, so without this
/// every HTTPS request from yt-dlp, gamdl or votify fails to verify. Best effort —
/// an otherwise usable venv should not fail to build because PyPI was unreachable,
/// and `find_ca_bundle` still falls back to the OS trust store.
async fn install_certifi<F: Fn(u8, &str)>(on_progress: &F) {
    on_progress(16, "Installing certificates...");
    let py = get_venv_python();
    let installed = matches!(
        run_to_completion(
            &py.to_string_lossy(),
            &["-m", "pip", "install", "--upgrade", "certifi"],
            None,
            None,
        )
        .await,
        Ok((_, _, 0))
    );
    invalidate_ca_bundle_cache();
    if !installed {
        on_progress(18, "Continuing with the system certificate store");
    }
}

fn venv_creation_error(code: i32, stderr: &str) -> MhError {
    let low = stderr.to_lowercase();
    if low.contains("ensurepip")
        || (low.contains("pip") && low.contains("not available"))
        || low.contains("no module named venv")
        || low.contains("no module named 'venv'")
    {
        return MhError::Subprocess(format!(
            "Python is installed but its 'venv'/'ensurepip' module is missing. \
             Install your system's Python venv package and try again \
             (Debian/Ubuntu: 'sudo apt install python3-venv'; \
             Fedora: 'sudo dnf install python3-pip'; \
             Alpine: 'apk add python3 py3-pip'; \
             Arch: already included).\n\nDetails: {}",
            stderr.trim()
        ));
    }
    MhError::Subprocess(format!(
        "Failed to create Python virtual environment (exit {code}): {stderr}"
    ))
}

pub async fn ensure_venv<F>(on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str) + Send,
{
    let _guard = venv_lock().lock().await;

    let venv_dir = get_venv_dir();
    let system_python = find_system_python().await?;

    let mut snapshot: Vec<String> = Vec::new();
    if is_venv_ready() {
        let venv_ver = read_venv_version();
        let sys_ver = get_python_minor_version(&system_python).await;
        let confirmed_mismatch = matches!((venv_ver, sys_ver), (Some(v), Some(s)) if v != s);

        if !confirmed_mismatch && venv_pip_works().await {
            return Ok(());
        }

        snapshot = snapshot_installed_tools().await;
        let why = if confirmed_mismatch {
            "Rebuilding Python environment after system update..."
        } else {
            "Repairing Python environment..."
        };
        on_progress(2, why);
        tokio::fs::remove_dir_all(&venv_dir).await.ok();
    }

    if let Some(parent) = venv_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    on_progress(5, "Creating MediaHarbor Python environment...");

    let venv_path = venv_dir.to_str().ok_or_else(|| {
        MhError::Other(
            "MediaHarbor's data directory path contains invalid (non-UTF-8) characters; \
             cannot create the Python environment."
                .to_string(),
        )
    })?;

    let (_, stderr, code) = run_to_completion(
        &system_python,
        &["-m", "venv", "--copies", venv_path],
        None,
        None,
    )
    .await?;

    if code != 0 || !is_venv_ready() {
        return Err(venv_creation_error(code, &stderr));
    }

    on_progress(15, "Python environment created");

    install_certifi(&on_progress).await;

    if !snapshot.is_empty() {
        reinstall_tools(&snapshot, &on_progress).await?;
    }

    // Anything reaching here created or rebuilt the venv — the "nothing changed"
    // case returned early above — and the venv's bin dir is the first entry of
    // `ffmpeg_search_dirs`, so its console scripts have to displace whatever was
    // cached from further down the search order.
    invalidate_binary_cache();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venv_dir_is_in_data_local() {
        let dir = get_venv_dir();
        let data = dirs::data_local_dir().unwrap();
        assert!(dir.starts_with(data));
    }

    fn touch_executable(path: &Path) {
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    /// The installer loop in miniature: probe before the file exists, then after.
    /// A remembered miss made the second answer wrong until the app restarted.
    #[test]
    fn cached_lookup_reprobes_after_a_miss() {
        let tmp = tempfile::TempDir::new().unwrap();
        let name = "mh_test_miss_then_hit";
        let bin = tmp.path().join(binary_file_name(name));
        let calls = std::cell::Cell::new(0u32);

        assert_eq!(
            cached_lookup(name, |_| {
                calls.set(calls.get() + 1);
                None
            }),
            None
        );

        touch_executable(&bin);
        let found = cached_lookup(name, |_| {
            calls.set(calls.get() + 1);
            Some(bin.clone())
        });

        assert_eq!(found.as_deref(), Some(bin.as_path()));
        assert_eq!(calls.get(), 2, "a miss must not be remembered");
    }

    #[test]
    fn cached_lookup_remembers_a_hit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let name = "mh_test_hit";
        let bin = tmp.path().join(binary_file_name(name));
        touch_executable(&bin);
        let calls = std::cell::Cell::new(0u32);

        for _ in 0..3 {
            assert_eq!(
                cached_lookup(name, |_| {
                    calls.set(calls.get() + 1);
                    Some(bin.clone())
                }),
                Some(bin.clone())
            );
        }
        assert_eq!(calls.get(), 1, "the hot path must stay memoised");
    }

    #[test]
    fn cached_lookup_forgets_a_path_that_stopped_being_executable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let name = "mh_test_vanishing";
        let bin = tmp.path().join(binary_file_name(name));
        touch_executable(&bin);
        assert!(cached_lookup(name, |_| Some(bin.clone())).is_some());

        std::fs::remove_file(&bin).unwrap();
        assert_eq!(cached_lookup(name, |_| None), None);
    }

    #[test]
    fn resolve_command_returns_name_when_absent() {
        let resolved = resolve_command("nonexistent_bin_xyz", &[]);
        assert_eq!(resolved, PathBuf::from("nonexistent_bin_xyz"));
    }

    #[test]
    fn resolved_tool_argv_keeps_program_intact_and_prepends_prefix() {
        let tool = ResolvedTool {
            program: "/Users/First Last/venv/bin/python".to_string(),
            prefix_args: vec!["-m".into(), "yt_dlp".into()],
        };
        assert_eq!(tool.program, "/Users/First Last/venv/bin/python");
        assert_eq!(tool.argv(&["--version"]), vec!["-m", "yt_dlp", "--version"]);
    }

    #[test]
    fn resolve_tool_candidates_offer_console_and_module_forms() {
        let cands = resolve_tool_candidates("yt-dlp", "yt_dlp");
        assert!(
            cands
                .iter()
                .any(|c| c.program == "yt-dlp" || c.program == "yt-dlp.exe"),
            "expected a bare console-script candidate"
        );
        assert!(
            cands
                .iter()
                .any(|c| c.prefix_args == vec!["-m".to_string(), "yt_dlp".to_string()]),
            "expected a `python -m yt_dlp` fallback candidate"
        );
    }

    #[test]
    fn find_binary_in_returns_none_for_absent() {
        let dirs = vec![std::env::temp_dir()];
        assert!(find_binary_in(&dirs, "definitely_absent_bin_xyz123").is_none());
    }

    #[test]
    fn find_binary_in_finds_executable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join(binary_file_name("ffmpeg"));
        std::fs::write(&file, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&file).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&file, perms).unwrap();
        }
        let found = find_binary_in(&[tmp.path().to_path_buf()], "ffmpeg");
        assert_eq!(found, Some(file));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn ffmpeg_search_dirs_lists_common_unix_locations() {
        let dirs = ffmpeg_search_dirs();
        for expected in ["/usr/bin", "/snap/bin", "/usr/local/bin", "/app/bin"] {
            assert!(
                dirs.iter().any(|d| d == &PathBuf::from(expected)),
                "expected {} in search dirs",
                expected
            );
        }
        assert_eq!(dirs.first(), Some(&get_venv_dir().join("bin")));
    }

    #[test]
    fn install_dir_is_a_search_dir() {
        let managed = managed_bin_dir().expect("managed bin dir");
        assert!(
            ffmpeg_search_dirs().contains(&managed),
            "installer target {:?} is not in ffmpeg_search_dirs()",
            managed
        );
    }

    #[test]
    fn certifi_in_venv_returns_none_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(certifi_in_venv(tmp.path()), None);
    }

    #[cfg(unix)]
    #[test]
    fn certifi_in_venv_finds_versioned_site_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let sp = tmp
            .path()
            .join("lib")
            .join("python3.14")
            .join("site-packages")
            .join("certifi");
        std::fs::create_dir_all(&sp).unwrap();
        let cacert = sp.join("cacert.pem");
        std::fs::write(&cacert, b"-----BEGIN CERTIFICATE-----").unwrap();
        assert_eq!(certifi_in_venv(tmp.path()), Some(cacert));
    }

    #[cfg(unix)]
    #[test]
    fn certifi_in_venv_prefers_the_newest_python_dir() {
        let tmp = tempfile::tempdir().unwrap();
        for ver in ["python3.9", "python3.14"] {
            let sp = tmp
                .path()
                .join("lib")
                .join(ver)
                .join("site-packages")
                .join("certifi");
            std::fs::create_dir_all(&sp).unwrap();
            std::fs::write(sp.join("cacert.pem"), b"x").unwrap();
        }
        let found = certifi_in_venv(tmp.path()).unwrap();
        assert!(found.to_string_lossy().contains("python3.14"));
    }

    #[test]
    fn python_env_always_carries_utf8_settings() {
        let env = python_env();
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"PYTHONIOENCODING"));
        assert!(keys.contains(&"PYTHONUTF8"));
        assert!(keys.contains(&"PYTHONUNBUFFERED"));
    }

    #[test]
    fn python_env_sets_both_ca_vars_to_the_same_bundle() {
        // Whether a bundle is found depends on the host, so assert the invariant
        // that holds either way: both vars appear together, pointing at one file.
        let env = python_env();
        let ssl = env.iter().find(|(k, _)| k == "SSL_CERT_FILE");
        let requests = env.iter().find(|(k, _)| k == "REQUESTS_CA_BUNDLE");
        match (ssl, requests) {
            (Some((_, a)), Some((_, b))) => {
                assert_eq!(a, b);
                assert!(Path::new(a).is_file());
            }
            (None, None) => {}
            _ => panic!("SSL_CERT_FILE and REQUESTS_CA_BUNDLE must be set together"),
        }
    }

    #[test]
    fn ensure_managed_bin_dir_creates_it() {
        ensure_managed_bin_dir();
        let managed = managed_bin_dir().expect("managed bin dir");
        assert!(managed.is_dir(), "{:?} should exist after ensure", managed);
    }

    #[cfg(unix)]
    #[test]
    fn parse_command_v_output_verifies_path() {
        assert!(parse_command_v_output(b"").is_none());
        assert!(parse_command_v_output(b"/no/such/binary_xyz123\n").is_none());

        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("ffmpeg");
        std::fs::write(&file, b"#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&file, perms).unwrap();

        let line = format!("{}\n", file.display());
        assert_eq!(parse_command_v_output(line.as_bytes()), Some(file));
    }
}
