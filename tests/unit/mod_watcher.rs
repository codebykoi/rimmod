use std::{
    fs, io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use super::*;

#[test]
fn creating_a_file_reports_a_folder_change() -> io::Result<()> {
    let watched_folder =
        std::env::temp_dir().join(format!("rimmod-watcher-test-{}", std::process::id()));

    if watched_folder.exists() {
        fs::remove_dir_all(&watched_folder)?;
    }
    fs::create_dir(&watched_folder)?;

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
            ModWatcherPoll::Changed => {
                change_detected = true;
                break;
            }
            ModWatcherPoll::Error(error) => return Err(io::Error::other(error)),
            ModWatcherPoll::Idle => thread::sleep(Duration::from_millis(50)),
        }
    }

    drop(watcher);
    fs::remove_dir_all(&watched_folder)?;

    assert!(change_detected);
    assert!(was_woken.load(Ordering::Relaxed));

    Ok(())
}
