use std::{
    borrow::Cow,
    cmp::{Ordering, Reverse},
    collections::{BinaryHeap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock, atomic, atomic::AtomicUsize, mpsc},
    thread,
    time::Duration,
};

use async_trait::async_trait;
use ignore::{WalkBuilder, WalkState};
use log::{debug, error, info, warn};
use notify::{EventKind, RecursiveMode, event::ModifyKind};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, new_debouncer_opt,
};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use paloma_extension_base::{Capability, SearchHandler, ToolHandler};
use paloma_extension_protocol::v1::{
    Action, CapabilityIcon, Hide, Item, ToolContent, ToolFacet, run_action_response::Behavior,
};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::clipboard::copy_to_clipboard;

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
#[cfg(not(windows))]
const FOLDER_ICON: &str = "folder";
#[cfg(not(windows))]
const FALLBACK_ICON: &str = "text-x-generic";

type FsWatcher = Debouncer<notify::RecommendedWatcher, NoCache>;

struct FileEntry {
    path: PathBuf,
    /// component count of `path`, precomputed so ranking ties are cheap
    depth: u16,
    is_dir: bool,
}

impl FileEntry {
    fn new(path: PathBuf, is_dir: bool) -> Self {
        let depth = path.components().count() as u16;
        Self {
            path,
            depth,
            is_dir,
        }
    }

    fn name(&self) -> Cow<'_, str> {
        self.path.file_name().unwrap_or_default().to_string_lossy()
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
            .then_with(|| other.entry.name().cmp(&self.entry.name()))
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
    roots: Vec<PathBuf>,
    _watcher: Arc<Mutex<FsWatcher>>,
}

impl Capability for FileSearch {
    fn id(&self) -> &str {
        "Files"
    }

    fn description(&self) -> &str {
        "Search files in your personal folders."
    }

    fn search_handler(&self) -> Option<&dyn SearchHandler> {
        Some(self)
    }

    fn tool_handler(&self) -> Option<&dyn ToolHandler> {
        Some(self)
    }
}

impl SearchHandler for FileSearch {
    fn search(&self, input: &str) -> Vec<Item> {
        let entries = self.entries.read().unwrap();
        rank(input, &entries)
            .into_iter()
            .map(|entry| build_item(&self.roots, entry))
            .collect()
    }

