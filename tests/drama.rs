//! The audio drama, end to end, without spending anything.
//!
//! Every line of a play is a paid request, so a run keeps the raw recording of
//! each line and reuses it when nothing that reaches ElevenLabs has changed
//! (see [`openwrite::drama::fingerprint`]). That is what these tests stand on:
//! a cache is laid down by hand, and the whole pipeline — parse the story,
//! work out what each line asks for, treat the samples, lay the pieces end to
//! end, write the file — then runs against it with no network, no key and no
//! ElevenLabs account.
//!
//! What is deliberately not covered here is the request itself. That is one
//! `curl` invocation against somebody else's paid service; the parts of it
//! that can be checked without one — that a key with a newline in it is
//! refused, that a voice id cannot escape into another endpoint, that both
//! shapes of the voice list are understood — are unit tests in `drama::voice`.

#![cfg(feature = "drama")]

use openwrite::drama::{self, audio, story, Story};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const VOICE: &str = "21m00Tcm4TlvDq8ikWAM";

/// What the first line says about itself when nothing in particular is being
/// tested. Written out at each call rather than appended to, because an
/// attribute given twice in one tag is not a thing to build a test on.
const PLAIN: &str = r#"state="normal" pos="left""#;

/// Two lines, cast, with `first` as the whole of the opening line's
/// attributes.
fn story_of(first: &str) -> Story {
    story::parse(&format!(
        r#"<story model="1.0">
             <voices>
               <character id="1" name="ben" gender="male">{VOICE}</character>
               <character id="2" name="faith" gender="female">{VOICE}</character>
             </voices>
             <dialog>
               <character line="1" id="1" {first}>I don't remember how it happened</character>
               <character line="2" id="2" state="normal" pos="right">What?</character>
             </dialog>
           </story>"#
    ))
}

/// Half a second of something, as ElevenLabs would send it.
fn pcm(seconds: f32, tone: f32) -> Vec<u8> {
    let n = (seconds * audio::SAMPLE_RATE as f32) as usize;
    (0..n)
        .flat_map(|i| {
            let t = i as f32 / audio::SAMPLE_RATE as f32;
            let value = ((std::f32::consts::TAU * tone * t).sin() * 12_000.0) as i16;
            value.to_le_bytes()
        })
        .collect()
}

/// A folder of this test's own, taken away afterwards.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("openwrite-drama-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a working folder");
        Dir(dir)
    }

    fn out(&self) -> PathBuf {
        self.0.join("play.wav")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Lay down the recordings a previous run would have left, so that this one
/// needs nothing but the disk.
fn prime(out: &Path, story: &Story, lengths: &[f32]) {
    let source = drama::parts_dir(out).join("source");
    std::fs::create_dir_all(&source).expect("a cache folder");
    for (at, line) in story.lines.iter().enumerate() {
        let name = format!("{:016x}.pcm", drama::fingerprint(VOICE, line));
        std::fs::write(source.join(name), pcm(lengths[at], 220.0)).expect("a recording");
    }
}

fn options(out: PathBuf) -> drama::Options {
    drama::Options { key: "sk_notused".to_string(), out, ..drama::Options::default() }
}

fn render(story: &Story, options: &drama::Options) -> Result<drama::Summary, drama::Error> {
    drama::render(story, options, &|_, _, _| {}, &AtomicBool::new(false))
}

/// Read a `.wav` back: how many frames it holds, and its peak.
fn read_wav(path: &Path) -> (usize, u16, u32, f32) {
    let bytes = std::fs::read(path).expect("the recording");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let channels = u16::from_le_bytes(bytes[22..24].try_into().unwrap());
    let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    assert_eq!(data, bytes.len() - 44, "the header disagrees with the file");
    let peak = bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (i16::from_le_bytes(*pair) as f32 / 32_768.0).abs())
        .fold(0.0f32, f32::max);
    (data / 2 / channels as usize, channels, rate, peak)
}

