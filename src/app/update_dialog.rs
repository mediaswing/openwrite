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
//! and the check does not come back to ask again. The dialog also carries the
//! switch that stops it for good, kept in [`crate::settings`]: refusing the one
//! thing this program does over the network unasked should not depend on
//! knowing that an environment variable exists, and the offer to stop belongs
//! where the asking happens rather than in a window nobody would think to open.
//!
//! That switch is in the language window as well — see
//! [`App::update_check_control`] — because this dialog is the one place it
//! cannot be reached from once it has been turned off.

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
            log::info("update", "the check is turned off by the environment");
            return;
        }
        if !self.settings.update_check {
            log::info("update", "the check is turned off in the settings");
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

    /// The switch that stops the check, wherever it is being offered.
    ///
    /// Drawn in two places on purpose, and the same control in both: the dialog
    /// that does the asking, which is where somebody wants it the moment they
    /// are asked, and the language window, which is where they have to be able
    /// to find it again afterwards. A switch that could only be turned off from
    /// a dialog that never comes back is not a switch.
    ///
    /// Written down the moment it changes rather than on the way out of
    /// whatever is holding it: the dialog is dismissed with Escape as often as
    /// with a button, and a choice that survives only one of those routes is
    /// not a choice. Answers whether it changed, so a caller can tell that
    /// something was said already.
    pub(super) fn update_check_control(&mut self, ui: &mut egui::Ui) -> bool {
        let was = self.settings.update_check;
        let mut wanted = was;
        let switch = ui.checkbox(&mut wanted, t!("update.keep_checking"));
        a11y::describe(&switch, t!("update.keep_checking.hint"));
        if update::disabled() {
            // Then the switch is not the whole story, and saying so is better
            // than looking broken the next time the editor starts.
            ui.label(
                RichText::new(t!("update.note", variable = update::DISABLE_ENV))
                    .weak()
                    .small(),
            );
        }
        if wanted == was {
            return false;
        }
        self.settings.update_check = wanted;
        self.settings.save();
        self.announce(if wanted {
            t!("status.update_check_on")
        } else {
            t!("status.update_check_off")
        });
        true
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
        let mut changed = false;

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
                changed = self.update_check_control(ui);
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
        // Consumed rather than read: `key_pressed` leaves the press in the
        // frame for everything else to find too, so one Escape meant for the
        // find bar closed this as well. Taking it means whatever handles it
        // handles it once.
        let escaped = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if dismiss || !keep_open || escaped {
            self.update.showing = false;
            if !changed {
                self.announce(t!("status.update_dismissed"));
            }
        }
    }
}
