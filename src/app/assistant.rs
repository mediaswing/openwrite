//! Asking a model on this machine what happens next.
//!
//! The window is a small one and it does a small thing: gather what the writer
//! knows (see [`crate::ai::prompt`]), put one question to a model running
//! locally, and show the answer in a box the writer can edit before any of it
//! reaches the screenplay. Nothing is ever inserted without a deliberate press.
//!
//! Two rules shape the whole of this file.
//!
//! **It never blocks the editor.** Finding a server and generating an answer
//! both happen on a worker thread, and the window says which it is doing. A
//! model that takes a minute, or a server that has gone away, slows down
//! nothing but this window.
//!
//! **It never surprises the writer.** No request is made until a button is
//! pressed; the answer lands in a box rather than in the script; and the window
//! says out loud where the model is, in words, including when it is not on this
//! machine.

use super::{a11y, theme, App, Pane, Tone};
use crate::ai::{self, prompt::Ask, Server};
use crate::{t, tn};
use eframe::egui::{self, Align, Id, Key, Layout, RichText, ScrollArea};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const RESULT_ID: &str = "openwrite-suggestion";

/// How long to wait for Ollama after starting it. A cold start on a laptop is
/// several seconds; past this something is wrong rather than slow.
const START_WAIT: std::time::Duration = std::time::Duration::from_secs(25);

/// Which of the three questions is being asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Do,
    Say,
    Next,
}

impl Kind {
    fn label(self) -> String {
        match self {
            Kind::Do => t!("ask.do"),
            Kind::Say => t!("ask.say"),
            Kind::Next => t!("ask.next"),
        }
    }

    fn needs_character(self) -> bool {
        !matches!(self, Kind::Next)
    }
}

/// What the worker thread has to report.
enum Event {
    Found(Result<Server, String>),
    Answer(Result<String, String>),
    /// Whether the Ollama command line is on this machine.
    Installed(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Nothing in flight.
    Idle,
    /// Looking for a model server.
    Looking,
    /// Waiting on an answer.
    Asking,
    /// Starting Ollama, and waiting for it to come up.
    Starting,
}

/// The window's state, and its one worker thread.
pub(super) struct Assistant {
    /// Where to look. From `OPENWRITE_AI_URL`, or Ollama's default port.
    pub url: String,
    /// The server, once something has answered.
    server: Option<Server>,
    /// The model chosen out of the ones the server offers.
    pub model: String,
    pub kind: Kind,
    /// Who the question is about.
    pub who: String,
    /// The suggestion, editable before it goes anywhere.
    pub result: String,
    /// Why the last attempt did not work.
    pub trouble: Option<String>,
    /// Whether Ollama is installed. Worked out once, off the main thread,
    /// because it costs a process launch to find out.
    installed: Option<bool>,
    state: State,
    events: Option<Receiver<Event>>,
}

impl Default for Assistant {
    fn default() -> Self {
        Assistant {
            url: ai::default_url(),
            server: None,
            model: String::new(),
            kind: Kind::Next,
            who: String::new(),
            result: String::new(),
            trouble: None,
            installed: None,
            state: State::Idle,
            events: None,
        }
    }
}

impl Assistant {
    fn busy(&self) -> bool {
        self.state != State::Idle
    }

    /// Go and see what is listening. Returns at once; the answer arrives
    /// through [`Assistant::poll`].
    fn look(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.state = State::Looking;
        self.trouble = None;

        let url = self.url.clone();
        let ctx = ctx.clone();
        let ask_installed = self.installed.is_none();
        std::thread::spawn(move || {
            let found = ai::discover(&url).map_err(|err| err.to_string());
            // Only worth knowing when there was nothing there: it is what
            // decides whether the window can offer to start it.
            if ask_installed && found.is_err() {
                let _ = tx.send(Event::Installed(ai::ollama_installed()));
            }
            let _ = tx.send(Event::Found(found));
            // Wake the window: it may be sitting still waiting for this.
            ctx.request_repaint();
        });
    }

