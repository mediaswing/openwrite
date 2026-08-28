//! Turning parsed elements into laid-out pages.
//!
//! Everything downstream (plain text, HTML, statistics) works from the pages
//! produced here, so the two renderers can never disagree about where a page
//! break or a `(MORE)` falls.
//!
//! Measurements are the US Letter / 12pt Courier standard: a 6" text column
//! (60 characters) inside a 1.5" left margin, 55 lines to a page.

use crate::element::{Element, Screenplay, Speech, SpeechPart};
use crate::inline::{self, Rich, Span, Style};

/// Indents, measured in characters from the left margin.
const ACTION_INDENT: usize = 0;
const DIALOGUE_INDENT: usize = 10;
const PAREN_INDENT: usize = 15;
const CHARACTER_INDENT: usize = 20;

const DIALOGUE_WIDTH: usize = 35;
const PAREN_WIDTH: usize = 25;
const CHARACTER_WIDTH: usize = 35;

/// Column geometry for side-by-side dialogue.
const DUAL_WIDTH: usize = 27;
const DUAL_RIGHT_INDENT: usize = 33;

/// What a laid-out line is, so renderers can style it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Blank,
    SceneHeading,
    Action,
    Centered,
    Character,
    Parenthetical,
    Dialogue,
    Lyric,
    Transition,
    /// `(MORE)` / `(CONT'D)` continuation marker.
    More,
    /// Pre-composed two-column dialogue.
    Dual,
    /// Title page text.
    Title,
}

/// One physical line on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidLine {
    pub kind: LineKind,
    pub indent: usize,
    pub spans: Rich,
}

impl LaidLine {
    fn new(kind: LineKind, indent: usize, spans: Rich) -> Self {
        LaidLine { kind, indent, spans }
    }

    fn blank() -> Self {
        LaidLine { kind: LineKind::Blank, indent: 0, spans: Vec::new() }
    }

    pub fn is_blank(&self) -> bool {
        self.kind == LineKind::Blank || inline::plain_text(&self.spans).trim().is_empty()
    }

    /// The line as plain text, indent included.
    pub fn to_text(&self) -> String {
        if self.is_blank() {
            return String::new();
        }
        let mut s = " ".repeat(self.indent);
        s.push_str(&inline::plain_text(&self.spans));
        s.trim_end().to_string()
    }
}

/// A finished page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// `None` for the title page, which is not numbered.
    pub number: Option<usize>,
    pub lines: Vec<LaidLine>,
    pub is_title_page: bool,
}

/// Formatting knobs.
#[derive(Debug, Clone)]
pub struct Options {
    /// Characters per line inside the margins.
    pub width: usize,
    /// Body lines per page.
    pub lines_per_page: usize,
    /// Render the title page.
    pub title_page: bool,
    /// Print `#1#` scene numbers in the right margin.
    pub scene_numbers: bool,
    /// Append `(CONT'D)` when a character speaks twice in a row.
    pub contd: bool,
    /// Two blank lines before a scene heading rather than one.
    pub double_space_scenes: bool,
    /// Number the pages.
    pub page_numbers: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 60,
            lines_per_page: 55,
            title_page: true,
            scene_numbers: false,
            contd: true,
            double_space_scenes: true,
            page_numbers: true,
        }
    }
}

/// How a run of lines may be broken across a page boundary.
struct Block {
    lines: Vec<LaidLine>,
    /// Blank lines to leave above this block, when it is not first on a page.
    space_before: usize,
    /// Minimum lines that must fit for the block to start on this page.
    min_lines: usize,
    /// A speech that can be split with `(MORE)` / `(CONT'D)`.
    speech: Option<Rich>,
    /// How many lines the character cue itself occupies.
    cue_height: usize,
    /// How many lines of *content* must follow this block on the same page.
    /// A scene heading asks for two, so it is never the last thing a reader
    /// sees before turning over.
    keep_with_next: usize,
    /// Forces the next block onto a fresh page.
    page_break: bool,
}

