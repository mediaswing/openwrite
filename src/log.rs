//! The debug log: what the program did, never what the writer wrote.
//!
//! A screenplay tool that repaginates on every keystroke, opens sockets and
//! runs worker threads has more going on inside it than the status bar can say.
//! When something is slow, or a model server will not answer, or a draft opens
//! wrong, the question is always "what actually happened just now" — and the
//! status bar only ever shows the last line of it.
//!
//! So there is a log. It is a ring buffer in memory, always running, holding
//! the last [`CAPACITY`] entries; <kbd>⇧⌘L</kbd> shows it, and it can be copied
//! or saved from there. Setting `OPENWRITE_LOG` to a path also writes every
//! entry to that file as it happens, which is the only way to see the entries
//! from a run that ended badly.
//!
//! # The one rule
//!
//! **The log records what the program did, never what the writer wrote.**
//!
//! Screenplay text, character names, world notes, prompts sent to a model and
//! the answers that come back are all the writer's work and none of them belong
//! in a file they might send to somebody to look at. What goes in is counts,
//! durations, sizes, formats, addresses and error text. "2,048 characters of
//! prompt, 3 characters of answer, 1,204 ms" says everything needed to debug a
//! model that is not answering, and gives away nothing.
//!
//! There is a test in `tests/ai.rs` that asks a stub model server a question
//! with a distinctive phrase in it and then asserts the phrase is nowhere in
//! the log.
//!
//! Nothing here is a Cargo feature and nothing here allocates a thread: it is a
//! `Mutex` around a `VecDeque`, which is affordable on any path that was
//! already going to allocate a message.

use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Set this to a path to write the log to a file as well as to memory.
pub const PATH_ENV: &str = "OPENWRITE_LOG";

/// How many entries are kept in memory. Long enough to cover a session's worth
/// of interesting events, short enough that it is never worth thinking about.
pub const CAPACITY: usize = 2_000;

/// How much an entry matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Routine, and there will be a lot of it: a reparse, a repagination.
    Debug,
    /// Something the writer asked for happened: a file opened, a model answered.
    Info,
    /// It worked, but not the way it should have.
    Warn,
    /// It did not work.
    Error,
}

impl Level {
    /// Fixed width, so the column lines up in a plain-text dump.
    fn tag(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag().trim_end())
    }
}

/// One thing that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Milliseconds since the program started. Relative rather than wall clock,
    /// because what a debug log is read for is how long things took and in
    /// what order.
    pub at_ms: u128,
    pub level: Level,
    /// Which part of the program: `open`, `reparse`, `ai`, `export`.
    pub area: &'static str,
    pub message: String,
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:>9} ms  {}  {:<10}  {}", self.at_ms, self.level.tag(), self.area, self.message)
    }
}

struct Log {
    entries: VecDeque<Entry>,
    /// How many entries have ever been written. The ring's length stops
    /// changing once it is full, so this is what a reader watches to know
    /// whether there is anything new to show.
    written: u64,
    /// Where the clock started, and what the wall clock said at the time.
    started: Option<Instant>,
    wall_clock: u64,
    /// A file that gets every entry as it happens, if one was asked for.
    sink: Option<(PathBuf, File)>,
    /// Set if the file could not be opened, so the window can say so.
    sink_error: Option<String>,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    entries: VecDeque::new(),
    written: 0,
    started: None,
    wall_clock: 0,
    sink: None,
    sink_error: None,
});

/// Take the lock, surviving a thread that panicked while holding it.
///
/// A poisoned log is still a perfectly good log, and losing the program over
/// one would be a poor trade for a diagnostic.
fn lock() -> std::sync::MutexGuard<'static, Log> {
    LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start the clock, and open the file sink if `OPENWRITE_LOG` names one.
///
/// Safe to call more than once; only the first call starts the clock.
pub fn start() {
    let mut log = lock();
    if log.started.is_none() {
        log.started = Some(Instant::now());
        log.wall_clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
    let Some(path) = std::env::var_os(PATH_ENV).filter(|p| !p.is_empty()) else {
        return;
    };
    let path = PathBuf::from(path);
    match File::create(&path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", header(&log));
            let _ = file.flush();
            log.sink = Some((path, file));
        }
        // Not being able to write a debug log is not a reason to fail to start.
        Err(err) => log.sink_error = Some(format!("could not open {}: {err}", path.display())),
    }
}

