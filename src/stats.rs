//! Script statistics — the numbers a writer actually asks for.

use crate::element::{Element, Screenplay, Speech};
use crate::inline;
use crate::layout::{Options, Page};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub pages: usize,
    pub scenes: usize,
    pub words: usize,
    pub dialogue_words: usize,
    pub action_words: usize,
    /// Speaking characters, with their line and word counts.
    pub characters: BTreeMap<String, CharacterStats>,
}

#[derive(Debug, Clone, Default)]
pub struct CharacterStats {
    /// Number of times the character has a cue.
    pub cues: usize,
    /// Number of spoken lines (as written, before wrapping).
    pub lines: usize,
    pub words: usize,
}

impl Stats {
    /// Rough running time: the industry's one-page-per-minute rule of thumb.
    pub fn estimated_minutes(&self) -> usize {
        self.pages
    }
}

pub fn compute(doc: &Screenplay, pages: &[Page]) -> Stats {
    let mut stats = Stats {
        pages: pages.iter().filter(|p| !p.is_title_page).count(),
        ..Default::default()
    };

    for element in &doc.elements {
        match element {
            Element::SceneHeading { .. } => stats.scenes += 1,
            Element::Action { text, .. } => {
                let n = count_words(&inline::plain_text(text));
                stats.action_words += n;
                stats.words += n;
            }
            Element::Dialogue(speech) => tally(&mut stats, speech),
            Element::DualDialogue(left, right) => {
                tally(&mut stats, left);
                tally(&mut stats, right);
            }
            _ => {}
        }
    }
    stats
}

/// Page count without rendering: useful for a live status bar.
pub fn quick_page_count(doc: &Screenplay, opts: &Options) -> usize {
    crate::layout::paginate(doc, opts).iter().filter(|p| !p.is_title_page).count()
}

fn tally(stats: &mut Stats, speech: &Speech) {
    let entry = stats.characters.entry(speech.character_name()).or_default();
    entry.cues += 1;
    for part in &speech.parts {
        let text = inline::plain_text(part.text());
        if text.trim().is_empty() {
            continue;
        }
        let n = count_words(&text);
        entry.lines += 1;
        entry.words += n;
        stats.dialogue_words += n;
        stats.words += n;
    }
}

fn count_words(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}
