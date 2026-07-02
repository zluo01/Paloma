use std::{
    cmp::{Ordering, Reverse},
    collections::{BinaryHeap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock, atomic, atomic::AtomicUsize, mpsc},
    thread,
    time::Duration,
};

use ignore::{WalkBuilder, WalkState};
use log::{debug, error, info, warn};
use notify::{EventKind, RecursiveMode, event::ModifyKind};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use rayon::prelude::*;

use crate::capability::{
    Action, ActionOutcome, Capability, CapabilityMeta, IconRef, Item, QueryHandler,
    native::copy_to_clipboard,
};

const MIN_QUERY_CHARS: usize = 2;
const MAX_RESULTS: usize = 30;
const MAX_ENTRIES: usize = 1_000_000;
/// A few ranking tasks per worker lets work-stealing balance uneven chunks
/// without drowning in per-task matcher setup (adaptive `fold` benched 3-5x
/// slower than sized chunks).
const RANK_TASKS_PER_THREAD: usize = 4;
/// Below this many entries, splitting costs more than it buys.
const RANK_MIN_CHUNK: usize = 1024;
const DEBOUNCE: Duration = Duration::from_millis(500);
const EXCLUDED_DIRS: &[&str] = &[
    "__MACOSX",
    "__pycache__",
    "bower_components",
    "lost+found",
    "node_modules",
    "venv",
];

const OPEN_ACTION_LABEL: &str = "Open";
const OPEN_FOLDER_ACTION_LABEL: &str = "Open Folder";
const COPY_PATH_ACTION_LABEL: &str = "Copy Path";
const FOLDER_ICON: &str = "folder";
const FALLBACK_ICON: &str = "text-x-generic";

type FsWatcher = Debouncer<notify::RecommendedWatcher, RecommendedCache>;

struct FileEntry {
    name: String,
    path: PathBuf,
    /// component count of `path`, precomputed so ranking ties are cheap
    depth: usize,
    is_dir: bool,
}

impl FileEntry {
    fn new(name: String, path: PathBuf, is_dir: bool) -> Self {
        let depth = path.components().count();
        Self {
            name,
            path,
            depth,
            is_dir,
        }
    }
}

/// A scored entry; `Ord` is "greater is a better result" so a min-heap of
/// `Reverse<Candidate>` keeps the best [`MAX_RESULTS`].
struct Candidate<'a> {
    score: u32,
    entry: &'a FileEntry,
}

impl Ord for Candidate<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| other.entry.depth.cmp(&self.entry.depth))
            .then_with(|| other.entry.name.cmp(&self.entry.name))
    }
}

impl PartialOrd for Candidate<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Candidate<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate<'_> {}

pub struct FileSearch {
    entries: Arc<RwLock<Vec<FileEntry>>>,
    home: PathBuf,
    _watcher: Arc<Mutex<FsWatcher>>,
}

impl Capability for FileSearch {
    fn id(&self) -> &'static str {
        "file_search"
    }

    fn metadata(&self) -> CapabilityMeta {
        CapabilityMeta {
            name: "Files".to_string(),
            description: "Search files in your home directory.".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            icon: None,
            homepage: None,
            author: None,
        }
    }
}

impl QueryHandler for FileSearch {
    fn query(&self, input: &str) -> Vec<Item> {
        let entries = self.entries.read().unwrap();
        rank(input, &entries)
            .into_iter()
            .map(|entry| build_item(&self.home, entry))
            .collect()
    }

    fn run(&self, action: Action) -> ActionOutcome {
        let Some(target) = action.params.into_iter().next() else {
            error!("file_search: action with no payload");
            return ActionOutcome::Hide;
        };

        match action.label.as_str() {
            OPEN_ACTION_LABEL | OPEN_FOLDER_ACTION_LABEL => open_path(&target),
            COPY_PATH_ACTION_LABEL => copy_to_clipboard(&target),
            other => error!("file_search: unknown action label: {other}"),
        }

        ActionOutcome::Hide
    }
}

