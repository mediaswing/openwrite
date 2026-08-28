//! Asking a model on this machine for a second opinion.
//!
//! This is optional in every sense. It is a Cargo feature, so a build can leave
//! it out entirely; nothing here runs unless the writer asks it to; and if no
//! model server is running, the editor says so once and carries on being an
//! editor. Nothing is sent anywhere by default, and what is sent goes to a port
//! on the writer's own machine.
//!
//! Two shapes of server are understood, which between them cover what people
//! actually run:
//!
//! - **Ollama**, on `/api/tags` and `/api/generate`.
//! - **Anything OpenAI-compatible** — llama.cpp's server, LM Studio, Jan — on
//!   `/v1/models` and `/v1/chat/completions`.
//!
//! [`discover`] works out which is there by asking, so the writer does not have
//! to know or care. The address comes from `OPENWRITE_AI_URL` if it is set, and
//! is `http://127.0.0.1:11434` otherwise, which is where Ollama listens.
//!
//! The prompt — the world, the characters, the page the writer is on — is built
//! in [`prompt`], and the transport in [`http`]. Neither of them knows about
//! the other.

pub mod http;
pub mod prompt;

// The JSON reader moved out to the crate root when the update check needed it
// too; re-exported so `ai::json` still means what it always did.
pub use crate::json;

pub use prompt::{Ask, Context};

use crate::log;
use http::Url;
use std::fmt;

/// Where a local model server usually listens: Ollama's default port.
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

/// Set this to point the tool at a server somewhere else.
pub const URL_ENV: &str = "OPENWRITE_AI_URL";

/// Set this to choose the model without going through the window.
pub const MODEL_ENV: &str = "OPENWRITE_AI_MODEL";

/// Warm enough to suggest something the writer had not thought of, cool enough
/// that it stays in the story it was given.
const TEMPERATURE: f32 = 0.85;

/// A hard ceiling on the answer. Three suggestions is a short answer; a model
/// that has started writing the rest of the film should be stopped.
const MAX_TOKENS: u32 = 600;

/// Which dialect a server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Ollama's own API.
    Ollama,
    /// The OpenAI chat-completions shape, as served by llama.cpp, LM Studio
    /// and Jan.
    OpenAiCompatible,
}

impl Backend {
    /// What to call it in the window.
    pub fn label(self) -> &'static str {
        match self {
            Backend::Ollama => "Ollama",
            Backend::OpenAiCompatible => "OpenAI-compatible server",
        }
    }
}

/// A model server that answered.
#[derive(Debug, Clone)]
pub struct Server {
    pub url: Url,
    pub backend: Backend,
    /// The models it has, in the order it listed them.
    pub models: Vec<String>,
}

impl Server {
    /// The model to use if the writer has not picked one: whatever `MODEL_ENV`
    /// says, if the server has it, and otherwise the first one listed.
    pub fn default_model(&self) -> Option<String> {
        let wanted = std::env::var(MODEL_ENV).ok().filter(|m| !m.trim().is_empty());
        if let Some(wanted) = wanted {
            let wanted = wanted.trim();
            if let Some(exact) = self.models.iter().find(|m| m.as_str() == wanted) {
                return Some(exact.clone());
            }
            // `llama3.2` should find `llama3.2:latest`.
            if let Some(tagged) = self.models.iter().find(|m| {
                m.split(':').next() == Some(wanted)
            }) {
                return Some(tagged.clone());
            }
        }
        self.models.first().cloned()
    }

    /// A sentence describing what was found, for the status bar.
    ///
    /// Written out of the language file rather than assembled here: "1 model"
    /// and "2 models" is an English rule, and where the server is has to be
    /// able to move within the sentence.
    pub fn summary(&self) -> String {
        let place = if self.url.is_loopback() {
            crate::t!("ideas.on_this_machine")
        } else {
            crate::t!("ideas.at_address", address = self.url.authority())
        };
        let backend = self.backend.label();
        match self.models.len() {
            0 => crate::t!("ideas.summary.no_models", backend = backend, place = place),
            n => crate::tn!("ideas.summary", n, backend = backend, place = place),
        }
    }
}