    /// Start Ollama and wait for it, reporting through the same channel as a
    /// search, because to the window the two end the same way.
    fn start(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.state = State::Starting;
        self.trouble = None;

        let url = self.url.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let started = ai::start_ollama(&url, START_WAIT).map_err(|err| err.to_string());
            let _ = tx.send(Event::Found(started));
            ctx.request_repaint();
        });
    }

    /// Put a question. `prompt` is already built, so the thread owns everything
    /// it needs and borrows nothing from the editor.
    fn send(&mut self, ctx: &egui::Context, prompt: String) {
        let (Some(server), false) = (self.server.clone(), self.busy()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.state = State::Asking;
        self.trouble = None;

        let model = self.model.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let answer = ai::generate(&server, &model, ai::prompt::SYSTEM, &prompt)
                .map_err(|err| err.to_string());
            let _ = tx.send(Event::Answer(answer));
            ctx.request_repaint();
        });
    }

    /// Collect whatever the worker has finished, and say what happened.
    ///
    /// Drains the channel rather than taking one event a frame. A worker sends
    /// `Installed` and then `Found` and wakes the window once; stopping after
    /// the first would leave the second sitting in the channel with no repaint
    /// coming, and the window would say "Looking\u{2026}" until the mouse moved.
    fn poll(&mut self) -> Option<(String, Tone)> {
        let event = loop {
            match self.events.as_ref()?.try_recv() {
                // Not the end of anything: the search that sent it is still
                // running, so keep reading.
                Ok(Event::Installed(installed)) => self.installed = Some(installed),
                Ok(event) => break event,
                Err(TryRecvError::Empty) => return None,
                // The thread died without sending: do not wait for it for ever.
                Err(TryRecvError::Disconnected) => {
                    self.events = None;
                    if self.busy() {
                        self.state = State::Idle;
                        self.trouble = Some(t!("ideas.stopped.detail"));
                        return Some((t!("ideas.stopped"), Tone::Bad));
                    }
                    return None;
                }
            }
        };
        self.events = None;
        self.state = State::Idle;

        match event {
            Event::Found(Ok(server)) => {
                if self.model.is_empty() || !server.models.contains(&self.model) {
                    self.model = server.default_model().unwrap_or_default();
                }
                let summary = server.summary();
                self.server = Some(server);
                Some((t!("status.found_model", summary = summary), Tone::Good))
            }
            Event::Found(Err(err)) => {
                self.server = None;
                self.trouble = Some(err);
                Some((t!("ideas.none_found"), Tone::Bad))
            }
            Event::Answer(Ok(answer)) => {
                let lines = answer.lines().filter(|l| !l.trim().is_empty()).count();
                self.result = answer;
                Some((tn!("status.suggestion_ready", lines), Tone::Good))
            }
            Event::Answer(Err(err)) => {
                self.trouble = Some(err);
                Some((t!("ideas.no_answer"), Tone::Bad))
            }
            // Drained in the loop above; it never reaches here.
            Event::Installed(_) => None,
        }
    }

    /// Can the window offer to start a model server itself?
    ///
    /// Only for one on this machine: there is no starting a server on somebody
    /// else's computer from here.
    fn can_start(&self) -> bool {
        self.installed == Some(true)
            && ai::http::url(&self.url).map(|u| u.is_loopback()).unwrap_or(false)
    }

    /// The question as the rest of the program understands it.
    fn ask(&self) -> Ask {
        match self.kind {
            Kind::Do => Ask::Do(self.who.clone()),
            Kind::Say => Ask::Say(self.who.clone()),
            Kind::Next => Ask::Next,
        }
    }
}

impl App {
    /// Open the window with a question about somebody ready to ask.
    pub(super) fn ask_about(&mut self, ctx: &egui::Context, name: &str, speaking: bool) {
        self.assistant.who = crate::bible::normalise(name);
        self.assistant.kind = if speaking { Kind::Say } else { Kind::Do };
        self.open_assistant(ctx);
    }

    pub(super) fn open_assistant(&mut self, ctx: &egui::Context) {
        self.show_assistant = true;
        if self.assistant.who.is_empty() {
            self.assistant.who = self.everybody().first().cloned().unwrap_or_default();
        }
        if self.assistant.server.is_none() {
            self.assistant.look(ctx);
            self.announce(t!("status.looking"));
        } else {
            self.announce(t!("status.ideas_open", ask = self.assistant.kind.label()));
        }
    }

    /// Everybody the question could be about: the profiled and the speaking.
    fn everybody(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.bible.profiles.iter().map(|p| p.name.clone()).collect();
        names.extend(self.stats.characters.keys().cloned());
        names.sort();
        names.dedup();
        names
    }

