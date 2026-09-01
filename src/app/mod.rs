//! The openwrite editor window.
//!
//! Three panes — scene outline, Fountain editor, formatted preview — over one
//! shared document. Everything is reachable from the keyboard, every pane has
//! an accessible name, and what the application does is announced through a
//! live region rather than only shown.

mod a11y;
#[cfg(feature = "ai")]
mod assistant;
pub(crate) mod characters;
mod debug_log;
#[cfg(feature = "drama")]
mod drama;
mod language;
pub(crate) mod shortcuts;
mod theme;
mod recovery_dialog;
#[cfg(feature = "update")]
mod update_dialog;

use crate::bible::Bible;
use crate::document;
use crate::log;
use crate::settings::Settings;
use crate::{t, tn};
use crate::layout::{self, Options, Page};
use crate::render::Format;
use crate::stats::Stats;
use crate::Screenplay;
use eframe::egui::{
    self, Align, Id, Key, Layout, RichText, ScrollArea, TextStyle, ThemePreference, Ui,
};
use shortcuts::{Action, Binding};
use std::path::{Path, PathBuf};

/// What the application calls itself: the window title, and the name a screen
/// reader reads out when the window takes focus.
const APP_NAME: &str = "Screenplay Creation Tool";

const EDITOR_ID: &str = "openwrite-editor";
const PREVIEW_ID: &str = "openwrite-preview";
const FIND_ID: &str = "openwrite-find-field";

/// Open the editor window.
pub fn run(path: Option<PathBuf>) -> eframe::Result {
    // Before anything else, so that whatever happens next is on the record.
    log::start();
    log::catch_panics();
    // And before the first frame, so the window opens in the writer's own
    // language rather than opening in English and correcting itself.
    let settings = Settings::load();
    crate::i18n::apply_setting(&settings.language);
    log::info(
        "start",
        format!(
            "openwrite {} on {}, local model support {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            if cfg!(feature = "ai") { "in" } else { "out" }
        ),
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id("openwrite")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([560.0, 400.0]),
        ..Default::default()
    };
    let outcome = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            // Publish an accessibility tree from the first frame, rather than
            // waiting for a screen reader to ask for one.
            cc.egui_ctx.enable_accesskit();
            theme::apply(&cc.egui_ctx, false);
            Ok(Box::new(App::new(path, settings)))
        }),
    );
    match &outcome {
        Ok(()) => log::info("start", "the window closed"),
        Err(err) => log::error("start", format!("the window could not be opened: {err}")),
    }
    outcome
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Outline,
    Editor,
    Preview,
}

impl Pane {
    fn label(self) -> String {
        match self {
            Pane::Outline => t!("pane.outline"),
            Pane::Editor => t!("pane.editor"),
            Pane::Preview => t!("pane.preview"),
        }
    }

    fn next(self) -> Pane {
        match self {
            Pane::Outline => Pane::Editor,
            Pane::Editor => Pane::Preview,
            Pane::Preview => Pane::Outline,
        }
    }

    fn previous(self) -> Pane {
        self.next().next()
    }
}

/// Which of the two things this program does is on screen.
///
/// A screenplay and a radio play are made of the same words but worked on
/// completely differently — one is three panes over a document, the other is a
/// cast list and a render queue — so they are two workspaces rather than one
/// workspace with a lot of it hidden. Which one is showing is not a setting
/// and not a tab: it follows from the menu somebody used, the same way the
/// characters window follows from the Story menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Screenplay,
    #[cfg(feature = "drama")]
    AudioDrama,
}

impl Workspace {
    /// What it is called, for the status line.
    fn label(self) -> String {
        match self {
            Workspace::Screenplay => t!("workspace.screenplay"),
            #[cfg(feature = "drama")]
            Workspace::AudioDrama => t!("workspace.audio_drama"),
        }
    }
}

/// Which colour scheme the user asked for. The palettes themselves live in
/// [`theme`]; this only chooses between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    System,
    Light,
    Dark,
}

impl Scheme {
    fn label(self) -> String {
        match self {
            Scheme::System => t!("scheme.system"),
            Scheme::Light => t!("scheme.light"),
            Scheme::Dark => t!("scheme.dark"),
        }
    }

    fn preference(self) -> ThemePreference {
        match self {
            Scheme::System => ThemePreference::System,
            Scheme::Light => ThemePreference::Light,
            Scheme::Dark => ThemePreference::Dark,
        }
    }
}

/// How a status message reads. Colour follows the tone, but never carries it
/// alone: the wording says what happened as well, because a colour is exactly
/// what a colour-blind or screen reader user does not get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Good,
    Bad,
}

/// Something waiting on the answer to "you have unsaved changes".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    New,
    Open,
    Close,
}

pub struct App {
    source: String,
    path: Option<PathBuf>,
    dirty: bool,

    doc: Screenplay,
    pages: Vec<Page>,
    page_text: Vec<String>,
    scenes: Vec<(String, usize)>,
    stats: Stats,
    opts: Options,
    needs_reparse: bool,

    bindings: Vec<Binding>,
    pane: Pane,
    focus_request: Option<Pane>,
    outline_sel: usize,
    scroll_to_scene: Option<usize>,

    /// What the writer knows about the story that the script does not say.
    bible: Bible,
    show_bible: bool,
    /// Which character the bible window is showing.
    bible_sel: Option<usize>,
    bible_new_name: String,
    /// A character whose Remove button has been pressed once. Losing typed
    /// notes to a stray click is not something to be casual about.
    bible_remove_armed: Option<String>,

    show_log: bool,
    debug_log: debug_log::DebugLog,

    /// What the writer chose in the language window: a code, or `auto`. The
    /// language actually in use lives in [`crate::i18n`]; this is only what is
    /// written down, so that the choice survives a restart.
    settings: Settings,
    show_language: bool,

    #[cfg(feature = "update")]
    update: update_dialog::Update,

    #[cfg(feature = "ai")]
    assistant: assistant::Assistant,
    #[cfg(feature = "ai")]
    show_assistant: bool,

    /// Which tab is on screen.
    workspace: Workspace,
    #[cfg(feature = "drama")]
    drama: drama::Drama,

    show_outline: bool,
    show_preview: bool,
    show_help: bool,
    show_find: bool,
    find_query: String,
    find_hits: Vec<usize>,
    find_at: usize,
    focus_find: bool,
    place_caret: bool,

    /// Where the caret is, tracked every frame so that saving can record it.
    caret: Option<usize>,
    /// A caret and a scene read out of a `.sct` file, waiting for the editor to
    /// exist so they can be restored.
    restore_caret: Option<usize>,
    restore_scene: Option<usize>,