fn header(log: &Log) -> String {
    format!(
        "openwrite {} debug log\nstarted at unix {}; times below are milliseconds since then\n\
         this log records what the program did, not what was written",
        env!("CARGO_PKG_VERSION"),
        log.wall_clock
    )
}

/// Record an entry.
///
/// `area` is a short fixed word — `open`, `reparse`, `ai` — so that a reader can
/// grep for one part of the program. `message` must contain no screenplay: see
/// the rule at the top of this module.
pub fn write(level: Level, area: &'static str, message: impl Into<String>) {
    let mut log = lock();
    let at_ms = log.started.map(|start| start.elapsed().as_millis()).unwrap_or(0);
    let entry = Entry { at_ms, level, area, message: message.into() };

    if let Some((_, file)) = log.sink.as_mut() {
        // Flushed every time: an entry that is still in a buffer when the
        // program stops is the one entry that was worth having.
        let _ = writeln!(file, "{entry}");
        let _ = file.flush();
    }

    if log.entries.len() == CAPACITY {
        log.entries.pop_front();
    }
    log.entries.push_back(entry);
    log.written += 1;
}

/// How many entries have ever been written, dropped ones included.
pub fn written() -> u64 {
    lock().written
}

pub fn debug(area: &'static str, message: impl Into<String>) {
    write(Level::Debug, area, message);
}

pub fn info(area: &'static str, message: impl Into<String>) {
    write(Level::Info, area, message);
}

pub fn warn(area: &'static str, message: impl Into<String>) {
    write(Level::Warn, area, message);
}

pub fn error(area: &'static str, message: impl Into<String>) {
    write(Level::Error, area, message);
}

/// Every entry currently held.
pub fn entries() -> Vec<Entry> {
    lock().entries.iter().cloned().collect()
}

/// How many entries are held, and how many of those are warnings or worse.
pub fn counts() -> (usize, usize) {
    let log = lock();
    let bad = log.entries.iter().filter(|e| e.level >= Level::Warn).count();
    (log.entries.len(), bad)
}

/// The whole log as plain text, with the header that says what it is.
pub fn text() -> String {
    text_from(Level::Debug)
}

/// The log as plain text, keeping entries at `min` and above.
///
/// Routine entries are the bulk of it — a reparse for every keystroke — and
/// filtering them out is usually the difference between a log somebody can read
/// and a log they scroll past.
pub fn text_from(min: Level) -> String {
    let log = lock();
    let mut out = header(&log);
    out.push('\n');
    if let Some((path, _)) = &log.sink {
        out.push_str(&format!("also being written to {}\n", path.display()));
    }
    if let Some(err) = &log.sink_error {
        out.push_str(&format!("file log unavailable: {err}\n"));
    }
    let shown = log.entries.iter().filter(|e| e.level >= min).count();
    if min > Level::Debug {
        out.push_str(&format!(
            "showing {shown} of {} entries, {} and above\n",
            log.entries.len(),
            min
        ));
    }
    out.push('\n');
    if shown == 0 {
        out.push_str("(nothing yet)\n");
    }
    for entry in log.entries.iter().filter(|e| e.level >= min) {
        out.push_str(&entry.to_string());
        out.push('\n');
    }
    out
}

/// Where the file copy is going, if anywhere.
pub fn sink_path() -> Option<PathBuf> {
    lock().sink.as_ref().map(|(path, _)| path.clone())
}

/// Write the log out to a file, whatever `OPENWRITE_LOG` said. Everything goes
/// in: a log being saved to be sent to somebody should be the whole of it.
pub fn save(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, text())
}

/// Throw the entries away. The clock and the file sink carry on.
pub fn clear() {
    let mut log = lock();
    log.entries.clear();
}

/// Record a panic before the program goes down.
///
/// Chained in front of whatever hook was already installed, so the usual
/// message still reaches the terminal and the platform crash reporter. The
/// entry only reaches a file if `OPENWRITE_LOG` was set — by the time this
/// runs, the window is not going to be showing anybody anything.
pub fn catch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let where_ = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "an unknown place".to_string());
        // The payload is the programmer's message, not the writer's text.
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_string());
        error("panic", format!("{what}, at {where_}"));
        previous(info);
    }));
}

