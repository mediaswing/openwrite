//! The audio drama file: `<story>`, its `<voices>`, and its `<dialog>`.
//!
//! Two spellings of the same format are read. The one in use now numbers its
//! people and says where they stand:
//!
//! ```xml
//! <story model="1.0">
//!     <voices>
//!         <character id="1" name="ben" gender="male">VOICE_ID</character>
//!     </voices>
//!     <dialog>
//!         <character line="1" id="1" age="12" state="normal" pos="left">I don't remember how it happened</character>
//!     </dialog>
//! </story>
//! ```
//!
//! The other refers to people by name and leaves out `pos`. Both are read the
//! same way and either can be written back out, because which one a file uses
//! is the file's business rather than this program's.
//!
//! # It is forgiving, and it says so
//!
//! A story is typed by hand, so this reader recovers from the mistakes hand
//! typing makes — a voice put inside the tag rather than between the tags, a
//! state nobody has heard of, a `<voices>` section that was never written —
//! and records every recovery in [`Story::problems`] instead of silently
//! deciding what somebody meant. The window lists them. Nothing here fails: a
//! file that is half wrong still gets you the half that is right.

use std::fmt;

/// The model this program writes when a file does not say.
pub const DEFAULT_MODEL: &str = "1.0";

/// The one this program knows how to make sound. `1.2` is the sound effects
/// version and is read, but its effects are not made yet.
pub const KNOWN_MODELS: [&str; 2] = ["1.0", "1.2"];

/// How a line is spoken.
///
/// `normal` is the absence of an instruction rather than an instruction, which
/// is why it is the fallback for a word this program does not know: an unknown
/// state should get you the line plainly rather than get you nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Normal,
    Whisper,
    Scared,
    Shout,
    Angry,
    Sad,
    Excited,
    Tired,
}

impl State {
    /// Every state, in the order the window offers them.
    pub const ALL: [State; 8] = [
        State::Normal,
        State::Whisper,
        State::Scared,
        State::Shout,
        State::Angry,
        State::Sad,
        State::Excited,
        State::Tired,
    ];

    /// Read a `state=""`. Several words mean the same thing because several
    /// words are what people write.
    pub fn parse(text: &str) -> Option<State> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "normal" | "neutral" | "plain" | "calm" => Some(State::Normal),
            "whisper" | "whispered" | "whispering" | "quiet" | "hushed" => Some(State::Whisper),
            "scared" | "afraid" | "frightened" | "terrified" | "fearful" => Some(State::Scared),
            "shout" | "shouting" | "shouted" | "yell" | "yelling" | "loud" => Some(State::Shout),
            "angry" | "anger" | "furious" | "cross" => Some(State::Angry),
            "sad" | "sorrowful" | "grieving" | "upset" | "crying" => Some(State::Sad),
            "excited" | "excitable" | "eager" | "elated" | "happy" => Some(State::Excited),
            "tired" | "weary" | "exhausted" | "sleepy" => Some(State::Tired),
            _ => None,
        }
    }

    /// The word written back into the file.
    pub fn word(self) -> &'static str {
        match self {
            State::Normal => "normal",
            State::Whisper => "whisper",
            State::Scared => "scared",
            State::Shout => "shout",
            State::Angry => "angry",
            State::Sad => "sad",
            State::Excited => "excited",
            State::Tired => "tired",
        }
    }

    /// The language key the window shows it under.
    pub fn key(self) -> &'static str {
        match self {
            State::Normal => "drama.state.normal",
            State::Whisper => "drama.state.whisper",
            State::Scared => "drama.state.scared",
            State::Shout => "drama.state.shout",
            State::Angry => "drama.state.angry",
            State::Sad => "drama.state.sad",
            State::Excited => "drama.state.excited",
            State::Tired => "drama.state.tired",
        }
    }
}

/// Where in the stereo picture the voice stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pos {
    Left,
    Centre,
    Right,
}

impl Pos {
    pub const ALL: [Pos; 3] = [Pos::Left, Pos::Centre, Pos::Right];

    /// Both spellings of the middle, because the format does not choose one.
    pub fn parse(text: &str) -> Option<Pos> {
        match text.trim().to_ascii_lowercase().as_str() {
            "left" | "l" => Some(Pos::Left),
            "" | "centre" | "center" | "middle" | "c" | "mid" => Some(Pos::Centre),
            "right" | "r" => Some(Pos::Right),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Pos::Left => "left",
            Pos::Centre => "centre",
            Pos::Right => "right",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Pos::Left => "drama.pos.left",
            Pos::Centre => "drama.pos.centre",
            Pos::Right => "drama.pos.right",
        }
    }
}

