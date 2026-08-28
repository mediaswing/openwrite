//! A very small HTTP/1.1 client, for talking to a model on this machine.
//!
//! Local model servers — Ollama, llama.cpp, LM Studio, Jan — all speak plain
//! HTTP on a loopback port. That is a request line, a few headers, and a JSON
//! body, so it is written out here on a [`TcpStream`] rather than brought in as
//! a dependency: the rest of this program has none, and a program that opens a
//! socket ought to be one whose reader can see exactly what it sends.
//!
//! There is no TLS, on purpose. The whole point of a local model is that
//! nothing leaves the machine, and `https://` is refused rather than quietly
//! downgraded — if a URL needs encrypting, this is the wrong client for it.
//!
//! Everything is bounded: connect, read and total time all have deadlines, and
//! the body has a size cap, so a model server that hangs or floods cannot take
//! the editor with it.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// How long to wait for the socket to open. A local server either answers at
/// once or is not running.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// How long to wait between bytes of a reply. Generation on a small machine is
/// slow, and the wait before the first token is the longest part of it.
const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// The most reply this client will hold. A suggestion is a paragraph; anything
/// past this is a server misbehaving.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// What can go wrong between here and a model.
#[derive(Debug)]
pub enum Error {
    /// The address could not be understood, or asks for something unsupported.
    Url(String),
    /// Nothing is listening, or the connection failed.
    Connect(String),
    /// The connection broke, or timed out, mid-conversation.
    Io(std::io::Error),
    /// The server answered, and the answer was a refusal.
    Status { code: u16, body: String },
    /// The server answered with something that is not an HTTP reply.
    Malformed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Url(what) => write!(f, "{what}"),
            Error::Connect(what) => write!(f, "could not reach the model server: {what}"),
            Error::Io(err) => write!(f, "the connection to the model server failed: {err}"),
            Error::Status { code, body } => {
                let detail = body.trim();
                if detail.is_empty() {
                    write!(f, "the model server answered {code}")
                } else {
                    write!(f, "the model server answered {code}: {}", first_line(detail))
                }
            }
            Error::Malformed(what) => write!(f, "the model server said something unexpected: {what}"),
        }
    }
}

impl std::error::Error for Error {}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

/// A parsed `http://host:port/path` address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Url {
    /// `host:port`, as the `Host` header wants it.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// This address with a different path, keeping the host and port.
    pub fn with_path(&self, path: &str) -> Url {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        Url { path, ..self.clone() }
    }

    /// Is this a server on the machine the editor is running on?
    ///
    /// Worth being able to say out loud: a writer should be told when their
    /// unpublished screenplay is about to cross a network, even their own.
    pub fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "http://{}{}", self.authority(), self.path)
    }
}

/// Parse an address, filling in the parts that were left out.
pub fn url(text: &str) -> Result<Url, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Url("no model server address".to_string()));
    }
    // The host and the path are written straight into a request line and a
    // `Host:` header. A carriage return or newline in either would end the line
    // early and let whatever followed it be read as headers of its own — and
    // this address comes from `OPENWRITE_AI_URL`, which is to say from outside
    // the program. Refuse the lot rather than reason about which byte is safe.
    if let Some(bad) = text.chars().find(|c| c.is_control() || *c == ' ') {
        return Err(Error::Url(format!(
            "{:?} cannot be part of an address",
            bad
        )));
    }
    if let Some(rest) = text.strip_prefix("https://") {
        let _ = rest;
        return Err(Error::Url(
            "this tool only talks to a model over plain http, so that nothing \
             is sent anywhere it cannot see"
                .to_string(),
        ));
    }
    // A bare `127.0.0.1:11434` is what people type, and what a server prints.
    let rest = text.strip_prefix("http://").unwrap_or(text);
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(Error::Url(format!("{text} has no host in it")));
    }

    // An IPv6 literal wears brackets, and the colons inside them are not the
    // colon that introduces the port.
    let (host, port) = if let Some(end) = authority.strip_prefix('[').and_then(|_| authority.find(']')) {
        let host = &authority[..=end];
        match authority[end + 1..].strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None => (host, None),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };

    let port = match port {
        Some(port) => port
            .parse()
            .map_err(|_| Error::Url(format!("{port:?} is not a port number")))?,
        None => 80,
    };
    let path = if path.is_empty() { "/" } else { path };

    Ok(Url { host: host.to_string(), port, path: path.to_string() })
}