    /// Move whatever is in the suggestion box into the screenplay, at the
    /// caret.
    fn insert_suggestion(&mut self) {
        let text = self.assistant.result.trim().to_string();
        if text.is_empty() {
            self.fail_soft(t!("status.nothing_to_insert"));
            return;
        }
        let caret = self.caret.unwrap_or(usize::MAX);
        let (source, caret) = splice(&self.source, caret, &text);

        self.source = source;
        // Leave the caret at the end of what was inserted, which is where the
        // writer will carry on from.
        self.caret = Some(caret);
        self.restore_caret = Some(caret);
        self.dirty = true;
        self.needs_reparse = true;
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        self.confirm_done(tn!("status.inserted", lines));
    }

    pub(super) fn assistant_dialog(&mut self, ctx: &egui::Context) {
        if let Some((message, tone)) = self.assistant.poll() {
            self.status = message;
            self.tone = tone;
        }
        if !self.show_assistant {
            return;
        }

        let people = self.everybody();
        let mut keep_open = true;
        // Decided in the closure, done after it, so that `self` is free.
        let mut suggest = false;
        let mut look_again = false;
        let mut start_ollama = false;
        let mut get_ollama = false;
        let mut insert = false;
        let mut copy = false;
        let mut announce: Option<(String, Tone)> = None;

        egui::Window::new(t!("ideas.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .default_width(620.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let palette = theme::palette(ui.visuals());

                // -- where the model is -----------------------------------
                let (line, colour) = match (&self.assistant.server, self.assistant.state) {
                    (_, State::Looking) => (t!("ideas.looking"), palette.muted),
                    (_, State::Asking) => (t!("ideas.thinking"), palette.muted),
                    (_, State::Starting) => (t!("ideas.starting"), palette.muted),
                    (Some(server), _) => (server.summary(), palette.ok),
                    (None, _) => (t!("ideas.none_found"), palette.bad),
                };
                ui.horizontal(|ui| {
                    let response = ui.label(RichText::new(&line).color(colour));
                    a11y::name(&response, t!("a11y.model_server", state = line));
                    a11y::live_region(&response);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button = ui.add_enabled(
                            !self.assistant.busy(),
                            egui::Button::new(if self.assistant.server.is_some() {
                                t!("ideas.look_again")
                            } else {
                                t!("ideas.look")
                            }),
                        );
                        a11y::describe(
                            &button,
                            t!("ideas.look.hint", url = self.assistant.url),
                        );
                        if button.clicked() {
                            look_again = true;
                        }
                    });
                });

                // What to do about it, rather than what went wrong. The
                // technical detail is still here, underneath and in small, for
                // whoever actually wants it — but a writer who has simply not
                // started Ollama should be one press from having started it.
                if self.assistant.server.is_none() && !self.assistant.busy() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        if self.assistant.can_start() {
                            ui.label(t!("ideas.installed_not_running"));
                            if ui
                                .button(t!("ideas.start"))
                                .on_hover_text(t!("ideas.start.hint"))
                                .clicked()
                            {
                                start_ollama = true;
                            }
                        } else if self.assistant.installed == Some(false) {
                            ui.label(t!("ideas.no_server"));
                            if ui.button(t!("ideas.get")).clicked() {
                                get_ollama = true;
                            }
                            ui.label(RichText::new(t!("ideas.get.note")).weak().small());
                        }
                    });
                }
                if let Some(trouble) = &self.assistant.trouble {
                    // Muted, not red: the headline above already says something
                    // is wrong, and repeating the alarm in a second colour just
                    // makes the window shout.
                    let response = ui.label(RichText::new(trouble).color(palette.muted).small());
                    a11y::describe(&response, t!("ideas.trouble.hint"));
                }
                if self.assistant.server.as_ref().is_some_and(|s| !s.url.is_loopback()) {
                    // Worth saying plainly: this is the one case where the
                    // screenplay leaves the machine it was written on.
                    ui.label(
                        RichText::new(t!("ideas.remote", url = self.assistant.url))
                            .color(palette.warn),
                    );
                }

                ui.add_space(6.0);
                ui.separator();

                // -- the question -----------------------------------------
                ui.horizontal(|ui| {
                    ui.label(t!("ideas.ask"));
                    for kind in [Kind::Do, Kind::Say, Kind::Next] {
                        let response =
                            ui.radio_value(&mut self.assistant.kind, kind, kind.label());
                        a11y::name(&response, kind.label());
                    }
                });

                if self.assistant.kind.needs_character() {
                    ui.horizontal(|ui| {
                        ui.label(t!("ideas.about"));
                        if people.is_empty() {
                            ui.label(RichText::new(t!("ideas.nobody")).weak());
                        } else {
                            let combo = egui::ComboBox::from_id_salt("openwrite-ask-who")
                                .selected_text(if self.assistant.who.is_empty() {
                                    t!("ideas.choose")
                                } else {
                                    self.assistant.who.clone()
                                })
                                .show_ui(ui, |ui| {
                                    for name in &people {
                                        ui.selectable_value(
                                            &mut self.assistant.who,
                                            name.clone(),
                                            name,
                                        );
                                    }
                                });
                            a11y::name(&combo.response, t!("a11y.which_character"));
                        }
                        // A profile is what makes the answer sound like them.
                        let known = self.bible.get(&self.assistant.who).is_some_and(|p| !p.is_bare());
                        if !known && !self.assistant.who.is_empty() {
                            ui.label(
                                RichText::new(t!("ideas.no_notes"))
                                    .color(palette.warn)
                                    .small(),
                            );
                        }
                    });
                }

                if let Some(server) = &self.assistant.server {
                    ui.horizontal(|ui| {
                        ui.label(t!("ideas.model"));
                        let combo = egui::ComboBox::from_id_salt("openwrite-ask-model")
                            .selected_text(self.assistant.model.clone())
                            .show_ui(ui, |ui| {
                                for name in &server.models {
                                    ui.selectable_value(
                                        &mut self.assistant.model,
                                        name.clone(),
                                        name,
                                    );
                                }
                            });
                        a11y::name(&combo.response, t!("a11y.which_model"));
                    });
                }

                ui.add_space(6.0);
                let ready = self.assistant.server.is_some()
                    && !self.assistant.busy()
                    && !self.assistant.model.is_empty()
                    && (!self.assistant.kind.needs_character() || !self.assistant.who.is_empty());
                let button = ui.add_enabled(ready, egui::Button::new(t!("ideas.suggest")));
                a11y::name(
                    &button,
                    t!("a11y.suggest", ask = self.assistant.kind.label()),
                );
                a11y::describe(&button, t!("ideas.suggest.hint"));
                if button.clicked() {
                    suggest = true;
                }

                ui.add_space(6.0);
                ui.separator();

                // -- the answer -------------------------------------------
                ui.label(RichText::new(t!("ideas.suggestion")).strong());
                ui.label(RichText::new(t!("ideas.suggestion.hint")).weak().small());
                ScrollArea::vertical()
                    .id_salt("openwrite-suggestion-scroll")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut self.assistant.result)
                                .id(Id::new(RESULT_ID))
                                .desired_rows(8)
                                .desired_width(f32::INFINITY)
                                .hint_text(t!("ideas.suggestion.placeholder")),
                        );
                        a11y::name(&response, t!("ideas.suggestion"));
                        a11y::describe(&response, t!("ideas.suggestion.describe"));
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let has = !self.assistant.result.trim().is_empty();
                    let insert_button =
                        ui.add_enabled(has, egui::Button::new(t!("ideas.insert")));
                    a11y::describe(&insert_button, t!("ideas.insert.hint"));
                    if insert_button.clicked() {
                        insert = true;
                    }
                    if ui.add_enabled(has, egui::Button::new(t!("button.copy"))).clicked() {
                        copy = true;
                    }
                    if ui.add_enabled(has, egui::Button::new(t!("button.clear"))).clicked() {
                        self.assistant.result.clear();
                        announce = Some((t!("status.suggestion_cleared"), Tone::Info));
                    }
                });
            });

        if look_again {
            self.assistant.look(ctx);
            self.announce(t!("status.looking_at", url = self.assistant.url));
        }
        if start_ollama {
            self.assistant.start(ctx);
            self.announce(t!("status.starting_ollama"));
        }
        if get_ollama {
            match crate::browser::open(ai::OLLAMA_DOWNLOAD) {
                Ok(()) => self.announce(t!("status.opened_page", url = ai::OLLAMA_DOWNLOAD)),
                Err(err) => self.fail_soft(err),
            }
        }
        if suggest {
            let ask = self.assistant.ask();
            let context = crate::ai::Context::gather(&self.source, self.caret, &self.bible, &ask);
            match context.thin() {
                Some(why) => self.fail_soft(why),
                None => {
                    let prompt = context.prompt(&ask);
                    self.assistant.send(ctx, prompt);
                    self.announce(t!("status.asking", ask = ask.label()));
                }
            }
        }
        if copy {
            ctx.copy_text(self.assistant.result.clone());
            self.confirm_done(t!("status.suggestion_copied"));
        }
        if insert {
            self.insert_suggestion();
        }
        if let Some((message, tone)) = announce {
            self.status = message;
            self.tone = tone;
        }

        if !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_assistant = false;
            self.announce(t!("status.ideas_closed"));
            self.focus_request = Some(Pane::Editor);
        }
    }
}