impl Block {
    fn simple(lines: Vec<LaidLine>, space_before: usize) -> Self {
        let min_lines = lines.len();
        Block {
            lines,
            space_before,
            min_lines,
            speech: None,
            cue_height: 1,
            keep_with_next: 0,
            page_break: false,
        }
    }
}

/// Turn each block's `keep_with_next` into a concrete line count.
///
/// The count has to be resolved against the blocks that actually follow,
/// because the blank line between two paragraphs is a line of the page too:
/// reserving "two lines" for what comes after a scene heading reserves the
/// wrong thing if one of them turns out to be a separator.
fn resolve_keeps(blocks: &mut [Block]) {
    for i in 0..blocks.len() {
        let wanted = blocks[i].keep_with_next;
        if wanted == 0 {
            continue;
        }
        let mut needed = blocks[i].lines.len();
        let mut found = 0;
        'ahead: for next in &blocks[i + 1..] {
            needed += next.space_before;
            for line in &next.lines {
                needed += 1;
                if !line.is_blank() {
                    found += 1;
                    if found >= wanted {
                        break 'ahead;
                    }
                }
            }
        }
        blocks[i].min_lines = blocks[i].min_lines.max(needed);
    }
}

/// Lay a screenplay out into pages.
pub fn paginate(doc: &Screenplay, opts: &Options) -> Vec<Page> {
    let mut pages = Vec::new();
    if opts.title_page && doc.has_title_page() {
        pages.push(title_page(doc, opts));
    }

    let mut blocks = build_blocks(doc, opts);
    resolve_keeps(&mut blocks);
    let mut body = flow(blocks, opts);

    let mut n = 1;
    for page in body.iter_mut() {
        page.number = if opts.page_numbers { Some(n) } else { None };
        n += 1;
    }
    pages.append(&mut body);
    pages
}

// ---------------------------------------------------------------------------
// Block construction
// ---------------------------------------------------------------------------

fn build_blocks(doc: &Screenplay, opts: &Options) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    // Tracks the last speaker so `(CONT'D)` can be added to a repeat cue.
    let mut last_speaker: Option<String> = None;

    for element in &doc.elements {
        match element {
            Element::Section { .. } | Element::Synopsis(_) => continue,

            Element::PageBreak => {
                last_speaker = None;
                if let Some(prev) = blocks.last_mut() {
                    prev.page_break = true;
                } else {
                    blocks.push(Block {
                        lines: Vec::new(),
                        space_before: 0,
                        min_lines: 0,
                        speech: None,
                        cue_height: 1,
                        keep_with_next: 0,
                        page_break: true,
                    });
                }
            }

            Element::SceneHeading { text, scene_number } => {
                last_speaker = None;
                let mut spans = upper(text);
                if opts.scene_numbers {
                    if let Some(num) = scene_number {
                        spans = number_slug(&spans, num, opts.width);
                    }
                }
                let lines: Vec<LaidLine> = wrap(&spans, opts.width)
                    .into_iter()
                    .map(|l| LaidLine::new(LineKind::SceneHeading, ACTION_INDENT, l))
                    .collect();
                let space = if opts.double_space_scenes { 2 } else { 1 };
                let mut block = Block::simple(lines, space);
                // A slug alone at the foot of a page is an orphan: it must be
                // followed by enough of its scene to be worth reading.
                block.keep_with_next = 2;
                blocks.push(block);
            }

            Element::Action { text, centered } => {
                last_speaker = None;
                let lines: Vec<LaidLine> = if *centered {
                    wrap(text, opts.width)
                        .into_iter()
                        .map(|l| {
                            let pad = (opts.width - inline::width(&l).min(opts.width)) / 2;
                            LaidLine::new(LineKind::Centered, pad, l)
                        })
                        .collect()
                } else {
                    wrap(text, opts.width)
                        .into_iter()
                        .map(|l| LaidLine::new(LineKind::Action, ACTION_INDENT, l))
                        .collect()
                };
                let mut block = Block::simple(lines, 1);
                // Action may split, but never leaving a single stranded line.
                block.min_lines = block.lines.len().min(2);
                blocks.push(block);
            }

            Element::Transition(text) => {
                last_speaker = None;
                let spans = upper(text);
                let lines: Vec<LaidLine> = wrap(&spans, opts.width)
                    .into_iter()
                    .map(|l| {
                        let indent = opts.width - inline::width(&l).min(opts.width);
                        LaidLine::new(LineKind::Transition, indent, l)
                    })
                    .collect();
                blocks.push(Block::simple(lines, 1));
            }

            Element::Dialogue(speech) => {
                let cue = cue_spans(speech, opts, &mut last_speaker);
                let cue_lines: Vec<LaidLine> = wrap(&cue, CHARACTER_WIDTH)
                    .into_iter()
                    .map(|l| LaidLine::new(LineKind::Character, CHARACTER_INDENT, l))
                    .collect();
                let cue_height = cue_lines.len().max(1);
                let mut lines = cue_lines;
                lines.extend(speech_lines(speech));
                let mut block = Block::simple(lines, 1);
                // Never leave a cue alone: cue plus at least one spoken line.
                block.min_lines = block.lines.len().min(cue_height + 1);
                block.cue_height = cue_height;
                block.speech = Some(cue);
                blocks.push(block);
            }

            Element::DualDialogue(left, right) => {
                last_speaker = None;
                blocks.push(Block::simple(dual_lines(left, right), 1));
            }
        }
    }
    blocks
}

