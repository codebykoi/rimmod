use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::models::RimworldMod;
use crate::services::mod_loader::{self, ModLoadReport, ModLoadWarning};
use crate::services::mod_watcher::{ModWatcher, ModWatcherPoll};
use crate::services::workshop;

use super::{App, LoadOrderStatus};

const MOD_RELOAD_DEBOUNCE: Duration = Duration::from_secs(2);
const MOD_RELOAD_MAX_WAIT: Duration = Duration::from_secs(10);

struct WatcherReloadOutcome {
    generation: u64,
    result: io::Result<ModLoadReport>,
}

#[derive(Default)]
pub(super) struct ModWatcherState {
    watcher: Option<ModWatcher>,
    pending_reload_at: Option<Instant>,
    first_pending_change_at: Option<Instant>,
    reload_receiver: Option<Receiver<WatcherReloadOutcome>>,
    reload_generation: u64,
    dirty_during_reload: Option<Instant>,
}

impl ModWatcherState {
    fn clear_pending(&mut self) {
        self.pending_reload_at = None;
        self.first_pending_change_at = None;
        self.dirty_during_reload = None;
    }

    fn invalidate_reload(&mut self) {
        self.reload_generation = self.reload_generation.wrapping_add(1);
        self.reload_receiver = None;
        self.clear_pending();
    }

    fn record_change(&mut self, at: Instant) {
        if self.reload_receiver.is_some() {
            self.dirty_during_reload = Some(
                self.dirty_during_reload
                    .map_or(at, |existing| existing.max(at)),
            );
            return;
        }

        let first = *self.first_pending_change_at.get_or_insert(at);
        let max_deadline = first + MOD_RELOAD_MAX_WAIT;
        let candidate = (at + MOD_RELOAD_DEBOUNCE).min(max_deadline);

        self.pending_reload_at = Some(self.pending_reload_at.map_or(candidate, |existing| {
            existing.max(candidate).min(max_deadline)
        }));
    }

    fn record_activity(&mut self, at: Instant) {
        if self.reload_receiver.is_some() {
            if let Some(existing) = self.dirty_during_reload.as_mut() {
                *existing = (*existing).max(at);
            }
            return;
        }

        let Some(existing) = self.pending_reload_at else {
            return;
        };

        let first = self.first_pending_change_at.unwrap_or(at);
        let max_deadline = first + MOD_RELOAD_MAX_WAIT;
        let candidate = (at + MOD_RELOAD_DEBOUNCE).min(max_deadline);

        self.pending_reload_at = Some(existing.max(candidate).min(max_deadline));
    }

    fn defer_reload(&mut self, now: Instant) {
        self.pending_reload_at = Some(now + MOD_RELOAD_DEBOUNCE);
        self.first_pending_change_at = Some(now);
    }
}

fn mods_are_equivalent(current: &[RimworldMod], loaded: &[RimworldMod]) -> bool {
    if current.len() != loaded.len() {
        return false;
    }

    let mut current_sorted: Vec<&RimworldMod> = current.iter().collect();
    let mut loaded_sorted: Vec<&RimworldMod> = loaded.iter().collect();
    let compare = |left: &&RimworldMod, right: &&RimworldMod| {
        (&left.folder, left.package_id.as_str()).cmp(&(&right.folder, right.package_id.as_str()))
    };
    current_sorted.sort_by(compare);
    loaded_sorted.sort_by(compare);

    current_sorted == loaded_sorted
}

fn warnings_are_equivalent(current: &[ModLoadWarning], loaded: &[ModLoadWarning]) -> bool {
    if current.len() != loaded.len() {
        return false;
    }

    if current == loaded {
        return true;
    }

    let mut current_sorted: Vec<String> = current.iter().map(ToString::to_string).collect();
    let mut loaded_sorted: Vec<String> = loaded.iter().map(ToString::to_string).collect();
    current_sorted.sort();
    loaded_sorted.sort();

    current_sorted == loaded_sorted
}

impl App {
    pub(super) fn cancel_pending_watcher_reload(&mut self) {
        self.mod_watcher.invalidate_reload();
        if let Some(watcher) = self.mod_watcher.watcher.as_ref() {
            let _ = watcher.poll();
        }
    }

    pub(super) fn restart_mod_watcher(&mut self, context: &egui::Context) {
        self.mod_watcher.watcher = None;
        self.mod_watcher.invalidate_reload();

        let mut paths = [
            self.settings.official_path(),
            self.settings.local_path(),
            self.settings.workshop_path().map(Path::to_path_buf),
            workshop::steamcmd_workshop_path(self.settings.steamcmd_path()),
        ]
        .into_iter()
        .flatten()
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

        paths.sort();
        paths.dedup();

        if paths.is_empty() {
            return;
        }

        let repaint_context = context.clone();
        match ModWatcher::new(paths, move || repaint_context.request_repaint()) {
            Ok(watcher) => self.mod_watcher.watcher = Some(watcher),
            Err(error) => {
                self.report_action_error(format!("Could not watch mod folders: {error}"));
            }
        }
    }

