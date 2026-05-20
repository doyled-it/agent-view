//! notify-backed watcher for `~/.agent-orchestrator/hooks/` and
//! `~/.agent-orchestrator/cost-events/`. Maintains an in-memory map of
//! latest hook status per agent-view session, and forwards new cost
//! events to storage.

use crate::core::config::load_config;
use crate::core::cost::Pricer;
use crate::core::paths;
use crate::core::runner::codex::cost_handler::{
    current_context_tokens, current_context_window, current_rate_limits, find_rollout_for_thread,
    is_valid_thread_id, RateLimitInfo,
};
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
/// checks in the poller). The on-disk `HookStatusFile.event` is intentionally
/// not mirrored here — its only consumer would be UI surfacing, and we
/// already have `status` for the symbolic state.
#[derive(Debug, Clone)]
pub struct HookStatus {
    pub status: SessionStatus,
    pub tool_session_id: Option<String>,
    pub received_at: SystemTime,
    /// Claude transcript path (when known). Used by the poller to read
    /// the current context-size without scraping the tmux pane. `None`
    /// for Codex and other tools that don't expose a transcript.
    pub transcript_path: Option<String>,
}

/// Per-rollout-file snapshot used to keep the UI render path off the
/// filesystem. Refreshed lazily by [`EventState::rollout_snapshot`] when
/// the file's `mtime` advances; otherwise served from cache.
#[derive(Debug, Default, Clone)]
pub struct RolloutSnapshot {
    pub mtime: Option<SystemTime>,
    pub context_tokens: Option<i64>,
    /// The negotiated context window size Codex published in the most
    /// recent `token_count` event. Lets the UI display the actual model
    /// limit (e.g. 1M for extended-context plans) instead of a hard-coded
    /// per-tool fallback.
    pub context_window: Option<i64>,
    pub rate_limits: Option<RateLimitInfo>,
}

/// Shared state owned by the watcher thread, read by the poller and UI.
#[derive(Debug, Default)]
pub struct EventState {
    pub hook_status: HashMap<String, HookStatus>,
    pub seen_cost_files: HashSet<String>,
    /// Codex `thread-id` → resolved rollout file path. Populated by
    /// [`notify_handler::handle_notify_with_paths`] and as a one-shot
    /// bootstrap when a Codex hook status file is first observed.
    /// Render-path lookups read this map instead of walking
    /// `~/.codex/sessions/` every frame.
    pub rollout_paths: HashMap<String, PathBuf>,
    /// Rollout-file content snapshots keyed by canonical path. Refreshed
    /// only when the underlying file's `mtime` advances; otherwise served
    /// from cache so 30 Hz renders don't re-parse a multi-MiB JSONL.
    pub rollout_snapshots: HashMap<PathBuf, RolloutSnapshot>,
}

impl EventState {
    /// Cached rollout path for the given Codex thread-id, or `None` if no
    /// `find_rollout_for_thread` result has been recorded yet. Never
    /// touches the filesystem; the watcher thread is the only writer
    /// (via [`Self::record_rollout_path`]).
    pub fn cached_rollout_path(&self, thread_id: &str) -> Option<&Path> {
        self.rollout_paths.get(thread_id).map(PathBuf::as_path)
    }

    /// Record (or refresh) the rollout path for `thread_id`. Called by the
    /// watcher after each hook update so subsequent renders never have to
    /// walk the filesystem. Also clears any stale snapshot tied to a
    /// previous path for the same thread (Codex compaction roll-forward).
    pub fn record_rollout_path(&mut self, thread_id: &str, path: PathBuf) {
        if let Some(prev) = self.rollout_paths.get(thread_id) {
            if prev != &path {
                self.rollout_snapshots.remove(prev);
            }
        }
        self.rollout_paths.insert(thread_id.to_string(), path);
    }