/// `GET path`, returning the body.
pub fn get(url: &Url) -> Result<String, Error> {
    send(url, "GET", None)
}

/// `POST path` with a JSON body, returning the body of the reply.
pub fn post_json(url: &Url, body: &str) -> Result<String, Error> {
    send(url, "POST", Some(body))
}

fn send(url: &Url, method: &str, body: Option<&str>) -> Result<String, Error> {
    let mut stream = connect(url)?;

    let mut request = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: openwrite\r\nAccept: application/json\r\n",
        url.path,
        url.authority()
    );
    // No keep-alive: one request per connection means the reply ends when the
    // server closes, which is one fewer thing that can go wrong.
    request.push_str("Connection: close\r\n");
    if let Some(body) = body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    if let Some(body) = body {
        request.push_str(body);
    }

    stream.write_all(request.as_bytes()).map_err(Error::Io)?;
    stream.flush().map_err(Error::Io)?;

    let mut raw = Vec::new();
    read_capped(&mut stream, &mut raw)?;
    let reply = split_reply(&raw)?;

    let body = if reply
        .header("transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        dechunk(reply.body)?
    } else {
        reply.body.to_vec()
    };
    let body = String::from_utf8_lossy(&body).into_owned();

    if (200..300).contains(&reply.status) {
        Ok(body)
    } else {
        Err(Error::Status { code: reply.status, body })
    }
}

fn connect(url: &Url) -> Result<TcpStream, Error> {
    let authority = url.authority();
    let addresses: Vec<_> = authority
        .to_socket_addrs()
        .map_err(|err| Error::Connect(format!("{authority} is not an address ({err})")))?
        .collect();
    let Some(last) = addresses.last().copied() else {
        return Err(Error::Connect(format!("{authority} resolved to nothing")));
    };

    // Try each address the name resolved to; report the last failure. A host
    // that answers on IPv6 but not IPv4, or the other way round, is common
    // enough on `localhost` that giving up on the first one would be wrong.
    let mut last_error = None;
    for address in &addresses {
        match TcpStream::connect_timeout(address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(Error::Io)?;
                stream.set_write_timeout(Some(CONNECT_TIMEOUT)).map_err(Error::Io)?;
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(err) => last_error = Some(err),
        }
    }
    Err(Error::Connect(format!(
        "nothing is listening on {last} ({})",
        last_error.map(|e| e.to_string()).unwrap_or_default()
    )))
}

fn read_capped(stream: &mut TcpStream, out: &mut Vec<u8>) -> Result<(), Error> {
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                if out.len() + n > MAX_BODY {
                    return Err(Error::Malformed("the reply was far too long".to_string()));
                }
                out.extend_from_slice(&buffer[..n]);
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(Error::Io(err)),
        }
    }
}

/// A reply, taken apart.
struct Reply<'a> {
    status: u16,
    /// Header names lower-cased, since HTTP does not care and servers differ.
    headers: Vec<(String, String)>,
    body: &'a [u8],
}

impl Reply<'_> {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Split a raw reply into its status code, its headers, and its body.
fn split_reply(raw: &[u8]) -> Result<Reply<'_>, Error> {
    let end = find(raw, b"\r\n\r\n")
        .map(|i| (i, i + 4))
        .or_else(|| find(raw, b"\n\n").map(|i| (i, i + 2)))
        .ok_or_else(|| Error::Malformed("the reply had no headers".to_string()))?;
    let head = String::from_utf8_lossy(&raw[..end.0]);
    let mut lines = head.lines();

    let status_line = lines
        .next()
        .ok_or_else(|| Error::Malformed("the reply was empty".to_string()))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| Error::Malformed(format!("{status_line:?} is not a status line")))?;

    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();

    Ok(Reply { status, headers, body: &raw[end.1..] })
}

