//! The `.sct` working document.
//!
//! A screenplay in progress is more than its text: it is also where you had got
//! to, and what you know about the story that the script does not say. A `.sct`
//! file is the Fountain source with a small header in front of it recording
//! that — the caret, the scene you were reading, the world, the people — so
//! that reopening a draft puts you back where you left off rather than at line
//! one, with your notes still beside you.
//!
//! ```text
//! SCT/1
//! caret: 1402
//! scene: 3
//! world: Ashfen stands on a salt lake.\nThe Guild rules by writ.
//! character: MAYA
//! role: Salt-runner. The younger sister.
//! want: To buy her brother out of his indenture
//! voice: Short sentences. Never says what she means.
//! ---
//! Title: The Last Bus
//!
//! INT. BUS SHELTER - NIGHT
//! ...
//! ```
//!
//! Header values are one line each, so a note that runs to several lines is
//! written with `\n` for the breaks and `\\` for a literal backslash. That
//! keeps the header line-oriented — which is what makes it readable in a diff
//! and greppable on the command line — without limiting what can be typed into
//! it.
//!
//! The format is deliberately plain text and deliberately forgiving. Anything
//! without the `SCT/1` first line is read as bare Fountain, so a `.fountain`
//! file opens as a document and a `.sct` file that loses its header is still a
//! screenplay rather than a loss. Unknown header keys are ignored, so a file
//! written by a later version still opens here.

use crate::bible::{Bible, Profile};
use std::path::Path;

/// The extension of the tool's own documents.
pub const EXTENSION: &str = "sct";

/// First line of a `.sct` file. The digit is the format version.
const MAGIC: &str = "SCT/1";

/// The line that ends the header and begins the screenplay.
const SEPARATOR: &str = "---";

/// Where the writer had got to. Every field is optional: none of it is worth
/// failing to open a screenplay over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Working {
    /// Caret position, as a character offset into the source.
    pub caret: Option<usize>,
    /// The scene selected in the outline, counting from zero.
    pub scene: Option<usize>,
}

/// A screenplay, the state of the session that was writing it, and what the
/// writer knows about the story.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub source: String,
    pub working: Working,
    pub bible: Bible,
}

impl Document {
    pub fn new(source: impl Into<String>) -> Self {
        Document { source: source.into(), ..Default::default() }
    }
}

/// Does this path name one of the tool's own documents?
pub fn is_document(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case(EXTENSION))
        .unwrap_or(false)
}

/// Read a `.sct` file, or bare Fountain, into a document.
///
/// This never fails: a file whose header is damaged is read as a screenplay
/// with no saved position, which is the outcome that loses the least.
/// One line ending, everywhere, whatever the file arrived with.
///
/// A screenplay is a text file that gets carried between machines, and both
/// halves of this module normalise so that `read(write(d)) == d` holds for a
/// document that came from anywhere. Reading alone is not enough: a document
/// read from a CRLF file, written, and read back would otherwise not equal
/// itself.
fn one_line_ending(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn read(text: &str) -> Document {
    let text = one_line_ending(text);

    let Some(rest) = text.strip_prefix(MAGIC) else {
        return Document::new(text);
    };
    // The magic must be a line of its own, not the start of a longer word.
    let Some(rest) = rest.strip_prefix('\n') else {
        return Document::new(text);
    };
    // Without a separator the header never ended; trust the text, not the header.
    let Some((header, source)) = split_header(rest) else {
        return Document::new(text);
    };

    let mut working = Working::default();
    let mut bible = Bible::default();
    // Which character block the reader is inside, if any.
    let mut current: Option<usize> = None;
    for line in header.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "caret" => working.caret = value.parse().ok(),
            "scene" => working.scene = value.parse().ok(),
            "world" => bible.world = unescape(value),
            // A `character` line opens a block: the keys under it belong to
            // that character until the next one. A name that appears twice —
            // which only a hand-edited file would have — reopens the first
            // block rather than starting a second person with the same name.
            "character" => {
                let name = crate::bible::normalise(&unescape(value));
                current = (!name.is_empty()).then(|| {
                    bible.ensure(&name);
                    bible.position(&name).expect("just ensured")
                });
            }
            field @ ("role" | "age" | "want" | "voice" | "notes") => {
                // A field before any `character` line belongs to nobody, and is
                // dropped rather than guessed at.
                if let Some(profile) = current.and_then(|i| bible.profiles.get_mut(i)) {
                    let slot = match field {
                        "role" => &mut profile.role,
                        "age" => &mut profile.age,
                        "want" => &mut profile.want,
                        "voice" => &mut profile.voice,
                        _ => &mut profile.notes,
                    };
                    *slot = unescape(value);
                }
            }
            // Anything else was written by a version that knows more than this
            // one does. Ignoring it is what makes the file forward compatible.
            _ => {}
        }
    }

    Document { source: source.to_string(), working, bible }
}

