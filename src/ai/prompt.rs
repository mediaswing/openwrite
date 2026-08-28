//! What to ask a model, and what to tell it first.
//!
//! A local model does not know the writer's story, so most of the work of
//! getting a useful answer is assembling the briefing: the world, the people,
//! and the last page or so of script leading up to where the caret is. That is
//! what [`Context`] gathers and [`Context::prompt`] writes out.
//!
//! The asks themselves are deliberately few, and each one is the question a
//! writer actually stops to ask ­— what would this person do here, what would
//! they say, what happens next — rather than "write my screenplay".
//!
//! Nothing here talks to anything. It builds strings, so it can be read and
//! tested without a model anywhere near it.

use crate::bible::{normalise, Bible};
use crate::t;

/// How much of the script to send. A page of screenplay is roughly 1,800
/// characters, so this is the last page or two: enough for the model to hear
/// where the scene has got to, short enough to leave a small model room to
/// think.
pub const EXCERPT: usize = 3_000;

/// The question being put to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// What would this character do here?
    Do(String),
    /// What would this character say here?
    Say(String),
    /// What should happen next?
    Next,
}

impl Ask {
    /// The character this ask is about, if it is about one.
    pub fn character(&self) -> Option<&str> {
        match self {
            Ask::Do(name) | Ask::Say(name) => Some(name),
            Ask::Next => None,
        }
    }

    /// How the ask reads in the window and in the status bar.
    ///
    /// The one part of this file that is spoken to the writer rather than to
    /// the model, so it is the one part that comes out of the language file.
    pub fn label(&self) -> String {
        match self {
            Ask::Do(name) => t!("ask.do.named", name = name),
            Ask::Say(name) => t!("ask.say.named", name = name),
            Ask::Next => t!("ask.next"),
        }
    }

    /// The instruction the model is given, and the shape of answer wanted.
    ///
    /// Each one says how many options and in what form, because a model left to
    /// choose will write an essay, and an essay cannot be pasted into a script.
    fn instruction(&self) -> String {
        match self {
            Ask::Do(name) => format!(
                "Suggest three different things {name} could do next — actions that are not in \
                 the script yet. Each one is a single short paragraph of action, present tense, \
                 describing only what a camera could see: no interior thoughts, no explanation \
                 of why, and no dialogue. Separate the three with a blank line. Write nothing \
                 else."
            ),
            Ask::Say(name) => format!(
                "Suggest three different things {name} could say next — lines that are not in the \
                 script yet. Write each as Fountain dialogue: the line \"{name}\" on its own, \
                 exactly that and nothing after it, then underneath it the line or two they \
                 speak. A parenthetical in round brackets on its own line is allowed, sparingly. \
                 Match their voice as described above. Separate the three with a blank line. \
                 Write no action, no scene heading, and nothing else."
            ),
            Ask::Next => "Suggest three different things that could happen next in this story. \
                 Write each as a single line beginning with \"= \" — that is a Fountain synopsis, \
                 a note to the writer that never prints. One sentence each, concrete, and each \
                 one a genuinely different direction rather than three shades of the same. \
                 Write nothing else."
                .to_string(),
        }
    }
}

/// The standing instruction, sent with every ask.
///
/// It is about restraint as much as about format. A suggestion the writer has
/// to unpick prose out of is worse than no suggestion, and a model that invents
/// a new character has answered a question nobody asked.
pub const SYSTEM: &str = "\
You are helping a screenwriter who is part-way through a draft. They will show you their notes on \
the world, their notes on the characters, and the last page or so of the script. Then they will \
ask you one question about it.

Answer with new material that can be pasted straight into the screenplay. Never open with a \
preamble like \"Here are some ideas\", never explain your suggestions afterwards, never number \
them, and never use Markdown headings, bullets or bold. Do not summarise the scene back to them, \
and never repeat a line that is already in the script — the writer has those. Everything you \
write should be what comes next, not what has just happened.

Stay inside the story as described. Use the world's own vocabulary. Do not invent a new named \
character unless the writer asks for one, and do not contradict what the notes say. If the notes \
do not settle something, choose the simplest possibility and go on.

