//! Audio drama: turning a story file into something you can listen to.
//!
//! A screenplay is read; a radio play is heard. The three parts of getting
//! from one to the other are in the three modules below, and this one is the
//! order they happen in.
//!
//! 1. [`story`] reads the file — who is in it, what they say, how old they
//!    are, how they say it and where they stand.
//! 2. [`voice`] asks ElevenLabs to read each line in the voice cast for that
//!    character, and gets back raw samples.
//! 3. [`audio`] does to those samples what the line asked for: the pitch of
//!    the age, the tremble of the state, the place in the stereo picture. Then
//!    it lays the pieces end to end.
//!
//! # What it costs, and what it does not spend twice
//!
//! Every line is a paid request, so a run keeps what it was given. The raw
//! recording of each line is written beside the finished play, named after a
//! fingerprint of the three things ElevenLabs was told — the voice, the state
//! and the words — so a later run that finds all three unchanged uses the
//! recording it already has. Changing an age, a `pos`, or the age slider and
//! re-rendering therefore costs nothing at all: none of those change what was
//! said, only what is done to it afterwards.
//!
//! Naming a recording after what is in it, rather than after the line it
//! belonged to, is what makes that survive editing. A cache numbered by
//! position looks right until somebody adds a line at the top of a scene, at
//! which point every line below it has moved, matches nothing, and is paid for
//! a second time.
//!
//! # What it is not doing yet
//!
//! Sound effects. `model="1.2"` is read and its dialogue is spoken, but a
//! story that expects footsteps will not get them.

pub mod audio;
pub mod story;
pub mod voice;

pub use story::{Line, Pos, State, Story, Voice};

use crate::log;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// What can stop a render.
#[derive(Debug)]
pub enum Error {
    /// There is nothing in the story to say.
    Nothing,
    /// Somebody who speaks has no voice cast for them.
    Uncast(Vec<String>),
    /// ElevenLabs, or getting to it.
    Service(voice::Error),
    /// Writing the recording.
    Disk(String),
    /// The writer pressed stop.
    Stopped,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Nothing => write!(f, "there is no dialogue in this story"),
            Error::Uncast(who) => write!(f, "no voice has been chosen for {}", who.join(", ")),
            Error::Service(err) => write!(f, "{err}"),
            Error::Disk(what) => write!(f, "{what}"),
            Error::Stopped => write!(f, "stopped"),
        }
    }
}

impl std::error::Error for Error {}

impl From<voice::Error> for Error {
    fn from(err: voice::Error) -> Error {
        Error::Service(err)
    }
}

/// Everything a render needs that the story does not carry.
#[derive(Debug, Clone)]
pub struct Options {
    /// The ElevenLabs API key.
    pub key: String,
    /// How much of the age shift to apply, 0 to 1 and a little beyond.
    ///
    /// A writer who has already cast a child's voice for a child does not want
    /// the `age="12"` applied on top of it; this is how they say so, without
    /// having to edit the story file to lie about anybody's age.
    pub age_strength: f32,
    /// Where the finished play goes.
    pub out: PathBuf,
    /// Whether to keep the individual lines beside it.
    pub keep_lines: bool,
    /// Whether to use recordings from a previous run where nothing that
    /// reaches ElevenLabs has changed.
    pub reuse: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            key: String::new(),
            age_strength: 1.0,
            out: PathBuf::new(),
            keep_lines: true,
            reuse: true,
        }
    }
}

/// How a finished render turned out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub path: PathBuf,
    pub lines: usize,
    /// How many of them came from a previous run rather than from ElevenLabs.
    pub reused: usize,
    /// How long the play is, in whole seconds.
    pub seconds: u32,
    pub bytes: usize,
}

/// What a line is about to have done to it, worked out without spending
/// anything.
///
/// This is what the window lists, so that a writer can see that Ben is going
/// up four semitones and standing on the left before they pay to hear it.
#[derive(Debug, Clone)]
pub struct Planned {
    pub speaker: String,
    pub treatment: audio::Treatment,
    pub state: State,
    pub age: Option<u32>,
    /// Whether there is a voice cast for whoever speaks this.
    pub cast: bool,
}

