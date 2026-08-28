//! Fountain parser.
//!
//! Fountain is the plain-text screenplay markup described at <https://fountain.io>.
//! The grammar is context-sensitive in one narrow way: most elements are
//! recognised by what surrounds them (blank lines) rather than by a sigil, so
//! this is a line scanner with one line of lookahead and lookbehind.
//!
//! # Two departures from the standard
//!
//! Both are read here and both are undone on the way out, so a `.fountain` file
//! written by this program is one any other Fountain reader agrees with.
//!
//! A speech may be written on one line — `MAYA: Forty-one.` — along with
//! `/transition:cut to`. That is [`crate::shorthand`], which also holds the
//! expansion back into the three-line form.
//!
//! And a title page is recognised only when its first key is ordinary-
//! capitalised (`Title`, `Production`) or a known title-page key in capitals
//! (`TITLE:`) — see [`opens_title_page`]. Fountain accepts any `Key: value`
//! first line, but under that rule `MAYA: Forty-one.` typed into an empty
//! document is a title page with a key called MAYA rather than Maya speaking.

use crate::element::{Element, Screenplay, Speech, SpeechPart};
use crate::inline;
use crate::shorthand;

/// Parse a Fountain document.
pub fn parse(source: &str) -> Screenplay {
    let source = strip_boneyard(&source.replace("\r\n", "\n").replace('\r', "\n"));
    let lines: Vec<String> = source.split('\n').map(|l| l.trim_end().to_string()).collect();

    let mut doc = Screenplay::default();
    let start = parse_title_page(&lines, &mut doc);
    parse_body(&lines[start.min(lines.len())..], &mut doc);
    doc
}

