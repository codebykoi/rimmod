use std::{path::PathBuf, sync::mpsc};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

enum ModWatcherMessage {
    Changed,
    Error(String),
}

pub(crate) enum ModWatcherPoll {
    Idle,
    Changed,
    Error(String),
}

pub(crate) struct ModWatcher {
    // The watcher stops when it is dropped, so App must keep owning it even
    // though events arrive through the separate channel.
    _watcher: RecommendedWatcher,
    messages: mpsc::Receiver<ModWatcherMessage>,
}

impl ModWatcher {
    pub(crate) fn new(
        paths: impl IntoIterator<Item = PathBuf>,
        wake_ui: impl Fn() + Send + 'static,
    ) -> notify::Result<Self> {
        let (sender, messages) = mpsc::channel();

        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                let message = match result {
                    // Reading metadata should not cause another mod reload.
                    Ok(event) if matches!(event.kind, EventKind::Access(_)) => return,
                    Ok(_) => ModWatcherMessage::Changed,
                    Err(error) => ModWatcherMessage::Error(error.to_string()),
                };

                if sender.send(message).is_ok() {
                    wake_ui();
                }
            })?;

        for path in paths {
            watcher.watch(&path, RecursiveMode::Recursive)?;
        }

        Ok(Self {
            _watcher: watcher,
            messages,
        })
    }

    pub(crate) fn poll(&self) -> ModWatcherPoll {
        let mut changed = false;
        let mut error = None;

        for message in self.messages.try_iter() {
            match message {
                ModWatcherMessage::Changed => changed = true,
                ModWatcherMessage::Error(message) => error = Some(message),
            }
        }

        match (error, changed) {
            (Some(error), _) => ModWatcherPoll::Error(error),
            (None, true) => ModWatcherPoll::Changed,
            (None, false) => ModWatcherPoll::Idle,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mod_watcher.rs"]
mod tests;
