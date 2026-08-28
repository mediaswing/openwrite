//! The Audio Drama tab.
//!
//! One workspace, four things in a column, in the order somebody actually does
//! them: open a story, cast it, look at what is about to happen to each line,
//! and record it.
//!
//! The rules are the ones the Ideas window follows, for the same reasons.
//!
//! **It never blocks the editor.** Fetching the list of voices and recording a
//! play both happen on a worker thread. A play is one paid request per line
//! and can run for minutes; the window stays live throughout, says which line
//! it is on, and can be stopped between lines.
//!
//! **It never surprises the writer.** Nothing is sent anywhere until Record is
//! pressed, the window says what every line will cost in advance — which voice,
//! which shift, which effect — and the one file it writes without being asked
//! is the story file itself, only when it has been edited, only when Record is
//! pressed, and only after saying so.

use super::{a11y, theme, App, Tone};
use crate::drama::{self, story, voice, Story, Summary};
use crate::settings;
use crate::{t, tn};
use eframe::egui::{self, Align, Layout, RichText, ScrollArea, Ui};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

/// What a story file is called.
const EXTENSION: &str = "xml";

/// Where to go for a key.
const SIGN_UP: &str = "https://elevenlabs.io/app/settings/api-keys";

/// What the worker thread has to report.
enum Event {
    Voices(Result<Vec<voice::Remote>, String>),
    /// Which line it has reached, out of how many, and who says it.
    Reached(usize, usize, String),
    Finished(Result<Summary, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    No,
    /// Asking ElevenLabs which voices this key can use.
    Listing,
    /// Recording.
    Working,
}

/// The tab's state, and its one worker thread.
pub(super) struct Drama {
    /// The story file, if one has been opened.
    pub path: Option<PathBuf>,
    pub story: Story,
    /// Whether the voices have been changed since the file was read or written.
    pub edited: bool,

    /// What is typed in the key box. Never shown unless asked for.
    pub key: String,
    pub reveal_key: bool,
    /// Whether the box has been filled in from the settings yet.
    ///
    /// Once, not every frame: seeding it whenever it is empty means a key
    /// cannot be cleared — the box refills from the settings between one frame
    /// and the next — so a saved key could never be removed or replaced by
    /// clearing the box first.
    key_seeded: bool,

    /// The voices the key can use, once asked for.
    pub catalogue: Vec<voice::Remote>,
    /// Narrows the voice pickers, because an account can have hundreds.
    pub filter: String,

    /// How much of each line's `age` to apply. See [`drama::Options`].
    pub age_strength: f32,
    pub keep_lines: bool,
    pub reuse: bool,

    /// Where the last recording went.
    pub last: Option<Summary>,
    pub trouble: Option<String>,

    busy: Busy,
    /// Line reached, out of how many, and who is speaking.
    progress: (usize, usize, String),
    events: Option<Receiver<Event>>,
    stop: Arc<AtomicBool>,
}

impl Default for Drama {
    fn default() -> Drama {
        Drama {
            path: None,
            story: Story::default(),
            edited: false,
            key: String::new(),
            reveal_key: false,
            key_seeded: false,
            catalogue: Vec::new(),
            filter: String::new(),
            age_strength: 1.0,
            keep_lines: true,
            reuse: true,
            last: None,
            trouble: None,
            busy: Busy::No,
            progress: (0, 0, String::new()),
            events: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drama {
    fn busy(&self) -> bool {
        self.busy != Busy::No
    }

    /// Is a recording running? What the menu greys Stop out by.
    pub(super) fn is_recording(&self) -> bool {
        self.busy == Busy::Working
    }

    /// Is there a play to record, and is everybody in it cast?
    ///
    /// The key is not part of it: whether one has been set is decided by the
    /// settings and the environment together, and the answer to "no key" is a
    /// sentence saying so rather than a menu entry that cannot be pressed and
    /// will not say why.
    pub(super) fn can_record(&self) -> bool {
        !self.busy() && !self.story.is_empty() && crate::drama::uncast(&self.story).is_empty()
    }

    /// Go and ask which voices this key can use.
    fn list(&mut self, ctx: &egui::Context, key: String) {
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.busy = Busy::Listing;
        self.trouble = None;

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = voice::voices(&key).map_err(|err| err.to_string());
            let _ = tx.send(Event::Voices(found));
            ctx.request_repaint();
        });
    }

    /// Record the play. Everything the thread needs is owned by it, so it
    /// borrows nothing from the editor while it runs.
    fn record(&mut self, ctx: &egui::Context, story: Story, options: drama::Options) {
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.busy = Busy::Working;
        self.trouble = None;
        self.last = None;
        self.progress = (0, story.lines.len(), String::new());
        self.stop = Arc::new(AtomicBool::new(false));

        let stop = Arc::clone(&self.stop);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let report = {
                let tx = tx.clone();
                let ctx = ctx.clone();
                move |at: usize, total: usize, who: &str| {
                    let _ = tx.send(Event::Reached(at, total, who.to_string()));
                    // Each line takes seconds; without this the window would
                    // sit still between them.
                    ctx.request_repaint();
                }
            };
            let done = drama::render(&story, &options, &report, &stop)
                .map_err(|err| err.to_string());
            let _ = tx.send(Event::Finished(done));
            ctx.request_repaint();
        });
    }

