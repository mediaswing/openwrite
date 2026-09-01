//! Keeping a copy of unsaved work, and offering it back.
//!
//! The reasoning for a copy rather than an auto-save over the writer's own file
//! is in [`crate::recovery`]. This is the part the writer meets: a copy written
//! quietly while there is something unsaved, taken away the moment there is
//! not, and — if the editor stopped before it could be taken away — one
//! question at the next start.
//!
//! **It says nothing while it works.** The status bar is a live region, so a
//! screen reader reads out whatever appears there. An editor that announced
//! itself every twenty seconds would be unusable with one, and would train
//! everybody else to stop reading the status bar. The copy goes in the debug
//! log and nowhere else. The one time this speaks is when it has something to
//! ask.

use super::{a11y, App, Pane, Tone};
use crate::t;
use crate::{document, log, recovery};
use eframe::egui::{self, Align, Key, Layout, RichText};
use std::time::{Duration, Instant};

/// How long between copies.
///
/// Long enough that a long screenplay is not being written to disk while
/// somebody types, short enough that what a crash costs is a sentence rather
/// than a scene.
const EVERY: Duration = Duration::from_secs(20);

impl App {
    /// Write a copy, if there is unsaved work and it is time.
    pub(super) fn keep_a_copy(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        // egui only redraws when something happens, and the moment a copy is
        // most wanted is the moment nothing is happening — the writer has
        // stopped to think, or walked away mid-scene. Asking for a repaint
        // keeps the clock running, and only while there is something to save,
        // so an editor with nothing unsaved in it stays properly idle.
        ctx.request_repaint_after(EVERY);
        if self.last_kept.elapsed() < EVERY {
            return;
        }
        self.last_kept = Instant::now();

        let timer = log::Timer::start();
        match recovery::keep(self.path.as_deref(), &self.document()) {
            // Sizes and durations, never the screenplay: see `crate::log`.
            Ok(_) => log::debug(
                "recovery",
                format!("{} characters kept, {} ms", self.source.len(), timer.ms()),
            ),
            // Not worth interrupting anybody over, and not worth a status line
            // either: the draft in front of them is unharmed, and the thing
            // that failed is a precaution.
            Err(err) => log::warn("recovery", format!("no copy could be kept: {err}")),
        }
    }

    /// Take away the copy for the document being put down.
    ///
    /// Called wherever the document stops being the one on screen — saved,
    /// replaced, opened over — because a copy that outlives the work it was
    /// standing in for is a question the writer will be asked for no reason.
    pub(super) fn forget_copy(&mut self, of: Option<std::path::PathBuf>) {
        recovery::discard(of.as_deref());
        self.last_kept = Instant::now();
    }

    /// Is there a copy left over from a run that ended badly?
    pub(super) fn look_for_a_copy(&mut self) {
        self.recovered = recovery::newest();
        if let Some(found) = &self.recovered {
            log::info(
                "recovery",
                format!(
                    "a copy is waiting, {} characters, {}",
                    found.document.source.len(),
                    if found.origin.is_some() { "from a file" } else { "never saved" }
                ),
            );
        }
    }

    pub(super) fn recovery_dialog(&mut self, ctx: &egui::Context) {
        let Some(found) = self.recovered.clone() else {
            return;
        };

        let mut restore = false;
        let mut discard = false;
        let mut keep_open = true;

        egui::Window::new(t!("recovery.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let headline = match found.name() {
                    Some(name) => t!("recovery.headline", name = name),
                    None => t!("recovery.headline.untitled"),
                };
                let response = ui.label(RichText::new(&headline).heading());
                a11y::name(&response, &headline);
                a11y::live_region(&response);

                ui.add_space(4.0);
                ui.add(egui::Label::new(t!("recovery.detail")).wrap());
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let no = ui.button(t!("recovery.discard"));
                    a11y::describe(&no, t!("recovery.discard.hint"));
                    if no.clicked() {
                        discard = true;
                    }
                    let yes = ui.button(t!("recovery.restore"));
                    a11y::describe(&yes, t!("recovery.restore.hint"));
                    if yes.clicked() {
                        restore = true;
                    }
                });
            });

        // As in the update dialog: consumed rather than read, so one press does
        // one thing.
        let escaped = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if restore {
            self.restore(found);
            return;
        }
        if discard {
            // The one gesture that throws work away, and it is the one that
            // says so on it. Nobody loses a draft by dismissing a window.
            let _ = std::fs::remove_file(&found.at);
            self.recovered = None;
            self.announce(t!("status.recovery_discarded"));
            self.focus_request = Some(Pane::Editor);
            return;
        }
        if !keep_open || escaped {
            // Escape and the window's own close button both mean "not now",
            // which is not the same as "throw it away". The copy stays where it
            // is and the question comes back at the next start; the writer is
            // told so, because a window that vanishes looks like a decision was
            // taken and here none has been.
            self.recovered = None;
            self.announce(t!("status.recovery_later"));
            self.focus_request = Some(Pane::Editor);
        }
    }

    /// Put a recovered draft back on screen, unsaved.
    ///
    /// Deliberately left dirty, and deliberately not written to the file it
    /// came from. The writer asked to see this work again, not to have it
    /// committed over whatever is on disk — which they have not seen since
    /// before the editor stopped.
    fn restore(&mut self, found: recovery::Recovered) {
        let document::Document { source, working, bible } = found.document;
        self.source = source;
        self.bible = bible;
        self.bible_sel = (!self.bible.profiles.is_empty()).then_some(0);
        self.path = found.origin;
        self.restore_caret = working.caret;
        self.restore_scene = working.scene;
        self.needs_reparse = true;
        self.dirty = true;
        self.outline_sel = 0;
        self.find_hits.clear();
        self.recovered = None;

        log::info("recovery", format!("{} characters restored", self.source.len()));
        self.status = t!("status.recovered");
        self.tone = Tone::Good;
        self.focus(Pane::Editor);
    }
}
