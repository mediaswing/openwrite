//! The keyboard shortcut table.
//!
//! Every command in the application is reachable from here, and the help
//! window is generated from this same table, so the two can never drift apart.
//!
//! The words are not here. `group` and `description` are keys into the
//! language files (see [`crate::i18n`]), looked up when the menu is drawn, so
//! that the shortcut list is in the writer's language too.

use eframe::egui::{Context, Key, KeyboardShortcut, Modifiers};

/// Everything the user can ask the application to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Export,
    CopyFormatted,
    Find,
    FindNext,
    FindPrevious,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FocusOutline,
    FocusEditor,
    FocusPreview,
    CyclePane,
    CyclePaneBack,
    NextScene,
    PreviousScene,
    /// The characters and world window.
    Characters,
    /// Ask a local model what happens next.
    Assist,
    ToggleOutline,
    TogglePreview,
    ToggleContrast,
    /// The language the interface is written in.
    Language,
    Help,
    /// What the program has been doing, for when something is wrong.
    DebugLog,
    Close,
}

pub struct Binding {
    pub action: Action,
    pub shortcut: KeyboardShortcut,
    /// Which group it appears under in the help window, as a language key.
    pub group: &'static str,
    /// What the command does, as a language key.
    pub description: &'static str,
}

impl Binding {
    /// What this command is called, in the language in use.
    pub fn label(&self) -> String {
        crate::i18n::text(self.description, &[])
    }

    /// The heading it sits under in the help window.
    pub fn group_label(&self) -> String {
        crate::i18n::text(self.group, &[])
    }
}

const fn key(modifiers: Modifiers, logical_key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, logical_key)
}

/// `Cmd` on macOS, `Ctrl` everywhere else.
const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};
const NONE: Modifiers = Modifiers::NONE;
const SHIFT: Modifiers = Modifiers::SHIFT;

pub fn bindings() -> Vec<Binding> {
    use Action::*;
    vec![
        Binding { action: New, shortcut: key(CMD, Key::N), group: "group.file", description: "shortcut.new" },
        Binding { action: Open, shortcut: key(CMD, Key::O), group: "group.file", description: "shortcut.open" },
        Binding { action: Save, shortcut: key(CMD, Key::S), group: "group.file", description: "shortcut.save" },
        Binding { action: SaveAs, shortcut: key(CMD_SHIFT, Key::S), group: "group.file", description: "shortcut.save_as" },
        Binding { action: Export, shortcut: key(CMD, Key::E), group: "group.file", description: "shortcut.export" },
        Binding { action: CopyFormatted, shortcut: key(CMD_SHIFT, Key::C), group: "group.file", description: "shortcut.copy_formatted" },
        Binding { action: Close, shortcut: key(CMD, Key::W), group: "group.file", description: "shortcut.close" },

        Binding { action: Characters, shortcut: key(CMD, Key::K), group: "group.story", description: "shortcut.characters" },
        // The one command that reaches outside the program gets a binding only
        // in a build that has it.
        #[cfg(feature = "ai")]
        Binding { action: Assist, shortcut: key(CMD, Key::I), group: "group.story", description: "shortcut.assist" },

        Binding { action: Find, shortcut: key(CMD, Key::F), group: "group.edit", description: "shortcut.find" },
        Binding { action: FindNext, shortcut: key(CMD, Key::G), group: "group.edit", description: "shortcut.find_next" },
        Binding { action: FindPrevious, shortcut: key(CMD_SHIFT, Key::G), group: "group.edit", description: "shortcut.find_previous" },

        Binding { action: FocusOutline, shortcut: key(CMD, Key::Num1), group: "group.navigate", description: "shortcut.focus_outline" },
        Binding { action: FocusEditor, shortcut: key(CMD, Key::Num2), group: "group.navigate", description: "shortcut.focus_editor" },
        Binding { action: FocusPreview, shortcut: key(CMD, Key::Num3), group: "group.navigate", description: "shortcut.focus_preview" },
        Binding { action: CyclePane, shortcut: key(NONE, Key::F6), group: "group.navigate", description: "shortcut.cycle_pane" },
        Binding { action: CyclePaneBack, shortcut: key(SHIFT, Key::F6), group: "group.navigate", description: "shortcut.cycle_pane_back" },
        Binding { action: NextScene, shortcut: key(CMD, Key::CloseBracket), group: "group.navigate", description: "shortcut.next_scene" },
        Binding { action: PreviousScene, shortcut: key(CMD, Key::OpenBracket), group: "group.navigate", description: "shortcut.previous_scene" },

        Binding { action: ZoomIn, shortcut: key(CMD, Key::Plus), group: "group.view", description: "shortcut.zoom_in" },
        Binding { action: ZoomOut, shortcut: key(CMD, Key::Minus), group: "group.view", description: "shortcut.zoom_out" },
        Binding { action: ZoomReset, shortcut: key(CMD, Key::Num0), group: "group.view", description: "shortcut.zoom_reset" },
        Binding { action: ToggleOutline, shortcut: key(CMD_SHIFT, Key::O), group: "group.view", description: "shortcut.toggle_outline" },
        Binding { action: TogglePreview, shortcut: key(CMD_SHIFT, Key::P), group: "group.view", description: "shortcut.toggle_preview" },
        Binding { action: ToggleContrast, shortcut: key(CMD_SHIFT, Key::H), group: "group.view", description: "shortcut.toggle_contrast" },

        Binding { action: Help, shortcut: key(NONE, Key::F1), group: "group.help", description: "shortcut.help" },
        Binding { action: DebugLog, shortcut: key(CMD_SHIFT, Key::L), group: "group.help", description: "shortcut.debug_log" },
    ]
}

/// Consume any shortcut pressed this frame.
///
/// Consuming matters: it stops the key also reaching the text editor, so
/// typing `[` in dialogue never jumps scene.
pub fn triggered(ctx: &Context, bindings: &[Binding]) -> Vec<Action> {
    let mut fired = Vec::new();
    ctx.input_mut(|input| {
        for binding in bindings {
            if input.consume_shortcut(&binding.shortcut) {
                fired.push(binding.action);
            }
        }
        // `Cmd =` is what an unshifted `Cmd +` actually reports on most layouts.
        if input.consume_shortcut(&key(CMD, Key::Equals)) {
            fired.push(Action::ZoomIn);
        }
    });
    fired
}

/// The shortcut for an action, formatted for the current platform.
pub fn label(ctx: &Context, bindings: &[Binding], action: Action) -> String {
    bindings
        .iter()
        .find(|b| b.action == action)
        .map(|b| ctx.format_shortcut(&b.shortcut))
        .unwrap_or_default()
}
