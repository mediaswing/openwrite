//! What the parser makes of Fountain's context-sensitive corners.

use openwrite::element::{Element, SpeechPart};
use openwrite::inline::plain_text;
use openwrite::parse;

fn elements(source: &str) -> Vec<Element> {
    parse(source).elements
}

#[test]
fn scene_headings_are_recognised_by_their_prefix() {
    for source in [
        "INT. KITCHEN - DAY\n\nShe waits.\n",
        "EXT. STREET - NIGHT\n\nRain.\n",
        "EST. THE TOWER - DAWN\n\nIt looms.\n",
        "INT./EXT. CAR - MOVING\n\nThey drive.\n",
        "I/E. CAR - DAY\n\nThey drive.\n",
    ] {
        assert!(
            matches!(elements(source).first(), Some(Element::SceneHeading { .. })),
            "not a scene heading: {source:?}"
        );
    }
}

#[test]
fn a_leading_dot_forces_a_scene_heading_and_two_dots_do_not() {
    let forced = elements(".BLACK\n\nNothing.\n");
    assert!(matches!(forced.first(), Some(Element::SceneHeading { .. })));

    let action = elements("..ellipsis leading\n");
    assert!(matches!(action.first(), Some(Element::Action { .. })));
}

#[test]
fn a_scene_number_is_lifted_off_the_heading() {
    let parsed = elements("INT. KITCHEN - DAY #1A#\n\nShe waits.\n");
    match &parsed[0] {
        Element::SceneHeading { text, scene_number } => {
            assert_eq!(plain_text(text), "INT. KITCHEN - DAY");
            assert_eq!(scene_number.as_deref(), Some("1A"));
        }
        other => panic!("expected a scene heading, got {other:?}"),
    }
}

#[test]
fn a_capitalised_line_above_text_is_a_character_cue() {
    let parsed = elements("MAYA\nHello.\n");
    match &parsed[0] {
        Element::Dialogue(speech) => {
            assert_eq!(speech.character_name(), "MAYA");
            assert_eq!(speech.parts.len(), 1);
        }
        other => panic!("expected dialogue, got {other:?}"),
    }
}

#[test]
fn a_capitalised_line_with_nothing_under_it_is_action() {
    // Otherwise every shouted line of action becomes a character cue.
    let parsed = elements("She turns.\n\nTHE DOOR SLAMS.\n\nSilence.\n");
    assert_eq!(parsed.len(), 3);
    assert!(matches!(parsed[1], Element::Action { .. }));
}

#[test]
fn parentheticals_and_lyrics_are_parts_of_a_speech() {
    let parsed = elements("MAYA\n(quietly)\nHello.\n~And so we sing\n");
    match &parsed[0] {
        Element::Dialogue(speech) => {
            assert!(matches!(speech.parts[0], SpeechPart::Parenthetical(_)));
            assert!(matches!(speech.parts[1], SpeechPart::Line(_)));
            assert!(matches!(speech.parts[2], SpeechPart::Lyric(_)));
        }
        other => panic!("expected dialogue, got {other:?}"),
    }
}

#[test]
fn a_caret_pairs_a_speech_with_the_one_before_it() {
    let parsed = elements("MAYA\nFirst.\n\nGEORGE ^\nSecond.\n");
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        Element::DualDialogue(left, right) => {
            assert_eq!(left.character_name(), "MAYA");
            assert_eq!(right.character_name(), "GEORGE");
        }
        other => panic!("expected dual dialogue, got {other:?}"),
    }
}

#[test]
fn an_extension_does_not_change_who_is_speaking() {
    let parsed = elements("MAYA (V.O.)\nHello.\n");
    match &parsed[0] {
        Element::Dialogue(speech) => assert_eq!(speech.character_name(), "MAYA"),
        other => panic!("expected dialogue, got {other:?}"),
    }
}

#[test]
fn transitions_end_in_to_and_can_be_forced() {
    assert!(matches!(elements("Out.\n\nCUT TO:\n\nIN.\n")[1], Element::Transition(_)));
    assert!(matches!(elements("> Smash cut\n")[0], Element::Transition(_)));
}

#[test]
fn a_line_between_angle_brackets_is_centred() {
    match &elements("> THE END <\n")[0] {
        Element::Action { text, centered } => {
            assert!(centered);
            assert_eq!(plain_text(text), "THE END");
        }
        other => panic!("expected centred action, got {other:?}"),
    }
}

