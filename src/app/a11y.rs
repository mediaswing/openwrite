//! Accessibility plumbing.
//!
//! eframe already publishes an AccessKit tree — VoiceOver on macOS, UI
//! Automation on Windows, AT-SPI on Linux — so widgets arrive at the screen
//! reader as real objects rather than painted pixels. What this module adds is
//! the part AccessKit cannot infer: accessible names for panes that are drawn
//! rather than labelled, descriptions that say how to work them, and a polite
//! live region so the application can report what it just did without stealing
//! the user's focus.
//!
//! The colour side of accessibility lives in [`super::theme`], which holds the
//! contrast floors and the focus ring.

use eframe::egui::{accesskit, Response};

/// Give a widget an accessible name — the phrase a screen reader leads with.
pub fn name(response: &Response, label: impl Into<String>) {
    let label = label.into();
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_label(label);
    });
}

/// Give a widget supplementary text, read after its name and role.
pub fn describe(response: &Response, description: impl Into<String>) {
    let description = description.into();
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_description(description);
    });
}

/// Mark a widget as a polite live region: assistive technology announces
/// changes to its text when the user is between tasks, rather than cutting in.
pub fn live_region(response: &Response) {
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_live(accesskit::Live::Polite);
    });
}

/// A page count as a phrase, for announcements that get read aloud.
///
/// Counted through the language file rather than with an `if`, because "one
/// page" and "two pages" are an English rule and other languages have their
/// own — see [`crate::i18n`].
pub fn pages_phrase(pages: usize) -> String {
    crate::tn!("phrase.pages", pages)
}
