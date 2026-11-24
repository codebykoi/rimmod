use eframe::egui;

use crate::models::ModId;
use crate::services::organization::{self, Organization};
use crate::ui::organize::{OrganizeView, show_organize_dialog};

use super::App;

impl App {
    pub(super) fn load_organization(&mut self) {
        let Some(path) = self.organization_path.clone() else {
            return;
        };

        match organization::load(&path) {
            Ok(organization) => self.organization = organization,

            Err(error) => {
                self.organization = Organization::default();
                self.report_action_error(format!(
                    "Could not load tags and colors from {}: {error}",
                    path.display()
                ));
            }
        }
    }

    pub(super) fn persist_organization(&mut self) -> bool {
        let Some(path) = self.organization_path.clone() else {
            return false;
        };

        match organization::save(&self.organization, &path) {
            Ok(()) => true,
            Err(error) => {
                self.action_error = Some(format!("Could not save tags and colors: {error}"));
                false
            }
        }
    }

    pub(super) fn open_organize(&mut self, mod_ids: Vec<ModId>) {
        let mod_ids = mod_ids
            .into_iter()
            .filter(|&mod_id| self.mods.get(mod_id).is_some())
            .collect::<Vec<_>>();

        if mod_ids.is_empty() {
            return;
        }

        self.state.organize_tag_input.clear();
        self.state.organize_target = Some(mod_ids);
    }

    pub(super) fn show_organize_dialog(&mut self, context: &egui::Context) {
        let Some(mod_ids) = self.state.organize_target.clone() else {
            return;
        };

        let mut mod_names = Vec::new();
        let mut package_ids = Vec::new();

        for mod_id in mod_ids {
            if let Some(rimworld_mod) = self.mods.get(mod_id) {
                mod_names.push(rimworld_mod.name.clone());
                package_ids.push(rimworld_mod.package_id.clone());
            }
        }

        if package_ids.is_empty() {
            self.state.organize_target = None;
            return;
        }

        let outcome = show_organize_dialog(
            context,
            OrganizeView {
                mod_names: &mod_names,
                package_ids: &package_ids,
                tag_input: &mut self.state.organize_tag_input,
                organization: &mut self.organization,
            },
        );

        if outcome.changed {
            self.persist_organization();
        }

        if !outcome.open {
            self.state.organize_target = None;
            self.state.organize_tag_input.clear();
        }
    }
}
