use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::models::{ModSource, PackageId};
use crate::services::load_order;
use crate::services::mod_loader;
use crate::services::modpack::{
    self, AppliedPack, ModpackManifest, PackApplyOptions, PackDiff, PackMod, PackOrganization,
    PackSource, PackSummary, PackWriteRequest,
};
use crate::services::workshop;
use crate::ui;
use crate::ui::OpenTarget;
use crate::ui::modpacks::{
    ApplyDiffDecision, ApplyDiffView, DropPreviewDecision, DropPreviewItem, DropPreviewView,
    ModpacksAction, ModpacksUiState, PackEditorMode, PackEditorOutcome, PackEditorView,
};

use super::workshop_ctrl::SubscriptionStatus;
use super::{App, AppTab, LoadOrderStatus};

pub(super) fn open_exported_pack_folder_action() -> egui::Id {
    egui::Id::new("open_exported_pack_folder")
}

struct PackBusy {
    title: String,
    label: Arc<Mutex<String>>,
    receiver: Receiver<PackTaskResult>,
    progress: Arc<AtomicU8>,
}

struct PackResultDialog {
    title: String,
    success: bool,
    lines: Vec<String>,
    warnings: Vec<String>,
}

impl PackResultDialog {
    fn failure(title: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            success: false,
            lines: vec![error.into()],
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct ProgressHandle {
    percent: Arc<AtomicU8>,
    label: Arc<Mutex<String>>,
}

impl ProgressHandle {
    fn set(&self, fraction: f32) {
        let percent = (fraction.clamp(0.0, 1.0) * 100.0).round() as u8;
        self.percent.store(percent, Ordering::Relaxed);
    }

    fn set_label(&self, label: impl Into<String>) {
        if let Ok(mut current) = self.label.lock() {
            *current = label.into();
        }
    }
}

enum PackTaskResult {
    Created(Result<PackSummary, String>),
    Updated {
        result: Result<PackSummary, String>,
        from_active: bool,
    },
    Duplicated(Result<PackSummary, String>),
    Imported {
        result: Result<Vec<PackSummary>, String>,
        apply_after: bool,
    },
    Exported(Result<PathBuf, String>),
    Applied(Result<Box<ApplyOutcome>, String>),
}

struct ApplyOutcome {
    pack_path: PathBuf,
    applied: AppliedPack,
    installed_workshop_mods: Vec<u64>,
    subscribed_workshop_mods: Vec<u64>,
    install_error: Option<String>,
    install_errors: Vec<(u64, String)>,
    subscribe_errors: Vec<(u64, String)>,
    subscription_unavailable: bool,
}

pub(super) struct PackEditor {
    pub(super) mode: PackEditorMode,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) tags_input: String,
    pub(super) mod_search: String,
    pub(super) icon: Option<Arc<[u8]>>,
    pub(super) icon_generation: u64,
    pub(super) icon_drop_hovered: bool,
    pub(super) mods: Vec<PackMod>,
    pub(super) local_mods: Vec<(String, PathBuf)>,
    pub(super) excluded_mods: HashSet<String>,
    pub(super) config_files: Vec<(String, PathBuf)>,
    pub(super) excluded_configs: HashSet<String>,
    pub(super) saves: Vec<(String, PathBuf)>,
    pub(super) selected_saves: HashSet<String>,
    pub(super) organization: BTreeMap<String, PackOrganization>,
    pub(super) game_version: Option<String>,
    pub(super) target_path: Option<PathBuf>,
    pub(super) update_diff: Option<PackDiff>,
}

struct PendingPackApply {
    path: PathBuf,
    options: PackApplyOptions,
}

#[derive(Default)]
pub(super) struct ModpackState {
    pub(super) packs: Vec<PackSummary>,
    pub(super) selected: Option<PathBuf>,
    pub(super) active_pack: Option<PathBuf>,
    pub(super) ui_state: ModpacksUiState,
    pub(super) loaded: bool,
    pub(super) library_error: Option<String>,
    pub(super) editor: Option<PackEditor>,
    busy: Option<PackBusy>,
    result: Option<PackResultDialog>,
    pending_apply: Option<PendingPackApply>,
    pending_delete: Option<PathBuf>,
    pending_drop: Option<Vec<DropPreviewItem>>,
    last_exported_folder: Option<PathBuf>,
}

impl ModpackState {
    pub(super) fn has_open_modal(&self) -> bool {
        self.editor.is_some()
            || self.busy.is_some()
            || self.result.is_some()
            || self.pending_apply.is_some()
            || self.pending_delete.is_some()
            || self.pending_drop.is_some()
    }
}

impl App {
    pub(super) fn ensure_modpack_library_loaded(&mut self) {
        if self.modpacks.loaded {
            return;
        }

        self.modpacks.loaded = true;
        self.refresh_pack_library();
    }

    pub(super) fn refresh_pack_library(&mut self) {
        match modpack::list_library() {
            Ok(packs) => {
                self.modpacks.packs = packs;
                self.modpacks.library_error = None;

                let selection_valid = self.modpacks.selected.as_ref().is_some_and(|selected| {
                    self.modpacks
                        .packs
                        .iter()
                        .any(|pack| paths_equal(&pack.path, selected))
                });

                if !selection_valid {
                    self.modpacks.selected =
                        self.modpacks.packs.first().map(|pack| pack.path.clone());
                }

                // Clear tracked active pack if its folder was deleted externally.
                if let Some(active) = self.modpacks.active_pack.as_ref()
                    && !self
                        .modpacks
                        .packs
                        .iter()
                        .any(|pack| paths_equal(&pack.path, active))
                {
                    self.modpacks.active_pack = None;
                }
            }
            Err(error) => {
                self.modpacks.library_error =
                    Some(format!("Could not read the modpack library: {error}"));
            }
        }
    }

    pub(super) fn pack_busy_label(&self) -> Option<String> {
        self.modpacks
            .busy
            .as_ref()
            .and_then(|busy| busy.label.lock().ok().map(|label| label.clone()))
    }

    pub(super) fn open_last_exported_pack_folder(&mut self) {
        if let Some(folder) = self.modpacks.last_exported_folder.clone() {
            self.open_target(OpenTarget::Folder(folder));
        }
    }

