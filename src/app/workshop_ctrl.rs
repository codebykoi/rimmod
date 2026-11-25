use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use eframe::egui;

use crate::models::major_minor_version_text;
use crate::services::no_version_warning::{self, CoveredMods};
use crate::services::workshop::{
    self, InstallOutcome, SubscribedSort, SubscriptionSnapshot, WorkshopItem, WorkshopPage,
    WorkshopQuery, WorkshopSort,
};
use crate::ui::workshop::{WorkshopImageViewer, WorkshopLoadStatus, WorkshopTab};

use super::App;

/// Background fetch state of the No Version Warning community reports.
pub(super) enum CoveredModsStatus {
    NotRequested,
    Loading(Receiver<Result<CoveredMods, String>>),
    Ready(CoveredMods),
    Failed(String),
}

pub(super) enum SubscriptionStatus {
    NotLoaded,
    Loading,
    Ready(HashSet<u64>),
    Failed(String),
}

impl SubscriptionStatus {
    pub(super) fn ids(&self) -> Option<&HashSet<u64>> {
        match self {
            Self::Ready(ids) => Some(ids),
            _ => None,
        }
    }

    pub(super) fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(error) => Some(error),
            _ => None,
        }
    }
}

enum SubscriptionTask {
    Changed {
        published_file_id: u64,
        subscribed: bool,
        error: Option<String>,
    },
}

pub(super) struct WorkshopState {
    pub(super) query: String,
    /// The text the current browse results were fetched with.
    pub(super) applied_query: String,
    pub(super) subscribed_query: String,
    pub(super) sort: WorkshopSort,
    pub(super) subscribed_sort: SubscribedSort,
    pub(super) versions: Vec<String>,
    pub(super) dlc: Vec<String>,
    pub(super) my_version_only: bool,
    pub(super) hide_subscribed: bool,
    pub(super) not_installed_only: bool,
    pub(super) tab: WorkshopTab,
    pub(super) page: u32,
    pub(super) subscribed_page: u32,
    pub(super) total: u64,
    pub(super) items: Vec<WorkshopItem>,
    pub(super) load_status: WorkshopLoadStatus,
    pub(super) pending_page: Option<u32>,
    pub(super) query_receiver: Option<Receiver<Result<WorkshopPage, String>>>,
    pub(super) installing_items: Vec<u64>,
    pub(super) install_receiver: Option<Receiver<Vec<InstallOutcome>>>,
    pub(super) covered_mods: CoveredModsStatus,
    pub(super) subscriptions: SubscriptionStatus,
    pub(super) subscribed_items: Vec<WorkshopItem>,
    pub(super) subscribed_load: WorkshopLoadStatus,
    subscription_load_receiver: Option<Receiver<Result<SubscriptionSnapshot, String>>>,
    subscription_change_sender: Sender<SubscriptionTask>,
    subscription_change_receiver: Receiver<SubscriptionTask>,
    pub(super) pending_subscriptions: HashSet<u64>,
    pub(super) detail: Option<WorkshopItem>,
    /// Active image in the in-app lightbox (either a preview gallery index or
    /// a standalone description image URL).
    pub(super) detail_image: Option<WorkshopImageViewer>,
    pub(super) pending_search: bool,
    pub(super) grid_generation: u64,
    pub(super) subscribed_generation: u64,
    pub(super) started: bool,
}

impl Default for WorkshopState {
    fn default() -> Self {
        let (subscription_change_sender, subscription_change_receiver) = mpsc::channel();

        Self {
            query: String::new(),
            applied_query: String::new(),
            subscribed_query: String::new(),
            sort: WorkshopSort::default(),
            subscribed_sort: SubscribedSort::default(),
            versions: Vec::new(),
            dlc: Vec::new(),
            my_version_only: false,
            hide_subscribed: false,
            not_installed_only: false,
            tab: WorkshopTab::Browse,
            page: 1,
            subscribed_page: 1,
            total: 0,
            items: Vec::new(),
            load_status: WorkshopLoadStatus::default(),
            pending_page: None,
            query_receiver: None,
            installing_items: Vec::new(),
            install_receiver: None,
            covered_mods: CoveredModsStatus::NotRequested,
            subscriptions: SubscriptionStatus::NotLoaded,
            subscribed_items: Vec::new(),
            subscribed_load: WorkshopLoadStatus::default(),
            subscription_load_receiver: None,
            subscription_change_sender,
            subscription_change_receiver,
            pending_subscriptions: HashSet::new(),
            detail: None,
            detail_image: None,
            pending_search: false,
            grid_generation: 0,
            subscribed_generation: 0,
            started: false,
        }
    }
}

impl App {
    /// Load the Workshop once per session: an initial browse page (when a key
    /// is available) and the account's subscriptions.
    pub(super) fn ensure_workshop_started(&mut self, context: &egui::Context) {
        if self.workshop.started {
            return;
        }

        self.workshop.started = true;

        if self.settings.steam_web_api_key().is_some() {
            self.start_workshop_query(1, context);
        }

        self.ensure_subscriptions_started(context);
    }