/// The character cue, uppercased, with `(CONT'D)` added where appropriate.
fn cue_spans(speech: &Speech, opts: &Options, last_speaker: &mut Option<String>) -> Rich {
    let mut spans = upper(&speech.character);
    let name = speech.character_name();
    let plain = inline::plain_text(&spans).to_uppercase();
    let repeated = last_speaker.as_deref() == Some(name.as_str());
    if opts.contd && repeated && !plain.contains("CONT'D") && !plain.contains("CONTD") {
        spans.push(Span::plain(" (CONT'D)"));
    }
    *last_speaker = Some(name);
    spans
}

/// Wrap the parts of a speech at dialogue width.
fn speech_lines(speech: &Speech) -> Vec<LaidLine> {
    let mut lines = Vec::new();
    for part in &speech.parts {
        match part {
            SpeechPart::Parenthetical(text) => {
                lines.extend(
                    wrap(text, PAREN_WIDTH)
                        .into_iter()
                        .map(|l| LaidLine::new(LineKind::Parenthetical, PAREN_INDENT, l)),
                );
            }
            SpeechPart::Line(text) => {
                if inline::plain_text(text).trim().is_empty() {
                    lines.push(LaidLine::blank());
                    continue;
                }
                lines.extend(
                    wrap(text, DIALOGUE_WIDTH)
                        .into_iter()
                        .map(|l| LaidLine::new(LineKind::Dialogue, DIALOGUE_INDENT, l)),
                );
            }
            SpeechPart::Lyric(text) => {
                let italic = restyle(text, |s| Style { italic: true, ..s });
                lines.extend(
                    wrap(&italic, DIALOGUE_WIDTH)
                        .into_iter()
                        .map(|l| LaidLine::new(LineKind::Lyric, DIALOGUE_INDENT, l)),
                );
            }
        }
    }
    lines
}

/// Compose two speeches into side-by-side columns.
fn dual_lines(left: &Speech, right: &Speech) -> Vec<LaidLine> {
    let column = |speech: &Speech| -> Vec<(usize, Rich)> {
        let mut out = Vec::new();
        let cue = upper(&speech.character);
        for l in wrap(&cue, DUAL_WIDTH) {
            let pad = (DUAL_WIDTH - inline::width(&l).min(DUAL_WIDTH)) / 2;
            out.push((pad, l));
        }
        for part in &speech.parts {
            let (indent, width) = match part {
                SpeechPart::Parenthetical(_) => (4, DUAL_WIDTH - 4),
                _ => (0, DUAL_WIDTH),
            };
            for l in wrap(part.text(), width) {
                out.push((indent, l));
            }
        }
        out
    };

    let (l, r) = (column(left), column(right));
    let height = l.len().max(r.len());
    let mut lines = Vec::with_capacity(height);
    for i in 0..height {
        let mut spans: Rich = Vec::new();
        let mut col = 0;
        if let Some((indent, text)) = l.get(i) {
            spans.push(Span::plain(" ".repeat(*indent)));
            spans.extend(text.iter().cloned());
            col = indent + inline::width(text);
        }
        if let Some((indent, text)) = r.get(i) {
            let target = DUAL_RIGHT_INDENT + indent;
            spans.push(Span::plain(" ".repeat(target.saturating_sub(col))));
            spans.extend(text.iter().cloned());
        }
        lines.push(LaidLine::new(LineKind::Dual, 0, spans));
    }
    lines
}

