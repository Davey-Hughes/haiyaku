//! The two HTTP APIs of the model servers: Ollama's own, and the OpenAI-compatible one that vLLM, llama.cpp's
//! server and others serve. Plain HTTP only: no TLS is built in, so an `https://` URL is refused when the
//! backend is built rather than failing at its first call.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Backend, Reply, Request, excerpt, json_object};
use crate::{Error, Result};

/// The port Ollama listens on unless told otherwise.
const OLLAMA_PORT: u16 = 11434;
/// Ollama's context window, in tokens. Its default can be shorter than a long chapter, and Ollama drops a
/// prompt's overflow without an error.
const OLLAMA_CONTEXT: u32 = 32768;
/// The longest reply asked for, in tokens. A reply cut off at this length is not whole JSON.
const MAX_REPLY: u32 = 8192;

/// The base URL of the Ollama at `host`, which takes `OLLAMA_HOST`'s forms: `host`, `host:port` or an `http://`
/// URL. `None` or empty is Ollama's default address. Without a port, a host with no scheme gets Ollama's port
/// and an `http://` URL gets 80, as Ollama's own client reads them.
pub fn ollama_url(host: Option<&str>) -> Result<String> {
    let host = host.unwrap_or_default().trim();
    if host.is_empty() {
        return Ok(format!("http://127.0.0.1:{OLLAMA_PORT}"));
    }
    let (rest, default_port) = match without_http(host)? {
        Some(rest) => (rest, 80),
        None => (host, OLLAMA_PORT),
    };
    let (authority, path) = authority_and_path(rest.trim_end_matches('/'));
    if authority.is_empty() {
        return Err(Error::Invalid(format!("the Ollama host {host:?} names no host")));
    }
    // More than one colon without brackets is an IPv6 address alone: a URL needs the brackets, and none of its
    // colons stands before a port.
    let authority = if !authority.starts_with('[') && authority.matches(':').count() > 1 {
        format!("[{authority}]")
    } else {
        authority.into()
    };
    let has_port = match authority.strip_prefix('[') {
        Some(ipv6) => ipv6.contains("]:"),
        None => authority.contains(':'),
    };
    let port = if has_port { String::new() } else { format!(":{default_port}") };
    Ok(format!("http://{authority}{port}{path}"))
}

/// What follows a URL's scheme, split where the path starts: the host with any port, and the path.
fn authority_and_path(rest: &str) -> (&str, &str) {
    rest.split_at(rest.find('/').unwrap_or(rest.len()))
}

/// What follows `url`'s `http://`, or `None` when it has no scheme. Any other scheme is [`Error::Invalid`].
fn without_http(url: &str) -> Result<Option<&str>> {
    match url.split_once("://") {
        None => Ok(None),
        Some(("http", rest)) => Ok(Some(rest)),
        Some(("https", _)) => Err(Error::Invalid(format!("{url}: TLS is not built in, so only http:// URLs work"))),
        Some(_) => Err(Error::Invalid(format!("{url}: only http:// URLs work"))),
    }
}

/// The API a [`Server`] speaks.
#[derive(Debug, Clone, Copy)]
enum Api {
    Ollama,
    OpenAi,
}

/// A model on a server, reached over HTTP.
#[derive(Debug)]
pub(super) struct Server {
    api: Api,
    /// The URL the API's paths start from, without a trailing `/`.
    base: String,
    model: String,
    timeout: Duration,
    agent: ureq::Agent,
}

impl Server {
    /// `model` on the Ollama at `url`, as [`ollama_url`] gives it.
    pub(super) fn ollama(url: String, model: &str, timeout: Duration) -> Server {
        Server::new(Api::Ollama, url, model, timeout)
    }

    /// `model` on the OpenAI-compatible server whose API starts at `base`.
    pub(super) fn openai(base: &str, model: &str, timeout: Duration) -> Result<Server> {
        let base = base.trim_end_matches('/');
        match without_http(base)? {
            Some(rest) if !authority_and_path(rest).0.is_empty() => {
                Ok(Server::new(Api::OpenAi, base.into(), model, timeout))
            }
            _ => Err(Error::Invalid(format!(
                "the base URL {base:?} is not an http:// URL, as in http://127.0.0.1:8000/v1"
            ))),
        }
    }

