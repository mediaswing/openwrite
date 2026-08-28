//! Talking to a local model, end to end, against a server that is not one.
//!
//! The point of these is the wiring: that the tool finds a server, works out
//! which dialect it speaks, sends a request the server can actually read, and
//! gets a usable suggestion back out of the reply. A stub on a loopback port
//! stands in for Ollama, so the tests need no model, no network and no GPU, and
//! they can assert on exactly what went over the wire.

#![cfg(feature = "ai")]

use openwrite::ai::{self, prompt::Ask};
use openwrite::bible::Bible;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// One HTTP request as the stub server saw it.
struct Seen {
    path: String,
    body: String,
}

/// A stub HTTP server that answers a fixed script of replies and reports what
/// it was asked. It closes after the last reply, which is what the client's
/// `Connection: close` expects.
fn stub(replies: Vec<(u16, String)>) -> (String, Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for (code, body) in replies {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut stream = stream;
            match read_request(&mut stream) {
                Ok(seen) => {
                    let _ = tx.send(seen);
                }
                Err(_) => return,
            }
            let reason = if code == 200 { "OK" } else { "Error" };
            let reply = format!(
                "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://{address}"), rx)
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Seen> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let path = request_line.split_whitespace().nth(1).unwrap_or("").to_string();

    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Seen { path, body: String::from_utf8_lossy(&body).into_owned() })
}

const SCRIPT: &str = "\
Title: Ashfen

INT. SALT HOUSE - NIGHT

Maya counts the tally sticks. Outside, the lake ticks as it cools.

MAYA
Forty-one.
";

fn bible() -> Bible {
    let mut bible = Bible {
        world: "Ashfen stands on a salt lake. The Guild rules by writ.".to_string(),
        ..Bible::default()
    };
    let maya = bible.ensure("MAYA");
    maya.role = "Salt-runner. The younger sister.".to_string();
    maya.want = "To buy her brother out of his indenture".to_string();
    maya.voice = "Short sentences. Never says what she means.".to_string();
    bible
}

#[test]
fn an_ollama_server_is_found_and_answers_a_question() {
    let (url, seen) = stub(vec![
        (200, r#"{"models":[{"name":"llama3.2:3b"},{"name":"mistral:7b"}]}"#.to_string()),
        (200, r#"{"model":"llama3.2:3b","response":"MAYA\nForty-two, if you're asking.","done":true}"#.to_string()),
    ]);

    let server = ai::discover(&url).expect("the stub should be found");
    assert_eq!(server.backend, ai::Backend::Ollama);
    assert_eq!(server.models, vec!["llama3.2:3b", "mistral:7b"]);
    assert_eq!(server.default_model().as_deref(), Some("llama3.2:3b"));

    let ask = Ask::Say("MAYA".to_string());
    let context = ai::Context::gather(SCRIPT, None, &bible(), &ask);
    let answer = ai::generate(&server, "llama3.2:3b", ai::prompt::SYSTEM, &context.prompt(&ask))
        .expect("a suggestion");
    assert_eq!(answer, "MAYA\nForty-two, if you're asking.");

    let tags = seen.recv().unwrap();
    assert_eq!(tags.path, "/api/tags");

    // The request carries the world, the character notes and the page.
    let generate = seen.recv().unwrap();
    assert_eq!(generate.path, "/api/generate");
    let sent = openwrite::ai::json::parse(&generate.body).expect("valid JSON went out");
    assert_eq!(sent.string("model"), Some("llama3.2:3b"));
    assert_eq!(sent.get("stream"), Some(&openwrite::ai::json::Value::Bool(false)));
    let prompt = sent.string("prompt").expect("a prompt");
    assert!(prompt.contains("salt lake"), "the world was not sent");
    assert!(prompt.contains("Never says what she means"), "the voice notes were not sent");
    assert!(prompt.contains("Forty-one."), "the page was not sent");
    assert!(prompt.contains("INT. SALT HOUSE - NIGHT"), "the scene was not sent");
}

#[test]
fn an_openai_compatible_server_is_found_when_ollama_is_not_there() {
    let (url, seen) = stub(vec![
        // Ollama's endpoint is not there on this server.
        (404, r#"{"error":"not found"}"#.to_string()),
        (200, r#"{"object":"list","data":[{"id":"local-model"}]}"#.to_string()),
        (
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"= The lake freezes early."}}]}"#
                .to_string(),
        ),
    ]);

    let server = ai::discover(&url).expect("the stub should be found");
    assert_eq!(server.backend, ai::Backend::OpenAiCompatible);
    assert_eq!(server.models, vec!["local-model"]);

    let ask = Ask::Next;
    let context = ai::Context::gather(SCRIPT, None, &bible(), &ask);
    let answer = ai::generate(&server, "local-model", ai::prompt::SYSTEM, &context.prompt(&ask))
        .expect("a suggestion");
    assert_eq!(answer, "= The lake freezes early.");

    assert_eq!(seen.recv().unwrap().path, "/api/tags");
    assert_eq!(seen.recv().unwrap().path, "/v1/models");

    let chat = seen.recv().unwrap();
    assert_eq!(chat.path, "/v1/chat/completions");
    let sent = openwrite::ai::json::parse(&chat.body).expect("valid JSON went out");
    let messages = sent.get("messages").and_then(openwrite::ai::json::Value::as_array).unwrap();
    assert_eq!(messages[0].string("role"), Some("system"));
    assert_eq!(messages[1].string("role"), Some("user"));
    assert!(messages[1].string("content").unwrap().contains("salt lake"));
}

