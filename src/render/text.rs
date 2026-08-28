//! Fixed-width text output: what a screenplay looks like on paper.

use crate::layout::{Options, Page};
use std::fmt::Write;

/// Render pages as plain text with right-aligned page numbers.
///
/// `form_feed` inserts `\f` between pages, which printers and `less` understand
/// as a page break.
pub fn render(pages: &[Page], opts: &Options, form_feed: bool) -> String {
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            if form_feed {
                out.push('\u{c}');
            } else {
                out.push('\n');
            }
        }
        render_page(&mut out, page, opts);
    }
    out
}

fn render_page(out: &mut String, page: &Page, opts: &Options) {
    if let Some(number) = page.number {
        let label = format!("{number}.");
        let pad = opts.width.saturating_sub(label.chars().count());
        let _ = writeln!(out, "{}{}", " ".repeat(pad), label);
        out.push('\n');
    }
    for line in &page.lines {
        out.push_str(&line.to_text());
        out.push('\n');
    }
}
