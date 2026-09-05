use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;

use crate::services::no_version_warning::{self, CoveredMods};
use crate::services::workshop::{self, InstallOutcome, WorkshopItem, WorkshopPage, WorkshopSort};
use crate::ui::workshop::WorkshopLoadStatus;

use super::App;

/// Background fetch state of the No Version Warning community reports.
pub(super) enum CoveredModsStatus {
    NotRequested,
    Loading(Receiver<Result<CoveredMods, String>>),
    Ready(CoveredMods),
    Failed(String),
}

pub(super) struct WorkshopState {
    pub(super) query: String,
    pub(super) sort: WorkshopSort,
    pub(super) page: u32,
    pub(super) total: u64,
    pub(super) items: Vec<WorkshopItem>,
    pub(super) load_status: WorkshopLoadStatus,
    pub(super) pending_page: Option<u32>,
    pub(super) query_receiver: Option<Receiver<Result<WorkshopPage, String>>>,
    pub(super) selected_items: HashSet<u64>,
    pub(super) installing_items: Vec<u64>,
    pub(super) install_receiver: Option<Receiver<Vec<InstallOutcome>>>,
    pub(super) covered_mods: CoveredModsStatus,
}

impl Default for WorkshopState {
    fn default() -> Self {
        Self {
            query: String::new(),
            sort: WorkshopSort::default(),
            page: 1,
            total: 0,
            items: Vec::new(),
            load_status: WorkshopLoadStatus::default(),
            pending_page: None,
            query_receiver: None,
            selected_items: HashSet::new(),
            installing_items: Vec::new(),
            install_receiver: None,
            covered_mods: CoveredModsStatus::NotRequested,
        }
    }
}

