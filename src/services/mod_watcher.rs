use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};

use crate::models::parse_version_tag;

const IGNORED_SUBFOLDERS: &[&str] = &[
    "textures",
    "sounds",
    "languages",
    "source",
    "news",
    "screenshots",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EventRelevance {
    Ignore,
    Activity,
    Changed,
}

#[derive(Default)]
struct ModWatcherEvents {
    last_change: Option<Instant>,
    last_activity: Option<Instant>,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModWatcherPoll {
    Idle,
    /// Non-metadata filesystem activity inside watched folders (such as Steam
    /// writing textures, sounds, translations, or empty staging directories).
    /// Never starts a reload on its own, but extends an already-pending
    /// debounce window so reloads wait for downloads to settle.
    Activity(Instant),
    /// A change that can affect the loaded mod list or inferred dependencies.
    Changed(Instant),
    Error(String),
}

pub(crate) struct ModWatcher {
    // The watcher stops when it is dropped, so App must keep owning it even
    // though events arrive through the shared state.
    _watcher: RecommendedWatcher,
    events: Arc<Mutex<ModWatcherEvents>>,
}

impl ModWatcher {
    pub(crate) fn new(
        paths: impl IntoIterator<Item = PathBuf>,
        wake_ui: impl Fn() + Send + 'static,
    ) -> notify::Result<Self> {
        let mut watch_paths = Vec::new();
        let mut match_roots = Vec::new();

        for path in paths {
            let canonical = path.canonicalize().ok();
            let watched = canonical.clone().unwrap_or_else(|| path.clone());
            watch_paths.push(watched);
            if let Some(canonical) = canonical {
                match_roots.push(canonical);
            }
            match_roots.push(path);
        }

        watch_paths.sort();
        watch_paths.dedup();
        match_roots.sort();
        match_roots.dedup();

        let events = Arc::new(Mutex::new(ModWatcherEvents::default()));
        let callback_events = Arc::clone(&events);

        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            let now = Instant::now();

            match result {
                Ok(event) => match classify_event(&event, &match_roots) {
                    EventRelevance::Ignore => {}
                    EventRelevance::Activity => {
                        if let Ok(mut pending) = callback_events.lock() {
                            pending.last_activity = Some(now);
                        }
                    }
                    EventRelevance::Changed => {
                        if let Ok(mut pending) = callback_events.lock() {
                            pending.last_change = Some(now);
                        }
                        wake_ui();
                    }
                },
                Err(error) => {
                    if let Ok(mut pending) = callback_events.lock() {
                        pending.error = Some(error.to_string());
                    }
                    wake_ui();
                }
            }
        })?;

        for path in watch_paths {
            watcher.watch(&path, RecursiveMode::Recursive)?;
        }

        Ok(Self {
            _watcher: watcher,
            events,
        })
    }

    pub(crate) fn poll(&self) -> ModWatcherPoll {
        let taken = match self.events.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };

        if let Some(error) = taken.error {
            ModWatcherPoll::Error(error)
        } else if let Some(changed_at) = taken.last_change {
            let latest = taken
                .last_activity
                .map_or(changed_at, |activity_at| activity_at.max(changed_at));
            ModWatcherPoll::Changed(latest)
        } else if let Some(activity_at) = taken.last_activity {
            ModWatcherPoll::Activity(activity_at)
        } else {
            ModWatcherPoll::Idle
        }
    }
}

fn classify_event(event: &Event, watched_roots: &[PathBuf]) -> EventRelevance {
    // Reading files or touching metadata/timestamps alone should not trigger a
    // mod reload.
    if matches!(
        event.kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) | EventKind::Other
    ) {
        return EventRelevance::Ignore;
    }

    if event.paths.is_empty() {
        return EventRelevance::Changed;
    }

    event
        .paths
        .iter()
        .map(|path| classify_path(path, event.kind, watched_roots))
        .max()
        .unwrap_or(EventRelevance::Ignore)
}