/// Somebody in the cast, and the ElevenLabs voice they speak with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Voice {
    /// The `id=""` the dialogue refers to them by, where the file uses ids.
    pub id: Option<String>,
    pub name: String,
    /// `male`, `female`, or whatever else was written. Not acted on — the
    /// ElevenLabs voice already has a gender — but shown, because it is what
    /// makes choosing one from a list of five hundred possible.
    pub gender: Option<String>,
    /// The ElevenLabs voice id. Empty until somebody sets it.
    pub voice_id: String,
}

impl Voice {
    pub fn is_ready(&self) -> bool {
        !self.voice_id.trim().is_empty()
    }
}

/// One thing somebody says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The `line=""` the file gave it, if it gave it one.
    pub number: Option<u32>,
    /// Whichever of these the file used to say who is speaking.
    pub id: Option<String>,
    pub name: Option<String>,
    pub age: Option<u32>,
    pub state: State,
    pub pos: Pos,
    pub text: String,
}

/// Something read that was not quite right, and what was done about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Which line of the file, counting from 1.
    pub line: usize,
    pub kind: Kind,
    /// The offending word, for putting in the message.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A `state=""` this program has no sound for. Spoken plainly instead.
    UnknownState,
    /// A `pos=""` that is not left, centre or right. Placed in the centre.
    UnknownPos,
    /// A line whose speaker is in no `<voices>` section. One was added.
    UnknownSpeaker,
    /// A voice id typed inside the tag rather than between the tags. Taken
    /// anyway, because there is only one thing it can have been meant to be.
    VoiceInTag,
    /// A `<voices>` section that was not there. One was made from the dialogue.
    NoVoices,
    /// A line with no words in it. Left out of the recording.
    Empty,
    /// A tag that never closed, or closed as something else.
    Malformed,
    /// A `model=""` this version does not know. Read as best it can be.
    UnknownModel,
}

impl Kind {
    /// Every kind, so that the language file can be checked against them.
    pub const ALL: [Kind; 8] = [
        Kind::UnknownState,
        Kind::UnknownPos,
        Kind::UnknownSpeaker,
        Kind::VoiceInTag,
        Kind::NoVoices,
        Kind::Empty,
        Kind::Malformed,
        Kind::UnknownModel,
    ];

    /// The language key the window shows it under.
    pub fn key(self) -> &'static str {
        match self {
            Kind::UnknownState => "drama.problem.state",
            Kind::UnknownPos => "drama.problem.pos",
            Kind::UnknownSpeaker => "drama.problem.speaker",
            Kind::VoiceInTag => "drama.problem.voice_in_tag",
            Kind::NoVoices => "drama.problem.no_voices",
            Kind::Empty => "drama.problem.empty",
            Kind::Malformed => "drama.problem.malformed",
            Kind::UnknownModel => "drama.problem.model",
        }
    }
}

/// A whole audio drama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Story {
    /// What the file's `model=""` said. `mode=""` is read too: it is what the
    /// format's own note calls the attribute, and disagreeing with somebody's
    /// file about the name of a thing helps nobody.
    pub model: String,
    pub voices: Vec<Voice>,
    pub lines: Vec<Line>,
    pub problems: Vec<Problem>,
    /// Whether the file refers to people by `id=""` rather than by name. Kept
    /// so that writing the file back does not quietly change its dialect.
    pub uses_ids: bool,
    /// Whether the file numbered its lines.
    pub numbers_lines: bool,
}

impl Default for Story {
    fn default() -> Story {
        Story {
            model: DEFAULT_MODEL.to_string(),
            voices: Vec::new(),
            lines: Vec::new(),
            problems: Vec::new(),
            uses_ids: false,
            numbers_lines: false,
        }
    }
}

impl Story {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Which voice speaks a line: by id where the file uses ids, by name
    /// otherwise, and by name as a second try either way.
    pub fn voice_of(&self, line: &Line) -> Option<usize> {
        if let Some(id) = line.id.as_deref().filter(|id| !id.is_empty()) {
            if let Some(at) = self.voices.iter().position(|v| v.id.as_deref() == Some(id)) {
                return Some(at);
            }
        }
        let name = line.name.as_deref()?;
        self.voices.iter().position(|v| same_name(&v.name, name))
    }