#[test]
fn a_whole_play_is_recorded_stitched_and_written() {
    let dir = Dir::new("whole");
    let story = story_of(PLAIN);
    prime(&dir.out(), &story, &[1.0, 0.5]);

    let summary = render(&story, &options(dir.out())).expect("a recording");
    assert_eq!(summary.lines, 2);
    assert_eq!(summary.reused, 2, "both lines came from the cache, so nothing was sent");

    let (frames, channels, rate, peak) = read_wav(&dir.out());
    assert_eq!(channels, 2, "a radio play is in stereo or the pos does nothing");
    assert_eq!(rate, audio::SAMPLE_RATE);
    assert!(peak > 0.1, "the recording is silent");

    // The two lines, plus the pause between two different speakers.
    let seconds = frames as f32 / rate as f32;
    assert!((seconds - 1.88).abs() < 0.05, "{seconds:.2}s is not the two lines and a pause");
    assert_eq!(summary.seconds, 2);
}

#[test]
fn each_line_is_kept_beside_the_play() {
    let dir = Dir::new("lines");
    let story = story_of(PLAIN);
    prime(&dir.out(), &story, &[1.0, 0.5]);
    render(&story, &options(dir.out())).expect("a recording");

    let parts = drama::parts_dir(&dir.out());
    // Named so that they sort into the order they are spoken, and after
    // whoever speaks them.
    assert!(parts.join("01-ben.wav").is_file());
    assert!(parts.join("02-faith.wav").is_file());
    let (_, channels, _, _) = read_wav(&parts.join("01-ben.wav"));
    assert_eq!(channels, 2);

    // And not, when it is not asked for.
    let dir = Dir::new("nolines");
    prime(&dir.out(), &story, &[1.0, 0.5]);
    let options = drama::Options { keep_lines: false, ..options(dir.out()) };
    render(&story, &options).expect("a recording");
    assert!(!drama::parts_dir(&dir.out()).join("01-ben.wav").exists());
}

/// The `pos` on a line has to survive all the way to the file, or the whole
/// stereo picture is decoration.
#[test]
fn a_line_panned_left_comes_out_of_the_left() {
    let dir = Dir::new("pan");
    let story = story_of(PLAIN);
    prime(&dir.out(), &story, &[1.0, 0.5]);
    render(&story, &options(dir.out())).expect("a recording");

    let bytes = std::fs::read(drama::parts_dir(&dir.out()).join("01-ben.wav")).unwrap();
    let (mut left, mut right) = (0.0f64, 0.0f64);
    for frame in bytes[44..].as_chunks::<4>().0 {
        let l = i16::from_le_bytes([frame[0], frame[1]]) as f64;
        let r = i16::from_le_bytes([frame[2], frame[3]]) as f64;
        left += l * l;
        right += r * r;
    }
    assert!(left > right * 5.0, "pos=\"left\" did not reach the left channel");
    assert!(right > 0.0, "a voice hard against one ear sounds like a fault");
}

