use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;

use crate::models::PackageId;
use crate::services::log_analysis::{self, LogReport, ModSnapshot};
use crate::services::log_parser::{self, LogFileInfo, LogLevel, LogSession};
use crate::ui::logs::{LogAction, LogFilter, LogModFilter};

use super::{App, AppTab};

const LOG_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum LogLoadStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

struct LogLoadOutcome {
    session: LogSession,
    path: PathBuf,
    result: Result<LogReport, String>,
    size: u64,
    modified: Option<SystemTime>,
}

pub(super) struct LogState {
    pub(super) session: LogSession,
    pub(super) sessions: Vec<LogFileInfo>,
    pub(super) status: LogLoadStatus,
    pub(super) report: Option<LogReport>,
    pub(super) filter: LogFilter,
    pub(super) selected_issue: Option<usize>,
    pub(super) auto_refresh: bool,
    pub(super) active_path: Option<PathBuf>,
    receiver: Option<Receiver<LogLoadOutcome>>,
    loaded_size: u64,
    loaded_modified: Option<SystemTime>,
    next_refresh: Option<Instant>,
    started: bool,
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            session: LogSession::Current,
            sessions: Vec::new(),
            status: LogLoadStatus::Idle,
            report: None,
            filter: LogFilter::default(),
            selected_issue: None,
            auto_refresh: true,
            active_path: None,
            receiver: None,
            loaded_size: 0,
            loaded_modified: None,
            next_refresh: None,
            started: false,
        }
    }
}

impl LogState {
    pub(super) fn invalidate(&mut self) {
        self.receiver = None;
        self.sessions.clear();
        self.report = None;
        self.selected_issue = None;
        self.active_path = None;
        self.status = LogLoadStatus::Idle;
        self.loaded_size = 0;
        self.loaded_modified = None;
        self.next_refresh = None;
        self.started = false;
        self.session = LogSession::Current;
        self.filter.mod_filter = LogModFilter::All;
    }
}

impl App {
    pub(super) fn ensure_logs_started(&mut self, context: &egui::Context) {
        if self.logs.started {
            return;
        }

        self.logs.started = true;
        self.refresh_log_sessions();

        if self.logs.sessions.is_empty() {
            self.logs.status = LogLoadStatus::Idle;
            return;
        }

        if !self
            .logs
            .sessions
            .iter()
            .any(|file| file.session == self.logs.session)
        {
            self.logs.session = self.logs.sessions[0].session;
        }

        self.start_log_load(context);
    }

    fn refresh_log_sessions(&mut self) {
        self.logs.sessions = self
            .settings
            .config_path()
            .map(log_parser::discover_log_files)
            .unwrap_or_default();
    }

    pub(super) fn start_log_load(&mut self, context: &egui::Context) {
        self.logs.receiver = None;
        self.refresh_log_sessions();

        let Some(file) = self
            .logs
            .sessions
            .iter()
            .find(|file| file.session == self.logs.session)
            .cloned()
        else {
            self.logs.report = None;
            self.logs.selected_issue = None;
            self.logs.active_path = None;
            self.logs.status = LogLoadStatus::Failed(
                "No Player.log was found for this session. Start RimWorld once and try again."
                    .to_owned(),
            );
            return;
        };

        if self.logs.active_path.as_ref() != Some(&file.path) {
            self.logs.report = None;
            self.logs.selected_issue = None;
        }

        self.logs.active_path = Some(file.path.clone());
        self.logs.status = LogLoadStatus::Loading;
        self.logs.next_refresh = None;

        let session = file.session;
        let path = file.path;
        let mods = ModSnapshot::collect(&self.mods);
        let (sender, receiver) = mpsc::channel();
        let repaint_context = context.clone();

        thread::spawn(move || {
            let metadata = fs::metadata(&path).ok();
            let size = metadata.as_ref().map_or(0, |metadata| metadata.len());
            let modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok());

            let result = log_parser::parse_log_file(&path)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))
                .map(|document| log_analysis::analyze(document, &mods));

            let _ = sender.send(LogLoadOutcome {
                session,
                path,
                result,
                size,
                modified,
            });