    /// What to call whoever speaks a line, whatever the file had to go on.
    pub fn speaker_of(&self, line: &Line) -> String {
        if let Some(at) = self.voice_of(line) {
            return self.voices[at].name.clone();
        }
        line.name
            .clone()
            .or_else(|| line.id.clone())
            .unwrap_or_default()
    }

    /// Give everybody who speaks an entry in `<voices>`, so that there is
    /// somewhere to put their voice.
    ///
    /// This is what happens to a file that arrived with no `<voices>` section
    /// at all: rather than refusing it, the section is built out of the people
    /// the dialogue already names, ready to be filled in.
    ///
    /// `had_section` decides whether each person added is worth a note. Where
    /// there was a cast list, somebody missing from it is worth pointing at —
    /// it is usually a misspelling in the dialogue. Where there was no cast
    /// list at all, everybody is missing from it, and saying so once per
    /// character would bury the one note that explains it.
    fn cast_from_dialogue(&mut self, had_section: bool) {
        let mut added = Vec::new();
        for index in 0..self.lines.len() {
            if self.voice_of(&self.lines[index]).is_some() {
                continue;
            }
            let line = &self.lines[index];
            let name = line
                .name
                .clone()
                .or_else(|| line.id.clone())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            added.push((line.id.clone(), name.clone()));
            self.voices.push(Voice {
                id: line.id.clone(),
                name,
                gender: None,
                voice_id: String::new(),
            });
        }
        if had_section {
            for (_, name) in added {
                self.problems.push(Problem {
                    line: 0,
                    kind: Kind::UnknownSpeaker,
                    detail: name,
                });
            }
        }
    }

    /// The file as this program would write it.
    ///
    /// The dialect is the one that was read: a file that used ids keeps its
    /// ids, and a file that numbered its lines keeps its numbers. The one
    /// deliberate change is that `<voices>` is always written, because the
    /// whole point of saving is to keep the voices somebody has just chosen.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("<story model=\"{}\">\n", escape(&self.model)));

        out.push_str("    <voices>\n");
        for voice in &self.voices {
            out.push_str("        <character");
            if let Some(id) = &voice.id {
                out.push_str(&format!(" id=\"{}\"", escape(id)));
            }
            out.push_str(&format!(" name=\"{}\"", escape(&voice.name)));
            if let Some(gender) = &voice.gender {
                out.push_str(&format!(" gender=\"{}\"", escape(gender)));
            }
            out.push_str(&format!(">{}</character>\n", escape(voice.voice_id.trim())));
        }
        out.push_str("    </voices>\n");

        out.push_str("    <dialog>\n");
        for (index, line) in self.lines.iter().enumerate() {
            out.push_str("        <character");
            if self.numbers_lines {
                let number = line.number.unwrap_or(index as u32 + 1);
                out.push_str(&format!(" line=\"{number}\""));
            }
            // Whichever way the file names people, name them that way again —
            // but never write an `id` the cast list does not have, or the file
            // would come back with a line nobody speaks.
            let voice = self.voice_of(line).map(|at| &self.voices[at]);
            match (self.uses_ids, voice.and_then(|v| v.id.clone())) {
                (true, Some(id)) => out.push_str(&format!(" id=\"{}\"", escape(&id))),
                _ => {
                    let name = voice
                        .map(|v| v.name.clone())
                        .or_else(|| line.name.clone())
                        .unwrap_or_default();
                    out.push_str(&format!(" name=\"{}\"", escape(&name)));
                }
            }
            if let Some(age) = line.age {
                out.push_str(&format!(" age=\"{age}\""));
            }
            out.push_str(&format!(" state=\"{}\"", line.state.word()));
            out.push_str(&format!(" pos=\"{}\"", line.pos.word()));
            out.push_str(&format!(">{}</character>\n", escape(&line.text)));
        }
        out.push_str("    </dialog>\n");
        out.push_str("</story>\n");
        out
    }
}

impl fmt::Display for Story {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_xml())
    }
}