    fn run_search_action(&self, action: Action) -> Behavior {
        let Some(target) = action.params.into_iter().next() else {
            error!("file_search: action with no payload");
            return Behavior::Hide(Hide {});
        };

        match action.label.as_str() {
            OPEN_ACTION_LABEL | OPEN_FOLDER_ACTION_LABEL => open_path(&target),
            COPY_PATH_ACTION_LABEL => copy_to_clipboard(&target),
            other => error!("file_search: unknown action label: {other}"),
        }

        Behavior::Hide(Hide {})
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileSearchArgs {
    /// A literal fragment of the file or directory name to search for,
    /// at least 2 characters. Matches names only — never paths, globs,
    /// or regex ("*.pdf" matches nothing; use "pdf" instead).
    ///
    /// A single word matches fuzzily: its characters must appear in
    /// the name in order but need not be adjacent, so "cargtom" finds
    /// "Cargo.toml". Multiple whitespace-separated words must each
    /// appear in the name as an exact contiguous substring, in any
    /// order: "annual report" finds "annual_report_2026.pdf" but not
    /// "annual_summary.pdf".
    ///
    /// Smart case: an all-lowercase word matches case-insensitively; a
    /// word containing an uppercase letter matches case-sensitively.
    pub query: String,
}

#[async_trait]
impl ToolHandler for FileSearch {
    fn facet(&self) -> ToolFacet {
        ToolFacet {
            description: include_str!("description.md").to_string(),
            parameters: serde_json::to_string(&schemars::schema_for!(FileSearchArgs))
                .expect("JsonSchema output is always serializable"),
        }
    }

    async fn invoke(
        &self,
        _session_id: &str,
        _call_id: &str,
        arguments: &str,
    ) -> Result<ToolContent, String> {
        let args: FileSearchArgs = serde_json::from_str(arguments).map_err(|e| e.to_string())?;
        let entries = self.entries.read().unwrap();
        tool_results(&args.query, &entries)
    }

    /// Everything synchronize, nothing to cancel.
    async fn cancel(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

impl FileSearch {
    pub fn new() -> notify::Result<Self> {
        let roots = roots()?;

        let entries: Arc<RwLock<Vec<FileEntry>>> = Arc::new(RwLock::new(Vec::new()));

        // The debouncer callback cannot touch the debouncer that owns it, so
        // it only forwards affected directories to the maintenance thread.
        let (dirty_tx, dirty_rx) = mpsc::channel::<Vec<PathBuf>>();
        let debouncer: FsWatcher = new_debouncer_opt(
            DEBOUNCE,
            None,
            move |result: DebounceEventResult| {
                let Ok(events) = result else { return };
                let dirs = dirty_dirs(&events);
                if !dirs.is_empty() {
                    debug!("file_search: rescanning {dirs:?}");
                    let _ = dirty_tx.send(dirs);
                }
            },
            NoCache,
            notify::Config::default(),
        )?;
        let watcher = Arc::new(Mutex::new(debouncer));

        // initial index in the background
        {
            let entries = Arc::clone(&entries);
            let watcher = Arc::clone(&watcher);
            let roots = roots.clone();
            thread::Builder::new()
                .name("paloma-file-index".into())
                .spawn(move || {
                    let mut total = 0;
                    let count = AtomicUsize::new(0);
                    for root in &roots {
                        let scanned = scan(root, &count);
                        watch_initial(root, &scanned, &watcher);
                        total += scanned.len();
                        entries.write().unwrap().extend(scanned);
                    }
                    info!("file_search: indexed {total} entries");
                })
                .expect("spawn file index thread");
        }

        {
            let entries = Arc::clone(&entries);
            let watcher = Arc::clone(&watcher);
            let roots = roots.clone();
            thread::Builder::new()
                .name("paloma-file-watch".into())
                .spawn(move || {
                    while let Ok(first) = dirty_rx.recv() {
                        // coalesce the whole backlog into one rescan cycle
                        let mut dirty = first;
                        while let Ok(more) = dirty_rx.try_recv() {
                            dirty.extend(more);
                        }
                        dirty.sort();
                        dirty.dedup();
                        rescan_dirs(&dirty, &roots, &entries, &watcher);
                    }
                })
                .expect("spawn file watch thread");
        }

        Ok(Self {
            entries,
            roots,
            _watcher: watcher,
        })
    }
}

#[cfg(not(windows))]
fn roots() -> notify::Result<Vec<PathBuf>> {
    match std::env::home_dir() {
        Some(home) => Ok(vec![home]),
        None => Err(notify::Error::generic("home directory is not set")),
    }
}

#[cfg(windows)]
fn roots() -> notify::Result<Vec<PathBuf>> {
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
        FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_Videos,
    };

    let found = [
        &FOLDERID_Profile,
        &FOLDERID_Desktop,
        &FOLDERID_Documents,
        &FOLDERID_Downloads,
        &FOLDERID_Music,
        &FOLDERID_Pictures,
        &FOLDERID_Videos,
    ]
    .into_iter()
    .filter_map(known_folder_path)
    .collect();

    let roots = dedupe_roots(found);
    if roots.is_empty() {
        return Err(notify::Error::generic("no user folders resolved"));
    }
    Ok(roots)
}

#[cfg(windows)]
pub(crate) fn known_folder_path(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    };

    let pw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let path = unsafe { pw.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(pw.as_ptr() as *const std::ffi::c_void)) };
    path.map(PathBuf::from)
}

/// Only keep the common share roots
#[cfg(windows)]
fn dedupe_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for root in roots {
        if kept.iter().any(|k| root.starts_with(k)) {
            continue;
        }
        kept.retain(|k| !k.starts_with(&root));
        kept.push(root);
    }
    kept
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
/// to watch (including `root` itself). `count` carries the [`MAX_ENTRIES`]
/// budget so multi-root callers share one global cap.
fn scan(root: &Path, count: &AtomicUsize) -> Vec<FileEntry> {
    let (tx, rx) = mpsc::channel::<FileEntry>();

    walker(root).build_parallel().run(|| {
        let tx = tx.clone();
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
            let _ = tx.send(FileEntry::new(entry.into_path(), is_dir));
            WalkState::Continue
        })
    });
    drop(tx);

    let mut found: Vec<FileEntry> = rx.iter().collect();
    found.shrink_to_fit();
    if count.load(atomic::Ordering::Relaxed) >= MAX_ENTRIES {
        warn!("file_search: entry cap {MAX_ENTRIES} reached; index is partial");
    }
    found
}