/// Split at the first line that is exactly the separator.
fn split_header(rest: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == SEPARATOR {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Write a document in `.sct` form.
pub fn write(document: &Document) -> String {
    let mut out = String::with_capacity(document.source.len() + 256);
    out.push_str(MAGIC);
    out.push('\n');
    if let Some(caret) = document.working.caret {
        out.push_str(&format!("caret: {caret}\n"));
    }
    if let Some(scene) = document.working.scene {
        out.push_str(&format!("scene: {scene}\n"));
    }
    if !document.bible.world.is_empty() {
        out.push_str(&format!("world: {}\n", escape(&document.bible.world)));
    }
    for profile in &document.bible.profiles {
        write_profile(&mut out, profile);
    }
    out.push_str(SEPARATOR);
    out.push('\n');
    out.push_str(&one_line_ending(&document.source));
    out
}

/// Put an extra key into the header of an already-written document.
///
/// The `.sct` header is a list of `key: value` lines and [`read`] ignores every
/// key it does not know, which is what makes a file written by a later version
/// still open in this one. That same property lets something outside this
/// module keep a note of its own in a document without the format having to
/// grow a field for it — [`crate::recovery`] records which file a recovered
/// draft came from that way.
///
/// Only meaningful on the output of [`write`]: a document with no header has
/// nowhere to put this.
pub fn with_header(written: &str, key: &str, value: &str) -> String {
    let opening = format!("{MAGIC}\n");
    match written.strip_prefix(&opening) {
        Some(rest) => format!("{opening}{key}: {}\n{rest}", escape(value)),
        None => written.to_string(),
    }
}

/// Read one header value back, without reading the document around it.
///
/// The pair of [`with_header`]. Answers `None` for a file with no header, a
/// header that never ended, or a key that is not in it.
pub fn header_value(text: &str, key: &str) -> Option<String> {
    let text = one_line_ending(text);
    let rest = text.strip_prefix(MAGIC)?.strip_prefix('\n')?;
    let (header, _) = split_header(rest)?;
    header.lines().find_map(|line| {
        let (found, value) = line.split_once(':')?;
        (found.trim() == key).then(|| unescape(value.trim()))
    })
}

fn write_profile(out: &mut String, profile: &Profile) {
    out.push_str(&format!("character: {}\n", escape(&profile.name)));
    for (key, value) in [
        ("role", &profile.role),
        ("age", &profile.age),
        ("want", &profile.want),
        ("voice", &profile.voice),
        ("notes", &profile.notes),
    ] {
        // An empty field is left out entirely rather than written blank: the
        // header should say what is known, and nothing about what is not.
        if !value.is_empty() {
            out.push_str(&format!("{key}: {}\n", escape(value)));
        }
    }
}

/// Fold a value onto one header line.
///
/// Only three characters need it: the backslash that does the escaping, and
/// the two line endings that would otherwise end the value early.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Unfold a header value. An unknown escape is kept as it was typed, since a
/// stray backslash in a note is far likelier than a mistake in this file.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Write a document for a given path: `.sct` keeps the working state and the
/// story bible, anything else gets the screenplay on its own.
///
/// Saving as `.fountain` deliberately drops the header rather than smuggling it
/// into a file another tool has to read — and, for the same reason, writes out
/// the one-line dialogue shorthand in full. `MAYA: Forty-one.` is a line of
/// action to any other Fountain reader, and it would be right; see
/// [`crate::shorthand`].
pub fn write_for(path: &Path, document: &Document) -> String {
    if is_document(path) {
        write(document)
    } else {
        crate::shorthand::expand(&one_line_ending(&document.source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(caret: Option<usize>, scene: Option<usize>, source: &str) -> Document {
        Document {
            source: source.to_string(),
            working: Working { caret, scene },
            bible: Bible::default(),
        }
    }

    #[test]
    fn a_document_survives_the_round_trip() {
        let original = doc(Some(1402), Some(3), "INT. HOUSE - DAY\n\nShe waits.\n");
        assert_eq!(read(&write(&original)), original);
    }

    #[test]
    fn a_document_with_no_saved_position_round_trips_too() {
        let original = doc(None, None, "INT. HOUSE - DAY\n");
        assert_eq!(read(&write(&original)), original);
    }

    #[test]
    fn bare_fountain_is_read_as_a_screenplay_with_no_position() {
        let source = "Title: A Script\n\nINT. HOUSE - DAY\n\nShe waits.\n";
        assert_eq!(read(source), Document::new(source));
    }

    #[test]
    fn a_damaged_header_still_yields_the_screenplay() {
        // No separator: the header never ends, so the whole file is the script.
        let text = "SCT/1\ncaret: 12\nINT. HOUSE - DAY\n";
        assert_eq!(read(text).source, text);

        // A caret that is not a number is simply not a caret.
        let text = "SCT/1\ncaret: half past three\n---\nINT. HOUSE - DAY\n";
        let document = read(text);
        assert_eq!(document.working.caret, None);
        assert_eq!(document.source, "INT. HOUSE - DAY\n");
    }

    #[test]
    fn keys_from_a_later_version_are_ignored_rather_than_fatal() {
        let text = "SCT/1\ncaret: 5\nrevision-colour: salmon\n---\nINT. HOUSE - DAY\n";
        let document = read(text);
        assert_eq!(document.working.caret, Some(5));
        assert_eq!(document.source, "INT. HOUSE - DAY\n");
    }

    #[test]
    fn a_screenplay_that_merely_begins_with_the_magic_word_is_not_a_header() {
        let source = "SCT/1 is a format, she said.\n\nINT. HOUSE - DAY\n";
        assert_eq!(read(source).source, source);
    }

    #[test]
    fn a_separator_inside_the_screenplay_does_not_confuse_the_reader() {
        // `---` is not Fountain markup, but it can appear in dialogue.
        let original = doc(Some(1), None, "MAYA\nWait --- no.\n\n---\n\nEXT. STREET - DAY\n");
        assert_eq!(read(&write(&original)), original);
    }

    #[test]
    fn saving_as_fountain_drops_the_header() {
        let document = doc(Some(9), Some(1), "INT. HOUSE - DAY\n");
        let fountain = write_for(Path::new("script.fountain"), &document);
        assert_eq!(fountain, "INT. HOUSE - DAY\n");
        assert!(!fountain.contains(MAGIC));

        let native = write_for(Path::new("script.sct"), &document);
        assert!(native.starts_with(MAGIC));
    }

    #[test]
    fn the_extension_is_recognised_whatever_its_case() {
        assert!(is_document(Path::new("a.sct")));
        assert!(is_document(Path::new("a.SCT")));
        assert!(!is_document(Path::new("a.fountain")));
        assert!(!is_document(Path::new("a")));
    }

    #[test]
    fn windows_line_endings_are_normalised_on_the_way_in() {
        let document = read("SCT/1\r\ncaret: 7\r\n---\r\nINT. HOUSE - DAY\r\n");
        assert_eq!(document.working.caret, Some(7));
        assert_eq!(document.source, "INT. HOUSE - DAY\n");
    }

    // -- the story bible ----------------------------------------------------

    fn peopled() -> Document {
        let mut bible = Bible {
            world: "Ashfen stands on a salt lake.\nThe Guild rules by writ.".to_string(),
            ..Bible::default()
        };
        let maya = bible.ensure("MAYA");
        maya.role = "Salt-runner. The younger sister.".to_string();
        maya.age = "19".to_string();
        maya.want = "To buy her brother out of his indenture".to_string();
        maya.voice = "Short sentences. Never says what she means.".to_string();
        maya.notes = "Lost her mother in the flood.\nCannot swim.".to_string();
        bible.ensure("DEV").role = "Guild clerk".to_string();
        Document {
            source: "INT. SALT HOUSE - NIGHT\n\nMaya counts.\n".to_string(),
            working: Working { caret: Some(12), scene: Some(0) },
            bible,
        }
    }

    #[test]
    fn the_story_bible_survives_the_round_trip() {
        let original = peopled();
        assert_eq!(read(&write(&original)), original);
    }

    #[test]
    fn a_note_that_runs_to_several_lines_stays_on_one_header_line() {
        let written = write(&peopled());
        let header = written.split("\n---\n").next().unwrap();
        assert!(header.contains("notes: Lost her mother in the flood.\\nCannot swim."));
        // The header is line-oriented; nothing in it may span lines.
        assert_eq!(header.lines().filter(|l| l.starts_with("notes:")).count(), 1);
    }

    #[test]
    fn a_backslash_in_a_note_is_still_a_backslash_when_it_comes_back() {
        let mut document = Document::new("INT. HOUSE - DAY\n");
        document.bible.ensure("MAYA").notes = "Reads C:\\notes\\ and \\n aloud".to_string();
        let back = read(&write(&document));
        assert_eq!(back.bible.get("MAYA").unwrap().notes, "Reads C:\\notes\\ and \\n aloud");
    }

    #[test]
    fn a_colon_in_a_value_belongs_to_the_value() {
        let mut document = Document::new("INT. HOUSE - DAY\n");
        document.bible.world = "One rule: nobody crosses the lake at night.".to_string();
        let back = read(&write(&document));
        assert_eq!(back.bible.world, "One rule: nobody crosses the lake at night.");
    }

    #[test]
    fn characters_keep_their_order_and_their_fields_apart() {
        let back = read(&write(&peopled()));
        let names: Vec<&str> = back.bible.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["MAYA", "DEV"]);
        assert_eq!(back.bible.get("DEV").unwrap().role, "Guild clerk");
        // DEV's blank fields did not inherit MAYA's.
        assert!(back.bible.get("DEV").unwrap().want.is_empty());
    }

    #[test]
    fn a_field_with_no_character_above_it_is_dropped_rather_than_guessed_at() {
        let text = "SCT/1\nwant: something\n---\nINT. HOUSE - DAY\n";
        let document = read(text);
        assert!(document.bible.profiles.is_empty());
        assert_eq!(document.source, "INT. HOUSE - DAY\n");
    }

    #[test]
    fn an_empty_bible_writes_no_header_lines_for_it() {
        let written = write(&doc(Some(1), None, "INT. HOUSE - DAY\n"));
        assert!(!written.contains("world:"));
        assert!(!written.contains("character:"));
    }

    #[test]
    fn saving_as_fountain_writes_the_shorthand_out_in_full() {
        // Another tool would read `MAYA: Forty-one.` as action, so it does not
        // leave this program in that form.
        let document = Document::new("INT. HOUSE - DAY\n\nMAYA (quietly): Forty-one.\n");
        let fountain = write_for(Path::new("script.fountain"), &document);
        assert_eq!(fountain, "INT. HOUSE - DAY\n\nMAYA\n(quietly)\nForty-one.\n");

        // And the tool's own file keeps what was typed.
        let native = write_for(Path::new("script.sct"), &document);
        assert!(native.contains("MAYA (quietly): Forty-one."));
    }

    #[test]
    fn the_expanded_fountain_is_the_same_screenplay() {
        let document = Document::new(
            "INT. HOUSE - DAY\n\nMAYA: One.\nDEV: Two.\n\n/transition:cut to\n",
        );
        let fountain = write_for(Path::new("script.fountain"), &document);
        assert_eq!(crate::parse(&fountain), crate::parse(&document.source));
    }

    #[test]
    fn saving_as_fountain_drops_the_bible_too() {
        let fountain = write_for(Path::new("script.fountain"), &peopled());
        assert!(!fountain.contains("Salt-runner"));
        assert!(fountain.starts_with("INT. SALT HOUSE - NIGHT"));
    }

    #[test]
    fn an_older_reader_would_still_find_the_screenplay() {
        // The separator is what an older version looks for, and every bible
        // line sits in front of it as an ordinary unknown key.
        let written = write(&peopled());
        let (header, source) = written.split_once("\n---\n").unwrap();
        assert!(header.lines().all(|l| l == MAGIC || l.contains(':')));
        assert!(source.starts_with("INT. SALT HOUSE - NIGHT"));
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn an_extra_header_key_survives_the_round_trip_and_disturbs_nothing() {
        let mut document = Document::new("INT. KITCHEN - DAY\n\nShe waits.\n");
        document.working.caret = Some(12);
        document.bible.world = "A salt lake.".to_string();

        let written = with_header(&write(&document), "origin", "/home/a/The Last Bus.sct");
        assert_eq!(
            header_value(&written, "origin").as_deref(),
            Some("/home/a/The Last Bus.sct")
        );
        // And the document itself reads back exactly as it would have without
        // it: an unknown key is ignored, which is the property this relies on.
        assert_eq!(read(&written), document);
        assert_eq!(read(&written), read(&write(&document)));
    }

    #[test]
    fn a_path_with_awkward_characters_in_it_comes_back_whole() {
        // Header values are one line each, so a newline in a path — which is
        // legal on Unix — has to survive as an escape rather than end the line.
        for origin in ["/tmp/a: b.sct", "/tmp/back\\slash.sct", "/tmp/two\nlines.sct"] {
            let written = with_header(&write(&Document::new("A.\n")), "origin", origin);
            assert_eq!(header_value(&written, "origin").as_deref(), Some(origin), "{origin:?}");
            assert_eq!(read(&written).source, "A.\n");
        }
    }

    #[test]
    fn a_file_with_no_header_has_no_values_and_is_not_given_one() {
        let bare = "INT. KITCHEN - DAY\n";
        assert_eq!(header_value(bare, "origin"), None);
        // Nowhere to put it, so it is left exactly as it was rather than
        // being given a header it never had.
        assert_eq!(with_header(bare, "origin", "/tmp/x"), bare);
    }

    #[test]
    fn a_key_that_is_not_there_is_none_rather_than_an_empty_string() {
        let written = write(&Document::new("A.\n"));
        assert_eq!(header_value(&written, "origin"), None);
    }
}