/// Undo `Transfer-Encoding: chunked`, which is how a model server that does not
/// know the length in advance sends a reply.
fn dechunk(body: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(body.len());
    let mut rest = body;
    loop {
        let Some(line_end) = find(rest, b"\r\n") else {
            // A truncated stream: keep what arrived rather than lose the lot.
            return Ok(out);
        };
        let header = String::from_utf8_lossy(&rest[..line_end]);
        // A chunk header may carry extensions after a semicolon.
        let size_text = header.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| Error::Malformed(format!("{size_text:?} is not a chunk length")))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if size > rest.len() {
            out.extend_from_slice(rest);
            return Ok(out);
        }
        out.extend_from_slice(&rest[..size]);
        // Skip the CRLF that closes the chunk.
        rest = &rest[(size + 2).min(rest.len())..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_filled_in_from_what_was_typed() {
        assert_eq!(
            url("http://127.0.0.1:11434/api/generate").unwrap(),
            Url { host: "127.0.0.1".into(), port: 11434, path: "/api/generate".into() }
        );
        // A bare host and port is what a server prints when it starts.
        assert_eq!(
            url("localhost:11434").unwrap(),
            Url { host: "localhost".into(), port: 11434, path: "/".into() }
        );
        assert_eq!(url("http://example").unwrap().port, 80);
    }

    #[test]
    fn an_ipv6_literal_keeps_its_brackets_and_loses_its_port() {
        let parsed = url("http://[::1]:11434/api/tags").unwrap();
        assert_eq!(parsed.host, "[::1]");
        assert_eq!(parsed.port, 11434);
        assert_eq!(parsed.path, "/api/tags");
        assert!(parsed.is_loopback());
    }

    #[test]
    fn a_path_can_be_swapped_without_losing_the_host() {
        let base = url("http://127.0.0.1:11434").unwrap();
        assert_eq!(base.with_path("/api/tags").to_string(), "http://127.0.0.1:11434/api/tags");
        assert_eq!(base.with_path("api/tags").path, "/api/tags");
    }

    #[test]
    fn https_is_refused_rather_than_quietly_downgraded() {
        let err = url("https://api.example.com/v1").unwrap_err();
        assert!(matches!(err, Error::Url(_)));
        assert!(err.to_string().contains("plain http"));
    }

    #[test]
    fn a_line_break_in_an_address_is_refused_rather_than_written_into_a_request() {
        // Anything that reaches a `Host:` header or a request line unescaped
        // could end the line and add headers of its own.
        for bad in [
            "http://127.0.0.1:11434/\r\nX-Injected: yes",
            "http://127.0.0.1\nevil:11434",
            "http://127.0.0.1:11434/a b",
            "http://127.0.0.1:11434/\u{0}",
        ] {
            assert!(url(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn an_empty_address_is_an_error() {
        assert!(url("   ").is_err());
        assert!(url("http://").is_err());
    }

    #[test]
    fn a_machine_elsewhere_is_not_loopback() {
        assert!(url("http://127.0.0.1:11434").unwrap().is_loopback());
        assert!(url("localhost:11434").unwrap().is_loopback());
        assert!(!url("http://192.168.1.5:11434").unwrap().is_loopback());
    }

    #[test]
    fn a_reply_splits_into_status_headers_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        let reply = split_reply(raw).unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.header("content-type"), Some("application/json"));
        assert_eq!(reply.body, b"{\"a\":1}");
    }

    #[test]
    fn a_chunked_body_is_put_back_together() {
        let body = b"1a\r\n{\"response\":\"She leaves.\"}\r\n0\r\n\r\n";
        assert_eq!(dechunk(body).unwrap(), b"{\"response\":\"She leaves.\"}");
    }

    #[test]
    fn a_chunk_extension_does_not_confuse_the_length() {
        let body = b"5;name=value\r\nhello\r\n0\r\n\r\n";
        assert_eq!(dechunk(body).unwrap(), b"hello");
    }

    #[test]
    fn a_reply_cut_off_mid_chunk_keeps_what_arrived() {
        let body = b"10\r\nhalf a chunk";
        assert_eq!(dechunk(body).unwrap(), b"half a chunk");
    }

    #[test]
    fn a_reply_that_is_not_http_is_an_error_rather_than_a_panic() {
        assert!(split_reply(b"").is_err());
        assert!(split_reply(b"not http at all").is_err());
        assert!(split_reply(b"HTTP/1.1\r\n\r\n").is_err());
    }
}
