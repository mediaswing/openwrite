//! The formatter's job: put every element in the column the industry expects,
//! and break pages where a reader would.

use openwrite::layout::{self, LineKind, Options};
use openwrite::render::{self, Format};

/// Format to text without a title page, so line 0 is the first line of script.
fn script(source: &str) -> String {
    let mut opts = Options::default();
    opts.title_page = false;
    opts.page_numbers = false;
    let doc = openwrite::parse(source);
    let pages = layout::paginate(&doc, &opts);
    render::text::render(&pages, &opts, false)
}

/// The indent of the first line containing `needle`.
fn indent_of(text: &str, needle: &str) -> usize {
    let line = text
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line containing {needle:?} in:\n{text}"));
    line.len() - line.trim_start().len()
}

#[test]
fn every_element_lands_in_its_own_column() {
    let text = script(
        "INT. KITCHEN - DAY\n\nShe waits.\n\nMAYA\n(quietly)\nHello.\n\nCUT TO:\n",
    );
    // Measured from the left margin: the 6 inch text column of a US Letter
    // page in 12 point Courier.
    assert_eq!(indent_of(&text, "INT. KITCHEN"), 0, "scene heading");
    assert_eq!(indent_of(&text, "She waits"), 0, "action");
    assert_eq!(indent_of(&text, "MAYA"), 20, "character cue");
    assert_eq!(indent_of(&text, "(quietly)"), 15, "parenthetical");
    assert_eq!(indent_of(&text, "Hello."), 10, "dialogue");
    // Transitions are flush right.
    let transition = text.lines().find(|l| l.contains("CUT TO:")).unwrap();
    assert_eq!(transition.len(), 60, "transition should end at the right margin");
}

#[test]
fn headings_cues_and_transitions_are_capitalised() {
    let text = script(".a quiet room\n\nmaya waits.\n\n@maya\nhello.\n\n> fade to black\n");
    assert!(text.contains("A QUIET ROOM"), "scene heading:\n{text}");
    assert!(text.contains("MAYA\n"), "character cue:\n{text}");
    assert!(text.contains("FADE TO BLACK"), "transition:\n{text}");
    // Action and dialogue keep the writer's capitalisation.
    assert!(text.contains("maya waits."), "action:\n{text}");
    assert!(text.contains("hello."), "dialogue:\n{text}");
}

#[test]
fn action_wraps_at_sixty_characters_and_dialogue_at_thirty_five() {
    let long = "word ".repeat(60);
    let text = script(&format!("INT. HOUSE - DAY\n\n{long}\n\nMAYA\n{long}\n"));
    for line in text.lines() {
        assert!(line.len() <= 60, "line runs past the right margin: {line:?}");
    }
    let dialogue: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("          word"))
        .collect();
    assert!(!dialogue.is_empty(), "no dialogue lines in:\n{text}");
    for line in dialogue {
        assert!(line.len() <= 45, "dialogue is wider than its column: {line:?}");
    }
}

#[test]
fn a_repeated_speaker_is_marked_contd() {
    // Two speeches by the same character, separated only by a parenthetical
    // beat, is the case (CONT'D) exists for.
    let source = "INT. HOUSE - DAY\n\nMAYA\nFirst.\n\nMAYA\nSecond.\n";
    assert!(script(source).contains("MAYA (CONT'D)"));

    let mut opts = Options::default();
    opts.title_page = false;
    opts.contd = false;
    let doc = openwrite::parse(source);
    let pages = layout::paginate(&doc, &opts);
    assert!(!render::text::render(&pages, &opts, false).contains("(CONT'D)"));
}

#[test]
fn an_intervening_scene_or_action_ends_the_continuation() {
    let text = script("MAYA\nFirst.\n\nShe turns.\n\nMAYA\nSecond.\n");
    assert!(!text.contains("(CONT'D)"), "action should break the run:\n{text}");
}

