//! notify-backed watcher for `~/.agent-orchestrator/hooks/` and
//! `~/.agent-orchestrator/cost-events/`. Maintains an in-memory map of
//! latest hook status per agent-view session, and forwards new cost
//! events to storage.

use crate::core::paths;
use crate::core::runner::hook_io::HookStatusFile;
use crate::core::storage::{CostEvent, Storage};
use crate::types::SessionStatus;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

/// In-memory hook status, derived from a hook status file. `received_at`
/// is the wall-clock time the watcher saw the file (used for freshness
/// checks in the poller).
#[derive(Debug, Clone)]
pub struct HookStatus {
    pub status: SessionStatus,
    pub claude_session_id: Option<String>,
    #[allow(dead_code)] // used in tests; reserved for UI display (Task 11+)
    pub event: String,
    pub received_at: SystemTime,
}

/// Shared state owned by the watcher thread, read by the poller.
#[derive(Debug, Default)]
pub struct EventState {
    pub hook_status: HashMap<String, HookStatus>,
    pub seen_cost_files: HashSet<String>,
}

pub type EventStateHandle = Arc<Mutex<EventState>>;

/// A storage handle the watcher uses to persist cost events. `Mutex`-guarded
/// because `rusqlite::Connection` is not `Sync`.
pub type SharedStorage = Arc<Mutex<Storage>>;

/// Spawn the watcher thread against the production directories and storage.
/// On any setup failure, returns a handle to an empty state and logs.
pub fn spawn() -> EventStateHandle {
    if let Err(e) = paths::ensure_event_dirs() {
        eprintln!("agent-view: event_watcher: ensure_event_dirs failed: {}", e);
        return Arc::new(Mutex::new(EventState::default()));
    }
    let storage = match Storage::open_default() {
        Ok(s) => Some(Arc::new(Mutex::new(s))),
        Err(e) => {
            eprintln!(
                "agent-view: event_watcher: open_default storage failed: {}; cost events will not be persisted",
                e
            );
            None
        }
    };
    spawn_in(paths::hooks_dir(), paths::cost_events_dir(), storage)
}

/// Same as [`spawn`] but with explicit directories and storage handle.
/// Designed for tests — production code should use [`spawn`].
///
/// `storage` may be `None`, in which case cost-event files are still
/// dedup-tracked but never persisted (and are not deleted from disk, so a
/// later run with a working storage handle can ingest them).
pub fn spawn_in(
    hooks: PathBuf,
    costs: PathBuf,
    storage: Option<SharedStorage>,
) -> EventStateHandle {
    let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));

    // Bootstrap from existing files BEFORE notify subscription so we don't
    // miss anything that landed before startup.
    load_existing(&state, &hooks, &costs, storage.as_ref());

    let state_thread = Arc::clone(&state);
    thread::spawn(move || {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("agent-view: event_watcher: notify init failed: {}", e);
                return;
            }
        };
        if let Err(e) = watcher.watch(&hooks, RecursiveMode::NonRecursive) {
            eprintln!("agent-view: event_watcher: watch hooks failed: {}", e);
        }
        if let Err(e) = watcher.watch(&costs, RecursiveMode::NonRecursive) {
            eprintln!("agent-view: event_watcher: watch costs failed: {}", e);
        }

        let mut pending: HashSet<PathBuf> = HashSet::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(ev)) => {
                    for p in ev.paths {
                        if p.extension().and_then(|e| e.to_str()) == Some("json") {
                            pending.insert(p);
                        }
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("agent-view: event_watcher: notify error: {}", e);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !pending.is_empty() {
                        let batch = std::mem::take(&mut pending);
                        for path in batch {
                            process_path(&state_thread, &path, storage.as_ref());
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });

    state
}

fn load_existing(
    state: &EventStateHandle,
    hooks: &Path,
    costs: &Path,
    storage: Option<&SharedStorage>,
) {
    if let Ok(entries) = std::fs::read_dir(hooks) {
        for e in entries.flatten() {
            process_path(state, &e.path(), storage);
        }
    }
    if let Ok(entries) = std::fs::read_dir(costs) {
        for e in entries.flatten() {
            process_path(state, &e.path(), storage);
        }
    }
}

fn process_path(state: &EventStateHandle, path: &Path, storage: Option<&SharedStorage>) {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return;
    }
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    match parent_name {
        Some("hooks") => process_hook_file(state, path),
        Some("cost-events") => process_cost_file(state, path, storage),
        _ => {}
    }
}

fn process_hook_file(state: &EventStateHandle, path: &Path) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return,
    };
    let file: HookStatusFile = match serde_json::from_slice(&bytes) {
        Ok(f) => f,
        Err(_) => return,
    };
    let session_id = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let status = match SessionStatus::try_parse_strict(&file.status) {
        Some(s) => s,
        None => return,
    };
    let claude_sid = if file.claude_session_id.is_empty() {
        None
    } else {
        Some(file.claude_session_id)
    };
    let entry = HookStatus {
        status,
        claude_session_id: claude_sid,
        event: file.event,
        received_at: SystemTime::now(),
    };
    if let Ok(mut s) = state.lock() {
        s.hook_status.insert(session_id, entry);
    }
}

