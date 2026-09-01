//! Talking to ElevenLabs.
//!
//! # Why `curl` again
//!
//! For the reason [`crate::update`] gives: ElevenLabs is HTTPS, and
//! [`crate::ai::http`] deliberately speaks only plain HTTP because its whole
//! purpose is that nothing leaves the machine. A TLS stack would be the wrong
//! tool wearing the wrong name, and a hundred crates to send a sentence.
//!
//! # The key is never on the command line
//!
//! Anything in a process's arguments is readable by every other process on the
//! machine — `ps` is all it takes. So nothing is passed as an argument: the
//! whole request, key included, is written to a curl configuration file with
//! no permissions on it but the owner's, and curl is run as
//! `curl --config <file>`. The file is deleted when the request ends, whether
//! it worked or not.
//!
//! That file has to go somewhere, and on Linux the temporary folder is `/tmp`,
//! which everybody logged in shares. So the folder it goes in is made with a
//! name drawn from the operating system's own randomness rather than from the
//! process id, and made in a way that fails if anything is already there — see
//! [`Scratch::new`]. A folder somebody else could name in advance is a folder
//! they can create first, world-readable, and read the key straight out of.
//!
//! # It asks for PCM
//!
//! `output_format=pcm_24000` rather than the MP3 that is the default, because
//! everything in [`super::audio`] is arithmetic on samples and there is no
//! arithmetic to be done on an MP3 without a decoder this program does not
//! have. 24 kHz is the highest PCM rate an ordinary account is given; 44.1
//! wants a paid tier, and for speech it would not be heard.

use super::story::State;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Where ElevenLabs is. Not configurable: it is one company's one API, and a
/// settable address here would only ever be a way to send somebody's key
/// somewhere else.
const API: &str = "https://api.elevenlabs.io";

/// The voice model. Multilingual v2 is the one that reads a script evenly and
/// takes direction from `voice_settings`, which is what this tool asks of it.
pub const DEFAULT_MODEL: &str = "eleven_multilingual_v2";

/// Overrides the model, for trying a new one without a new build.
pub const MODEL_ENV: &str = "OPENWRITE_ELEVENLABS_MODEL";

/// Signed 16-bit, little-endian, mono, 24 kHz — see the module note.
const OUTPUT_FORMAT: &str = "pcm_24000";

/// How long one line may take. Generation is a second or two; this is the
/// point at which something is wrong rather than slow.
const TIMEOUT: Duration = Duration::from_secs(120);

/// How long to wait for the list of voices.
const LIST_TIMEOUT: Duration = Duration::from_secs(20);

/// The most audio one line may be, so that a reply that will not stop cannot
/// fill the disk. Ten minutes of 24 kHz mono is far past any line of dialogue.
const MAX_AUDIO: u64 = 24_000 * 2 * 600;

/// The most JSON to read back into a string.
///
/// Generous on purpose. An account with hundreds of voices answers `/v2/voices`
/// with hundreds of kilobytes — each voice carries its labels, its settings, a
/// preview address and its languages — so a cap set for the size of an error
/// message would quietly hand back nothing and look exactly like an account
/// with no voices in it.
const MAX_JSON: u64 = 8 * 1024 * 1024;