    scheme: Scheme,
    high_contrast: bool,
    /// Set when the contrast setting changes, so the theme is rebuilt once
    /// rather than every frame.
    restyle: bool,
    zoom: f32,

    status: String,
    tone: Tone,
    error: Option<String>,
    confirm: Option<Pending>,

    /// When a copy of unsaved work was last written, and one found waiting at
    /// startup. See [`crate::recovery`].
    last_kept: std::time::Instant,
    recovered: Option<crate::recovery::Recovered>,
}

impl App {
    fn new(path: Option<PathBuf>, settings: Settings) -> Self {
        let mut app = App {
            source: String::new(),
            path: None,
            dirty: false,
            doc: Screenplay::default(),
            pages: Vec::new(),
            page_text: Vec::new(),
            scenes: Vec::new(),
            stats: Stats::default(),
            opts: Options::default(),
            needs_reparse: true,
            bindings: shortcuts::bindings(),
            pane: Pane::Editor,
            focus_request: Some(Pane::Editor),
            outline_sel: 0,
            scroll_to_scene: None,
            bible: Bible::default(),
            show_bible: false,
            bible_sel: None,
            bible_new_name: String::new(),
            bible_remove_armed: None,
            show_log: false,
            debug_log: debug_log::DebugLog::default(),
            settings,
            show_language: false,
            #[cfg(feature = "update")]
            update: update_dialog::Update::default(),
            #[cfg(feature = "ai")]
            assistant: assistant::Assistant::default(),
            #[cfg(feature = "ai")]
            show_assistant: false,
            workspace: Workspace::Screenplay,
            #[cfg(feature = "drama")]
            drama: drama::Drama::default(),
            show_outline: true,
            show_preview: true,
            show_help: false,
            show_find: false,
            find_query: String::new(),
            find_hits: Vec::new(),
            find_at: 0,
            focus_find: false,
            place_caret: false,
            caret: None,
            restore_caret: None,
            restore_scene: None,
            scheme: Scheme::System,
            high_contrast: false,
            restyle: false,
            zoom: 1.0,
            status: String::new(),
            tone: Tone::Info,
            error: None,
            confirm: None,
            last_kept: std::time::Instant::now(),
            recovered: None,
        };

        match path {
            // A story file opens the tab that can do something with it,
            // rather than opening as a screenplay full of angle brackets.
            #[cfg(feature = "drama")]
            Some(path) if crate::drama::is_story(&path) => {
                app.source = welcome();
                app.workspace = Workspace::AudioDrama;
                app.open_drama(&path);
            }
            Some(path) => app.load(&path),
            None => {
                app.source = welcome();
                app.announce(t!("status.first_screenplay"));
            }
        }
        // After the document is on screen, so the question is asked over
        // something rather than over an empty window.
        app.look_for_a_copy();
        app
    }

    // -- document -----------------------------------------------------------

    fn reparse(&mut self) {
        let timer = log::Timer::start();
        self.doc = crate::parse(&self.source);
        self.pages = layout::paginate(&self.doc, &self.opts);
        self.scenes = layout::scene_pages(&self.pages);
        self.stats = crate::stats::compute(&self.doc, &self.pages);
        self.page_text = self
            .pages
            .iter()
            .map(|page| {
                let mut lines: Vec<String> = page.lines.iter().map(|l| l.to_text()).collect();
                lines.resize(self.opts.lines_per_page, String::new());
                lines.join("\n")
            })
            .collect();
        if self.outline_sel >= self.scenes.len() {
            self.outline_sel = self.scenes.len().saturating_sub(1);
        }
        self.needs_reparse = false;
        // Routine, and the one thing that runs on every keystroke, so it is
        // where a slow document shows up first.
        log::debug(
            "reparse",
            format!(
                "{} elements, {} pages, {} scenes, {} words, {} ms",
                self.doc.elements.len(),
                self.pages.len(),
                self.scenes.len(),
                self.stats.words,
                timer.ms()
            ),
        );
    }