The script is written in Fountain: a scene heading is a line starting INT. or EXT.; a character \
cue is a line in capitals with the dialogue underneath it; action is plain prose in the present \
tense. Write in the same form.

Be brief. The writer is mid-sentence and wants a way forward, not a draft.";

/// Everything the model is told before the question.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The screenplay's title, if it has one yet.
    pub title: Option<String>,
    /// The world as the writer described it.
    pub world: String,
    /// Character briefings, most relevant first.
    pub people: Vec<String>,
    /// The scene heading the caret is under.
    pub scene: Option<String>,
    /// The script leading up to the caret.
    pub excerpt: String,
}

impl Context {
    /// Gather the briefing for one ask.
    ///
    /// `caret` is a character offset into `source`; without one the end of the
    /// script is used, which is where a writer usually is.
    pub fn gather(source: &str, caret: Option<usize>, bible: &Bible, ask: &Ask) -> Context {
        let doc = crate::parse(source);
        let here = excerpt(source, caret, EXCERPT);
        Context {
            title: doc.meta_line(&["Title"]).filter(|t| !t.trim().is_empty()),
            world: bible.world.trim().to_string(),
            people: people_for(bible, ask, &here.text),
            scene: here.heading,
            excerpt: here.text,
        }
    }

    /// The briefing and the question, as one prompt.
    pub fn prompt(&self, ask: &Ask) -> String {
        let mut out = String::with_capacity(self.excerpt.len() + 1024);

        if let Some(title) = &self.title {
            out.push_str(&format!("THE SCREENPLAY\n{title}\n\n"));
        }
        if !self.world.is_empty() {
            out.push_str(&format!("THE WORLD\n{}\n\n", self.world));
        }
        if !self.people.is_empty() {
            out.push_str("THE CHARACTERS\n");
            for person in &self.people {
                out.push_str(person);
                if !person.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push('\n');
        }

        out.push_str("THE SCRIPT SO FAR\n");
        match &self.scene {
            Some(heading) => {
                out.push_str(&format!("The caret is in this scene: {heading}\n\n"));
            }
            None => out.push_str("The script has not reached a scene heading yet.\n\n"),
        }
        if self.excerpt.trim().is_empty() {
            out.push_str("(The page is still empty.)\n");
        } else {
            out.push_str(&self.excerpt);
            if !self.excerpt.ends_with('\n') {
                out.push('\n');
            }
        }

        out.push_str("\nTHE QUESTION\n");
        out.push_str(&ask.instruction());
        out.push('\n');
        out
    }

    /// Is there enough here for an answer to be worth anything?
    ///
    /// A model given nothing will happily invent somebody else's story, so it
    /// is better to say what is missing than to ask and be disappointed.
    pub fn thin(&self) -> Option<String> {
        if self.excerpt.trim().is_empty() && self.world.is_empty() && self.people.is_empty() {
            Some(t!("ideas.nothing_to_go_on"))
        } else {
            None
        }
    }
}

/// Which characters to describe, and in what order.
///
/// The one being asked about comes first and is always included, even if the
/// writer has not written anything about them yet — the model needs to know
/// whose turn it is. After that come the ones who appear in the excerpt, then
/// everybody else, because a scene is mostly about the people in it.
fn people_for(bible: &Bible, ask: &Ask, excerpt: &str) -> Vec<String> {
    let focus = ask.character().map(normalise);
    let upper = excerpt.to_uppercase();

    let mut ordered: Vec<(u8, String)> = bible
        .profiles
        .iter()
        .filter(|profile| !profile.is_bare() || Some(&profile.name) == focus.as_ref())
        .map(|profile| {
            let rank = if Some(&profile.name) == focus.as_ref() {
                0
            } else if upper.contains(&profile.name) {
                1
            } else {
                2
            };
            (rank, profile.brief())
        })
        .collect();
    ordered.sort_by_key(|(rank, _)| *rank);

    let mut people: Vec<String> = ordered.into_iter().map(|(_, brief)| brief).collect();

    // Asked about somebody with no profile at all: say who they are anyway,
    // rather than leaving the model to guess which name is the character.
    if let Some(name) = focus {
        if bible.get(&name).is_none() {
            people.insert(0, format!("{name}\n  (no notes yet — go by how they speak below)\n"));
        }
    }
    people
}

/// The piece of script leading up to the caret, and the scene it is in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excerpt {
    pub heading: Option<String>,
    pub text: String,
}

