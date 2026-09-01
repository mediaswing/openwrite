//! The draft as it stood a moment ago, in case the power goes.
//!
//! This is not auto-save, and the difference is the whole design. Auto-save
//! writes over the writer's file on a timer, and this program will not: saving
//! as `.fountain` drops the story bible (see [`crate::document::write_for`]),
//! "close without saving" is a real thing to want after a bad hour, and an
//! untitled draft — the one most likely to be lost — has no file to be written
//! over anyway.
//!
//! So instead a copy goes somewhere of its own, beside the settings, and the
//! writer's file is never touched. What is written is always a full `.sct`
//! document whatever the real file is, so the bible, the caret and the selected
//! scene all survive a crash even for a draft being kept as plain Fountain.
//! Where it came from is recorded in the header with
//! [`crate::document::with_header`], so a restored draft knows which file it
//! belongs to.
//!
//! # It is cleared, not accumulated
//!
//! A copy is written while there is unsaved work and removed the moment there
//! is not: on every successful save, and whenever the document is put away. So
//! finding one at startup means the editor did not get the chance to remove it,
//! which is exactly the case this exists for. One copy per document, named from
//! the path it belongs to, so two editors open on two screenplays do not write
//! over each other's.
//!
//! # It is the writer's work
//!
//! An unpublished screenplay sitting in a configuration folder is worth the
//! same care as the API key next to it: written owner-only where the platform
//! has permissions, and written to a temporary name and renamed into place, so
//! that a crash during the write cannot leave half a draft where a whole one
//! was.

use crate::document::{self, Document};
use std::path::{Path, PathBuf};

/// What a recovered draft turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// The file it was being written to, if it had one yet.
    pub origin: Option<PathBuf>,
    pub document: Document,
    /// Where the copy itself is, so it can be removed once it has been used.
    pub at: PathBuf,
}

impl Recovered {
    /// What to call it in a sentence: the file's name, or nothing if it never
    /// had one.
    pub fn name(&self) -> Option<String> {
        self.origin
            .as_ref()?
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }
}

/// The header key holding the path the copy belongs to.
const ORIGIN: &str = "origin";

/// Where the copies live.
pub fn dir() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("recovery"))
}

/// The file a given document's copy goes in.
///
/// Named from the path rather than from the clock, so that saving twice
/// replaces the copy instead of leaving a trail of them. An untitled draft has
/// no path to be named from and gets one name of its own — there is only ever
/// one untitled draft in an editor that holds one screenplay at a time.
fn file_in(base: &Path, origin: Option<&Path>) -> PathBuf {
    let name = match origin {
        Some(path) => format!("{:016x}.sct", hash(&path.to_string_lossy())),
        None => "untitled.sct".to_string(),
    };
    base.join(name)
}

/// FNV-1a, written out because the answer has to be the same next week.
///
/// The standard library's hasher is seeded from the operating system and gives
/// a different answer every run, which would orphan every copy at every start.
fn hash(text: &str) -> u64 {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x0000_0100_0000_01b3);
    }
    value
}

/// Put a copy of the draft where it can be found again.
pub fn keep(origin: Option<&Path>, document: &Document) -> std::io::Result<PathBuf> {
    let base = dir().ok_or_else(|| std::io::Error::other("there is nowhere to keep a copy"))?;
    keep_in(&base, origin, document)
}

/// The whole of [`keep`], with the folder named rather than looked up, so that
/// a test can be given one of its own instead of the writer's.
fn keep_in(base: &Path, origin: Option<&Path>, document: &Document) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(base)?;
    let path = file_in(base, origin);

    let mut text = document::write(document);
    if let Some(origin) = origin {
        text = document::with_header(&text, ORIGIN, &origin.to_string_lossy());
    }

    // Written beside the real name and renamed onto it. A rename within one
    // folder is atomic on every platform this runs on, so a crash leaves either
    // the copy from last time or the copy from this time, and never half of
    // one.
    let temporary = path.with_extension("writing");
    write_private(&temporary, text.as_bytes())?;
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