            repaint_context.request_repaint();
        });

        self.logs.receiver = Some(receiver);
    }

    pub(super) fn poll_log_tasks(&mut self, context: &egui::Context) {
        if let Some(receiver) = self.logs.receiver.as_ref() {
            match receiver.try_recv() {
                Ok(outcome) => {
                    self.logs.receiver = None;

                    if outcome.session != self.logs.session {
                        return;
                    }

                    self.logs.active_path = Some(outcome.path);
                    self.logs.loaded_size = outcome.size;
                    self.logs.loaded_modified = outcome.modified;

                    match outcome.result {
                        Ok(report) => {
                            let first_issue = report
                                .issues
                                .iter()
                                .enumerate()
                                .filter(|(_, issue)| issue.level >= LogLevel::Warning)
                                .max_by_key(|(_, issue)| {
                                    (!issue.suspects.is_empty(), issue.level, issue.last_index)
                                })
                                .map(|(index, _)| index)
                                .or_else(|| (!report.issues.is_empty()).then_some(0));

                            self.logs.report = Some(report);
                            self.logs.selected_issue = first_issue;
                            self.logs.status = LogLoadStatus::Ready;
                        }

                        Err(error) => {
                            self.logs.status = LogLoadStatus::Failed(error);
                        }
                    }
                }

                Err(TryRecvError::Empty) => {}

                Err(TryRecvError::Disconnected) => {
                    self.logs.receiver = None;
                    self.logs.status = LogLoadStatus::Failed(
                        "The log reader stopped before it could finish".to_owned(),
                    );
                }
            }
        }

        self.maybe_refresh_log(context);
    }

    fn maybe_refresh_log(&mut self, context: &egui::Context) {
        if !self.logs.auto_refresh || self.current_tab != AppTab::Log {
            return;
        }

        // The game can append to the log without any input event reaching us,
        // so schedule the next poll instead of waiting for a repaint.
        context.request_repaint_after(LOG_REFRESH_INTERVAL);

        if matches!(self.logs.status, LogLoadStatus::Loading) {
            return;
        }

        let now = Instant::now();

        if self.logs.next_refresh.is_some_and(|next| now < next) {
            return;
        }

        self.logs.next_refresh = Some(now + LOG_REFRESH_INTERVAL);

        if self.logs.active_path.is_none() {
            self.refresh_log_sessions();

            if let Some(file) = self.logs.sessions.first() {
                self.logs.session = file.session;
                self.start_log_load(context);
            }

            return;
        }

        let Some(path) = self.logs.active_path.as_ref() else {
            return;
        };

        let Ok(metadata) = fs::metadata(path) else {
            self.refresh_log_sessions();

            if self.logs.sessions.is_empty() {
                self.logs.report = None;
                self.logs.selected_issue = None;
                self.logs.active_path = None;
                self.logs.status = LogLoadStatus::Failed(
                    "The log file was removed. Start RimWorld once and try again.".to_owned(),
                );
            }

            return;
        };

        let changed = metadata.len() != self.logs.loaded_size
            || metadata.modified().ok() != self.logs.loaded_modified;

        if changed {
            self.start_log_load(context);
        }
    }

    pub(super) fn apply_log_action(&mut self, action: LogAction, context: &egui::Context) {
        match action {
            LogAction::Reload => self.start_log_load(context),

            LogAction::SelectSession(session) => {
                if self.logs.session != session {
                    self.logs.session = session;
                    self.logs.report = None;
                    self.logs.selected_issue = None;
                    self.start_log_load(context);
                }
            }

            LogAction::SelectMod(package_id) => {
                if self.focus_mod_by_package_id(&package_id) {
                    self.current_tab = AppTab::Mods;
                } else {
                    self.report_action_error(format!(
                        "Mod {} is no longer installed",
                        package_id.as_str()
                    ));
                }
            }

            LogAction::SetModEnabled {
                package_id,
                enabled,
            } => {
                self.set_mod_enabled_from_log(&package_id, enabled);
            }

            LogAction::Open(target) => self.open_target(target),
        }
    }

    fn set_mod_enabled_from_log(&mut self, package_id: &PackageId, enabled: bool) {
        let Some(mod_id) = self.mods.find_id_by_package_id(package_id) else {
            self.report_action_error(format!(
                "Mod {} is no longer installed",
                package_id.as_str()
            ));
            return;
        };

        let is_enabled = self.mods.enabled_ids().contains(&mod_id);

        if is_enabled == enabled {
            return;
        }

        let name = self
            .mods
            .get(mod_id)
            .map(|rimworld_mod| rimworld_mod.name.clone())
            .unwrap_or_else(|| package_id.as_str().to_owned());

        let previous = self.load_order_snapshot();
        let result = if enabled {
            self.mods.enable_many(&[mod_id], None)
        } else {
            self.mods.disable_many(&[mod_id], None)
        };

        match result {
            Ok(()) => {
                self.clear_action_error();
                self.record_load_order_change(previous);
                self.focus_mod_by_package_id(package_id);

                if let Some(report) = &mut self.logs.report {
                    for issue in &mut report.issues {
                        for suspect in &mut issue.suspects {
                            if &suspect.package_id == package_id {
                                suspect.enabled = enabled;
                            }
                        }
                    }

                    for suspect in &mut report.suspects {
                        if &suspect.package_id == package_id {
                            suspect.enabled = enabled;
                        }
                    }
                }

                self.toasts.success(format!(
                    "{} {name}. Save the load order to keep this change.",
                    if enabled { "Enabled" } else { "Disabled" }
                ));
            }

            Err(error) => {
                self.report_action_error(format!("Could not update the mod list: {error}"));
            }
        }
    }
}