    fn new(api: Api, base: String, model: &str, timeout: Duration) -> Server {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // An error status comes back as a response, so the message can carry what the server said about it.
            .http_status_as_error(false)
            // ureq's default proxy comes from the environment, which the library leaves to its caller.
            .proxy(None)
            .build()
            .into();
        Server { api, base, model: model.into(), timeout, agent }
    }

    /// The request body. Neither API is sent a temperature, so the server's own sampling applies: under
    /// greedy decoding, a reply that looped would loop again on the retry.
    fn body(&self, request: &Request<'_>) -> Value {
        let messages = json!([
            {"role": "system", "content": request.system},
            {"role": "user", "content": request.user},
        ]);
        match self.api {
            Api::Ollama => json!({
                "model": self.model,
                "messages": messages,
                "stream": false,
                "format": request.schema,
                "think": false,
                "options": {"num_ctx": OLLAMA_CONTEXT, "num_predict": MAX_REPLY},
            }),
            Api::OpenAi => json!({
                "model": self.model,
                "messages": messages,
                "max_tokens": MAX_REPLY,
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {"name": "events", "schema": request.schema, "strict": true},
                },
            }),
        }
    }

    fn endpoint(&self) -> String {
        let path = match self.api {
            Api::Ollama => "/api/chat",
            Api::OpenAi => "/chat/completions",
        };
        format!("{}{path}", self.base)
    }

    /// The server's reply to `body` at `url`, parsed as JSON, or why there is none.
    fn post(&self, url: &str, body: &Value) -> std::result::Result<Value, String> {
        let mut response = self.agent.post(url).send_json(body).map_err(|e| self.cause(e))?;
        let status = response.status();
        let text = response.body_mut().read_to_string();
        if !status.is_success() {
            return Err(match text {
                Ok(said) if !said.trim().is_empty() => format!("HTTP {status}: {}", excerpt(&said)),
                _ => format!("HTTP {status}"),
            });
        }
        let text = text.map_err(|e| self.cause(e))?;
        serde_json::from_str(&text).map_err(|e| format!("the server's reply is not JSON ({e}): {}", excerpt(&text)))
    }

    fn cause(&self, error: ureq::Error) -> String {
        match error {
            ureq::Error::Timeout(_) => format!("timed out after {:?}", self.timeout),
            other => other.to_string(),
        }
    }

    /// The model's text out of the server's reply, unless the reply was cut off.
    fn content<'v>(&self, reply: &'v Value) -> std::result::Result<&'v str, String> {
        let (content, finish, field) = match self.api {
            Api::Ollama => (&reply["message"]["content"], &reply["done_reason"], "message.content"),
            Api::OpenAi => {
                let choice = &reply["choices"][0];
                (&choice["message"]["content"], &choice["finish_reason"], "choices[0].message.content")
            }
        };
        if finish.as_str() == Some("length") {
            return Err(format!("the reply was cut off at {MAX_REPLY} tokens"));
        }
        content.as_str().ok_or_else(|| format!("the server's reply has no {field}: {}", excerpt(&reply.to_string())))
    }
}

impl Backend for Server {
    fn name(&self) -> &str {
        match self.api {
            Api::Ollama => "ollama",
            Api::OpenAi => "openai",
        }
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn call(&self, request: &Request<'_>) -> Result<Reply> {
        let start = Instant::now();
        let url = self.endpoint();
        let fail = |cause: String| Error::Model(format!("{} at {url}: {cause}", self.name()));
        let reply = self.post(&url, &self.body(request)).map_err(fail)?;
        let raw = self.content(&reply).map_err(fail)?;
        let json = json_object(raw).map_err(fail)?;
        Ok(Reply { json, raw: raw.into(), seconds: start.elapsed().as_secs_f64(), cost_usd: None })
    }
}
