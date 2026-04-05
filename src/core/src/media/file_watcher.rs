use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::errors::{MhError, MhResult};
use crate::media::file_discovery::is_media_file;

const DEBOUNCE_MS: u64 = 800;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    pub fn stop(&mut self) {
    }
}

pub fn start_watching(
    dirs: &[PathBuf],
    on_added: impl Fn(PathBuf) + Send + Sync + 'static,
    on_removed: impl Fn(PathBuf) + Send + Sync + 'static,
) -> MhResult<FileWatcher> {
    let pending_added: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let pending_removed: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let last_event: Arc<Mutex<Instant>> = Arc::new(Mutex::new(Instant::now()));

    let added_cb = Arc::new(on_added);
    let removed_cb = Arc::new(on_removed);

    let flush_added = Arc::clone(&pending_added);
    let flush_removed = Arc::clone(&pending_removed);
    let flush_last = Arc::clone(&last_event);
    let flush_added_cb = Arc::clone(&added_cb);
    let flush_removed_cb = Arc::clone(&removed_cb);

    std::thread::spawn(move || {
        let debounce = Duration::from_millis(DEBOUNCE_MS);
        loop {
            std::thread::sleep(Duration::from_millis(100));

            let elapsed = {
                let guard = flush_last.lock().unwrap();
                guard.elapsed()
            };

            if elapsed < debounce {
                continue;
            }

            let added: Vec<PathBuf> = {
                let mut guard = flush_added.lock().unwrap();
                guard.drain().collect()
            };
            let removed: Vec<PathBuf> = {
                let mut guard = flush_removed.lock().unwrap();
                guard.drain().collect()
            };

            for p in added {
                flush_added_cb(p);
            }
            for p in removed {
                flush_removed_cb(p);
            }
        }
    });

    let event_added = Arc::clone(&pending_added);
    let event_removed = Arc::clone(&pending_removed);
    let event_last = Arc::clone(&last_event);

    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            let event = match res {
                Ok(e) => e,
                Err(_) => return,
            };

            let paths: Vec<PathBuf> = event
                .paths
                .iter()
                .filter(|p| {
                    if !is_media_file(p) {
                        return false;
                    }
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    !name.starts_with('.')
                        && !name.ends_with(".tmp")
                        && !name.ends_with(".part")
                        && !name.ends_with(".crdownload")
                })
                .cloned()
                .collect();

            if paths.is_empty() {
                return;
            }

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    let mut guard = event_added.lock().unwrap();
                    for p in &paths {
                        guard.insert(p.clone());
                    }
                    let mut rguard = event_removed.lock().unwrap();
                    for p in &paths {
                        rguard.remove(p);
                    }
                }
                EventKind::Remove(_) => {
                    let mut guard = event_removed.lock().unwrap();
                    for p in &paths {
                        guard.insert(p.clone());
                    }
                    let mut aguard = event_added.lock().unwrap();
                    for p in &paths {
                        aguard.remove(p);
                    }
                }
                _ => {}
            }

            let mut last = event_last.lock().unwrap();
            *last = Instant::now();
        },
        Config::default(),
    )
    .map_err(|e| MhError::Other(format!("notify: {}", e)))?;

    for dir in dirs {
        watcher
            .watch(dir, RecursiveMode::Recursive)
            .map_err(|e| MhError::Other(format!("notify watch {}: {}", dir.display(), e)))?;
    }

    Ok(FileWatcher { _watcher: watcher })
}