    /// A settings change can add a Web API key or point at another Workshop
    /// folder, so the Workshop reloads its data on the next visit.
    pub(super) fn reset_workshop_data(&mut self) {
        self.workshop.started = false;
        self.workshop.covered_mods = CoveredModsStatus::NotRequested;
        self.workshop.subscriptions = SubscriptionStatus::NotLoaded;
        self.workshop.subscribed_items.clear();
        self.workshop.subscribed_load = WorkshopLoadStatus::Idle;
        self.workshop.load_status = WorkshopLoadStatus::Idle;
        self.workshop.items.clear();
        self.workshop.total = 0;
        self.workshop.page = 1;
        self.workshop.subscribed_page = 1;
        self.workshop.detail = None;
        self.workshop.detail_image = None;
    }

    /// Fetch the No Version Warning community reports once per session so
    /// un-installed Workshop items can show their community compatibility.
    pub(super) fn ensure_covered_mods(&mut self, context: &egui::Context) {
        if !matches!(self.workshop.covered_mods, CoveredModsStatus::NotRequested) {
            return;
        }

        let context = context.clone();
        let game_version = self.game_version.clone();
        let (sender, receiver) = mpsc::channel();

        self.workshop.covered_mods = CoveredModsStatus::Loading(receiver);

        std::thread::spawn(move || {
            let result = no_version_warning::fetch_covered_mods(game_version.as_deref());
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

    fn workshop_query(&self, page: u32) -> WorkshopQuery {
        let mut versions = self.workshop.versions.clone();

        if self.workshop.my_version_only
            && let Some(version) = self
                .game_version
                .as_deref()
                .and_then(major_minor_version_text)
            && !versions.contains(&version)
        {
            versions.push(version);
        }

        WorkshopQuery {
            search_text: self.workshop.applied_query.clone(),
            sort: self.workshop.sort,
            page: page.max(1),
            versions,
            dlc: self.workshop.dlc.clone(),
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

        let query = self.workshop_query(page);
        let context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.workshop.load_status = WorkshopLoadStatus::Loading;
        self.workshop.pending_page = Some(query.page);
        self.workshop.query_receiver = Some(receiver);

        std::thread::spawn(move || {
            let result = workshop::query_workshop(&api_key, &query);
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    /// Start a fresh search page. Changes made while a request is in flight
    /// are coalesced into one follow-up request instead of being dropped.
    pub(super) fn search_workshop(&mut self, context: &egui::Context) {
        self.workshop.applied_query = self.workshop.query.clone();

        if self.workshop.query.trim().is_empty() && self.workshop.sort == WorkshopSort::Relevance {
            self.workshop.sort = WorkshopSort::Trending;
        }

        if matches!(self.workshop.load_status, WorkshopLoadStatus::Loading) {
            self.workshop.pending_search = true;
            return;
        }

        self.start_workshop_query(1, context);
    }

    /// Load the account's subscriptions as soon as the Mods tab needs them,
    /// so its rows can tell subscribed Workshop mods apart.
    pub(super) fn ensure_subscriptions_started(&mut self, context: &egui::Context) {
        if matches!(self.workshop.subscriptions, SubscriptionStatus::NotLoaded) {
            self.start_subscription_load(context);
        }
    }

    pub(super) fn start_subscription_load(&mut self, context: &egui::Context) {
        if matches!(self.workshop.subscriptions, SubscriptionStatus::Loading) {
            return;
        }

        let api_key = self.settings.steam_web_api_key().map(str::to_owned);
        let workshop_path = self.settings.workshop_path().map(Path::to_path_buf);
        let context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.workshop.subscriptions = SubscriptionStatus::Loading;
        self.workshop.subscribed_load = WorkshopLoadStatus::Loading;
        self.workshop.subscription_load_receiver = Some(receiver);

        std::thread::spawn(move || {
            let result = workshop::load_subscriptions(api_key.as_deref(), workshop_path.as_deref());
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    pub(super) fn set_subscription(
        &mut self,
        published_file_id: u64,
        subscribed: bool,
        context: &egui::Context,
    ) {
        if self
            .workshop
            .pending_subscriptions
            .contains(&published_file_id)
        {
            return;
        }

        let Some(api_key) = self.settings.steam_web_api_key().map(str::to_owned) else {
            self.report_action_error(
                "Add a Steam Web API key in Settings to subscribe or unsubscribe",
            );
            return;
        };

        let context = context.clone();
        let sender = self.workshop.subscription_change_sender.clone();

        self.workshop
            .pending_subscriptions
            .insert(published_file_id);

        std::thread::spawn(move || {
            let result = workshop::set_subscription(&api_key, published_file_id, subscribed);
            let _ = sender.send(SubscriptionTask::Changed {
                published_file_id,
                subscribed,
                error: result.err(),
            });
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

    fn apply_subscription_snapshot(&mut self, snapshot: SubscriptionSnapshot) {
        self.workshop.subscriptions = SubscriptionStatus::Ready(snapshot.ids);
        self.workshop.subscribed_items = snapshot.items;
        self.workshop.subscribed_page = 1;
        self.workshop.subscribed_load = WorkshopLoadStatus::Ready;
        self.workshop.subscribed_generation = self.workshop.subscribed_generation.wrapping_add(1);
    }

    fn apply_subscription_change(
        &mut self,
        published_file_id: u64,
        subscribed: bool,
        error: Option<String>,
    ) {
        self.workshop
            .pending_subscriptions
            .remove(&published_file_id);

        let Some(error) = error else {
            let title = self
                .workshop
                .detail
                .as_ref()
                .filter(|item| item.published_file_id == published_file_id)
                .or_else(|| {
                    self.workshop
                        .items
                        .iter()
                        .find(|item| item.published_file_id == published_file_id)
                })
                .or_else(|| {
                    self.workshop
                        .subscribed_items
                        .iter()
                        .find(|item| item.published_file_id == published_file_id)
                })
                .map(|item| item.title.clone());

            if let SubscriptionStatus::Ready(ids) = &mut self.workshop.subscriptions {
                if subscribed {
                    ids.insert(published_file_id);
                } else {
                    ids.remove(&published_file_id);
                }
            } else if subscribed {
                // The full list could not be loaded, but the account change
                // succeeded, so at least this item is known to be subscribed.
                self.workshop.subscriptions =
                    SubscriptionStatus::Ready(HashSet::from([published_file_id]));
            }

            if subscribed {
                if let Some(item) = self
                    .workshop
                    .detail
                    .clone()
                    .filter(|item| item.published_file_id == published_file_id)
                    .or_else(|| {
                        self.workshop
                            .items
                            .iter()
                            .find(|item| item.published_file_id == published_file_id)
                            .cloned()
                    })
                    && !self
                        .workshop
                        .subscribed_items
                        .iter()
                        .any(|item| item.published_file_id == published_file_id)
                {
                    self.workshop.subscribed_items.push(item);
                }
            } else {
                self.workshop
                    .subscribed_items
                    .retain(|item| item.published_file_id != published_file_id);
            }

            let name = title.unwrap_or_else(|| format!("Workshop item {published_file_id}"));

            if subscribed {
                self.toasts.success(format!(
                    "Subscribed to {name}. Steam downloads it the next time RimWorld starts."
                ));
            } else {
                self.toasts.success(format!(
                    "Unsubscribed from {name}. Steam removes its files after RimWorld closes."
                ));
            }

            self.workshop.subscribed_load = WorkshopLoadStatus::Ready;
            return;
        };

        self.report_action_error(format!(
            "Could not change the subscription for Workshop item {published_file_id}: {error}"
        ));
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
                self.workshop.grid_generation = self.workshop.grid_generation.wrapping_add(1);
            }
            Some(Ok(Err(error))) => {
                self.workshop.pending_page = None;
                self.workshop.load_status = WorkshopLoadStatus::Error(error.clone());
                self.workshop.query_receiver = None;
                self.toasts
                    .error(format!("Workshop search failed: {error}"));
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

        if !matches!(self.workshop.load_status, WorkshopLoadStatus::Loading)
            && std::mem::take(&mut self.workshop.pending_search)
        {
            self.start_workshop_query(1, context);
        }

        let subscription_load_result = self
            .workshop
            .subscription_load_receiver
            .as_ref()
            .map(|receiver| receiver.try_recv());

        match subscription_load_result {
            Some(Ok(Ok(snapshot))) => {
                self.workshop.subscription_load_receiver = None;
                self.apply_subscription_snapshot(snapshot);
            }
            Some(Ok(Err(error))) => {
                self.workshop.subscription_load_receiver = None;
                self.workshop.subscribed_load = WorkshopLoadStatus::Error(error.clone());
                self.workshop.subscriptions = SubscriptionStatus::Failed(error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.workshop.subscription_load_receiver = None;
                let error = "The subscription worker stopped unexpectedly".to_owned();
                self.workshop.subscribed_load = WorkshopLoadStatus::Error(error.clone());
                self.workshop.subscriptions = SubscriptionStatus::Failed(error);
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        // Subscription changes can overlap (one thread per card click), so
        // drain everything that finished since the last frame.
        let mut subscription_changes = Vec::new();
        loop {
            match self.workshop.subscription_change_receiver.try_recv() {
                Ok(task) => subscription_changes.push(task),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.workshop.pending_subscriptions.clear();
                    self.report_action_error("The subscription worker stopped unexpectedly");
                    break;
                }
            }
        }

        for SubscriptionTask::Changed {
            published_file_id,
            subscribed,
            error,
        } in subscription_changes
        {
            self.apply_subscription_change(published_file_id, subscribed, error);
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
