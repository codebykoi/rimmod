use std::io;
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::models::{ModId, ModSource, PackageId};
use crate::services::mod_deletion;
use crate::services::mod_loader;
use crate::services::workshop;

use super::{App, LoadOrderStatus};

struct ModDeletionTarget {
    name: String,
    package_id: PackageId,
    folder: PathBuf,
    was_enabled: bool,
}

pub(super) struct PendingModDeletion {
    targets: Vec<ModDeletionTarget>,
    includes_workshop_mod: bool,
}

impl App {
    pub(super) fn request_mod_deletion(&mut self, mod_ids: &[ModId]) {
        let includes_official_content = mod_ids.iter().any(|&mod_id| {
            self.mods
                .get(mod_id)
                .is_some_and(|rimworld_mod| matches!(rimworld_mod.source, ModSource::Official))
        });

        if includes_official_content {
            self.report_action_error("Official RimWorld content cannot be deleted");
            return;
        }

        let targets = mod_ids
            .iter()
            .filter_map(|&mod_id| {
                let rimworld_mod = self.mods.get(mod_id)?;

                Some(ModDeletionTarget {
                    name: rimworld_mod.name.clone(),
                    package_id: rimworld_mod.package_id.clone(),
                    folder: rimworld_mod.folder.clone(),
                    was_enabled: self.mods.enabled_ids().contains(&mod_id),
                })
            })
            .collect::<Vec<_>>();

        if targets.len() != mod_ids.len() || targets.is_empty() {
            self.report_action_error("Could not prepare the selected mods for deletion");
            return;
        }

        let includes_workshop_mod = mod_ids.iter().any(|&mod_id| {
            self.mods.get(mod_id).is_some_and(|rimworld_mod| {
                matches!(rimworld_mod.source, ModSource::SteamWorkshop { .. })
            })
        });

        self.pending_mod_deletion = Some(PendingModDeletion {
            targets,
            includes_workshop_mod,
        });
    }

    fn delete_pending_mods(&mut self) {
        let Some(pending) = self.pending_mod_deletion.take() else {
            return;
        };

        let allowed_roots = [
            self.settings.local_path(),
            self.settings.workshop_path().map(Path::to_path_buf),
            workshop::steamcmd_workshop_path(self.settings.steamcmd_path()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let mut deleted_targets = Vec::new();
        let mut failures = Vec::new();

        for target in pending.targets {
            match mod_deletion::remove_mod_folder(&target.folder, &allowed_roots) {
                Ok(()) => deleted_targets.push(target),
                Err(error) => failures.push(format!("{}: {error}", target.name)),
            }
        }

        if !deleted_targets.is_empty() {
            let remaining_active_package_ids = self
                .mods
                .active_package_ids()
                .into_iter()
                .filter(|package_id| {
                    !deleted_targets
                        .iter()
                        .any(|target| target.was_enabled && target.package_id == *package_id)
                })
                .collect();

            if let Err(error) = self.reload_after_mod_deletion(remaining_active_package_ids) {
                failures.push(format!("could not reload mods: {error}"));
            }

            let count = deleted_targets.len();
            let noun = if count == 1 { "mod" } else { "mods" };
            let deleted_an_active_mod = deleted_targets.iter().any(|target| target.was_enabled);
            let message = if deleted_an_active_mod {
                format!("Deleted {count} {noun}; save to update the RimWorld load order")
            } else {
                format!("Deleted {count} {noun}")
            };
            self.toasts.success(message);
        }

        if failures.is_empty() {
            self.clear_action_error();
        } else {
            self.report_action_error(format!("Could not delete: {}", failures.join("; ")));
        }
    }

    fn reload_after_mod_deletion(&mut self, active_package_ids: Vec<PackageId>) -> io::Result<()> {
        let mut report = match mod_loader::load_mods(&self.settings, self.game_version.as_deref()) {
            Ok(report) => report,
            Err(error) => {
                self.mod_load_warnings.clear();
                self.load_order_status = LoadOrderStatus::Unavailable {
                    reason: error.to_string(),
                };
                return Err(error);
            }
        };
        report.mods.replace_active_package_ids(active_package_ids);

        self.mods = report.mods;
        self.mod_load_warnings = report.warnings;
        self.load_order_status = LoadOrderStatus::Ready;
        self.clear_mod_selection();
        self.clear_load_order_history();

        Ok(())
    }

    pub(super) fn show_mod_deletion_confirmation(&mut self, context: &egui::Context) {
        let Some(pending) = self.pending_mod_deletion.as_ref() else {
            return;
        };

        enum DialogAction {
            Cancel,
            Delete,
        }

        let mut action = None;
        let response =
            egui::Modal::new(egui::Id::new("confirm_mod_deletion")).show(context, |ui| {
                let count = pending.targets.len();
                let noun = if count == 1 { "this mod" } else { "these mods" };

                ui.heading("Delete mods from disk?");
                ui.add_space(8.0);
                ui.label(format!(
                    "This permanently deletes {noun} and cannot be undone:"
                ));
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for target in &pending.targets {
                            ui.label(format!("- {}", target.name));
                        }
                    });

                if pending.includes_workshop_mod {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "Subscribed Workshop mods may be downloaded again by Steam.",
                        )
                        .italics(),
                    );
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        action = Some(DialogAction::Cancel);
                    }

                    let delete_button = egui::Button::new(
                        egui::RichText::new("Delete permanently")
                            .color(ui.visuals().error_fg_color),
                    );
                    if ui.add(delete_button).clicked() {
                        action = Some(DialogAction::Delete);
                    }
                });
            });

        if response.should_close() {
            action = Some(DialogAction::Cancel);
        }

        match action {
            Some(DialogAction::Cancel) => self.pending_mod_deletion = None,
            Some(DialogAction::Delete) => self.delete_pending_mods(),
            None => {}
        }
    }
}
