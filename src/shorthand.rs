//! A quicker way to type dialogue, and what it means in Fountain.
//!
//! Fountain writes a speech over three lines — a cue, an optional
//! parenthetical, then the words. That is what a screenplay looks like on the
//! page, and it is a lot of Return-pressing for a fast exchange. This module
//! understands a one-line form as well:
//!
//! ```text
//! MAYA: Forty-one.
//! DEV (not looking up): You know what this is.
//! /transition:cut to
//! ```
//!
//! which is the same screenplay as:
//!
//! ```text
//! MAYA
//! Forty-one.
//!
//! DEV
//! (not looking up)
//! You know what this is.
//!
//! CUT TO:
//! ```
//!
//! A speech written this way ends at the end of its line. Press Return and the
//! next line is ordinary action again — unless it is another cue, so a rapid
//! exchange can be typed without a blank line between every line of it.
//!
//! # This is not Fountain
//!
//! It is worth being plain about that. `MAYA: Forty-one.` in a file handed to
//! another tool is a line of action, not a speech, and the tool would be right:
//! Fountain has no such form. So the shorthand is understood on the way in and
//! written out in full on the way out — [`expand`] turns it back into ordinary
//! Fountain, and [`crate::document::write_for`] runs it over anything saved as
//! `.fountain`. What the tool's own `.sct` file keeps is what was typed.
//!
//! # Parentheses before the colon
//!
//! These become a parenthetical — a stage direction under the cue — with one
//! exception. `(V.O.)`, `(O.S.)`, `(O.C.)` and `(CONT'D)` are not stage
//! directions but part of the cue itself, and every screenplay prints them on
//! the cue line, so that is where they go.

use crate::parser;

/// How a transition is written: `/transition:cut to`.
const TRANSITION: &str = "/transition:";

/// Extensions that belong on the cue line rather than under it.
const EXTENSIONS: [&str; 8] = ["V.O.", "VO", "O.S.", "OS", "O.C.", "OC", "CONT'D", "CONTD"];

/// Capitalised words that are followed by a colon and are not somebody speaking.
///
/// Screenplays are full of these — `SUPER: THREE YEARS LATER` is a caption, not
/// a character called SUPER — and so are title pages, whose keys are written
/// this way as often as not. Every one of them would otherwise be read as a cue
/// and, worse, written into an exported file as one.
///
/// The cost is that a character genuinely called INSERT cannot use the one-line
/// form. They can still be written the ordinary Fountain way.
const NOT_NAMES: [&str; 24] = [
    // Captions and camera directions.
    "SUPER", "SUPERIMPOSE", "TITLE CARD", "SUBTITLE", "CHYRON", "LEGEND", "INSERT",
    "INTERCUT", "MONTAGE", "SERIES OF SHOTS", "ANGLE ON", "CLOSE ON", "BACK TO",
    "FADE IN", "FADE OUT", "THE END",
    // Title page keys, for the rare file whose title page this has to survive
    // without help from `parser::title_page_span`.
    "TITLE", "CREDIT", "AUTHOR", "AUTHORS", "SOURCE", "CONTACT", "COPYRIGHT",
    "DRAFT DATE",
];

/// A speech written on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    /// The character, including any `(V.O.)` extension.
    pub name: String,
    /// A stage direction to print under the cue, brackets included.
    pub parenthetical: Option<String>,
    /// What they say. Never empty: the words have to be on this line for it to
    /// be this form at all.
    pub speech: String,
}

