//! The characters and world window.
//!
//! Everything a writer knows about the story that the script does not say, in
//! one place beside the script: the world at the top, the people underneath.
//! It is saved in the `.sct` file with the screenplay (see
//! [`crate::document`]), so notes and draft travel together.
//!
//! The window is one column of ordinary controls on purpose. A screen reader
//! reads it top to bottom in the order it is written, Tab moves through it, and
//! every field says what it is for — which a canvas of draggable index cards
//! would not.

use super::{a11y, theme, App, Tone};
use crate::bible::normalise;
use crate::{t, tn};
use eframe::egui::{self, Align, Id, Key, Layout, RichText, ScrollArea, Ui};

/// One field of a profile: what it is called, what it is for, and how much room
/// it gets. The first two are language keys rather than words.
pub(crate) struct Field {
    pub label: &'static str,
    pub hint: &'static str,
    rows: usize,
}

/// The prompts are questions rather than headings. "Wants" gets a writer
/// further than "Motivation".
pub(crate) const FIELDS: [Field; 5] = [
    Field { label: "bible.field.role", hint: "bible.field.role.hint", rows: 1 },
    Field { label: "bible.field.age", hint: "bible.field.age.hint", rows: 1 },
    Field { label: "bible.field.wants", hint: "bible.field.wants.hint", rows: 2 },
    Field { label: "bible.field.voice", hint: "bible.field.voice.hint", rows: 2 },
    Field { label: "bible.field.notes", hint: "bible.field.notes.hint", rows: 4 },
];

impl App {
    /// Open the window, on a particular character if one is wanted.
    pub(super) fn open_bible(&mut self, on: Option<&str>) {
        self.show_bible = true;
        if let Some(name) = on {
            self.bible.ensure(name);
            self.bible_sel = self.bible.position(name);
        } else if self.bible_sel.is_none() {
            self.bible_sel = (!self.bible.profiles.is_empty()).then_some(0);
        }
        self.announce(match self.bible.profiles.len() {
            0 => t!("status.characters_opened_empty"),
            n => tn!("status.characters_opened", n),
        });
    }

    /// Character names that speak in the script but have no profile yet.
    pub(super) fn unprofiled(&self) -> Vec<String> {
        let speaking: Vec<&str> = self.stats.characters.keys().map(String::as_str).collect();
        self.bible.unprofiled(speaking)
    }

    pub(super) fn bible_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_bible {
            return;
        }
        let mut keep_open = true;
        // What the frame decided, acted on after the borrow of `self` ends.
        let mut announce: Option<(String, Tone)> = None;
        let mut select: Option<usize> = None;
        let mut add: Option<String> = None;
        let mut adopt_all = false;
        let mut remove: Option<String> = None;
        let mut ask: Option<(String, bool)> = None;
        let mut edited = false;
        let mut save = false;
        let mut close = false;

        egui::Window::new(t!("bible.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .default_width(780.0)
            .default_height(560.0)
            // The fields inside ask for all the width they can get, so the
            // window has to be the thing that says no.
            .max_width(900.0)
            .max_height(620.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Room kept back for the footer, so the Save button does not
                // scroll away from somebody half-way down a long cast list.
                let footer = ui.spacing().interact_size.y + 34.0;
                let room = (ui.available_height() - footer).max(160.0);
                ScrollArea::vertical().max_height(room).auto_shrink([false, false]).show(ui, |ui| {
                    // -- the world ----------------------------------------
                    ui.heading(t!("bible.world"));
                    ui.label(RichText::new(t!("bible.world.hint")).weak().small());
                    let world = ui.add(
                        egui::TextEdit::multiline(&mut self.bible.world)
                            .id(Id::new("openwrite-world"))
                            .desired_rows(4)
                            .desired_width(f32::INFINITY)
                            .hint_text(t!("bible.world.placeholder")),
                    );
                    a11y::name(&world, t!("bible.world"));
                    a11y::describe(&world, t!("bible.world.describe"));
                    if world.changed() {
                        edited = true;
                    }

                    ui.add_space(10.0);
                    ui.separator();

                    // -- the people ----------------------------------------
                    ui.heading(t!("bible.characters"));
                    ui.add_space(4.0);
                    // The list only needs room for a name; the fields need all
                    // the rest, because that is where the writing happens. Both
                    // columns lay out downwards inside a fixed width, or the
                    // fields would flow sideways off the window.
                    ui.horizontal_top(|ui| {
                        let full = ui.available_width();
                        let list = (full * 0.28).clamp(140.0, 230.0);
                        let fields = (full - list - 24.0).max(200.0);
                        let column = |ui: &mut Ui, width: f32, build: &mut dyn FnMut(&mut Ui)| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(width);
                                    build(ui);
                                },
                            );
                        };
                        column(ui, list, &mut |ui| {
                            self.character_list(ui, &mut select, &mut add, &mut adopt_all)
                        });
                        ui.separator();
                        column(ui, fields, &mut |ui| {
                            self.character_fields(ui, &mut remove, &mut ask, &mut edited)
                        });
                    });
                });