/// Names are compared the way people type them, which is to say carelessly.
fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Read an audio drama. Never fails: what could not be understood comes back
/// in [`Story::problems`].
pub fn parse(source: &str) -> Story {
    let (roots, mut problems) = read_xml(source);

    let mut story = Story::default();
    let root = roots
        .iter()
        .find(|element| element.name.eq_ignore_ascii_case("story"))
        .or_else(|| roots.first());
    let Some(root) = root else {
        story.problems = problems;
        return story;
    };

    // `model` is what the files say; `mode` is what the format's own note
    // calls it. Take either.
    if let Some(model) = root.attr("model").or_else(|| root.attr("mode")) {
        story.model = model.trim().to_string();
        if !KNOWN_MODELS.contains(&story.model.as_str()) {
            problems.push(Problem {
                line: root.line,
                kind: Kind::UnknownModel,
                detail: story.model.clone(),
            });
        }
    }

    let voices_section = root.child("voices");
    for element in voices_section.iter().flat_map(|s| s.children("character")) {
        let mut voice_id = element.text.trim().to_string();
        // The one recovery worth making: `gender="female"VOICE_ID></character>`
        // is a quote in the wrong place, and there is nothing else a bare word
        // wedged among the attributes of a cast entry could have been.
        if voice_id.is_empty() {
            if let Some(stray) = element.junk.first() {
                voice_id = stray.clone();
                problems.push(Problem {
                    line: element.line,
                    kind: Kind::VoiceInTag,
                    detail: stray.clone(),
                });
            }
        }
        let id = element.attr("id").map(str::to_string);
        story.uses_ids |= id.is_some();
        story.voices.push(Voice {
            id,
            name: element.attr("name").unwrap_or_default().trim().to_string(),
            gender: element
                .attr("gender")
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty()),
            voice_id,
        });
    }

    let dialog = root
        .child("dialog")
        .or_else(|| root.child("dialogue"))
        .or(Some(root));
    for element in dialog.iter().flat_map(|s| s.children("character")) {
        let text = element.text.trim().to_string();
        if text.is_empty() {
            problems.push(Problem {
                line: element.line,
                kind: Kind::Empty,
                detail: element.attr("name").unwrap_or_default().to_string(),
            });
            continue;
        }

        let state = match element.attr("state") {
            Some(word) => State::parse(word).unwrap_or_else(|| {
                problems.push(Problem {
                    line: element.line,
                    kind: Kind::UnknownState,
                    detail: word.trim().to_string(),
                });
                State::Normal
            }),
            None => State::Normal,
        };
        let pos = match element.attr("pos") {
            Some(word) => Pos::parse(word).unwrap_or_else(|| {
                problems.push(Problem {
                    line: element.line,
                    kind: Kind::UnknownPos,
                    detail: word.trim().to_string(),
                });
                Pos::Centre
            }),
            None => Pos::Centre,
        };

        let number = element.attr("line").and_then(|n| n.trim().parse().ok());
        story.numbers_lines |= number.is_some();
        let id = element.attr("id").map(str::to_string);
        story.uses_ids |= id.is_some();

        story.lines.push(Line {
            number,
            id,
            name: element
                .attr("name")
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty()),
            age: element.attr("age").and_then(|a| a.trim().parse().ok()),
            state,
            pos,
            text,
        });
    }

    if voices_section.is_none() && !story.lines.is_empty() {
        problems.push(Problem {
            line: root.line,
            kind: Kind::NoVoices,
            detail: String::new(),
        });
    }

    story.problems = problems;
    story.cast_from_dialogue(voices_section.is_some());
    story
}

// -- a very small XML reader --------------------------------------------------

/// One element, with enough of the file's shape kept to report on it.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    /// Everything between the tags, with the entities resolved.
    text: String,
    children: Vec<Node>,
    /// Which line of the file it opened on, counting from 1.
    line: usize,
    /// Words that stood where an attribute should have been but had no value.
    junk: Vec<String>,
}

impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn child(&self, name: &str) -> Option<&Node> {
        self.children
            .iter()
            .find(|child| child.name.eq_ignore_ascii_case(name))
    }

    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children
            .iter()
            .filter(move |child| child.name.eq_ignore_ascii_case(name))
    }
}

