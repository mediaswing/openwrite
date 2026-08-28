//! Handing a link to the system.
//!
//! Two places need to open a web page — the download for an update, and where
//! to get Ollama if it is not installed — and neither is worth a dependency.
//! Every desktop platform has a command that opens a URL the way the user's own
//! click would, so this is that command and nothing else.
//!
//! It never opens anything but `http://` and `https://`. Handing an arbitrary
//! string to a shell-adjacent opener is how a link turns into a program, and
//! the two callers here only ever have web addresses.

use crate::t;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

/// A Windows system program, named by its full path rather than by its name.
///
/// `Command::new("rundll32")` leaves Windows to find it, and the search order
/// it uses starts with the directory the program was launched from and the
/// working directory — both of which can be a folder somebody downloaded a
/// screenplay into. A `rundll32.exe` sitting there would be run in preference
/// to the real one. `%SystemRoot%` settles it, and `sub` is the folder under it
/// the program actually lives in: `System32` for most, nothing for Explorer.
#[cfg(target_os = "windows")]
fn system_program(sub: &str, exe: &str) -> OsString {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    let mut path = std::path::PathBuf::from(root);
    if !sub.is_empty() {
        path.push(sub);
    }
    path.push(exe);
    path.into_os_string()
}

/// Open a web address in whatever the user browses with.
pub fn open(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(t!("error.not_a_web_address", url = url));
    }

    // Not `cmd /c start` on Windows: `cmd.exe` re-parses its command line after
    // Rust has quoted it, so `&`, `^`, `|` and `%VAR%` in a URL would be read as
    // shell syntax rather than as part of the address. This goes straight to the
    // protocol handler with no shell in between.
    #[cfg(target_os = "windows")]
    let (program, args): (OsString, &[&str]) =
        (system_program("System32", "rundll32.exe"), &["url.dll,FileProtocolHandler"]);
    #[cfg(target_os = "macos")]
    let (program, args): (OsString, &[&str]) = (OsString::from("/usr/bin/open"), &[]);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (program, args): (OsString, &[&str]) = (OsString::from("xdg-open"), &[]);

    Command::new(program)
        .args(args)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| t!("error.browser", error = err))
}

/// Show a folder in the file manager.
///
/// Its own function rather than a second use of [`open`], which takes web
/// addresses and nothing else on purpose. This one takes a path and hands it
/// straight to the platform's file manager, with no shell in between and no
/// string parsing anywhere: the only caller is the languages folder, which the
/// program worked out itself.
pub fn show_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(t!("error.not_a_folder", path = path.display()));
    }

    // Full paths for the same reason as in [`open`].
    #[cfg(target_os = "windows")]
    let program = system_program("", "explorer.exe");
    #[cfg(target_os = "macos")]
    let program = OsString::from("/usr/bin/open");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = OsString::from("xdg-open");

    Command::new(program)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        // `explorer` returns a non-zero exit code even when it worked, so the
        // spawn is all that is ever checked here.
        .map_err(|err| t!("error.file_manager", error = err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_is_not_there_is_not_handed_to_anything() {
        // Nothing is spawned for this either.
        assert!(show_folder(Path::new("/no/such/folder/anywhere")).is_err());
    }

    #[test]
    fn only_web_addresses_are_opened() {
        // Nothing is spawned for these, so the test opens no windows.
        assert!(open("file:///etc/passwd").is_err());
        assert!(open("javascript:alert(1)").is_err());
        assert!(open("/bin/sh").is_err());
        assert!(open("").is_err());
    }
}
