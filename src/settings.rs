//! The three things the editor has to remember between runs.
//!
//! Almost nothing here is a setting, on purpose: a screenplay carries its own
//! state in its own file (see [`crate::document`]), and an editor that
//! remembers which panes were open is an editor that eventually opens wrong.
//! There are three exceptions, and each earns it. The language, because
//! somebody who has chosen to work in their own language should not have to
//! choose again every morning. The ElevenLabs key, because a key that had to be
//! pasted in again before every recording would not be worth having. And
//! whether to check for a newer version, because that is the one thing this
//! program does over the network without being asked, and an answer of no has
//! to survive the restart it is given in.
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
    /// Whether to ask GitHub at startup if there is a newer version.
    ///
    /// On by default, and turned off from the dialog that does the asking —
    /// [`crate::update`] is the one thing this program does over the network
    /// without being asked, so refusing it should not require knowing that an
    /// environment variable exists. `OPENWRITE_NO_UPDATE_CHECK` still overrides
    /// this, for a machine where the answer has to be no whatever the file says.
    pub update_check: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            language: crate::i18n::AUTO.to_string(),
            elevenlabs_key: String::new(),
            update_check: true,
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
                // Anything but a plain no is a yes: a line somebody has typed
                // over should leave the check on rather than quietly off.
                "update_check" => {
                    settings.update_check =
                        !matches!(value.to_ascii_lowercase().as_str(), "no" | "false" | "off" | "0")
                }
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
             elevenlabs_key = \"{}\"\n\
             #\n\
             # update_check: whether to ask GitHub at startup whether there is a\n\
             # newer version. \"no\" stops it. This is the only thing the program\n\
             # does over the network without being asked for it.\n\
             update_check = \"{}\"\n",
            self.language,
            self.elevenlabs_key.trim(),
            if self.update_check { "yes" } else { "no" }
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
    // `mode` above is applied only to a file this call creates. One that was
    // already there — made by an older build, restored from a backup, copied
    // in from another machine — keeps whatever permissions it arrived with,
    // and there is a key going into it. So it is said again now the handle is
    // open, on the handle rather than on the path, which leaves nothing in
    // between for anybody to swap.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
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
            update_check: false,
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
    fn the_update_check_can_be_turned_off_and_survives_a_restart() {
        // On unless it has been turned off.
        assert!(Settings::default().update_check);
        assert!(Settings::parse("").update_check);

        let off = Settings { update_check: false, ..Settings::default() };
        assert!(!Settings::parse(&off.to_text()).update_check);
        assert!(Settings::parse(&Settings::default().to_text()).update_check);

        for no in ["no", "No", "false", "off", "0"] {
            assert!(!Settings::parse(&format!("update_check = \"{no}\"\n")).update_check, "{no}");
        }
        // Anything else leaves it on rather than quietly off.
        for yes in ["yes", "true", "on", "1", "perhaps"] {
            assert!(Settings::parse(&format!("update_check = \"{yes}\"\n")).update_check, "{yes}");
        }
    }

    /// The file holds a key, and the permissions have to be right on a file
    /// that was already there as well as on one this call makes.
    #[test]
    #[cfg(unix)]
    fn a_settings_file_that_already_existed_is_still_closed_to_everybody_else() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("openwrite-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");

        // As an older build, or a restored backup, might have left it.
        std::fs::write(&path, b"language = \"en\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_private(&path, b"elevenlabs_key = \"sk_secret\"\n").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{path:?} is still readable by somebody else");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quotes_around_the_value_are_optional() {
        assert_eq!(Settings::parse("language = fr\n").language, "fr");
        assert_eq!(Settings::parse("language = \"pt-BR\"\n").language, "pt-BR");
    }
}
