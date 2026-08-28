//! The debug log window.
//!
//! What the program has been doing, in the order it did it — see [`crate::log`]
//! for what goes in and, more importantly, what does not.
//!
//! It is a window rather than a file the writer has to go and find, because the
//! moment a debug log is worth anything is the moment something has just gone
//! wrong, and that is not the moment to be explaining where the application
//! support directory is. Copy or Save is one press from here.
//!
//! Routine entries — a repagination for every keystroke — are hidden by
//! default. They are the bulk of the log and almost never the interesting part,
//! but when a document has become slow they are the only part that matters, so
//! there is a switch rather than a decision made on the writer's behalf.

use super::{a11y, theme, App, Tone};
use crate::log::{self, Level};
use crate::{t, tn};
use eframe::egui::{self, Align, Id, Key, Layout, RichText, ScrollArea};

const LOG_ID: &str = "openwrite-debug-log";

/// The window's own state: what it is showing, and the text it last built.
///
/// The text is cached because rebuilding a couple of thousand lines every frame
/// to show the same thing would be exactly the sort of waste this window exists
/// to find.
#[derive(Default)]
pub(super) struct DebugLog {
    /// Show the routine entries as well.
    pub routine: bool,
    /// The rendered log, and the entry count it was rendered at.
    cached: String,
    at: Option<(u64, bool)>,
}

impl DebugLog {
    /// The log as text, rebuilt only when there is something new to say.
    fn text(&mut self) -> &str {
        let key = (log::written(), self.routine);
        if self.at != Some(key) {
            let min = if self.routine { Level::Debug } else { Level::Info };
            self.cached = log::text_from(min);
            self.at = Some(key);
        }
        &self.cached
    }

    /// Forget the cache, so the next frame rebuilds.
    fn invalidate(&mut self) {
        self.at = None;
    }
}

impl App {
    pub(super) fn open_debug_log(&mut self) {
        self.show_log = !self.show_log;
        if !self.show_log {
            self.announce(t!("status.log_closed"));
            return;
        }
        let (total, bad) = log::counts();
        self.announce(t!(
            "status.log_opened",
            entries = tn!("phrase.entries", total),
            trouble = log_trouble(bad)
        ));
    }

    pub(super) fn debug_log_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_log {
            return;
        }
        let mut keep_open = true;
        let mut copy = false;
        let mut save = false;
        let mut clear = false;
        let mut announce: Option<(String, Tone)> = None;

        egui::Window::new(t!("log.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .default_width(820.0)
            .default_height(600.0)
            .max_width(980.0)
            .max_height(640.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let palette = theme::palette(ui.visuals());
                let (total, bad) = log::counts();

                ui.horizontal(|ui| {
                    let summary = t!(
                        "log.summary",
                        entries = tn!("phrase.entries", total),
                        trouble = log_trouble(bad)
                    );
                    let colour = if bad == 0 { palette.muted } else { palette.warn };
                    let response = ui.label(RichText::new(&summary).color(colour));
                    a11y::name(&response, t!("a11y.log", summary = summary));
                    a11y::live_region(&response);

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let switch =
                            ui.checkbox(&mut self.debug_log.routine, t!("log.routine"));
                        a11y::describe(&switch, t!("log.routine.hint"));
                    });
                });

                match log::sink_path() {
                    Some(path) => {
                        // The name, not the whole path: a log file lives
                        // wherever OPENWRITE_LOG pointed, which can be long,
                        // and the full path is in the text below and read out
                        // to anybody who asks the label what it says.
                        let name = super::file_label(&path);
                        let response = ui
                            .label(RichText::new(t!("log.also_written", name = name)).weak().small());
                        a11y::name(&response, t!("log.also_written", name = path.display()));
                    }
                    None => {
                        ui.label(
                            RichText::new(t!("log.memory_only", variable = log::PATH_ENV))
                                .weak()
                                .small(),
                        );
                    }
                }

                ui.add_space(6.0);
                ui.separator();

                ScrollArea::vertical()
                    .id_salt("openwrite-log-scroll")
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(440.0)
                    .show(ui, |ui| {
                        // A text field rather than a label: it is selectable and
                        // a screen reader can walk it line by line. Read-only,
                        // because editing a record of what happened is not a
                        // thing anybody wants to do by accident.
                        let mut text = self.debug_log.text();
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .id(Id::new(LOG_ID))
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(18)
                                .desired_width(f32::INFINITY),
                        );
                        a11y::name(&response, t!("log.title"));
                        a11y::describe(&response, t!("log.describe"));
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("button.copy")).clicked() {
                        copy = true;
                    }
                    let saved = ui.button(t!("log.save"));
                    a11y::describe(&saved, t!("log.save.hint"));
                    if saved.clicked() {
                        save = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(t!("button.clear")).clicked() {
                            clear = true;
                        }
                    });
                });
            });

        if copy {
            ctx.copy_text(log::text());
            announce = Some((t!("status.log_copied"), Tone::Good));
        }
        if save {
            announce = Some(self.save_debug_log());
        }
        if clear {
            log::clear();
            self.debug_log.invalidate();
            log::info("log", "cleared by the writer");
            announce = Some((t!("status.log_cleared"), Tone::Info));
        }
        if let Some((message, tone)) = announce {
            self.status = message;
            self.tone = tone;
        }

        if !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_log = false;
            self.announce(t!("status.log_closed"));
            self.focus_request = Some(super::Pane::Editor);
        }
    }

    fn save_debug_log(&mut self) -> (String, Tone) {
        let picked = rfd::FileDialog::new()
            .set_title(t!("log.save.title"))
            .add_filter(t!("filter.text_log"), &["txt", "log"])
            .set_file_name("openwrite-debug.txt")
            .save_file();
        let Some(path) = picked else {
            return (t!("status.save_cancelled"), Tone::Info);
        };
        match log::save(&path) {
            Ok(()) => (
                t!("status.log_saved", name = super::file_label(&path)),
                Tone::Good,
            ),
            Err(err) => (t!("error.log_save", error = err), Tone::Bad),
        }
    }
}

/// How the count of warnings and errors reads, for the one sentence that says
/// both how much there is and whether any of it matters.
fn log_trouble(bad: usize) -> String {
    if bad == 0 {
        t!("log.nothing_wrong")
    } else {
        tn!("phrase.warnings", bad)
    }
}