/// Put `text` into `source` at a caret given in characters, with a blank line
/// either side of it.
///
/// The blank lines are not decoration: Fountain reads by what surrounds a line,
/// so a suggestion glued to the line above it turns into dialogue under
/// somebody else's cue, or an action paragraph that swallows a scene heading.
/// Existing blank lines are counted rather than doubled, so inserting into a
/// gap that is already there leaves the gap the size it was.
///
/// Returns the new source and where the caret should end up: the end of what
/// was inserted, which is where the writer carries on from.
fn splice(source: &str, caret: usize, text: &str) -> (String, usize) {
    // A caret past the end of a screenplay that has since been shortened lands
    // at the end of it.
    let caret = caret.min(source.chars().count());
    let at = source
        .char_indices()
        .nth(caret)
        .map(|(i, _)| i)
        .unwrap_or(source.len());
    let (before, after) = source.split_at(at);

    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let tail = if after.is_empty() || after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };

    let insertion = format!("{lead}{text}{tail}");
    let end = caret + lead.chars().count() + text.chars().count();
    (format!("{before}{insertion}{after}"), end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caret comes back pointing at the end of the suggestion, not the end
    /// of the padding — that is where the writer wants to be.
    fn spliced(source: &str, caret: usize, text: &str) -> String {
        let (out, end) = splice(source, caret, text);
        assert!(end <= out.chars().count());
        let upto: String = out.chars().take(end).collect();
        assert!(upto.ends_with(text), "the caret is not at the end of {text:?}: {upto:?}");
        out
    }

    #[test]
    fn a_suggestion_dropped_mid_paragraph_gets_a_blank_line_either_side() {
        let source = "INT. HOUSE - DAY\n\nShe waits.";
        assert_eq!(
            spliced(source, source.chars().count(), "MAYA\nNo."),
            "INT. HOUSE - DAY\n\nShe waits.\n\nMAYA\nNo."
        );
    }

    #[test]
    fn a_gap_that_is_already_there_is_not_doubled() {
        let source = "INT. HOUSE - DAY\n\n";
        assert_eq!(
            spliced(source, source.chars().count(), "She waits."),
            "INT. HOUSE - DAY\n\nShe waits."
        );
    }

    #[test]
    fn one_newline_gets_the_second_one_it_needs() {
        let source = "INT. HOUSE - DAY\n";
        assert_eq!(
            spliced(source, source.chars().count(), "She waits."),
            "INT. HOUSE - DAY\n\nShe waits."
        );
    }

    #[test]
    fn inserting_in_the_middle_pads_both_ends() {
        let source = "One.\n\nTwo.";
        let at = source.find("Two.").unwrap();
        assert_eq!(spliced(source, at, "MIDDLE"), "One.\n\nMIDDLE\n\nTwo.");
    }

    #[test]
    fn inserting_into_an_empty_screenplay_adds_no_padding_at_all() {
        assert_eq!(spliced("", 0, "INT. HOUSE - DAY"), "INT. HOUSE - DAY");
    }

    #[test]
    fn inserting_at_the_very_start_pushes_the_script_down() {
        assert_eq!(spliced("She waits.", 0, "INT. HOUSE - DAY"), "INT. HOUSE - DAY\n\nShe waits.");
    }

    #[test]
    fn a_caret_past_the_end_lands_at_the_end() {
        let source = "She waits.";
        assert_eq!(spliced(source, 9_999, "MAYA\nNo."), "She waits.\n\nMAYA\nNo.");
    }

    #[test]
    fn the_caret_counts_characters_rather_than_bytes() {
        // Would land mid-character if the offsets were confused.
        let source = "INT. CAFÉ — DAY\n\nShe waits.";
        let caret = source.chars().count();
        assert_eq!(spliced(source, caret, "MAYA\nNo."), format!("{source}\n\nMAYA\nNo."));

        // And a caret in the middle of the multi-byte run splits cleanly.
        let caret = "INT. CAFÉ".chars().count();
        let (out, _) = splice(source, caret, "X");
        assert!(out.starts_with("INT. CAFÉ\n\nX\n\n — DAY"), "{out:?}");
    }
}