    pub(super) fn handle_modpacks_action(
        &mut self,
        action: ModpacksAction,
        context: &egui::Context,
    ) {
        match action {
            ModpacksAction::NewFromActive => {
                if self.pack_busy_label().is_some() {
                    self.toasts
                        .info("Wait for the current modpack task to finish first");
                    return;
                }

                self.begin_pack_editor();
            }
            ModpacksAction::EditDetails(path) => {
                if self.pack_busy_label().is_some() {
                    self.toasts
                        .info("Wait for the current modpack task to finish first");
                    return;
                }

                self.begin_edit_pack_details(path);
            }
            ModpacksAction::UpdateFromActive(path) => {
                if self.pack_busy_label().is_some() {
                    self.toasts
                        .info("Wait for the current modpack task to finish first");
                    return;
                }

                self.begin_update_pack_from_active(path);
            }
            ModpacksAction::Duplicate(path) => self.duplicate_pack(path, context),
            ModpacksAction::Import => self.import_pack_from_dialog(context),
            ModpacksAction::Refresh => {
                if self.pack_busy_label().is_some() {
                    self.toasts
                        .info("Wait for the current modpack task to finish first");
                    return;
                }

                self.refresh_pack_library();
            }
            ModpacksAction::Select(path) => {
                self.modpacks.selected = Some(path);
            }
            ModpacksAction::Apply(path) => {
                if self.pack_busy_label().is_some() {
                    self.toasts
                        .info("Wait for the current modpack task to finish first");
                    return;
                }

                self.modpacks.pending_apply = Some(PendingPackApply {
                    path,
                    options: PackApplyOptions::default(),
                });
            }
            ModpacksAction::Export(path) => self.export_pack(path, context),
            ModpacksAction::CopyModList(path) => {
                if let Some(pack) = self
                    .modpacks
                    .packs
                    .iter()
                    .find(|pack| paths_equal(&pack.path, &path))
                {
                    let formatted = modpack::format_mod_list_for_share(&pack.manifest);
                    context.copy_text(formatted);
                    self.toasts.info("Mod list copied to clipboard");
                }
            }
            ModpacksAction::CopyText { text, label } => {
                context.copy_text(text);
                self.toasts.info(format!("{label} copied to clipboard"));
            }
            ModpacksAction::Open(target) => self.open_target(target),
            ModpacksAction::Delete(path) => {
                if self.pack_busy_label().is_some() {
                    self.toasts
                        .info("Wait for the current modpack task to finish first");
                    return;
                }

                self.modpacks.pending_delete = Some(path);
            }
            ModpacksAction::ChooseIcon => self.choose_pack_icon(),
            ModpacksAction::RemoveIcon => {
                if let Some(editor) = self.modpacks.editor.as_mut() {
                    editor.icon = None;
                    editor.icon_generation = editor.icon_generation.wrapping_add(1);
                }
            }
            ModpacksAction::SaveEditor => self.save_pack_editor(context),
            ModpacksAction::CancelEditor => self.modpacks.editor = None,
        }
    }

    /// Handle OS file drag-and-drop across the entire window:
    /// - `.rimpack` files anywhere → drop overlay + preview modal (`Import` / `Import & Apply`)
    /// - Image files while the Pack Editor is open → set modpack icon
    pub(super) fn handle_file_drop(&mut self, context: &egui::Context) {
        let (hovered_files, dropped_files) = context.input(|input| {
            (
                input.raw.hovered_files.clone(),
                input.raw.dropped_files.clone(),
            )
        });

        if let Some(editor) = self.modpacks.editor.as_mut() {
            editor.icon_drop_hovered = !hovered_files.is_empty()
                && hovered_files.iter().any(|file| {
                    file.path
                        .as_deref()
                        .is_none_or(modpack::is_supported_icon_path)
                });

            if !dropped_files.is_empty() {
                let image_path = dropped_files
                    .iter()
                    .map(|file| file.path())
                    .find(|path| modpack::is_supported_icon_path(path))
                    .map(Path::to_path_buf);

                if let Some(path) = image_path {
                    self.apply_icon_from_path(&path);
                } else if dropped_files
                    .iter()
                    .map(|file| file.path())
                    .any(modpack::is_pack_path)
                {
                    self.toasts
                        .info("Close the modpack editor before importing a .rimpack file");
                }
            }

            return;
        }

        if self.pack_busy_label().is_some() {
            return;
        }

        if !hovered_files.is_empty() {
            let known_paths: Vec<&Path> = hovered_files
                .iter()
                .filter_map(|file| file.path.as_deref())
                .collect();
            let rimpack_names: Vec<String> = known_paths
                .iter()
                .filter(|path| modpack::is_pack_path(path))
                .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
                .map(str::to_owned)
                .collect();

            // Show the overlay when at least one hovered file is a `.rimpack`,
            // or when the platform withholds paths until drop.
            let show_overlay = !rimpack_names.is_empty() || known_paths.is_empty();
            if show_overlay {
                ui::modpacks::show_drop_overlay(context, &rimpack_names);
            }
        }

        if dropped_files.is_empty() {
            return;
        }

        let rimpack_paths: Vec<PathBuf> = dropped_files
            .iter()
            .map(|file| file.path())
            .filter(|path| modpack::is_pack_path(path))
            .map(Path::to_path_buf)
            .collect();

        if rimpack_paths.is_empty() {
            self.toasts.info("Drop a .rimpack file to import a modpack");
            return;
        }

        self.ensure_modpack_library_loaded();

        let mut items = Vec::new();
        let mut errors = Vec::new();

        for path in rimpack_paths {
            match modpack::read_archive_preview(&path) {
                Ok(loaded) => {
                    let file_size = fs::metadata(&path).map_or(0, |meta| meta.len());
                    let already_in_library = self.modpacks.packs.iter().any(|pack| {
                        pack.manifest
                            .name
                            .eq_ignore_ascii_case(&loaded.manifest.name)
                    });

                    items.push(DropPreviewItem {
                        path,
                        file_size,
                        manifest: loaded.manifest,
                        icon: loaded.icon,
                        already_in_library,
                    });
                }
                Err(error) => {
                    errors.push(error.to_string());
                }
            }
        }

        if items.is_empty() {
            if let Some(first_error) = errors.first() {
                self.report_action_error(format!("Could not read dropped modpack: {first_error}"));
            }
            return;
        }

        if !errors.is_empty() {
            self.toasts.warning(format!(
                "Skipped {} unreadable file{}",
                errors.len(),
                plural(errors.len())
            ));
        }

        self.modpacks.pending_drop = Some(items);
    }

    pub(super) fn show_drop_preview_modal(&mut self, context: &egui::Context) {
        let Some(items) = self.modpacks.pending_drop.as_ref() else {
            return;
        };

        let single_diff = if items.len() == 1 {
            Some(modpack::compute_pack_diff(&items[0].manifest, &self.mods))
        } else {
            None
        };

        let decision = ui::modpacks::show_drop_preview_modal(
            context,
            DropPreviewView {
                items,
                diff: single_diff.as_ref(),
                game_version: self.game_version.as_deref(),
                api_key_configured: self.settings.steam_web_api_key().is_some(),
                steamcmd_available: workshop::steamcmd_available(self.settings.steamcmd_path()),
            },
        );

        let Some(decision) = decision else {
            return;
        };

        let Some(items) = self.modpacks.pending_drop.take() else {
            return;
        };

        match decision {
            DropPreviewDecision::Cancel => {}
            DropPreviewDecision::ImportOnly => {
                let paths = items.into_iter().map(|item| item.path).collect();
                self.start_import_archives(paths, false, context);
            }
            DropPreviewDecision::ImportAndApply => {
                let paths = items.into_iter().map(|item| item.path).collect();
                self.start_import_archives(paths, true, context);
            }
        }
    }

    fn begin_pack_editor(&mut self) {
        match self.pack_editor_from_current(PackEditorMode::Create) {
            Some(editor) => self.modpacks.editor = Some(editor),
            None => self.report_action_error("Enable at least one mod before creating a modpack"),
        }
    }

    /// Edit metadata or trim mods/configs/saves inside an existing modpack
    /// WITHOUT replacing its contents with the currently active mod list.
    fn begin_edit_pack_details(&mut self, path: PathBuf) {
        let loaded = match modpack::read_instance(&path) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.report_action_error(format!("Could not read modpack: {error}"));
                return;
            }
        };

        let manifest = loaded.manifest;
        let local_mods = modpack::instance_local_mods(&path, &manifest);
        let config_files = modpack::instance_configs(&path, &manifest);
        let saves = modpack::instance_saves(&path, &manifest);
        let selected_saves = saves
            .iter()
            .map(|(file_name, _)| file_name.clone())
            .collect::<HashSet<_>>();

