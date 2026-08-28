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

//! openwrite — parse and format screenplays written in Fountain.
//!
//! [`parse`] reads [Fountain](https://fountain.io) and one addition of this
//! tool's own: a speech may be written on a single line as `MAYA: Forty-one.`,
//! and a transition as `/transition:cut to`. That is not Fountain, and
//! [`shorthand::expand`] turns it back into Fountain — which is what
//! [`document::write_for`] does to anything saved as `.fountain`, so a file
//! leaving this program is one any other reader agrees with. See
//! [`shorthand`] for the whole of it.
//!
//! The words the window says are not written into it either: they come out of
//! a language file, so the editor can be translated by somebody who does not
//! program. See [`i18n`].
//!
//! ```no_run
//! let doc = openwrite::parse("INT. KITCHEN - DAY\n\nMaya waits.\n");
//! let opts = openwrite::layout::Options::default();
//! let pages = openwrite::layout::paginate(&doc, &opts);
//! print!("{}", openwrite::render::text::render(&pages, &opts, false));
//! ```

#[cfg(feature = "ai")]
pub mod ai;
#[cfg(feature = "gui")]
pub mod app;
pub mod bible;
pub mod browser;
pub mod document;
pub mod element;
pub mod i18n;
pub mod inline;
pub mod json;
pub mod layout;
pub mod log;
pub mod parser;
pub mod render;
pub mod settings;
pub mod shorthand;
pub mod stats;
#[cfg(feature = "update")]
pub mod update;

pub use element::{Element, Screenplay};
pub use parser::parse;

/// Format a screenplay source string in one call.
///
/// The source is Fountain, or Fountain with the one-line dialogue form in
/// [`shorthand`]; the rendered output is the same either way.
pub fn format(source: &str, opts: &layout::Options, format: render::Format) -> String {
    let doc = parse(source);
    match format {
        render::Format::Text => {
            let pages = layout::paginate(&doc, opts);
            render::text::render(&pages, opts, false)
        }
        render::Format::Html => render::html::render(&doc, opts),
        render::Format::Fdx => render::fdx::render(&doc),
    }
}