    fn start_watcher_mod_reload(&mut self, context: &egui::Context) {
        self.mod_watcher.reload_generation = self.mod_watcher.reload_generation.wrapping_add(1);
        let generation = self.mod_watcher.reload_generation;
        let settings = self.settings.clone();
        let game_version = self.game_version.clone();
        let repaint_context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.mod_watcher.reload_receiver = Some(receiver);

        std::thread::spawn(move || {
            let result = mod_loader::load_mods(&settings, game_version.as_deref());
            let _ = sender.send(WatcherReloadOutcome { generation, result });
            repaint_context.request_repaint();
        });
    }

    fn apply_watcher_reload_result(&mut self, result: io::Result<ModLoadReport>) -> bool {
        match result {
            Ok(mut report) => {
                let mods_changed = !mods_are_equivalent(&self.mods.all, &report.mods.all);
                let warnings_changed =
                    !warnings_are_equivalent(&self.mod_load_warnings, &report.warnings);
                let recovering_from_unavailable =
                    matches!(self.load_order_status, LoadOrderStatus::Unavailable { .. });

                if !mods_changed && !recovering_from_unavailable {
                    if warnings_changed {
                        self.mod_load_warnings = report.warnings;
                    }
                    return false;
                }

                let active_package_ids = self.mods.active_package_ids();
                let selected_package_ids = self
                    .state
                    .selected_mod_ids
                    .iter()
                    .filter_map(|&mod_id| self.mods.get(mod_id))
                    .map(|rimworld_mod| rimworld_mod.package_id.clone())
                    .collect::<Vec<_>>();
                let selected_package_id = self
                    .state
                    .selected_mod_id
                    .and_then(|mod_id| self.mods.get(mod_id))
                    .map(|rimworld_mod| rimworld_mod.package_id.clone());
                let selection_anchor_package_id = self
                    .state
                    .selection_anchor_id
                    .and_then(|mod_id| self.mods.get(mod_id))
                    .map(|rimworld_mod| rimworld_mod.package_id.clone());

                report.mods.replace_active_package_ids(active_package_ids);

                self.mods = report.mods;
                self.mod_load_warnings = report.warnings;
                self.load_order_status = LoadOrderStatus::Ready;
                self.clear_load_order_history();
                self.logs.invalidate();
                self.state.selected_mod_ids = selected_package_ids
                    .iter()
                    .filter_map(|package_id| self.mods.find_id_by_package_id(package_id))
                    .collect();
                self.state.selected_mod_id = selected_package_id
                    .as_ref()
                    .and_then(|package_id| self.mods.find_id_by_package_id(package_id))
                    .or_else(|| self.state.selected_mod_ids.last().copied());

                if self.state.selected_mod_ids.is_empty() {
                    self.clear_mod_selection();
                } else {
                    self.state.selection_anchor_id = selection_anchor_package_id
                        .as_ref()
                        .and_then(|package_id| self.mods.find_id_by_package_id(package_id))
                        .filter(|mod_id| self.state.selected_mod_ids.contains(mod_id));
                }

                true
            }
            Err(error) => {
                self.mod_load_warnings.clear();
                self.load_order_status = LoadOrderStatus::Unavailable {
                    reason: error.to_string(),
                };

                false
            }
        }
    }