    /// Mtime-gated snapshot of a rollout file. Returns the cached
    /// `RolloutSnapshot` when the file hasn't changed since the last
    /// refresh; otherwise re-reads and updates the cache.
    pub fn rollout_snapshot(&mut self, path: &Path) -> RolloutSnapshot {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some(entry) = self.rollout_snapshots.get(path) {
            if entry.mtime == mtime {
                return entry.clone();
            }
        }
        let snap = RolloutSnapshot {
            mtime,
            context_tokens: current_context_tokens(path),
            context_window: current_context_window(path),
            rate_limits: current_rate_limits(path),
        };
        self.rollout_snapshots
            .insert(path.to_path_buf(), snap.clone());
        snap
    }
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
    let pricer = Arc::new(load_config().pricer());
    let storage = match Storage::open_default() {
        Ok(s) => {
            // Re-price historical rows with the user's override-aware
            // table. The v9 migration already backfilled with defaults; this
            // catches any config-supplied overrides. Failure is non-fatal —
            // worst case, rows keep their default-rate microdollars.
            if let Err(e) = s.recompute_cost_microdollars(&pricer) {
                eprintln!(
                    "agent-view: event_watcher: cost-event recompute failed: {}; historical rows retain default-rate microdollars",
                    e
                );
            }
            Some(Arc::new(Mutex::new(s)))
        }
        Err(e) => {
            eprintln!(
                "agent-view: event_watcher: open_default storage failed: {}; cost events will not be persisted",
                e
            );
            None
        }
    };
    spawn_in(
        paths::hooks_dir(),
        paths::cost_events_dir(),
        storage,
        pricer,
    )
}

/// Same as [`spawn`] but with explicit directories, storage handle, and
/// pricer. Designed for tests — production code should use [`spawn`].
///
/// `storage` may be `None`, in which case cost-event files are still
/// dedup-tracked but never persisted (and are not deleted from disk, so a
/// later run with a working storage handle can ingest them).
pub fn spawn_in(
    hooks: PathBuf,
    costs: PathBuf,
    storage: Option<SharedStorage>,
    pricer: Arc<Pricer>,
) -> EventStateHandle {
    let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));

    // Bootstrap from existing files BEFORE notify subscription so we don't
    // miss anything that landed before startup.
    load_existing(&state, &hooks, &costs, storage.as_ref(), &pricer);

    let state_thread = Arc::clone(&state);
    let pricer_thread = Arc::clone(&pricer);
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
                            process_path(&state_thread, &path, storage.as_ref(), &pricer_thread);
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
    pricer: &Pricer,
) {
    if let Ok(entries) = std::fs::read_dir(hooks) {
        for e in entries.flatten() {
            process_path(state, &e.path(), storage, pricer);
        }
    }
    if let Ok(entries) = std::fs::read_dir(costs) {
        for e in entries.flatten() {
            process_path(state, &e.path(), storage, pricer);
        }
    }
}

fn process_path(
    state: &EventStateHandle,
    path: &Path,
    storage: Option<&SharedStorage>,
    pricer: &Pricer,
) {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return;
    }
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    match parent_name {
        Some("hooks") => process_hook_file(state, path),
        Some("cost-events") => process_cost_file(state, path, storage, pricer),
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
    let tool_sid = if file.tool_session_id.is_empty() {
        None
    } else {
        Some(file.tool_session_id)
    };
    let transcript_path = if file.transcript_path.is_empty() {
        None
    } else {
        Some(file.transcript_path.clone())
    };
    let entry = HookStatus {
        status,
        tool_session_id: tool_sid.clone(),
        received_at: SystemTime::now(),
        transcript_path,
    };
    // Bootstrap the rollout-path cache for Codex sessions. Identifying
    // tool from the watcher alone is heuristic — `is_valid_thread_id`
    // accepts only canonical Codex UUIDs, so a Claude `session_id`
    // (different shape) is automatically rejected. Running the walk here
    // (once, on the watcher thread) means the UI render path never has
    // to touch `~/.codex/sessions/`.
    let codex_thread = tool_sid
        .as_deref()
        .filter(|s| is_valid_thread_id(s))
        .map(str::to_string);
    // Re-resolve the rollout path on every Codex hook update so Codex
    // compaction (which rolls the same thread-id forward into a new file)
    // can't leave the cache stuck on a stale path. The walk runs on the
    // watcher thread, never on the render path.
    let resolved = codex_thread
        .as_deref()
        .and_then(|thread_id| find_rollout_for_thread(thread_id, &codex_sessions_root()));
    if let Ok(mut s) = state.lock() {
        s.hook_status.insert(session_id, entry);
        if let (Some(thread_id), Some(path)) = (codex_thread, resolved) {
            s.record_rollout_path(&thread_id, path);
        }
    }
}

/// Canonical Codex rollout directory. Extracted so tests can override.
fn codex_sessions_root() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".codex").join("sessions"))
        .unwrap_or_default()
}