fn process_cost_file(state: &EventStateHandle, path: &Path, storage: Option<&SharedStorage>) {
    let key = match path.to_str() {
        Some(p) => p.to_string(),
        None => return,
    };
    {
        let mut s = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if s.seen_cost_files.contains(&key) {
            return;
        }
        s.seen_cost_files.insert(key.clone());
    }

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return,
    };
    let event: CostEvent = match deserialize_cost_event(&bytes) {
        Some(e) => e,
        None => return,
    };
    let Some(storage) = storage else {
        // No storage — leave file in place so a future run can ingest it.
        return;
    };
    let inserted = match storage.lock() {
        Ok(s) => s.insert_cost_event(&event).is_ok(),
        Err(_) => false,
    };
    if inserted {
        // After a successful insert the file is redundant: the dedup set
        // (in-memory) covers the rest of this process; the SQLite row
        // covers future processes. Removing keeps cost-events/ from
        // growing unboundedly on long-running deployments.
        let _ = std::fs::remove_file(path);
    }
}

fn deserialize_cost_event(bytes: &[u8]) -> Option<CostEvent> {
    #[derive(serde::Deserialize)]
    struct Wire {
        session_id: String,
        model: String,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_creation_tokens: i64,
        ts: i64,
    }
    let w: Wire = serde_json::from_slice(bytes).ok()?;
    Some(CostEvent {
        session_id: w.session_id,
        model: w.model,
        input_tokens: w.input_tokens,
        output_tokens: w.output_tokens,
        cache_read_tokens: w.cache_read_tokens,
        cache_creation_tokens: w.cache_creation_tokens,
        ts: w.ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::test_helpers::{make_test_session, test_storage};
    use std::fs;

    fn write_hook_file(dir: &Path, session_id: &str, status: &str) -> PathBuf {
        let path = dir.join(format!("{}.json", session_id));
        let body = serde_json::json!({
            "status": status,
            "event": "Stop",
            "ts": 1700000000,
        });
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        path
    }

    fn write_cost_file(dir: &Path, session_id: &str, ts: i64) -> PathBuf {
        let path = dir.join(format!("{}_{}.json", session_id, ts));
        let body = serde_json::json!({
            "session_id": session_id, "model": "m",
            "input_tokens": 1, "output_tokens": 2,
            "cache_read_tokens": 0, "cache_creation_tokens": 0,
            "ts": ts,
        });
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        path
    }

    #[test]
    fn test_process_hook_file_inserts_status() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        fs::create_dir(&hooks).unwrap();
        let path = write_hook_file(&hooks, "sess-1", "running");
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None);
        let g = state.lock().unwrap();
        let entry = g.hook_status.get("sess-1").unwrap();
        assert_eq!(entry.status, SessionStatus::Running);
        assert_eq!(entry.event, "Stop");
    }

    #[test]
    fn test_process_hook_file_unknown_status_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        fs::create_dir(&hooks).unwrap();
        let path = write_hook_file(&hooks, "sess-2", "not-a-real-status");
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None);
        let g = state.lock().unwrap();
        assert!(g.hook_status.get("sess-2").is_none());
    }

    #[test]
    fn test_load_existing_picks_up_files_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        let costs = dir.path().join("cost-events");
        fs::create_dir(&hooks).unwrap();
        fs::create_dir(&costs).unwrap();
        write_hook_file(&hooks, "abc", "waiting");
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        load_existing(&state, &hooks, &costs, None);
        assert_eq!(
            state.lock().unwrap().hook_status.get("abc").unwrap().status,
            SessionStatus::Waiting
        );
    }

    #[test]
    fn test_process_cost_file_dedupes() {
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();
        let path = write_cost_file(&costs, "sess-1", 12345);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None);
        process_path(&state, &path, None);
        assert_eq!(state.lock().unwrap().seen_cost_files.len(), 1);
    }

    #[test]
    fn test_process_cost_file_persists_and_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();

        // Need a session row for the FK on cost_events.
        let (storage, _db_dir) = test_storage();
        storage.save_session(&make_test_session("sess-X")).unwrap();
        let storage: SharedStorage = Arc::new(Mutex::new(storage));

        let path = write_cost_file(&costs, "sess-X", 999);
        assert!(path.exists());

        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, Some(&storage));

        let totals = storage
            .lock()
            .unwrap()
            .cost_totals_for_session("sess-X")
            .unwrap();
        assert_eq!(totals.input, 1);
        assert_eq!(totals.output, 2);
        assert!(
            !path.exists(),
            "cost-event file should be deleted after successful insert"
        );
    }

    #[test]
    fn test_process_cost_file_keeps_file_when_no_storage() {
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();
        let path = write_cost_file(&costs, "sess-Y", 1);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None);
        assert!(
            path.exists(),
            "file must be retained when no storage is available"
        );
    }

    #[test]
    fn test_spawn_in_consumes_new_hook_file_via_notify_thread() {
        // End-to-end: write a hook file AFTER the watcher starts, verify the
        // notify thread picks it up and mutates shared state.
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        let costs = dir.path().join("cost-events");
        fs::create_dir(&hooks).unwrap();
        fs::create_dir(&costs).unwrap();

        let state = spawn_in(hooks.clone(), costs, None);

        // Give notify a moment to attach its watch.
        std::thread::sleep(Duration::from_millis(200));
        write_hook_file(&hooks, "live-sess", "running");

        // Watcher debounces ~100ms after the last event; poll up to ~3s.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if state.lock().unwrap().hook_status.contains_key("live-sess") {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!("watcher never picked up the new hook file");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            state
                .lock()
                .unwrap()
                .hook_status
                .get("live-sess")
                .unwrap()
                .status,
            SessionStatus::Running
        );
    }
}