    /// Collect whatever the worker has finished, and say what happened.
    ///
    /// Drains the channel rather than taking one event a frame: a recording
    /// sends a line's progress and then the next line's, and stopping after
    /// the first would leave the window a line behind for ever.
    ///
    /// The channel is checked through a `match` rather than a `?`, and that is
    /// not a style choice. The events that finish something close the channel
    /// as they are handled, so the next turn of this loop finds no channel at
    /// all — and a `?` there would return `None` from the whole function and
    /// throw away the very message it had just built.
    fn poll(&mut self) -> Option<(String, Tone)> {
        let mut announcement = None;
        loop {
            let Some(events) = self.events.as_ref() else {
                // Nothing left in flight: whatever this pass collected is the
                // answer.
                return announcement;
            };
            let event = match events.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => return announcement,
                // The thread died without finishing: do not wait for it for ever.
                Err(TryRecvError::Disconnected) => {
                    self.events = None;
                    if self.busy() {
                        self.busy = Busy::No;
                        self.trouble = Some(t!("drama.stopped.detail"));
                        return Some((t!("drama.stopped"), Tone::Bad));
                    }
                    return announcement;
                }
            };
            match event {
                Event::Reached(at, total, who) => {
                    self.progress = (at, total, who);
                    // Not announced: a live region that fires once a line
                    // would talk over everything else a screen reader is
                    // saying. The progress line below is polite instead.
                }
                Event::Voices(Ok(voices)) => {
                    self.events = None;
                    self.busy = Busy::No;
                    let count = voices.len();
                    self.catalogue = voices;
                    announcement = Some((tn!("drama.status.voices", count), Tone::Good));
                }
                Event::Voices(Err(err)) => {
                    self.events = None;
                    self.busy = Busy::No;
                    self.trouble = Some(err);
                    announcement = Some((t!("drama.status.no_voices"), Tone::Bad));
                }
                Event::Finished(Ok(summary)) => {
                    self.events = None;
                    self.busy = Busy::No;
                    let message = t!(
                        "drama.status.recorded",
                        name = file_label(&summary.path),
                        length = length_of(summary.seconds)
                    );
                    self.last = Some(summary);
                    announcement = Some((message, Tone::Good));
                }
                Event::Finished(Err(err)) => {
                    self.events = None;
                    self.busy = Busy::No;
                    self.trouble = Some(err);
                    announcement = Some((t!("drama.status.not_recorded"), Tone::Bad));
                }
            }
        }
    }

    /// The voices worth showing in a picker, given what is typed in the filter.
    fn matching(&self) -> Vec<&voice::Remote> {
        let needle = self.filter.trim().to_lowercase();
        self.catalogue
            .iter()
            .filter(|remote| {
                needle.is_empty()
                    || remote.name.to_lowercase().contains(&needle)
                    || remote.description.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// What the catalogue calls the voice somebody has chosen.
    fn named(&self, voice_id: &str) -> Option<&voice::Remote> {
        let voice_id = voice_id.trim();
        self.catalogue.iter().find(|remote| remote.id == voice_id)
    }
}

/// `1:23`, for a length somebody reads rather than measures.
fn length_of(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// What is about to be done to a line, as a phrase.
///
/// Written out rather than shown as numbers on a dial, because the question it
/// answers is "is that what I meant by twelve and frightened?" and that is a
/// question about words.
fn treatment_words(planned: &drama::Planned) -> String {
    let mut parts: Vec<String> = Vec::new();
    let treatment = &planned.treatment;
    if treatment.semitones.abs() >= 0.05 {
        parts.push(t!(
            "drama.effect.pitch",
            semitones = format!("{:+.1}", treatment.semitones)
        ));
    }
    if (treatment.tempo - 1.0).abs() >= 0.01 {
        parts.push(if treatment.tempo > 1.0 {
            t!("drama.effect.faster", percent = ((treatment.tempo - 1.0) * 100.0).round())
        } else {
            t!("drama.effect.slower", percent = ((1.0 - treatment.tempo) * 100.0).round())
        });
    }
    if treatment.wobble > 0.0 {
        parts.push(t!("drama.effect.tremble"));
    }
    if treatment.breath > 0.0 {
        parts.push(t!("drama.effect.breath"));
    }
    if treatment.drive > 0.0 {
        parts.push(t!("drama.effect.forced"));
    }
    if treatment.gain <= 0.9 {
        parts.push(t!("drama.effect.quieter"));
    } else if treatment.gain >= 1.1 {
        parts.push(t!("drama.effect.louder"));
    }
    if parts.is_empty() {
        parts.push(t!("drama.effect.none"));
    }
    parts.push(crate::i18n::text(planned.treatment.pos.key(), &[]));
    parts.join(", ")
}

impl App {
    /// Read a story file into the tab.
    pub(super) fn open_drama(&mut self, path: &std::path::Path) {
        let timer = crate::log::Timer::start();
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let story = story::parse(&text);
                // Sizes and shapes, never a word of the dialogue.
                crate::log::info(
                    "drama",
                    format!(
                        "opened {} bytes, model {}, {} voices, {} lines, {} problems, {} ms",
                        text.len(),
                        story.model,
                        story.voices.len(),
                        story.lines.len(),
                        story.problems.len(),
                        timer.ms()
                    ),
                );
                let lines = story.lines.len();
                // A file with no cast list gets one, built from whoever
                // speaks — which is the whole point of noticing.
                let added = story.problems.iter().any(|p| p.kind == story::Kind::NoVoices);
                self.drama.story = story;
                self.drama.path = Some(path.to_path_buf());
                self.drama.edited = added;
                self.drama.last = None;
                self.drama.trouble = None;
                let note = if added { t!("drama.status.cast_added") } else { String::new() };
                self.confirm_done(t!(
                    "drama.status.opened",
                    name = file_label(path),
                    lines = tn!("drama.phrase.lines", lines),
                    note = note
                ));
            }
            Err(err) => {
                crate::log::error("drama", format!("{err}"));
                self.fail(t!("error.open", name = file_label(path), error = err))
            }
        }
    }

    pub(super) fn open_drama_dialog(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title(t!("drama.dialog.open"))
            .add_filter(t!("drama.filter.story"), &[EXTENSION])
            .pick_file();
        match picked {
            Some(path) => self.open_drama(&path),
            None => self.announce(t!("status.open_cancelled")),
        }
    }

    /// Write the story back, voices and all.
    pub(super) fn save_drama_to(&mut self, path: PathBuf) {
        let contents = self.drama.story.to_xml();
        let bytes = contents.len();
        match std::fs::write(&path, contents) {
            Ok(()) => {
                crate::log::info("drama", format!("saved {bytes} bytes"));
                self.drama.path = Some(path.clone());
                self.drama.edited = false;
                self.confirm_done(t!("drama.status.saved", name = file_label(&path)));
            }
            Err(err) => {
                crate::log::error("drama", format!("{err}"));
                self.fail(t!("error.save", name = file_label(&path), error = err));
            }
        }
    }

    pub(super) fn save_drama_as(&mut self) {
        let name = self
            .drama
            .path
            .as_ref()
            .map(|path| file_label(path))
            .unwrap_or_else(|| format!("{}.{EXTENSION}", t!("drama.file.untitled")));
        let picked = rfd::FileDialog::new()
            .set_title(t!("drama.dialog.save"))
            .add_filter(t!("drama.filter.story"), &[EXTENSION])
            .set_file_name(name)
            .save_file();
        match picked {
            Some(path) if path.extension().is_none() => {
                self.save_drama_to(path.with_extension(EXTENSION))
            }
            Some(path) => self.save_drama_to(path),
            None => self.announce(t!("status.save_cancelled")),
        }
    }

    pub(super) fn stop_recording(&mut self) {
        if !self.drama.is_recording() {
            return;
        }
        // Between lines rather than in the middle of one: a half-written file
        // would be worse than one more line of a play you asked to stop.
        self.drama.stop.store(true, Ordering::Relaxed);
        self.announce(t!("drama.status.stopping"));
    }

    /// Ask where the recording goes, then start it.
    pub(super) fn start_recording(&mut self, ctx: &egui::Context) {
        let key = self.settings.key();
        if key.trim().is_empty() {
            self.fail_soft(t!("drama.status.need_key"));
            return;
        }
        let missing = drama::uncast(&self.drama.story);
        if !missing.is_empty() {
            self.fail_soft(t!("drama.status.uncast", who = missing.join(", ")));
            return;
        }

        let stem = self
            .drama
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| t!("drama.file.untitled"));
        let picked = rfd::FileDialog::new()
            .set_title(t!("drama.dialog.record"))
            .add_filter(t!("drama.filter.recording"), &["wav"])
            .set_file_name(format!("{stem}.wav"))
            .save_file();
        let Some(out) = picked else {
            self.announce(t!("drama.status.record_cancelled"));
            return;
        };
        let out = if out.extension().is_none() { out.with_extension("wav") } else { out };

        // The voices about to be used are worth keeping. Said out loud rather
        // than done quietly, because it is somebody else's file.
        if self.drama.edited {
            if let Some(path) = self.drama.path.clone() {
                self.save_drama_to(path);
            }
        }

        let options = drama::Options {
            key,
            age_strength: self.drama.age_strength,
            out,
            keep_lines: self.drama.keep_lines,
            reuse: self.drama.reuse,
        };
        let story = self.drama.story.clone();
        let lines = story.lines.len();
        self.drama.record(ctx, story, options);
        self.announce(tn!("drama.status.recording", lines));
    }

    /// The whole tab.
    pub(super) fn drama_panel(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        if let Some((message, tone)) = self.drama.poll() {
            self.status = message;
            self.tone = tone;
        }

        // Decided in the closures, done after them, so that `self` is free.
        let mut open = false;
        let mut save = false;
        let mut save_as = false;
        let mut list = false;
        let mut record = false;
        let mut stop = false;
        let mut get_key = false;
        let mut show_folder = false;
        let mut save_key = false;
        let mut back = false;

        egui::CentralPanel::default().show(ui, |ui| {
            let palette = theme::palette(ui.visuals());
            let busy = self.drama.busy();

            // -- the story ------------------------------------------------
            ui.horizontal(|ui| {
                // The menu bar is the way in and the way out, but somebody who
                // arrived here should not have to remember which menu they
                // came from to leave.
                if ui.button(t!("menu.drama.screenplay")).clicked() {
                    back = true;
                }
                ui.heading(t!("drama.heading"));
                let name = match &self.drama.path {
                    Some(path) if self.drama.edited => {
                        t!("drama.file.edited", name = file_label(path))
                    }
                    Some(path) => file_label(path),
                    None => t!("drama.no_story"),
                };
                ui.label(RichText::new(name).color(palette.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t!("drama.save_as"))).clicked() {
                        save_as = true;
                    }
                    let can_save = self.drama.path.is_some() && !self.drama.story.is_empty();
                    if ui.add_enabled(can_save && !busy, egui::Button::new(t!("button.save"))).clicked() {
                        save = true;
                    }
                    let button = ui.add_enabled(!busy, egui::Button::new(t!("drama.open")));
                    a11y::describe(&button, t!("drama.open.hint"));
                    if button.clicked() {
                        open = true;
                    }
                });
            });
            ui.separator();

            if self.drama.story.is_empty() {
                ui.add_space(8.0);
                ui.label(t!("drama.empty"));
                ui.label(RichText::new(t!("drama.empty.hint")).weak().small());
                return;
            }

            ScrollArea::vertical()
                .id_salt("openwrite-drama-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.drama_key(ui, palette, &mut list, &mut get_key, &mut save_key);
                    ui.add_space(8.0);
                    self.drama_cast(ui, palette);
                    ui.add_space(8.0);
                    self.drama_lines(ui, palette);
                    ui.add_space(8.0);
                    self.drama_problems(ui, palette);
                    ui.add_space(8.0);
                    self.drama_record(ui, palette, &mut record, &mut stop, &mut show_folder);
                });
        });

        if back {
            self.show_workspace(super::Workspace::Screenplay);
        }
        if open {
            self.open_drama_dialog();
        }
        if save {
            if let Some(path) = self.drama.path.clone() {
                self.save_drama_to(path);
            }
        }
        if save_as {
            self.save_drama_as();
        }
        if save_key {
            self.settings.elevenlabs_key = self.drama.key.trim().to_string();
            self.settings.save();
            self.confirm_done(t!("drama.status.key_saved"));
        }
        if list {
            let key = self.settings.key();
            if key.is_empty() {
                self.fail_soft(t!("drama.status.need_key"));
            } else {
                self.drama.list(ctx, key);
                self.announce(t!("drama.status.listing"));
            }
        }
        if get_key {
            match crate::browser::open(SIGN_UP) {
                Ok(()) => self.announce(t!("status.opened_page", url = SIGN_UP)),
                Err(err) => self.fail_soft(err),
            }
        }
        if record {
            self.start_recording(ctx);
        }
        if stop {
            self.stop_recording();
        }
        if show_folder {
            if let Some(folder) = self.drama.last.as_ref().and_then(|s| s.path.parent()) {
                match crate::browser::show_folder(folder) {
                    Ok(()) => self.announce(t!("drama.status.shown")),
                    Err(err) => self.fail_soft(err),
                }
            }
        }
    }

    // -- the sections ---------------------------------------------------------

    fn drama_key(
        &mut self,
        ui: &mut Ui,
        palette: theme::Palette,
        list: &mut bool,
        get_key: &mut bool,
        save_key: &mut bool,
    ) {
        ui.label(RichText::new(t!("drama.key.heading")).strong());
        let from_environment = settings::Settings::key_from_environment();

        if from_environment {
            // Nothing to type: the environment has already answered, and a box
            // that cannot change anything should not look like one.
            ui.label(
                RichText::new(t!("drama.key.from_environment", name = settings::KEY_ENV))
                    .color(palette.ok),
            );
        } else {
            if !self.drama.key_seeded {
                self.drama.key = self.settings.elevenlabs_key.clone();
                self.drama.key_seeded = true;
            }
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.drama.key)
                        .desired_width(320.0)
                        .password(!self.drama.reveal_key)
                        .hint_text(t!("drama.key.hint")),
                );
                a11y::name(&field, t!("drama.key.heading"));
                a11y::describe(&field, t!("drama.key.describe"));

                let toggle = ui.selectable_label(self.drama.reveal_key, t!("drama.key.show"));
                if toggle.clicked() {
                    self.drama.reveal_key = !self.drama.reveal_key;
                }
                let changed = self.drama.key.trim() != self.settings.elevenlabs_key.trim();
                if ui
                    .add_enabled(changed, egui::Button::new(t!("drama.key.remember")))
                    .on_hover_text(t!("drama.key.remember.hint"))
                    .clicked()
                {
                    *save_key = true;
                }
                if ui.button(t!("drama.key.get")).clicked() {
                    *get_key = true;
                }
            });
        }

        ui.horizontal(|ui| {
            let has_key = !self.settings.key().is_empty();
            let button = ui.add_enabled(
                has_key && !self.drama.busy(),
                egui::Button::new(if self.drama.catalogue.is_empty() {
                    t!("drama.voices.fetch")
                } else {
                    t!("drama.voices.again")
                }),
            );
            a11y::describe(&button, t!("drama.voices.fetch.hint"));
            if button.clicked() {
                *list = true;
            }
            let (line, colour) = match (self.drama.busy, self.drama.catalogue.len()) {
                (Busy::Listing, _) => (t!("drama.voices.fetching"), palette.muted),
                (_, 0) => (t!("drama.voices.none"), palette.muted),
                (_, count) => (tn!("drama.voices.have", count), palette.ok),
            };
            let response = ui.label(RichText::new(&line).color(colour));
            a11y::name(&response, line);
            a11y::live_region(&response);
        });

        if let Some(trouble) = &self.drama.trouble {
            // Muted rather than red: whatever went wrong has already been said
            // in the status bar, and a second alarm in a second colour only
            // makes the tab shout.
            let response = ui.label(RichText::new(trouble).color(palette.muted).small());
            a11y::describe(&response, t!("drama.trouble.hint"));
        }
    }

    fn drama_cast(&mut self, ui: &mut Ui, palette: theme::Palette) {
        ui.label(RichText::new(t!("drama.cast.heading")).strong());
        ui.label(RichText::new(t!("drama.cast.hint")).weak().small());

        if !self.drama.catalogue.is_empty() {
            ui.horizontal(|ui| {
                ui.label(t!("drama.cast.filter"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.drama.filter)
                        .desired_width(220.0)
                        .hint_text(t!("drama.cast.filter.hint")),
                );
                a11y::name(&field, t!("drama.cast.filter"));
            });
        }

        // Which voice each combo chose, applied after the loop so that the
        // catalogue is not borrowed while the cast is being written to.
        let mut chosen: Option<(usize, String)> = None;
        let matching: Vec<voice::Remote> =
            self.drama.matching().into_iter().cloned().collect();
        // What each cast member's voice is *called*, looked up before the loop
        // for the same reason — and from the whole catalogue rather than the
        // filtered list, so that typing in the filter box never makes it look
        // as though somebody's voice has been forgotten.
        let names: Vec<Option<String>> = self
            .drama
            .story
            .voices
            .iter()
            .map(|member| self.drama.named(&member.voice_id).map(|remote| remote.name.clone()))
            .collect();
        let busy = self.drama.busy();

        egui::Grid::new("openwrite-drama-cast")
            .num_columns(4)
            .striped(true)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new(t!("drama.cast.who")).weak().small());
                ui.label(RichText::new(t!("drama.cast.gender")).weak().small());
                ui.label(RichText::new(t!("drama.cast.voice")).weak().small());
                ui.label(RichText::new(t!("drama.cast.voice_id")).weak().small());
                ui.end_row();

                for (index, member) in self.drama.story.voices.iter_mut().enumerate() {
                    let ready = member.is_ready();
                    ui.label(
                        RichText::new(&member.name)
                            .color(if ready { palette.ok } else { palette.warn }),
                    );
                    ui.label(
                        RichText::new(member.gender.clone().unwrap_or_default())
                            .color(palette.muted),
                    );

                    // The picker, when there is a catalogue to pick from.
                    if matching.is_empty() {
                        ui.label(RichText::new(t!("drama.cast.no_catalogue")).weak().small());
                    } else {
                        let selected = match names.get(index).cloned().flatten() {
                            Some(name) => name,
                            // Cast, but to a voice this catalogue has never
                            // heard of: an id pasted from somewhere else, or
                            // one that has since been deleted. Say which,
                            // rather than showing an empty box.
                            None if member.is_ready() => {
                                t!("drama.cast.elsewhere")
                            }
                            None => t!("drama.cast.choose"),
                        };
                        let combo = egui::ComboBox::from_id_salt(("openwrite-drama-voice", index))
                            .selected_text(selected)
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for remote in &matching {
                                    if ui
                                        .selectable_label(
                                            remote.id == member.voice_id.trim(),
                                            remote.label(),
                                        )
                                        .clicked()
                                    {
                                        chosen = Some((index, remote.id.clone()));
                                    }
                                }
                            });
                        a11y::name(
                            &combo.response,
                            t!("drama.cast.which", name = member.name.clone()),
                        );
                    }

                    // And the raw id, always, so that a voice id from anywhere
                    // else can simply be pasted in.
                    let field = ui.add_enabled(
                        !busy,
                        egui::TextEdit::singleline(&mut member.voice_id)
                            .desired_width(230.0)
                            .hint_text(t!("drama.cast.voice_id.hint")),
                    );
                    a11y::name(&field, t!("drama.cast.which_id", name = member.name.clone()));
                    if field.changed() {
                        self.drama.edited = true;
                    }
                    ui.end_row();
                }
            });

        if let Some((index, voice_id)) = chosen {
            if let Some(member) = self.drama.story.voices.get_mut(index) {
                member.voice_id = voice_id;
                self.drama.edited = true;
            }
        }
    }

    fn drama_lines(&mut self, ui: &mut Ui, palette: theme::Palette) {
        let planned = drama::plan(&self.drama.story, self.drama.age_strength);
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("drama.lines.heading")).strong());
            ui.label(
                RichText::new(tn!("drama.phrase.lines", planned.len()))
                    .color(palette.muted)
                    .small(),
            );
        });

        ui.horizontal(|ui| {
            ui.label(t!("drama.age_strength"));
            let slider = ui.add(
                egui::Slider::new(&mut self.drama.age_strength, 0.0..=1.5)
                    .show_value(false)
                    .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
            );
            a11y::name(&slider, t!("drama.age_strength"));
            a11y::describe(&slider, t!("drama.age_strength.hint"));
            ui.label(
                RichText::new(format!("{:.0}%", self.drama.age_strength * 100.0))
                    .color(palette.muted),
            );
        });
        ui.label(RichText::new(t!("drama.age_strength.note")).weak().small());

        egui::Grid::new("openwrite-drama-lines")
            .num_columns(4)
            .striped(true)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new(t!("drama.cast.who")).weak().small());
                ui.label(RichText::new(t!("drama.lines.age")).weak().small());
                ui.label(RichText::new(t!("drama.lines.treatment")).weak().small());
                ui.label(RichText::new(t!("drama.lines.words")).weak().small());
                ui.end_row();

                for (index, plan) in planned.iter().enumerate() {
                    let line = &self.drama.story.lines[index];
                    ui.label(
                        RichText::new(&plan.speaker)
                            .color(if plan.cast { palette.ok } else { palette.warn }),
                    );
                    ui.label(
                        RichText::new(match plan.age {
                            Some(age) => age.to_string(),
                            None => "—".to_string(),
                        })
                        .color(palette.muted),
                    );
                    let words = treatment_words(plan);
                    let state = crate::i18n::text(plan.state.key(), &[]);
                    ui.label(RichText::new(format!("{state} · {words}")).small());
                    let response = ui.label(RichText::new(shorten(&line.text)).monospace());
                    // The whole line, for a screen reader, with everything that
                    // is about to happen to it said first.
                    a11y::name(
                        &response,
                        t!(
                            "a11y.drama_line",
                            index = index + 1,
                            who = plan.speaker.clone(),
                            treatment = words,
                            text = line.text.clone()
                        ),
                    );
                    ui.end_row();
                }
            });
    }

    fn drama_problems(&mut self, ui: &mut Ui, palette: theme::Palette) {
        if self.drama.story.problems.is_empty() {
            return;
        }
        ui.label(RichText::new(t!("drama.problems.heading")).strong());
        ui.label(RichText::new(t!("drama.problems.hint")).weak().small());
        for problem in &self.drama.story.problems {
            let what = crate::i18n::text(
                problem.kind.key(),
                &[("detail", problem.detail.clone())],
            );
            let text = if problem.line > 0 {
                t!("drama.problems.at", line = problem.line, what = what)
            } else {
                what
            };
            ui.label(RichText::new(text).color(palette.warn).small());
        }
    }

    fn drama_record(
        &mut self,
        ui: &mut Ui,
        palette: theme::Palette,
        record: &mut bool,
        stop: &mut bool,
        show_folder: &mut bool,
    ) {
        ui.separator();
        ui.horizontal(|ui| {
            let uncast = drama::uncast(&self.drama.story);
            // Deliberately not conditioned on there being a key — the same
            // rule the menu entry follows. A button that is dead because of
            // something it does not mention is worse than one that answers
            // "set a key first" when it is pressed.
            let button = ui.add_enabled(
                self.drama.can_record(),
                egui::Button::new(t!("drama.record")),
            );
            a11y::describe(&button, t!("drama.record.hint"));
            if button.clicked() {
                *record = true;
            }
            if ui
                .add_enabled(self.drama.is_recording(), egui::Button::new(t!("drama.stop")))
                .clicked()
            {
                *stop = true;
            }

            let mut checkbox = |on: &mut bool, label: String, hint: String| {
                let response = ui.checkbox(on, label);
                a11y::describe(&response, hint);
            };
            checkbox(
                &mut self.drama.reuse,
                t!("drama.reuse"),
                t!("drama.reuse.hint"),
            );
            checkbox(
                &mut self.drama.keep_lines,
                t!("drama.keep_lines"),
                t!("drama.keep_lines.hint"),
            );

            if !uncast.is_empty() {
                ui.label(
                    RichText::new(t!("drama.uncast", who = uncast.join(", ")))
                        .color(palette.warn)
                        .small(),
                );
            }
        });

        // -- where it has got to ------------------------------------------
        if self.drama.is_recording() {
            let (at, total, who) = self.drama.progress.clone();
            let fraction = if total == 0 { 0.0 } else { at as f32 / total as f32 };
            let text = t!(
                "drama.progress",
                index = at + 1,
                total = total,
                who = who
            );
            let bar = ui.add(
                egui::ProgressBar::new(fraction)
                    .text(RichText::new(&text).small())
                    .desired_width(420.0),
            );
            a11y::name(&bar, text);
            a11y::live_region(&bar);
        }

        if let Some(summary) = &self.drama.last {
            ui.horizontal(|ui| {
                let reused = if summary.reused > 0 {
                    tn!("drama.result.reused", summary.reused)
                } else {
                    String::new()
                };
                let text = t!(
                    "drama.result",
                    name = file_label(&summary.path),
                    length = length_of(summary.seconds),
                    lines = tn!("drama.phrase.lines", summary.lines),
                    reused = reused
                );
                let response = ui.label(RichText::new(&text).color(palette.ok));
                a11y::name(&response, text);
                a11y::live_region(&response);
                if ui.button(t!("drama.show")).clicked() {
                    *show_folder = true;
                }
            });
            if self.drama.keep_lines {
                ui.label(
                    RichText::new(t!(
                        "drama.result.lines_in",
                        folder = file_label(&drama::parts_dir(&summary.path))
                    ))
                    .weak()
                    .small(),
                );
            }
        }
    }
}

/// A line of dialogue, cut to something that fits in a table.
fn shorten(text: &str) -> String {
    const MOST: usize = 64;
    if text.chars().count() <= MOST {
        return text.to_string();
    }
    let kept: String = text.chars().take(MOST - 1).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_is_cut_at_a_character_rather_than_a_byte() {
        // Would split a multi-byte character if this counted bytes.
        let text = "café ".repeat(40);
        let short = shorten(&text);
        assert!(short.chars().count() <= 64);
        assert!(short.ends_with('…'));
        // And a short one is left exactly as it is.
        assert_eq!(shorten("What?"), "What?");
    }

    #[test]
    fn a_length_reads_as_minutes_and_seconds() {
        assert_eq!(length_of(0), "0:00");
        assert_eq!(length_of(9), "0:09");
        assert_eq!(length_of(83), "1:23");
        assert_eq!(length_of(600), "10:00");
    }
}