/// Read a document into elements, and say what was wrong with it.
///
/// Written out here rather than brought in, for the reason the whole of this
/// program gives: it is two hundred lines for the subset a story file uses, and
/// this is a tool with two dependencies.
fn read_xml(source: &str) -> (Vec<Node>, Vec<Problem>) {
    let mut problems = Vec::new();
    let mut roots: Vec<Node> = Vec::new();
    let mut stack: Vec<Node> = Vec::new();
    let bytes = source.as_bytes();
    let mut at = 0usize;

    while at < bytes.len() {
        let Some(offset) = source[at..].find('<') else {
            push_text(&mut stack, &source[at..]);
            break;
        };
        push_text(&mut stack, &source[at..at + offset]);
        at += offset;
        let rest = &source[at..];
        let line = line_of(source, at);

        if rest.starts_with("<!--") {
            at = skip_to(source, at + 4, "-->", &mut problems, line, "<!--");
        } else if rest.starts_with("<![CDATA[") {
            let start = at + 9;
            let end = source[start..]
                .find("]]>")
                .map(|i| start + i)
                .unwrap_or(source.len());
            push_text(&mut stack, &source[start..end]);
            at = (end + 3).min(source.len());
        } else if rest.starts_with("<?") {
            at = skip_to(source, at + 2, "?>", &mut problems, line, "<?");
        } else if rest.starts_with("<!") {
            at = skip_to(source, at + 2, ">", &mut problems, line, "<!");
        } else if rest.starts_with("</") {
            let end = source[at..].find('>').map(|i| at + i).unwrap_or(source.len());
            let name = source[at + 2..end].trim().to_string();
            close(&mut stack, &mut roots, &name, line, &mut problems);
            at = (end + 1).min(source.len());
        } else {
            let (node, next, empty) = read_tag(source, at, line, &mut problems);
            at = next;
            if empty {
                finish(&mut stack, &mut roots, node);
            } else {
                stack.push(node);
            }
        }
    }

    // Whatever is still open never closed. Keep it rather than lose it, and
    // say so.
    while let Some(node) = stack.pop() {
        problems.push(Problem {
            line: node.line,
            kind: Kind::Malformed,
            detail: node.name.clone(),
        });
        finish(&mut stack, &mut roots, node);
    }
    (roots, problems)
}

fn skip_to(
    source: &str,
    from: usize,
    terminator: &str,
    problems: &mut Vec<Problem>,
    line: usize,
    what: &str,
) -> usize {
    match source[from..].find(terminator) {
        Some(i) => from + i + terminator.len(),
        None => {
            problems.push(Problem {
                line,
                kind: Kind::Malformed,
                detail: what.to_string(),
            });
            source.len()
        }
    }
}

/// Read an opening tag and its attributes. Returns the element, where the
/// reader got to, and whether the tag closed itself.
fn read_tag(
    source: &str,
    at: usize,
    line: usize,
    problems: &mut Vec<Problem>,
) -> (Node, usize, bool) {
    let bytes = source.as_bytes();
    let mut i = at + 1;
    let start = i;
    while i < bytes.len() && !is_break(bytes[i]) {
        i += 1;
    }
    let mut node = Node {
        name: source[start..i].to_string(),
        line,
        ..Node::default()
    };

    let mut empty = false;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            problems.push(Problem {
                line,
                kind: Kind::Malformed,
                detail: node.name.clone(),
            });
            break;
        }
        if bytes[i] == b'>' {
            i += 1;
            break;
        }
        if bytes[i] == b'/' {
            empty = true;
            i += 1;
            continue;
        }

        let name_start = i;
        while i < bytes.len() && !is_break(bytes[i]) && bytes[i] != b'=' {
            i += 1;
        }
        if i == name_start {
            // A character that can begin nothing: step over it rather than
            // spin on it for ever.
            i += 1;
            continue;
        }
        let name = source[name_start..i].to_string();

        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            // No value. Remembered rather than dropped, because in a cast
            // entry a bare word is almost certainly a voice id that lost its
            // quote — see the recovery in `parse`.
            node.junk.push(name);
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let (value, next) = read_value(source, j);
        node.attrs.push((name, value));
        i = next;
    }
    (node, i, empty)
}

fn read_value(source: &str, at: usize) -> (String, usize) {
    let bytes = source.as_bytes();
    if at >= bytes.len() {
        return (String::new(), at);
    }
    let quote = bytes[at];
    if quote == b'"' || quote == b'\'' {
        let start = at + 1;
        let end = source[start..]
            .find(quote as char)
            .map(|i| start + i)
            .unwrap_or(source.len());
        (unescape(&source[start..end]), (end + 1).min(source.len()))
    } else {
        let start = at;
        let mut i = at;
        while i < bytes.len() && !is_break(bytes[i]) {
            i += 1;
        }
        (unescape(&source[start..i]), i)
    }
}

