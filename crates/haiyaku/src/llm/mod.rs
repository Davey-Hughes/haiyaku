//! The model backends: each is given a system prompt, a user prompt and a JSON schema, and answers with one
//! JSON object that should fit the schema. [`backend`] builds one from a spec such as `ollama:gemma3`; a
//! [`Replay`] gives out recorded replies, so no test calls a model.

mod claude;
mod http;
mod replay;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

pub use http::ollama_url;
pub use replay::Replay;

use crate::{Error, Result};

/// What a backend is asked.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub system: &'a str,
    pub user: &'a str,
    /// The JSON schema the reply should fit.
    pub schema: &'a Value,
}

/// What a backend answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The reply parsed: always a JSON object, though nothing here checks it against the schema.
    pub json: Value,
    /// The reply as the model wrote it.
    pub raw: String,
    /// The wall-clock time of the call, 0 for a replay.
    pub seconds: f64,
    /// What the call cost, where the backend says.
    pub cost_usd: Option<f64>,
}

/// A model that answers a [`Request`].
pub trait Backend: std::fmt::Debug {
    /// `claude`, `ollama`, `openai` or `replay`: what [`BookRecord::backend`](crate::log::BookRecord::backend) records.
    fn name(&self) -> &str;
    /// The model the spec named, or `none` for a replay: what
    /// [`BookRecord::model`](crate::log::BookRecord::model) records.
    fn model(&self) -> &str;
    /// One JSON object that should fit the schema. Any failure is [`Error::Model`].
    fn call(&self, request: &Request<'_>) -> Result<Reply>;
}

/// What the backends take from the program that runs them. The library reads no environment variable: a
/// program that honours `OLLAMA_HOST` passes it in as `ollama_host`.
#[derive(Debug, Clone)]
pub struct Settings {
    /// How long one call may take, from start to finish.
    pub timeout: Duration,
    /// Where Ollama listens, in any form [`ollama_url`] takes; `None` for Ollama's default.
    pub ollama_host: Option<String>,
    /// The program the Claude backend runs: a bare name is found on the `PATH`, and a relative path is read
    /// from the working directory when the backend is built. A wrapper script must `exec` the program: a timeout
    /// kills only the process started, and a program the wrapper ran as its child would go on, and bill, after
    /// the call has given up.
    pub claude_program: PathBuf,
}

impl Default for Settings {
    /// Ten minutes a call, Ollama at its default address, and `claude` found on the `PATH`.
    fn default() -> Self {
        Settings { timeout: Duration::from_secs(600), ollama_host: None, claude_program: PathBuf::from("claude") }
    }
}

/// The backend a spec names: `claude` (Sonnet), `claude:<model>`, `ollama:<model>`, `openai:<base url>#<model>`
/// or `replay:<dir>` (a [`Replay`] of the directory's files). An OpenAI-compatible server's base URL is the
/// one its API's paths start from, as in `http://127.0.0.1:8000/v1`.
pub fn backend(spec: &str, settings: &Settings) -> Result<Box<dyn Backend>> {
    let (scheme, rest) = match spec.split_once(':') {
        None if spec == "claude" => ("claude", "sonnet"),
        parts => parts.unwrap_or((spec, "")),
    };
    match scheme {
        "claude" => Ok(Box::new(claude::Claude::new(
            settings.claude_program.clone(),
            named(spec, rest, "model")?,
            settings.timeout,
        )?)),
        "ollama" => {
            let url = ollama_url(settings.ollama_host.as_deref())?;
            Ok(Box::new(http::Server::ollama(url, named(spec, rest, "model")?, settings.timeout)))
        }
        "openai" => {
            let Some((base, model)) = rest.split_once('#') else {
                return Err(Error::Invalid(format!(
                    "the backend {spec:?} names no model: write openai:<base url>#<model>"
                )));
            };
            Ok(Box::new(http::Server::openai(base, named(spec, model, "model")?, settings.timeout)?))
        }
        "replay" => Ok(Box::new(Replay::from_dir(Path::new(named(spec, rest, "directory")?))?)),
        _ => Err(Error::Invalid(format!(
            "unknown backend {spec:?}: use claude, claude:<model>, ollama:<model>, openai:<base url>#<model> or \
             replay:<dir>"
        ))),
    }
}

/// `value`, the part of `spec` after its scheme, unless it is empty.
fn named<'a>(spec: &str, value: &'a str, what: &str) -> Result<&'a str> {
    if value.is_empty() { Err(Error::Invalid(format!("the backend {spec:?} names no {what}"))) } else { Ok(value) }
}

/// A model's reply text as the JSON object it should be, or why it is not one.
fn json_object(raw: &str) -> std::result::Result<Value, String> {
    match serde_json::from_str(raw) {
        Ok(json @ Value::Object(_)) => Ok(json),
        Ok(_) => Err("the reply is JSON but not an object".into()),
        Err(e) => Err(format!("the reply is not JSON: {e}")),
    }
}

/// How many characters of what a program or a server sent an error shows: enough to say what it was without
/// filling a terminal.
const SHOWN: usize = 300;

/// The start of what a program or a server sent.
fn excerpt(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(SHOWN) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.into(),
    }
}

/// The end of a program's stderr, where it says why it stopped.
fn ending(text: &str) -> String {
    match text.char_indices().rev().nth(SHOWN - 1) {
        Some((start, _)) if start > 0 => format!("...{}", &text[start..]),
        _ => text.into(),
    }
}