fn dir_paths(entries: &[FileEntry]) -> impl Iterator<Item = &Path> {
    entries
        .iter()
        .filter(|entry| entry.is_dir)
        .map(|entry| entry.path.as_path())
}

// FSEvents and ReadDirectoryChangesW are recursive natively
#[cfg(any(target_os = "macos", windows))]
fn watch_initial(root: &Path, _entries: &[FileEntry], watcher: &Mutex<FsWatcher>) {
    let watched = watcher
        .lock()
        .unwrap()
        .watch(root, RecursiveMode::Recursive)
        .is_ok();
    if !watched {
        warn!("file_search: failed to watch {root:?}; results may go stale");
    }
}

#[cfg(target_os = "linux")]
fn watch_initial(root: &Path, entries: &[FileEntry], watcher: &Mutex<FsWatcher>) {
    watch_dirs(std::iter::once(root).chain(dir_paths(entries)), watcher);
}

/// The recursive root watch already covers directories created later.
#[cfg(any(target_os = "macos", windows))]
fn watch_dirs<'a>(_dirs: impl Iterator<Item = &'a Path>, _watcher: &Mutex<FsWatcher>) {}

#[cfg(target_os = "linux")]
fn watch_dirs<'a>(dirs: impl Iterator<Item = &'a Path>, watcher: &Mutex<FsWatcher>) {
    let mut guard = watcher.lock().unwrap();
    let failures = dirs
        .filter(|dir| guard.watch(dir, RecursiveMode::NonRecursive).is_err())
        .count();
    if failures > 0 {
        warn!("file_search: failed to watch {failures} directories; results there may go stale");
    }
}

#[cfg(any(target_os = "macos", windows))]
fn unwatch_dirs(_dirs: &[PathBuf], _watcher: &Mutex<FsWatcher>) {}

