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
use crate::t;
use std::fmt::Write;

pub fn render(doc: &Screenplay, opts: &Options) -> String {
    let title = doc
        .meta_line(&["Title"])
        .map(|title| meta_plain(&title))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| t!("export.untitled"));
    let mut out = String::new();

    let _ = write!(
        out,
        r##"<!DOCTYPE html>
<html lang="{}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>{}</style>
</head>
<body>
<a class="skip-link" href="#screenplay">{}</a>
"##,
        escape(&language(doc)),
        escape(&title),
        CSS,
        escape(&t!("export.skip_link"))
    );

    // Exactly one `h1`, and it is the screenplay's name. The title page carries
    // it where there is one; where there is not — a working draft that has not
    // been given a `Title:` yet — it is still there to be found, just not to be
    // looked at. Without it the contents list and every scene heading are `h2`
    // with nothing above them, and heading navigation starts halfway down.
    if opts.title_page && doc.has_title_page() {
        title_page(&mut out, doc);
    } else {
        let _ = writeln!(
            out,
            r#"<h1 class="visually-hidden">{}</h1>"#,
            escape(&title)
        );
    }

    scene_nav(&mut out, doc);

    let _ = writeln!(
        out,
        r#"<main id="screenplay" class="script" tabindex="-1" aria-label="{}">"#,
        escape(&t!("export.main_label", title = title))
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
                    .map(|n| {
                        format!(
                            r#" <span class="scene-number" aria-label="{}">{}</span>"#,
                            escape(&t!("export.scene_number", number = n)),
                            escape(n)
                        )
                    })
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
                let label = t!("export.dialogue_label", name = speech.character_name());
                out.push_str(&speech_html(speech, &label));
            }
            Element::DualDialogue(left, right) => {
                let _ = writeln!(
                    out,
                    r#"<div class="dual" role="group" aria-label="{}">"#,
                    escape(&t!("export.dual_label"))
                );
                out.push_str(&speech_html(
                    left,
                    &t!("export.dual_first", name = left.character_name()),
                ));
                out.push_str(&speech_html(
                    right,
                    &t!("export.dual_second", name = right.character_name()),
                ));
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
    out.push_str(&status_and_script());
    out.push_str("</body>\n</html>\n");
    out
}