/// A stopwatch, for the durations that make up most of a useful debug log.
pub struct Timer(Instant);

impl Timer {
    pub fn start() -> Timer {
        Timer(Instant::now())
    }

    /// Milliseconds since it started.
    pub fn ms(&self) -> u128 {
        self.0.elapsed().as_millis()
    }
}

impl Default for Timer {
    fn default() -> Self {
        Timer::start()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These tests share one global log, so they run one at a time and clear up
    /// after themselves. A lock of their own rather than `--test-threads=1`,
    /// which would slow down every other test in the crate.
    static ALONE: Mutex<()> = Mutex::new(());

    fn alone() -> std::sync::MutexGuard<'static, ()> {
        let guard = ALONE.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        guard
    }

    #[test]
    fn an_entry_says_when_what_and_where_from() {
        let _alone = alone();
        start();
        info("open", "1204 bytes, .sct header, 2 profiles");

        let entries = entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].level, Level::Info);
        assert_eq!(entries[0].area, "open");
        assert!(entries[0].message.contains("2 profiles"));

        let line = entries[0].to_string();
        assert!(line.contains("ms"), "{line}");
        assert!(line.contains("INFO"), "{line}");
        assert!(line.contains("open"), "{line}");
    }

    #[test]
    fn the_ring_keeps_the_most_recent_and_drops_the_oldest() {
        let _alone = alone();
        start();
        for i in 0..CAPACITY + 50 {
            debug("reparse", format!("entry {i}"));
        }
        let entries = entries();
        assert_eq!(entries.len(), CAPACITY);
        assert!(entries[0].message.ends_with("50"), "{}", entries[0].message);
        assert!(entries[CAPACITY - 1].message.ends_with(&format!("{}", CAPACITY + 49)));
    }

    #[test]
    fn the_counts_pick_out_what_went_wrong() {
        let _alone = alone();
        start();
        debug("reparse", "3 pages");
        info("save", "ok");
        warn("ai", "no models");
        error("open", "no such file");

        assert_eq!(counts(), (4, 2));
    }

    #[test]
    fn the_text_dump_says_what_it_is_and_what_it_is_not() {
        let _alone = alone();
        start();
        info("open", "1204 bytes");

        let text = text();
        assert!(text.contains("openwrite"));
        assert!(text.contains("not what was written"));
        assert!(text.contains("1204 bytes"));
    }

    #[test]
    fn clearing_leaves_the_log_usable() {
        let _alone = alone();
        start();
        info("open", "one");
        clear();
        assert!(entries().is_empty());
        info("open", "two");
        assert_eq!(entries().len(), 1);
    }

    #[test]
    fn routine_entries_can_be_left_out() {
        let _alone = alone();
        start();
        debug("reparse", "3 pages");
        info("save", "ok");
        error("open", "no such file");

        let all = text_from(Level::Debug);
        assert!(all.contains("3 pages"));

        let quiet = text_from(Level::Info);
        assert!(!quiet.contains("3 pages"), "{quiet}");
        assert!(quiet.contains("no such file"));
        assert!(quiet.contains("showing 2 of 3"), "{quiet}");
    }

    #[test]
    fn an_empty_log_says_so_rather_than_showing_a_blank() {
        let _alone = alone();
        start();
        assert!(text().contains("(nothing yet)"));
    }

    #[test]
    fn the_written_count_keeps_going_after_the_ring_is_full() {
        let _alone = alone();
        start();
        let before = written();
        for _ in 0..CAPACITY + 10 {
            debug("reparse", "x");
        }
        assert_eq!(written(), before + CAPACITY as u64 + 10);
        assert_eq!(entries().len(), CAPACITY);
    }

    #[test]
    fn a_timer_measures_forwards() {
        let timer = Timer::start();
        // Not a duration assertion — those are flaky. Only that it is a number
        // and does not go backwards.
        let first = timer.ms();
        assert!(timer.ms() >= first);
    }
}