impl FileSearch {
    pub fn new() -> notify::Result<Self> {
        let Some(home) = std::env::home_dir() else {
            return Err(notify::Error::generic("home directory is not set"));
        };

        let entries: Arc<RwLock<Vec<FileEntry>>> = Arc::new(RwLock::new(Vec::new()));

        // The debouncer callback cannot touch the debouncer that owns it, so
        // it only forwards affected directories to the maintenance thread.
        let (dirty_tx, dirty_rx) = mpsc::channel::<Vec<PathBuf>>();
        let debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let dirs = dirty_dirs(&events);
            if !dirs.is_empty() {
                debug!("file_search: rescanning {dirs:?}");
                let _ = dirty_tx.send(dirs);
            }
        })?;
        let watcher = Arc::new(Mutex::new(debouncer));

        // initial index in the background; queries see an empty index until done
        {
            let entries = Arc::clone(&entries);
            let watcher = Arc::clone(&watcher);
            let home = home.clone();
            thread::Builder::new()
                .name("scry-file-index".into())
                .spawn(move || {
                    let (found, dirs) = scan(&home);
                    info!("file_search: indexed {} entries", found.len());
                    *entries.write().unwrap() = found;
                    watch_dirs(&dirs, &watcher);
                })
                .expect("spawn file index thread");
        }

        {
            let entries = Arc::clone(&entries);
            let watcher = Arc::clone(&watcher);
            let home = home.clone();
            thread::Builder::new()
                .name("scry-file-watch".into())
                .spawn(move || {
                    while let Ok(first) = dirty_rx.recv() {
                        // coalesce the whole backlog into one rescan cycle
                        let mut dirty = first;
                        while let Ok(more) = dirty_rx.try_recv() {
                            dirty.extend(more);
                        }
                        dirty.sort();
                        dirty.dedup();
                        rescan_dirs(&dirty, &home, &entries, &watcher);
                    }
                })
                .expect("spawn file watch thread");
        }

        Ok(Self {
            entries,
            home,
            _watcher: watcher,
        })
    }
}

fn dirty_dirs(events: &[DebouncedEvent]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_))
            )
        })
        .flat_map(|event| event.paths.iter())
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Shared walker configuration: skips hidden files, honors gitignore rules,
/// and prunes [`EXCLUDED_DIRS`] subtrees.
fn walker(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.follow_links(false).filter_entry(|entry| {
        !(entry.file_type().is_some_and(|t| t.is_dir())
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| EXCLUDED_DIRS.contains(&name)))
    });
    builder
}

/// Walk `root` in parallel; returns the entries below it and every directory
/// to watch (including `root` itself).
fn scan(root: &Path) -> (Vec<FileEntry>, Vec<PathBuf>) {
    let (tx, rx) = mpsc::channel::<FileEntry>();
    let count = AtomicUsize::new(0);

    walker(root).build_parallel().run(|| {
        let tx = tx.clone();
        let count = &count;
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            if entry.depth() == 0 {
                return WalkState::Continue;
            }
            if count.fetch_add(1, atomic::Ordering::Relaxed) >= MAX_ENTRIES {
                return WalkState::Quit;
            }

            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            let name = entry.file_name().to_string_lossy().into_owned();
            let _ = tx.send(FileEntry::new(name, entry.into_path(), is_dir));
            WalkState::Continue
        })
    });
    drop(tx);

    let found: Vec<FileEntry> = rx.iter().collect();
    if found.len() >= MAX_ENTRIES {
        warn!("file_search: entry cap {MAX_ENTRIES} reached; index is partial");
    }

    let dirs = std::iter::once(root.to_path_buf())
        .chain(found.iter().filter(|e| e.is_dir).map(|e| e.path.clone()))
        .collect();
    (found, dirs)
}

fn watch_dirs(dirs: &[PathBuf], watcher: &Mutex<FsWatcher>) {
    let mut guard = watcher.lock().unwrap();
    let failures = dirs
        .iter()
        .filter(|dir| guard.watch(dir, RecursiveMode::NonRecursive).is_err())
        .count();
    if failures > 0 {
        warn!("file_search: failed to watch {failures} directories; results there may go stale");
    }
}

struct DirState {
    indexed: bool,
    known: Vec<(PathBuf, bool)>,
}