/// The contract the whole cache rests on: the three things ElevenLabs is told
/// decide whether a line is paid for again, and nothing else does.
#[test]
fn only_what_reaches_elevenlabs_makes_a_line_worth_recording_again() {
    let base = story_of(PLAIN);
    let line = &base.lines[0];
    let print = drama::fingerprint(VOICE, line);

    // An age is arithmetic done here afterwards. So is a position.
    let aged = story_of(r#"age="12" state="normal" pos="left""#);
    assert_eq!(drama::fingerprint(VOICE, &aged.lines[0]), print, "an age is not a new recording");
    let mut moved = line.clone();
    moved.pos = story::Pos::Right;
    assert_eq!(drama::fingerprint(VOICE, &moved), print, "a position is not a new recording");
    let mut renumbered = line.clone();
    renumbered.number = Some(99);
    assert_eq!(drama::fingerprint(VOICE, &renumbered), print);

    // The words, the voice and the state all go to ElevenLabs.
    let mut reworded = line.clone();
    reworded.text = "Something else entirely".to_string();
    assert_ne!(drama::fingerprint(VOICE, &reworded), print);
    let mut restated = line.clone();
    restated.state = story::State::Scared;
    assert_ne!(drama::fingerprint(VOICE, &restated), print);
    assert_ne!(drama::fingerprint("another-voice", line), print);
}

/// And the same thing through the whole pipeline: changing an age re-renders
/// for nothing, and changes what comes out.
#[test]
fn changing_an_age_costs_nothing_and_still_changes_the_sound() {
    let dir = Dir::new("age");
    let plain = story_of(PLAIN);
    prime(&dir.out(), &plain, &[1.0, 0.5]);
    render(&plain, &options(dir.out())).expect("a recording");
    let before = std::fs::read(dir.out()).unwrap();

    // The same cache, a twelve-year-old on the first line.
    let child = story_of(r#"age="12" state="normal" pos="left""#);
    let summary = render(&child, &options(dir.out())).expect("a recording");
    assert_eq!(summary.reused, 2, "an age should not cost a single request");

    let after = std::fs::read(dir.out()).unwrap();
    assert_ne!(before, after, "the age made no audible difference");

    // The pitch moved, and the play is still the length it was. Not to the
    // byte: the stretching that keeps the duration works in frames of thirty
    // milliseconds and lands within a few percent, which is the point of
    // measuring it rather than assuming it.
    let drift = after.len() as f32 / before.len() as f32 - 1.0;
    assert!(
        drift.abs() < 0.03,
        "lifting a line to a twelve-year-old changed the play's length by {:.1}%",
        drift * 100.0
    );
}

/// The slider that exists because a child's voice cast for a child should not
/// have the child applied twice.
#[test]
fn the_age_shift_can_be_turned_off_without_editing_the_story() {
    let dir = Dir::new("strength");
    let child = story_of(r#"age="12" state="normal" pos="left""#);
    prime(&dir.out(), &child, &[1.0, 0.5]);

    render(&child, &options(dir.out())).expect("a recording");
    let full = std::fs::read(dir.out()).unwrap();

    let none = drama::Options { age_strength: 0.0, ..options(dir.out()) };
    render(&child, &none).expect("a recording");
    let off = std::fs::read(dir.out()).unwrap();
    assert_ne!(full, off, "the age shift was applied either way");
}

#[test]
fn a_play_nobody_is_cast_in_is_refused_before_anything_is_sent() {
    let dir = Dir::new("uncast");
    let story = story::parse(
        r#"<story><dialog><character name="ben">Hello</character></dialog></story>"#,
    );
    match render(&story, &options(dir.out())) {
        Err(drama::Error::Uncast(who)) => assert_eq!(who, ["ben"]),
        other => panic!("expected an uncast error, got {other:?}"),
    }
    assert!(!dir.out().exists(), "nothing should have been written");

    // And a story with no dialogue at all.
    let empty = story::parse(r#"<story><dialog/></story>"#);
    assert!(matches!(render(&empty, &options(dir.out())), Err(drama::Error::Nothing)));
}

#[test]
fn stopping_takes_effect_and_writes_nothing() {
    let dir = Dir::new("stop");
    let story = story_of(PLAIN);
    prime(&dir.out(), &story, &[1.0, 0.5]);

    let stop = AtomicBool::new(true);
    let outcome = drama::render(&story, &options(dir.out()), &|_, _, _| {}, &stop);
    assert!(matches!(outcome, Err(drama::Error::Stopped)));
    assert!(!dir.out().exists());
}

/// The window shows this before anything is spent, so it has to agree with
/// what the render will actually do.
#[test]
fn the_plan_says_what_will_happen_before_anything_is_sent() {
    let story = story_of(r#"age="12" state="scared" pos="left""#);
    let planned = drama::plan(&story, 1.0);
    assert_eq!(planned.len(), 2);

    assert_eq!(planned[0].speaker, "ben");
    assert_eq!(planned[0].age, Some(12));
    assert!(planned[0].cast);
    assert!(planned[0].treatment.semitones > 3.0, "twelve should lift the pitch");
    assert!(planned[0].treatment.wobble > 0.0, "fright should tremble");
    assert_eq!(planned[0].treatment.pos, story::Pos::Left);

    // The second line asks for nothing, and is told so.
    assert!(planned[1].treatment.is_plain());
    assert_eq!(planned[1].treatment.pos, story::Pos::Right);

    // And the plan agrees with the render about who has a voice.
    assert!(drama::uncast(&story).is_empty());
}

/// Progress is what the window puts in front of somebody waiting minutes.
#[test]
fn every_line_is_reported_as_it_is_reached() {
    let dir = Dir::new("progress");
    let story = story_of(PLAIN);
    prime(&dir.out(), &story, &[1.0, 0.5]);

    let seen = std::sync::Mutex::new(Vec::new());
    let report = |at: usize, total: usize, who: &str| {
        seen.lock().unwrap().push((at, total, who.to_string()));
    };
    drama::render(&story, &options(dir.out()), &report, &AtomicBool::new(false))
        .expect("a recording");

    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], (0, 2, "ben".to_string()));
    assert_eq!(seen[1], (1, 2, "faith".to_string()));
}

/// A story that arrives without a cast list is the case the tab exists for:
/// it should come back with one, ready to be filled in and saved.
#[test]
fn a_story_with_no_cast_list_comes_back_with_one_to_fill_in() {
    let mut story = story::parse(
        r#"<story model="1.2">
             <dialog>
               <character name="ben" age="12" state="normal">I don't remember how it happened</character>
               <character name="faith" age="15" state="whisper">What?</character>
             </dialog>
           </story>"#,
    );
    assert_eq!(drama::uncast(&story), ["ben", "faith"]);
    assert_eq!(story.voices.len(), 2);

    // Cast it, as the tab does, and write it back.
    for voice in &mut story.voices {
        voice.voice_id = VOICE.to_string();
    }
    assert!(drama::uncast(&story).is_empty());

    let reopened = story::parse(&story.to_xml());
    assert!(reopened.voices.iter().all(|voice| voice.voice_id == VOICE));
    assert_eq!(reopened.lines.len(), 2);
    assert_eq!(reopened.lines[1].state, story::State::Whisper);
}

/// A story with `first` inserted ahead of the two lines `story_of` makes, so
/// that everything below it has moved.
fn grown_story() -> Story {
    story::parse(&format!(
        r#"<story model="1.0">
             <voices>
               <character id="1" name="ben" gender="male">{VOICE}</character>
               <character id="2" name="faith" gender="female">{VOICE}</character>
             </voices>
             <dialog>
               <character line="1" id="2" state="normal" pos="centre">Ben. Ben, look at me.</character>
               <character line="2" id="1" state="normal" pos="left">I don't remember how it happened</character>
               <character line="3" id="2" state="normal" pos="right">What?</character>
             </dialog>
           </story>"#
    ))
}

/// Every recording currently held for a play.
fn cached(out: &Path) -> Vec<String> {
    let source = drama::parts_dir(out).join("source");
    let mut names: Vec<String> = std::fs::read_dir(source)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The reason a recording is named after what is in it rather than after the
/// line it belonged to. A cache numbered by position looks right until
/// somebody adds a line at the top of a scene — and then bills them for every
/// line underneath it.
#[test]
fn adding_a_line_does_not_make_the_lines_below_it_worth_paying_for_again() {
    let dir = Dir::new("insert");
    let first = story_of(PLAIN);
    prime(&dir.out(), &first, &[1.0, 0.5]);
    render(&first, &options(dir.out())).expect("a recording");

    // Lay down a recording for the new opening line, and nothing else.
    let grown = grown_story();
    let source = drama::parts_dir(&dir.out()).join("source");
    let name = format!("{:016x}.pcm", drama::fingerprint(VOICE, &grown.lines[0]));
    std::fs::write(source.join(name), pcm(0.6, 220.0)).expect("a recording");

    let summary = render(&grown, &options(dir.out())).expect("a recording");
    assert_eq!(summary.lines, 3);
    assert_eq!(
        summary.reused, 3,
        "the two lines that only moved should not have been recorded again"
    );
}

/// And the other side of naming them that way: a line that has been rewritten
/// leaves a recording nothing wants, which should not be kept for ever.
#[test]
fn a_recording_no_line_asks_for_any_more_is_swept_up() {
    let dir = Dir::new("sweep");
    let grown = grown_story();
    prime(&dir.out(), &grown, &[0.6, 1.0, 0.5]);
    render(&grown, &options(dir.out())).expect("a recording");
    assert_eq!(cached(&dir.out()).len(), 3);

    // Back to the two-line version: the opening line's recording is orphaned.
    let first = story_of(PLAIN);
    render(&first, &options(dir.out())).expect("a recording");
    let left = cached(&dir.out());
    assert_eq!(left.len(), 2, "the orphaned recording is still there: {left:?}");
    for line in &first.lines {
        let name = format!("{:016x}.pcm", drama::fingerprint(VOICE, line));
        assert!(left.contains(&name), "{name} was swept up by mistake");
    }

    // Turning reuse off sweeps nothing, because it wrote nothing.
    let kept = drama::Options { reuse: false, ..options(dir.out()) };
    render(&grown, &kept).expect_err("no cache and no key");
    assert_eq!(cached(&dir.out()).len(), 2);
}