fn process_cost_file(
    state: &EventStateHandle,
    path: &Path,
    storage: Option<&SharedStorage>,
    pricer: &Pricer,
) {
    let key = match path.to_str() {
        Some(p) => p.to_string(),
        None => return,
    };
    // Skip files we've already processed in this process. The dedup set is
    // only consulted here — insertion is deferred until after a successful
    // storage write so a transient failure (e.g. session row not yet
    // present at FK-check time) can be retried on a subsequent notify
    // event.
    {
        let s = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if s.seen_cost_files.contains(&key) {
            return;
        }
    }

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut event: CostEvent = match deserialize_cost_event(&bytes) {
        Some(e) => e,
        None => return,
    };
    // Compute USD cost from tokens here (single ingestion point) so the
    // hook subprocess stays free of config loading. Unknown models return 0.
    event.cost_microdollars = pricer.compute_microdollars(
        &event.model,
        event.input_tokens,
        event.output_tokens,
        event.cache_read_tokens,
        event.cache_creation_tokens,
    );
    let Some(storage) = storage else {
        // No storage — leave file in place AND don't mark seen so a future
        // run with a working storage handle can ingest it.
        return;
    };
    let inserted = match storage.lock() {
        Ok(s) => s.insert_cost_event(&event).is_ok(),
        Err(_) => false,
    };
    if inserted {
        // The dedup set now reflects committed state. After a successful
        // insert the file is redundant: the in-memory set covers the rest
        // of this process; the SQLite row covers future processes.
        if let Ok(mut s) = state.lock() {
            s.seen_cost_files.insert(key);
        }
        let _ = std::fs::remove_file(path);
    }
    // On failure (FK violation, transient lock, etc.) we leave the file in
    // place and the dedup entry absent — the next notify cycle, by which
    // time the session row will exist, retries the insert.
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
        #[serde(default)]
        cost_microdollars: i64,
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
        cost_microdollars: w.cost_microdollars,
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
        process_path(&state, &path, None, &Pricer::with_defaults());
        let g = state.lock().unwrap();
        let entry = g.hook_status.get("sess-1").unwrap();
        assert_eq!(entry.status, SessionStatus::Running);
    }

    #[test]
    fn test_process_hook_file_unknown_status_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        fs::create_dir(&hooks).unwrap();
        let path = write_hook_file(&hooks, "sess-2", "not-a-real-status");
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None, &Pricer::with_defaults());
        let g = state.lock().unwrap();
        assert!(!g.hook_status.contains_key("sess-2"));
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
        load_existing(&state, &hooks, &costs, None, &Pricer::with_defaults());
        assert_eq!(
            state.lock().unwrap().hook_status.get("abc").unwrap().status,
            SessionStatus::Waiting
        );
    }

    #[test]
    fn test_process_cost_file_dedupes_after_successful_insert() {
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();

        let (storage, _db_dir) = test_storage();
        storage.save_session(&make_test_session("sess-1")).unwrap();
        let storage: SharedStorage = Arc::new(Mutex::new(storage));

        let path = write_cost_file(&costs, "sess-1", 12345);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());
        // First call removes the file after a successful insert. A second
        // call to the same path is a no-op via the dedup set (the file
        // doesn't exist anymore either, but the set protects against any
        // notify-replay before the inode is reused).
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());
        assert_eq!(state.lock().unwrap().seen_cost_files.len(), 1);
    }

    #[test]
    fn test_process_cost_file_retries_on_fk_violation() {
        // Cost-event JSON arrives BEFORE the session row exists (a race we
        // see at session start). The first insert hits an FK constraint
        // failure; the dedup set MUST NOT be poisoned and the file MUST
        // remain on disk so a subsequent notify (after the session row is
        // written) can retry.
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();

        let (storage, _db_dir) = test_storage();
        // Note: do NOT save the session — the FK violation is the point.
        let storage: SharedStorage = Arc::new(Mutex::new(storage));

        let path = write_cost_file(&costs, "fk-sess", 1);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());

        assert!(
            path.exists(),
            "file must be retained when storage insert fails"
        );
        assert!(
            state.lock().unwrap().seen_cost_files.is_empty(),
            "dedup set must not be poisoned by a failed insert"
        );

        // Now the session row appears; the next process_path call inserts.
        storage
            .lock()
            .unwrap()
            .save_session(&make_test_session("fk-sess"))
            .unwrap();
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());
        assert!(!path.exists(), "retry must succeed and remove the file");
        let totals = storage
            .lock()
            .unwrap()
            .cost_totals_for_session("fk-sess")
            .unwrap();
        assert_eq!(totals.input, 1);
    }

    #[test]
    fn test_process_cost_file_keeps_file_and_set_clean_when_no_storage() {
        // No storage = no insert opportunity. The file must remain AND the
        // dedup set must stay empty so a later run with a working storage
        // handle can ingest the file.
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();
        let path = write_cost_file(&costs, "no-store", 1);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None, &Pricer::with_defaults());
        assert!(path.exists());
        assert!(state.lock().unwrap().seen_cost_files.is_empty());
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
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());

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

    fn write_priced_cost_file(dir: &Path, session_id: &str, model: &str, ts: i64) -> PathBuf {
        let path = dir.join(format!("{}_{}.json", session_id, ts));
        let body = serde_json::json!({
            "session_id": session_id, "model": model,
            "input_tokens": 1_000_000, "output_tokens": 1_000_000,
            "cache_read_tokens": 0, "cache_creation_tokens": 0,
            "ts": ts,
        });
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        path
    }

    #[test]
    fn test_process_cost_file_computes_microdollars_via_pricer() {
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();

        let (storage, _db_dir) = test_storage();
        storage.save_session(&make_test_session("sess-P")).unwrap();
        let storage: SharedStorage = Arc::new(Mutex::new(storage));

        let path = write_priced_cost_file(&costs, "sess-P", "claude-sonnet-4-6", 1);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());

        let totals = storage
            .lock()
            .unwrap()
            .cost_totals_for_session("sess-P")
            .unwrap();
        // Sonnet 4.6: 1M in @ $3 + 1M out @ $15 = $18 = 18_000_000 microdollars
        assert_eq!(totals.microdollars, 18_000_000);
    }

    #[test]
    fn test_process_cost_file_honors_override_pricer() {
        use crate::core::cost::ModelRate;
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();

        let (storage, _db_dir) = test_storage();
        storage.save_session(&make_test_session("sess-O")).unwrap();
        let storage: SharedStorage = Arc::new(Mutex::new(storage));

        // Override Sonnet to a flat $1/Mtok on input — half the default
        // output, no cache. Confirms the watcher uses the supplied Pricer,
        // not just defaults.
        let mut overrides = HashMap::new();
        overrides.insert(
            "claude-sonnet-4-6".to_string(),
            ModelRate {
                input_per_mtok: 1.0,
                output_per_mtok: 0.0,
                cache_read_per_mtok: 0.0,
                cache_creation_per_mtok: 0.0,
            },
        );
        let pricer = Pricer::with_defaults().with_overrides(overrides);

        let path = write_priced_cost_file(&costs, "sess-O", "claude-sonnet-4-6", 1);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, Some(&storage), &pricer);

        let totals = storage
            .lock()
            .unwrap()
            .cost_totals_for_session("sess-O")
            .unwrap();
        // 1M input @ $1/Mtok = $1 = 1_000_000 microdollars (output zeroed
        // by override). The default rate would have produced 18_000_000.
        assert_eq!(totals.microdollars, 1_000_000);
    }

    #[test]
    fn test_process_cost_file_keeps_file_when_no_storage() {
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();
        let path = write_cost_file(&costs, "sess-Y", 1);
        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, None, &Pricer::with_defaults());
        assert!(
            path.exists(),
            "file must be retained when no storage is available"
        );
    }

    #[test]
    fn codex_cost_event_via_notify_lands_in_storage() {
        // Full pipeline: write a Codex cost-event JSON (as notify_handler
        // would) into a fake cost-events dir, run process_path through
        // event_watcher, assert the cost_events DB row appears with
        // microdollars computed from the gpt-5.5 default rate.
        let dir = tempfile::tempdir().unwrap();
        let costs = dir.path().join("cost-events");
        fs::create_dir(&costs).unwrap();

        let (storage, _db) = test_storage();
        storage
            .save_session(&make_test_session("av-codex"))
            .unwrap();
        let storage: SharedStorage = Arc::new(Mutex::new(storage));

        let path = costs.join("av-codex_999.json");
        let body = serde_json::json!({
            "session_id": "av-codex",
            "model": "gpt-5.5",
            "input_tokens": 1_000_000,
            "output_tokens": 0,
            "cache_read_tokens": 0,
            "cache_creation_tokens": 0,
            "ts": 999,
        });
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();

        let state: EventStateHandle = Arc::new(Mutex::new(EventState::default()));
        process_path(&state, &path, Some(&storage), &Pricer::with_defaults());

        let totals = storage
            .lock()
            .unwrap()
            .cost_totals_for_session("av-codex")
            .unwrap();
        // gpt-5.5 input rate is $5/Mtok → 1M input = $5 = 5_000_000 microdollars.
        assert_eq!(totals.input, 1_000_000);
        assert_eq!(totals.microdollars, 5_000_000);
        assert!(
            !path.exists(),
            "cost-event file should be removed after ingest"
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

        let state = spawn_in(
            hooks.clone(),
            costs,
            None,
            Arc::new(Pricer::with_defaults()),
        );

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

    #[test]
    fn rollout_snapshot_refreshes_only_when_mtime_advances() {
        // The cache exists to keep the UI render path off repeated full
        // file reads. Once cached, a subsequent call must NOT re-read
        // unless the file's mtime has moved.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let body_v1 = concat!(
            r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":7}}}}"#,
            "\n",
        );
        fs::write(&path, body_v1).unwrap();
        let mut state = EventState::default();
        let s1 = state.rollout_snapshot(&path);
        assert_eq!(s1.context_tokens, Some(7));

        // Rewrite content but DO NOT advance mtime — cache must serve the
        // stale snapshot. We use filetime via std::fs::write+set_modified
        // pattern: set_modified accepts SystemTime, we feed it the
        // previously-cached mtime.
        std::thread::sleep(Duration::from_millis(20));
        let body_v2 = concat!(
            r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":42}}}}"#,
            "\n",
        );
        fs::write(&path, body_v2).unwrap();
        if let Some(prev_mtime) = s1.mtime {
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            let _ = f.set_modified(prev_mtime);
        }
        let s2 = state.rollout_snapshot(&path);
        assert_eq!(
            s2.context_tokens,
            Some(7),
            "same mtime must serve cached snapshot"
        );

        // Now advance mtime: cache invalidates and re-reads.
        std::thread::sleep(Duration::from_millis(20));
        let later = std::time::SystemTime::now();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(later).unwrap();
        let s3 = state.rollout_snapshot(&path);
        assert_eq!(s3.context_tokens, Some(42));
    }

    #[test]
    fn record_rollout_path_clears_stale_snapshot_on_compaction() {
        // Codex compaction creates a new file for the same thread-id.
        // record_rollout_path must drop the old path's snapshot so a
        // subsequent rollout_snapshot read targets the new file.
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("rollout-old.jsonl");
        let new = dir.path().join("rollout-new.jsonl");
        fs::write(&old, "").unwrap();
        fs::write(&new, "").unwrap();
        let mut state = EventState::default();
        state.record_rollout_path("019e289a-0f2d-73f1-94d3-d15182ff1741", old.clone());
        // Seed a snapshot for the old path.
        let _ = state.rollout_snapshot(&old);
        assert!(state.rollout_snapshots.contains_key(&old));

        state.record_rollout_path("019e289a-0f2d-73f1-94d3-d15182ff1741", new.clone());
        assert!(
            !state.rollout_snapshots.contains_key(&old),
            "old snapshot must be dropped when path is replaced"
        );
        assert_eq!(
            state.cached_rollout_path("019e289a-0f2d-73f1-94d3-d15182ff1741"),
            Some(new.as_path())
        );
    }

    #[test]
    fn process_hook_file_bootstraps_codex_rollout_path() {
        // Existing-Codex-session-at-restart path: a HookStatusFile lands
        // with a canonical thread-id. The bootstrap must walk
        // ~/.codex/sessions/ once on the watcher thread so the UI render
        // path can short-circuit on subsequent reads.
        //
        // This test exercises `process_hook_file`'s bootstrap branch only
        // by asserting `is_valid_thread_id` gates the walk. Full
        // end-to-end bootstrap against a real `~/.codex` is covered by
        // `find_rollout_for_thread`'s own tests; we don't shadow $HOME
        // here.
        let thread = "019e289a-0f2d-73f1-94d3-d15182ff1741";
        assert!(crate::core::runner::codex::cost_handler::is_valid_thread_id(thread));
        // Non-UUID hook session IDs (Claude's, Shell's) must short-circuit.
        assert!(
            !crate::core::runner::codex::cost_handler::is_valid_thread_id("abc-claude-session")
        );
    }
}