/// Characters that end a name or an unquoted value.
fn is_break(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte == b'>' || byte == b'/'
}

fn push_text(stack: &mut [Node], text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(node) = stack.last_mut() {
        node.text.push_str(&unescape(text));
    }
}

/// Close the innermost tag of this name, so that a stray `</b>` in the middle
/// of an `<a>` does not take the `<a>` down with it.
fn close(
    stack: &mut Vec<Node>,
    roots: &mut Vec<Node>,
    name: &str,
    line: usize,
    problems: &mut Vec<Problem>,
) {
    let found = stack
        .iter()
        .rposition(|node| node.name.eq_ignore_ascii_case(name));
    let Some(found) = found else {
        problems.push(Problem {
            line,
            kind: Kind::Malformed,
            detail: format!("</{name}>"),
        });
        return;
    };
    while stack.len() > found {
        let node = stack.pop().expect("the element being closed");
        if stack.len() > found {
            problems.push(Problem {
                line: node.line,
                kind: Kind::Malformed,
                detail: node.name.clone(),
            });
        }
        finish(stack, roots, node);
    }
}

fn finish(stack: &mut [Node], roots: &mut Vec<Node>, node: Node) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => roots.push(node),
    }
}

fn line_of(source: &str, at: usize) -> usize {
    source[..at].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Turn `&amp;` and friends back into what they stand for.
fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let resolved = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix('#')
                .and_then(|number| match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => number.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match resolved {
            Some(character) => {
                out.push(character);
                rest = &rest[end + 1..];
            }
            // Not an entity this reader knows: leave it exactly as it was
            // rather than eat it.
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The five characters that cannot be written as themselves.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The format as the note that came with it spells it: numbered people,
    /// numbered lines, and a position for each.
    const NUMBERED: &str = r#"<story model="1.0">
    <voices>
        <character id="1" name="ben" gender="male">VOICE_ID_HERE</character>
        <character id="2" name="faith" gender="female"VOICE_ID_HERE></character>
    </voices>
    <dialog>
        <character line="1" id="1" age="12" state="normal" pos="left">I don't remember how it happened</character>
        <character line="2" id="2" age="15" state="whisper" pos="right">What?</character>
    </dialog>
</story>"#;

    /// The other spelling: people by name, and no positions.
    const NAMED: &str = r#"<story model="1.2">
    <voices>
        <character name="ben" gender="male"></character>
        <character name="faith" gender="female"></character>
    </voices>
    <dialog>
        <character name="ben" age="12" state="normal">I don't remember how it happened</character>
        <character name="faith" age="15" state="whisper">What?</character>
    </dialog>
</story>"#;

    #[test]
    fn the_numbered_spelling_is_read_whole() {
        let story = parse(NUMBERED);
        assert_eq!(story.model, "1.0");
        assert!(story.uses_ids);
        assert!(story.numbers_lines);

        assert_eq!(story.voices.len(), 2);
        assert_eq!(story.voices[0].id.as_deref(), Some("1"));
        assert_eq!(story.voices[0].name, "ben");
        assert_eq!(story.voices[0].gender.as_deref(), Some("male"));

        assert_eq!(story.lines.len(), 2);
        assert_eq!(story.lines[0].number, Some(1));
        assert_eq!(story.lines[0].age, Some(12));
        assert_eq!(story.lines[0].state, State::Normal);
        assert_eq!(story.lines[0].pos, Pos::Left);
        assert_eq!(story.lines[0].text, "I don't remember how it happened");
        assert_eq!(story.lines[1].state, State::Whisper);
        assert_eq!(story.lines[1].pos, Pos::Right);

        // And each line knows who says it, through the id.
        assert_eq!(story.speaker_of(&story.lines[0]), "ben");
        assert_eq!(story.speaker_of(&story.lines[1]), "faith");
    }

    /// The quote in the wrong place is the mistake this format invites, and
    /// losing somebody's voice to it would be a poor way to greet them.
    #[test]
    fn a_voice_typed_inside_the_tag_is_taken_anyway_and_reported() {
        let story = parse(NUMBERED);
        assert_eq!(story.voices[1].voice_id, "VOICE_ID_HERE");
        let problem = story
            .problems
            .iter()
            .find(|problem| problem.kind == Kind::VoiceInTag)
            .expect("the misplaced voice should be reported");
        assert_eq!(problem.detail, "VOICE_ID_HERE");
        assert_eq!(problem.line, 4);
    }

    #[test]
    fn the_named_spelling_is_read_whole_too() {
        let story = parse(NAMED);
        assert_eq!(story.model, "1.2");
        assert!(!story.uses_ids);
        assert!(!story.numbers_lines);
        assert_eq!(story.lines.len(), 2);
        assert_eq!(story.speaker_of(&story.lines[1]), "faith");
        // No `pos` in this spelling, so everybody stands in the middle.
        assert!(story.lines.iter().all(|line| line.pos == Pos::Centre));
        // And nobody has a voice yet, which is the tab's whole first job.
        assert!(story.voices.iter().all(|voice| !voice.is_ready()));
    }

    /// A file with no cast list should get one note explaining that, not that
    /// note plus one more per character saying the same thing again.
    #[test]
    fn a_missing_cast_list_is_reported_once_rather_than_once_per_character() {
        let story = parse(
            r#"<story model="1.2"><dialog>
                 <character name="ben">Hello</character>
                 <character name="faith">Hello yourself</character>
                 <character name="mo">And me</character>
               </dialog></story>"#,
        );
        assert_eq!(story.voices.len(), 3, "everybody who speaks is still added");
        assert_eq!(
            story.problems.iter().filter(|p| p.kind == Kind::NoVoices).count(),
            1
        );
        assert_eq!(
            story.problems.iter().filter(|p| p.kind == Kind::UnknownSpeaker).count(),
            0,
            "there was no cast list to be missing from"
        );
    }

    /// The case the tab exists to fix: a file with dialogue and no cast list.
    #[test]
    fn a_file_with_no_voices_section_gets_one_built_from_whoever_speaks() {
        let story = parse(
            r#"<story model="1.0">
                 <dialog>
                   <character name="ben" age="12">Hello</character>
                   <character name="faith">Hello yourself</character>
                   <character name="ben">Again</character>
                 </dialog>
               </story>"#,
        );
        let names: Vec<&str> = story.voices.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["ben", "faith"], "each speaker once, in the order they speak");
        assert!(story.voices.iter().all(|voice| voice.voice_id.is_empty()));
        assert!(story.problems.iter().any(|p| p.kind == Kind::NoVoices));
        // And the writing-back puts the section in, which is what makes the
        // choice survive.
        assert!(story.to_xml().contains("<voices>"));
    }

    /// Somebody the cast list forgot is added rather than dropped — a line
    /// nobody can speak is worse than a cast list with a stranger in it.
    #[test]
    fn a_speaker_missing_from_the_cast_list_is_added_and_reported() {
        let story = parse(
            r#"<story><voices><character name="ben">v1</character></voices>
               <dialog><character name="mo">Who am I?</character></dialog></story>"#,
        );
        assert_eq!(story.voices.len(), 2);
        assert!(story.voices.iter().any(|voice| voice.name == "mo"));
        assert!(story
            .problems
            .iter()
            .any(|p| p.kind == Kind::UnknownSpeaker && p.detail == "mo"));
    }

    #[test]
    fn a_story_written_back_out_reads_the_same_again() {
        for source in [NUMBERED, NAMED] {
            let first = parse(source);
            let again = parse(&first.to_xml());
            assert_eq!(first.model, again.model);
            assert_eq!(first.voices, again.voices);
            assert_eq!(first.lines, again.lines);
            assert_eq!(first.uses_ids, again.uses_ids);
            assert_eq!(first.numbers_lines, again.numbers_lines);
        }
    }

    /// The apostrophe in "I don't" becomes an entity on the way out, and has
    /// to come back as an apostrophe rather than as `&apos;`.
    #[test]
    fn punctuation_survives_a_round_trip() {
        let mut story = parse(NAMED);
        story.lines[0].text = "\"Ben & Faith\" <both> said it — didn't they?".to_string();
        let again = parse(&story.to_xml());
        assert_eq!(again.lines[0].text, "\"Ben & Faith\" <both> said it — didn't they?");
    }

    #[test]
    fn every_entity_a_story_might_carry_is_understood() {
        assert_eq!(unescape("&amp;&lt;&gt;&quot;&apos;"), "&<>\"'");
        assert_eq!(unescape("&#65;&#x42;"), "AB");
        // Something that is not an entity is left exactly as it was rather
        // than swallowed.
        assert_eq!(unescape("Tom & Jerry"), "Tom & Jerry");
        assert_eq!(unescape("&nosuchthing;"), "&nosuchthing;");
        assert_eq!(unescape("100% & rising"), "100% & rising");
    }

    #[test]
    fn a_state_nobody_has_heard_of_is_spoken_plainly_and_reported() {
        let story = parse(
            r#"<story><dialog><character name="ben" state="baffled" pos="up">Eh?</character></dialog></story>"#,
        );
        assert_eq!(story.lines[0].state, State::Normal);
        assert_eq!(story.lines[0].pos, Pos::Centre);
        assert!(story
            .problems
            .iter()
            .any(|p| p.kind == Kind::UnknownState && p.detail == "baffled"));
        assert!(story
            .problems
            .iter()
            .any(|p| p.kind == Kind::UnknownPos && p.detail == "up"));
    }

    #[test]
    fn the_several_words_people_use_for_the_same_state_all_work() {
        assert_eq!(State::parse("WHISPERING"), Some(State::Whisper));
        assert_eq!(State::parse(" terrified "), Some(State::Scared));
        assert_eq!(State::parse("yell"), Some(State::Shout));
        assert_eq!(State::parse(""), Some(State::Normal));
        assert_eq!(State::parse("baffled"), None);
        // And both spellings of the middle.
        assert_eq!(Pos::parse("center"), Some(Pos::Centre));
        assert_eq!(Pos::parse("CENTRE"), Some(Pos::Centre));
    }

    /// `mode` is what the format's own note calls the attribute and `model` is
    /// what the files use. Disagreeing with somebody's file about the name of
    /// a thing helps nobody.
    #[test]
    fn either_spelling_of_the_version_attribute_is_read() {
        assert_eq!(parse(r#"<story mode="1.2"><dialog/></story>"#).model, "1.2");
        assert_eq!(parse(r#"<story model="1.2"><dialog/></story>"#).model, "1.2");
        // And a version this build does not know is still read, with a note.
        let future = parse(r#"<story model="9.9"><dialog/></story>"#);
        assert_eq!(future.model, "9.9");
        assert!(future.problems.iter().any(|p| p.kind == Kind::UnknownModel));
    }

    #[test]
    fn a_line_with_nothing_in_it_is_left_out_of_the_recording() {
        let story = parse(
            r#"<story><dialog>
                 <character name="ben">   </character>
                 <character name="ben">Something</character>
               </dialog></story>"#,
        );
        assert_eq!(story.lines.len(), 1);
        assert!(story.problems.iter().any(|p| p.kind == Kind::Empty));
    }

    #[test]
    fn comments_and_declarations_are_stepped_over() {
        let story = parse(
            r#"<?xml version="1.0" encoding="utf-8"?>
               <!-- the first draft -->
               <story model="1.0">
                 <dialog><character name="ben"><![CDATA[Nothing < here & there]]></character></dialog>
               </story>"#,
        );
        assert_eq!(story.lines.len(), 1);
        assert_eq!(story.lines[0].text, "Nothing < here & there");
    }

    /// A file that is half wrong should still get you the half that is right.
    #[test]
    fn a_tag_that_never_closes_keeps_what_was_in_it() {
        let story = parse(
            r#"<story><dialog><character name="ben">Unfinished business</dialog></story>"#,
        );
        assert_eq!(story.lines.len(), 1);
        assert_eq!(story.lines[0].text, "Unfinished business");
        assert!(story.problems.iter().any(|p| p.kind == Kind::Malformed));
    }

    #[test]
    fn nothing_at_all_is_an_empty_story_rather_than_a_panic() {
        for source in ["", "   ", "not xml", "<", "<story", "<<<>>>", "</story>"] {
            let story = parse(source);
            assert!(story.is_empty(), "{source:?} should read as nothing");
        }
    }

    /// The writing-back must never name somebody the cast list does not have,
    /// or the file would come back with a line nobody speaks.
    #[test]
    fn writing_back_never_invents_a_speaker_the_cast_list_lacks() {
        let story = parse(NUMBERED);
        let again = parse(&story.to_xml());
        for line in &again.lines {
            assert!(again.voice_of(line).is_some(), "{line:?} lost its speaker");
        }
    }
}