/// Take away the copy of a document that no longer needs one.
///
/// Quiet about a copy that was not there: that is the ordinary case every time
/// a document with nothing unsaved in it is saved again.
pub fn discard(origin: Option<&Path>) {
    if let Some(base) = dir() {
        discard_in(&base, origin);
    }
}

fn discard_in(base: &Path, origin: Option<&Path>) {
    let path = file_in(base, origin);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("writing"));
}

/// The most recent copy that is worth offering back.
///
/// A copy whose text the file it came from already holds is not worth offering:
/// it means the draft was saved after the copy was made and the editor simply
/// did not get to tidy up. Those are removed here rather than shown, so the
/// only thing a writer is ever asked about is work that would otherwise be
/// gone.
pub fn newest() -> Option<Recovered> {
    newest_in(&dir()?)
}

fn newest_in(base: &Path) -> Option<Recovered> {
    let entries = std::fs::read_dir(base).ok()?;
    let mut best: Option<(std::time::SystemTime, Recovered)> = None;

    for entry in entries.flatten() {
        let at = entry.path();
        if at.extension().is_none_or(|ext| ext != "sct") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&at) else {
            continue;
        };
        let origin = document::header_value(&text, ORIGIN).map(PathBuf::from);
        let document = document::read(&text);

        if already_saved(origin.as_deref(), &document) {
            let _ = std::fs::remove_file(&at);
            continue;
        }

        let when = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let recovered = Recovered { origin, document, at };
        if best.as_ref().is_none_or(|(best, _)| when > *best) {
            best = Some((when, recovered));
        }
    }
    best.map(|(_, recovered)| recovered)
}

/// Does the file this copy came from already have this text in it?
///
/// Only the screenplay is compared, not the caret or the outline: where the
/// writer's cursor was is not work, and being asked to recover a draft because
/// the cursor moved would teach anybody to dismiss the question unread.
///
/// Both forms of the source count as a match. A draft saved as `.fountain` has
/// the one-line dialogue shorthand written out in full on the way to disk — see
/// [`crate::document::write_for`] — so the file holds `MAYA\nForty-one.` where
/// the editor holds `MAYA: Forty-one.`, and comparing only the text as typed
/// would call every such file unsaved.
fn already_saved(origin: Option<&Path>, document: &Document) -> bool {
    let Some(origin) = origin else {
        // An untitled draft has no file to be compared against, so a copy of
        // one is always worth offering.
        return false;
    };
    let Ok(text) = std::fs::read_to_string(origin) else {
        // The file is gone, or unreadable. Then the copy is all there is.
        return false;
    };
    let theirs = document::read(&text).source;
    let theirs = theirs.trim_end();
    theirs == document.source.trim_end()
        || theirs == crate::shorthand::expand(&document.source).trim_end()
}

