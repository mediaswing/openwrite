//! The one or two things the editor has to remember between runs.
//!
//! Almost nothing here is a setting, on purpose: a screenplay carries its own
//! state in its own file (see [`crate::document`]), and an editor that
//! remembers which panes were open is an editor that eventually opens wrong.
//! The language is the exception, and the reason is plain — somebody who has
//! chosen to work in their own language should not have to choose again every
//! morning.
//!
//! The file is written only when something changes, and a file that cannot be
//! read is not an error worth interrupting anybody over: the defaults are
//! perfectly usable, so a bad line goes to the debug log and the editor opens.

use std::path::PathBuf;

/// What the file is called, inside [`config_dir`].
const FILE: &str = "settings.toml";

/// Where the settings and the languages folder live.
///
/// Worked out by hand rather than with a crate, because this program has two
/// dependencies and neither of them is for finding a directory. The three
/// answers are the conventional ones: Application Support on macOS, `%APPDATA%`
/// on Windows, and `$XDG_CONFIG_HOME` — or `~/.config` — everywhere else.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("openwrite"),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("openwrite"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
            if !base.is_empty() {
                return Some(PathBuf::from(base).join("openwrite"));
            }
        }
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".config").join("openwrite"))
    }
}

/// Everything the editor remembers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// A language code like `fr`, or [`crate::i18n::AUTO`] for "ask the
    /// computer".
    pub language: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            language: crate::i18n::AUTO.to_string(),
        }
    }
}

impl Settings {
    /// Reads the settings file, falling back to the defaults for anything it
    /// does not say.
    pub fn load() -> Settings {
        let path = match config_dir() {
            Some(dir) => dir.join(FILE),
            None => return Settings::default(),
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // Not having settings yet is the ordinary case for a first run.
            Err(_) => return Settings::default(),
        };
        Settings::parse(&text)
    }

    /// Writes the settings file, creating the folder if it is not there.
    ///
    /// Reports through the log rather than through the interface: this is
    /// called when the writer changes a setting, and the setting has already
    /// taken effect by then. Telling them it will not survive a restart is
    /// worth a log line, not a dialog in front of the screenplay.
    pub fn save(&self) {
        let dir = match config_dir() {
            Some(dir) => dir,
            None => {
                crate::log::warn("settings", "there is no folder to save settings in");
                return;
            }
        };
        if let Err(err) = std::fs::create_dir_all(&dir) {
            crate::log::warn("settings", format!("{} could not be made: {err}", dir.display()));
            return;
        }
        let path = dir.join(FILE);
        match std::fs::write(&path, self.to_text()) {
            Ok(()) => crate::log::info("settings", format!("saved, language {}", self.language)),
            Err(err) => crate::log::warn("settings", format!("could not be saved: {err}")),
        }
    }

    fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = match line.split_once('=') {
                Some(pair) => pair,
                None => continue,
            };
            let value = value.trim().trim_matches('"').trim();
            if key.trim() == "language" && !value.is_empty() {
                settings.language = value.to_string();
            }
        }
        settings
    }

    fn to_text(&self) -> String {
        format!(
            "# Screenplay Creation Tool settings.\n\
             #\n\
             # language: a code like \"fr\", or \"auto\" for whatever this computer\n\
             # is set to. The languages themselves are files in the `languages`\n\
             # folder beside this one.\n\
             language = \"{}\"\n",
            self.language
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_here_reads_back_the_same() {
        let settings = Settings {
            language: "fr".to_string(),
        };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
    }

    #[test]
    fn an_empty_or_broken_file_leaves_the_defaults_standing() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("nonsense\n"), Settings::default());
        assert_eq!(Settings::parse("language =\n"), Settings::default());
        // And an entry this version does not know is simply ignored.
        assert_eq!(Settings::parse("colour = \"green\"\n"), Settings::default());
    }

    #[test]
    fn quotes_around_the_value_are_optional() {
        assert_eq!(Settings::parse("language = fr\n").language, "fr");
        assert_eq!(Settings::parse("language = \"pt-BR\"\n").language, "pt-BR");
    }
}