    pub(super) fn process_mod_watcher(&mut self, context: &egui::Context) {
        let reload_outcome = self
            .mod_watcher
            .reload_receiver
            .as_ref()
            .map(Receiver::try_recv);

        match reload_outcome {
            Some(Ok(outcome)) => {
                self.mod_watcher.reload_receiver = None;

                if outcome.generation == self.mod_watcher.reload_generation
                    && self.apply_watcher_reload_result(outcome.result)
                {
                    self.toasts.info("Mods reloaded after folder changes");
                }

                if let Some(dirty_at) = self.mod_watcher.dirty_during_reload.take() {
                    self.mod_watcher.record_change(dirty_at);
                }
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.mod_watcher.reload_receiver = None;
                if let Some(dirty_at) = self.mod_watcher.dirty_during_reload.take() {
                    self.mod_watcher.record_change(dirty_at);
                }
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let poll = self
            .mod_watcher
            .watcher
            .as_ref()
            .map_or(ModWatcherPoll::Idle, ModWatcher::poll);

        match poll {
            ModWatcherPoll::Idle => {}
            ModWatcherPoll::Activity(at) => {
                self.mod_watcher.record_activity(at);
            }
            ModWatcherPoll::Changed(at) => {
                self.mod_watcher.record_change(at);
            }
            ModWatcherPoll::Error(error) => {
                self.report_action_error(format!("Could not watch mod folders: {error}"));
            }
        }

        if self.mod_watcher.reload_receiver.is_some() {
            return;
        }

        let Some(reload_at) = self.mod_watcher.pending_reload_at else {
            return;
        };
        let now = Instant::now();

        if now < reload_at {
            context.request_repaint_after(reload_at - now);
            return;
        }

        if !self.workshop.installing_items.is_empty() || self.pack_busy_label().is_some() {
            self.mod_watcher.defer_reload(now);
            context.request_repaint_after(MOD_RELOAD_DEBOUNCE);
            return;
        }

        self.mod_watcher.clear_pending();
        self.start_watcher_mod_reload(context);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::models::{ModSource, ModType, PackageId};

    use super::*;

    fn sample_mod(package_id: &str, folder: &str) -> RimworldMod {
        RimworldMod {
            name: package_id.to_owned(),
            package_id: PackageId::new(package_id).expect("valid package id"),
            description: String::new(),
            supported_versions: vec!["1.6".to_owned()],
            community_supported_versions: Vec::new(),
            loader_after: Vec::new(),
            loader_before: Vec::new(),
            force_load_after: Vec::new(),
            force_load_before: Vec::new(),
            mod_dependencies: Vec::new(),
            incompatible_with: Vec::new(),
            inferred_after: Vec::new(),
            is_patch_only: false,
            folder: PathBuf::from(folder),
            source: ModSource::SteamWorkshop { workshop_id: 123 },
            mod_type: ModType::Xml,
        }
    }

    #[test]
    fn activity_does_not_schedule_reload_when_idle_but_extends_pending_debounce() {
        let start = Instant::now();
        let mut state = ModWatcherState::default();

        state.record_activity(start);
        assert!(state.pending_reload_at.is_none());

        let changed_at = start + Duration::from_secs(1);
        state.record_change(changed_at);
        assert_eq!(
            state.pending_reload_at,
            Some(changed_at + MOD_RELOAD_DEBOUNCE)
        );

        let activity_at = changed_at + Duration::from_secs(1);
        state.record_activity(activity_at);
        assert_eq!(
            state.pending_reload_at,
            Some(activity_at + MOD_RELOAD_DEBOUNCE)
        );
    }

    #[test]
    fn repeated_events_are_capped_by_max_wait() {
        let start = Instant::now();
        let mut state = ModWatcherState::default();

        state.record_change(start);
        state.record_activity(start + Duration::from_secs(9));
        state.record_change(start + Duration::from_secs(12));

        assert_eq!(state.pending_reload_at, Some(start + MOD_RELOAD_MAX_WAIT));
    }

    #[test]
    fn changes_during_in_flight_reload_coalesce_into_a_follow_up() {
        let start = Instant::now();
        let (_sender, receiver) = mpsc::channel();
        let mut state = ModWatcherState {
            reload_receiver: Some(receiver),
            ..ModWatcherState::default()
        };

        // Activity alone during reload does not queue a follow-up reload.
        state.record_activity(start);
        assert!(state.dirty_during_reload.is_none());

        let change_at = start + Duration::from_secs(1);
        state.record_change(change_at);
        assert_eq!(state.dirty_during_reload, Some(change_at));

        // Subsequent activity advances the follow-up timestamp.
        let later_activity = change_at + Duration::from_secs(1);
        state.record_activity(later_activity);
        assert_eq!(state.dirty_during_reload, Some(later_activity));
    }

    #[test]
    fn mods_equivalence_ignores_directory_scan_order_and_detects_metadata_edits() {
        let mut b_edited = sample_mod("author.b", "workshop/2");
        b_edited.mod_type = ModType::CSharp;

        assert!(mods_are_equivalent(
            &[
                sample_mod("author.a", "workshop/1"),
                sample_mod("author.b", "workshop/2"),
            ],
            &[
                sample_mod("author.b", "workshop/2"),
                sample_mod("author.a", "workshop/1"),
            ]
        ));
        assert!(!mods_are_equivalent(
            &[sample_mod("author.a", "workshop/1")],
            &[
                sample_mod("author.a", "workshop/1"),
                sample_mod("author.b", "workshop/2"),
            ]
        ));
        assert!(!mods_are_equivalent(
            &[
                sample_mod("author.a", "workshop/1"),
                sample_mod("author.b", "workshop/2"),
            ],
            &[sample_mod("author.a", "workshop/1"), b_edited]
        ));
    }
}