/// Remove `/* ... */` blocks, which may span lines.
fn strip_boneyard(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        match rest[start + 2..].find("*/") {
            Some(end) => {
                out.push_str(&rest[..start]);
                rest = &rest[start + 2 + end + 2..];
            }
            None => {
                // Unterminated: everything from here on is commented out.
                out.push_str(&rest[..start]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// How many leading lines of a source belong to the title page.
///
/// The shorthand expander in [`crate::shorthand`] needs to know, because a
/// title page is a run of `Key: value` lines and that is exactly the shape it
/// is looking for elsewhere. Answering here rather than there keeps one
/// definition of where the title page ends.
///
/// Counted in the lines of the text as given, boneyard and all. [`parse`]
/// removes `/* ... */` before anything else, which shifts every line number
/// after a multi-line comment; the expander is rewriting the real file and needs
/// the real line numbers, so this does not.
pub fn title_page_span(source: &str) -> usize {
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = source.split('\n').map(|l| l.trim_end().to_string()).collect();
    let mut discarded = Screenplay::default();
    parse_title_page(&lines, &mut discarded)
}

/// Parse `Key: Value` pairs at the top of the file. Returns the index of the
/// first body line.
fn parse_title_page(lines: &[String], doc: &mut Screenplay) -> usize {
    let first = match lines.iter().position(|l| !l.trim().is_empty()) {
        Some(i) => i,
        None => return lines.len(),
    };
    match key_of(&lines[first]) {
        Some((key, _)) if opens_title_page(&key) => {}
        // Not a title page: either not `Key: value` at all, or a key that is a
        // character's name. `MAYA: Forty-one.` at the top of an empty document
        // is somebody starting to write, not a title page with a key called
        // MAYA — see `opens_title_page`.
        _ => return 0,
    }

    let mut i = first;
    let mut current: Option<(String, Vec<String>)> = None;
    while i < lines.len() {
        let line = &lines[i];
        if line.trim().is_empty() {
            i += 1;
            break;
        }
        match key_of(line) {
            Some((key, value)) => {
                if let Some(pair) = current.take() {
                    doc.title_page.push(pair);
                }
                let mut values = Vec::new();
                if !value.trim().is_empty() {
                    values.push(value.trim().to_string());
                }
                current = Some((key, values));
            }
            None => {
                // Indented continuation of the previous key.
                if let Some((_, values)) = current.as_mut() {
                    values.push(line.trim().to_string());
                }
            }
        }
        i += 1;
    }
    if let Some(pair) = current.take() {
        doc.title_page.push(pair);
    }
    // Skip any further blank lines before the body.
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    i
}

/// The keys a title page is conventionally made of.
///
/// Fountain lets a title page carry any key it likes, and this does not change
/// that — every key inside a title page is still kept. What the list settles is
/// only the first line, which is what decides whether there is a title page at
/// all.
const TITLE_PAGE_KEYS: [&str; 14] = [
    "title", "credit", "author", "authors", "source", "notes", "draft date",
    "date", "contact", "copyright", "revision", "format", "language", "series",
];

/// Can this key be the first line of a title page?
///
/// Anything but an all-capitals key can, which covers `Title`, `Credit` and
/// whatever else somebody wants to put up there. An all-capitals key is
/// ambiguous — it is also how a character cue is written in the one-line form
/// this tool understands (see [`crate::shorthand`]) — so for those the key has
/// to be one a title page actually uses. `TITLE:` is a title; `MAYA:` is Maya.
pub fn opens_title_page(key: &str) -> bool {
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    if !is_upper(key) {
        return true;
    }
    let lower = key.to_lowercase();
    TITLE_PAGE_KEYS.contains(&lower.as_str())
}

/// Split `Key: value`, if the line looks like a title page entry.
fn key_of(line: &str) -> Option<(String, String)> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let colon = line.find(':')?;
    let key = &line[..colon];
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '_' || c == '-')
    {
        return None;
    }
    if !key.chars().next()?.is_alphabetic() {
        return None;
    }
    Some((key.trim().to_string(), line[colon + 1..].to_string()))
}

fn parse_body(lines: &[String], doc: &mut Screenplay) {
    let mut i = 0;
    // Consecutive non-blank action lines are gathered into one paragraph.
    let mut action: Vec<String> = Vec::new();
    // Whether the line just read was a one-line speech. An exchange typed as
    // `MAYA: One.` / `DEV: Two.` has no blank line between its halves, so the
    // second is allowed to be a cue on the strength of the first.
    let mut after_shorthand = false;

    macro_rules! flush_action {
        () => {
            if !action.is_empty() {
                let text = std::mem::take(&mut action).join("\n");
                push_action(doc, &text);
            }
        };
    }

    while i < lines.len() {
        let raw = &lines[i];
        let line = raw.trim();
        let blank_before = i == 0 || lines[i - 1].trim().is_empty();
        let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");

        // Reset up front, so every path out of this iteration clears it and the
        // one-line form below is the only thing that sets it again.
        let continuing = after_shorthand;
        after_shorthand = false;

        if line.is_empty() {
            flush_action!();
            i += 1;
            continue;
        }


        // --- Elements identified by a leading sigil -------------------------
        if line.chars().all(|c| c == '=') && line.len() >= 3 {
            flush_action!();
            doc.elements.push(Element::PageBreak);
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            flush_action!();
            let extra = rest.chars().take_while(|&c| c == '#').count();
            doc.elements.push(Element::Section {
                level: (extra as u8 + 1).min(6),
                text: rest[extra..].trim().to_string(),
            });
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix('=') {
            flush_action!();
            doc.elements.push(Element::Synopsis(rest.trim().to_string()));
            i += 1;
            continue;
        }
        if line.starts_with('>') && line.ends_with('<') && line.len() > 1 {
            flush_action!();
            let inner = line[1..line.len() - 1].trim();
            doc.elements.push(Element::Action { text: rich(inner), centered: true });
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix('>') {
            flush_action!();
            doc.elements.push(Element::Transition(rich(rest.trim())));
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix('!') {
            flush_action!();
            action.push(rest.to_string());
            i += 1;
            continue;
        }
        if line.starts_with('.') && !line.starts_with("..") && line.len() > 1 {
            flush_action!();
            doc.elements.push(scene_heading(&line[1..]));
            i += 1;
            continue;
        }

        // --- Scene heading --------------------------------------------------
        if blank_before && is_scene_heading(line) {
            flush_action!();
            doc.elements.push(scene_heading(line));
            i += 1;
            continue;
        }

        // --- Transition -----------------------------------------------------
        if blank_before && next.is_empty() && is_transition(line) {
            flush_action!();
            doc.elements.push(Element::Transition(rich(line)));
            i += 1;
            continue;
        }

        // --- The one-line forms ---------------------------------------------
        //
        // Below the sigil dispatch on purpose: `.SALT HOUSE: NIGHT` is a forced
        // scene heading and `# ACT ONE: THE FALL` is a section, and either would
        // otherwise be read as somebody speaking.
        if let Some(kind) = shorthand::transition(line) {
            flush_action!();
            doc.elements.push(Element::Transition(rich(&kind)));
            i += 1;
            continue;
        }
        // A cue is only looked for at the start of a block or straight after
        // another one, so a colon in the middle of a paragraph of action stays
        // a colon in the middle of a paragraph of action.
        if blank_before || continuing {
            if let Some(cue) = shorthand::cue(line) {
                flush_action!();
                let mut parts = Vec::new();
                if let Some(parenthetical) = &cue.parenthetical {
                    parts.push(SpeechPart::Parenthetical(rich(parenthetical)));
                }
                parts.push(SpeechPart::Line(rich(&cue.speech)));
                doc.elements.push(Element::Dialogue(Speech { character: rich(&cue.name), parts }));
                after_shorthand = true;
                i += 1;
                continue;
            }
        }

        // --- Character cue and the dialogue under it ------------------------
        let forced_cue = line.starts_with('@');
        if !next.is_empty() && (forced_cue || (blank_before && is_character(line))) {
            flush_action!();
            let cue = if forced_cue { line[1..].trim() } else { line };
            let (cue, dual) = match cue.strip_suffix('^') {
                Some(c) => (c.trim(), true),
                None => (cue, false),
            };
            let (parts, consumed) = parse_speech(&lines[i + 1..]);
            let speech = Speech { character: rich(cue), parts };
            i += 1 + consumed;

            match (dual, doc.elements.last()) {
                (true, Some(Element::Dialogue(_))) => {
                    let Some(Element::Dialogue(left)) = doc.elements.pop() else { unreachable!() };
                    doc.elements.push(Element::DualDialogue(left, speech));
                }
                _ => doc.elements.push(Element::Dialogue(speech)),
            }
            continue;
        }

        // --- Anything else is action ----------------------------------------
        action.push(raw.to_string());
        i += 1;
    }
    flush_action!();
}

/// Consume the lines belonging to a character cue.
fn parse_speech(lines: &[String]) -> (Vec<SpeechPart>, usize) {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let raw = &lines[i];
        let line = raw.trim();
        if line.is_empty() {
            // A line of two or more spaces is an intentional blank *inside*
            // dialogue; a truly empty line ends the block.
            if raw.len() >= 2 && raw.chars().all(|c| c == ' ') {
                parts.push(SpeechPart::Line(Vec::new()));
                i += 1;
                continue;
            }
            break;
        }
        if line.starts_with('(') && line.ends_with(')') {
            parts.push(SpeechPart::Parenthetical(rich(line)));
        } else if let Some(lyric) = line.strip_prefix('~') {
            parts.push(SpeechPart::Lyric(rich(lyric.trim())));
        } else {
            parts.push(SpeechPart::Line(rich(line)));
        }
        i += 1;
    }
    // A cue with nothing under it is not dialogue at all.
    (parts, i)
}

fn push_action(doc: &mut Screenplay, text: &str) {
    let text = text.trim_end_matches('\n');
    if text.trim().is_empty() {
        return;
    }
    doc.elements.push(Element::Action { text: rich(text), centered: false });
}

fn scene_heading(line: &str) -> Element {
    let line = line.trim();
    // A trailing `#1A#` is the scene number.
    let (text, number) = match line.strip_suffix('#').and_then(|l| l.rfind('#').map(|i| (i, l))) {
        Some((i, l)) if i + 1 < l.len() => (l[..i].trim(), Some(l[i + 1..].to_string())),
        _ => (line, None),
    };
    Element::SceneHeading { text: rich(text), scene_number: number }
}

/// Scene headings open with a standard location abbreviation.
///
/// Public because the model briefing in [`crate::ai`] has to find the scene the
/// caret is in, and it should agree with the parser about what one looks like.
pub fn is_scene_heading(line: &str) -> bool {
    const PREFIXES: [&str; 8] =
        ["INT./EXT.", "INT/EXT.", "INT./EXT", "INT.", "EXT.", "EST.", "I/E.", "I/E "];
    let upper = line.to_uppercase();
    PREFIXES.iter().any(|p| upper.starts_with(p))
        || ["INT ", "EXT ", "EST ", "INT/EXT "].iter().any(|p| upper.starts_with(p))
}

/// `CUT TO:` and friends: uppercase, ending in `TO:`.
///
/// Public because the shorthand in [`crate::shorthand`] has to know that
/// `CUT TO:` is a transition rather than a character called CUT TO.
pub fn is_transition(line: &str) -> bool {
    line.ends_with("TO:") && is_upper(line) && line.chars().any(|c| c.is_alphabetic())
}

/// A character cue is an uppercase line. Any `(V.O.)`-style extension is
/// ignored for the case test, since those are often typed in lower case.
fn is_character(line: &str) -> bool {
    let base = match line.find('(') {
        Some(i) => line[..i].trim(),
        None => line.trim_end_matches('^').trim(),
    };
    if base.is_empty() || !base.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    // Rule out things that only look like cues.
    if base.ends_with(':') {
        return false;
    }
    is_upper(base)
}

fn is_upper(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
}

fn rich(text: &str) -> inline::Rich {
    inline::parse(&inline::strip_notes(text))
}
