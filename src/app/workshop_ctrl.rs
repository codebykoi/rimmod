use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;

use crate::services::workshop::{self, WorkshopItem, WorkshopPage, WorkshopSort};
use crate::ui::workshop::WorkshopLoadStatus;

use super::App;

pub(super) struct WorkshopState {
    pub(super) query: String,
    pub(super) sort: WorkshopSort,
    pub(super) page: u32,
    pub(super) total: u64,
    pub(super) items: Vec<WorkshopItem>,
    pub(super) load_status: WorkshopLoadStatus,
    pub(super) pending_page: Option<u32>,
    pub(super) query_receiver: Option<Receiver<Result<WorkshopPage, String>>>,
    pub(super) installing_item: Option<u64>,
    pub(super) install_receiver: Option<Receiver<(u64, io::Result<()>)>>,
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
            installing_item: None,
            install_receiver: None,
        }
    }
}

impl App {
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
        published_file_id: u64,
        context: &egui::Context,
    ) {
        if self.workshop.installing_item.is_some() {
            return;
        }

        let steamcmd_path = self
            .settings
            .steamcmd_path()
            .map(Path::to_path_buf)
            .unwrap_or_else(workshop::default_steamcmd_path);
        let context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.workshop.installing_item = Some(published_file_id);
        self.workshop.install_receiver = Some(receiver);
        self.toasts
            .info(format!("Installing Workshop item {published_file_id}..."));

        std::thread::spawn(move || {
            let result = workshop::install_workshop_item(&steamcmd_path, published_file_id);
            let _ = sender.send((published_file_id, result));
            context.request_repaint();
        });
    }

    pub(super) fn poll_workshop_tasks(&mut self, context: &egui::Context) {
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
            Some(Ok((published_file_id, Ok(())))) => {
                self.workshop.installing_item = None;
                self.workshop.install_receiver = None;
                self.reload_mods();
                self.restart_mod_watcher(context);

                let steamcmd_workshop_path =
                    workshop::steamcmd_workshop_path(self.settings.steamcmd_path());
                if workshop::is_item_installed(
                    self.settings.workshop_path(),
                    steamcmd_workshop_path.as_deref(),
                    published_file_id,
                ) {
                    self.clear_action_error();
                    self.toasts.success(format!(
                        "Workshop item {published_file_id} installed and added to the Mods list"
                    ));
                } else {
                    self.toasts.warning(format!(
                        "SteamCMD finished, but item {published_file_id} was not found in the configured Workshop folder"
                    ));
                }
            }
            Some(Ok((published_file_id, Err(error)))) => {
                self.workshop.installing_item = None;
                self.workshop.install_receiver = None;
                self.report_action_error(format!(
                    "Could not install Workshop item {published_file_id}: {error}"
                ));
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.workshop.installing_item = None;
                self.workshop.install_receiver = None;
                self.report_action_error("The SteamCMD worker stopped unexpectedly");
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
    }
}