#[test]
fn a_speech_broken_over_a_page_gets_more_and_contd() {
    // A speech long enough that it cannot sit on one page.
    let speech = (1..=40)
        .map(|i| format!("This is line number {i} of a very long speech indeed."))
        .collect::<Vec<_>>()
        .join("\n");
    let text = script(&format!("INT. HOUSE - DAY\n\nMAYA\n{speech}\n"));

    assert!(text.contains("(MORE)"), "no (MORE) marker in:\n{text}");
    assert!(text.contains("MAYA (CONT'D)"), "no continuation cue in:\n{text}");

    // (MORE) sits at the cue column, and is the last thing on its page.
    let lines: Vec<&str> = text.lines().collect();
    let more = lines.iter().position(|l| l.contains("(MORE)")).unwrap();
    assert_eq!(lines[more].len() - lines[more].trim_start().len(), 20);
    assert!(lines[more + 1..].iter().any(|l| l.contains("MAYA (CONT'D)")));
}

#[test]
fn a_scene_heading_is_never_left_stranded_at_the_foot_of_a_page() {
    let opts = Options { title_page: false, ..Options::default() };

    // Walk the scene break across every position near a page boundary, so the
    // rule is tested at the exact line where it has to fire rather than at one
    // arbitrary length.
    for paragraphs in 20..40 {
        let filler = "Something happens.\n\n".repeat(paragraphs);
        let doc = openwrite::parse(&format!(
            "INT. FIRST - DAY\n\n{filler}INT. SECOND - DAY\n\nAnd then this.\n\nAnd this too.\n"
        ));
        let pages = layout::paginate(&doc, &opts);

        for (p, page) in pages.iter().enumerate() {
            for (i, line) in page.lines.iter().enumerate() {
                if line.kind != LineKind::SceneHeading {
                    continue;
                }
                // Only test the last line of a wrapped heading.
                if page.lines.get(i + 1).map(|l| l.kind) == Some(LineKind::SceneHeading) {
                    continue;
                }
                let following = page.lines[i + 1..].iter().filter(|l| !l.is_blank()).count();
                assert!(
                    following >= 2,
                    "with {paragraphs} paragraphs, the heading on page {p} is followed by \
                     only {following} lines:\n{}",
                    page.lines.iter().map(|l| l.to_text()).collect::<Vec<_>>().join("\n")
                );
            }
        }
    }
}

#[test]
fn no_page_runs_past_its_line_budget_or_starts_blank() {
    let opts = Options::default();
    let source = std::fs::read_to_string("examples/sample.fountain").unwrap();
    let doc = openwrite::parse(&source);
    let pages = layout::paginate(&doc, &opts);

    for (i, page) in pages.iter().enumerate() {
        assert!(
            page.lines.len() <= opts.lines_per_page,
            "page {i} has {} lines, over the {} line budget",
            page.lines.len(),
            opts.lines_per_page
        );
        if !page.is_title_page {
            assert!(!page.lines.is_empty(), "page {i} is empty");
            assert!(!page.lines[0].is_blank(), "page {i} starts with a blank line");
            assert!(!page.lines.last().unwrap().is_blank(), "page {i} ends blank");
        }
    }
}

#[test]
fn an_explicit_page_break_starts_a_new_page() {
    let opts = Options { title_page: false, ..Options::default() };
    let doc = openwrite::parse("INT. ONE - DAY\n\nFirst.\n\n===\n\nINT. TWO - DAY\n\nSecond.\n");
    let pages = layout::paginate(&doc, &opts);
    assert_eq!(pages.len(), 2);
    assert!(pages[1].lines[0].to_text().contains("INT. TWO"));
}

#[test]
fn pages_are_numbered_from_one_after_the_title_page() {
    let doc = openwrite::parse("Title: A Script\n\nINT. ONE - DAY\n\nFirst.\n\n===\n\nSecond.\n");
    let pages = layout::paginate(&doc, &Options::default());
    assert!(pages[0].is_title_page);
    assert_eq!(pages[0].number, None);
    assert_eq!(pages[1].number, Some(1));
    assert_eq!(pages[2].number, Some(2));
}

#[test]
fn dual_dialogue_is_set_in_two_columns() {
    let text = script("MAYA\nI really should.\n\nGEORGE ^\nIt is a big cake.\n");
    let cues = text.lines().find(|l| l.contains("MAYA")).unwrap();
    assert!(cues.contains("GEORGE"), "both cues share a line: {cues:?}");
    assert!(cues.len() <= 60);
}

