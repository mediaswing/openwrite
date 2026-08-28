//! Inline emphasis: `*italic*`, `**bold**`, `***bold italic***`, `_underline_`.
//!
//! Text is parsed into styled [`Span`]s up front so that line wrapping can split
//! a run of emphasis across lines without losing (or leaking) the styling.

/// Character styling carried by a [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    pub fn is_plain(self) -> bool {
        self == Style::default()
    }
}

/// A run of text sharing one [`Style`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Span { text: text.into(), style }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Span { text: text.into(), style: Style::default() }
    }
}

/// A styled line or paragraph: a sequence of spans.
pub type Rich = Vec<Span>;

/// Total character count of a rich string.
pub fn width(rich: &[Span]) -> usize {
    rich.iter().map(|s| s.text.chars().count()).sum()
}

/// Flatten to unstyled text.
pub fn plain_text(rich: &[Span]) -> String {
    rich.iter().map(|s| s.text.as_str()).collect()
}

/// Parse emphasis markup into styled spans.
///
/// A marker only opens emphasis if it is followed by a non-space, and only
/// closes it if preceded by a non-space, which keeps arithmetic and stray
/// asterisks (`2 * 3`, `a_b`) from being swallowed. Unmatched markers are
/// emitted literally. `\*` escapes a marker.
pub fn parse(text: &str) -> Rich {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Rich = Vec::new();
    let mut buf = String::new();
    let mut style = Style::default();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !buf.is_empty() {
                out.push(Span::new(std::mem::take(&mut buf), style));
            }
        };
    }

    while i < chars.len() {
        let c = chars[i];

        if c == '\\' && i + 1 < chars.len() && matches!(chars[i + 1], '*' | '_') {
            buf.push(chars[i + 1]);
            i += 2;
            continue;
        }

        if c == '*' || c == '_' {
            let run = chars[i..].iter().take_while(|&&x| x == c).count();
            // `_` is only ever a single-character marker.
            let run = if c == '_' { 1 } else { run.min(3) };

            let (bold, italic, underline) = match (c, run) {
                ('_', _) => (false, false, true),
                ('*', 1) => (false, true, false),
                ('*', 2) => (true, false, false),
                ('*', _) => (true, true, false),
                _ => unreachable!(),
            };
            let active = (bold && style.bold) || (italic && style.italic) || (underline && style.underline);

            let next = chars.get(i + run).copied();
            let prev = if i == 0 { None } else { Some(chars[i - 1]) };
            let can_open = next.map_or(false, |n| !n.is_whitespace())
                && has_closer(&chars, i + run, c, run);
            let can_close = active && prev.map_or(false, |p| !p.is_whitespace());

            if can_close {
                flush!();
                style.bold ^= bold;
                style.italic ^= italic;
                style.underline ^= underline;
                i += run;
                continue;
            }
            if can_open {
                flush!();
                style.bold |= bold;
                style.italic |= italic;
                style.underline |= underline;
                i += run;
                continue;
            }
        }

        buf.push(c);
        i += 1;
    }

    flush!();
    out
}

/// Is there a matching closing marker later in the line?
fn has_closer(chars: &[char], from: usize, marker: char, run: usize) -> bool {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == marker && !chars[i - 1].is_whitespace() {
            let here = chars[i..].iter().take_while(|&&x| x == marker).count();
            if here >= run {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Strip `[[notes]]`, which never appear in a formatted script.
pub fn strip_notes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        if let Some(end) = rest[start + 2..].find("]]") {
            out.push_str(&rest[..start]);
            rest = &rest[start + 2 + end + 2..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}