    fn load(&mut self, path: &Path) {
        let timer = log::Timer::start();
        // What is being put down, so its copy can go with it. Nothing at all on
        // the first load of a session, which is what stops the copy this run
        // may be about to be offered being deleted before it is seen.
        let replacing = (self.dirty || self.path.is_some()).then(|| self.path.clone());
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let bytes = text.len();
                let document = document::read(&text);
                // Sizes and shapes, never the screenplay or the file's name.
                log::info(
                    "open",
                    format!(
                        "{bytes} bytes, {}, caret {}, {} profiles, {} ms",
                        if document::is_document(path) { "sct" } else { "plain fountain" },
                        if document.working.caret.is_some() { "restored" } else { "none" },
                        document.bible.profiles.len(),
                        timer.ms()
                    ),
                );
                self.source = document.source;
                self.bible = document.bible;
                self.bible_sel = (!self.bible.profiles.is_empty()).then_some(0);
                if let Some(replaced) = replacing {
                    self.forget_copy(replaced);
                }
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                self.needs_reparse = true;
                // Put the writer back where they left off.
                self.restore_caret = document.working.caret;
                self.restore_scene = document.working.scene;
                let resumed = if document.working.caret.is_some() {
                    t!("status.opened.resumed")
                } else {
                    String::new()
                };
                let notes = match self.bible.profiles.len() {
                    0 => String::new(),
                    n => tn!("status.opened.profiles", n),
                };
                self.confirm_done(t!(
                    "status.opened",
                    name = file_label(path),
                    resumed = resumed,
                    notes = notes
                ));
            }
            Err(err) => {
                log::error("open", format!("{err}, after {} ms", timer.ms()));
                self.fail(t!("error.open", name = file_label(path), error = err))
            }
        }
    }

    /// The document as it would be written: the screenplay plus where the
    /// writer has got to.
    fn document(&self) -> document::Document {
        document::Document {
            source: self.source.clone(),
            working: document::Working {
                caret: self.caret,
                scene: (!self.scenes.is_empty()).then_some(self.outline_sel),
            },
            bible: self.bible.clone(),
        }
    }

    fn save_to(&mut self, path: PathBuf) {
        let timer = log::Timer::start();
        let previous = self.path.clone();
        let contents = document::write_for(&path, &self.document());
        let bytes = contents.len();
        match std::fs::write(&path, contents) {
            Ok(()) => {
                log::info(
                    "save",
                    format!(
                        "{bytes} bytes, {}, {} profiles, {} ms",
                        if document::is_document(&path) { "sct" } else { "plain fountain" },
                        self.bible.profiles.len(),
                        timer.ms()
                    ),
                );
                let message = if document::is_document(&path) {
                    // Worth saying: this is the format that remembers.
                    t!("status.saved", name = file_label(&path))
                } else if self.bible.is_empty() {
                    t!("status.saved_fountain", name = file_label(&path))
                } else {
                    // Plain Fountain has nowhere to put the notes, and finding
                    // that out later is how a story bible gets lost.
                    t!(
                        "status.saved_fountain_only",
                        name = file_label(&path),
                        extension = document::EXTENSION
                    )
                };
                self.confirm_done(message);
                self.path = Some(path);
                self.dirty = false;
                // The work is on the writer's own disk now, so the copy
                // standing in for it has nothing left to stand in for. Both
                // names, because Save As moves the document to a new one and
                // leaves the old copy behind otherwise.
                self.forget_copy(previous);
                self.forget_copy(self.path.clone());
            }
            Err(err) => {
                log::error("save", format!("{err}, after {} ms", timer.ms()));
                self.fail(t!("error.save", error = err))
            }
        }
    }

    fn save(&mut self) {
        match self.path.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.{}", t!("file.untitled"), document::EXTENSION));
        let picked = rfd::FileDialog::new()
            .set_title(t!("dialog.save.title"))
            .add_filter(t!("filter.document"), &[document::EXTENSION])
            .add_filter(t!("filter.fountain"), &["fountain"])
            .set_file_name(name)
            .save_file();
        match picked {
            // A name typed without an extension is a `.sct` document: that is
            // the format the Save dialog offered first.
            Some(path) if path.extension().is_none() => {
                self.save_to(path.with_extension(document::EXTENSION))
            }
            Some(path) => self.save_to(path),
            None => self.announce(t!("status.save_cancelled")),
        }
    }

    fn open_dialog(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title(t!("dialog.open.title"))
            .add_filter(
                t!("filter.screenplays"),
                &[document::EXTENSION, "fountain", "spmd", "txt"],
            )
            .add_filter(t!("filter.document"), &[document::EXTENSION])
            .add_filter(t!("filter.fountain"), &["fountain", "spmd", "txt"])
            .pick_file();
        match picked {
            Some(path) => self.load(&path),
            None => self.announce(t!("status.open_cancelled")),
        }
    }

    fn export(&mut self) {
        let stem = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| t!("file.screenplay"));
        let picked = rfd::FileDialog::new()
            .set_title(t!("dialog.export.title"))
            .add_filter(t!("filter.text"), &["txt"])
            .add_filter(t!("filter.html"), &["html"])
            .add_filter(t!("filter.fdx"), &["fdx"])
            .set_file_name(format!("{stem}.txt"))
            .save_file();
        let Some(path) = picked else {
            self.announce(t!("status.export_cancelled"));
            return;
        };
        let format = Format::from_path(&path).unwrap_or(Format::Text);
        let timer = log::Timer::start();
        let rendered = self.render(format);
        let bytes = rendered.len();
        log::info(
            "export",
            format!("{} bytes as {}, rendered in {} ms", bytes, format.extension(), timer.ms()),
        );
        match std::fs::write(&path, rendered) {
            Ok(()) => self.confirm_done(t!(
                "status.exported",
                name = file_label(&path),
                format = format.extension().to_uppercase()
            )),
            Err(err) => {
                log::error("export", format!("{err}"));
                self.fail(t!("error.export", error = err))
            }
        }
    }

    fn render(&self, format: Format) -> String {
        match format {
            Format::Text => crate::render::text::render(&self.pages, &self.opts, false),
            Format::Html => crate::render::html::render(&self.doc, &self.opts),
            Format::Fdx => crate::render::fdx::render(&self.doc),
        }
    }

    // -- announcements ------------------------------------------------------

    /// Say something in the status bar, which is a live region — so a screen
    /// reader announces it without the user having to go and look.
    fn announce(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.tone = Tone::Info;
    }

    /// Something did not work, but not badly enough for a dialog.
    fn fail_soft(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.tone = Tone::Bad;
    }

    /// Something finished successfully.
    fn confirm_done(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.tone = Tone::Good;
    }

    fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status = message.clone();
        self.tone = Tone::Bad;
        self.error = Some(message);
    }

    fn script_pages(&self) -> usize {
        self.pages.iter().filter(|p| !p.is_title_page).count()
    }

    // -- actions ------------------------------------------------------------

    fn perform(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::New => self.guarded(Pending::New),
            Action::Open => self.guarded(Pending::Open),
            Action::Close => self.guarded(Pending::Close),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::Export => self.export(),
            Action::CopyFormatted => {
                ctx.copy_text(self.render(Format::Text));
                let pages = self.script_pages();
                self.confirm_done(t!(
                    "status.copied_script",
                    pages = a11y::pages_phrase(pages)
                ));
            }
            Action::Find => {
                self.show_find = true;
                self.focus_find = true;
                self.announce(t!("status.find_opened"));
            }
            Action::FindNext => self.step_find(1),
            Action::FindPrevious => self.step_find(-1),
            Action::ZoomIn => self.set_zoom(ctx, self.zoom + 0.1),
            Action::ZoomOut => self.set_zoom(ctx, self.zoom - 0.1),
            Action::ZoomReset => self.set_zoom(ctx, 1.0),
            Action::FocusOutline => self.focus(Pane::Outline),
            Action::FocusEditor => self.focus(Pane::Editor),
            Action::FocusPreview => self.focus(Pane::Preview),
            Action::CyclePane => self.focus(self.pane.next()),
            Action::CyclePaneBack => self.focus(self.pane.previous()),
            Action::NextScene => self.step_scene(1),
            Action::PreviousScene => self.step_scene(-1),
            Action::Characters => {
                if self.show_bible {
                    self.show_bible = false;
                    self.announce(t!("status.characters_closed"));
                } else {
                    self.open_bible(None);
                }
            }
            Action::Assist => self.toggle_assistant(ctx),
            #[cfg(feature = "drama")]
            Action::AudioDrama => self.show_workspace(Workspace::AudioDrama),
            #[cfg(feature = "drama")]
            Action::Screenplay => self.show_workspace(Workspace::Screenplay),
            // Each of these shows the tab as well as doing the thing, because
            // every one of them changes something on it, and doing that out of
            // sight would be the surprise.
            #[cfg(feature = "drama")]
            Action::DramaOpen => {
                self.show_workspace(Workspace::AudioDrama);
                self.open_drama_dialog();
            }
            #[cfg(feature = "drama")]
            Action::DramaSave => {
                self.show_workspace(Workspace::AudioDrama);
                if let Some(path) = self.drama.path.clone() {
                    self.save_drama_to(path);
                }
            }
            #[cfg(feature = "drama")]
            Action::DramaSaveAs => {
                self.show_workspace(Workspace::AudioDrama);
                self.save_drama_as();
            }
            #[cfg(feature = "drama")]
            Action::DramaRecord => {
                self.show_workspace(Workspace::AudioDrama);
                self.start_recording(ctx);
            }
            #[cfg(feature = "drama")]
            Action::DramaStop => self.stop_recording(),
            Action::DebugLog => self.open_debug_log(),
            Action::Language => self.open_language(),
            Action::ToggleOutline => {
                self.show_outline = !self.show_outline;
                self.announce(if self.show_outline {
                    t!("status.outline_shown")
                } else {
                    t!("status.outline_hidden")
                });
                if !self.show_outline && self.pane == Pane::Outline {
                    self.focus(Pane::Editor);
                }
            }
            Action::TogglePreview => {
                self.show_preview = !self.show_preview;
                self.announce(if self.show_preview {
                    t!("status.preview_shown")
                } else {
                    t!("status.preview_hidden")
                });
                if !self.show_preview && self.pane == Pane::Preview {
                    self.focus(Pane::Editor);
                }
            }
            Action::ToggleContrast => {
                self.high_contrast = !self.high_contrast;
                self.restyle = true;
                self.announce(if self.high_contrast {
                    t!("status.contrast_on")
                } else {
                    t!("status.contrast_off")
                });
            }
            Action::Help => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.announce(t!("status.help_opened"));
                }
            }
        }
    }

    /// Run an action, or ask about unsaved work first.
    fn guarded(&mut self, pending: Pending) {
        if self.unsaved() {
            self.confirm = Some(pending);
            let message = self.unsaved_status();
            self.announce(message);
        } else {
            self.commit(pending);
        }
    }

    /// Are there unsaved changes in *either* document?
    ///
    /// Both, because both are work. Casting a play is half an hour of choosing
    /// voices, and it is kept in the story file rather than in the screenplay,
    /// so a guard that only asked about the screenplay would let all of it go
    /// on the way out of the door without a word.
    fn unsaved(&self) -> bool {
        self.dirty || self.drama_unsaved()
    }

    fn drama_unsaved(&self) -> bool {
        #[cfg(feature = "drama")]
        {
            self.drama.edited
        }
        #[cfg(not(feature = "drama"))]
        {
            false
        }
    }

    /// Which document the warning is about. Said in as many words rather than
    /// left as "unsaved changes", because the answer decides which file
    /// somebody is about to lose.
    fn unsaved_status(&self) -> String {
        match (self.dirty, self.drama_unsaved()) {
            (true, true) => t!("status.unsaved.both"),
            (false, true) => t!("status.unsaved.drama"),
            _ => t!("status.unsaved"),
        }
    }

    fn unsaved_body(&self) -> String {
        match (self.dirty, self.drama_unsaved()) {
            (true, true) => t!("confirm.body.both"),
            (false, true) => t!("confirm.body.drama"),
            _ => t!("confirm.body"),
        }
    }

    /// Save whatever has not been saved, asking where only if it has never had
    /// a name.
    fn save_unsaved(&mut self) {
        if self.dirty {
            self.save();
        }
        #[cfg(feature = "drama")]
        if self.drama.edited {
            match self.drama.path.clone() {
                Some(path) => self.save_drama_to(path),
                None => self.save_drama_as(),
            }
        }
    }

    fn commit(&mut self, pending: Pending) {
        self.confirm = None;
        match pending {
            Pending::New => {
                let replacing = (self.dirty || self.path.is_some()).then(|| self.path.clone());
                if let Some(replaced) = replacing {
                    self.forget_copy(replaced);
                }
                self.source = String::new();
                self.bible = Bible::default();
                self.bible_sel = None;
                self.bible_remove_armed = None;
                self.path = None;
                self.dirty = false;
                self.needs_reparse = true;
                self.caret = None;
                self.restore_caret = None;
                self.restore_scene = None;
                self.outline_sel = 0;
                self.find_hits.clear();
                self.focus(Pane::Editor);
                self.announce(t!("status.new_screenplay"));
            }
            Pending::Open => self.open_dialog(),
            Pending::Close => {
                // They were asked about the unsaved work and said to close
                // anyway, so the copy standing in for it goes too. Left behind,
                // it would offer back at the next start exactly the draft they
                // had just chosen to abandon.
                self.forget_copy(self.path.clone());
                std::process::exit(0)
            }
        }
    }

    fn set_zoom(&mut self, ctx: &egui::Context, zoom: f32) {
        self.zoom = zoom.clamp(0.7, 3.0);
        ctx.set_zoom_factor(self.zoom);
        self.announce(t!("status.text_size", percent = (self.zoom * 100.0).round() as i32));
    }

    /// Open or close the ideas window. A build without the `ai` feature has no
    /// such window, and says so rather than doing nothing at all.
    #[cfg(feature = "ai")]
    fn toggle_assistant(&mut self, ctx: &egui::Context) {
        if self.show_assistant {
            self.show_assistant = false;
            self.announce(t!("status.ideas_closed"));
        } else {
            self.open_assistant(ctx);
        }
    }

    #[cfg(not(feature = "ai"))]
    fn toggle_assistant(&mut self, _ctx: &egui::Context) {
        self.fail_soft(t!("status.no_model_feature"));
    }

    /// Put a question about a character to a local model, from wherever the
    /// writer happened to be. Without the feature there is nothing to ask.
    fn ask_the_model(&mut self, ctx: &egui::Context, name: &str, speaking: bool) {
        #[cfg(feature = "ai")]
        self.ask_about(ctx, name, speaking);
        #[cfg(not(feature = "ai"))]
        {
            let _ = (ctx, name, speaking);
            self.fail_soft(t!("status.no_model_feature"));
        }
    }

    fn focus(&mut self, pane: Pane) {
        self.pane = pane;
        self.focus_request = Some(pane);
        let label = pane.label();
        let message = match pane {
            Pane::Outline if self.scenes.is_empty() => t!("status.pane.outline_empty", pane = label),
            Pane::Outline => tn!("status.pane.outline", self.scenes.len(), pane = label),
            Pane::Preview => t!(
                "status.pane.preview",
                pane = label,
                pages = a11y::pages_phrase(self.script_pages())
            ),
            Pane::Editor => label,
        };
        self.announce(message);
    }

    fn step_scene(&mut self, delta: i32) {
        if self.scenes.is_empty() {
            self.announce(t!("status.no_scenes"));
            return;
        }
        let last = self.scenes.len() as i32 - 1;
        let next = (self.outline_sel as i32 + delta).clamp(0, last) as usize;
        self.select_scene(next);
    }

    fn select_scene(&mut self, index: usize) {
        let Some((heading, page)) = self.scenes.get(index).cloned() else {
            return;
        };
        self.outline_sel = index;
        self.scroll_to_scene = Some(index);
        self.announce(t!(
            "status.scene",
            index = index + 1,
            total = self.scenes.len(),
            page = page,
            heading = heading
        ));
    }

    fn run_find(&mut self) {
        self.find_hits.clear();
        self.find_at = 0;
        let needle = self.find_query.to_lowercase();
        if needle.is_empty() {
            self.announce(t!("status.nothing_to_find"));
            return;
        }
        let haystack = self.source.to_lowercase();
        let mut from = 0;
        while let Some(hit) = haystack[from..].find(&needle) {
            self.find_hits.push(from + hit);
            from += hit + needle.len();
        }
        // The count, never the phrase: what somebody searched their own script
        // for is theirs.
        log::debug("find", format!("{} matches", self.find_hits.len()));
        match self.find_hits.len() {
            0 => self.fail_soft(t!("status.no_matches", query = self.find_query)),
            n => {
                let query = self.find_query.clone();
                self.show_find = false;
                self.place_caret = true;
                self.focus(Pane::Editor);
                self.announce(tn!("status.matches", n, query = query));
            }
        }
    }

    fn step_find(&mut self, delta: i32) {
        if self.find_hits.is_empty() {
            self.announce(t!("status.nothing_found_yet"));
            return;
        }
        let n = self.find_hits.len() as i32;
        self.find_at = (((self.find_at as i32 + delta) % n + n) % n) as usize;
        self.place_caret = true;
        self.focus(Pane::Editor);
        self.announce(t!("status.match_of", index = self.find_at + 1, total = n));
    }

    /// Put the editor's caret on the current match, and select it.
    fn move_caret_to_match(&mut self, ctx: &egui::Context) {
        self.place_caret = false;
        let Some(&start) = self.find_hits.get(self.find_at) else {
            return;
        };
        let id = Id::new(EDITOR_ID);
        let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) else {
            return;
        };
        let before = self.source[..start].chars().count();
        let length = self.find_query.chars().count();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(before),
            egui::text::CCursor::new(before + length),
        )));
        state.store(ctx, id);
    }

    /// Note where the caret is, so that saving can record it.
    fn track_caret(&mut self, ctx: &egui::Context) {
        let Some(state) = egui::text_edit::TextEditState::load(ctx, Id::new(EDITOR_ID)) else {
            return;
        };
        if let Some(range) = state.cursor.char_range() {
            self.caret = Some(range.primary.index.0);
        }
    }

    /// Restore the caret and outline selection read out of a `.sct` file.
    fn restore_position(&mut self, ctx: &egui::Context) {
        if let Some(scene) = self.restore_scene.take() {
            if scene < self.scenes.len() {
                self.outline_sel = scene;
                self.scroll_to_scene = Some(scene);
            }
        }
        let Some(caret) = self.restore_caret.take() else {
            return;
        };
        let id = Id::new(EDITOR_ID);
        let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) else {
            // The editor has not been drawn yet; try again next frame.
            self.restore_caret = Some(caret);
            return;
        };
        // A caret past the end of a shortened screenplay lands at the end.
        let caret = caret.min(self.source.chars().count());
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
        state.store(ctx, id);
        self.caret = Some(caret);
    }

    // -- panes --------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut Ui) -> Option<Action> {
        let mut chosen = None;
        egui::Panel::top(Id::new("openwrite-menu")).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(t!("menu.file"), |ui| {
                    for action in [
                        Action::New,
                        Action::Open,
                        Action::Save,
                        Action::SaveAs,
                        Action::Export,
                        Action::CopyFormatted,
                    ] {
                        if self.menu_item(ui, action) {
                            chosen = Some(action);
                        }
                    }
                });
                ui.menu_button(t!("menu.edit"), |ui| {
                    for action in [Action::Find, Action::FindNext, Action::FindPrevious] {
                        if self.menu_item(ui, action) {
                            chosen = Some(action);
                        }
                    }
                });
                ui.menu_button(t!("menu.story"), |ui| {
                    if self.menu_item(ui, Action::Characters) {
                        chosen = Some(Action::Characters);
                    }
                    if cfg!(feature = "ai") && self.menu_item(ui, Action::Assist) {
                        chosen = Some(Action::Assist);
                    }
                    #[cfg(feature = "drama")]
                    if self.menu_item(ui, Action::AudioDrama) {
                        chosen = Some(Action::AudioDrama);
                    }
                });
                // Its own menu rather than a corner of Story, because it is a
                // second thing to do with a script rather than another note
                // about one — and because it has commands of its own that want
                // somewhere to live.
                #[cfg(feature = "drama")]
                ui.menu_button(t!("menu.audio_drama"), |ui| {
                    if self.menu_item(ui, Action::AudioDrama) {
                        chosen = Some(Action::AudioDrama);
                    }
                    if self.plain_item(ui, t!("menu.drama.screenplay"), true) {
                        chosen = Some(Action::Screenplay);
                    }
                    ui.separator();
                    if self.plain_item(ui, t!("drama.open"), true) {
                        chosen = Some(Action::DramaOpen);
                    }
                    let opened = self.drama.path.is_some() && !self.drama.story.is_empty();
                    if self.plain_item(ui, t!("menu.drama.save"), opened) {
                        chosen = Some(Action::DramaSave);
                    }
                    if self.plain_item(ui, t!("menu.drama.save_as"), !self.drama.story.is_empty())
                    {
                        chosen = Some(Action::DramaSaveAs);
                    }
                    ui.separator();
                    if self.plain_item(ui, t!("menu.drama.record"), self.drama.can_record()) {
                        chosen = Some(Action::DramaRecord);
                    }
                    if self.plain_item(ui, t!("drama.stop"), self.drama.is_recording()) {
                        chosen = Some(Action::DramaStop);
                    }
                });
                ui.menu_button(t!("menu.view"), |ui| {
                    for action in [
                        Action::ToggleOutline,
                        Action::TogglePreview,
                        Action::ToggleContrast,
                        Action::ZoomIn,
                        Action::ZoomOut,
                        Action::ZoomReset,
                    ] {
                        if self.menu_item(ui, action) {
                            chosen = Some(action);
                        }
                    }
                    ui.separator();
                    ui.label(t!("view.colour_scheme"));
                    for scheme in [Scheme::System, Scheme::Light, Scheme::Dark] {
                        let response = ui.radio_value(&mut self.scheme, scheme, scheme.label());
                        if response.changed() {
                            self.status = t!("status.colour_scheme", name = scheme.label());
                        }
                    }
                    ui.separator();
                    // Not a shortcut, so it is not in the bindings table: a
                    // setting somebody changes once does not need a key of its
                    // own, and the ones it would have to take are all spoken for.
                    if ui.button(t!("menu.language")).clicked() {
                        ui.close();
                        chosen = Some(Action::Language);
                    }
                });
                ui.menu_button(t!("menu.navigate"), |ui| {
                    for action in [
                        Action::NextScene,
                        Action::PreviousScene,
                        Action::FocusOutline,
                        Action::FocusEditor,
                        Action::FocusPreview,
                    ] {
                        if self.menu_item(ui, action) {
                            chosen = Some(action);
                        }
                    }
                });
                ui.menu_button(t!("menu.help"), |ui| {
                    for action in [Action::Help, Action::DebugLog] {
                        if self.menu_item(ui, action) {
                            chosen = Some(action);
                        }
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Whichever document the window is showing, and whether it
                    // has unsaved changes.
                    let (title, edited) = match self.workspace {
                        Workspace::Screenplay => (
                            self.path
                                .as_deref()
                                .map(file_label)
                                .unwrap_or_else(|| t!("file.untitled")),
                            self.dirty,
                        ),
                        #[cfg(feature = "drama")]
                        Workspace::AudioDrama => (
                            self.drama
                                .path
                                .as_deref()
                                .map(file_label)
                                .unwrap_or_else(|| t!("drama.file.untitled")),
                            self.drama.edited,
                        ),
                    };
                    let palette = theme::palette(ui.visuals());
                    if edited {
                        ui.label(
                            RichText::new(t!("menu.edited", name = title)).color(palette.warn),
                        );
                    } else {
                        ui.label(RichText::new(title).color(palette.muted));
                    }
                });
            });
        });
        chosen
    }

    /// Switch tab, and say so.
    fn show_workspace(&mut self, workspace: Workspace) {
        if self.workspace == workspace {
            return;
        }
        self.workspace = workspace;
        self.announce(t!("status.tab", name = workspace.label()));
        if workspace == Workspace::Screenplay {
            self.focus_request = Some(Pane::Editor);
        }
    }

    /// A menu entry that has no shortcut, and may be unavailable.
    ///
    /// [`Self::menu_item`] takes its words from the shortcut table, which is
    /// what keeps the menus and the help window from drifting apart. These
    /// have no key of their own — the ones they would want are all spoken for —
    /// so they carry their own words, as the language entry in the View menu
    /// already does.
    fn plain_item(&self, ui: &mut Ui, label: String, enabled: bool) -> bool {
        let response = ui.add_enabled(enabled, egui::Button::new(label));
        if response.clicked() {
            ui.close();
            return true;
        }
        false
    }

    fn menu_item(&self, ui: &mut Ui, action: Action) -> bool {
        let binding = self.bindings.iter().find(|b| b.action == action);
        let text = binding.map(Binding::label).unwrap_or_default();
        let keys = binding
            .map(|b| ui.ctx().format_shortcut(&b.shortcut))
            .unwrap_or_default();
        let response = ui.add(egui::Button::new(text).shortcut_text(keys));
        if response.clicked() {
            ui.close();
            return true;
        }
        false
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        egui::Panel::bottom(Id::new("openwrite-status")).show(ui, |ui| {
            ui.horizontal(|ui| {
                // Counting pages while the audio drama is on screen would be
                // counting the wrong document.
                let summary = match self.workspace {
                    Workspace::Screenplay => t!(
                        "status.summary",
                        pages = a11y::pages_phrase(self.script_pages()),
                        scenes = tn!("phrase.scenes", self.stats.scenes),
                        words = tn!("phrase.words", self.stats.words)
                    ),
                    #[cfg(feature = "drama")]
                    Workspace::AudioDrama => t!(
                        "drama.summary",
                        lines = tn!("drama.phrase.lines", self.drama.story.lines.len()),
                        cast = tn!("drama.phrase.cast", self.drama.story.voices.len())
                    ),
                };
                let response = ui.label(RichText::new(&summary).monospace());
                a11y::name(&response, t!("a11y.summary", summary = summary));

                ui.separator();

                // The live region: how the application reports what it just did
                // to somebody who cannot see the screen.
                let palette = theme::palette(ui.visuals());
                let colour = match self.tone {
                    Tone::Info => palette.muted,
                    Tone::Good => palette.ok,
                    Tone::Bad => palette.bad,
                };
                let status = ui.label(RichText::new(&self.status).color(colour));
                a11y::name(&status, self.status.clone());
                a11y::live_region(&status);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let hint = t!(
                        "status.help_hint",
                        keys = shortcuts::label(ui.ctx(), &self.bindings, Action::Help)
                    );
                    ui.label(RichText::new(hint).color(palette.muted));
                });
            });
        });
    }

    fn outline_panel(&mut self, ui: &mut Ui) {
        egui::Panel::left(Id::new("openwrite-outline"))
            .resizable(true)
            .default_size(260.0)
            .show(ui, |ui| {
                ui.heading(t!("outline.heading"));
                ui.separator();
                if self.scenes.is_empty() {
                    ui.label(t!("outline.empty"));
                    ui.label(RichText::new(t!("outline.empty.hint")).weak().small());
                    return;
                }
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let total = self.scenes.len();
                        let wanted = self.focus_request == Some(Pane::Outline);
                        let selected_index = self.outline_sel;
                        let mut clicked = None;

                        for (i, (heading, page)) in self.scenes.iter().enumerate() {
                            let selected = i == selected_index;
                            let response = ui.selectable_label(
                                selected,
                                RichText::new(format!("{:>3}. {heading}", i + 1)).monospace(),
                            );
                            a11y::name(
                                &response,
                                t!(
                                    "a11y.scene",
                                    index = i + 1,
                                    total = total,
                                    heading = heading
                                ),
                            );
                            a11y::describe(&response, t!("a11y.page", page = page));
                            if wanted && selected {
                                response.request_focus();
                                response.scroll_to_me(Some(Align::Center));
                            }
                            if response.clicked() {
                                clicked = Some(i);
                            }
                        }
                        if let Some(i) = clicked {
                            self.select_scene(i);
                        }
                    });
            });
    }

    fn preview_panel(&mut self, ui: &mut Ui) {
        let scroll_to = self.scroll_to_scene.take();
        egui::Panel::right(Id::new("openwrite-preview"))
            .resizable(true)
            .default_size(560.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(t!("preview.heading"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(a11y::pages_phrase(self.script_pages()))
                                .weak()
                                .small(),
                        );
                    });
                });
                ui.separator();

                let row = ui.text_style_height(&TextStyle::Monospace);
                let page_height = row * (self.opts.lines_per_page as f32 + 2.0) + 40.0;
                let count = self.page_text.len().max(1);
                let pane_rect = ui.available_rect_before_wrap();

                let mut area = ScrollArea::vertical().auto_shrink([false, false]);
                if let Some(scene) = scroll_to {
                    // Page numbers count the script only; the title page sits
                    // in front of it.
                    let offset = self
                        .scenes
                        .get(scene)
                        .map(|(_, page)| {
                            let title_pages = self.pages.len() - self.script_pages();
                            (page.saturating_sub(1) + title_pages) as f32 * page_height
                        })
                        .unwrap_or(0.0);
                    area = area.vertical_scroll_offset(offset);
                }

                area.show_rows(ui, page_height, count, |ui, range| {
                    for index in range {
                        self.page_ui(ui, index, page_height);
                    }
                });

                // Let the pane itself take keyboard focus, so the preview can be
                // reached and read without a pointer.
                let response = ui.interact(
                    pane_rect,
                    Id::new(PREVIEW_ID),
                    egui::Sense::focusable_noninteractive(),
                );
                a11y::name(&response, t!("pane.preview"));
                a11y::describe(&response, t!("a11y.preview_hint"));
                if self.focus_request == Some(Pane::Preview) {
                    response.request_focus();
                }
            });
    }

    fn page_ui(&self, ui: &mut Ui, index: usize, height: f32) {
        let Some(text) = self.page_text.get(index) else {
            return;
        };
        let label = match self.pages[index].number {
            Some(n) => t!("preview.page", page = n),
            None => t!("preview.title_page"),
        };
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_min_height(height - 28.0);
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&label).weak().small());
                let body = ui.add(
                    egui::Label::new(RichText::new(text).monospace())
                        .selectable(true)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
                // Screen readers read the page as one block of script, with the
                // page number in front of it.
                a11y::name(&body, t!("a11y.page_body", label = label, text = text));
            });
    }

    fn editor_panel(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(t!("editor.heading"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let response = ui.label(RichText::new(t!("editor.markup")).weak().small());
                    response.on_hover_text(t!("editor.markup.hint"));
                });
            });
            ui.separator();

            let wanted = self.focus_request == Some(Pane::Editor);
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let size = ui.available_size();
                    let response = ui.add_sized(
                        size,
                        egui::TextEdit::multiline(&mut self.source)
                            .id(Id::new(EDITOR_ID))
                            .code_editor()
                            .lock_focus(true)
                            .hint_text(t!("editor.hint")),
                    );

                    a11y::name(&response, t!("pane.editor"));
                    a11y::describe(&response, t!("a11y.editor_hint"));
                    if response.changed() {
                        self.dirty = true;
                        self.needs_reparse = true;
                    }
                    if wanted {
                        response.request_focus();
                    }
                });
        });
    }

    // -- dialogs ------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        // First: it is a question about work that is not on screen, and every
        // other dialog is about work that is.
        self.recovery_dialog(ctx);
        #[cfg(feature = "update")]
        self.update_dialog(ctx);
        self.debug_log_dialog(ctx);
        self.language_dialog(ctx);
        self.bible_dialog(ctx);
        #[cfg(feature = "ai")]
        self.assistant_dialog(ctx);
        self.find_dialog(ctx);
        self.help_dialog(ctx);
        self.confirm_dialog(ctx);
        self.error_dialog(ctx);
    }

    fn find_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_find {
            return;
        }
        let mut keep_open = true;
        let mut search = false;
        let mut close = false;
        egui::Window::new(t!("find.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 60.0])
            .show(ctx, |ui| {
                ui.label(t!("find.label"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.find_query)
                        .id(Id::new(FIND_ID))
                        .desired_width(300.0)
                        .hint_text(t!("find.hint")),
                );
                a11y::name(&field, t!("find.label"));
                if self.focus_find {
                    field.request_focus();
                    self.focus_find = false;
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    search = true;
                }
                ui.horizontal(|ui| {
                    if ui.button(t!("find.go")).clicked() {
                        search = true;
                    }
                    if ui.button(t!("button.close")).clicked() {
                        close = true;
                    }
                });
            });

        if search {
            self.run_find();
        }
        if close || !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_find = false;
            self.focus_request = Some(Pane::Editor);
        }
    }

    fn help_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut keep_open = true;
        egui::Window::new(t!("help.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .default_width(540.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                    let mut group = "";
                    for binding in &self.bindings {
                        if binding.group != group {
                            if !group.is_empty() {
                                ui.add_space(10.0);
                            }
                            ui.label(RichText::new(binding.group_label()).strong());
                            group = binding.group;
                        }
                        ui.horizontal(|ui| {
                            let keys = ui.ctx().format_shortcut(&binding.shortcut);
                            ui.label(RichText::new(format!("{keys:<16}")).monospace());
                            ui.label(binding.label());
                        });
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new(t!("group.writing")).strong());
                    for (markup, what) in MARKUP {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{markup:<22}")).monospace());
                            ui.label(crate::i18n::text(what, &[]));
                        });
                    }
                    ui.label(RichText::new(t!("help.markup.note")).weak().small());

                    ui.add_space(10.0);
                    ui.label(RichText::new(t!("group.within_pane")).strong());
                    for (keys, what) in [
                        (t!("help.key.tab"), t!("help.within.tab")),
                        (t!("help.key.arrows"), t!("help.within.arrows")),
                        (t!("help.key.return"), t!("help.within.return")),
                        (t!("help.key.escape"), t!("help.within.escape")),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{keys:<16}")).monospace());
                            ui.label(what);
                        });
                    }
                });
            });
        if !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_help = false;
            self.announce(t!("status.help_closed"));
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.confirm else {
            return;
        };
        let mut decision: Option<Decision> = None;
        egui::Window::new(t!("confirm.title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(self.unsaved_body());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("button.save")).clicked() {
                        decision = Some(Decision::Save);
                    }
                    if ui.button(t!("button.discard")).clicked() {
                        decision = Some(Decision::Discard);
                    }
                    if ui.button(t!("button.cancel")).clicked() {
                        decision = Some(Decision::Cancel);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            decision = Some(Decision::Cancel);
        }
        match decision {
            Some(Decision::Save) => {
                self.save_unsaved();
                // Only go on if it worked: a cancelled Save-as leaves the work
                // exactly where it was, and closing anyway would throw away
                // what was just being rescued.
                if !self.unsaved() {
                    self.commit(pending);
                }
            }
            Some(Decision::Discard) => {
                self.dirty = false;
                #[cfg(feature = "drama")]
                {
                    self.drama.edited = false;
                }
                self.commit(pending);
            }
            Some(Decision::Cancel) => {
                self.confirm = None;
                self.announce(t!("status.cancelled"));
            }
            None => {}
        }
    }

    fn error_dialog(&mut self, ctx: &egui::Context) {
        let Some(message) = self.error.clone() else {
            return;
        };
        let mut keep_open = true;
        let mut dismissed = false;
        egui::Window::new(t!("error.title"))
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let palette = theme::palette(ui.visuals());
                ui.label(RichText::new(&message).color(palette.bad));
                if ui.button(t!("button.ok")).clicked() {
                    dismissed = true;
                }
            });
        if dismissed || !keep_open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.error = None;
        }
    }

    /// Arrow-key handling for whichever pane holds focus, and the bookkeeping
    /// that keeps `self.pane` honest when focus moves by Tab or by mouse.
    fn pane_keys(&mut self, ctx: &egui::Context) {
        if self.show_help || self.show_find || self.show_bible || self.show_log
            || self.show_language
            || self.confirm.is_some()
            || self.error.is_some()
        {
            return;
        }
        #[cfg(feature = "ai")]
        if self.show_assistant {
            return;
        }
        // The other workspace has no panes to walk, and its own fields would
        // otherwise eat the arrow keys.
        if self.workspace != Workspace::Screenplay {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        if focused == Some(Id::new(EDITOR_ID)) {
            self.pane = Pane::Editor;
            return;
        }
        if focused == Some(Id::new(PREVIEW_ID)) {
            self.pane = Pane::Preview;
            return;
        }
        // The outline's items are the only other focusable things that matter
        // here; egui gives them generated ids, so ask whether anything in the
        // outline panel has focus by checking against the selection instead.
        if self.show_outline && focused.is_some() && self.pane == Pane::Outline {
            let (down, up) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                )
            });
            if down {
                self.step_scene(1);
                self.focus_request = Some(Pane::Outline);
            } else if up {
                self.step_scene(-1);
                self.focus_request = Some(Pane::Outline);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Decision {
    Save,
    Discard,
    Cancel,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if self.restyle {
            theme::apply(&ctx, self.high_contrast);
            self.restyle = false;
        }
        ctx.set_theme(self.scheme.preference());

        if self.needs_reparse {
            self.reparse();
        }
        // After the window exists, so nothing waits on a network round trip to
        // see a screenplay.
        #[cfg(feature = "update")]
        self.check_for_update(&ctx);

        self.keep_a_copy(&ctx);

        for action in shortcuts::triggered(&ctx, &self.bindings) {
            self.perform(action, &ctx);
        }
        self.pane_keys(&ctx);

        // The window's own close button has to go through the unsaved-changes
        // question too, or the one route out of the app that needs no keyboard
        // is also the one that loses work.
        if ctx.input(|i| i.viewport().close_requested()) && self.unsaved() && self.confirm.is_none()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.guarded(Pending::Close);
        }

        if let Some(action) = self.menu_bar(ui) {
            self.perform(action, &ctx);
        }
        self.status_bar(ui);
        match self.workspace {
            Workspace::Screenplay => {
                if self.show_outline {
                    self.outline_panel(ui);
                }
                if self.show_preview {
                    self.preview_panel(ui);
                }
                self.editor_panel(ui);
            }
            #[cfg(feature = "drama")]
            Workspace::AudioDrama => self.drama_panel(ui, &ctx),
        }
        self.dialogs(&ctx);

        if self.place_caret {
            self.move_caret_to_match(&ctx);
        }
        self.restore_position(&ctx);
        self.track_caret(&ctx);
        self.focus_request = None;

        // The program's own name is not translated — it is a name — but
        // everything around it is.
        let name = match self.workspace {
            Workspace::Screenplay => match &self.path {
                Some(path) => file_label(path),
                None => t!("file.untitled"),
            },
            // The window is showing the story file, so that is the document it
            // should be named after.
            #[cfg(feature = "drama")]
            Workspace::AudioDrama => match &self.drama.path {
                Some(path) => file_label(path),
                None => t!("drama.file.untitled"),
            },
        };
        let edited = match self.workspace {
            Workspace::Screenplay => self.dirty,
            #[cfg(feature = "drama")]
            Workspace::AudioDrama => self.drama.edited,
        };
        let title = if edited {
            t!("window.title.edited", name = name, app = APP_NAME)
        } else {
            t!("window.title", name = name, app = APP_NAME)
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// The markup, as the help window lists it.
///
/// Kept beside the shortcut table for the same reason that table exists: one
/// place to change when the parser learns something, rather than a help page
/// that quietly stops being true.
///
/// The left column is markup, which is the same in every language; the right
/// column is a language key, looked up when the help window is drawn.
pub(crate) const MARKUP: [(&str, &str); 8] = [
    ("INT. / EXT. ...", "help.markup.scene"),
    (".FORCED HEADING", "help.markup.forced"),
    ("MAYA", "help.markup.cue"),
    ("MAYA: Forty-one.", "help.markup.one_line"),
    ("MAYA (quietly): No.", "help.markup.parenthetical"),
    ("/transition:cut to", "help.markup.transition"),
    ("# Act One", "help.markup.section"),
    ("= She says it.", "help.markup.synopsis"),
];

/// What a new screenplay starts as.
///
/// It is a page of working markup rather than an empty box, because the two
/// ways of writing dialogue are the only thing about this editor somebody has
/// to be told, and showing them costs eleven lines they can type over.
///
/// It comes out of the language file, so a writer working in their own
/// language is not handed an English page to type over — but the markup in it
/// (`INT.`, the colon form, `/transition:`) is what the parser reads and stays
/// as it is, whatever language surrounds it.
fn welcome() -> String {
    t!("welcome.screenplay")
}