/// Work out what will happen to every line.
pub fn plan(story: &Story, age_strength: f32) -> Vec<Planned> {
    story
        .lines
        .iter()
        .map(|line| Planned {
            speaker: story.speaker_of(line),
            treatment: audio::Treatment::for_line(line, age_strength),
            state: line.state,
            age: line.age,
            cast: story
                .voice_of(line)
                .is_some_and(|at| story.voices[at].is_ready()),
        })
        .collect()
}

/// Everybody who speaks but has no voice.
pub fn uncast(story: &Story) -> Vec<String> {
    let mut names: Vec<String> = story
        .lines
        .iter()
        .filter(|line| !story.voice_of(line).is_some_and(|at| story.voices[at].is_ready()))
        .map(|line| story.speaker_of(line))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Record the whole play.
///
/// `report` is called before each line with the line's number and how many
/// there are, so the window can show where it has got to; `stop` is checked
/// between lines, so pressing stop takes effect at the next join rather than
/// at the end.
///
/// This runs for as long as the play has lines in it and must not be called on
/// the thread drawing the window.
pub fn render(
    story: &Story,
    options: &Options,
    report: &dyn Fn(usize, usize, &str),
    stop: &AtomicBool,
) -> Result<Summary, Error> {
    if story.lines.is_empty() {
        return Err(Error::Nothing);
    }
    let missing = uncast(story);
    if !missing.is_empty() {
        return Err(Error::Uncast(missing));
    }

    let timer = log::Timer::start();
    let total = story.lines.len();
    let parts_dir = parts_dir(&options.out);
    let mut cache = Cache::open(&parts_dir.join("source"), options.reuse);

    let mut pieces = Vec::with_capacity(total);
    let mut reused = 0usize;

    for (index, line) in story.lines.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let speaker = story.speaker_of(line);
        report(index, total, &speaker);

        let at = story.voice_of(line).expect("a cast line");
        let voice_id = story.voices[at].voice_id.trim().to_string();
        let print = fingerprint(&voice_id, line);

        let raw = match cache.take(print) {
            Some(raw) => {
                reused += 1;
                raw
            }
            None => {
                let bytes = voice::speak(&options.key, &voice_id, &line.text, line.state)?;
                log::info(
                    "drama",
                    format!("line {} of {total}: {} bytes of audio", index + 1, bytes.len()),
                );
                let raw = audio::Mono::from_pcm16(&bytes);
                cache.put(print, &bytes)?;
                raw
            }
        };

        let treatment = audio::Treatment::for_line(line, options.age_strength);
        // The fingerprint doubles as the seed, so the same line trembles the
        // same way every time it is rendered.
        let (left, right) = treatment.apply(&raw, print);
        let piece = audio::Piece { left, right, speaker: speaker.clone() };

        if options.keep_lines {
            let name = format!("{:02}-{}.wav", index + 1, tidy(&speaker));
            let interleaved = audio::stitch(std::slice::from_ref(&piece));
            write(
                &parts_dir.join(name),
                &audio::wav(&interleaved, audio::SAMPLE_RATE, 2),
            )?;
        }
        pieces.push(piece);
    }

    if stop.load(Ordering::Relaxed) {
        return Err(Error::Stopped);
    }
    cache.close();

    let interleaved = audio::stitch(&pieces);
    let seconds = (interleaved.len() / 2) as f32 / audio::SAMPLE_RATE as f32;
    let bytes = audio::wav(&interleaved, audio::SAMPLE_RATE, 2);
    write(&options.out, &bytes)?;

    // Shapes and sizes, never a word of the dialogue.
    log::info(
        "drama",
        format!(
            "{total} lines, {reused} reused, {seconds:.1} s, {} bytes, {} ms",
            bytes.len(),
            timer.ms()
        ),
    );
    Ok(Summary {
        path: options.out.clone(),
        lines: total,
        reused,
        seconds: seconds.round() as u32,
        bytes: bytes.len(),
    })
}