/// Entries to drop from the index: exact file paths and whole subtrees.
#[derive(Default)]
struct Removals {
    files: Vec<PathBuf>,
    subtrees: Vec<PathBuf>,
}

fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// Byte-level `Path::parent`, avoiding component parsing.
fn parent_bytes(path: &Path) -> Option<&[u8]> {
    let bytes = path_bytes(path);
    if bytes.len() <= 1 {
        return None;
    }
    let idx = bytes.iter().rposition(|&b| b == b'/')?;
    Some(&bytes[..idx.max(1)])
}

/// `bytes` is `prefix` itself or a path below it.
fn in_subtree(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.starts_with(prefix) && (bytes.len() == prefix.len() || bytes[prefix.len()] == b'/')
}

/// Re-list changed directories and diff them against the index. The whole
/// batch costs one pass over the index, regardless of how many directories
/// are dirty.
fn rescan_dirs(
    dirty: &[PathBuf],
    home: &Path,
    entries: &RwLock<Vec<FileEntry>>,
    watcher: &Mutex<FsWatcher>,
) {
    if dirty.is_empty() {
        return;
    }

    // Single read pass: which dirty dirs are indexed, and their known
    // children. Only indexed directories are rescanned; events can name
    // ancestors of home (e.g. an event on home itself), and rescanning those
    // would re-index the whole tree as "new".
    let mut states: HashMap<&[u8], DirState> = dirty
        .iter()
        .map(|dir| {
            let state = DirState {
                indexed: dir.as_path() == home,
                known: Vec::new(),
            };
            (path_bytes(dir), state)
        })
        .collect();
    {
        let guard = entries.read().unwrap();
        for entry in guard.iter() {
            if entry.is_dir
                && let Some(state) = states.get_mut(path_bytes(&entry.path))
            {
                state.indexed = true;
            }
            if let Some(parent) = parent_bytes(&entry.path)
                && let Some(state) = states.get_mut(parent)
            {
                state.known.push((entry.path.clone(), entry.is_dir));
            }
        }
    }

    let mut removals = Removals::default();
    let mut additions: Vec<FileEntry> = Vec::new();

    for dir in dirty {
        let Some(state) = states.remove(path_bytes(dir)) else {
            continue;
        };
        if !state.indexed {
            continue;
        }
        if !dir.is_dir() {
            removals.subtrees.push(dir.clone());
            continue;
        }

        let mut current: Vec<FileEntry> = Vec::new();
        for result in walker(dir).max_depth(Some(1)).build() {
            let Ok(entry) = result else { continue };
            if entry.depth() == 0 {
                continue;
            }
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            let name = entry.file_name().to_string_lossy().into_owned();
            current.push(FileEntry::new(name, entry.into_path(), is_dir));
        }

        // Diff on (path, kind): a path replaced by the other kind (file ->
        // dir or dir -> file) must be removed and re-added, not treated as
        // unchanged.
        let current_kinds: HashMap<&[u8], bool> = current
            .iter()
            .map(|e| (path_bytes(&e.path), e.is_dir))
            .collect();
        for (path, was_dir) in &state.known {
            if current_kinds.get(path_bytes(path)) != Some(was_dir) {
                if *was_dir {
                    removals.subtrees.push(path.clone());
                } else {
                    removals.files.push(path.clone());
                }
            }
        }

        let known_kinds: HashMap<&[u8], bool> = state
            .known
            .iter()
            .map(|(path, is_dir)| (path_bytes(path), *is_dir))
            .collect();
        for entry in current {
            if known_kinds.get(path_bytes(&entry.path)) == Some(&entry.is_dir) {
                continue;
            }
            additions.push(entry);
        }
    }

    apply_removals(&removals, entries, watcher);

    for entry in additions {
        if entry.is_dir {
            let (found, dirs) = scan(&entry.path);
            entries.write().unwrap().extend(found);
            watch_dirs(&dirs, watcher);
        }
        entries.write().unwrap().push(entry);
    }
}