/// Push a scene number out to both margins of the slug line.
fn number_slug(spans: &Rich, number: &str, width: usize) -> Rich {
    let label = number.trim();
    let mut out = vec![Span::plain(format!("{label} "))];
    out.extend(spans.iter().cloned());
    let used = inline::width(&out) + label.len() + 1;
    if used < width {
        out.push(Span::plain(" ".repeat(width - used)));
    } else {
        out.push(Span::plain(" "));
    }
    out.push(Span::plain(label.to_string()));
    out
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

fn flow(blocks: Vec<Block>, opts: &Options) -> Vec<Page> {
    let mut pages: Vec<Page> = Vec::new();
    let mut current: Vec<LaidLine> = Vec::new();
    let cap = opts.lines_per_page.max(4);

    macro_rules! finish_page {
        () => {
            while current.last().map_or(false, |l| l.is_blank()) {
                current.pop();
            }
            if !current.is_empty() {
                pages.push(Page {
                    number: None,
                    lines: std::mem::take(&mut current),
                    is_title_page: false,
                });
            }
            current.clear();
        };
    }

    /// Place lines on the page, spilling to the next when it fills. A blank
    /// line never opens a page.
    macro_rules! place {
        ($lines:expr) => {
            for line in $lines {
                if current.len() >= cap {
                    finish_page!();
                }
                if current.is_empty() && line.is_blank() {
                    continue;
                }
                current.push(line);
            }
        };
    }

    for block in blocks {
        if !block.lines.is_empty() {
            let gap = if current.is_empty() { 0 } else { block.space_before };
            let mut placed = false;

            // A speech too tall for what is left is broken rather than moved:
            // moving it whole would leave a hole at the foot of the page.
            let cue = block.speech.clone();
            if let Some(cue) = cue.filter(|_| current.len() + gap + block.lines.len() > cap) {
                let room = cap.saturating_sub(current.len() + gap);
                let mut split = split_speech(&block, &cue, room);
                if split.is_none() && !current.is_empty() {
                    // No legal break point in the room left. Start a fresh page
                    // and look again, with a whole page to work with.
                    finish_page!();
                    split = split_speech(&block, &cue, cap);
                }
                if let Some((head, tail)) = split {
                    let gap = if current.is_empty() { 0 } else { block.space_before };
                    for _ in 0..gap {
                        current.push(LaidLine::blank());
                    }
                    current.extend(head);
                    finish_page!();
                    place!(tail);
                    placed = true;
                }
            }

            if !placed {
                // Whatever has to stay together must fit here. `min_lines` can
                // exceed the block's own height: a scene heading asks for room
                // for the lines that follow it, so that it is never the last
                // thing on a page.
                let gap = if current.is_empty() { 0 } else { block.space_before };
                if current.len() + gap + block.min_lines > cap {
                    finish_page!();
                }
                let gap = if current.is_empty() { 0 } else { block.space_before };
                for _ in 0..gap {
                    current.push(LaidLine::blank());
                }
                place!(block.lines);
            }
        }

        if block.page_break {
            finish_page!();
        }
    }
    finish_page!();
    pages
}

/// Split a speech across a page boundary, returning the lines that stay on this
/// page (ending in `(MORE)`) and those that start the next (led by the cue with
/// `(CONT'D)`). Returns `None` when no legal break point exists.
fn split_speech(block: &Block, cue: &Rich, room: usize) -> Option<(Vec<LaidLine>, Vec<LaidLine>)> {
    // Leave space for the `(MORE)` marker itself.
    let usable = room.checked_sub(1)?;
    // Cue plus at least one spoken line has to fit above the break.
    let least = block.cue_height + 1;
    if usable < least {
        return None;
    }
    // And at least one line has to survive to the next page.
    let mut split = usable.min(block.lines.len().saturating_sub(1));
    // Never break just after a parenthetical, and never strand one.
    while split > least
        && (block.lines[split - 1].kind == LineKind::Parenthetical
            || block.lines[split].kind == LineKind::Parenthetical)
    {
        split -= 1;
    }
    if split < least || split >= block.lines.len() {
        return None;
    }

    let mut head = block.lines[..split].to_vec();
    head.push(LaidLine::new(LineKind::More, CHARACTER_INDENT, vec![Span::plain("(MORE)")]));

    let mut contd = cue.clone();
    if !inline::plain_text(cue).to_uppercase().contains("CONT'D") {
        contd.push(Span::plain(" (CONT'D)"));
    }
    let mut tail: Vec<LaidLine> = wrap(&contd, CHARACTER_WIDTH)
        .into_iter()
        .map(|l| LaidLine::new(LineKind::Character, CHARACTER_INDENT, l))
        .collect();
    tail.extend(block.lines[split..].iter().cloned());
    Some((head, tail))
}

// ---------------------------------------------------------------------------
// Title page
// ---------------------------------------------------------------------------

fn title_page(doc: &Screenplay, opts: &Options) -> Page {
    let mut lines = vec![LaidLine::blank(); opts.lines_per_page];
    let centered = |text: &str, kind: LineKind| -> Vec<LaidLine> {
        wrap(&inline::parse(text), opts.width)
            .into_iter()
            .map(|l| {
                let pad = (opts.width - inline::width(&l).min(opts.width)) / 2;
                LaidLine::new(kind, pad, l)
            })
            .collect()
    };

    // Title block, sitting a little above the vertical centre.
    let mut block: Vec<LaidLine> = Vec::new();
    if let Some(title) = doc.meta("Title") {
        for part in title {
            block.extend(centered(part, LineKind::Title));
        }
    }
    let credit = doc.meta_line(&["Credit"]).unwrap_or_else(|| "written by".to_string());
    if let Some(authors) = doc.meta("Author").or_else(|| doc.meta("Authors")) {
        block.push(LaidLine::blank());
        block.extend(centered(&credit, LineKind::Title));
        block.push(LaidLine::blank());
        for part in authors {
            block.extend(centered(part, LineKind::Title));
        }
    }
    if let Some(source) = doc.meta("Source") {
        block.push(LaidLine::blank());
        for part in source {
            block.extend(centered(part, LineKind::Title));
        }
    }

    let top = ((opts.lines_per_page.saturating_sub(block.len())) / 2).saturating_sub(4);
    for (i, line) in block.into_iter().enumerate() {
        if top + i < lines.len() {
            lines[top + i] = line;
        }
    }

    // Contact bottom left, date and copyright bottom right.
    let mut left: Vec<LaidLine> = Vec::new();
    for key in ["Contact", "Contact Info"] {
        if let Some(v) = doc.meta(key) {
            for part in v {
                left.extend(
                    wrap(&inline::parse(part), opts.width / 2)
                        .into_iter()
                        .map(|l| LaidLine::new(LineKind::Title, 0, l)),
                );
            }
            break;
        }
    }
    let mut right: Vec<LaidLine> = Vec::new();
    for key in ["Draft date", "Date", "Notes", "Copyright"] {
        if let Some(v) = doc.meta(key) {
            for part in v {
                for l in wrap(&inline::parse(part), opts.width / 2) {
                    let indent = opts.width - inline::width(&l).min(opts.width);
                    right.push(LaidLine::new(LineKind::Title, indent, l));
                }
            }
        }
    }

    let bottom = opts.lines_per_page.saturating_sub(1);
    for (i, line) in left.iter().rev().enumerate() {
        if let Some(slot) = bottom.checked_sub(i) {
            lines[slot] = line.clone();
        }
    }
    let right_start = bottom.saturating_sub(left.len().max(right.len()).saturating_sub(1));
    for (i, line) in right.into_iter().enumerate() {
        let slot = right_start + i;
        if slot < lines.len() && lines[slot].is_blank() {
            lines[slot] = line;
        } else if slot < lines.len() {
            // Merge onto the same row as the contact line.
            let mut merged = lines[slot].clone();
            let used = merged.indent + inline::width(&merged.spans);
            let pad = line.indent.saturating_sub(used);
            merged.spans.push(Span::plain(" ".repeat(pad)));
            merged.spans.extend(line.spans);
            lines[slot] = merged;
        }
    }

    Page { number: None, lines, is_title_page: true }
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Greedy word wrap over styled text. Hard newlines are honoured.
pub fn wrap(rich: &[Span], width: usize) -> Vec<Rich> {
    let width = width.max(1);
    let mut out = Vec::new();
    for para in split_hard_lines(rich) {
        let chars = to_chars(&para);
        if chars.is_empty() {
            out.push(Vec::new());
            continue;
        }
        let mut start = 0;
        while start < chars.len() {
            if chars.len() - start <= width {
                out.push(from_chars(&chars[start..]));
                break;
            }
            // Break at the last space that fits; otherwise break a long word.
            let window = &chars[start..start + width + 1];
            let brk = window.iter().rposition(|(c, _)| *c == ' ');
            let (end, next) = match brk {
                Some(i) if i > 0 => (start + i, start + i + 1),
                _ => (start + width, start + width),
            };
            out.push(from_chars(&chars[start..end]));
            start = next;
            while chars.get(start).map_or(false, |(c, _)| *c == ' ') {
                start += 1;
            }
        }
    }
    out
}

fn split_hard_lines(rich: &[Span]) -> Vec<Rich> {
    let mut out: Vec<Rich> = vec![Vec::new()];
    for span in rich {
        let mut parts = span.text.split('\n');
        if let Some(first) = parts.next() {
            if !first.is_empty() {
                out.last_mut().unwrap().push(Span::new(first, span.style));
            }
        }
        for part in parts {
            out.push(Vec::new());
            if !part.is_empty() {
                out.last_mut().unwrap().push(Span::new(part, span.style));
            }
        }
    }
    out
}

fn to_chars(rich: &[Span]) -> Vec<(char, Style)> {
    let mut out = Vec::new();
    for span in rich {
        for c in span.text.chars() {
            out.push((c, span.style));
        }
    }
    // Trailing spaces never survive a wrap.
    while out.last().map_or(false, |(c, _)| *c == ' ') {
        out.pop();
    }
    out
}

fn from_chars(chars: &[(char, Style)]) -> Rich {
    let mut out: Rich = Vec::new();
    for (c, style) in chars {
        match out.last_mut() {
            Some(span) if span.style == *style => span.text.push(*c),
            _ => out.push(Span::new(c.to_string(), *style)),
        }
    }
    out
}

/// Uppercase every span, preserving styling.
fn upper(rich: &[Span]) -> Rich {
    rich.iter().map(|s| Span::new(s.text.to_uppercase(), s.style)).collect()
}

fn restyle(rich: &[Span], f: impl Fn(Style) -> Style) -> Rich {
    rich.iter().map(|s| Span::new(s.text.clone(), f(s.style))).collect()
}

/// Scene headings paired with the page they start on, for outlines and
/// navigation. Wrapped headings count once.
pub fn scene_pages(pages: &[Page]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for page in pages {
        if page.is_title_page {
            continue;
        }
        let number = page.number.unwrap_or(0);
        let mut previous_was_heading = false;
        for line in &page.lines {
            let heading = line.kind == LineKind::SceneHeading;
            if heading && !previous_was_heading {
                out.push((line.to_text().trim().to_string(), number));
            }
            previous_was_heading = heading;
        }
    }
    out
}