/// The most of a refusal worth reading. These are a sentence.
const MAX_ERROR: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No key has been set.
    NoKey,
    /// The key, or a voice id, has something in it that cannot be sent.
    Rejected(String),
    /// `curl` is not on this machine, or would not run.
    NoCurl(String),
    /// The request failed, timed out, or was refused.
    Request(String),
    /// ElevenLabs answered, and said no.
    Refused { code: u16, detail: String },
    /// The answer was not what was asked for.
    Reply(String),
    /// A scratch file could not be written or read.
    Disk(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoKey => write!(f, "no ElevenLabs API key has been set"),
            Error::Rejected(what) => write!(f, "{what}"),
            Error::NoCurl(what) => write!(f, "could not run curl: {what}"),
            Error::Request(what) => write!(f, "could not reach ElevenLabs: {what}"),
            Error::Refused { code, detail } => {
                let detail = detail.trim();
                match (code, detail.is_empty()) {
                    (401, _) => write!(f, "ElevenLabs did not accept the API key"),
                    (429, _) => write!(f, "ElevenLabs is rate limiting this key; try again shortly"),
                    (_, true) => write!(f, "ElevenLabs answered {code}"),
                    (_, false) => write!(f, "ElevenLabs answered {code}: {detail}"),
                }
            }
            Error::Reply(what) => write!(f, "ElevenLabs said something unexpected: {what}"),
            Error::Disk(what) => write!(f, "a working file could not be used: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// A voice as ElevenLabs lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub id: String,
    pub name: String,
    /// Whatever the labels say — `male`, `middle aged`, `british`. Shown
    /// beside the name, because that is what makes a list of hundreds usable.
    pub description: String,
}

impl Remote {
    /// Name and labels together, for a list.
    pub fn label(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            format!("{} — {}", self.name, self.description)
        }
    }
}

/// Is this something that can be sent as a key at all?
///
/// Strict rather than escaped: a key is base-64-ish, and anything else in it is
/// a paste that went wrong. Refusing is a better answer than sending a header
/// with a newline in it.
pub fn check_key(key: &str) -> Result<&str, Error> {
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::NoKey);
    }
    if key.len() > 200 {
        return Err(Error::Rejected(
            "that is much too long to be an API key".to_string(),
        ));
    }
    if let Some(bad) = key
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        return Err(Error::Rejected(format!(
            "an API key cannot contain {bad:?}"
        )));
    }
    Ok(key)
}

/// The same, for a voice id, which goes into a URL.
pub fn check_voice_id(id: &str) -> Result<&str, Error> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::Rejected("no voice has been chosen".to_string()));
    }
    if id.len() > 100 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(Error::Rejected(format!("{id:?} is not a voice id")));
    }
    Ok(id)
}

/// How hard the voice is steered, before any of the arithmetic in
/// [`super::audio`] is done to what comes back.
///
/// Two levers, and they pull against each other. `stability` is how closely
/// the model sticks to the voice as recorded: high is even and safe, low lets
/// it act. `style` is how much of the original speaker's delivery it
/// exaggerates. A frightened line wants low stability and high style; a
/// whispered one wants the opposite, because a whisper that wanders stops
/// being the same person.
pub fn settings_for(state: State) -> (f32, f32, f32) {
    // stability, similarity_boost, style
    match state {
        State::Normal => (0.45, 0.80, 0.25),
        State::Whisper => (0.70, 0.85, 0.15),
        State::Scared => (0.22, 0.75, 0.70),
        State::Shout => (0.30, 0.75, 0.85),
        State::Angry => (0.28, 0.75, 0.80),
        State::Sad => (0.60, 0.80, 0.40),
        State::Excited => (0.30, 0.78, 0.70),
        State::Tired => (0.65, 0.82, 0.30),
    }
}

/// Every voice the key can use.
pub fn voices(key: &str) -> Result<Vec<Remote>, Error> {
    let key = check_key(key)?;
    // v2 is the current listing and pages; v1 is what older keys and
    // self-hosted proxies answer. Ask for the first and accept the second.
    let body = match get(key, "/v2/voices?page_size=100", LIST_TIMEOUT) {
        Ok(body) => body,
        Err(Error::Refused { code, .. }) if code == 404 || code == 400 => {
            get(key, "/v1/voices", LIST_TIMEOUT)?
        }
        Err(err) => return Err(err),
    };
    read_voices(&body)
}