/// Take the script up to the caret, back as far as the current scene heading —
/// or, if that scene is a long one, the last `budget` characters of it.
///
/// Cutting at the scene heading matters more than cutting at a fixed length: a
/// model that can see where the scene started knows where it is, and a model
/// handed the middle of a scene does not.
pub fn excerpt(source: &str, caret: Option<usize>, budget: usize) -> Excerpt {
    let end = match caret {
        Some(caret) => source
            .char_indices()
            .nth(caret)
            .map(|(i, _)| i)
            .unwrap_or(source.len()),
        None => source.len(),
    };
    let before = &source[..end];

    // Walk back to the last scene heading at or before the caret.
    let mut start = 0;
    let mut heading = None;
    for (offset, line) in line_starts(before) {
        if let Some(text) = scene_heading(line) {
            start = offset;
            heading = Some(text);
        }
    }

    let mut text = &before[start..];
    if text.chars().count() > budget {
        // Too long: keep the end, and start at a line boundary so the excerpt
        // never opens mid-word.
        let skip = text.chars().count() - budget;
        let cut = text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
        text = match text[cut..].find('\n') {
            Some(i) => &text[cut + i + 1..],
            None => &text[cut..],
        };
    }

    Excerpt { heading, text: text.trim_start_matches('\n').to_string() }
}

fn line_starts(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    text.split('\n').map(move |line| {
        let start = offset;
        offset += line.len() + 1;
        (start, line)
    })
}

