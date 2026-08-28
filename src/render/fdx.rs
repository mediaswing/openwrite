//! Final Draft XML (`.fdx`) export, so a script can leave here for a
//! production workflow. Final Draft repaginates on open, so this is written
//! from the elements rather than from laid-out pages.

use crate::element::{Element, Screenplay, Speech, SpeechPart};
use crate::inline::{self, Rich};
use crate::render::html::escape;
use std::fmt::Write;

pub fn render(doc: &Screenplay) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    out.push_str("<FinalDraft DocumentType=\"Script\" Template=\"No\" Version=\"1\">\n");
    out.push_str("  <Content>\n");

    for element in &doc.elements {
        match element {
            Element::SceneHeading { text, scene_number } => {
                let number = scene_number
                    .as_deref()
                    .map(|n| format!(" Number=\"{}\"", escape(n)))
                    .unwrap_or_default();
                paragraph(&mut out, "Scene Heading", &upper(text), &number);
            }
            Element::Action { text, centered } => {
                let attrs = if *centered { " Alignment=\"Center\"" } else { "" };
                for line in inline::plain_text(text).split('\n') {
                    paragraph_text(&mut out, "Action", line, attrs);
                }
            }
            Element::Dialogue(speech) => speech_xml(&mut out, speech, false),
            Element::DualDialogue(left, right) => {
                speech_xml(&mut out, left, true);
                speech_xml(&mut out, right, true);
            }
            Element::Transition(text) => paragraph(&mut out, "Transition", &upper(text), ""),
            Element::PageBreak => {
                out.push_str("    <Paragraph Type=\"Action\" StartsNewPage=\"Yes\"><Text></Text></Paragraph>\n");
            }
            Element::Section { .. } | Element::Synopsis(_) => {}
        }
    }

    out.push_str("  </Content>\n");
    title_page(&mut out, doc);
    out.push_str("</FinalDraft>\n");
    out
}

fn speech_xml(out: &mut String, speech: &Speech, dual: bool) {
    let attrs = if dual { " DualDialogue=\"Yes\"" } else { "" };
    paragraph(out, "Character", &upper(&speech.character), attrs);
    for part in &speech.parts {
        match part {
            SpeechPart::Parenthetical(t) => paragraph(out, "Parenthetical", t, attrs),
            SpeechPart::Line(t) | SpeechPart::Lyric(t) => {
                if inline::plain_text(t).trim().is_empty() {
                    continue;
                }
                paragraph(out, "Dialogue", t, attrs);
            }
        }
    }
}

fn paragraph(out: &mut String, kind: &str, text: &Rich, attrs: &str) {
    paragraph_text(out, kind, &inline::plain_text(text), attrs);
}

fn paragraph_text(out: &mut String, kind: &str, text: &str, attrs: &str) {
    let _ = writeln!(
        out,
        "    <Paragraph Type=\"{kind}\"{attrs}><Text>{}</Text></Paragraph>",
        escape(text)
    );
}

fn title_page(out: &mut String, doc: &Screenplay) {
    if !doc.has_title_page() {
        return;
    }
    out.push_str("  <TitlePage>\n    <Content>\n");
    for (key, values) in &doc.title_page {
        for value in values {
            let _ = writeln!(
                out,
                "      <Paragraph Alignment=\"Center\"><Text>{}: {}</Text></Paragraph>",
                escape(key),
                escape(value)
            );
        }
    }
    out.push_str("    </Content>\n  </TitlePage>\n");
}

fn upper(text: &Rich) -> Rich {
    text.iter()
        .map(|s| crate::inline::Span::new(s.text.to_uppercase(), s.style))
        .collect()
}
