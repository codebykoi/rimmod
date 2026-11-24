use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;

use crate::services::updater::{self, AvailableUpdate};
use crate::ui::OpenTarget;

use super::App;

pub(super) fn open_update_release_action() -> egui::Id {
    egui::Id::new("open_update_release")
}

#[derive(Default)]
pub(super) struct UpdateState {
    check_receiver: Option<Receiver<Result<Option<AvailableUpdate>, String>>>,
    pub(super) available: Option<AvailableUpdate>,
    pub(super) launching: bool,
}

impl App {
    /// Silently check GitHub Releases once on startup. Failures are ignored so
    /// offline use or API rate limits never intrude on the user.
    pub(super) fn start_update_check(&mut self, context: &egui::Context) {
        if self.update.check_receiver.is_some() || self.update.available.is_some() {
            return;
        }

        let context = context.clone();
        let (sender, receiver) = mpsc::channel();

        self.update.check_receiver = Some(receiver);

        std::thread::spawn(move || {
            let result = updater::check_for_update();
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    pub(super) fn poll_update_check(&mut self) {
        let outcome = self
            .update
            .check_receiver
            .as_ref()
            .map(|receiver| receiver.try_recv());

        match outcome {
            Some(Ok(Ok(available))) => {
                self.update.available = available;
                self.update.check_receiver = None;
            }
            Some(Ok(Err(_))) | Some(Err(TryRecvError::Disconnected)) => {
                self.update.check_receiver = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
    }

    pub(super) fn start_update_install(&mut self, context: &egui::Context) {
        if self.update.launching {
            return;
        }

        let Some(update) = self.update.available.clone() else {
            return;
        };

        match updater::launch_updater(&update) {
            Ok(()) => {
                self.update.launching = true;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(error) => {
                self.toasts
                    .error(format!("Could not start update: {error}"))
                    .click_action(open_update_release_action());
            }
        }
    }

    pub(super) fn open_update_release_page(&mut self) {
        if let Some(update) = self.update.available.as_ref() {
            self.open_target(OpenTarget::Url(update.release_url.clone()));
        }
    }
}