impl App {
    /// Fetch the No Version Warning community reports once per session so
    /// un-installed Workshop items can show their community compatibility.
    pub(super) fn ensure_covered_mods(&mut self, context: &egui::Context) {
        if !matches!(self.workshop.covered_mods, CoveredModsStatus::NotRequested) {
            return;
        }

        let context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.workshop.covered_mods = CoveredModsStatus::Loading(receiver);

        std::thread::spawn(move || {
            let result = no_version_warning::fetch_covered_mods();
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    pub(super) fn retry_covered_mods(&mut self, context: &egui::Context) {
        if matches!(self.workshop.covered_mods, CoveredModsStatus::Failed(_)) {
            self.workshop.covered_mods = CoveredModsStatus::NotRequested;
            self.ensure_covered_mods(context);
        }
    }

    pub(super) fn start_workshop_query(&mut self, page: u32, context: &egui::Context) {
        let Some(api_key) = self.settings.steam_web_api_key().map(str::to_owned) else {
            self.workshop.load_status = WorkshopLoadStatus::Error(
                "Add a Steam Web API key in Settings before searching".to_owned(),
            );
            return;
        };

        if matches!(self.workshop.load_status, WorkshopLoadStatus::Loading) {
            return;
        }

        let query = self.workshop.query.clone();
        let sort = self.workshop.sort;
        let page = page.max(1);
        let context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.workshop.load_status = WorkshopLoadStatus::Loading;
        self.workshop.pending_page = Some(page);
        self.workshop.query_receiver = Some(receiver);

        std::thread::spawn(move || {
            let result = workshop::query_workshop(&api_key, &query, sort, page);
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    pub(super) fn start_workshop_install(
        &mut self,
        published_file_ids: Vec<u64>,
        context: &egui::Context,
    ) {
        if !self.workshop.installing_items.is_empty() || published_file_ids.is_empty() {
            return;
        }

        let steamcmd_path = self
            .settings
            .steamcmd_path()
            .map(Path::to_path_buf)
            .unwrap_or_else(workshop::default_steamcmd_path);
        let context = context.clone();
        let (sender, receiver) = mpsc::channel();
        let installing_items = published_file_ids.clone();

        self.workshop.installing_items = installing_items;
        self.workshop.install_receiver = Some(receiver);
        let message = if published_file_ids.len() == 1 {
            format!("Installing Workshop item {}...", published_file_ids[0])
        } else {
            format!("Installing {} Workshop items...", published_file_ids.len())
        };
        self.toasts.info(message);

        std::thread::spawn(move || {
            let outcomes = workshop::install_workshop_items(&steamcmd_path, &published_file_ids)
                .unwrap_or_else(|error| {
                    let message = error.to_string();
                    published_file_ids
                        .iter()
                        .map(|&published_file_id| InstallOutcome {
                            published_file_id,
                            error: Some(message.clone()),
                        })
                        .collect()
                });
            let _ = sender.send(outcomes);
            context.request_repaint();
        });
    }

    pub(super) fn poll_workshop_tasks(&mut self, context: &egui::Context) {
        let covered_result = match &self.workshop.covered_mods {
            CoveredModsStatus::Loading(receiver) => Some(receiver.try_recv()),
            _ => None,
        };

        match covered_result {
            Some(Ok(Ok(covered_mods))) => {
                self.workshop.covered_mods = CoveredModsStatus::Ready(covered_mods);
            }
            Some(Ok(Err(error))) => {
                self.workshop.covered_mods = CoveredModsStatus::Failed(error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.workshop.covered_mods = CoveredModsStatus::Failed(
                    "the background worker stopped unexpectedly".to_owned(),
                );
            }
            // The worker is still running; check again next frame.
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let query_result = self
            .workshop
            .query_receiver
            .as_ref()
            .map(|receiver| receiver.try_recv());

        match query_result {
            Some(Ok(Ok(page))) => {
                self.workshop.page = self.workshop.pending_page.take().unwrap_or(1);
                self.workshop.total = page.total;
                self.workshop.items = page.items;
                self.workshop.load_status = WorkshopLoadStatus::Ready;
                self.workshop.query_receiver = None;
            }
            Some(Ok(Err(error))) => {
                self.workshop.pending_page = None;
                self.workshop.load_status = WorkshopLoadStatus::Error(error);
                self.workshop.query_receiver = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.workshop.pending_page = None;
                self.workshop.load_status = WorkshopLoadStatus::Error(
                    "The Workshop search worker stopped unexpectedly".to_owned(),
                );
                self.workshop.query_receiver = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let install_result = self
            .workshop
            .install_receiver
            .as_ref()
            .map(|receiver| receiver.try_recv());

        match install_result {
            Some(Ok(outcomes)) => {
                self.workshop.installing_items = Vec::new();
                self.workshop.install_receiver = None;
                for outcome in &outcomes {
                    self.workshop
                        .selected_items
                        .remove(&outcome.published_file_id);
                }
                self.reload_mods();
                self.restart_mod_watcher(context);

                let steamcmd_workshop_path =
                    workshop::steamcmd_workshop_path(self.settings.steamcmd_path());
                for outcome in outcomes {
                    match outcome.error {
                        Some(error) => self.report_action_error(format!(
                            "Could not install Workshop item {}: {error}",
                            outcome.published_file_id
                        )),
                        None => {
                            if workshop::is_item_installed(
                                self.settings.workshop_path(),
                                steamcmd_workshop_path.as_deref(),
                                outcome.published_file_id,
                            ) {
                                self.clear_action_error();
                                self.toasts.success(format!(
                                    "Workshop item {} installed and added to the Mods list",
                                    outcome.published_file_id
                                ));
                            } else {
                                self.toasts.warning(format!(
                                    "SteamCMD finished, but item {} was not found in the configured Workshop folder",
                                    outcome.published_file_id
                                ));
                            }
                        }
                    }
                }
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.workshop.installing_items = Vec::new();
                self.workshop.install_receiver = None;
                self.report_action_error("The SteamCMD worker stopped unexpectedly");
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
    }
}