#[cfg(target_os = "linux")]
fn unwatch_dirs(dirs: &[PathBuf], watcher: &Mutex<FsWatcher>) {
    let mut guard = watcher.lock().unwrap();
    for dir in dirs {
        let _ = guard.unwatch(dir);
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
    let idx = bytes
        .iter()
        .rposition(|&b| std::path::is_separator(b as char))?;
    // keep the separator when the parent is a filesystem root ("/", "D:\")
    let end = if idx == 0 || (idx == 2 && bytes[1] == b':') {
        idx + 1
    } else {
        idx
    };
    Some(&bytes[..end])
}

/// `bytes` is `prefix` itself or a path below it.
fn in_subtree(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.starts_with(prefix)
        && (bytes.len() == prefix.len()
            || prefix
                .last()
                .is_some_and(|&b| std::path::is_separator(b as char))
            || std::path::is_separator(bytes[prefix.len()] as char))
}

/// Re-list changed directories and diff them against the index. The whole
/// batch costs one pass over the index, regardless of how many directories
/// are dirty.
fn rescan_dirs(
    dirty: &[PathBuf],
    roots: &[PathBuf],
    entries: &RwLock<Vec<FileEntry>>,
    watcher: &Mutex<FsWatcher>,
) {
    if dirty.is_empty() {
        return;
    }

    // Single read pass: which dirty dirs are indexed, and their known
    // children. Only indexed directories are rescanned; events can name
    // ancestors of a root (e.g. an event on the root itself), and rescanning
    // those would re-index the whole tree as "new".
    let mut states: HashMap<&[u8], DirState> = dirty
        .iter()
        .map(|dir| {
            let state = DirState {
                indexed: roots.iter().any(|root| dir.as_path() == root),
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
            current.push(FileEntry::new(entry.into_path(), is_dir));
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
            let found = scan(&entry.path, &AtomicUsize::new(0));
            watch_dirs(
                std::iter::once(entry.path.as_path()).chain(dir_paths(&found)),
                watcher,
            );
            entries.write().unwrap().extend(found);
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

    unwatch_dirs(&unwatch, watcher);
}

fn rank<'a>(input: &str, entries: &'a [FileEntry]) -> Vec<&'a FileEntry> {
    let input = input.trim();
    if input.chars().count() < MIN_QUERY_CHARS {
        return Vec::new();
    }

    // A single word matches fuzzily (subsequence, typo-friendly). Multi-word
    // input requires every word as a contiguous substring: fuzzy atoms
    // degenerate at corpus scale — scattered word-boundary hits let almost
    // any sentence match some long file name.
    let pattern = if input.contains(char::is_whitespace) {
        Pattern::new(
            input,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Substring,
        )
    } else {
        Pattern::parse(input, CaseMatching::Smart, Normalization::Smart)
    };

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
        let name = entry.name();
        let haystack = Utf32Str::new(&name, &mut buf);
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

fn build_item(roots: &[PathBuf], entry: &FileEntry) -> Item {
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
        title: entry.name().into_owned(),
        subtitle: Some(display_parent(roots, &entry.path)),
        icon: Some(entry_icon(entry)),
        actions,
    }
}

fn tool_results(query: &str, entries: &[FileEntry]) -> Result<ToolContent, String> {
    let query = query.trim();
    if query.chars().count() < MIN_QUERY_CHARS {
        return Err(format!(
            "query must be at least {MIN_QUERY_CHARS} characters"
        ));
    }

    let ranked = rank(query, entries);
    let mut content = ToolContent::new("file_search_results")
        .attr("query", query)
        .attr("count", ranked.len());
    for entry in ranked {
        let tag = if entry.is_dir { "dir" } else { "file" };
        content = content.child(ToolContent::new(tag).attr("path", entry.path.display()));
    }
    Ok(content)
}

#[cfg(windows)]
fn display_parent(_roots: &[PathBuf], path: &Path) -> String {
    path.parent().unwrap_or(path).display().to_string()
}

/// Parent directory for display, with home abbreviated to `~`.
#[cfg(not(windows))]
fn display_parent(roots: &[PathBuf], path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    match roots.first().map(|home| parent.strip_prefix(home)) {
        Some(Ok(rel)) if rel.as_os_str().is_empty() => "~".to_string(),
        Some(Ok(rel)) => format!("~/{}", rel.display()),
        _ => parent.display().to_string(),
    }
}

/// Windows renders the entry's real icon from its path.
#[cfg(windows)]
fn entry_icon(entry: &FileEntry) -> CapabilityIcon {
    CapabilityIcon::path(entry.path.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
fn entry_icon(entry: &FileEntry) -> CapabilityIcon {
    CapabilityIcon::name(icon_name(entry))
}

/// Freedesktop icon name for the entry's mime type, e.g. `text-plain`.
#[cfg(not(windows))]
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

    fn entry(path: &str, is_dir: bool) -> FileEntry {
        FileEntry::new(PathBuf::from(path), is_dir)
    }

    fn test_watcher() -> Mutex<FsWatcher> {
        Mutex::new(
            new_debouncer_opt(
                DEBOUNCE,
                None,
                |_: DebounceEventResult| {},
                NoCache,
                notify::Config::default(),
            )
            .unwrap(),
        )
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

        let found = scan(root.path(), &AtomicUsize::new(0));

        let names: Vec<String> = found.iter().map(|e| e.name().into_owned()).collect();
        assert!(names.iter().any(|n| n == "docs"));
        assert!(names.iter().any(|n| n == "report.txt"));
        let dirs: Vec<&Path> = dir_paths(&found).collect();
        assert!(dirs.contains(&root.path().join("docs").as_path()));
    }

    #[test]
    fn scan_skips_hidden_files() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join(".hidden_zzq"));

        let found = scan(root.path(), &AtomicUsize::new(0));

        assert!(found.is_empty());
    }

    #[test]
    fn scan_skips_excluded_directories() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        touch(&root.path().join("node_modules/package.json"));

        let found = scan(root.path(), &AtomicUsize::new(0));

        assert!(found.is_empty());
    }

    #[test]
    fn scan_honors_a_shared_cap_spent_by_earlier_roots() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("a.txt"));

        let count = AtomicUsize::new(MAX_ENTRIES);

        assert!(scan(root.path(), &count).is_empty());
    }

    #[test]
    fn rank_matches_fuzzily() {
        let entries = vec![
            entry("/home/u/proj/Cargo.toml", false),
            entry("/home/u/photo.png", false),
        ];

        let ranked = rank("cargtom", &entries);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].name(), "Cargo.toml");
    }

    #[test]
    fn rank_prefers_shallower_paths_on_ties() {
        let entries = vec![
            entry("/home/u/a/b/c/notes.md", false),
            entry("/home/u/notes.md", false),
        ];

        let ranked = rank("notes", &entries);

        assert_eq!(ranked[0].path, PathBuf::from("/home/u/notes.md"));
    }

    #[test]
    fn multi_word_queries_require_every_word_as_substring() {
        let entries = vec![
            entry("/home/u/annual_report_2026.pdf", false),
            entry("/home/u/annual_summary_export.pdf", false),
        ];

        let ranked = rank("annual report", &entries);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].name(), "annual_report_2026.pdf");
    }

    #[test]
    fn sentences_do_not_match_scattered_names() {
        let entries = vec![entry(
            "/home/u/homework_todo_maker_playlist_agenda.txt",
            false,
        )];

        assert!(rank("how to make pasta", &entries).is_empty());
    }

    #[test]
    fn rank_rejects_short_queries() {
        let entries = vec![entry("/home/u/a.txt", false)];

        assert!(rank("a", &entries).is_empty());
    }

    #[test]
    fn rank_caps_results() {
        let entries: Vec<FileEntry> = (0..100)
            .map(|i| entry(&format!("/home/u/{i}/match.txt"), false))
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
            &[root.path().to_path_buf()],
            &entries,
            &watcher,
        );

        let guard = entries.read().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].name(), "new.txt");
    }

    #[test]
    fn rescan_drops_deleted_subtrees() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("gone");
        let entries = RwLock::new(vec![
            FileEntry::new(dir.clone(), true),
            FileEntry::new(dir.join("inner.txt"), false),
        ]);
        let watcher = test_watcher();

        rescan_dirs(
            &[root.path().to_path_buf()],
            &[root.path().to_path_buf()],
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
        let entries = RwLock::new(vec![FileEntry::new(target.clone(), false)]);
        let watcher = test_watcher();
        fs::create_dir(&target).unwrap();

        rescan_dirs(
            &[root.path().to_path_buf()],
            &[root.path().to_path_buf()],
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
            FileEntry::new(root.path().join("a"), true),
            FileEntry::new(root.path().join("b"), true),
        ]);
        let watcher = test_watcher();
        touch(&root.path().join("a/x.txt"));
        touch(&root.path().join("b/y.txt"));

        rescan_dirs(
            &[root.path().join("a"), root.path().join("b")],
            &[root.path().to_path_buf()],
            &entries,
            &watcher,
        );

        let guard = entries.read().unwrap();
        let names: Vec<String> = guard.iter().map(|e| e.name().into_owned()).collect();
        assert!(names.iter().any(|n| n == "x.txt"));
        assert!(names.iter().any(|n| n == "y.txt"));
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
            &[root.path().to_path_buf()],
            &entries,
            &watcher,
        );

        assert!(entries.read().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn parent_bytes_of_a_drive_root_child_matches_the_drive_root_key() {
        assert_eq!(
            parent_bytes(Path::new(r"D:\file.txt")),
            Some(path_bytes(Path::new(r"D:\")))
        );
    }

    #[cfg(windows)]
    #[test]
    fn in_subtree_accepts_a_drive_root_prefix() {
        assert!(in_subtree(
            path_bytes(Path::new(r"D:\sub")),
            path_bytes(Path::new(r"D:\"))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn roots_resolve_nonempty_and_disjoint() {
        let roots = roots().unwrap();
        assert!(!roots.is_empty());
        for (i, a) in roots.iter().enumerate() {
            for (j, b) in roots.iter().enumerate() {
                assert!(i == j || !a.starts_with(b), "{a:?} nested in {b:?}");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn dedupe_drops_roots_nested_in_earlier_ones() {
        let roots = vec![
            PathBuf::from("/home/u/Documents"),
            PathBuf::from("/home/u/Downloads"),
            PathBuf::from("/home/u"),
            PathBuf::from("/data/docs"),
            PathBuf::from("/data/images"),
        ];

        assert_eq!(
            dedupe_roots(roots),
            [
                PathBuf::from("/home/u"),
                PathBuf::from("/data/docs"),
                PathBuf::from("/data/images")
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn dedupe_keeps_prefix_siblings_apart() {
        let roots = vec![PathBuf::from("/home/u"), PathBuf::from("/home/u2")];

        assert_eq!(dedupe_roots(roots.clone()), roots);
    }

    #[test]
    fn tool_results_reject_short_queries() {
        let entries = vec![entry("/home/u/a.txt", false)];

        let error = tool_results("a", &entries).unwrap_err();

        assert!(error.contains("at least"), "got: {error}");
    }

    #[test]
    fn tool_results_render_matches_as_children() {
        let entries = vec![
            entry("/home/u/notes", true),
            entry("/home/u/notes.md", false),
            entry("/home/u/photo.png", false),
        ];

        let content = tool_results("notes", &entries).unwrap();

        assert_eq!(content.tag, "file_search_results");
        assert!(
            content
                .attributes
                .iter()
                .any(|attribute| attribute.key == "count" && attribute.value == "2"),
            "got: {content:?}"
        );
        let results: Vec<(&str, &str)> = content
            .children()
            .iter()
            .map(|child| (child.tag.as_str(), child.attributes[0].value.as_str()))
            .collect();
        assert!(results.contains(&("dir", "/home/u/notes")));
        assert!(results.contains(&("file", "/home/u/notes.md")));
    }

    #[test]
    fn files_offer_open_folder_action() {
        let item = build_item(
            &[PathBuf::from("/home/u")],
            &entry("/home/u/docs/a.txt", false),
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
        let item = build_item(&[PathBuf::from("/home/u")], &entry("/home/u/docs", true));

        let labels: Vec<_> = item.actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, [OPEN_ACTION_LABEL, COPY_PATH_ACTION_LABEL]);
    }

    #[cfg(not(windows))]
    #[test]
    fn display_parent_abbreviates_home() {
        let roots = [PathBuf::from("/home/u")];

        assert_eq!(
            display_parent(&roots, Path::new("/home/u/docs/report.txt")),
            "~/docs"
        );
        assert_eq!(display_parent(&roots, Path::new("/home/u/report.txt")), "~");
    }

    #[cfg(windows)]
    #[test]
    fn display_parent_shows_full_parent_path() {
        let roots = [PathBuf::from(r"C:\Users\u")];

        assert_eq!(
            display_parent(&roots, Path::new(r"C:\Users\u\docs\report.txt")),
            r"C:\Users\u\docs"
        );
        assert_eq!(
            display_parent(&roots, Path::new(r"D:\archive\report.pdf")),
            r"D:\archive"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn display_parent_falls_back_to_full_path_outside_roots() {
        let roots = [PathBuf::from("/home/u")];

        assert_eq!(
            display_parent(&roots, Path::new("/srv/shared/report.txt")),
            "/srv/shared"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn icon_name_maps_mime_types() {
        assert_eq!(icon_name(&entry("/home/u/a.png", false)), "image-png");
        assert_eq!(icon_name(&entry("/home/u/docs", true)), FOLDER_ICON);
        assert_eq!(icon_name(&entry("/home/u/noext", false)), FALLBACK_ICON);
    }

    #[cfg(windows)]
    #[test]
    fn entries_use_shell_path_icons() {
        use paloma_extension_protocol::v1::capability_icon::Icon;

        let item = build_item(
            &[PathBuf::from(r"C:\Users\u")],
            &entry(r"C:\Users\u\photo.png", false),
        );

        assert_eq!(
            item.icon.unwrap().icon,
            Some(Icon::Path(r"C:\Users\u\photo.png".into()))
        );
    }
}