/// Pull the voices out of either shape of reply.
fn read_voices(body: &str) -> Result<Vec<Remote>, Error> {
    let value = crate::json::parse(body).map_err(|err| Error::Reply(err.to_string()))?;
    let list = value
        .get("voices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Reply("no voices in the reply".to_string()))?;

    let mut voices: Vec<Remote> = list
        .iter()
        .filter_map(|entry| {
            let id = entry.string("voice_id")?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let name = entry.string("name").unwrap_or("").trim().to_string();
            Some(Remote {
                id,
                name: if name.is_empty() { "?".to_string() } else { name },
                description: labels(entry),
            })
        })
        .collect();
    voices.sort_by_key(|voice| voice.name.to_lowercase());
    Ok(voices)
}

/// The handful of labels worth showing beside a name.
///
/// Not all of them: a voice carries a dozen, and a picker that shows a dozen
/// is a picker nobody reads.
fn labels(entry: &crate::json::Value) -> String {
    let Some(labels) = entry.get("labels") else {
        return String::new();
    };
    ["gender", "age", "accent", "description"]
        .iter()
        .filter_map(|key| labels.string(key))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read one line aloud, and hand back the samples.
///
/// The reply is raw PCM — see the module note — so it goes to a file and comes
/// back as bytes rather than through a string, which would mangle every byte
/// over 127.
pub fn speak(key: &str, voice_id: &str, text: &str, state: State) -> Result<Vec<u8>, Error> {
    let key = check_key(key)?;
    let voice_id = check_voice_id(voice_id)?;
    if text.trim().is_empty() {
        return Err(Error::Rejected("there is nothing to say".to_string()));
    }

    let (stability, similarity, style) = settings_for(state);
    let model = std::env::var(MODEL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| {
            !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let body = format!(
        "{{\"text\":\"{}\",\"model_id\":\"{}\",\"voice_settings\":{{\
         \"stability\":{stability},\"similarity_boost\":{similarity},\
         \"style\":{style},\"use_speaker_boost\":true}}}}",
        crate::json::escape(text.trim()),
        crate::json::escape(&model),
    );

    let scratch = Scratch::new()?;
    let body_file = scratch.path("request.json");
    write_private(&body_file, body.as_bytes())?;
    let audio_file = scratch.path("reply.pcm");

    let url = format!("{API}/v1/text-to-speech/{voice_id}?output_format={OUTPUT_FORMAT}");
    let code = run(&scratch, key, &url, Some(&body_file), &audio_file, TIMEOUT)?;
    if code != 200 {
        // On a refusal the body is JSON rather than audio, and it is the only
        // place ElevenLabs says which of the many reasons this was.
        return Err(Error::Refused {
            code,
            detail: detail_of(&read_text(&audio_file, MAX_ERROR)),
        });
    }

    let audio = std::fs::read(&audio_file).map_err(|err| Error::Disk(err.to_string()))?;
    if audio.is_empty() {
        return Err(Error::Reply("an empty recording".to_string()));
    }
    Ok(audio)
}

/// One `GET`, for the things that answer in JSON.
fn get(key: &str, path: &str, timeout: Duration) -> Result<String, Error> {
    let scratch = Scratch::new()?;
    let out = scratch.path("reply.json");
    let url = format!("{API}{path}");
    let code = run(&scratch, key, &url, None, &out, timeout)?;
    let body = read_text(&out, MAX_JSON);
    if code != 200 {
        return Err(Error::Refused { code, detail: detail_of(&body) });
    }
    Ok(body)
}

/// Read a reply back as text, up to `most` bytes.
///
/// A reply longer than the cap comes back empty rather than truncated: half a
/// JSON document is not a JSON document, and an empty one at least fails as
/// what it is.
fn read_text(path: &Path, most: u64) -> String {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() <= most => std::fs::read_to_string(path).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Run one request and return its status code.
///
/// Everything that matters goes into the configuration file rather than into
/// the arguments, so that the key is never visible in the process table. What
/// comes back on standard output is the status code and nothing else, because
/// the body has been sent to a file — which is what lets a recording be
/// megabytes of PCM without any of it passing through a `String`. Reading that
/// file is the caller's job, since only the caller knows whether it is holding
/// audio or a sentence.
fn run(
    scratch: &Scratch,
    key: &str,
    url: &str,
    body_file: Option<&Path>,
    output: &Path,
    timeout: Duration,
) -> Result<u16, Error> {
    let mut config = String::new();
    config.push_str(&format!("url = \"{}\"\n", quote(url)));
    config.push_str(&format!("header = \"xi-api-key: {}\"\n", quote(key)));
    config.push_str("header = \"Accept: */*\"\n");
    config.push_str("user-agent = \"openwrite\"\n");
    if let Some(body_file) = body_file {
        config.push_str("header = \"Content-Type: application/json\"\n");
        config.push_str(&format!(
            "data-binary = \"@{}\"\n",
            quote(&body_file.to_string_lossy())
        ));
    }
    config.push_str(&format!("output = \"{}\"\n", quote(&output.to_string_lossy())));
    // The status code, and only the status code, on standard output.
    config.push_str("write-out = \"%{http_code}\"\n");
    config.push_str("silent\n");
    config.push_str("show-error\n");
    config.push_str("location\n");
    // As in the update check: `location` on its own will follow a reply down
    // to plain http, or into a file:// URL.
    config.push_str("proto = \"=https\"\n");
    config.push_str("proto-redir = \"=https\"\n");
    config.push_str("max-redirs = 3\n");
    config.push_str(&format!("max-time = {}\n", timeout.as_secs()));
    config.push_str(&format!("max-filesize = {MAX_AUDIO}\n"));

    let config_file = scratch.path("curl.conf");
    write_private(&config_file, config.as_bytes())?;

    let output_of = Command::new(crate::curl::path())
        .arg("--config")
        .arg(&config_file)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| Error::NoCurl(err.to_string()))?;

    // The key was in that file; it has served its purpose.
    let _ = std::fs::remove_file(&config_file);

    let code: u16 = String::from_utf8_lossy(&output_of.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    if code == 0 {
        let why = String::from_utf8_lossy(&output_of.stderr).trim().to_string();
        let why = if why.is_empty() {
            format!("curl exited {}", output_of.status)
        } else {
            why
        };
        return Err(Error::Request(redact(&why, key)));
    }
    Ok(code)
}

/// The sentence out of an ElevenLabs error document, rather than the document.
fn detail_of(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return String::new();
    }
    if let Ok(value) = crate::json::parse(body) {
        // `{"detail":{"message":"...","status":"..."}}` is the usual shape,
        // and `{"detail":"..."}` is the other one.
        if let Some(detail) = value.get("detail") {
            if let Some(message) = detail.string("message") {
                return message.to_string();
            }
            if let Some(message) = detail.as_str() {
                return message.to_string();
            }
        }
        if let Some(message) = value.string("message") {
            return message.to_string();
        }
    }
    body.lines().next().unwrap_or("").chars().take(200).collect()
}

/// Make sure a key that found its way into a message does not survive into a
/// log or a status line.
fn redact(text: &str, key: &str) -> String {
    if key.is_empty() {
        return text.to_string();
    }
    text.replace(key, "…")
}

/// A value going into a curl configuration file.
///
/// The keys and voice ids have already been refused if they contain anything
/// but letters, digits and dashes; this is for the paths, which come from the
/// system's temporary folder and can contain anything a user name can.
fn quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Write a file only its owner can read, and only where there was no file
/// before.
///
/// Two things, and both matter. The permissions are set as the file is made
/// rather than afterwards: a `create` and then a `chmod` leaves a moment in
/// which the key is readable, and that moment is all anybody would need.
///
/// And it is `create_new` rather than `create`, so a path that already exists
/// is an error instead of something to open. `create` would happily write
/// through a symbolic link somebody else had left there — into the writer's
/// own files — and, worse, the mode below is applied only when a file is
/// actually created, so opening one that was already there would have left it
/// with whatever permissions its owner gave it and written the API key into it
/// anyway. The unguessable directory in [`Scratch`] is what makes this
/// condition unreachable; this is what makes it safe if it ever is not.
fn write_private(path: &Path, contents: &[u8]) -> Result<(), Error> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| Error::Disk(format!("{}: {err}", path.display())))?;
    file.write_all(contents)
        .map_err(|err| Error::Disk(format!("{}: {err}", path.display())))?;
    file.flush()
        .map_err(|err| Error::Disk(format!("{}: {err}", path.display())))
}

/// A name nothing else can work out in advance.
///
/// There is no random number generator in the standard library, and this
/// program has no dependencies to borrow one from. There is, though, a hasher
/// whose keys the operating system seeds with random bytes — it is what stops
/// anybody choosing a `HashMap`'s collisions on purpose. Hashing a counter, the
/// process id and the clock with a fresh one of those gives a value that cannot
/// be guessed, for exactly the reason the hasher could not be either.
fn unique() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u64(u64::from(std::process::id()));
    if let Ok(since) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.write_u128(since.as_nanos());
    }
    format!("{:016x}", hasher.finish())
}

