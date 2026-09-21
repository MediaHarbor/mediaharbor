use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify_debouncer_full::{
    new_debouncer_opt, DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache,
};
use tokio::sync::mpsc;

use crate::errors::{MhError, MhResult};
use crate::media::file_discovery::is_media_file;
use crate::media::library::scanner::{forget_prefixes, ingest_paths, scan_root, ScanContext};

const DEBOUNCE_MS: u64 = 2000;
const TICK_MS: u64 = 250;
const ESCALATION_THRESHOLD: usize = 5000;

pub struct WatcherHandle {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    _task: tokio::task::JoinHandle<()>,
}

pub fn start_watching(roots: Vec<PathBuf>, ctx: Arc<ScanContext>) -> MhResult<WatcherHandle> {
    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, RecommendedCache>(
        Duration::from_millis(DEBOUNCE_MS),
        Some(Duration::from_millis(TICK_MS)),
        tx,
        RecommendedCache::new(),
        notify::Config::default().with_follow_symlinks(false),
    )
    .map_err(|e| MhError::Other(format!("debouncer: {}", e)))?;

    for root in &roots {
        debouncer
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| MhError::Other(format!("watch {}: {}", root.display(), e)))?;
    }

    let (async_tx, mut async_rx) = mpsc::channel::<Vec<DebouncedEvent>>(64);

    std::thread::spawn(move || {
        while let Ok(res) = rx.recv() {
            if let Ok(events) = res {
                let _ = async_tx.blocking_send(events);
            }
        }
    });

    let roots_for_task = roots.clone();
    let task = tokio::spawn(async move {
        while let Some(events) = async_rx.recv().await {
            handle_events(events, &ctx, &roots_for_task).await;
        }
    });

    Ok(WatcherHandle {
        _debouncer: debouncer,
        _task: task,
    })
}

async fn handle_events(events: Vec<DebouncedEvent>, ctx: &Arc<ScanContext>, roots: &[PathBuf]) {
    if events.len() > ESCALATION_THRESHOLD {
        for root in roots {
            let _ = scan_root(ctx, root, false).await;
        }
        return;
    }

    let mut added: HashSet<PathBuf> = HashSet::new();
    let mut removed: HashSet<PathBuf> = HashSet::new();

    for ev in events {
        let kind = ev.event.kind;
        use notify::event::{ModifyKind, RenameMode};

        if let notify::EventKind::Modify(ModifyKind::Name(mode)) = kind {
            let paths = ev.event.paths.as_slice();
            match (mode, paths) {
                (RenameMode::Both, [from, to]) => {
                    if !is_hidden_or_temp(from) {
                        removed.insert(from.clone());
                        added.remove(from);
                    }
                    if path_is_interesting(to) {
                        added.insert(to.clone());
                        removed.remove(to);
                    }
                    continue;
                }
                (RenameMode::From, _) => {
                    for p in paths {
                        if is_hidden_or_temp(p) {
                            continue;
                        }
                        removed.insert(p.clone());
                        added.remove(p);
                    }
                    continue;
                }
                _ => {}
            }
        }

        for p in &ev.event.paths {
            use notify::EventKind;
            match kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    if !path_is_interesting(p) {
                        continue;
                    }
                    added.insert(p.clone());
                    removed.remove(p);
                }
                EventKind::Remove(_) => {
                    if is_hidden_or_temp(p) {
                        continue;
                    }
                    removed.insert(p.clone());
                    added.remove(p);
                }
                _ => {}
            }
        }
    }

    let root = roots.first().cloned().unwrap_or_default();

    if !added.is_empty() {
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut art_dirs: HashSet<PathBuf> = HashSet::new();
        for p in added {
            if is_folder_art(&p) {
                if let Some(dir) = p.parent() {
                    art_dirs.insert(dir.to_path_buf());
                }
            } else {
                paths.push(p);
            }
        }
        for dir in art_dirs {
            paths.extend(ctx.db.paths_in_dir(&dir).unwrap_or_default());
        }
        let _ = ingest_paths(ctx, paths, &root).await;
    }
    if !removed.is_empty() {
        let paths: Vec<PathBuf> = removed.into_iter().collect();
        let _ = forget_prefixes(ctx, &paths, &root);
    }
}

fn is_hidden_or_temp(p: &std::path::Path) -> bool {
    let name = match p.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return true,
    };
    name.starts_with('.')
        || name.ends_with(".tmp")
        || name.ends_with(".part")
        || name.ends_with(".crdownload")
}

fn path_is_interesting(p: &std::path::Path) -> bool {
    !is_hidden_or_temp(p) && (is_media_file(p) || is_folder_art(p))
}

/// A cover dropped into a folder after its tracks were scanned used to need a full
/// rescan before it showed up, because the watcher only ever looked at media files.
fn is_folder_art(p: &std::path::Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(crate::media::library::covers::is_folder_art_name)
}
