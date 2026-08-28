//! The two things the editor has to remember between runs.
//!
//! Almost nothing here is a setting, on purpose: a screenplay carries its own
//! state in its own file (see [`crate::document`]), and an editor that
//! remembers which panes were open is an editor that eventually opens wrong.
//! There are two exceptions, and both earn it. The language, because somebody
//! who has chosen to work in their own language should not have to choose
//! again every morning. And the ElevenLabs key, because a key that had to be
//! pasted in again before every recording would not be worth having.
//!
//! The file is written only when something changes, and a file that cannot be
//! read is not an error worth interrupting anybody over: the defaults are
//! perfectly usable, so a bad line goes to the debug log and the editor opens.
//!
//! # One of these is a secret
//!
//! The ElevenLabs key is here because that is where it was asked to go, and it
//! is worth being plain about what that means: this is an unencrypted text
//! file, and anything in it is readable by anything that can read the file. So
//! the file is written with owner-only permissions where the platform has
//! them, the key is never written to the debug log, and the file says so at
//! the top of itself for whoever opens it later. Setting
//! `OPENWRITE_ELEVENLABS_KEY` in the environment overrides it and is not
//! written down at all, for anybody who would rather it were not.

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

/// Overrides the saved ElevenLabs key, and is not saved anywhere.
pub const KEY_ENV: &str = "OPENWRITE_ELEVENLABS_KEY";

/// Everything the editor remembers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// A language code like `fr`, or [`crate::i18n::AUTO`] for "ask the
    /// computer".
    pub language: String,
    /// The ElevenLabs API key, for the audio drama. Empty until somebody sets
    /// one. See the note at the top of this file about what storing it here
    /// does and does not protect.
    pub elevenlabs_key: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            language: crate::i18n::AUTO.to_string(),
            elevenlabs_key: String::new(),
        }
    }
}

impl Settings {
    /// The key actually in use: the environment first, then the file.
    ///
    /// The environment wins so that somebody who does not want a key on their
    /// disk has a way of not having one, and so that a shared machine can set
    /// it per session.
    pub fn key(&self) -> String {
        match std::env::var(KEY_ENV) {
            Ok(key) if !key.trim().is_empty() => key.trim().to_string(),
            _ => self.elevenlabs_key.trim().to_string(),
        }
    }

    /// Whether the key in use came from the environment rather than the file.
    pub fn key_from_environment() -> bool {
        std::env::var(KEY_ENV).is_ok_and(|key| !key.trim().is_empty())
    }

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
        // Written owner-only, and the permissions are set as the file is made
        // rather than after: there is a key in here, and a `write` followed by
        // a `chmod` leaves a moment in which anybody could read it.
        match write_private(&path, self.to_text().as_bytes()) {
            // The key itself never appears in the log — only whether there is
            // one, which is the part that helps when something is wrong.
            Ok(()) => crate::log::info(
                "settings",
                format!(
                    "saved, language {}, ElevenLabs key {}",
                    self.language,
                    if self.elevenlabs_key.trim().is_empty() { "not set" } else { "set" }
                ),
            ),
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
            match key.trim() {
                "language" if !value.is_empty() => settings.language = value.to_string(),
                "elevenlabs_key" => settings.elevenlabs_key = value.to_string(),
                _ => {}
            }
        }
        settings
    }

    fn to_text(&self) -> String {
        format!(
            "# Screenplay Creation Tool settings.\n\
             #\n\
             # KEEP THIS FILE TO YOURSELF. The ElevenLabs key below is not\n\
             # encrypted and anyone who can read this file can spend it. Do not\n\
             # copy it to another machine, into a repository, or into a bug\n\
             # report. To keep the key out of a file altogether, clear it here\n\
             # and set OPENWRITE_ELEVENLABS_KEY in your environment instead —\n\
             # that takes precedence over this and is never written down.\n\
             #\n\
             # language: a code like \"fr\", or \"auto\" for whatever this computer\n\
             # is set to. The languages themselves are files in the `languages`\n\
             # folder beside this one.\n\
             language = \"{}\"\n\
             #\n\
             # elevenlabs_key: for the Audio Drama tab. Empty means the tab asks\n\
             # for one.\n\
             elevenlabs_key = \"{}\"\n",
            self.language,
            self.elevenlabs_key.trim()
        )
    }
}

/// Write a file only its owner can read. See the note at the top of this file.
fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_here_reads_back_the_same() {
        let settings = Settings {
            language: "fr".to_string(),
            elevenlabs_key: "sk_abc123".to_string(),
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

    /// The warning is the point of it: whoever opens this file next should not
    /// have to guess that one of the lines is worth money.
    #[test]
    fn the_file_warns_about_the_key_it_carries() {
        let text = Settings::default().to_text();
        assert!(text.contains("KEEP THIS FILE TO YOURSELF"));
        assert!(text.contains(KEY_ENV));
    }

    #[test]
    fn quotes_around_the_value_are_optional() {
        assert_eq!(Settings::parse("language = fr\n").language, "fr");
        assert_eq!(Settings::parse("language = \"pt-BR\"\n").language, "pt-BR");
    }
}