fn classify_path(path: &Path, kind: EventKind, watched_roots: &[PathBuf]) -> EventRelevance {
    let Some(components) = relative_normal_components(path, watched_roots) else {
        return EventRelevance::Changed;
    };

    match components.as_slice() {
        // The watched root itself.
        [] => match kind {
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)) => {
                EventRelevance::Changed
            }
            _ => EventRelevance::Activity,
        },

        // Direct child of a watched root (typically a mod folder).
        [_entry] => match kind {
            EventKind::Remove(_) => EventRelevance::Changed,
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                if path.is_dir() {
                    if has_about_xml(path) {
                        EventRelevance::Changed
                    } else {
                        EventRelevance::Activity
                    }
                } else if has_relevant_extension(path) {
                    EventRelevance::Changed
                } else {
                    EventRelevance::Activity
                }
            }
            EventKind::Modify(ModifyKind::Name(_)) => EventRelevance::Changed,
            EventKind::Create(_) => {
                if path.is_dir() {
                    if has_about_xml(path) {
                        EventRelevance::Changed
                    } else {
                        // Steam creates an empty Workshop directory before
                        // copying files into it; wait for About.xml or other
                        // mod files instead of reloading prematurely.
                        EventRelevance::Activity
                    }
                } else if has_relevant_extension(path) {
                    EventRelevance::Changed
                } else {
                    EventRelevance::Activity
                }
            }
            _ => {
                if path.is_dir() {
                    // On Windows, ReadDirectoryChangesW emits Modify(Any) on
                    // the parent mod folder whenever children change or folder
                    // timestamps update; relevant child files emit their own
                    // events.
                    EventRelevance::Activity
                } else if has_relevant_extension(path) {
                    EventRelevance::Changed
                } else {
                    EventRelevance::Activity
                }
            }
        },

        // Inside a mod folder: `<root>/<mod_folder>/<sub...>`.
        [_mod_folder, sub @ ..] => classify_mod_subpath(path, sub, kind),
    }
}

fn classify_mod_subpath(path: &Path, sub: &[&OsStr], kind: EventKind) -> EventRelevance {
    if let Some(first) = sub.first()
        && first.eq_ignore_ascii_case(".git")
    {
        return match sub.len() {
            1 if matches!(
                kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_))
            ) =>
            {
                EventRelevance::Changed
            }
            2 if sub[1].eq_ignore_ascii_case("config") => EventRelevance::Changed,
            _ => EventRelevance::Activity,
        };
    }

    if sub.iter().any(|component| is_ignored_subfolder(component)) {
        return EventRelevance::Activity;
    }

    if path.is_dir() {
        return match kind {
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)) => {
                EventRelevance::Changed
            }
            EventKind::Create(_) => {
                if dir_is_non_empty(path) {
                    EventRelevance::Changed
                } else {
                    EventRelevance::Activity
                }
            }
            _ => EventRelevance::Activity,
        };
    }

    let Some(file_name) = sub.last().copied() else {
        return EventRelevance::Activity;
    };

    if is_relevant_file_name(file_name) || has_relevant_extension(path) {
        return EventRelevance::Changed;
    }

    // A removed or renamed subdirectory (e.g. `About`, `Defs`, `Assemblies`, or
    // a version folder like `1.6`) is no longer a directory on disk.
    if matches!(
        kind,
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    ) && (path.extension().is_none() || file_name.to_str().and_then(parse_version_tag).is_some())
    {
        return EventRelevance::Changed;
    }

    EventRelevance::Activity
}

fn is_ignored_subfolder(component: &OsStr) -> bool {
    IGNORED_SUBFOLDERS
        .iter()
        .any(|ignored| component.eq_ignore_ascii_case(ignored))
}

fn is_relevant_file_name(file_name: &OsStr) -> bool {
    file_name.eq_ignore_ascii_case("About.xml")
        || file_name.eq_ignore_ascii_case("LoadFolders.xml")
        || file_name.eq_ignore_ascii_case("ModIdsToFix.xml")
}

fn has_relevant_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("xml") || extension.eq_ignore_ascii_case("dll")
    })
}

fn has_about_xml(folder: &Path) -> bool {
    folder.join("About").join("About.xml").is_file()
}

fn dir_is_non_empty(folder: &Path) -> bool {
    fs::read_dir(folder)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

fn strip_windows_verbatim(path: &Path) -> &Path {
    #[cfg(windows)]
    {
        if let Some(raw) = path.to_str()
            && let Some(stripped) = raw.strip_prefix(r"\\?\")
        {
            return Path::new(stripped);
        }
    }

    path
}

fn relative_normal_components<'a>(path: &'a Path, roots: &[PathBuf]) -> Option<Vec<&'a OsStr>> {
    let normalized_path = strip_windows_verbatim(path);

    for root in roots {
        let normalized_root = strip_windows_verbatim(root);
        if let Some(relative) = strip_prefix_platform(normalized_path, normalized_root) {
            return Some(relative);
        }
    }

    None
}

fn strip_prefix_platform<'a>(path: &'a Path, root: &Path) -> Option<Vec<&'a OsStr>> {
    let mut path_components = path.components();

    for root_component in root.components() {
        let path_component = path_components.next()?;
        if !components_equal(path_component, root_component) {
            return None;
        }
    }

    Some(
        path_components
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part),
                _ => None,
            })
            .collect(),
    )
}

fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str().eq_ignore_ascii_case(right.as_os_str())
    }

    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mod_watcher.rs"]
mod tests;
