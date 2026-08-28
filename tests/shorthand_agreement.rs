//! The expander and the parser have to agree.
//!
//! `document::write_for` runs the expander over anything saved as `.fountain`,
//! so if the two ever disagree the file on disk is a different screenplay from
//! the one on screen. This checks the property directly over the awkward cases.

const AWKWARD: &[&str] = &[
    "MAYA: Forty-one.\n",
    "INT. HOUSE - DAY\n\nMAYA: One.\nDEV: Two.\nMAYA: Three.\n",
    "INT. HOUSE - DAY\n\nThe camera pans.\nMAYA: Forty-one.\n",
    "INT. HOUSE - DAY\n\nMAYA\nDEV: I know what you did.\n",
    "INT. HOUSE - DAY\n\n.SALT HOUSE: NIGHT\n",
    "# ACT ONE: THE FALL\n\nINT. HOUSE - DAY\n\nMAYA: Yes.\n",
    "= She finally says it: no.\n\nMAYA: No.\n",
    "FADE IN:\n\nINT. HOUSE - DAY\n\nMAYA: One.\n",
    "INT. HOUSE - DAY\n\nSUPER: THREE YEARS LATER\n",
    "Title: Ashfen\nAUTHOR: W. Richards\n\nMAYA: One.\n",
    "/* a\nb\nc */\nTitle: Ashfen\nAUTHOR: W. Richards\n\nMAYA: One.\n",
    "INT. HOUSE - DAY\n\nMAYA (quietly): One.\n\n/transition:cut to\n\nEXT. STREET - DAY\n",
    "INT. CAF\u{c9} \u{2014} DAY\n\n\u{201c}I know,\u{201d} she says.\n\nMAYA: \u{201c}No.\u{201d}\n",
    "INT. HOUSE - DAY\n\nMAYA: One.\n\n\n\nShe waits.\n",
    "MAYA (V.O.): One.\nDEV (CONT'D): Two.\n",
    "INT. HOUSE - DAY\n\nMAYA ^\nDual.\n\nDEV\nAlso dual.\n",
    "",
    "\n\n\n",
];

#[test]
fn expanding_never_changes_the_screenplay() {
    for source in AWKWARD {
        let expanded = openwrite::shorthand::expand(source);
        assert_eq!(
            openwrite::parse(&expanded),
            openwrite::parse(source),
            "expanding changed the screenplay:\n--- from ---\n{source}\n--- to ---\n{expanded}"
        );
    }
}

#[test]
fn expanding_is_a_fixed_point() {
    for source in AWKWARD {
        let once = openwrite::shorthand::expand(source);
        assert_eq!(openwrite::shorthand::expand(&once), once, "not settled for {source:?}");
    }
}

#[test]
fn an_expanded_screenplay_has_no_shorthand_left_in_it() {
    for source in AWKWARD {
        let expanded = openwrite::shorthand::expand(source);
        let start = openwrite::parser::title_page_span(&expanded);
        for (i, line) in expanded.split('\n').enumerate().skip(start) {
            let blank_before = i == start || expanded.split('\n').nth(i - 1).is_none_or(|l| l.trim().is_empty());
            if blank_before {
                assert!(
                    openwrite::shorthand::cue(line).is_none(),
                    "{line:?} is still shorthand after expanding {source:?}"
                );
            }
            assert!(openwrite::shorthand::transition(line).is_none(), "{line:?} in {source:?}");
        }
    }
}
