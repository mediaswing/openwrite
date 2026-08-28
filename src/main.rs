// Screenplay Creation Tool — write and format screenplays in Fountain.
// Copyright (C) 2026 mediaswing
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
// more details.
//
// You should have received a copy of the GNU General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

//! The application.
//!
//! It opens a window. The only arguments it takes are the screenplay or audio
//! drama to open —
//! which is what Finder and Explorer pass when somebody double-clicks a
//! document — and the two conventions every executable is expected to answer,
//! plus `--self-check`, which is there for the release build rather than for
//! anybody using this.

// No console window behind the editor. The build is a windowed application on
// Windows; `--self-check` reports through its exit code rather than through a
// terminal it has not got, and a debug build keeps its console so that
// `println!` still works while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod self_check;

use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "\
Screenplay Creation Tool — write and format screenplays in Fountain

USAGE
  screenplay-creation-tool [FILE]

  FILE is a .sct document or a .fountain screenplay to open. With no
  argument the editor starts on an empty screenplay. An .xml audio drama
  opens in the Audio Drama tab instead.

  Screenplays are written in Fountain, plus a one-line way to write a
  speech that is this tool's own -- MAYA: Forty-one. -- and is expanded
  back into ordinary Fountain whenever a .fountain file is saved.

  Everything else the program does, it does in the window. Press F1 there
  for the keyboard shortcuts.

OPTIONS
  -h, --help       Show this help
  -V, --version    Show the version
      --self-check Verify the formatting engine and exit

ENVIRONMENT
  OPENWRITE_AI_URL    Where to look for a local model server.
                      Default http://127.0.0.1:11434, which is Ollama.
  OPENWRITE_AI_MODEL  Which model to use, if the server has more than one.
  OPENWRITE_ELEVENLABS_KEY
                      The API key for the Audio Drama tab. Takes precedence
                      over the one in the settings file, and is never written
                      to disk -- which is the way to use one without leaving
                      it lying about. Keys are made at
                      elevenlabs.io -> Settings -> API keys; the tab has a
                      button that opens the page.
  OPENWRITE_ELEVENLABS_MODEL
                      Which ElevenLabs voice model to use.
                      Default eleven_multilingual_v2.
  OPENWRITE_NO_UPDATE_CHECK
                      Set to stop the editor asking GitHub, once at startup,
                      whether there is a newer release.
  OPENWRITE_LOG       A path to write the debug log to as well as to memory.
                      The log is in the window at Shift-Cmd-L either way; a
                      file is what catches a run that ended badly.

  None of these is needed to write a screenplay. The editor asks GitHub for
  the latest version once at startup and nothing else goes out unasked: a
  model is only ever sent what you ask it about, an audio drama is only sent
  to ElevenLabs when you press Record, and the debug log stays in memory
  unless you name a file for it.
";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();

    match first.as_deref().and_then(|a| a.to_str()) {
        Some("-h" | "--help") => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("Screenplay Creation Tool {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--self-check") => self_check::run(),
        _ => {
            // Finder and Explorer pass the document being opened as the first
            // argument, which is exactly what the editor wants.
            let path = first.map(PathBuf::from).filter(|path| path.exists());
            launch(path)
        }
    }
}

#[cfg(feature = "gui")]
fn launch(path: Option<PathBuf>) -> ExitCode {
    match openwrite::app::run(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // With no console there is nowhere for this to go, but a debug
            // build has a terminal to complain in, and the platform log picks
            // it up either way.
            eprintln!("Screenplay Creation Tool: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "gui"))]
fn launch(_path: Option<PathBuf>) -> ExitCode {
    eprintln!(
        "Screenplay Creation Tool: this build has no window \
         (compiled with --no-default-features)."
    );
    ExitCode::FAILURE
}