/// Write a file only its owner can read. The same care the API key gets in
/// [`crate::settings`], and for the same reason: this is the writer's work.
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // As in `settings`: the mode above lands only on a file this call creates,
    // and the temporary name may well be one left behind by a write that was
    // interrupted.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recovery folder of its own for each test, and a separate folder for
    /// the screenplays it refers to — kept apart because that is how they are
    /// in life, and because a scan of the recovery folder would otherwise find
    /// the very files it is meant to be shadowing.
    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("openwrite-recovery-test-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let (copies, drafts) = (root.join("recovery"), root.join("drafts"));
        std::fs::create_dir_all(&copies).unwrap();
        std::fs::create_dir_all(&drafts).unwrap();
        (copies, drafts)
    }

    fn draft(source: &str) -> Document {
        let mut document = Document::new(source);
        document.working.caret = Some(7);
        document.bible.world = "A salt lake.".to_string();
        document
    }

    /// Set a file's modification time, so "the most recent" can be tested
    /// without sleeping and hoping.
    fn touch(path: &Path, seconds_ago: u64) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn a_kept_draft_comes_back_whole_and_knows_where_it_belongs() {
        let (base, drafts) = scratch("whole");
        let origin = drafts.join("The Last Bus.sct");
        // The file on disk is a version behind what was being typed.
        std::fs::write(&origin, crate::document::write(&draft("INT. A - DAY\n"))).unwrap();
        let working = draft("INT. A - DAY\n\nShe waits.\n");

        keep_in(&base, Some(&origin), &working).unwrap();
        let found = newest_in(&base).unwrap();

        assert_eq!(found.origin.as_deref(), Some(origin.as_path()));
        assert_eq!(found.name().as_deref(), Some("The Last Bus.sct"));
        // The bible and the caret come back too: the copy is a full document
        // whatever the real file's extension is.
        assert_eq!(found.document, working);
        assert_eq!(found.document.bible.world, "A salt lake.");
    }

    /// The case that most needs this: a draft that has never been saved has no
    /// file to be auto-saved over, and is offered back on its own.
    #[test]
    fn a_draft_that_was_never_saved_is_kept_too() {
        let (base, _drafts) = scratch("untitled");
        let working = draft("INT. NOWHERE - DAY\n");
        keep_in(&base, None, &working).unwrap();

        let found = newest_in(&base).unwrap();
        assert_eq!(found.origin, None);
        assert_eq!(found.name(), None);
        assert_eq!(found.document, working);
    }

    /// Finding a copy has to mean work was lost. A copy the real file has
    /// already caught up with is tidied away rather than offered, or the
    /// question becomes one people dismiss unread.
    #[test]
    fn a_copy_the_file_has_caught_up_with_is_taken_away_rather_than_offered() {
        let (base, drafts) = scratch("caught-up");
        let origin = drafts.join("Saved.sct");
        let working = draft("INT. A - DAY\n\nShe waits.\n");
        std::fs::write(&origin, crate::document::write(&working)).unwrap();

        let copy = keep_in(&base, Some(&origin), &working).unwrap();
        assert!(newest_in(&base).is_none(), "an up-to-date copy was offered back");
        assert!(!copy.exists(), "and it was not tidied away either");
    }

    /// Where the cursor was is not work. Being asked to recover a draft
    /// because the caret moved would train anybody to say no.
    #[test]
    fn a_caret_that_moved_is_not_treated_as_unsaved_work() {
        let (base, drafts) = scratch("caret");
        let origin = drafts.join("Same.sct");
        let mut saved = draft("INT. A - DAY\n");
        saved.working.caret = Some(1);
        std::fs::write(&origin, crate::document::write(&saved)).unwrap();

        let mut moved = saved.clone();
        moved.working.caret = Some(99);
        keep_in(&base, Some(&origin), &moved).unwrap();
        assert!(newest_in(&base).is_none());
    }

    #[test]
    fn the_most_recent_copy_is_the_one_offered() {
        let (base, drafts) = scratch("newest");
        let old = drafts.join("Old.sct");
        let new = drafts.join("New.sct");

        let old_copy = keep_in(&base, Some(&old), &draft("INT. OLD - DAY\n")).unwrap();
        let new_copy = keep_in(&base, Some(&new), &draft("INT. NEW - DAY\n")).unwrap();
        touch(&old_copy, 600);
        touch(&new_copy, 10);

        assert_eq!(newest_in(&base).unwrap().origin.as_deref(), Some(new.as_path()));
    }

    #[test]
    fn two_screenplays_do_not_write_over_each_others_copies() {
        let (base, drafts) = scratch("two");
        let one = drafts.join("One.sct");
        let two = drafts.join("Two.sct");
        assert_ne!(file_in(&base, Some(&one)), file_in(&base, Some(&two)));
        assert_ne!(file_in(&base, Some(&one)), file_in(&base, None));
        // And the same screenplay always lands on the same name, so saving
        // twice replaces the copy rather than leaving a trail of them.
        assert_eq!(file_in(&base, Some(&one)), file_in(&base, Some(&one)));
    }

    #[test]
    fn a_copy_is_taken_away_when_it_is_no_longer_needed() {
        let (base, drafts) = scratch("discard");
        let origin = drafts.join("Done.sct");
        let copy = keep_in(&base, Some(&origin), &draft("INT. A - DAY\n")).unwrap();
        assert!(copy.exists());

        discard_in(&base, Some(&origin));
        assert!(!copy.exists());
        // And asking twice is not an error: that is the ordinary case every
        // time a document with nothing unsaved in it is saved again.
        discard_in(&base, Some(&origin));
    }

    #[test]
    fn nothing_is_left_behind_by_a_write_that_finished() {
        let (base, drafts) = scratch("tidy");
        let origin = drafts.join("Tidy.sct");
        keep_in(&base, Some(&origin), &draft("INT. A - DAY\n")).unwrap();

        let strays: Vec<_> = std::fs::read_dir(&base)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "writing"))
            .collect();
        assert!(strays.is_empty(), "a half-written copy was left: {strays:?}");
    }

    /// It is an unpublished screenplay in a configuration folder, and deserves
    /// what the API key beside it gets.
    #[test]
    #[cfg(unix)]
    fn a_copy_is_readable_only_by_the_writer() {
        use std::os::unix::fs::PermissionsExt;
        let (base, _drafts) = scratch("private");
        let copy = keep_in(&base, None, &draft("INT. PRIVATE - DAY\n")).unwrap();
        let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{copy:?} is readable by somebody else");
    }

    /// The whole reason a copy is a full `.sct` document whatever the real file
    /// is: saving as Fountain has nowhere to put the story bible, so a crash
    /// while working that way would otherwise lose the notes and keep the text.
    #[test]
    fn a_draft_kept_as_plain_fountain_still_has_its_bible_in_the_copy() {
        let (base, drafts) = scratch("fountain");
        let origin = drafts.join("The Last Bus.fountain");
        let mut working = draft("INT. A - DAY\n\nMAYA: Forty-one.\n");
        working.bible.profiles.push(crate::bible::Profile {
            name: "MAYA".to_string(),
            role: "Salt-runner.".to_string(),
            ..Default::default()
        });
        // As the editor would have written it: the bible is not in there.
        std::fs::write(&origin, crate::document::write_for(&origin, &working)).unwrap();

        keep_in(&base, Some(&origin), &working).unwrap();
        // Nothing was lost between the file and the copy, so nothing is
        // offered -- but the copy that was written held everything.
        let copy = file_in(&base, Some(&origin));
        let kept = crate::document::read(&std::fs::read_to_string(&copy).unwrap());
        assert_eq!(kept.bible.profiles.len(), 1);
        assert_eq!(kept.bible.profiles[0].role, "Salt-runner.");
        assert_eq!(kept.working.caret, Some(7));
    }

    /// Saving as Fountain writes the one-line shorthand out in full, so the
    /// file never matches the text as typed. Comparing only that form would
    /// call every Fountain draft unsaved and ask about it at every start.
    #[test]
    fn a_fountain_file_is_recognised_as_holding_the_work_it_holds() {
        let (base, drafts) = scratch("shorthand");
        let origin = drafts.join("Shorthand.fountain");
        let working = draft("INT. A - DAY\n\nMAYA: Forty-one.\n");
        std::fs::write(&origin, crate::document::write_for(&origin, &working)).unwrap();

        keep_in(&base, Some(&origin), &working).unwrap();
        assert!(newest_in(&base).is_none(), "a saved Fountain draft was called unsaved");

        // And real unsaved work in one is still offered.
        let more = draft("INT. A - DAY\n\nMAYA: Forty-one.\n\nShe leaves.\n");
        keep_in(&base, Some(&origin), &more).unwrap();
        assert_eq!(newest_in(&base).unwrap().origin.as_deref(), Some(origin.as_path()));
    }

    /// The hash names the file, so it has to give the same answer next week as
    /// it did today or every copy is orphaned at every start.
    #[test]
    fn the_name_a_path_is_given_does_not_change_between_runs() {
        assert_eq!(hash("/home/a/The Last Bus.sct"), 0xf447_af07_df8f_2ee1);
        assert_ne!(hash("/home/a/One.sct"), hash("/home/a/Two.sct"));
    }
}