/// The heading text of a line, if the line is a scene heading — including the
/// `.FORCED` form, whose leading dot is markup rather than part of the name.
fn scene_heading(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('.') {
        if !rest.starts_with('.') && !rest.is_empty() {
            return Some(rest.trim().to_string());
        }
        return None;
    }
    crate::parser::is_scene_heading(line).then(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bible::Bible;

    const SCRIPT: &str = "\
Title: Ashfen

INT. SALT HOUSE - NIGHT

Maya counts the tally sticks.

MAYA
Forty-one.

EXT. THE LAKE - LATER

Wind off the water.

DEV
You cannot cross tonight.
";

    fn bible() -> Bible {
        let mut bible =
            Bible { world: "Ashfen stands on a salt lake.".to_string(), ..Bible::default() };
        let maya = bible.ensure("MAYA");
        maya.want = "To buy her brother out".to_string();
        maya.voice = "Short sentences.".to_string();
        bible.ensure("DEV").role = "Guild clerk".to_string();
        bible
    }

    #[test]
    fn the_excerpt_starts_at_the_scene_the_caret_is_in() {
        let caret = SCRIPT.chars().count();
        let here = excerpt(SCRIPT, Some(caret), EXCERPT);
        assert_eq!(here.heading.as_deref(), Some("EXT. THE LAKE - LATER"));
        assert!(here.text.starts_with("EXT. THE LAKE - LATER"));
        assert!(here.text.contains("You cannot cross tonight."));
        // The scene before it is not in the way.
        assert!(!here.text.contains("Forty-one."));
    }

    #[test]
    fn a_caret_in_an_earlier_scene_gets_that_scene() {
        let caret = SCRIPT.find("Forty-one.").unwrap();
        let caret = SCRIPT[..caret].chars().count();
        let here = excerpt(SCRIPT, Some(caret), EXCERPT);
        assert_eq!(here.heading.as_deref(), Some("INT. SALT HOUSE - NIGHT"));
        assert!(!here.text.contains("THE LAKE"));
    }

    #[test]
    fn no_caret_means_the_end_of_the_script() {
        assert_eq!(excerpt(SCRIPT, None, EXCERPT), excerpt(SCRIPT, Some(usize::MAX), EXCERPT));
    }

    #[test]
    fn a_forced_heading_counts_and_loses_its_dot() {
        let source = "Action.\n\n.THE VOID\n\nNothing here.\n";
        let here = excerpt(source, None, EXCERPT);
        assert_eq!(here.heading.as_deref(), Some("THE VOID"));
        // Two dots is an escaped full stop, not a heading.
        let source = "Action.\n\n..not a heading\n\nMore.\n";
        assert_eq!(excerpt(source, None, EXCERPT).heading, None);
    }

    #[test]
    fn a_script_with_no_heading_yet_still_gives_its_text() {
        let here = excerpt("Just some action.\n", None, EXCERPT);
        assert_eq!(here.heading, None);
        assert_eq!(here.text, "Just some action.\n");
    }

    #[test]
    fn a_long_scene_is_cut_at_a_line_boundary_and_keeps_the_end() {
        let source = format!("INT. HALL - DAY\n\n{}\n\nThe last line.\n", "Filler line.\n".repeat(200));
        let here = excerpt(&source, None, 200);
        assert!(here.text.chars().count() <= 200);
        assert!(here.text.ends_with("The last line.\n"));
        assert!(here.text.starts_with("Filler line."), "cut mid-line: {:?}", &here.text[..20]);
        // The heading is still reported even though it did not fit.
        assert_eq!(here.heading.as_deref(), Some("INT. HALL - DAY"));
    }

    #[test]
    fn a_multi_byte_caret_lands_on_a_character_not_a_byte() {
        let source = "INT. CAFÉ — DAY\n\nShe waits.\n";
        let caret = source.chars().count();
        // Would panic on a byte boundary if the offsets were confused.
        assert_eq!(excerpt(source, Some(caret), EXCERPT).text, source);
    }

    #[test]
    fn the_briefing_carries_the_world_the_people_and_the_scene() {
        let ask = Ask::Say("MAYA".to_string());
        let context = Context::gather(SCRIPT, None, &bible(), &ask);
        let prompt = context.prompt(&ask);

        assert!(prompt.contains("Ashfen stands on a salt lake."));
        assert!(prompt.contains("Wants: To buy her brother out"));
        assert!(prompt.contains("EXT. THE LAKE - LATER"));
        assert!(prompt.contains("You cannot cross tonight."));
        assert!(prompt.contains("Ashfen"), "the title is missing");
        assert!(prompt.contains("THE QUESTION"), "the ask is missing");
        assert!(prompt.contains("Fountain dialogue"), "the ask is not the one that was made");
    }

    #[test]
    fn the_character_being_asked_about_is_described_first() {
        let ask = Ask::Do("DEV".to_string());
        let context = Context::gather(SCRIPT, None, &bible(), &ask);
        assert!(context.people[0].starts_with("DEV"), "{:?}", context.people[0]);
    }

    #[test]
    fn asking_about_somebody_with_no_notes_still_names_them() {
        let ask = Ask::Say("STRANGER".to_string());
        let context = Context::gather(SCRIPT, None, &Bible::default(), &ask);
        assert!(context.people[0].starts_with("STRANGER"));
        assert!(context.prompt(&ask).contains("no notes yet"));
    }

    #[test]
    fn ideas_are_asked_for_as_synopsis_lines_so_they_never_print() {
        let prompt = Ask::Next.instruction();
        assert!(prompt.contains("= "));
        assert!(prompt.contains("never prints"));
    }

    #[test]
    fn an_empty_page_with_no_notes_says_so_rather_than_asking() {
        let context = Context::gather("", None, &Bible::default(), &Ask::Next);
        assert!(context.thin().is_some());
        // With a world written down, there is something to work from.
        let bible = Bible { world: "A salt lake.".to_string(), ..Bible::default() };
        assert!(Context::gather("", None, &bible, &Ask::Next).thin().is_none());
    }
}