/// Read a one-line speech, if that is what this line is.
///
/// Two things make the form unambiguous, and both are required. The name has to
/// be in capitals, so `She reads the sign: NO ENTRY` is a sentence rather than a
/// speech. And the words have to be on the same line, so `FADE IN:` is the
/// transition it has always been rather than a character called FADE IN — a
/// colon with nothing after it is not this form, whatever comes next.
pub fn cue(line: &str) -> Option<Cue> {
    let line = line.trim();
    // A line that opens with Fountain markup is that element, whatever else it
    // looks like: `.SALT HOUSE: NIGHT` is a forced scene heading and
    // `# ACT ONE: THE FALL` is a section. The parser dispatches on the sigil
    // before it gets here, and saying so here as well means no other caller has
    // to remember to.
    if line.starts_with(['#', '=', '>', '!', '.', '@', '~']) {
        return None;
    }
    let (head, rest) = split_at_colon(line)?;
    let head = head.trim();
    if head.is_empty() {
        return None;
    }

    // `CUT TO:` is a transition and always was; it is not a character called
    // "CUT TO". The same goes for a scene heading with a colon in it.
    if parser::is_transition(&format!("{head}:")) || parser::is_scene_heading(head) {
        return None;
    }

    let (name, bracketed) = split_parenthetical(head);
    let name = name.trim();
    if !is_name(name) || is_not_a_name(name) {
        return None;
    }

    // An extension is part of the cue; anything else is a stage direction.
    let (name, parenthetical) = match bracketed {
        Some(inner) if is_extension(inner) => (format!("{name} ({inner})"), None),
        Some(inner) => (name.to_string(), Some(format!("({inner})"))),
        None => (name.to_string(), None),
    };

    let speech = rest.trim();
    if speech.is_empty() {
        return None;
    }
    Some(Cue { name, parenthetical, speech: speech.to_string() })
}

/// Read `/transition:cut to`, giving the transition as it should print.
///
/// The type is put into capitals and given the colon that a transition ends
/// with, unless it already ends in punctuation of its own — `FADE OUT.` takes a
/// full stop, and correcting that would be wrong.
pub fn transition(line: &str) -> Option<String> {
    let line = line.trim();
    // `get` rather than a slice: this runs over every line of the screenplay on
    // every keystroke, and byte twelve of a line beginning with a curly quote or
    // an em dash is inside a character, not at the edge of one.
    if !line.get(..TRANSITION.len())?.eq_ignore_ascii_case(TRANSITION) {
        return None;
    }
    let kind = line[TRANSITION.len()..].trim().to_uppercase();
    if kind.is_empty() {
        // A transition to nothing is a typo, and swallowing the line would hide
        // it. Let it be read as whatever it would otherwise have been.
        return None;
    }
    Some(match kind.chars().last() {
        Some(':' | '.' | '!' | '?') => kind,
        _ => format!("{kind}:"),
    })
}