#[test]
fn the_title_page_is_read_including_indented_continuations() {
    let doc = parse("Title:\n    THE LAST BUS\n    A Film\nAuthor: A. Writer\n\nINT. HOUSE - DAY\n");
    assert_eq!(doc.meta("Title").unwrap(), ["THE LAST BUS", "A Film"]);
    assert_eq!(doc.meta_line(&["Author"]).unwrap(), "A. Writer");
    assert_eq!(doc.elements.len(), 1);
}

#[test]
fn a_document_with_no_title_page_keeps_its_first_line() {
    let doc = parse("INT. HOUSE - DAY\n\nShe waits.\n");
    assert!(!doc.has_title_page());
    assert_eq!(doc.elements.len(), 2);
}

#[test]
fn notes_and_the_boneyard_are_removed() {
    let doc = parse("She waits[[ check this ]].\n\n/* cut for now\n\nINT. GONE - DAY\n*/\n\nShe leaves.\n");
    let text: Vec<String> = doc
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Action { text, .. } => Some(plain_text(text)),
            _ => None,
        })
        .collect();
    assert_eq!(text, ["She waits.", "She leaves."]);
}

#[test]
fn sections_and_synopses_are_parsed_but_kept_out_of_the_script() {
    let doc = parse("# Act One\n\n= She finally says it.\n\nINT. HOUSE - DAY\n");
    assert!(matches!(doc.elements[0], Element::Section { level: 1, .. }));
    assert!(matches!(doc.elements[1], Element::Synopsis(_)));

    let opts = openwrite::layout::Options::default();
    let pages = openwrite::layout::paginate(&doc, &opts);
    let printed = openwrite::render::text::render(&pages, &opts, false);
    assert!(!printed.contains("Act One"));
    assert!(!printed.contains("finally says it"));
}

#[test]
fn windows_line_endings_parse_the_same_as_unix_ones() {
    let unix = parse("INT. HOUSE - DAY\n\nShe waits.\n");
    let windows = parse("INT. HOUSE - DAY\r\n\r\nShe waits.\r\n");
    assert_eq!(unix, windows);
}

// -- the one-line shorthand -------------------------------------------------
//
// `MAYA: Forty-one.` is not Fountain, and is understood anyway: see
// `src/shorthand.rs` for why, and for the expansion that keeps exported files
// honest.


