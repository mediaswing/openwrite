//! The parsed document model.

use crate::inline::Rich;

/// One screenplay element, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    /// `INT. KITCHEN - DAY`, optionally with a `#1A#` scene number.
    SceneHeading { text: Rich, scene_number: Option<String> },
    /// A paragraph of action / description.
    Action { text: Rich, centered: bool },
    /// A character cue plus everything they say.
    Dialogue(Speech),
    /// Two speeches printed side by side (`CHARACTER ^`).
    DualDialogue(Speech, Speech),
    /// `CUT TO:` — printed flush right.
    Transition(Rich),
    /// An explicit `===` page break.
    PageBreak,
    /// `# Act One` — outline structure, never printed in the script.
    Section { level: u8, text: String },
    /// `= She finally says it.` — outline note, never printed.
    Synopsis(String),
}

/// A character cue and the lines under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speech {
    pub character: Rich,
    pub parts: Vec<SpeechPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechPart {
    /// `(beat)`
    Parenthetical(Rich),
    /// Spoken words.
    Line(Rich),
    /// `~Row, row, row your boat` — printed in italic.
    Lyric(Rich),
}

impl SpeechPart {
    pub fn text(&self) -> &Rich {
        match self {
            SpeechPart::Parenthetical(t) | SpeechPart::Line(t) | SpeechPart::Lyric(t) => t,
        }
    }
}

impl Speech {
    /// The cue with any `(V.O.)`, `(CONT'D)` extension removed, uppercased —
    /// the identity used for statistics and `(CONT'D)` continuation.
    pub fn character_name(&self) -> String {
        let full = crate::inline::plain_text(&self.character);
        let base = match full.find('(') {
            Some(i) => &full[..i],
            None => &full[..],
        };
        base.trim().trim_end_matches('^').trim().to_uppercase()
    }
}

/// A complete parsed screenplay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screenplay {
    /// Title page keys in source order, e.g. `("Title", ["THE BIG SLEEP"])`.
    pub title_page: Vec<(String, Vec<String>)>,
    pub elements: Vec<Element>,
}

impl Screenplay {
    /// Look up a title page value, case-insensitively.
    pub fn meta(&self, key: &str) -> Option<&[String]> {
        self.title_page
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_slice())
    }

    /// First matching title page key, joined into one line.
    pub fn meta_line(&self, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|k| self.meta(k))
            .map(|v| v.join(" "))
    }

    pub fn has_title_page(&self) -> bool {
        !self.title_page.is_empty()
    }
}