/// Split a line at its first colon, ignoring any inside brackets.
///
/// The colon inside `DEV (beat: then): Fine.` is part of the stage direction,
/// and the one that matters is the one after it.
fn split_at_colon(line: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (i, c) in line.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some((&line[..i], &line[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Take a trailing `(...)` off the head of the line.
fn split_parenthetical(head: &str) -> (&str, Option<&str>) {
    let Some(open) = head.find('(') else {
        return (head, None);
    };
    let trimmed = head.trim_end();
    if !trimmed.ends_with(')') {
        return (head, None);
    }
    let inner = &trimmed[open + 1..trimmed.len() - 1];
    (&head[..open], Some(inner))
}

/// Is this one of the capitalised words that is not a character?
fn is_not_a_name(name: &str) -> bool {
    let name = name.trim();
    NOT_NAMES.iter().any(|word| word.eq_ignore_ascii_case(name))
}

fn is_extension(inner: &str) -> bool {
    let inner = inner.trim().to_uppercase();
    EXTENSIONS.iter().any(|e| *e == inner)
}

/// Is this a character's name?
///
/// Capitals, at least one letter, and nothing in it that a name would not have.
/// The punctuation allowed is what turns up in real cues: `MRS. HALE`,
/// `O'BRIEN`, `JEAN-LUC`, `GUARD #2`.
fn is_name(name: &str) -> bool {
    if name.is_empty() || !name.chars().any(char::is_alphabetic) {
        return false;
    }
    name.chars().all(|c| {
        c.is_uppercase() || c.is_numeric() || " .'-#/&".contains(c)
    })
}

/// Rewrite every shorthand line as ordinary Fountain.
///
/// This is what makes the form safe to use: a screenplay saved as `.fountain`
/// goes out in the three-line form that every other tool reads, so the
/// convenience stops at the edge of this program rather than following the
/// script around.
pub fn expand(source: &str) -> String {
    let normalised = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalised.split('\n').collect();

    // The title page is `Key: value`, which is the shape this module is looking
    // for; leaving it alone is the difference between a title page and a
    // character called AUTHOR.
    let start = parser::title_page_span(&normalised).min(lines.len());

    let mut out: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
    let mut changed = false;
    // The parser only reads a line as a one-line cue at the start of a block or
    // straight after another cue (see `parser::parse_body`). Expanding has to
    // apply exactly the same rule, or a saved `.fountain` file would be a
    // different screenplay from the one on screen.
    let mut after_shorthand = false;

    for (i, line) in lines.iter().enumerate().skip(start) {
        let blank_before = i == start || lines[i - 1].trim().is_empty();
        let continuing = after_shorthand;
        after_shorthand = false;

        if line.trim().is_empty() {
            out.push(line.to_string());
            continue;
        }
        if let Some(kind) = transition(line) {
            changed = true;
            blank_before_cue(&mut out);
            // The forced form for anything Fountain would not recognise on its
            // own, so `FADE OUT.` does not come out as a line of action.
            if parser::is_transition(&kind) {
                out.push(kind);
            } else {
                out.push(format!("> {kind}"));
            }
            blank_after(&mut out, &lines, i);
            continue;
        }
        if !blank_before && !continuing {
            out.push(line.to_string());
            continue;
        }
        let Some(cue) = cue(line) else {
            out.push(line.to_string());
            continue;
        };
        changed = true;
        blank_before_cue(&mut out);
        out.push(cue.name);
        if let Some(parenthetical) = cue.parenthetical {
            out.push(parenthetical);
        }
        out.push(cue.speech);
        blank_after(&mut out, &lines, i);
        after_shorthand = true;
    }

    if !changed {
        return normalised;
    }
    let mut text = out.join("\n");
    while text.ends_with("\n\n") {
        text.pop();
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Make sure there is a blank line before a cue that is about to be written.
fn blank_before_cue(out: &mut Vec<String>) {
    match out.last() {
        None => {}
        Some(last) if last.trim().is_empty() => {}
        Some(_) => out.push(String::new()),
    }
}

/// Close a speech off, unless the line after it in the source is already blank.
///
/// Only the blank lines this function puts in are its business. A writer who
/// left three blank lines somewhere meant to, and squashing them on the way out
/// would be editing their screenplay rather than translating it.
fn blank_after(out: &mut Vec<String>, lines: &[&str], at: usize) {
    let next_is_blank = lines.get(at + 1).is_none_or(|l| l.trim().is_empty());
    if !next_is_blank {
        out.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spoken(line: &str) -> (String, Option<String>, String) {
        let cue = cue(line).unwrap_or_else(|| panic!("{line:?} should be a cue"));
        (cue.name, cue.parenthetical, cue.speech)
    }

    #[test]
    fn a_name_a_colon_and_a_line_is_a_speech() {
        assert_eq!(
            spoken("MAYA: Forty-one."),
            ("MAYA".into(), None, "Forty-one.".into())
        );
    }

    #[test]
    fn brackets_before_the_colon_are_a_stage_direction() {
        assert_eq!(
            spoken("DEV (not looking up): You know what this is."),
            ("DEV".into(), Some("(not looking up)".into()), "You know what this is.".into())
        );
    }

    #[test]
    fn the_standard_extensions_stay_on_the_cue_line() {
        // (V.O.) is not a stage direction, and no screenplay prints it as one.
        assert_eq!(spoken("MAYA (V.O.): Forty-one.").0, "MAYA (V.O.)");
        assert_eq!(spoken("MAYA (V.O.): Forty-one.").1, None);
        assert_eq!(spoken("DEV (CONT'D): And another thing.").0, "DEV (CONT'D)");
        // Anything else is.
        assert_eq!(spoken("DEV (quietly): Fine.").1, Some("(quietly)".into()));
    }

    #[test]
    fn a_colon_with_nothing_after_it_is_not_this_form() {
        // `FADE IN:` is the oldest line in screenwriting and is not a character
        // called FADE IN. Requiring the words on the same line settles every
        // case of this at once.
        assert!(cue("MAYA:").is_none());
        assert!(cue("FADE IN:").is_none());
        assert!(cue("THE END:").is_none());
        assert!(cue("MAYA:   ").is_none());
    }

    #[test]
    fn a_colon_in_the_speech_belongs_to_the_speech() {
        assert_eq!(spoken("MAYA: One rule: nobody crosses.").2, "One rule: nobody crosses.");
    }

    #[test]
    fn a_colon_inside_the_brackets_is_not_the_one_that_splits() {
        let (name, parenthetical, speech) = spoken("DEV (a beat: then): Fine.");
        assert_eq!(name, "DEV");
        assert_eq!(parenthetical, Some("(a beat: then)".into()));
        assert_eq!(speech, "Fine.");
    }

    #[test]
    fn a_name_can_have_the_punctuation_names_have() {
        for line in ["MRS. HALE: Sit.", "O'BRIEN: No.", "JEAN-LUC: Engage.", "GUARD #2: Halt."] {
            assert!(cue(line).is_some(), "{line:?} should be a cue");
        }
    }

    #[test]
    fn a_sentence_with_a_colon_in_it_is_not_a_speech() {
        for line in [
            "She reads the sign: NO ENTRY",
            "Maya counts: forty-one.",
            "Title: The Last Bus",
            "http://example.com",
            "",
            "   ",
        ] {
            assert!(cue(line).is_none(), "{line:?} should not be a cue");
        }
    }

    #[test]
    fn a_transition_is_not_a_character_called_cut_to() {
        assert!(cue("CUT TO:").is_none());
        assert!(cue("FADE TO: BLACK").is_none());
        assert!(cue("DISSOLVE TO:").is_none());
    }

    #[test]
    fn a_scene_heading_with_a_colon_is_still_a_scene_heading() {
        assert!(cue("INT. HOUSE - DAY: LATER").is_none());
    }

    #[test]
    fn a_line_that_opens_with_markup_is_that_markup() {
        for line in [
            ".SALT HOUSE: NIGHT",
            "# ACT ONE: THE FALL",
            "= She says it: no.",
            "> CUT TO:",
            "!MAYA: not dialogue",
            "@MAYA: forced",
            "~MAYA: a lyric",
        ] {
            assert!(cue(line).is_none(), "{line:?} should not be a cue");
        }
    }

    #[test]
    fn a_transition_is_written_with_a_slash() {
        assert_eq!(transition("/transition:cut to").as_deref(), Some("CUT TO:"));
        assert_eq!(transition("/Transition: dissolve to").as_deref(), Some("DISSOLVE TO:"));
        assert_eq!(transition("/TRANSITION:smash cut to").as_deref(), Some("SMASH CUT TO:"));
    }

    #[test]
    fn a_transition_that_ends_in_its_own_punctuation_keeps_it() {
        assert_eq!(transition("/transition:fade out.").as_deref(), Some("FADE OUT."));
        assert_eq!(transition("/transition:CUT TO:").as_deref(), Some("CUT TO:"));
    }

    #[test]
    fn a_line_of_non_ascii_text_does_not_crash_the_reader() {
        // Byte twelve of these is inside a character, not at the edge of one.
        // The parser runs `transition` over every line on every keystroke, so
        // this was a crash on typing a curly quote.
        for line in [
            "\u{201C}I know,\u{201D} she says.",
            "Ashfen \u{2014} the salt city \u{2014} at dusk.",
            "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}",
            "\u{1f3ac}",
            "/tra\u{e9}",
        ] {
            assert_eq!(transition(line), None, "{line:?}");
            let _ = cue(line);
        }
    }

    #[test]
    fn a_caption_is_not_a_character() {
        // `SUPER: THREE YEARS LATER` is a caption; nobody is called SUPER.
        for line in [
            "SUPER: THREE YEARS LATER",
            "TITLE CARD: 1994",
            "INSERT: the note reads NO.",
            "INTERCUT: the two kitchens",
            "AUTHOR: W. Richards",
            "TITLE: Ashfen",
        ] {
            assert!(cue(line).is_none(), "{line:?} should not be a cue");
        }
    }

    #[test]
    fn a_transition_to_nothing_is_left_alone() {
        assert_eq!(transition("/transition:"), None);
        assert_eq!(transition("/transition:   "), None);
        assert_eq!(transition("transition: cut to"), None);
        assert_eq!(transition("/trans:cut to"), None);
    }

    // -- expanding back to Fountain -----------------------------------------

    #[test]
    fn a_shorthand_speech_expands_to_the_three_line_form() {
        let source = "INT. SALT HOUSE - NIGHT\n\nMAYA: Forty-one.\n";
        assert_eq!(
            expand(source),
            "INT. SALT HOUSE - NIGHT\n\nMAYA\nForty-one.\n"
        );
    }

    #[test]
    fn an_exchange_typed_without_blank_lines_comes_out_with_them() {
        let source = "MAYA: One.\nDEV: Two.\nMAYA: Three.\n";
        assert_eq!(expand(source), "MAYA\nOne.\n\nDEV\nTwo.\n\nMAYA\nThree.\n");
    }

    #[test]
    fn a_stage_direction_expands_onto_its_own_line() {
        let source = "DEV (quietly): Fine.\n";
        assert_eq!(expand(source), "DEV\n(quietly)\nFine.\n");
    }

    #[test]
    fn a_transition_expands_to_something_fountain_recognises() {
        // `CUT TO:` is a transition to any Fountain reader on its own.
        assert_eq!(expand("/transition:cut to\n"), "CUT TO:\n");
        // `FADE OUT.` is not, so it gets the forced form.
        assert_eq!(expand("/transition:fade out.\n"), "> FADE OUT.\n");
    }

    #[test]
    fn a_title_page_is_not_mistaken_for_a_cast_list() {
        let source = "Title: Ashfen\nAUTHOR: W. Richards\n\nMAYA: Forty-one.\n";
        let expanded = expand(source);
        assert!(expanded.starts_with("Title: Ashfen\nAUTHOR: W. Richards\n"), "{expanded:?}");
        assert!(expanded.contains("MAYA\nForty-one."), "{expanded:?}");
    }

    #[test]
    fn expanding_only_touches_lines_the_parser_would_have_read_as_cues() {
        // Mid-paragraph: the parser reads one action paragraph, so the
        // expander has to leave it as one.
        let source = "INT. HOUSE - DAY\n\nThe camera pans.\nMAYA: Forty-one.\n";
        assert_eq!(expand(source), source);

        // Under a Fountain cue: `DEV: ...` is MAYA's dialogue, not a new
        // speech, and expanding it would orphan the MAYA cue above it.
        let source = "INT. HOUSE - DAY\n\nMAYA\nDEV: I know what you did.\n";
        assert_eq!(expand(source), source);
    }

    #[test]
    fn a_sigil_line_with_a_colon_is_left_to_its_sigil() {
        for source in [
            "INT. HOUSE - DAY\n\n.SALT HOUSE: NIGHT\n",
            "# ACT ONE: THE FALL\n\nINT. HOUSE - DAY\n",
            "INT. HOUSE - DAY\n\n= She finally says it: no.\n",
        ] {
            assert_eq!(expand(source), source, "{source:?}");
        }
    }

    #[test]
    fn a_deliberate_gap_in_the_screenplay_survives_expanding() {
        // Three blank lines are a gap the writer meant; only the blank lines
        // the expander itself adds are its business.
        let source = "INT. HOUSE - DAY\n\nMAYA: One.\n\n\n\nShe waits.\n";
        let expanded = expand(source);
        assert!(expanded.contains("MAYA\nOne.\n\n\n\nShe waits."), "{expanded:?}");
    }

    #[test]
    fn a_boneyard_above_the_title_page_does_not_shift_the_keys() {
        // `parse` strips `/* ... */` before counting lines; the expander is
        // rewriting the real file, so its idea of where the title page ends has
        // to be counted in real lines.
        let source = "/* a\nb\nc */\nTitle: Ashfen\nAUTHOR: W. Richards\n\nShe waits.\n";
        let expanded = expand(source);
        assert!(expanded.contains("AUTHOR: W. Richards"), "the key became a cue: {expanded:?}");
    }

    #[test]
    fn a_screenplay_with_no_shorthand_in_it_is_left_exactly_alone() {
        let source = "INT. HOUSE - DAY\n\nMAYA\nForty-one.\n\nShe waits.\n";
        assert_eq!(expand(source), source);
    }

    #[test]
    fn expanding_twice_changes_nothing_the_second_time() {
        let source = "INT. HOUSE - DAY\n\nMAYA (quietly): One.\nDEV: Two.\n\n/transition:cut to\n";
        let once = expand(source);
        assert_eq!(expand(&once), once, "expansion is not a fixed point");
    }

    #[test]
    fn a_fountain_opening_is_left_exactly_as_it_was() {
        // The line that broke this first: `FADE IN:` must survive a round trip
        // through the expander untouched.
        let source = "FADE IN:\n\nINT. HOUSE - DAY\n\nMAYA: Forty-one.\n";
        let expanded = expand(source);
        assert!(expanded.starts_with("FADE IN:\n"), "{expanded:?}");
        assert!(expanded.contains("MAYA\nForty-one."), "{expanded:?}");
    }
}