                ui.separator();
                self.bible_footer(ui, &mut save, &mut close);
            });

        if let Some(index) = select {
            self.bible_sel = Some(index);
            self.bible_remove_armed = None;
            if let Some(profile) = self.bible.profiles.get(index) {
                announce = Some((t!("status.selected", name = profile.name), Tone::Info));
            }
        }
        if let Some(name) = add {
            let name = normalise(&name);
            if name.is_empty() {
                announce = Some((t!("status.type_a_name"), Tone::Bad));
            } else if self.bible.get(&name).is_some() {
                self.bible_sel = self.bible.position(&name);
                announce = Some((t!("status.already_here", name = name), Tone::Info));
            } else {
                self.bible.ensure(&name);
                self.bible_sel = self.bible.position(&name);
                self.bible_new_name.clear();
                self.dirty = true;
                announce = Some((t!("status.added", name = name), Tone::Good));
            }
        }
        if adopt_all {
            let missing = self.unprofiled();
            for name in &missing {
                self.bible.ensure(name);
            }
            match missing.len() {
                0 => announce = Some((t!("status.everybody_here"), Tone::Info)),
                n => {
                    self.bible_sel = self.bible.position(&missing[0]);
                    self.dirty = true;
                    announce = Some((tn!("status.added_from_script", n), Tone::Good));
                }
            }
        }
        if let Some(name) = remove {
            if self.bible_remove_armed.as_deref() == Some(name.as_str()) {
                self.bible.remove(&name);
                self.bible_remove_armed = None;
                self.bible_sel = (!self.bible.profiles.is_empty()).then_some(0);
                self.dirty = true;
                announce = Some((t!("status.removed", name = name), Tone::Good));
            } else {
                self.bible_remove_armed = Some(name.clone());
                announce = Some((t!("status.remove_sure", name = name), Tone::Bad));
            }
        }
        if let Some((name, speaking)) = ask {
            self.ask_the_model(ctx, &name, speaking);
        }
        if edited {
            self.dirty = true;
        }
        if let Some((message, tone)) = announce {
            self.status = message;
            self.tone = tone;
        }
        if save {
            // The ordinary document save: these notes live in the screenplay
            // file, so there is one Save and this is it.
            self.save();
        }

        if close || !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_bible = false;
            self.bible_remove_armed = None;
            self.announce(t!("status.characters_closed"));
            self.focus_request = Some(super::Pane::Editor);
        }
    }

    /// The bottom of the window: what happens to all this, and the button that
    /// makes it happen.
    ///
    /// There is no separate save for the story bible, because it is not a
    /// separate file — it is part of the screenplay, saved in the same `.sct`
    /// document. Saying so here is the point of the line of text: a window full
    /// of typing with no visible Save is a window somebody will close nervously.
    fn bible_footer(&mut self, ui: &mut Ui, save: &mut bool, close: &mut bool) {
        let palette = theme::palette(ui.visuals());
        ui.horizontal(|ui| {
            let note = if self.dirty {
                RichText::new(t!("bible.footer.unsaved")).color(palette.warn)
            } else {
                RichText::new(t!("bible.footer.saved")).color(palette.muted)
            };
            let response = ui.label(note.small());
            a11y::live_region(&response);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(t!("button.close")).clicked() {
                    *close = true;
                }
                let keys = super::shortcuts::label(
                    ui.ctx(),
                    &self.bindings,
                    super::shortcuts::Action::Save,
                );
                let button = ui.add_enabled(
                    self.dirty,
                    egui::Button::new(t!("bible.save")).shortcut_text(keys),
                );
                a11y::name(
                    &button,
                    if self.dirty {
                        t!("bible.save.unsaved")
                    } else {
                        t!("bible.save.saved")
                    },
                );
                a11y::describe(&button, t!("bible.save.describe"));
                if button.clicked() {
                    *save = true;
                }
            });
        });
    }

    fn character_list(
        &mut self,
        ui: &mut Ui,
        select: &mut Option<usize>,
        add: &mut Option<String>,
        adopt_all: &mut bool,
    ) {
        let total = self.bible.profiles.len();
        if total == 0 {
            ui.label(RichText::new(t!("bible.nobody")).weak().small());
        }
        // No scroll area of its own: the window already scrolls, and a list
        // boxed inside a fixed height is a list that hides somebody.
        for (i, profile) in self.bible.profiles.iter().enumerate() {
            let selected = self.bible_sel == Some(i);
            let response =
                ui.selectable_label(selected, RichText::new(&profile.name).monospace());
            a11y::name(
                &response,
                t!(
                    "a11y.character",
                    name = profile.name,
                    index = i + 1,
                    total = total
                ),
            );
            // Say whether there is anything under the name, so that a screen
            // reader user can hear which ones still need work.
            a11y::describe(
                &response,
                if profile.is_bare() {
                    t!("bible.nothing_written")
                } else {
                    t!("bible.has_notes")
                },
            );
            if response.clicked() {
                *select = Some(i);
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.bible_new_name)
                    .id(Id::new("openwrite-new-character"))
                    .desired_width(ui.available_width() - 54.0)
                    .hint_text(t!("bible.new_character")),
            );
            a11y::name(&field, t!("bible.new_character.name"));
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button(t!("button.add")).clicked() || entered {
                *add = Some(self.bible_new_name.clone());
            }
        });

        let missing = self.unprofiled();
        if !missing.is_empty() {
            let label = match missing.len() {
                1 => t!("bible.adopt_one", name = missing[0]),
                n => tn!("bible.adopt", n),
            };
            let response = ui.button(&label);
            a11y::describe(&response, t!("a11y.adopt", names = missing.join(", ")));
            if response.clicked() {
                *adopt_all = true;
            }
        }
    }

    fn character_fields(
        &mut self,
        ui: &mut Ui,
        remove: &mut Option<String>,
        ask: &mut Option<(String, bool)>,
        edited: &mut bool,
    ) {
        let Some(index) = self.bible_sel.filter(|i| *i < self.bible.profiles.len()) else {
            ui.label(RichText::new(t!("bible.choose_someone")).weak());
            return;
        };
        let name = self.bible.profiles[index].name.clone();
        ui.label(RichText::new(&name).heading());

        // How much this character is in the script already — the one number
        // that tells a writer whether a profile is worth filling in.
        if let Some(stats) = self.stats.characters.get(&name) {
            let speeches = tn!("phrase.speeches", stats.cues);
            let words = tn!("phrase.words", stats.words);
            ui.label(
                RichText::new(t!("bible.in_the_script", speeches = speeches, words = words))
                    .weak()
                    .small(),
            );
        } else {
            ui.label(RichText::new(t!("bible.not_spoken")).weak().small());
        }
        ui.add_space(4.0);

        let profile = &mut self.bible.profiles[index];
        for (i, field) in FIELDS.iter().enumerate() {
            let value = match field.label {
                "bible.field.role" => &mut profile.role,
                "bible.field.age" => &mut profile.age,
                "bible.field.wants" => &mut profile.want,
                "bible.field.voice" => &mut profile.voice,
                _ => &mut profile.notes,
            };
            let label = crate::i18n::text(field.label, &[]);
            let hint = crate::i18n::text(field.hint, &[]);
            ui.label(RichText::new(&label).strong());
            let response = ui.add(
                egui::TextEdit::multiline(value)
                    .id(Id::new(("openwrite-profile-field", index, i)))
                    .desired_rows(field.rows)
                    .desired_width(f32::INFINITY)
                    .hint_text(&hint),
            );
            a11y::name(&response, t!("a11y.field", field = label, name = name));
            a11y::describe(&response, hint);
            if response.changed() {
                *edited = true;
            }
            ui.add_space(4.0);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if cfg!(feature = "ai") {
                if ui.button(t!("ask.do")).clicked() {
                    *ask = Some((name.clone(), false));
                }
                if ui.button(t!("ask.say")).clicked() {
                    *ask = Some((name.clone(), true));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let armed = self.bible_remove_armed.as_deref() == Some(name.as_str());
                let palette = theme::palette(ui.visuals());
                let label = if armed {
                    RichText::new(t!("button.remove_sure")).color(palette.bad)
                } else {
                    RichText::new(t!("button.remove"))
                };
                let response = ui.button(label);
                a11y::name(
                    &response,
                    if armed {
                        t!("a11y.remove_armed", name = name)
                    } else {
                        t!("a11y.remove", name = name)
                    },
                );
                if response.clicked() {
                    *remove = Some(name.clone());
                }
            });
        });
    }
}
