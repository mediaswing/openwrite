//! "There is a newer version" — asked once, at startup, and easy to refuse.
//!
//! The check itself is in [`crate::update`], including what it sends and how to
//! turn it off. This is only what the writer sees, and it is written to two
//! rules.
//!
//! **It is never in the way.** The question is asked on a worker thread after
//! the window is already up, so a slow network delays nothing; if there is
//! nothing newer, or the check failed, the writer never learns that it
//! happened. The dialog appears only when there is genuinely something to say.
//!
//! **It takes no for an answer.** Dismissing it dismisses it for the session,
//! and the check does not come back to ask again.

use super::{a11y, theme, App, Tone};
use crate::t;
use crate::{browser, log, update};
use eframe::egui::{self, Key, RichText};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// The startup check, and what it found.
#[derive(Default)]
pub(super) struct Update {
    /// Set once the check has been started, so it happens only once.
    asked: bool,
    events: Option<Receiver<Option<update::Release>>>,
    /// A release newer than this build, if there is one.
    found: Option<update::Release>,
    /// Whether the dialog is up. Separate from `found`, so that dismissing it
    /// does not throw away what was found.
    showing: bool,
}

impl App {
    /// Ask, once, in the background.
    pub(super) fn check_for_update(&mut self, ctx: &egui::Context) {
        if self.update.asked {
            return;
        }
        self.update.asked = true;
        if update::disabled() {
            log::info("update", "the check is turned off");
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.update.events = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = match update::check() {
                Ok(found) => found,
                Err(err) => {
                    // Not being able to check is not something to interrupt
                    // somebody's writing over. It goes in the log and nowhere
                    // else.
                    log::warn("update", err.to_string());
                    None
                }
            };
            let _ = tx.send(found);
            ctx.request_repaint();
        });
    }

    /// Collect the answer, and put the dialog up if there is one worth putting.
    fn poll_update(&mut self) {
        let Some(events) = self.update.events.as_ref() else {
            return;
        };
        match events.try_recv() {
            Ok(found) => {
                self.update.events = None;
                if let Some(release) = found {
                    self.announce(t!(
                        "status.update_available",
                        version = release.number(),
                        yours = env!("CARGO_PKG_VERSION")
                    ));
                    self.update.found = Some(release);
                    self.update.showing = true;
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.update.events = None,
        }
    }

    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) {
        self.poll_update();
        if !self.update.showing {
            return;
        }
        let Some(release) = self.update.found.clone() else {
            self.update.showing = false;
            return;
        };

        let mut download = false;
        let mut dismiss = false;
        let mut keep_open = true;

        egui::Window::new(t!("update.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let palette = theme::palette(ui.visuals());
                let headline = t!("update.headline", version = release.number());
                let response = ui.label(RichText::new(&headline).heading());
                a11y::name(&response, &headline);
                a11y::live_region(&response);

                ui.label(
                    RichText::new(t!("update.yours", version = env!("CARGO_PKG_VERSION")))
                        .color(palette.muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let button = ui.button(t!("update.download"));
                    a11y::describe(&button, t!("update.download.hint", url = release.url));
                    if button.clicked() {
                        download = true;
                    }
                    if ui.button(t!("update.not_now")).clicked() {
                        dismiss = true;
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t!("update.note", variable = update::DISABLE_ENV))
                        .weak()
                        .small(),
                );
            });

        if download {
            match browser::open(&release.url) {
                Ok(()) => {
                    self.status = t!("status.download_page", tag = release.tag);
                    self.tone = Tone::Good;
                }
                Err(err) => {
                    // The link is still useful even if nothing would open it.
                    ctx.copy_text(release.url.clone());
                    self.status = t!("status.link_copied", error = err);
                    self.tone = Tone::Bad;
                }
            }
            self.update.showing = false;
        }
        if dismiss || !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.update.showing = false;
            self.announce(t!("status.update_dismissed"));
        }
    }
}
