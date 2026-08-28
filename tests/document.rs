//! The `.sct` working document, end to end: what the tool saves is what the
//! tool reads back, and the header never reaches the printed page.

use openwrite::document::{self, Document, Working};
use openwrite::layout::{self, Options};

/// The sample screenplay, with the line endings settled.
///
/// Checked-out line endings are a property of whoever cloned the repository,
/// not of the screenplay: git on Windows may well hand these tests a CRLF file.
/// The tool normalises what it reads, so these tests compare against what the
/// tool works with rather than against whatever happens to be on this disk.
fn sample() -> String {
    std::fs::read_to_string("examples/sample.fountain")
        .unwrap()
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// The sample exactly as it sits on disk, line endings and all.
fn sample_on_disk() -> String {
    std::fs::read_to_string("examples/sample.fountain").unwrap()
}

#[test]
fn a_saved_draft_reopens_with_the_screenplay_and_the_place_intact() {
    let saved = Document {
        source: sample(),
        working: Working { caret: Some(1402), scene: Some(2) },
        ..Default::default()
    };
    let reopened = document::read(&document::write(&saved));

    assert_eq!(reopened.source, saved.source);
    assert_eq!(reopened.working.caret, Some(1402));
    assert_eq!(reopened.working.scene, Some(2));
}

#[test]
fn the_header_never_reaches_the_printed_page() {
    let saved = Document {
        source: sample(),
        working: Working { caret: Some(42), scene: Some(1) },
        ..Default::default()
    };
    let opts = Options::default();

    let from_document = openwrite::parse(&document::read(&document::write(&saved)).source);
    let from_fountain = openwrite::parse(&sample());
    assert_eq!(from_document, from_fountain, "the header changed the screenplay");

    let printed = openwrite::render::text::render(
        &layout::paginate(&from_document, &opts),
        &opts,
        false,
    );
    assert!(!printed.contains("SCT/1"), "the magic line was printed");
    assert!(!printed.contains("caret:"), "the header was printed");
}

#[test]
fn a_fountain_file_opens_as_a_document_without_ceremony() {
    let opened = document::read(&sample_on_disk());
    assert_eq!(opened.source, sample());
    assert_eq!(opened.working, Working::default());
}

/// The same screenplay typed on Windows opens as the same screenplay.
///
/// This is the case that broke the first release build: the file on a Windows
/// runner arrives with CRLF, and a round trip has to survive that rather than
/// depend on it.
#[test]
fn a_draft_written_on_windows_round_trips_like_any_other() {
    let windows = sample().replace('\n', "\r\n");
    let saved = Document {
        source: windows,
        working: Working { caret: Some(1402), scene: Some(2) },
        ..Default::default()
    };
    let reopened = document::read(&document::write(&saved));

    assert_eq!(reopened.source, sample(), "line endings were not settled");
    assert_eq!(reopened.working, saved.working);
    // And the fixed point holds from there on.
    assert_eq!(document::read(&document::write(&reopened)), reopened);
}

#[test]
fn a_draft_saved_as_fountain_is_ordinary_fountain() {
    let saved = Document {
        source: sample(),
        working: Working { caret: Some(42), scene: Some(1) },
        ..Default::default()
    };
    let written = document::write_for(std::path::Path::new("out.fountain"), &saved);
    assert_eq!(written, sample());
}

#[test]
fn a_caret_beyond_a_shortened_screenplay_is_still_readable() {
    // Saved at character 5000, then the screenplay was cut down to nothing
    // much. Reading must not panic or lose the text; clamping is the app's job,
    // but the document has to survive the trip.
    let text = "SCT/1\ncaret: 5000\n---\nINT. HOUSE - DAY\n";
    let document = document::read(text);
    assert_eq!(document.working.caret, Some(5000));
    assert_eq!(document.source, "INT. HOUSE - DAY\n");
    assert!(document.working.caret.unwrap() > document.source.chars().count());
}