/// Drop all removed files and subtrees in one pass over the index.
fn apply_removals(
    removals: &Removals,
    entries: &RwLock<Vec<FileEntry>>,
    watcher: &Mutex<FsWatcher>,
) {
    if removals.files.is_empty() && removals.subtrees.is_empty() {
        return;
    }

    let files: HashSet<&[u8]> = removals.files.iter().map(|p| path_bytes(p)).collect();
    let subtrees: Vec<&[u8]> = removals.subtrees.iter().map(|p| path_bytes(p)).collect();

    let mut unwatch: Vec<PathBuf> = Vec::new();
    entries.write().unwrap().retain(|entry| {
        let bytes = path_bytes(&entry.path);
        let dead = files.contains(bytes) || subtrees.iter().any(|prefix| in_subtree(bytes, prefix));
        if dead && entry.is_dir {
            unwatch.push(entry.path.clone());
        }
        !dead
    });

    let mut guard = watcher.lock().unwrap();
    for dir in &unwatch {
        let _ = guard.unwatch(dir);
    }
}

fn rank<'a>(input: &str, entries: &'a [FileEntry]) -> Vec<&'a FileEntry> {
    let input = input.trim();
    if input.chars().count() < MIN_QUERY_CHARS {
        return Vec::new();
    }

    let pattern = Pattern::parse(input, CaseMatching::Smart, Normalization::Smart);

    let chunk_size = entries
        .len()
        .div_ceil(rayon::current_num_threads() * RANK_TASKS_PER_THREAD)
        .max(RANK_MIN_CHUNK);
    let mut merged: Vec<Candidate> = entries
        .par_chunks(chunk_size)
        .flat_map_iter(|chunk| top_matches(&pattern, chunk))
        .collect();

    merged.sort_unstable_by(|left, right| right.cmp(left));
    merged.truncate(MAX_RESULTS);
    merged
        .into_iter()
        .map(|candidate| candidate.entry)
        .collect()
}

/// Single pass over one chunk keeping only its best [`MAX_RESULTS`] matches.
fn top_matches<'a>(pattern: &Pattern, entries: &'a [FileEntry]) -> Vec<Candidate<'a>> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut best: BinaryHeap<Reverse<Candidate>> = BinaryHeap::with_capacity(MAX_RESULTS + 1);

    for entry in entries {
        let haystack = Utf32Str::new(&entry.name, &mut buf);
        let Some(score) = pattern.score(haystack, &mut matcher) else {
            continue;
        };

        let candidate = Candidate { score, entry };
        if best.len() < MAX_RESULTS {
            best.push(Reverse(candidate));
        } else if best.peek().is_some_and(|Reverse(worst)| *worst < candidate) {
            best.pop();
            best.push(Reverse(candidate));
        }
    }

    best.into_iter()
        .map(|Reverse(candidate)| candidate)
        .collect()
}

fn build_item(home: &Path, entry: &FileEntry) -> Item {
    let path = entry.path.to_string_lossy().into_owned();

    let mut actions = vec![Action {
        label: OPEN_ACTION_LABEL.into(),
        params: vec![path.clone()],
        primary: true,
    }];
    // directories open themselves; only files need a way to their folder
    if !entry.is_dir {
        let parent = entry
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        actions.push(Action {
            label: OPEN_FOLDER_ACTION_LABEL.into(),
            params: vec![parent],
            primary: false,
        });
    }
    actions.push(Action {
        label: COPY_PATH_ACTION_LABEL.into(),
        params: vec![path],
        primary: false,
    });

    Item {
        title: entry.name.clone(),
        subtitle: Some(display_parent(home, &entry.path)),
        icon: Some(IconRef::Name(icon_name(entry))),
        actions,
    }
}

/// Parent directory for display, with home abbreviated to `~`.
fn display_parent(home: &Path, path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    match parent.strip_prefix(home) {
        Ok(rel) if rel.as_os_str().is_empty() => "~".to_string(),
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => parent.display().to_string(),
    }
}

/// Freedesktop icon name for the entry's mime type, e.g. `text-plain`.
fn icon_name(entry: &FileEntry) -> String {
    if entry.is_dir {
        return FOLDER_ICON.to_string();
    }
    match mime_guess::from_path(&entry.path).first() {
        Some(mime) => mime.essence_str().replace('/', "-"),
        None => FALLBACK_ICON.to_string(),
    }
}

