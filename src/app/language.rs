//! The language window.
//!
//! One picker, and everything a translator needs to work: what is wrong with
//! the file in use, where to put a new one, and a button that re-reads the
//! folder without restarting the editor. That last one is the whole of the
//! translator's loop — change a line, press Reload, read the window — and
//! without it the loop runs through a restart, which is slow enough to make a
//! long file a chore.
//!
//! The picker names each language in itself, because somebody looking for their
//! own language is looking for the word they call it by, not for the English
//! word for it.

use super::{a11y, App, Pane, Tone};
use crate::i18n;
use crate::{t, tn};
use eframe::egui::{self, Align, Key, Layout, RichText, ScrollArea};

impl App {
    pub(super) fn open_language(&mut self) {
        self.show_language = !self.show_language;
        if !self.show_language {
            self.announce(t!("status.language_closed"));
            return;
        }
        self.announce(t!("status.language_opened", name = i18n::current_name()));
    }

    pub(super) fn language_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_language {
            return;
        }
        let mut keep_open = true;
        // Decided inside the window, acted on after it, so that `self` is free.
        let mut chosen = self.settings.language.clone();
        let mut reload = false;
        let mut show_folder = false;
        let mut close = false;

        egui::Window::new(t!("language.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let available = i18n::available();

                ui.label(t!("language.caption"));
                let selected = if chosen == i18n::AUTO {
                    t!("language.system")
                } else {
                    available
                        .iter()
                        .find(|(code, _)| *code == chosen)
                        .map(|(_, name)| name.clone())
                        .unwrap_or_else(|| chosen.clone())
                };
                let combo = egui::ComboBox::from_id_salt("openwrite-language")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut chosen,
                            i18n::AUTO.to_string(),
                            t!("language.system"),
                        );
                        for (code, name) in &available {
                            ui.selectable_value(&mut chosen, code.clone(), name);
                        }
                    });
                a11y::name(&combo.response, t!("language.caption"));
                a11y::describe(&combo.response, t!("language.hint"));

                // What is wrong with the file in use, if it is somebody's own.
                // The built-in language never has anything to say here, which a
                // test sees to.
                let problems = i18n::current_problems();
                if !problems.is_empty() {
                    ui.add_space(6.0);
                    let palette = super::theme::palette(ui.visuals());
                    ui.label(
                        RichText::new(tn!("language.problem_count", problems.len()))
                            .color(palette.warn),
                    );
                    ScrollArea::vertical()
                        .id_salt("openwrite-language-problems")
                        .max_height(120.0)
                        .show(ui, |ui| {
                            for problem in &problems {
                                ui.label(
                                    RichText::new(t!(
                                        "language.problem",
                                        line = problem.line,
                                        what = problem.what
                                    ))
                                    .small(),
                                );
                            }
                        });
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add(egui::Label::new(t!("language.help")).wrap());

                if let Some(dir) = i18n::languages_dir() {
                    ui.add_space(4.0);
                    let path = ui.label(
                        RichText::new(t!("language.folder", path = dir.display()))
                            .weak()
                            .small(),
                    );
                    a11y::name(&path, t!("language.folder", path = dir.display()));
                    ui.horizontal(|ui| {
                        let button = ui.button(t!("language.show_folder"));
                        a11y::describe(&button, t!("language.show_folder.hint"));
                        if button.clicked() {
                            show_folder = true;
                        }
                        let again = ui.button(t!("language.reload"));
                        a11y::describe(&again, t!("language.reload.hint"));
                        if again.clicked() {
                            reload = true;
                        }
                    });
                }

                ui.add_space(6.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t!("button.close")).clicked() {
                        close = true;
                    }
                });
            });

        if chosen != self.settings.language {
            self.switch_language(chosen);
        }
        if reload {
            i18n::reload();
            i18n::apply_setting(&self.settings.language);
            self.status = t!("status.language_reloaded", name = i18n::current_name());
            self.tone = Tone::Good;
        }
        if show_folder {
            self.show_languages_folder();
        }

        if close || !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_language = false;
            self.announce(t!("status.language_closed"));
            self.focus_request = Some(Pane::Editor);
        }
    }

    /// Put a newly chosen language into use, and write the choice down.
    ///
    /// Saved immediately rather than on the way out, because the one thing
    /// worse than an editor in the wrong language is an editor that was put
    /// right and forgot by morning.
    fn switch_language(&mut self, wanted: String) {
        self.settings.language = wanted;
        self.settings.save();
        i18n::apply_setting(&self.settings.language);
        // In the new language, since that is the one the writer is now reading.
        self.status = t!("status.language_changed", name = i18n::current_name());
        self.tone = Tone::Good;
    }

    /// Open the languages folder in the file manager, making it first — an
    /// invitation to put a file somewhere is not much of an invitation if the
    /// somewhere does not exist yet.
    fn show_languages_folder(&mut self) {
        let Some(dir) = i18n::languages_dir() else {
            self.fail_soft(t!("language.no_folder"));
            return;
        };
        if let Err(err) = std::fs::create_dir_all(&dir) {
            crate::log::warn(
                "language",
                format!("{} could not be made: {err}", dir.display()),
            );
            self.fail_soft(t!("language.folder_failed", error = err));
            return;
        }
        match crate::browser::show_folder(&dir) {
            Ok(()) => self.announce(t!("status.folder_opened", path = dir.display())),
            Err(err) => self.fail_soft(err),
        }
    }
}
