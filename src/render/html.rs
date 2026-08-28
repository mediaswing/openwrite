//! Accessible, printable HTML.
//!
//! Unlike the text renderer this works from the parsed elements rather than
//! laid-out lines, so the document keeps real semantics — headings for scenes,
//! paragraphs for speech — which is what a screen reader needs. Screenplay
//! geometry is reproduced in CSS `ch` units, and `@page` rules make printing to
//! PDF produce a correctly margined script.

use crate::element::{Element, Screenplay, Speech, SpeechPart};
use crate::inline::{Rich, Span};
use crate::layout::Options;
use std::fmt::Write;

pub fn render(doc: &Screenplay, opts: &Options) -> String {
    let title = doc.meta_line(&["Title"]).unwrap_or_else(|| "Screenplay".to_string());
    let mut out = String::new();

    let _ = write!(
        out,
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>{}</style>
</head>
<body>
<a class="skip-link" href="#screenplay">Skip to screenplay</a>
"##,
        escape(&title),
        CSS
    );

    if opts.title_page && doc.has_title_page() {
        title_page(&mut out, doc);
    }

    scene_nav(&mut out, doc);

    let _ = writeln!(
        out,
        r#"<main id="screenplay" class="script" tabindex="-1" aria-label="Screenplay: {}">"#,
        escape(&title)
    );

    let mut scene = 0usize;
    let mut open_section = false;
    for element in &doc.elements {
        match element {
            Element::SceneHeading { text, scene_number } => {
                if open_section {
                    out.push_str("</section>\n");
                }
                scene += 1;
                let heading = plain(text).to_uppercase();
                let _ = writeln!(
                    out,
                    r#"<section class="scene" id="scene-{scene}" aria-labelledby="scene-{scene}-heading" tabindex="-1">"#
                );
                let number = scene_number
                    .as_deref()
                    .map(|n| format!(r#" <span class="scene-number" aria-label="Scene number {}">{}</span>"#, escape(n), escape(n)))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    r#"<h2 class="slugline" id="scene-{scene}-heading">{}{number}</h2>"#,
                    escape(&heading)
                );
                open_section = true;
            }
            Element::Action { text, centered } => {
                let class = if *centered { "action centered" } else { "action" };
                let _ = writeln!(out, r#"<p class="{class}">{}</p>"#, rich_html_multiline(text));
            }
            Element::Dialogue(speech) => {
                out.push_str(&speech_html(speech, None));
            }
            Element::DualDialogue(left, right) => {
                let _ = writeln!(out, r#"<div class="dual" role="group" aria-label="Simultaneous dialogue">"#);
                out.push_str(&speech_html(left, Some("Speaking simultaneously, first")));
                out.push_str(&speech_html(right, Some("Speaking simultaneously, second")));
                out.push_str("</div>\n");
            }
            Element::Transition(text) => {
                let _ = writeln!(
                    out,
                    r#"<p class="transition">{}</p>"#,
                    rich_html(&upper_rich(text))
                );
            }
            Element::PageBreak => out.push_str(r#"<hr class="page-break" aria-hidden="true">"#),
            Element::Section { level, text } => {
                // Outline structure: kept out of the printed script but exposed
                // to assistive technology as a landmark comment.
                let _ = writeln!(
                    out,
                    r#"<p class="outline-section" data-level="{level}">{}</p>"#,
                    escape(text)
                );
            }
            Element::Synopsis(text) => {
                let _ = writeln!(out, r#"<p class="outline-synopsis">{}</p>"#, escape(text));
            }
        }
    }
    if open_section {
        out.push_str("</section>\n");
    }

    out.push_str("</main>\n");
    let _ = write!(out, "{}", STATUS_AND_SCRIPT);
    out.push_str("</body>\n</html>\n");
    out
}

fn speech_html(speech: &Speech, aria: Option<&str>) -> String {
    let mut out = String::new();
    let name = escape(&speech.character_name());
    let label = match aria {
        Some(extra) => format!(r#" aria-label="{extra}: {name}""#),
        None => format!(r#" aria-label="Dialogue: {name}""#),
    };
    let _ = writeln!(out, r#"<div class="dialogue-block" role="group"{label}>"#);
    let _ = writeln!(
        out,
        r#"<p class="character">{}</p>"#,
        rich_html(&upper_rich(&speech.character))
    );
    for part in &speech.parts {
        match part {
            SpeechPart::Parenthetical(t) => {
                let _ = writeln!(out, r#"<p class="parenthetical">{}</p>"#, rich_html(t));
            }
            SpeechPart::Line(t) => {
                if plain(t).trim().is_empty() {
                    continue;
                }
                let _ = writeln!(out, r#"<p class="dialogue">{}</p>"#, rich_html(t));
            }
            SpeechPart::Lyric(t) => {
                let _ = writeln!(out, r#"<p class="lyric"><em>{}</em></p>"#, rich_html(t));
            }
        }
    }
    out.push_str("</div>\n");
    out
}

fn title_page(out: &mut String, doc: &Screenplay) {
    out.push_str(r#"<div class="title-page" role="region" aria-label="Title page">"#);
    out.push('\n');
    if let Some(title) = doc.meta("Title") {
        let _ = writeln!(out, "<h1>{}</h1>", escape(&title.join(" ")));
    }
    if let Some(authors) = doc.meta("Author").or_else(|| doc.meta("Authors")) {
        let credit = doc.meta_line(&["Credit"]).unwrap_or_else(|| "written by".into());
        let _ = writeln!(out, r#"<p class="credit">{}</p>"#, escape(&credit));
        let _ = writeln!(out, r#"<p class="author">{}</p>"#, escape(&authors.join(", ")));
    }
    if let Some(source) = doc.meta("Source") {
        let _ = writeln!(out, r#"<p class="source">{}</p>"#, escape(&source.join(" ")));
    }
    let mut footer = Vec::new();
    for key in ["Draft date", "Date", "Contact", "Copyright"] {
        if let Some(v) = doc.meta(key) {
            footer.push(format!(
                r#"<p class="meta"><span class="meta-key">{}:</span> {}</p>"#,
                escape(key),
                escape(&v.join(", "))
            ));
        }
    }
    if !footer.is_empty() {
        let _ = writeln!(out, r#"<div class="title-meta">{}</div>"#, footer.join("\n"));
    }
    out.push_str("</div>\n");
}

fn scene_nav(out: &mut String, doc: &Screenplay) {
    let scenes: Vec<String> = doc
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::SceneHeading { text, .. } => Some(plain(text).to_uppercase()),
            _ => None,
        })
        .collect();
    if scenes.is_empty() {
        return;
    }
    out.push_str(r#"<nav class="scene-nav" aria-label="Scenes"><h2 id="scene-list">Scenes</h2><ol aria-labelledby="scene-list">"#);
    out.push('\n');
    for (i, scene) in scenes.iter().enumerate() {
        let _ = writeln!(
            out,
            r##"<li><a href="#scene-{}">{}</a></li>"##,
            i + 1,
            escape(scene)
        );
    }
    out.push_str("</ol></nav>\n");
}

fn upper_rich(rich: &Rich) -> Rich {
    rich.iter().map(|s| Span::new(s.text.to_uppercase(), s.style)).collect()
}

fn plain(rich: &Rich) -> String {
    crate::inline::plain_text(rich)
}

fn rich_html(rich: &Rich) -> String {
    let mut out = String::new();
    for span in rich {
        let text = escape(&span.text);
        let (mut open, mut close) = (String::new(), String::new());
        if span.style.bold {
            open.push_str("<strong>");
            close.insert_str(0, "</strong>");
        }
        if span.style.italic {
            open.push_str("<em>");
            close.insert_str(0, "</em>");
        }
        if span.style.underline {
            open.push_str(r#"<span class="u">"#);
            close.insert_str(0, "</span>");
        }
        let _ = write!(out, "{open}{text}{close}");
    }
    out
}

/// Action paragraphs keep their internal line breaks.
fn rich_html_multiline(rich: &Rich) -> String {
    rich_html(rich).replace('\n', "<br>\n")
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

const CSS: &str = r#"
:root {
  --ink: #111; --paper: #fff; --muted: #555; --rule: #ccc; --accent: #0b57d0;
  --focus: #0b57d0;
}
@media (prefers-color-scheme: dark) {
  :root { --ink: #e8e8e8; --paper: #17181a; --muted: #a8a8a8; --rule: #444; --accent: #8ab4f8; --focus: #8ab4f8; }
}
@media (prefers-contrast: more) {
  :root { --ink: #000; --paper: #fff; --muted: #000; --rule: #000; }
}
* { box-sizing: border-box; }
body {
  background: var(--paper); color: var(--ink); margin: 0;
  font-family: "Courier Prime", "Courier New", Courier, monospace;
  font-size: 12pt; line-height: 1.2;
}
.skip-link {
  position: absolute; left: -9999px; top: 0; padding: .5rem 1rem;
  background: var(--accent); color: #fff; z-index: 10;
}
.skip-link:focus { left: 0; }
:focus-visible { outline: 3px solid var(--focus); outline-offset: 2px; }
.script {
  max-width: 60ch; margin: 0 auto; padding: 2rem 1rem 4rem;
  white-space: pre-wrap; overflow-wrap: break-word;
}
.slugline { font-size: 1em; font-weight: 700; margin: 2em 0 1em; text-transform: uppercase; }
.action { margin: 0 0 1em; }
.centered { text-align: center; }
.dialogue-block { margin: 0 0 1em; }
.character { margin: 0 0 0 20ch; max-width: 35ch; }
.parenthetical { margin: 0 0 0 15ch; max-width: 25ch; }
.dialogue, .lyric { margin: 0 0 0 10ch; max-width: 35ch; }
.transition { margin: 1em 0; text-align: right; text-transform: uppercase; }
.scene-number { float: right; font-weight: 400; }
.dual { display: flex; gap: 2ch; margin: 0 0 1em; }
.dual .dialogue-block { flex: 1 1 0; min-width: 0; }
.dual .character { margin-left: 0; text-align: center; max-width: none; }
.dual .parenthetical { margin-left: 2ch; max-width: none; }
.dual .dialogue { margin-left: 0; max-width: none; }
.page-break { border: 0; border-top: 1px dashed var(--rule); margin: 2em 0; }
.u { text-decoration: underline; }
.outline-section, .outline-synopsis { color: var(--muted); font-style: italic; margin: 1em 0; }
.title-page {
  max-width: 60ch; margin: 0 auto; padding: 6rem 1rem 3rem; text-align: center;
  border-bottom: 1px solid var(--rule);
}
.title-page h1 { font-size: 1em; font-weight: 700; text-transform: uppercase; text-decoration: underline; }
.credit, .author, .source { margin: .5em 0; }
.title-meta { margin-top: 4rem; text-align: left; color: var(--muted); }
.meta { margin: .2em 0; }
.meta-key { font-weight: 700; }
.scene-nav {
  max-width: 60ch; margin: 0 auto; padding: 1rem;
  border-bottom: 1px solid var(--rule);
}
.scene-nav h2 { font-size: 1em; }
.scene-nav ol { padding-left: 3ch; }
.scene-nav a { color: var(--accent); }
#status { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
@media (prefers-reduced-motion: reduce) { * { scroll-behavior: auto !important; animation: none !important; } }
@media (prefers-reduced-motion: no-preference) { html { scroll-behavior: smooth; } }
@media print {
  @page { size: letter; margin: 1in 1in 1in 1.5in; }
  body { font-size: 12pt; }
  .skip-link, .scene-nav, #status { display: none; }
  .script { max-width: none; padding: 0; margin: 0; }
  .title-page { page-break-after: always; border: 0; }
  .dialogue-block, .slugline { page-break-inside: avoid; }
  .slugline { page-break-after: avoid; }
  .page-break { page-break-after: always; border: 0; }
}
"#;

const STATUS_AND_SCRIPT: &str = r#"<p id="status" role="status" aria-live="polite"></p>
<script>
// Keyboard navigation. Shortcuts are ignored while a text field has focus, and
// every jump is announced through the live region for screen reader users.
(function () {
  var scenes = Array.prototype.slice.call(document.querySelectorAll('.scene'));
  var status = document.getElementById('status');
  var at = -1;
  function say(msg) { if (status) { status.textContent = msg; } }
  function go(i) {
    if (!scenes.length) { return; }
    at = Math.max(0, Math.min(scenes.length - 1, i));
    var scene = scenes[at];
    scene.focus();
    scene.scrollIntoView({ block: 'start' });
    say('Scene ' + (at + 1) + ' of ' + scenes.length + ': ' + scene.querySelector('.slugline').textContent);
  }
  document.addEventListener('keydown', function (e) {
    var el = document.activeElement;
    if (e.metaKey || e.ctrlKey || e.altKey) { return; }
    if (el && (el.isContentEditable || /^(input|textarea|select)$/i.test(el.tagName))) { return; }
    switch (e.key) {
      case 'n': case 'j': go(at + 1); e.preventDefault(); break;
      case 'p': case 'k': go(at - 1); e.preventDefault(); break;
      case 'g': go(0); e.preventDefault(); break;
      case 'G': go(scenes.length - 1); e.preventDefault(); break;
      case '?': say('Keyboard shortcuts: n or j next scene, p or k previous scene, g first scene, shift G last scene.'); e.preventDefault(); break;
    }
  });
})();
</script>
"#;