fn open_path(target: &str) {
    match open::that_detached(target) {
        Ok(()) => debug!("file_search: opened {target}"),
        Err(err) => error!("file_search: failed to open {target}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Instant};

    use notify::{
        Event,
        event::{AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, RenameMode},
    };
    use tempfile::TempDir;

    use super::*;

    fn debounced(kind: EventKind, path: &str) -> DebouncedEvent {
        DebouncedEvent {
            event: Event::new(kind).add_path(PathBuf::from(path)),
            time: Instant::now(),
        }
    }

    fn touch(path: &Path) {
        fs::write(path, b"").unwrap();
    }

    fn entry(name: &str, path: &str, is_dir: bool) -> FileEntry {
        FileEntry::new(name.to_string(), PathBuf::from(path), is_dir)
    }

    fn test_watcher() -> Mutex<FsWatcher> {
        Mutex::new(new_debouncer(DEBOUNCE, None, |_: DebounceEventResult| {}).unwrap())
    }

    #[test]
    fn dirty_dirs_maps_structural_events_to_parents() {
        let events = [
            debounced(EventKind::Create(CreateKind::File), "/home/u/docs/a.txt"),
            debounced(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                "/home/u/docs/b.txt",
            ),
            debounced(
                EventKind::Remove(notify::event::RemoveKind::File),
                "/home/u/pics/c.png",
            ),
        ];

        assert_eq!(
            dirty_dirs(&events),
            [PathBuf::from("/home/u/docs"), PathBuf::from("/home/u/pics")]
        );
    }

    #[test]
    fn dirty_dirs_ignores_content_and_metadata_events() {
        let events = [
            debounced(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                "/home/u/app.log",
            ),
            debounced(
                EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions)),
                "/home/u/docs/a.txt",
            ),
            debounced(
                EventKind::Access(AccessKind::Close(AccessMode::Write)),
                "/home/u/app.log",
            ),
        ];

        assert!(dirty_dirs(&events).is_empty());
    }

    #[test]
    fn scan_indexes_files_and_directories() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        touch(&root.path().join("docs/report.txt"));

        let (found, dirs) = scan(root.path());

        let names: Vec<_> = found.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"docs"));
        assert!(names.contains(&"report.txt"));
        assert!(dirs.contains(&root.path().to_path_buf()));
        assert!(dirs.contains(&root.path().join("docs")));
    }

    #[test]
    fn scan_skips_hidden_files() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join(".hidden_zzq"));

        let (found, _) = scan(root.path());

        assert!(found.is_empty());
    }

    #[test]
    fn scan_skips_excluded_directories() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        touch(&root.path().join("node_modules/package.json"));

        let (found, _) = scan(root.path());

        assert!(found.is_empty());
    }

    #[test]
    fn rank_matches_fuzzily() {
        let entries = vec![
            entry("Cargo.toml", "/home/u/proj/Cargo.toml", false),
            entry("photo.png", "/home/u/photo.png", false),
        ];

        let ranked = rank("cargtom", &entries);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].name, "Cargo.toml");
    }

    #[test]
    fn rank_prefers_shallower_paths_on_ties() {
        let entries = vec![
            entry("notes.md", "/home/u/a/b/c/notes.md", false),
            entry("notes.md", "/home/u/notes.md", false),
        ];

        let ranked = rank("notes", &entries);

        assert_eq!(ranked[0].path, PathBuf::from("/home/u/notes.md"));
    }

    #[test]
    fn rank_rejects_short_queries() {
        let entries = vec![entry("a.txt", "/home/u/a.txt", false)];

        assert!(rank("a", &entries).is_empty());
    }

    #[test]
    fn rank_caps_results() {
        let entries: Vec<FileEntry> = (0..100)
            .map(|i| entry("match.txt", &format!("/home/u/{i}/match.txt"), false))
            .collect();

        assert_eq!(rank("match", &entries).len(), MAX_RESULTS);
    }

    #[test]
    fn rescan_picks_up_created_files() {
        let root = TempDir::new().unwrap();
        let entries = RwLock::new(Vec::new());
        let watcher = test_watcher();

        touch(&root.path().join("new.txt"));
        rescan_dirs(
            &[root.path().to_path_buf()],
            root.path(),
            &entries,
            &watcher,
        );

        let guard = entries.read().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].name, "new.txt");
    }

    #[test]
    fn rescan_drops_deleted_subtrees() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("gone");
        let entries = RwLock::new(vec![
            FileEntry::new("gone".into(), dir.clone(), true),
            FileEntry::new("inner.txt".into(), dir.join("inner.txt"), false),
        ]);
        let watcher = test_watcher();

        rescan_dirs(
            &[root.path().to_path_buf()],
            root.path(),
            &entries,
            &watcher,
        );

        assert!(entries.read().unwrap().is_empty());
    }

    #[test]
    fn rescan_replaces_type_changed_entries() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("thing");
        // indexed as a file, but on disk it is now a directory
        let entries = RwLock::new(vec![FileEntry::new("thing".into(), target.clone(), false)]);
        let watcher = test_watcher();
        fs::create_dir(&target).unwrap();

        rescan_dirs(
            &[root.path().to_path_buf()],
            root.path(),
            &entries,
            &watcher,
        );

        let guard = entries.read().unwrap();
        assert_eq!(guard.len(), 1);
        assert!(guard[0].is_dir);
    }

    #[test]
    fn rescan_handles_multiple_directories_per_cycle() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        fs::create_dir(root.path().join("b")).unwrap();
        let entries = RwLock::new(vec![
            FileEntry::new("a".into(), root.path().join("a"), true),
            FileEntry::new("b".into(), root.path().join("b"), true),
        ]);
        let watcher = test_watcher();
        touch(&root.path().join("a/x.txt"));
        touch(&root.path().join("b/y.txt"));

        rescan_dirs(
            &[root.path().join("a"), root.path().join("b")],
            root.path(),
            &entries,
            &watcher,
        );

        let guard = entries.read().unwrap();
        let names: Vec<&str> = guard.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"x.txt"));
        assert!(names.contains(&"y.txt"));
    }

    #[test]
    fn rescan_ignores_unindexed_directories() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        touch(&outside.path().join("stray.txt"));
        let entries = RwLock::new(Vec::new());
        let watcher = test_watcher();

        rescan_dirs(
            &[outside.path().to_path_buf()],
            root.path(),
            &entries,
            &watcher,
        );

        assert!(entries.read().unwrap().is_empty());
    }

    #[test]
    fn files_offer_open_folder_action() {
        let item = build_item(
            Path::new("/home/u"),
            &entry("a.txt", "/home/u/docs/a.txt", false),
        );

        let labels: Vec<_> = item.actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                OPEN_ACTION_LABEL,
                OPEN_FOLDER_ACTION_LABEL,
                COPY_PATH_ACTION_LABEL
            ]
        );
        assert_eq!(item.actions[1].params, vec!["/home/u/docs".to_string()]);
    }

    #[test]
    fn directories_omit_open_folder_action() {
        let item = build_item(Path::new("/home/u"), &entry("docs", "/home/u/docs", true));

        let labels: Vec<_> = item.actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, [OPEN_ACTION_LABEL, COPY_PATH_ACTION_LABEL]);
    }

    #[test]
    fn display_parent_abbreviates_home() {
        let home = Path::new("/home/u");

        assert_eq!(
            display_parent(home, Path::new("/home/u/docs/report.txt")),
            "~/docs"
        );
        assert_eq!(display_parent(home, Path::new("/home/u/report.txt")), "~");
    }

    #[test]
    fn icon_name_maps_mime_types() {
        assert_eq!(
            icon_name(&entry("a.png", "/home/u/a.png", false)),
            "image-png"
        );
        assert_eq!(icon_name(&entry("docs", "/home/u/docs", true)), FOLDER_ICON);
        assert_eq!(
            icon_name(&entry("noext", "/home/u/noext", false)),
            FALLBACK_ICON
        );
    }
}