        self.modpacks.editor = Some(PackEditor {
            mode: PackEditorMode::EditDetails,
            name: manifest.name,
            description: manifest.description,
            tags_input: manifest.tags.join(", "),
            mod_search: String::new(),
            icon: loaded.icon,
            icon_generation: 0,
            icon_drop_hovered: false,
            mods: manifest.mods,
            local_mods,
            excluded_mods: HashSet::new(),
            config_files,
            excluded_configs: HashSet::new(),
            saves,
            selected_saves,
            organization: manifest.organization,
            game_version: manifest.game_version,
            target_path: Some(path),
            update_diff: None,
        });
    }

    /// Explicitly re-capture the current active mods, settings, and saves into
    /// an existing library modpack while preserving its metadata and icon.
    fn begin_update_pack_from_active(&mut self, path: PathBuf) {
        let Some((name, description, tags, icon, configs, saves, diff)) = self
            .modpacks
            .packs
            .iter()
            .find(|pack| paths_equal(&pack.path, &path))
            .map(|pack| {
                (
                    pack.manifest.name.clone(),
                    pack.manifest.description.clone(),
                    pack.manifest.tags.clone(),
                    pack.icon.clone(),
                    pack.manifest.configs.clone(),
                    pack.manifest.saves.clone(),
                    modpack::compute_pack_diff(&pack.manifest, &self.mods),
                )
            })
        else {
            self.report_action_error("That modpack is no longer in the library");
            return;
        };

        let Some(mut editor) = self.pack_editor_from_current(PackEditorMode::UpdateFromActive)
        else {
            self.report_action_error("Enable at least one mod before updating a modpack");
            return;
        };

        editor.name = name;
        editor.description = description;
        editor.tags_input = tags.join(", ");
        editor.icon = icon;
        editor.icon_generation = 0;
        editor.target_path = Some(path);
        editor.update_diff = Some(diff);

        // Keep the previous settings/save selection when those files still
        // exist, so updating mostly means "capture the current progress".
        editor.excluded_configs = editor
            .config_files
            .iter()
            .filter(|(relative_path, _)| {
                !configs
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(relative_path))
            })
            .map(|(relative_path, _)| relative_path.clone())
            .collect();
        editor.selected_saves = editor
            .saves
            .iter()
            .filter(|(file_name, _)| {
                saves
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(file_name))
            })
            .map(|(file_name, _)| file_name.clone())
            .collect();

        self.modpacks.editor = Some(editor);
    }

    fn pack_editor_from_current(&self, mode: PackEditorMode) -> Option<PackEditor> {
        if self.mods.enabled_ids().is_empty() {
            return None;
        }

        let mut mods = Vec::new();
        let mut local_mods = Vec::new();
        let mut organization = BTreeMap::new();
        let mut used_folders = HashSet::new();

        for &mod_id in self.mods.enabled_ids() {
            let Some(rimworld_mod) = self.mods.get(mod_id) else {
                continue;
            };

            let raw_folder_name = rimworld_mod
                .folder
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("mod");
            let folder_name = unique_folder_name(raw_folder_name, &mut used_folders);

            let source = match &rimworld_mod.source {
                ModSource::Official => PackSource::Official,
                ModSource::SteamWorkshop { workshop_id } => PackSource::Workshop {
                    workshop_id: *workshop_id,
                },
                ModSource::Git { remote_url } => {
                    local_mods.push((folder_name.clone(), rimworld_mod.folder.clone()));

                    PackSource::Local {
                        remote_url: Some(remote_url.clone()),
                        path: format!("mods/{folder_name}"),
                    }
                }
                ModSource::Local | ModSource::Unknown => {
                    local_mods.push((folder_name.clone(), rimworld_mod.folder.clone()));

                    PackSource::Local {
                        remote_url: None,
                        path: format!("mods/{folder_name}"),
                    }
                }
            };

            let tags = self.organization.tags(&rimworld_mod.package_id).to_vec();
            let color = self.organization.color(&rimworld_mod.package_id);

            if !tags.is_empty() || color.is_some() {
                organization.insert(
                    rimworld_mod.package_id.as_str().to_owned(),
                    PackOrganization { tags, color },
                );
            }

            mods.push(PackMod {
                package_id: rimworld_mod.package_id.as_str().to_owned(),
                name: rimworld_mod.name.clone(),
                folder_name,
                source,
            });
        }

        let config_files = self
            .settings
            .config_path()
            .map(modpack::discover_config_files)
            .unwrap_or_default();

        let saves = self
            .settings
            .config_path()
            .and_then(|config_path| config_path.parent())
            .map(modpack::discover_save_files)
            .unwrap_or_default();

        Some(PackEditor {
            mode,
            name: "My modpack".to_owned(),
            description: String::new(),
            tags_input: String::new(),
            mod_search: String::new(),
            icon: None,
            icon_generation: 0,
            icon_drop_hovered: false,
            mods,
            local_mods,
            excluded_mods: HashSet::new(),
            config_files,
            excluded_configs: HashSet::new(),
            saves,
            selected_saves: HashSet::new(),
            organization,
            game_version: self.game_version.clone(),
            target_path: None,
            update_diff: None,
        })
    }

    pub(super) fn show_pack_editor(&mut self, context: &egui::Context) {
        let outcome = {
            let Some(editor) = self.modpacks.editor.as_mut() else {
                return;
            };

            ui::modpacks::show_pack_editor(
                context,
                PackEditorView {
                    mode: editor.mode,
                    name: &mut editor.name,
                    description: &mut editor.description,
                    tags_input: &mut editor.tags_input,
                    mod_search: &mut editor.mod_search,
                    icon: editor.icon.as_ref(),
                    icon_generation: editor.icon_generation,
                    icon_drop_hovered: editor.icon_drop_hovered,
                    mods: &editor.mods,
                    excluded_mods: &mut editor.excluded_mods,
                    config_files: &editor.config_files,
                    excluded_configs: &mut editor.excluded_configs,
                    saves: &editor.saves,
                    selected_saves: &mut editor.selected_saves,
                    game_version: editor.game_version.as_deref(),
                    update_diff: editor.update_diff.as_ref(),
                },
            )
        };

        self.handle_pack_editor_outcome(outcome, context);
    }

    fn handle_pack_editor_outcome(&mut self, outcome: PackEditorOutcome, context: &egui::Context) {
        // A save that could not start puts the draft back so the user does not
        // lose their metadata. Successful saves and cancels clear it.
        let keep_open_after_action = matches!(&outcome.action, Some(ModpacksAction::SaveEditor));

        if let Some(action) = outcome.action {
            self.handle_modpacks_action(action, context);
        }

        if !outcome.open && !keep_open_after_action {
            self.modpacks.editor = None;
        }
    }

    fn choose_pack_icon(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose modpack icon")
            .add_filter("Images", &["png", "jpg", "jpeg", "gif"])
            .pick_file()
        else {
            return;
        };

        self.apply_icon_from_path(&path);
    }

    fn apply_icon_from_path(&mut self, path: &Path) {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.report_action_error(format!(
                    "Could not read the icon {}: {error}",
                    path.display()
                ));
                return;
            }
        };

        let icon = match modpack::normalize_icon(&bytes) {
            Ok(icon) => icon,
            Err(error) => {
                self.report_action_error(format!("Could not use that image as an icon: {error}"));
                return;
            }
        };

        if let Some(editor) = self.modpacks.editor.as_mut() {
            editor.icon = Some(Arc::from(icon));
            editor.icon_generation = editor.icon_generation.wrapping_add(1);
            self.toasts.success("Modpack icon updated");
        }

        self.clear_action_error();
    }

    fn save_pack_editor(&mut self, context: &egui::Context) {
        if self.pack_busy_label().is_some() {
            self.toasts
                .info("Wait for the current modpack task to finish first");
            return;
        }

        let Some(editor) = self.modpacks.editor.take() else {
            return;
        };

        let name = editor.name.trim();

        if name.is_empty() {
            self.modpacks.editor = Some(editor);
            self.report_action_error("Give the modpack a name first");
            return;
        }

        let mut manifest = ModpackManifest::new(
            name.chars()
                .take(modpack::MAX_PACK_NAME_CHARS)
                .collect::<String>(),
        );
        manifest.description = editor
            .description
            .trim()
            .chars()
            .take(modpack::MAX_PACK_DESCRIPTION_CHARS)
            .collect();
        manifest.tags = modpack::normalize_tags(&editor.tags_input);
        manifest.game_version = editor.game_version.clone();

        let included_mods = editor
            .mods
            .iter()
            .filter(|pack_mod| !editor.excluded_mods.contains(&pack_mod.package_id))
            .cloned()
            .collect::<Vec<_>>();
        let included_folders = included_mods
            .iter()
            .filter(|pack_mod| pack_mod.is_local())
            .map(|pack_mod| modpack::sanitize_component(&pack_mod.folder_name))
            .collect::<HashSet<_>>();
        let local_mods = editor
            .local_mods
            .iter()
            .filter(|(folder_name, _)| included_folders.contains(folder_name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let configs = editor
            .config_files
            .iter()
            .filter(|(relative_path, source_path)| {
                !editor.excluded_configs.contains(relative_path) && source_path.is_file()
            })
            .cloned()
            .collect::<Vec<_>>();
        let saves = editor
            .saves
            .iter()
            .filter(|(file_name, source_path)| {
                editor.selected_saves.contains(file_name) && source_path.is_file()
            })
            .cloned()
            .collect::<Vec<_>>();

        if included_mods.is_empty() && configs.is_empty() && saves.is_empty() {
            self.modpacks.editor = Some(editor);
            self.report_action_error("Include at least one mod, settings file or save");
            return;
        }

        let included_package_ids = included_mods
            .iter()
            .map(|pack_mod| pack_mod.package_id.clone())
            .collect::<HashSet<_>>();

        manifest.mods = included_mods;
        manifest.organization = editor
            .organization
            .iter()
            .filter(|(package_id, _)| included_package_ids.contains(package_id.as_str()))
            .map(|(package_id, organization)| (package_id.clone(), organization.clone()))
            .collect();

        let mode = editor.mode;
        let destination = match editor.target_path.clone() {
            Some(destination) => {
                let in_library = modpack::library_directory()
                    .as_deref()
                    .is_some_and(|library| destination.starts_with(library));

                if !in_library {
                    self.modpacks.editor = Some(editor);
                    self.report_action_error("That modpack is not in the library");
                    return;
                }

                destination
            }
            None => match modpack::unique_instance_path(&manifest.name) {
                Ok(destination) => destination,
                Err(error) => {
                    self.modpacks.editor = Some(editor);
                    self.report_action_error(format!("Could not create the modpack: {error}"));
                    return;
                }
            },
        };

        let icon = editor.icon.clone();
        let destination_for_task = destination.clone();
        let title = match mode {
            PackEditorMode::Create => "Saving modpack".to_owned(),
            PackEditorMode::EditDetails => "Saving modpack details".to_owned(),
            PackEditorMode::UpdateFromActive => "Updating modpack".to_owned(),
        };
        let step = format!("Writing {}…", manifest.name);

        self.spawn_pack_task(context, title, step, move |sender, progress| {
            let result = (|| -> io::Result<PackSummary> {
                progress.set_label("Copying mods, settings and saves…");
                let mut report = |fraction| progress.set(fraction);

                modpack::write_instance(
                    &destination_for_task,
                    PackWriteRequest {
                        manifest: &manifest,
                        icon: icon.as_deref(),
                        local_mods: &local_mods,
                        configs: &configs,
                        saves: &saves,
                    },
                    &mut report,
                )?;

                modpack::read_instance_summary(&destination_for_task)
            })()
            .map_err(|error| error.to_string());

            let _ = match mode {
                PackEditorMode::Create => sender.send(PackTaskResult::Created(result)),
                PackEditorMode::EditDetails => sender.send(PackTaskResult::Updated {
                    result,
                    from_active: false,
                }),
                PackEditorMode::UpdateFromActive => sender.send(PackTaskResult::Updated {
                    result,
                    from_active: true,
                }),
            };
        });
    }

    fn duplicate_pack(&mut self, source: PathBuf, context: &egui::Context) {
        if self.pack_busy_label().is_some() {
            self.toasts
                .info("Wait for the current modpack task to finish first");
            return;
        }

        let base_name = self
            .modpacks
            .packs
            .iter()
            .find(|pack| paths_equal(&pack.path, &source))
            .map_or_else(|| "Modpack".to_owned(), |pack| pack.manifest.name.clone());
        let new_name = format!("{base_name} (Copy)");

        let destination = match modpack::unique_instance_path(&new_name) {
            Ok(path) => path,
            Err(error) => {
                self.report_action_error(format!("Could not duplicate modpack: {error}"));
                return;
            }
        };

        self.spawn_pack_task(
            context,
            "Duplicating modpack".to_owned(),
            format!("Copying {base_name}…"),
            move |sender, progress| {
                progress.set(0.3);
                let result = modpack::duplicate_instance(&source, &destination, &new_name)
                    .map_err(|error| error.to_string());
                progress.set(1.0);
                let _ = sender.send(PackTaskResult::Duplicated(result));
            },
        );
    }

    fn import_pack_from_dialog(&mut self, context: &egui::Context) {
        if self.pack_busy_label().is_some() {
            self.toasts
                .info("Wait for the current modpack task to finish first");
            return;
        }

        let Some(source) = rfd::FileDialog::new()
            .set_title("Import modpack")
            .add_filter("RimMod modpack", &[modpack::PACK_EXTENSION])
            .pick_file()
        else {
            return;
        };

        self.start_import_archives(vec![source], false, context);
    }

    fn start_import_archives(
        &mut self,
        sources: Vec<PathBuf>,
        apply_after: bool,
        context: &egui::Context,
    ) {
        if self.pack_busy_label().is_some() {
            self.toasts
                .info("Wait for the current modpack task to finish first");
            return;
        }

        if sources.is_empty() {
            return;
        }

        let count = sources.len();
        let title = if count == 1 {
            "Importing modpack".to_owned()
        } else {
            format!("Importing {count} modpacks")
        };

        self.spawn_pack_task(
            context,
            title,
            "Reading .rimpack archive…".to_owned(),
            move |sender, progress| {
                let result = (|| -> io::Result<Vec<PackSummary>> {
                    let mut imported = Vec::with_capacity(count);

                    for (index, source) in sources.iter().enumerate() {
                        let manifest = modpack::read_archive_manifest(source)?;
                        let destination = modpack::unique_instance_path(&manifest.name)?;
                        let name = manifest.name.clone();

                        progress.set_label(if count == 1 {
                            format!("Extracting {name}…")
                        } else {
                            format!("Extracting {name} ({}/{count})…", index + 1)
                        });

                        let mut report = |fraction: f32| {
                            let overall = (index as f32 + fraction) / count as f32;
                            progress.set(overall);
                        };

                        modpack::import_archive(source, &destination, &mut report)?;
                        imported.push(modpack::read_instance_summary(&destination)?);
                    }

                    Ok(imported)
                })()
                .map_err(|error| error.to_string());

                let _ = sender.send(PackTaskResult::Imported {
                    result,
                    apply_after,
                });
            },
        );
    }

    fn export_pack(&mut self, source: PathBuf, context: &egui::Context) {
        if self.pack_busy_label().is_some() {
            self.toasts
                .info("Wait for the current modpack task to finish first");
            return;
        }

        let name = self
            .modpacks
            .packs
            .iter()
            .find(|pack| paths_equal(&pack.path, &source))
            .map_or_else(
                || "modpack".to_owned(),
                |pack| modpack::sanitize_file_stem(&pack.manifest.name),
            );

        let file_name = format!("{name}.{}", modpack::PACK_EXTENSION);
        let dialog = rfd::FileDialog::new()
            .set_title("Export modpack")
            .set_file_name(&file_name)
            .add_filter("RimMod modpack", &[modpack::PACK_EXTENSION]);

        let Some(mut destination) = dialog.save_file() else {
            return;
        };

        if destination.extension().is_none() {
            destination.set_extension(modpack::PACK_EXTENSION);
        }

        if paths_equal(&destination, &source) {
            self.toasts
                .info("The modpack is already saved at that location");
            return;
        }

        self.spawn_pack_task(
            context,
            "Exporting modpack".to_owned(),
            format!("Packing {name}…"),
            move |sender, progress| {
                progress.set_label("Writing .rimpack archive…");
                let mut report = |fraction| progress.set(fraction);
                let result = modpack::export_instance(&source, &destination, &mut report)
                    .map(|_| destination)
                    .map_err(|error| error.to_string());

                let _ = sender.send(PackTaskResult::Exported(result));
            },
        );
    }

    fn start_apply_pack(
        &mut self,
        path: PathBuf,
        options: PackApplyOptions,
        context: &egui::Context,
    ) {
        if self.pack_busy_label().is_some() {
            self.toasts
                .info("Wait for the current modpack task to finish first");
            return;
        }

        let Some(mods_root) = self.settings.local_path().map(|path| path.to_path_buf()) else {
            self.report_action_error("RimWorld's Mods folder is not configured");
            return;
        };

        let Some(config_root) = self.settings.config_path().map(|path| path.to_path_buf()) else {
            self.report_action_error("RimWorld's configuration folder is not configured");
            return;
        };

        let Some(saves_root) = config_root
            .parent()
            .map(|data_root| data_root.join("Saves"))
        else {
            self.report_action_error("RimWorld's save folder could not be located");
            return;
        };

        let api_key = self.settings.steam_web_api_key().map(str::to_owned);
        let workshop_path = self.settings.workshop_path().map(Path::to_path_buf);
        let steamcmd_workshop_path =
            workshop::steamcmd_workshop_path(self.settings.steamcmd_path());
        let steamcmd_available = workshop::steamcmd_available(self.settings.steamcmd_path());
        let steamcmd_path = self
            .settings
            .steamcmd_path()
            .map(Path::to_path_buf)
            .unwrap_or_else(workshop::default_steamcmd_path);

        self.spawn_pack_task(
            context,
            "Applying modpack".to_owned(),
            "Copying mods, settings and saves…".to_owned(),
            move |sender, progress| {
                let result = (|| -> Result<ApplyOutcome, String> {
                    let mut apply_report = |fraction: f32| progress.set(fraction * 0.6);
                    let applied = modpack::apply_instance_with_options(
                        &path,
                        &mods_root,
                        &config_root,
                        &saves_root,
                        options,
                        &mut apply_report,
                    )
                    .map_err(|error| error.to_string())?;

                    progress.set_label("Checking Workshop mods…");
                    progress.set(0.6);

                    let missing_workshop_mods = applied
                        .manifest
                        .mods
                        .iter()
                        .filter_map(PackMod::workshop_id)
                        .filter(|workshop_id| {
                            !workshop::is_item_installed(
                                workshop_path.as_deref(),
                                steamcmd_workshop_path.as_deref(),
                                *workshop_id,
                            )
                        })
                        .collect::<Vec<_>>();

                    let mut installed_workshop_mods = Vec::new();
                    let mut install_errors = Vec::new();
                    let mut install_error = None;

                    if !missing_workshop_mods.is_empty() && steamcmd_available {
                        progress.set_label(format!(
                            "Downloading {} missing Workshop mod{}…",
                            missing_workshop_mods.len(),
                            plural(missing_workshop_mods.len())
                        ));
                        progress.set(0.62);

                        match workshop::install_workshop_items(
                            &steamcmd_path,
                            &missing_workshop_mods,
                        ) {
                            Ok(outcomes) => {
                                for outcome in outcomes {
                                    match outcome.error {
                                        Some(error) => {
                                            install_errors.push((outcome.published_file_id, error));
                                        }
                                        None => {
                                            installed_workshop_mods.push(outcome.published_file_id)
                                        }
                                    }
                                }
                            }
                            Err(error) => install_error = Some(error.to_string()),
                        }
                    }

                    progress.set(0.7);

                    let subscription_unavailable =
                        !missing_workshop_mods.is_empty() && api_key.is_none();
                    let mut subscribed_workshop_mods = Vec::new();
                    let mut subscribe_errors = Vec::new();

                    if let Some(api_key) = api_key.as_deref() {
                        progress.set_label(format!(
                            "Subscribing to {} missing Workshop mod{}…",
                            missing_workshop_mods.len(),
                            plural(missing_workshop_mods.len())
                        ));
                        let total = missing_workshop_mods.len().max(1);

                        for (index, workshop_id) in missing_workshop_mods.iter().enumerate() {
                            match workshop::set_subscription(api_key, *workshop_id, true) {
                                Ok(()) => subscribed_workshop_mods.push(*workshop_id),
                                Err(error) => subscribe_errors.push((*workshop_id, error)),
                            }

                            progress.set(0.7 + 0.22 * ((index + 1) as f32 / total as f32));
                        }
                    }

                    let remaining_missing = missing_workshop_mods
                        .iter()
                        .copied()
                        .filter(|workshop_id| {
                            !workshop::is_item_installed(
                                workshop_path.as_deref(),
                                steamcmd_workshop_path.as_deref(),
                                *workshop_id,
                            )
                        })
                        .collect::<Vec<_>>();

                    let can_wait_for_steam = !remaining_missing.is_empty()
                        && (!subscribed_workshop_mods.is_empty()
                            || !installed_workshop_mods.is_empty());

                    if can_wait_for_steam {
                        let deadline = Instant::now() + Duration::from_secs(120);
                        let total = remaining_missing.len();

                        loop {
                            let installed = remaining_missing
                                .iter()
                                .filter(|workshop_id| {
                                    workshop::is_item_installed(
                                        workshop_path.as_deref(),
                                        steamcmd_workshop_path.as_deref(),
                                        **workshop_id,
                                    )
                                })
                                .count();

                            if installed == total {
                                break;
                            }

                            progress.set_label(format!(
                                "Waiting for Steam to download Workshop mods ({installed} / {total})…"
                            ));
                            progress.set(0.92 + 0.06 * (installed as f32 / total as f32));

                            if Instant::now() >= deadline {
                                break;
                            }

                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }

                    progress.set_label("Loading mods and saving the load order…");
                    progress.set(0.99);

                    Ok(ApplyOutcome {
                        pack_path: path,
                        applied,
                        installed_workshop_mods,
                        subscribed_workshop_mods,
                        install_error,
                        install_errors,
                        subscribe_errors,
                        subscription_unavailable,
                    })
                })();

                let _ = sender.send(PackTaskResult::Applied(result.map(Box::new)));
            },
        );
    }

    fn spawn_pack_task<F>(&mut self, context: &egui::Context, title: String, step: String, task: F)
    where
        F: FnOnce(Sender<PackTaskResult>, ProgressHandle) + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        let context = context.clone();
        let progress = Arc::new(AtomicU8::new(0));
        let label = Arc::new(Mutex::new(step));
        let handle = ProgressHandle {
            percent: progress.clone(),
            label: label.clone(),
        };

        self.modpacks.result = None;
        self.modpacks.busy = Some(PackBusy {
            title,
            label,
            receiver,
            progress,
        });

        std::thread::spawn(move || {
            task(sender, handle);
            context.request_repaint();
        });
    }

    pub(super) fn poll_pack_tasks(&mut self, context: &egui::Context) {
        let result = match &self.modpacks.busy {
            Some(busy) => busy.receiver.try_recv(),
            None => return,
        };

        match result {
            Ok(task) => {
                self.modpacks.busy = None;
                self.finish_pack_task(task, context);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.modpacks.busy = None;
                self.report_action_error("The modpack worker stopped unexpectedly");
            }
        }
    }

    fn finish_pack_task(&mut self, task: PackTaskResult, context: &egui::Context) {
        let dialog: Option<PackResultDialog> = match task {
            PackTaskResult::Created(result) => match result {
                Ok(pack) => {
                    let diff = modpack::compute_pack_diff(&pack.manifest, &self.mods);
                    if diff.is_in_sync() {
                        self.modpacks.active_pack = Some(pack.path.clone());
                    }
                    let name = pack.manifest.name.clone();
                    let mod_count = pack.manifest.mods.len();
                    self.modpacks.selected = Some(pack.path);
                    self.refresh_pack_library();
                    self.clear_action_error();
                    self.toasts.success(format!(
                        "Saved modpack \"{name}\" ({mod_count} mod{})",
                        plural(mod_count)
                    ));
                    None
                }
                Err(error) => {
                    self.refresh_pack_library();
                    self.action_error = Some(format!("Could not save modpack: {error}"));
                    Some(PackResultDialog::failure("Could not save modpack", error))
                }
            },
            PackTaskResult::Updated {
                result,
                from_active,
            } => match result {
                Ok(pack) => {
                    if from_active {
                        let diff = modpack::compute_pack_diff(&pack.manifest, &self.mods);
                        if diff.is_in_sync() {
                            self.modpacks.active_pack = Some(pack.path.clone());
                        }
                    }
                    let name = pack.manifest.name.clone();
                    self.modpacks.selected = Some(pack.path);
                    self.refresh_pack_library();
                    self.clear_action_error();
                    self.toasts.success(if from_active {
                        format!("Updated \"{name}\" from active mods")
                    } else {
                        format!("Saved details for \"{name}\"")
                    });
                    None
                }
                Err(error) => {
                    self.refresh_pack_library();
                    self.action_error = Some(format!("Could not save modpack changes: {error}"));
                    Some(PackResultDialog::failure(
                        "Could not save modpack changes",
                        error,
                    ))
                }
            },
            PackTaskResult::Duplicated(result) => match result {
                Ok(pack) => {
                    let name = pack.manifest.name.clone();
                    self.modpacks.selected = Some(pack.path);
                    self.refresh_pack_library();
                    self.clear_action_error();
                    self.toasts.success(format!("Duplicated as \"{name}\""));
                    None
                }
                Err(error) => {
                    self.refresh_pack_library();
                    self.action_error = Some(format!("Could not duplicate modpack: {error}"));
                    Some(PackResultDialog::failure(
                        "Could not duplicate modpack",
                        error,
                    ))
                }
            },
            PackTaskResult::Imported {
                result,
                apply_after,
            } => match result {
                Ok(packs) => {
                    self.refresh_pack_library();
                    self.clear_action_error();

                    if let Some(last) = packs.last() {
                        self.modpacks.selected = Some(last.path.clone());
                    }

                    if apply_after && packs.len() == 1 {
                        let path = packs[0].path.clone();
                        self.start_apply_pack(path, PackApplyOptions::default(), context);
                        return;
                    }

                    self.current_tab = AppTab::Modpacks;

                    if packs.len() == 1 {
                        self.toasts
                            .success(format!("Imported modpack \"{}\"", packs[0].manifest.name));
                    } else {
                        self.toasts
                            .success(format!("Imported {} modpacks", packs.len()));
                    }
                    None
                }
                Err(error) => {
                    self.action_error = Some(format!("Could not import modpack: {error}"));
                    Some(PackResultDialog::failure("Could not import modpack", error))
                }
            },
            PackTaskResult::Exported(result) => match result {
                Ok(path) => {
                    self.clear_action_error();
                    self.modpacks.last_exported_folder = path.parent().map(Path::to_path_buf);
                    let file_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("modpack.rimpack");
                    self.toasts
                        .success(format!("Exported {file_name} — click to open folder"))
                        .click_action(open_exported_pack_folder_action());
                    None
                }
                Err(error) => {
                    self.action_error = Some(format!("Could not export modpack: {error}"));
                    Some(PackResultDialog::failure("Could not export modpack", error))
                }
            },
            PackTaskResult::Applied(result) => match result {
                Ok(outcome) => self.finish_applying_pack(*outcome, context),
                Err(error) => {
                    self.action_error = Some(format!("Could not apply modpack: {error}"));
                    Some(PackResultDialog::failure("Could not apply modpack", error))
                }
            },
        };

        self.modpacks.result = dialog;
    }

    fn finish_applying_pack(
        &mut self,
        outcome: ApplyOutcome,
        context: &egui::Context,
    ) -> Option<PackResultDialog> {
        let ApplyOutcome {
            pack_path,
            applied,
            installed_workshop_mods,
            subscribed_workshop_mods,
            install_error,
            install_errors,
            subscribe_errors,
            subscription_unavailable,
        } = outcome;

        let mut report = match mod_loader::load_mods(&self.settings, self.game_version.as_deref()) {
            Ok(report) => report,
            Err(error) => {
                let message =
                    format!("Modpack files were copied, but mods could not be reloaded: {error}");
                self.action_error = Some(message.clone());
                return Some(PackResultDialog::failure(
                    "Could not finish applying modpack",
                    message,
                ));
            }
        };

        let mut enabled_package_ids = Vec::new();
        let mut seen_package_ids = HashSet::new();
        let mut missing_mods = Vec::new();

        let subscribed_ids = subscribed_workshop_mods
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let installed_ids = installed_workshop_mods
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let mut save_package_ids = Vec::new();
        let mut pending_workshop_mods = Vec::new();
        let mut pending_package_ids = Vec::new();

        for pack_mod in &applied.manifest.mods {
            let Some(package_id) = PackageId::new(&pack_mod.package_id) else {
                missing_mods.push(pack_mod.clone());
                continue;
            };

            if report.mods.find_id_by_package_id(&package_id).is_some() {
                if seen_package_ids.insert(package_id.clone()) {
                    enabled_package_ids.push(package_id.clone());
                }

                if !save_package_ids.contains(&package_id) {
                    save_package_ids.push(package_id);
                }
            } else {
                let is_pending_workshop_mod = pack_mod.workshop_id().is_some_and(|workshop_id| {
                    subscribed_ids.contains(&workshop_id) || installed_ids.contains(&workshop_id)
                });

                if is_pending_workshop_mod {
                    if !save_package_ids.contains(&package_id) {
                        save_package_ids.push(package_id.clone());
                    }

                    if !pending_package_ids.contains(&package_id) {
                        pending_package_ids.push(package_id);
                    }

                    pending_workshop_mods.push(pack_mod.clone());
                } else {
                    missing_mods.push(pack_mod.clone());
                }
            }
        }

        let mut active_package_ids = enabled_package_ids.clone();
        active_package_ids.extend(pending_package_ids);

        report.mods.replace_active_package_ids(active_package_ids);

        self.mods = report.mods;
        self.mod_load_warnings = report.warnings;
        self.load_order_status = LoadOrderStatus::Ready;
        self.modpacks.active_pack = Some(pack_path);
        self.clear_mod_selection();
        self.clear_load_order_history();

        for (package_id, pack_organization) in &applied.manifest.organization {
            let Some(package_id) = PackageId::new(package_id) else {
                continue;
            };

            for tag in &pack_organization.tags {
                self.organization.add_tag(&package_id, tag);
            }

            if let Some(color) = pack_organization.color {
                self.organization.set_color(&package_id, Some(color));
            }
        }

        let organization_saved = self.persist_organization();

        let save_result = self
            .settings
            .config_path()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "RimWorld's configuration folder is not configured",
                )
            })
            .and_then(|config_path| {
                if pending_workshop_mods.is_empty() {
                    load_order::save_load_order(config_path, &self.mods)
                } else {
                    load_order::save_load_order_package_ids(config_path, &save_package_ids)
                }
            });

        self.current_tab = AppTab::Mods;
        self.restart_mod_watcher(context);

        let mut lines = vec![format!("{} mods enabled", enabled_package_ids.len())];
        let mut warnings = Vec::new();

        let local_installed = applied.installed_local_mods.len();
        let configs_restored = applied.restored_configs.len();
        let saves_restored = applied.restored_saves.len();

        if local_installed > 0 {
            lines.push(format!(
                "{local_installed} local mod{} copied",
                plural(local_installed)
            ));
        }

        if configs_restored > 0 {
            lines.push(format!(
                "{configs_restored} settings file{} restored",
                plural(configs_restored)
            ));
        }

        if saves_restored > 0 {
            lines.push(format!(
                "{saves_restored} save{} restored",
                plural(saves_restored)
            ));
        }

        if !installed_workshop_mods.is_empty() {
            lines.push(format!(
                "{} Workshop mod{} downloaded",
                installed_workshop_mods.len(),
                plural(installed_workshop_mods.len())
            ));
        }

        if !subscribed_workshop_mods.is_empty() {
            lines.push(format!(
                "{} Workshop subscription{} added",
                subscribed_workshop_mods.len(),
                plural(subscribed_workshop_mods.len())
            ));
            // The Workshop tab should reload its subscription list next time.
            self.workshop.subscriptions = SubscriptionStatus::NotLoaded;
        }

        if !pending_workshop_mods.is_empty() {
            lines.push(format!(
                "{} Workshop mod{} queued for download",
                pending_workshop_mods.len(),
                plural(pending_workshop_mods.len())
            ));
        }

        if let Err(error) = save_result {
            let message = format!(
                "Mods were enabled in RimMod, but the load order could not be saved: {error}"
            );
            self.action_error = Some(message.clone());
            self.load_order_status = LoadOrderStatus::Unavailable {
                reason: message.clone(),
            };

            return Some(PackResultDialog {
                title: "Could not finish applying modpack".to_owned(),
                success: false,
                lines,
                warnings,
            });
        }

        if !pending_workshop_mods.is_empty() {
            self.load_order_status = LoadOrderStatus::Pending {
                reason: format!(
                    "Steam is downloading: {}. Press Run to start RimWorld; saving is disabled until they are installed.",
                    summarize_mods(&pending_workshop_mods)
                ),
            };
        }

        if organization_saved {
            self.clear_action_error();
        } else {
            warnings.push("Tags and colors could not be saved.".to_owned());
        }

        if let Some(error) = install_error {
            warnings.push(format!("Could not download Workshop mods: {error}"));
        }

        if !install_errors.is_empty() {
            let ids = install_errors
                .iter()
                .take(4)
                .map(|(workshop_id, _)| workshop_id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            warnings.push(format!(
                "SteamCMD could not download {} Workshop mod{}: {ids}{}",
                install_errors.len(),
                plural(install_errors.len()),
                if install_errors.len() > 4 {
                    ", …"
                } else {
                    ""
                }
            ));
        }

        if subscription_unavailable {
            warnings.push(
                "Add a Steam Web API key in Settings to auto-subscribe missing Workshop mods."
                    .to_owned(),
            );
        }

        if !subscribe_errors.is_empty() {
            let ids = subscribe_errors
                .iter()
                .take(4)
                .map(|(workshop_id, _)| workshop_id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            warnings.push(format!(
                "Could not subscribe to {} Workshop mod{}: {ids}{}",
                subscribe_errors.len(),
                plural(subscribe_errors.len()),
                if subscribe_errors.len() > 4 {
                    ", …"
                } else {
                    ""
                }
            ));
        }

        if !pending_workshop_mods.is_empty() {
            warnings.push(format!(
                "{} Workshop mod{} already in the load order and will be downloaded by Steam automatically. Press Run when it finishes.",
                pending_workshop_mods.len(),
                plural(pending_workshop_mods.len())
            ));
        }

        if !applied.skipped_saves.is_empty() {
            let save_count = applied.skipped_saves.len();
            warnings.push(format!(
                "Kept {save_count} existing save{}: {}. Enable \"Replace existing saves\" to overwrite.",
                plural(save_count),
                summarize_names(&applied.skipped_saves)
            ));
        }

        if !applied.skipped_local_mods.is_empty() {
            warnings.push(format!(
                "Kept existing mod folders untouched: {}",
                summarize_names(&applied.skipped_local_mods)
            ));
        }

        if !missing_mods.is_empty() {
            let workshop_links = missing_mods
                .iter()
                .filter_map(PackMod::workshop_url)
                .count();
            let noun = if missing_mods.len() == 1 {
                "mod"
            } else {
                "mods"
            };
            let verb = if missing_mods.len() == 1 { "is" } else { "are" };

            let mut warning = format!(
                "{} {noun} from this pack {verb} not installed and {} left out of the load order: {}",
                missing_mods.len(),
                if missing_mods.len() == 1 {
                    "was"
                } else {
                    "were"
                },
                summarize_mods(&missing_mods)
            );

            if workshop_links > 0 {
                warning.push_str(
                    ". Configure a Steam Web API key or SteamCMD in Settings to include missing Workshop mods automatically.",
                );
            }

            warnings.push(warning);
        }

        if warnings.is_empty() {
            self.toasts.success(format!(
                "Applied \"{}\" ({} mods enabled)",
                applied.manifest.name,
                enabled_package_ids.len()
            ));
            None
        } else {
            Some(PackResultDialog {
                title: "Modpack applied with warnings".to_owned(),
                success: true,
                lines,
                warnings,
            })
        }
    }

    pub(super) fn show_pack_progress_dialog(&mut self, context: &egui::Context) {
        if let Some(busy) = self.modpacks.busy.as_ref() {
            let title = busy.title.clone();
            let step = busy
                .label
                .lock()
                .ok()
                .map(|label| label.clone())
                .unwrap_or_default();
            let progress = busy.progress.load(Ordering::Relaxed) as f32 / 100.0;

            // The custom bar has no built-in animation, so keep the modal
            // repainting while the worker updates its counters.
            context.request_repaint_after(Duration::from_millis(100));

            // Long pack operations are intentionally not dismissible. The
            // modal is the single place where the user sees what is happening.
            let _response =
                egui::Modal::new(egui::Id::new("modpack_progress")).show(context, |ui| {
                    ui.set_width(500.0);

                    ui.horizontal(|ui| {
                        ui::theme::chip(ui, "Working", ui::theme::ACCENT);
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(title)
                                .text_style(ui::theme::title_style())
                                .color(ui::theme::TEXT),
                        );
                    });

                    ui.add_space(12.0);

                    ui::theme::group().show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(step.as_str()).color(ui::theme::TEXT_MUTED),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0}%", progress * 100.0))
                                            .text_style(ui::theme::strong_style())
                                            .color(ui::theme::TEXT),
                                    );
                                },
                            );
                        });

                        ui.add_space(8.0);
                        ui::theme::progress_bar(ui, progress);
                    });

                    ui.add_space(8.0);
                    ui::theme::hint(ui, "Large packs and Workshop downloads can take a while.");
                });

            return;
        }

        let Some(result) = self.modpacks.result.as_ref() else {
            return;
        };

        let title = result.title.clone();
        let success = result.success;
        let lines = result.lines.clone();
        let warnings = result.warnings.clone();
        let status = if !success {
            "Failed"
        } else if warnings.is_empty() {
            "Success"
        } else {
            "Warnings"
        };
        let status_color = if !success {
            ui::theme::DANGER
        } else if warnings.is_empty() {
            ui::theme::SUCCESS
        } else {
            ui::theme::WARNING
        };
        let mut close = false;

        let response = egui::Modal::new(egui::Id::new("modpack_result")).show(context, |ui| {
            ui.set_width(500.0);

            ui.horizontal(|ui| {
                ui::theme::chip(ui, status, status_color);
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(title)
                        .text_style(ui::theme::title_style())
                        .color(ui::theme::TEXT),
                );
            });

            ui.add_space(12.0);

            ui::theme::group().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 4.0;

                for line in &lines {
                    ui.label(egui::RichText::new(line).color(ui::theme::TEXT));
                }
            });

            if !warnings.is_empty() {
                ui.add_space(12.0);

                egui::Frame::new()
                    .fill(ui::theme::tint(ui::theme::WARNING, 22))
                    .corner_radius(egui::CornerRadius::same(ui::theme::RADIUS_MD))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        egui::ScrollArea::vertical()
                            .id_salt("modpack_result_warnings")
                            .max_height(220.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for warning in &warnings {
                                    ui.label(
                                        egui::RichText::new(warning).color(ui::theme::WARNING),
                                    );
                                }
                            });
                    });
            }

            ui.add_space(16.0);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::theme::primary_button(ui, "Done").clicked() {
                    close = true;
                }
            });
        });

        if close || response.should_close() {
            self.modpacks.result = None;
        }
    }

    fn delete_library_pack(&mut self, path: &Path) {
        let library = modpack::library_directory();

        if !library
            .as_deref()
            .is_some_and(|library| path.starts_with(library))
        {
            self.report_action_error("That modpack is not in the library");
            return;
        }

        let name = self
            .modpacks
            .packs
            .iter()
            .find(|pack| paths_equal(&pack.path, path))
            .map(|pack| pack.manifest.name.clone())
            .unwrap_or_else(|| "modpack".to_owned());

        if let Err(error) = fs::remove_dir_all(path) {
            self.report_action_error(format!("Could not delete modpack: {error}"));
            return;
        }

        if self
            .modpacks
            .selected
            .as_ref()
            .is_some_and(|selected| paths_equal(selected, path))
        {
            self.modpacks.selected = None;
        }

        if self
            .modpacks
            .active_pack
            .as_ref()
            .is_some_and(|active| paths_equal(active, path))
        {
            self.modpacks.active_pack = None;
        }

        self.refresh_pack_library();
        self.clear_action_error();
        self.toasts.success(format!("Deleted modpack \"{name}\""));
    }

    pub(super) fn show_pack_apply_confirmation(&mut self, context: &egui::Context) {
        let Some(pending) = self.modpacks.pending_apply.as_mut() else {
            return;
        };

        let path = pending.path.clone();
        let mut options = pending.options;

        let Some(pack) = self
            .modpacks
            .packs
            .iter()
            .find(|pack| paths_equal(&pack.path, &path))
        else {
            self.modpacks.pending_apply = None;
            return;
        };

        let diff = modpack::compute_pack_diff(&pack.manifest, &self.mods);
        let api_key_configured = self.settings.steam_web_api_key().is_some();
        let steamcmd_available = workshop::steamcmd_available(self.settings.steamcmd_path());

        let decision = ui::modpacks::show_apply_diff_modal(
            context,
            ApplyDiffView {
                pack,
                diff: &diff,
                game_version: self.game_version.as_deref(),
                api_key_configured,
                steamcmd_available,
                options: &mut options,
            },
        );

        if let Some(pending) = self.modpacks.pending_apply.as_mut() {
            pending.options = options;
        }

        match decision {
            Some(ApplyDiffDecision::Apply) => {
                self.modpacks.pending_apply = None;
                self.start_apply_pack(path, options, context);
            }
            Some(ApplyDiffDecision::Cancel) => {
                self.modpacks.pending_apply = None;
            }
            None => {}
        }
    }

    pub(super) fn show_pack_delete_confirmation(&mut self, context: &egui::Context) {
        let Some(path) = self.modpacks.pending_delete.clone() else {
            return;
        };

        let name = self
            .modpacks
            .packs
            .iter()
            .find(|pack| paths_equal(&pack.path, &path))
            .map_or_else(|| "modpack".to_owned(), |pack| pack.manifest.name.clone());

        let mut delete = false;
        let mut cancel = false;

        let response =
            egui::Modal::new(egui::Id::new("delete_modpack")).show(context, |ui| {
                ui.set_width(420.0);
                ui.heading("Delete modpack?");
                ui.add_space(8.0);
                ui.label(format!(
                    "\"{name}\" will be removed from the modpack library. Exported copies are not affected."
                ));
                ui.add_space(14.0);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::theme::danger_button(ui, "Delete").clicked() {
                        delete = true;
                    }

                    if ui::theme::secondary_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if response.should_close() {
            cancel = true;
        }

        if delete {
            self.modpacks.pending_delete = None;
            self.delete_library_pack(&path);
        } else if cancel {
            self.modpacks.pending_delete = None;
        }
    }
}

fn unique_folder_name(raw: &str, used: &mut HashSet<String>) -> String {
    let base = modpack::sanitize_component(raw);
    let mut candidate = base.clone();
    let mut number = 2;

    while !used.insert(candidate.clone()) {
        candidate = format!("{base}-{number}");
        number += 1;
    }

    candidate
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

fn summarize_names(names: &[String]) -> String {
    let mut summary = names.iter().take(4).cloned().collect::<Vec<_>>().join(", ");

    if names.len() > 4 {
        summary.push_str(&format!(" +{} more", names.len() - 4));
    }

    summary
}

fn summarize_mods(mods: &[PackMod]) -> String {
    let mut summary = mods
        .iter()
        .take(5)
        .map(|pack_mod| pack_mod.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    if mods.len() > 5 {
        summary.push_str(&format!(" +{} more", mods.len() - 5));
    }

    summary
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
