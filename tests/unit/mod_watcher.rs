use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use notify::event::{
    AccessKind, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode,
};

use super::*;

fn temp_test_dir(name: &str) -> io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rimmod-watcher-{name}-{}", std::process::id()));

    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;

    Ok(dir)
}

fn event(kind: EventKind, path: impl Into<PathBuf>) -> Event {
    Event::new(kind).add_path(path.into())
}

#[test]
fn creating_a_file_reports_a_folder_change() -> io::Result<()> {
    let watched_folder = temp_test_dir("create-file")?;

    let was_woken = Arc::new(AtomicBool::new(false));
    let wake_flag = Arc::clone(&was_woken);
    let watcher = ModWatcher::new([watched_folder.clone()], move || {
        wake_flag.store(true, Ordering::Relaxed);
    })
    .map_err(io::Error::other)?;

    fs::write(watched_folder.join("new-mod-file.xml"), "<xml />")?;

    let mut change_detected = false;
    for _ in 0..50 {
        match watcher.poll() {
            ModWatcherPoll::Changed(_) => {
                change_detected = true;
                break;
            }
            ModWatcherPoll::Error(error) => return Err(io::Error::other(error)),
            ModWatcherPoll::Idle | ModWatcherPoll::Activity(_) => {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    drop(watcher);
    fs::remove_dir_all(&watched_folder)?;

    assert!(change_detected);
    assert!(was_woken.load(Ordering::Relaxed));

    Ok(())
}

#[test]
fn writing_irrelevant_assets_reports_activity_without_waking_ui() -> io::Result<()> {
    let watched_folder = temp_test_dir("irrelevant-assets")?;
    let textures = watched_folder.join("123456").join("Textures");
    let languages = watched_folder
        .join("123456")
        .join("Languages")
        .join("English");
    fs::create_dir_all(&textures)?;
    fs::create_dir_all(&languages)?;

    let was_woken = Arc::new(AtomicBool::new(false));
    let wake_flag = Arc::clone(&was_woken);
    let watcher = ModWatcher::new([watched_folder.clone()], move || {
        wake_flag.store(true, Ordering::Relaxed);
    })
    .map_err(io::Error::other)?;

    // Drain any setup noise before writing assets.
    thread::sleep(Duration::from_millis(100));
    let _ = watcher.poll();
    was_woken.store(false, Ordering::Relaxed);

    fs::write(textures.join("icon.png"), b"png")?;
    fs::write(languages.join("Keys.xml"), "<LanguageData />")?;

    let mut saw_activity = false;
    let mut saw_change = false;
    for _ in 0..20 {
        match watcher.poll() {
            ModWatcherPoll::Changed(_) => {
                saw_change = true;
                break;
            }
            ModWatcherPoll::Activity(_) => {
                saw_activity = true;
            }
            ModWatcherPoll::Error(error) => return Err(io::Error::other(error)),
            ModWatcherPoll::Idle => thread::sleep(Duration::from_millis(25)),
        }
    }

    drop(watcher);
    fs::remove_dir_all(&watched_folder)?;

    assert!(!saw_change);
    assert!(saw_activity);
    assert!(!was_woken.load(Ordering::Relaxed));

    Ok(())
}

#[test]
fn classify_event_ignores_access_metadata_and_other_events() {
    let root = PathBuf::from("workshop");
    let about = root.join("123456").join("About").join("About.xml");
    let roots = [root];

    assert_eq!(
        classify_event(&event(EventKind::Access(AccessKind::Any), &about), &roots),
        EventRelevance::Ignore
    );
    assert_eq!(
        classify_event(
            &event(
                EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
                &about
            ),
            &roots
        ),
        EventRelevance::Ignore
    );
    assert_eq!(
        classify_event(&event(EventKind::Other, &about), &roots),
        EventRelevance::Ignore
    );
}

#[test]
fn classify_event_distinguishes_mod_files_from_assets_and_staging_folders() -> io::Result<()> {
    let root = temp_test_dir("classify")?;
    let empty_mod = root.join("111111");
    fs::create_dir_all(&empty_mod)?;

    let complete_mod = root.join("222222");
    fs::create_dir_all(complete_mod.join("About"))?;
    fs::write(
        complete_mod.join("About").join("About.xml"),
        "<ModMetaData />",
    )?;

    let roots = vec![root.clone()];

    // Empty top-level mod folder created by Steam before copying files is only activity.
    assert_eq!(
        classify_event(
            &event(EventKind::Create(CreateKind::Folder), &empty_mod),
            &roots
        ),
        EventRelevance::Activity
    );
    // Complete mod folder moved in atomically with About.xml is a real change.
    assert_eq!(
        classify_event(
            &event(EventKind::Create(CreateKind::Folder), &complete_mod),
            &roots
        ),
        EventRelevance::Changed
    );
    // Windows directory Modify(Any) on the mod folder itself is only activity.
    assert_eq!(
        classify_event(
            &event(EventKind::Modify(ModifyKind::Any), &complete_mod),
            &roots
        ),
        EventRelevance::Activity
    );
    // Deleting a mod folder is always a real change.
    assert_eq!(
        classify_event(
            &event(EventKind::Remove(RemoveKind::Folder), root.join("333333")),
            &roots
        ),
        EventRelevance::Changed
    );

    // Mod metadata, defs, patches, assemblies, and No Version Warning files are changes.
    for relevant in [
        complete_mod.join("About").join("About.xml"),
        complete_mod.join("LoadFolders.xml"),
        complete_mod.join("1.6").join("Defs").join("Thing.xml"),
        complete_mod.join("Patches").join("Patch.xml"),
        complete_mod.join("1.6").join("Assemblies").join("Mod.dll"),
        complete_mod.join("1.6").join("ModIdsToFix.xml"),
        complete_mod.join(".git").join("config"),
    ] {
        assert_eq!(
            classify_event(
                &event(
                    EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                    &relevant
                ),
                &roots
            ),
            EventRelevance::Changed,
            "expected {} to be a relevant change",
            relevant.display()
        );
    }

    // Assets, translations, previews, PublishedFileId.txt, and .git internals are only activity.
    for noisy in [
        complete_mod.join("About").join("Preview.png"),
        complete_mod.join("About").join("PublishedFileId.txt"),
        complete_mod
            .join("Textures")
            .join("Things")
            .join("Pawn.png"),
        complete_mod.join("Sounds").join("Click.ogg"),
        complete_mod
            .join("Languages")
            .join("English")
            .join("Keyed")
            .join("Keys.xml"),
        complete_mod.join("Source").join("Mod.cs"),
        complete_mod.join(".git").join("index"),
        complete_mod.join("download.tmp"),
    ] {
        assert_eq!(
            classify_event(&event(EventKind::Create(CreateKind::File), &noisy), &roots),
            EventRelevance::Activity,
            "expected {} to be activity only",
            noisy.display()
        );
    }

    // Removing a version subfolder like `1.6` (which has a dot in its name) is still a change.
    assert_eq!(
        classify_event(
            &event(EventKind::Remove(RemoveKind::Any), complete_mod.join("1.6")),
            &roots
        ),
        EventRelevance::Changed
    );
    assert_eq!(
        classify_event(
            &event(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                complete_mod.join("Defs")
            ),
            &roots
        ),
        EventRelevance::Changed
    );
    // Removing an ignored preview/temp file is only activity.
    assert_eq!(
        classify_event(
            &event(
                EventKind::Remove(RemoveKind::File),
                complete_mod.join("About").join("Preview.png")
            ),
            &roots
        ),
        EventRelevance::Activity
    );

    fs::remove_dir_all(&root)?;
    assert!(!Path::new(&root).exists());

    Ok(())
}