/// One speech. `label` is its accessible name, already in the writer's
/// language: who is speaking, and in a dual block which side they are.
fn speech_html(speech: &Speech, label: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<div class="dialogue-block" role="group" aria-label="{}">"#,
        escape(label)
    );
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
    let _ = write!(
        out,
        r#"<div class="title-page" role="region" aria-label="{}">"#,
        escape(&t!("export.title_page_label"))
    );
    out.push('\n');
    if let Some(title) = doc.meta("Title") {
        let _ = writeln!(out, "<h1>{}</h1>", meta_html(&title.join(" ")));
    }
    if let Some(authors) = doc.meta("Author").or_else(|| doc.meta("Authors")) {
        let credit = doc.meta_line(&["Credit"]).unwrap_or_else(|| "written by".into());
        let _ = writeln!(out, r#"<p class="credit">{}</p>"#, meta_html(&credit));
        let _ = writeln!(out, r#"<p class="author">{}</p>"#, meta_html(&authors.join(", ")));
    }
    if let Some(source) = doc.meta("Source") {
        let _ = writeln!(out, r#"<p class="source">{}</p>"#, meta_html(&source.join(" ")));
    }
    let mut footer = Vec::new();
    for key in ["Draft date", "Date", "Contact", "Copyright"] {
        if let Some(v) = doc.meta(key) {
            footer.push(format!(
                r#"<p class="meta"><span class="meta-key">{}:</span> {}</p>"#,
                escape(key),
                meta_html(&v.join(", "))
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
    let heading = t!("export.scenes");
    let _ = write!(
        out,
        r#"<nav class="scene-nav" aria-label="{0}"><h2 id="scene-list">{0}</h2><p class="nav-hint">{1}</p><ol aria-labelledby="scene-list">"#,
        escape(&heading),
        escape(&t!("export.nav_hint"))
    );
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

/// A title page value as markup: `_**THE LAST BUS**_` is emphasis, not four
/// underscores and four asterisks.
///
/// The printed page has always read these with [`crate::inline::parse`] — see
/// `layout::title_page`, which does it to every one of them — and this did not,
/// so the same screenplay came out of the two renderers differently and the
/// markers went into the web page as themselves.
fn meta_html(text: &str) -> String {
    rich_html(&crate::inline::parse(text))
}

/// The same value with the markup taken off and nothing put in its place, for
/// the places that hold text rather than markup: `<title>`, an `aria-label`.
fn meta_plain(text: &str) -> String {
    crate::inline::plain_text(&crate::inline::parse(text))
}

/// Action paragraphs keep their internal line breaks.
fn rich_html_multiline(rich: &Rich) -> String {
    rich_html(rich).replace('\n', "<br>\n")
}

/// The language tag the exported page declares itself to be in.
///
/// A `Language:` line on the title page wins, for the writer whose editor is in
/// one language and whose screenplay is in another; otherwise it is the
/// language the editor is being used in. Either way the page says what it is,
/// so a screen reader reads a French script in a French voice rather than
/// sounding French words out with English rules.
fn language(doc: &Screenplay) -> String {
    doc.meta_line(&["Language"])
        .map(|tag| tag.trim().to_string())
        .filter(|tag| is_language_tag(tag))
        .unwrap_or_else(crate::i18n::current_code)
}

/// Is this shaped like a BCP 47 tag — `fr`, `pt-BR`, `zh-Hant-TW`?
///
/// It goes into `lang=`, and a title page is a place anybody can type anything.
/// Anything that is not a tag is dropped in favour of the editor's own
/// language, which is a better guess than a `lang` nothing can act on.
fn is_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 35
        && tag.split('-').all(|part| {
            !part.is_empty() && part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric())
        })
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
.visually-hidden {
  position: absolute; width: 1px; height: 1px; overflow: hidden;
  clip: rect(0 0 0 0); clip-path: inset(50%); white-space: nowrap;
}
.nav-hint { color: var(--muted); font-size: .9em; margin: .5em 0 0; }
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
  .visually-hidden { display: none; }
  .script { max-width: none; padding: 0; margin: 0; }
  .title-page { page-break-after: always; border: 0; }
  .dialogue-block, .slugline { page-break-inside: avoid; }
  .slugline { page-break-after: avoid; }
  .page-break { page-break-after: always; border: 0; }
}
"#;

/// The live region and the keyboard navigation, in the writer's language.
///
/// The two spoken strings are put through [`crate::json::escape`] rather than
/// [`escape`]: they are going inside a JavaScript string literal, not inside
/// markup, and an apostrophe in "Scène 3 sur 9" would end the literal.
fn status_and_script() -> String {
    SCRIPT
        .replace("__SCENE_OF__", &crate::json::escape(&t!("export.js_scene")))
        .replace("__HELP__", &crate::json::escape(&t!("export.js_help")))
}

const SCRIPT: &str = r#"<p id="status" role="status" aria-live="polite"></p>
<script>
// Keyboard navigation, announced through the live region so that a screen
// reader user hears where each jump landed.
//
// The letters are live only while the screenplay itself has focus — reach it
// with the skip link, by clicking into it, or from the contents list. Bound to
// the document instead, as they were, a single unmodified letter fires while
// somebody is dictating into speech recognition software, which is what WCAG
// 2.1.4 Character Key Shortcuts is about: a shortcut has to be switchable off,
// remappable, or live only when a component has focus. This is the third.
(function () {
  var SCENE_OF = "__SCENE_OF__";
  var HELP = "__HELP__";
  var main = document.getElementById('screenplay');
  var scenes = Array.prototype.slice.call(document.querySelectorAll('.scene'));
  var status = document.getElementById('status');
  var at = -1;
  function say(msg) { if (status) { status.textContent = msg; } }
  function listening() {
    var el = document.activeElement;
    if (!main || !el) { return false; }
    if (el.isContentEditable || /^(input|textarea|select)$/i.test(el.tagName)) { return false; }
    return el === main || main.contains(el);
  }
  function go(i) {
    if (!scenes.length) { return; }
    at = Math.max(0, Math.min(scenes.length - 1, i));
    var scene = scenes[at];
    var slug = scene.querySelector('.slugline');
    scene.focus();
    scene.scrollIntoView({ block: 'start' });
    say(SCENE_OF
      .replace('{index}', at + 1)
      .replace('{total}', scenes.length)
      .replace('{heading}', slug ? slug.textContent : ''));
  }
  // Clicking into the script is one of the ways of reaching it, but the
  // paragraphs are not focusable, so the click has to be passed on to the
  // landmark itself. `preventScroll` keeps the page where the reader put it.
  if (main) {
    main.addEventListener('click', function () {
      if (!listening()) { main.focus({ preventScroll: true }); }
    });
  }
  document.addEventListener('keydown', function (e) {
    if (e.metaKey || e.ctrlKey || e.altKey) { return; }
    if (!listening()) { return; }
    switch (e.key) {
      case 'n': case 'j': go(at + 1); e.preventDefault(); break;
      case 'p': case 'k': go(at - 1); e.preventDefault(); break;
      case 'g': go(0); e.preventDefault(); break;
      case 'G': go(scenes.length - 1); e.preventDefault(); break;
      case '?': say(HELP); e.preventDefault(); break;
    }
  });
})();
</script>
"#;
