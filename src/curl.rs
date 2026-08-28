//! Where `curl` is.
//!
//! Two things in this program speak HTTPS — the update check and the audio
//! drama — and neither of them brings a TLS stack with it. Both shell out to
//! `curl`, which ships with macOS and with Windows 10 and later and is on any
//! Linux worth the name, and both want the same answer to the same question,
//! so the question is asked in one place.

/// The command to run.
///
/// Named by its full path on Windows, where the search order for a bare name
/// starts with the folder the program was launched from and the working
/// directory: a `curl.exe` in a folder somebody downloaded a screenplay into
/// would otherwise be run in preference to the real one. Everywhere else the
/// name is right, because `PATH` is the whole of the search.
pub fn path() -> std::ffi::OsString {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("SystemRoot")
            .unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows"));
        let mut path = std::path::PathBuf::from(root);
        path.push("System32");
        path.push("curl.exe");
        return path.into_os_string();
    }
    #[cfg(not(target_os = "windows"))]
    std::ffi::OsString::from("curl")
}