#[test]
fn a_chunked_reply_is_read_like_any_other() {
    // Ollama sends chunked when it does not know the length in advance.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        for body in [
            r#"{"models":[{"name":"m"}]}"#,
            r#"{"response":"She says nothing.","done":true}"#,
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });

    let url = format!("http://{address}");
    let server = ai::discover(&url).unwrap();
    let answer = ai::generate(&server, "m", "system", "user").unwrap();
    assert_eq!(answer, "She says nothing.");
}

#[test]
fn nothing_listening_is_a_message_a_writer_can_act_on() {
    // A port nobody is on: bind it, learn the number, drop it.
    let free = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let err = ai::discover(&format!("http://{free}")).unwrap_err();
    let message = err.to_string();
    // Short and plain: the window offers the button that fixes it, so the
    // message does not have to explain how to start a model server.
    assert!(message.contains("Nothing is listening"), "{message}");
    assert!(message.contains(&free.to_string()), "{message}");
}

#[test]
fn a_server_with_no_models_says_so_rather_than_failing_later() {
    let (url, _seen) = stub(vec![(200, r#"{"models":[]}"#.to_string())]);
    let err = ai::discover(&url).unwrap_err();
    assert!(err.to_string().contains("no models"), "{err}");
}

#[test]
fn a_model_that_refuses_is_reported_not_pasted_into_the_script() {
    let (url, _seen) = stub(vec![
        (200, r#"{"models":[{"name":"m"}]}"#.to_string()),
        (404, r#"{"error":"model 'nope' not found"}"#.to_string()),
    ]);
    let server = ai::discover(&url).unwrap();
    let err = ai::generate(&server, "nope", "system", "user").unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");
}

#[test]
fn a_screenplay_full_of_quotation_marks_still_makes_a_valid_request() {
    let (url, seen) = stub(vec![
        (200, r#"{"models":[{"name":"m"}]}"#.to_string()),
        (200, r#"{"response":"Fine."}"#.to_string()),
    ]);
    let awkward = "INT. HOUSE - DAY\n\nShe reads: \"a \\ b\", then stops.\n\nMAYA\n\"No.\"\n";
    let server = ai::discover(&url).unwrap();

    let ask = Ask::Do("MAYA".to_string());
    let context = ai::Context::gather(awkward, None, &Bible::default(), &ask);
    ai::generate(&server, "m", ai::prompt::SYSTEM, &context.prompt(&ask)).unwrap();

    let _ = seen.recv().unwrap();
    let generate = seen.recv().unwrap();
    let sent = openwrite::ai::json::parse(&generate.body).expect("valid JSON went out");
    assert!(sent.string("prompt").unwrap().contains("\"a \\ b\""));
}

#[test]
fn a_reply_that_is_not_json_is_an_error_rather_than_a_suggestion() {
    let (url, _seen) = stub(vec![
        (200, r#"{"models":[{"name":"m"}]}"#.to_string()),
        (200, "<html>proxy error</html>".to_string()),
    ]);
    let server = ai::discover(&url).unwrap();
    assert!(ai::generate(&server, "m", "system", "user").is_err());
}

/// The log has to be safe to send to somebody.
///
/// Everything about a model request except its size is the writer's work: the
/// world they invented, the people in it, the page they are on, and the draft
/// that comes back. A debug log that quietly kept a copy would be a worse
/// privacy problem than the network request it was there to help debug.
#[test]
fn nothing_the_writer_wrote_reaches_the_debug_log() {
    openwrite::log::start();

    // Phrases that exist nowhere else, so finding one in the log means it came
    // from the screenplay or from the model.
    const IN_THE_SCRIPT: &str = "zarquon-the-tally-keeper";
    const IN_THE_WORLD: &str = "quibblesnitch-salt-doctrine";
    const IN_THE_ANSWER: &str = "flimberwatch-of-the-guild";

    let (url, _seen) = stub(vec![
        (200, r#"{"models":[{"name":"m"}]}"#.to_string()),
        (200, format!(r#"{{"response":"MAYA\n{IN_THE_ANSWER}."}}"#)),
    ]);

    let script = format!("INT. SALT HOUSE - NIGHT\n\nMaya counts. {IN_THE_SCRIPT}.\n");
    let bible = Bible { world: IN_THE_WORLD.to_string(), ..Bible::default() };

    let server = ai::discover(&url).unwrap();
    let ask = Ask::Say("MAYA".to_string());
    let context = ai::Context::gather(&script, None, &bible, &ask);
    let prompt = context.prompt(&ask);

    // The prompt really does carry all three, so the log is being tested
    // against something rather than against nothing.
    assert!(prompt.contains(IN_THE_SCRIPT));
    assert!(prompt.contains(IN_THE_WORLD));

    let answer = ai::generate(&server, "m", ai::prompt::SYSTEM, &prompt).unwrap();
    assert!(answer.contains(IN_THE_ANSWER));

    let log = openwrite::log::text();
    for secret in [IN_THE_SCRIPT, IN_THE_WORLD, IN_THE_ANSWER] {
        assert!(!log.contains(secret), "{secret:?} reached the debug log:\n{log}");
    }

    // And it did log the request, so the absence above is not simply an
    // absence of logging.
    assert!(log.contains("characters of prompt"), "the request was not logged:\n{log}");
    assert!(log.contains("characters of answer"), "the answer was not logged:\n{log}");
}