/// Does this look like an audio drama rather than a screenplay?
///
/// By its extension, which is all there is to go on when Finder hands the
/// program a file somebody double-clicked. A `.xml` that turns out to be
/// something else opens as an empty story with a note saying so, which is a
/// better answer than reading a story file as Fountain and getting a page of
/// nonsense.
pub fn is_story(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
}

/// Where the individual lines go: a folder beside the play, named after it.
pub fn parts_dir(out: &Path) -> PathBuf {
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "audio-drama".to_string());
    out.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-lines"))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| Error::Disk(format!("{}: {err}", parent.display())))?;
    }
    std::fs::write(path, bytes).map_err(|err| Error::Disk(format!("{}: {err}", path.display())))
}

/// A file name made out of a character's name.
fn tidy(name: &str) -> String {
    let tidied: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let tidied = tidied.trim_matches('-').to_string();
    if tidied.is_empty() {
        "voice".to_string()
    } else {
        tidied.chars().take(40).collect()
    }
}

/// What decides whether a line has to be recorded again.
///
/// Public because it is the contract, not an implementation detail: it is what
/// says which changes cost money and which do not.
///
/// The three things ElevenLabs is told: which voice, which words, and the
/// settings the state asks for. An age, a `pos` and the age slider are all
/// absent on purpose — none of them reach ElevenLabs, so none of them are a
/// reason to pay for the line twice.
pub fn fingerprint(voice_id: &str, line: &Line) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(voice_id.as_bytes());
    eat(b"\x1f");
    eat(line.state.word().as_bytes());
    eat(b"\x1f");
    eat(line.text.trim().as_bytes());
    eat(b"\x1f");
    eat(voice::DEFAULT_MODEL.as_bytes());
    hash
}

/// The recordings a previous run left behind.
///
/// Each is named after its own fingerprint, so a recording is found by what is
/// in it rather than by where it sat in the story. Nothing here is an error
/// worth stopping for: a cache that cannot be read costs money, not work, and
/// the answer is to record the line again.
struct Cache {
    dir: PathBuf,
    on: bool,
    /// Everything this run wants, so that whatever is left over can be swept
    /// up at the end rather than accumulating a recording of every sentence
    /// the writer has ever tried.
    used: Vec<u64>,
}

impl Cache {
    fn open(dir: &Path, on: bool) -> Cache {
        Cache { dir: dir.to_path_buf(), on, used: Vec::new() }
    }

    fn path(&self, print: u64) -> PathBuf {
        self.dir.join(format!("{print:016x}.pcm"))
    }

    /// The recording for this fingerprint, if there is one.
    fn take(&mut self, print: u64) -> Option<audio::Mono> {
        if !self.on {
            return None;
        }
        let bytes = std::fs::read(self.path(print)).ok()?;
        if bytes.is_empty() {
            return None;
        }
        self.used.push(print);
        Some(audio::Mono::from_pcm16(&bytes))
    }

    fn put(&mut self, print: u64, bytes: &[u8]) -> Result<(), Error> {
        self.used.push(print);
        if self.on {
            write(&self.path(print), bytes)?;
        }
        Ok(())
    }

    /// Throw away recordings of lines that are not in the story any more.
    ///
    /// Only `.pcm` files, and only ones this run did not want: the folder is
    /// this program's own, but deleting is deleting, and a file that is not
    /// recognised is left where it is.
    fn close(&self) {
        if !self.on {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "pcm") {
                continue;
            }
            let print = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| u64::from_str_radix(stem, 16).ok());
            match print {
                Some(print) if self.used.contains(&print) => {}
                _ => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        // The index an older version kept beside these. There is nothing in it
        // that the file names do not now say.
        let _ = std::fs::remove_file(self.dir.join("recorded.txt"));
    }
}