/// Why an ask did not produce a suggestion.
#[derive(Debug)]
pub enum Error {
    /// Nothing answered on that address.
    NotFound { url: String, detail: String },
    /// The server is there, but has no model to run.
    NoModels { url: String },
    /// The conversation failed.
    Transport(http::Error),
    /// The reply was not the shape it should have been.
    Reply(String),
    /// The model ran, and produced nothing.
    Empty,
    /// Ollama would not start, or started and never answered.
    NotStarted(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Just the fact. Why it could not connect is an operating system
            // message about sockets, which helps nobody standing in front of
            // the window — it goes to the debug log, where somebody actually
            // debugging can find it.
            Error::NotFound { url, .. } => write!(f, "Nothing is listening at {url}."),
            Error::NoModels { url } => write!(
                f,
                "A model server is running at {url} but has no models. Install one first — \
                 for Ollama, `ollama pull llama3.2`."
            ),
            Error::Transport(err) => write!(f, "{err}"),
            Error::Reply(what) => write!(f, "The model server replied with {what}"),
            Error::Empty => write!(f, "The model had nothing to suggest. Try asking again."),
            Error::NotStarted(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<http::Error> for Error {
    fn from(err: http::Error) -> Self {
        Error::Transport(err)
    }
}

/// Where to get Ollama, for a machine that has not got it.
pub const OLLAMA_DOWNLOAD: &str = "https://ollama.com/download";

/// Is the Ollama command line on this machine?
///
/// Asked by running it, which is the only answer that is actually true: a
/// binary on `PATH` that will not run is not installed as far as this is
/// concerned. `--version` is the cheapest subcommand and needs no server.
pub fn ollama_installed() -> bool {
    std::process::Command::new("ollama")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Start Ollama's server, and wait until it answers.
///
/// Detached, with its output sent nowhere: this is the writer starting a
/// service on their own machine, not a subprocess of the editor, and it should
/// outlive the editor rather than dying with it.
///
/// Returns once the server is up, which is what the caller actually wants to
/// know — a spawn that succeeded and a server that is listening are not the
/// same thing, and the wait is a few seconds on a cold start.
pub fn start_ollama(base: &str, wait: std::time::Duration) -> Result<Server, Error> {
    let spawned = std::process::Command::new("ollama")
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    if let Err(err) = spawned {
        log::warn("ai", format!("could not run `ollama serve`: {err}"));
        // On macOS, Ollama is often an app bundle whose command line was never
        // put on PATH. Asking the system to open it comes to the same thing.
        if cfg!(target_os = "macos") {
            // The full path, not `open`: a bare name is resolved against the
            // working directory before anything else on some platforms, and
            // this one is always here.
            let opened = std::process::Command::new("/usr/bin/open")
                .args(["-a", "Ollama"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if !matches!(opened, Ok(status) if status.success()) {
                return Err(Error::NotStarted(format!("could not start Ollama: {err}")));
            }
        } else {
            return Err(Error::NotStarted(format!("could not start Ollama: {err}")));
        }
    }
    log::info("ai", format!("started Ollama, waiting up to {} s", wait.as_secs()));

    // Poll rather than sleep-then-hope: a warm start answers in well under a
    // second and there is no reason to make the writer wait for a fixed guess.
    let deadline = std::time::Instant::now() + wait;
    let mut last = None;
    while std::time::Instant::now() < deadline {
        match discover(base) {
            Ok(server) => return Ok(server),
            Err(err) => last = Some(err),
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    Err(Error::NotStarted(format!(
        "Ollama was started but had not answered after {} seconds. {}",
        wait.as_secs(),
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

/// The address to try: `OPENWRITE_AI_URL`, or Ollama's default port.
pub fn default_url() -> String {
    std::env::var(URL_ENV)
        .ok()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// Find out what is listening, and what it can run.
///
/// This is the only thing that touches the network before the writer asks a
/// question, and it only happens when they open the window that needs it.
pub fn discover(base: &str) -> Result<Server, Error> {
    let base = http::url(base)?;
    let timer = log::Timer::start();
    // The address is the program's own configuration, not the writer's work,
    // so it goes in the log; nothing else here does.
    log::info("ai", format!("looking for a model server at {}", base.authority()));

    // Ollama first: it is what most people have, and it is what the default
    // address points at.
    let ollama = http::get(&base.with_path("/api/tags"));
    if let Ok(body) = &ollama {
        if let Ok(models) = ollama_models(body) {
            log::info("ai", format!("ollama answered in {} ms", timer.ms()));
            return finish(base, Backend::Ollama, models);
        }
    }

    let openai = http::get(&base.with_path("/v1/models"));
    if let Ok(body) = &openai {
        if let Ok(models) = openai_models(body) {
            log::info("ai", format!("openai-compatible server answered in {} ms", timer.ms()));
            return finish(base, Backend::OpenAiCompatible, models);
        }
    }

    let detail = match (ollama, openai) {
        (Err(err), _) | (_, Err(err)) => err.to_string(),
        _ => "the reply was not a list of models".to_string(),
    };
    log::warn("ai", format!("nothing found in {} ms: {detail}", timer.ms()));
    Err(Error::NotFound { url: base.authority(), detail })
}

fn finish(url: Url, backend: Backend, models: Vec<String>) -> Result<Server, Error> {
    if models.is_empty() {
        log::warn("ai", format!("{} has no models installed", url.authority()));
        return Err(Error::NoModels { url: url.authority() });
    }
    log::info(
        "ai",
        format!("{} at {}, {} models, loopback {}", backend.label(), url.authority(), models.len(), url.is_loopback()),
    );
    Ok(Server { url, backend, models })
}

fn ollama_models(body: &str) -> Result<Vec<String>, Error> {
    let value = json::parse(body).map_err(|e| Error::Reply(e.to_string()))?;
    let models = value
        .get("models")
        .and_then(json::Value::as_array)
        .ok_or_else(|| Error::Reply("no list of models".to_string()))?;
    Ok(models.iter().filter_map(|m| m.string("name")).map(str::to_string).collect())
}

fn openai_models(body: &str) -> Result<Vec<String>, Error> {
    let value = json::parse(body).map_err(|e| Error::Reply(e.to_string()))?;
    let models = value
        .get("data")
        .and_then(json::Value::as_array)
        .ok_or_else(|| Error::Reply("no list of models".to_string()))?;
    Ok(models.iter().filter_map(|m| m.string("id")).map(str::to_string).collect())
}

/// Put one question to a model and wait for the whole answer.
///
/// Deliberately not streamed. A suggestion is a paragraph, the wait is a few
/// seconds, and a half-written suggestion arriving a word at a time is a worse
/// thing to put in front of somebody who is trying to write than a moment's
/// wait — particularly somebody reading it through a screen reader.
pub fn generate(server: &Server, model: &str, system: &str, user: &str) -> Result<String, Error> {
    let (path, body) = match server.backend {
        Backend::Ollama => (
            "/api/generate",
            format!(
                concat!(
                    r#"{{"model":"{}","system":"{}","prompt":"{}","stream":false,"#,
                    r#""options":{{"temperature":{},"num_predict":{}}}}}"#
                ),
                json::escape(model),
                json::escape(system),
                json::escape(user),
                TEMPERATURE,
                MAX_TOKENS
            ),
        ),
        Backend::OpenAiCompatible => (
            "/v1/chat/completions",
            format!(
                concat!(
                    r#"{{"model":"{}","messages":["#,
                    r#"{{"role":"system","content":"{}"}},"#,
                    r#"{{"role":"user","content":"{}"}}],"#,
                    r#""stream":false,"temperature":{},"max_tokens":{}}}"#
                ),
                json::escape(model),
                json::escape(system),
                json::escape(user),
                TEMPERATURE,
                MAX_TOKENS
            ),
        ),
    };

    // Sizes and timings only. The prompt is the writer's world, characters and
    // page, and the answer is a draft of their script: neither goes in the log.
    let timer = log::Timer::start();
    log::info(
        "ai",
        format!(
            "asking {} via {path}, {} characters of prompt",
            model,
            user.chars().count()
        ),
    );

    let reply = match http::post_json(&server.url.with_path(path), &body) {
        Ok(reply) => reply,
        Err(err) => {
            log::error("ai", format!("the request failed after {} ms: {err}", timer.ms()));
            return Err(err.into());
        }
    };
    let answer = match server.backend {
        Backend::Ollama => ollama_answer(&reply),
        Backend::OpenAiCompatible => openai_answer(&reply),
    };
    let answer = match answer {
        Ok(answer) => answer,
        Err(err) => {
            log::error("ai", format!("unusable reply after {} ms: {err}", timer.ms()));
            return Err(err);
        }
    };

    let answer = tidy(&answer);
    if answer.is_empty() {
        log::warn("ai", format!("the model answered nothing in {} ms", timer.ms()));
        return Err(Error::Empty);
    }
    log::info(
        "ai",
        format!("{} characters of answer in {} ms", answer.chars().count(), timer.ms()),
    );
    Ok(answer)
}

fn ollama_answer(reply: &str) -> Result<String, Error> {
    let value = json::parse(reply).map_err(|e| Error::Reply(e.to_string()))?;
    // A refusal comes back as `{"error": "..."}` with a 200 in some versions.
    if let Some(message) = value.string("error") {
        return Err(Error::Reply(format!("an error: {message}")));
    }
    value
        .string("response")
        .map(str::to_string)
        .ok_or_else(|| Error::Reply("no suggestion in it".to_string()))
}

fn openai_answer(reply: &str) -> Result<String, Error> {
    let value = json::parse(reply).map_err(|e| Error::Reply(e.to_string()))?;
    if let Some(message) = value.get("error").and_then(|e| e.string("message")) {
        return Err(Error::Reply(format!("an error: {message}")));
    }
    value
        .get("choices")
        .and_then(|c| c.at(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.string("content"))
        .map(str::to_string)
        .ok_or_else(|| Error::Reply("no suggestion in it".to_string()))
}

/// Take the model's habits off the answer.
///
/// Small models like to wrap things in a code fence whatever they are told, and
/// the fence would be pasted into the screenplay as action. Anything more
/// opinionated than this is left alone: it is the writer's page, and a
/// suggestion they have to un-mangle is worse than one they have to trim.
pub fn tidy(answer: &str) -> String {
    let mut text = answer.trim();

    if let Some(rest) = text.strip_prefix("```") {
        // The opening fence may carry a language tag on the same line.
        let rest = rest.split_once('\n').map(|(_, body)| body).unwrap_or("");
        text = rest.trim_end().strip_suffix("```").unwrap_or(rest).trim();
    }

    // Some models close with a fence they never opened.
    let text = text.strip_suffix("```").unwrap_or(text).trim_end();

    // Blank runs longer than one line read as a page break in Fountain, and
    // they are never what the model meant.
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollamas_model_list_is_read() {
        let body = r#"{"models":[{"name":"llama3.2:3b","size":1},{"name":"mistral:7b"}]}"#;
        assert_eq!(ollama_models(body).unwrap(), vec!["llama3.2:3b", "mistral:7b"]);
    }

    #[test]
    fn an_openai_compatible_model_list_is_read() {
        let body = r#"{"object":"list","data":[{"id":"local-model","object":"model"}]}"#;
        assert_eq!(openai_models(body).unwrap(), vec!["local-model"]);
    }

    #[test]
    fn a_list_that_is_not_a_list_is_an_error_rather_than_an_empty_server() {
        assert!(ollama_models("not json").is_err());
        assert!(ollama_models(r#"{"something":"else"}"#).is_err());
        assert!(openai_models(r#"{"models":[]}"#).is_err());
    }

    #[test]
    fn a_suggestion_is_lifted_out_of_either_shape_of_reply() {
        let ollama = r#"{"model":"llama3.2","response":"She puts the tally down.","done":true}"#;
        assert_eq!(ollama_answer(ollama).unwrap(), "She puts the tally down.");

        let openai = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"MAYA\nNo."}}]}"#;
        assert_eq!(openai_answer(openai).unwrap(), "MAYA\nNo.");
    }

    #[test]
    fn an_error_in_the_reply_is_reported_rather_than_read_as_a_suggestion() {
        let err = ollama_answer(r#"{"error":"model \"nope\" not found"}"#).unwrap_err();
        assert!(err.to_string().contains("not found"));

        let err = openai_answer(r#"{"error":{"message":"no model loaded"}}"#).unwrap_err();
        assert!(err.to_string().contains("no model loaded"));
    }

    #[test]
    fn a_code_fence_is_taken_off() {
        assert_eq!(tidy("```fountain\nMAYA\nNo.\n```"), "MAYA\nNo.");
        assert_eq!(tidy("```\nMAYA\nNo.\n```"), "MAYA\nNo.");
        // A fence that was never opened.
        assert_eq!(tidy("MAYA\nNo.\n```"), "MAYA\nNo.");
    }

    #[test]
    fn long_runs_of_blank_lines_are_closed_up() {
        // Three blank lines in Fountain would read as an intentional gap.
        assert_eq!(tidy("One.\n\n\n\nTwo."), "One.\n\nTwo.");
    }

    #[test]
    fn an_answer_of_nothing_at_all_is_nothing() {
        assert!(tidy("   \n\n  ").is_empty());
        assert!(tidy("```\n\n```").is_empty());
    }

    #[test]
    fn a_request_body_is_valid_json_whatever_is_in_the_prompt() {
        // A screenplay is full of quotation marks and apostrophes; the body has
        // to survive them.
        let awkward = "She said \"no\" — then\ttyped a backslash: \\";
        let body = format!(
            r#"{{"model":"{}","prompt":"{}"}}"#,
            json::escape("llama3.2:3b"),
            json::escape(awkward)
        );
        let parsed = json::parse(&body).unwrap();
        assert_eq!(parsed.string("prompt"), Some(awkward));
    }

    #[test]
    fn the_default_model_prefers_what_the_environment_asked_for() {
        let server = Server {
            url: http::url(DEFAULT_URL).unwrap(),
            backend: Backend::Ollama,
            models: vec!["mistral:7b".into(), "llama3.2:latest".into()],
        };
        // With nothing set, the first listed model.
        assert_eq!(server.default_model().as_deref(), Some("mistral:7b"));
    }

    #[test]
    fn a_server_says_what_it_is_in_one_line() {
        let server = Server {
            url: http::url(DEFAULT_URL).unwrap(),
            backend: Backend::Ollama,
            models: vec!["llama3.2:3b".into()],
        };
        assert_eq!(server.summary(), "Ollama on this machine, 1 model");

        let elsewhere = Server {
            url: http::url("http://192.168.1.5:11434").unwrap(),
            models: vec!["a".into(), "b".into()],
            ..server
        };
        assert!(elsewhere.summary().contains("at 192.168.1.5:11434"));
    }
}
