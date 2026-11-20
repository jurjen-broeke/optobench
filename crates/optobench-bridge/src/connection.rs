//! One connection to SolidCraft's control channel: requests answered in order, and the
//! events that arrive between them.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::BridgeError;
use crate::discovery::Endpoint;

/// How long a request may take: SolidCraft answers every request within 60 s, with a
/// "still running" reply at worst.
const REQUEST_TIME: Duration = Duration::from_secs(70);

/// An event the app sent without being asked (`{"event", "params"}`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Its name, such as `design.changed`.
    pub name: String,
    /// What it carries.
    pub params: Value,
}

/// A connection. The first request carries the token; later ones need not.
pub struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    token: Option<String>,
    next_id: u64,
    events: VecDeque<Event>,
    /// A line read in part before a timeout, completed by the next read.
    partial: String,
    /// The folders file paths may resolve in, sent with every request.
    allowed: Vec<PathBuf>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Connection {{ next_id: {} }}", self.next_id)
    }
}

impl Connection {
    /// Connects to `endpoint` within `timeout`.
    pub fn open(endpoint: &Endpoint, timeout: Duration) -> Result<Self, BridgeError> {
        let stream = TcpStream::connect_timeout(&endpoint.address(), timeout)
            .map_err(|e| BridgeError::Connection(format!("{endpoint}: {e}")))?;
        stream
            .set_nodelay(true)
            .map_err(|e| BridgeError::Connection(e.to_string()))?;
        let writer = stream
            .try_clone()
            .map_err(|e| BridgeError::Connection(e.to_string()))?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            token: Some(endpoint.token().to_owned()),
            next_id: 1,
            events: VecDeque::new(),
            partial: String::new(),
            allowed: Vec::new(),
        })
    }

    /// The folders file paths in requests must resolve inside (SolidCraft's
    /// `allow_paths`); none by default, which SolidCraft takes as no limit.
    pub fn allow_paths(&mut self, folders: Vec<PathBuf>) {
        self.allowed = folders;
    }

    /// Sends `method` with `params` and waits for its reply's `result`. Events that arrive
    /// meanwhile are kept for [`Connection::next_event`].
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, BridgeError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({ "id": id, "method": method, "params": params });
        if let Some(token) = self.token.take() {
            request["token"] = json!(token);
        }
        if !self.allowed.is_empty() {
            request["allow_paths"] = json!(
                self.allowed
                    .iter()
                    .map(|p| p.to_string_lossy())
                    .collect::<Vec<_>>()
            );
        }
        let mut line = request.to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| BridgeError::Connection(e.to_string()))?;
        let deadline = Instant::now() + REQUEST_TIME;
        loop {
            let Some(reply) = self.read_message(deadline)? else {
                return Err(BridgeError::Connection(format!(
                    "no reply to `{method}` in {} s",
                    REQUEST_TIME.as_secs()
                )));
            };
            match reply.get("id") {
                Some(got) if got == &json!(id) => return answer(method, reply),
                Some(_) => continue,
                None => self.keep_event(reply),
            }
        }
    }

    /// Runs one engine command (`engine.execute`).
    pub fn execute(&mut self, command: &str, params: Value) -> Result<Value, BridgeError> {
        self.call(
            "engine.execute",
            json!({ "command": command, "params": params }),
        )
    }

    /// The next event, waiting at most `timeout`; `None` when none came.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<Event>, BridgeError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let deadline = Instant::now() + timeout;
        while let Some(message) = self.read_message(deadline)? {
            if message.get("id").is_none() {
                self.keep_event(message);
                return Ok(self.events.pop_front());
            }
        }
        Ok(None)
    }

    fn keep_event(&mut self, message: Value) {
        if let Some(name) = message["event"].as_str() {
            self.events.push_back(Event {
                name: name.to_owned(),
                params: message["params"].clone(),
            });
        }
    }

    /// The next whole line as JSON, or `None` at the deadline.
    fn read_message(&mut self, deadline: Instant) -> Result<Option<Value>, BridgeError> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.reader
                .get_ref()
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))
                .map_err(|e| BridgeError::Connection(e.to_string()))?;
            match self.reader.read_line(&mut self.partial) {
                Ok(0) => {
                    return Err(BridgeError::Connection(
                        "SolidCraft closed the connection".into(),
                    ));
                }
                Ok(_) if self.partial.ends_with('\n') => {
                    let line = std::mem::take(&mut self.partial);
                    if line.trim().is_empty() {
                        continue;
                    }
                    return serde_json::from_str(line.trim())
                        .map(Some)
                        .map_err(|e| BridgeError::Protocol(format!("not JSON: {e}")));
                }
                Ok(_) => continue,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue;
                }
                Err(e) => return Err(BridgeError::Connection(e.to_string())),
            }
        }
    }
}

/// A reply's result, or what its error means.
fn answer(method: &str, reply: Value) -> Result<Value, BridgeError> {
    if reply["ok"] == true {
        return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
    }
    let text = reply["error"]
        .as_str()
        .unwrap_or("no reason given")
        .to_owned();
    Err(if text.starts_with("unauthorized") {
        BridgeError::Unauthorized
    } else if reply["busy"] == true {
        BridgeError::Busy(text)
    } else if reply["running"] == true {
        BridgeError::StillRunning(text)
    } else {
        BridgeError::Refused(format!("{method}: {text}"))
    })
}