#[test]
fn scene_numbers_appear_in_both_margins_when_asked_for() {
    let mut opts = Options { title_page: false, scene_numbers: true, ..Options::default() };
    opts.page_numbers = false;
    let doc = openwrite::parse("INT. KITCHEN - DAY #1A#\n\nShe waits.\n");
    let pages = layout::paginate(&doc, &opts);
    let text = render::text::render(&pages, &opts, false);
    let heading = text.lines().find(|l| l.contains("KITCHEN")).unwrap();
    assert!(heading.starts_with("1A "), "no number in the left margin: {heading:?}");
    assert!(heading.ends_with("1A"), "no number in the right margin: {heading:?}");
}

#[test]
fn emphasis_markers_are_stripped_from_the_printed_page() {
    let text = script("INT. HOUSE - DAY\n\nShe was *very* **quiet** and _still_.\n");
    assert!(text.contains("She was very quiet and still."), "{text}");
}

#[test]
fn html_output_is_escaped_and_carries_its_landmarks() {
    let doc = openwrite::parse("Title: A <Script>\n\nINT. KITCHEN - DAY\n\nTom & Jerry.\n\nMAYA\nHi.\n");
    let html = render::html::render(&doc, &Options::default());

    assert!(html.contains("&lt;Script&gt;"), "title not escaped");
    assert!(html.contains("Tom &amp; Jerry."), "action not escaped");
    assert!(html.contains(r#"<main id="screenplay""#), "no main landmark");
    assert!(html.contains(r#"class="skip-link""#), "no skip link");
    assert!(html.contains(r#"<nav class="scene-nav""#), "no scene navigation");
    assert!(html.contains(r#"role="status""#), "no live region");
    assert!(html.contains("<h2 class=\"slugline\""), "scene heading is not a heading");
    assert!(html.contains(r#"aria-label="Dialogue: MAYA""#), "dialogue is unlabelled");
    assert!(html.contains("@media print"), "no print rules");
}

#[test]
fn final_draft_export_is_well_formed_and_typed() {
    let doc = openwrite::parse("INT. KITCHEN - DAY\n\nShe waits.\n\nMAYA\n(quietly)\nHi & bye.\n");
    let fdx = render::fdx::render(&doc);

    assert!(fdx.starts_with("<?xml"));
    assert!(fdx.contains(r#"<Paragraph Type="Scene Heading">"#));
    assert!(fdx.contains(r#"<Paragraph Type="Character">"#));
    assert!(fdx.contains(r#"<Paragraph Type="Parenthetical">"#));
    assert!(fdx.contains("Hi &amp; bye."));
    assert_eq!(
        fdx.matches("<Paragraph").count(),
        fdx.matches("</Paragraph>").count(),
        "unbalanced paragraphs"
    );
}

#[test]
fn the_one_call_helper_agrees_with_the_long_way_round() {
    let source = std::fs::read_to_string("examples/sample.fountain").unwrap();
    let opts = Options::default();
    let doc = openwrite::parse(&source);
    let pages = layout::paginate(&doc, &opts);

    assert_eq!(
        openwrite::format(&source, &opts, Format::Text),
        render::text::render(&pages, &opts, false)
    );
    assert_eq!(
        openwrite::format(&source, &opts, Format::Html),
        render::html::render(&doc, &opts)
    );
}

#[test]
fn statistics_count_what_a_writer_would_count() {
    let doc = openwrite::parse(
        "INT. ONE - DAY\n\nShe waits here.\n\nMAYA\nTwo words.\n\nEXT. TWO - DAY\n\nHe leaves.\n",
    );
    let pages = layout::paginate(&doc, &Options::default());
    let stats = openwrite::stats::compute(&doc, &pages);

    assert_eq!(stats.scenes, 2);
    assert_eq!(stats.action_words, 5);
    assert_eq!(stats.dialogue_words, 2);
    assert_eq!(stats.words, 7);
    assert_eq!(stats.characters["MAYA"].cues, 1);
    assert_eq!(stats.characters["MAYA"].words, 2);
}