/// A folder for one request, which takes itself away afterwards.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    /// A new folder, with a name nobody could have used first.
    ///
    /// The temporary folder is shared between everybody logged in on Linux —
    /// `/tmp`, mode 1777 — so a name another user can work out in advance is a
    /// name they can get to first. It used to be the process id and a counter
    /// from zero, and both of those are readable off `ps`.
    ///
    /// Two changes make that not worth trying. The name comes from
    /// [`unique`] rather than from anything visible. And the folder is made
    /// with `create`, which fails when something is already there, rather than
    /// `create_dir_all`, which succeeds — handing the request somebody else's
    /// folder, with their permissions on it and their files already inside. The
    /// mode goes on as it is created for the same reason it does in
    /// [`write_private`].
    fn new() -> Result<Scratch, Error> {
        let base = std::env::temp_dir();
        // A collision means the name was already taken, which should not happen
        // and costs nothing to survive. Anything else is a real failure.
        for _ in 0..16 {
            let dir = base.join(format!("openwrite-drama-{}", unique()));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&dir) {
                Ok(()) => return Ok(Scratch { dir }),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(Error::Disk(format!("{}: {err}", dir.display()))),
            }
        }
        Err(Error::Disk(format!(
            "{} had no unused name in it",
            base.display()
        )))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Nothing to be done if it fails, and nothing worth interrupting
        // anybody over: the folder is in the system's temporary directory.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_with_anything_odd_in_it_is_refused_rather_than_sent() {
        // Any of these would end the header line and let what follows be read
        // as a header of its own.
        for bad in ["sk_a\r\nX-Injected: yes", "sk_a\nb", "sk a", "sk\"a", "sk\\a"] {
            assert!(check_key(bad).is_err(), "{bad:?} should not be sent");
        }
        assert!(check_key("").is_err());
        assert!(check_key("sk_0123456789abcdef-ABCDEF.x").is_ok());
    }

    #[test]
    fn a_voice_id_with_a_slash_in_it_cannot_reach_another_endpoint() {
        assert!(check_voice_id("../../v1/history").is_err());
        assert!(check_voice_id("abc?x=1").is_err());
        assert!(check_voice_id("21m00Tcm4TlvDq8ikWAM").is_ok());
    }

    #[test]
    fn both_shapes_of_the_voice_list_are_read() {
        let body = r#"{"voices":[
            {"voice_id":"b2","name":"Bella","labels":{"gender":"female","age":"young"}},
            {"voice_id":"a1","name":"Adam","labels":{"gender":"male"}}
        ]}"#;
        let voices = read_voices(body).unwrap();
        assert_eq!(voices.len(), 2);
        // Sorted by name, so a long list can be walked.
        assert_eq!(voices[0].id, "a1");
        assert_eq!(voices[1].label(), "Bella — female, young");
    }

    #[test]
    fn a_voice_without_an_id_is_left_out_rather_than_offered() {
        let body = r#"{"voices":[{"name":"Nameless"},{"voice_id":"ok","name":"Fine"}]}"#;
        let voices = read_voices(body).unwrap();
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, "ok");
    }

    #[test]
    fn a_reply_that_is_not_a_voice_list_is_an_error_rather_than_an_empty_cast() {
        assert!(read_voices("not json").is_err());
        assert!(read_voices("{}").is_err());
    }

    #[test]
    fn the_sentence_is_taken_out_of_an_error_document() {
        assert_eq!(
            detail_of(r#"{"detail":{"status":"invalid_uid","message":"A voice with that id was not found"}}"#),
            "A voice with that id was not found"
        );
        assert_eq!(detail_of(r#"{"detail":"Unauthenticated"}"#), "Unauthenticated");
        // Anything else still says something rather than nothing.
        assert_eq!(detail_of("gateway timeout"), "gateway timeout");
        assert_eq!(detail_of(""), "");
    }

    #[test]
    fn a_key_never_survives_into_a_message() {
        let key = "sk_secret";
        assert!(!redact("curl: (60) sk_secret failed", key).contains(key));
    }

    /// The folder for a request is one nobody could have got to first.
    ///
    /// On Linux the temporary folder is shared, so a predictable name is a name
    /// another user creates first — world-readable, with a file already planted
    /// where the key is about to be written.
    #[test]
    fn two_requests_never_share_a_folder_and_neither_name_can_be_guessed() {
        let (one, two) = (Scratch::new().unwrap(), Scratch::new().unwrap());
        assert_ne!(one.dir, two.dir);

        // Nothing of the process in the name: that is what `ps` gives away.
        let name = one.dir.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.contains(&std::process::id().to_string()), "{name}");
        assert!(name.starts_with("openwrite-drama-"), "{name}");

        for dir in [&one.dir, &two.dir] {
            assert!(dir.is_dir());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(dir).unwrap().permissions().mode();
                assert_eq!(mode & 0o077, 0, "{dir:?} is readable by somebody else");
            }
        }

        let (kept, gone) = (one.dir.clone(), two.dir.clone());
        drop(two);
        assert!(!gone.exists(), "the folder outlived the request");
        assert!(kept.exists(), "the wrong folder was taken away");
    }

    /// A file that is already there is never written through — not a planted
    /// one, and not a symbolic link into the writer's own files.
    #[test]
    fn the_key_is_never_written_into_a_file_that_was_already_there() {
        let scratch = Scratch::new().unwrap();
        let path = scratch.path("curl.conf");

        assert!(write_private(&path, b"first").is_ok());
        // Second time round the path exists, and that is refused rather than
        // opened: `create` would have written through it, and would have left
        // whatever permissions it already had.
        assert!(matches!(write_private(&path, b"second"), Err(Error::Disk(_))));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        #[cfg(unix)]
        {
            let target = scratch.path("elsewhere");
            let link = scratch.path("planted.conf");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(write_private(&link, b"xi-api-key: secret").is_err());
            assert!(!target.exists(), "the key was written through the link");
        }
    }

    #[test]
    fn a_path_with_a_quote_in_it_cannot_end_the_configuration_line() {
        assert_eq!(quote(r#"/tmp/a"b\c"#), r#"/tmp/a\"b\\c"#);
        assert_eq!(quote("/tmp/a\nb"), "/tmp/a\\nb");
    }

    #[test]
    fn a_whisper_is_steadier_than_a_fright() {
        let (whisper, _, whisper_style) = settings_for(State::Whisper);
        let (scared, _, scared_style) = settings_for(State::Scared);
        assert!(whisper > scared);
        assert!(scared_style > whisper_style);
    }
}