fn speeches(source: &str) -> Vec<(String, Vec<String>)> {
    openwrite::parse(source)
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Dialogue(speech) => Some((
                speech.character_name(),
                speech
                    .parts
                    .iter()
                    .map(|p| match p {
                        SpeechPart::Parenthetical(t) => {
                            format!("({})", openwrite::inline::plain_text(t).trim_matches(['(', ')']))
                        }
                        other => openwrite::inline::plain_text(other.text()),
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_name_a_colon_and_a_line_is_a_speech() {
    let doc = "INT. SALT HOUSE - NIGHT\n\nMAYA: Forty-one.\n";
    assert_eq!(speeches(doc), vec![("MAYA".to_string(), vec!["Forty-one.".to_string()])]);
}

#[test]
fn an_exchange_needs_no_blank_lines_between_its_halves() {
    let doc = "INT. SALT HOUSE - NIGHT\n\nMAYA: One.\nDEV: Two.\nMAYA: Three.\n";
    let names: Vec<String> = speeches(doc).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["MAYA", "DEV", "MAYA"]);
}

#[test]
fn brackets_before_the_colon_become_a_parenthetical() {
    let doc = "INT. HOUSE - DAY\n\nDEV (not looking up): You know what this is.\n";
    assert_eq!(
        speeches(doc),
        vec![(
            "DEV".to_string(),
            vec!["(not looking up)".to_string(), "You know what this is.".to_string()]
        )]
    );
}

#[test]
fn a_voice_over_stays_on_the_cue_rather_than_under_it() {
    let doc = "INT. HOUSE - DAY\n\nMAYA (V.O.): Forty-one.\n";
    let parsed = speeches(doc);
    // The extension is part of the cue, so the speech has only the line in it.
    assert_eq!(parsed[0].0, "MAYA");
    assert_eq!(parsed[0].1, vec!["Forty-one.".to_string()]);
}

#[test]
fn the_speech_ends_at_the_end_of_the_line() {
    let doc = "INT. HOUSE - DAY\n\nMAYA: Forty-one.\nShe puts the tally down.\n";
    let elements = openwrite::parse(doc).elements;
    assert!(matches!(elements[1], Element::Dialogue(_)));
    match &elements[2] {
        Element::Action { text, .. } => {
            assert_eq!(openwrite::inline::plain_text(text), "She puts the tally down.")
        }
        other => panic!("expected action after the speech, got {other:?}"),
    }
}

#[test]
fn a_colon_in_the_middle_of_a_paragraph_stays_a_colon() {
    let doc = "INT. HOUSE - DAY\n\nShe reads the sign.\nNO ENTRY: KEEP OUT\n";
    // The second line is inside a paragraph, so it is not a cue.
    assert_eq!(speeches(doc), vec![]);
}

#[test]
fn a_transition_can_be_written_with_a_slash() {
    let doc = "INT. HOUSE - DAY\n\n/transition:cut to\n\nEXT. STREET - DAY\n";
    let transitions: Vec<String> = openwrite::parse(doc)
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Transition(t) => Some(openwrite::inline::plain_text(t)),
            _ => None,
        })
        .collect();
    assert_eq!(transitions, vec!["CUT TO:".to_string()]);
}

#[test]
fn a_screenplay_typed_in_shorthand_lays_out_like_one_typed_in_fountain() {
    let shorthand = "INT. HOUSE - DAY\n\nMAYA (quietly): Forty-one.\n\n/transition:cut to\n";
    let fountain = "INT. HOUSE - DAY\n\nMAYA\n(quietly)\nForty-one.\n\nCUT TO:\n";
    assert_eq!(openwrite::parse(shorthand), openwrite::parse(fountain));
}

#[test]
fn a_document_that_opens_with_a_cue_is_dialogue_not_a_title_page() {
    // Somebody typing into an empty editor starts here.
    let doc = "MAYA: Forty-one.\n";
    assert!(!openwrite::parse(doc).has_title_page());
    assert_eq!(speeches(doc), vec![("MAYA".to_string(), vec!["Forty-one.".to_string()])]);
}

#[test]
fn a_title_page_in_capitals_is_still_a_title_page() {
    let doc = "TITLE: Ashfen\nAUTHOR: W. Richards\n\nINT. HOUSE - DAY\n";
    let parsed = openwrite::parse(doc);
    assert!(parsed.has_title_page());
    assert_eq!(parsed.meta_line(&["Title"]).as_deref(), Some("Ashfen"));
    assert_eq!(parsed.meta_line(&["Author"]).as_deref(), Some("W. Richards"));
}

#[test]
fn a_forced_heading_or_a_section_with_a_colon_keeps_its_sigil() {
    // Both parsed correctly before the one-line form existed, and both are
    // uppercase-with-a-colon, so the cue check has to sit below the sigils.
    match &elements("INT. HOUSE - DAY\n\n.SALT HOUSE: NIGHT\n")[1] {
        Element::SceneHeading { text, .. } => {
            assert_eq!(plain_text(text), "SALT HOUSE: NIGHT")
        }
        other => panic!("expected a forced scene heading, got {other:?}"),
    }
    match &elements("# ACT ONE: THE FALL\n\nINT. HOUSE - DAY\n")[0] {
        Element::Section { text, .. } => assert_eq!(text, "ACT ONE: THE FALL"),
        other => panic!("expected a section, got {other:?}"),
    }
}

#[test]
fn a_caption_is_action_rather_than_somebody_speaking() {
    let doc = "INT. HOUSE - DAY\n\nSUPER: THREE YEARS LATER\n";
    assert_eq!(speeches(doc), vec![], "SUPER is a caption, not a character");
}

#[test]
fn a_line_of_non_ascii_text_parses_without_crashing() {
    // This panicked: the shorthand looked at byte twelve of every line.
    for source in [
        "INT. HOUSE - DAY\n\n\u{201C}I know,\u{201D} she says.\n",
        "INT. CAF\u{c9} \u{2014} DAY\n\nShe waits \u{2014} and waits.\n",
        "\u{1f3ac}\n",
    ] {
        let _ = parse(source);
    }
}
