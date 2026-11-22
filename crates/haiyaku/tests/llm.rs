//! The model backends against recorded replies and a one-shot server on 127.0.0.1: what each sends, what it
//! makes of the reply, and how each failure comes back.

#![cfg(feature = "extract")]

mod common;

use std::fmt::Debug;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use common::serve_once;
use haiyaku::llm::{Backend, Replay, Reply, Request, Settings, backend, ollama_url};
use haiyaku::{Error, Result};
use serde_json::{Value, json};
use tempfile::TempDir;

const SYSTEM: &str = "List the characters.";
const USER: &str = "The keeper climbed to the lamp.";

fn schema() -> Value {
    json!({"type": "object", "properties": {"characters": {"type": "array"}}, "required": ["characters"]})
}

fn ask(backend: &dyn Backend) -> Result<Reply> {
    backend.call(&Request { system: SYSTEM, user: USER, schema: &schema() })
}

fn messages() -> Value {
    json!([{"role": "system", "content": SYSTEM}, {"role": "user", "content": USER}])
}

fn model_error<T: Debug>(result: Result<T>) -> String {
    match result {
        Err(Error::Model(why)) => why,
        other => panic!("expected Error::Model, got {other:?}"),
    }
}

fn invalid<T: Debug>(result: Result<T>) -> String {
    match result {
        Err(Error::Invalid(why)) => why,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

/// Default settings, with Ollama at `url`.
fn ollama_at(url: &str) -> Settings {
    Settings { ollama_host: Some(url.into()), ..Settings::default() }
}

/// The request line and the JSON body of a request `serve_once` read.
fn line_and_body(request: &str) -> (&str, Value) {
    let (head, body) = request.split_once("\r\n\r\n").unwrap();
    (head.lines().next().unwrap(), serde_json::from_str(body).unwrap())
}

/// Ollama's reply to a call that is not streamed.
fn ollama_reply(content: &str, done_reason: &str) -> String {
    json!({
        "model": "gemma",
        "message": {"role": "assistant", "content": content},
        "done": true,
        "done_reason": done_reason,
    })
    .to_string()
}

/// An OpenAI-compatible server's reply.
fn openai_reply(content: &str, finish_reason: &str) -> String {
    json!({
        "object": "chat.completion",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": finish_reason}],
    })
    .to_string()
}

#[test]
fn replay_returns_replies_in_order_then_fails() {
    let replay = Replay::new(vec![r#"{"characters":["ada"]}"#.into(), r#"{"characters":[]}"#.into()]);
    assert_eq!((replay.name(), replay.model()), ("replay", "none"));
    assert_eq!(replay.remaining(), 2);
    let first = Reply {
        json: json!({"characters": ["ada"]}),
        raw: r#"{"characters":["ada"]}"#.into(),
        seconds: 0.0,
        cost_usd: None,
    };
    assert_eq!(ask(&replay).unwrap(), first);
    assert_eq!(replay.remaining(), 1);
    assert_eq!(ask(&replay).unwrap().json, json!({"characters": []}));
    assert_eq!(replay.remaining(), 0);
    let why = model_error(ask(&replay));
    assert!(why.contains("answering 2 calls"), "{why}");
    assert_eq!(replay.remaining(), 0);
}

#[test]
fn replay_from_dir_sorts_by_file_name() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("002.json"), r#"{"n":2}"#).unwrap();
    std::fs::write(dir.path().join("001.json"), r#"{"n":1}"#).unwrap();
    let replay = Replay::from_dir(dir.path()).unwrap();
    assert_eq!(replay.remaining(), 2);
    assert_eq!(ask(&replay).unwrap().json, json!({"n": 1}));
    assert_eq!(ask(&replay).unwrap().json, json!({"n": 2}));
}

#[test]
fn replay_from_dir_passes_over_hidden_files_and_backups() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".gitkeep"), "").unwrap();
    std::fs::write(dir.path().join(".001.json.swp"), "swap").unwrap();
    std::fs::write(dir.path().join("001.json~"), r#"{"n":0}"#).unwrap();
    std::fs::write(dir.path().join("001.json"), r#"{"n":1}"#).unwrap();
    let replay = Replay::from_dir(dir.path()).unwrap();
    assert_eq!(replay.remaining(), 1);
    assert_eq!(ask(&replay).unwrap().json, json!({"n": 1}));
}

#[test]
fn replay_from_dir_errors_name_the_path() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("missing");
    let io_error = |result: Result<Replay>| match result {
        Err(Error::Io(error)) => error.to_string(),
        other => panic!("expected Error::Io, got {other:?}"),
    };
    let why = io_error(Replay::from_dir(&missing));
    assert!(why.contains(&missing.display().to_string()), "{why}");
    let unreadable = dir.path().join("001.json");
    std::fs::write(&unreadable, [0xFF, 0xFE]).unwrap();
    let why = io_error(Replay::from_dir(dir.path()));
    assert!(why.contains(&unreadable.display().to_string()), "{why}");
}

#[test]
fn replay_reply_that_is_not_json_is_a_model_error() {
    let replay = Replay::new(vec!["The keeper and her gull.".into(), "[1, 2]".into(), r#"{"n":3}"#.into()]);
    let why = model_error(ask(&replay));
    assert!(why.contains("not JSON"), "{why}");
    let why = model_error(ask(&replay));
    assert!(why.contains("not an object"), "{why}");
    assert_eq!(ask(&replay).unwrap().json, json!({"n": 3}));
}

#[test]
fn spec_strings() {
    let dir = TempDir::new().unwrap();
    let settings = Settings::default();
    let named = |spec: &str| {
        let backend = backend(spec, &settings).unwrap();
        (backend.name().to_owned(), backend.model().to_owned())
    };
    assert_eq!(named("claude"), ("claude".into(), "sonnet".into()));
    assert_eq!(named("claude:opus"), ("claude".into(), "opus".into()));
    assert_eq!(named("ollama:gemma"), ("ollama".into(), "gemma".into()));
    assert_eq!(named("ollama:gemma3:27b"), ("ollama".into(), "gemma3:27b".into()));
    assert_eq!(named("openai:http://h:8000/v1/#m"), ("openai".into(), "m".into()));
    assert_eq!(named(&format!("replay:{}", dir.path().display())), ("replay".into(), "none".into()));
    let refused = [
        "",
        "gpt",
        "claude:",
        "ollama:",
        "openai:http://h",
        "openai:http://h#",
        "openai:h:8000#m",
        "openai:http:///v1#m",
    ];
    for spec in refused.into_iter().chain(["replay:"]) {
        invalid(backend(spec, &settings));
    }
    let why = invalid(backend("openai:https://h#m", &settings));
    assert!(why.contains("TLS"), "{why}");
}

#[test]
fn ollama_host_forms() {
    let default = "http://127.0.0.1:11434";
    assert_eq!(ollama_url(None).unwrap(), default);
    assert_eq!(ollama_url(Some("")).unwrap(), default);
    assert_eq!(ollama_url(Some("192.168.1.5")).unwrap(), "http://192.168.1.5:11434");
    assert_eq!(ollama_url(Some("0.0.0.0:8080")).unwrap(), "http://0.0.0.0:8080");
    assert_eq!(ollama_url(Some("http://box:1234/")).unwrap(), "http://box:1234");
    assert_eq!(ollama_url(Some("[::1]")).unwrap(), "http://[::1]:11434");
    assert_eq!(ollama_url(Some("[::1]:8080")).unwrap(), "http://[::1]:8080");
    // More than one colon and no brackets is an IPv6 address alone, as Ollama reads it.
    assert_eq!(ollama_url(Some("::1")).unwrap(), "http://[::1]:11434");
    assert_eq!(ollama_url(Some("2001:db8::5")).unwrap(), "http://[2001:db8::5]:11434");
    assert_eq!(ollama_url(Some("http://::1")).unwrap(), "http://[::1]:80");
    // As Ollama's own client reads them: a host with no scheme and no port is on Ollama's port, and an http://
    // URL with no port is on HTTP's.
    assert_eq!(ollama_url(Some("box")).unwrap(), "http://box:11434");
    assert_eq!(ollama_url(Some("box/ollama")).unwrap(), "http://box:11434/ollama");
    assert_eq!(ollama_url(Some("http://box")).unwrap(), "http://box:80");
    assert_eq!(ollama_url(Some("http://box/ollama/")).unwrap(), "http://box:80/ollama");
    let why = invalid(ollama_url(Some("https://box")));
    assert!(why.contains("TLS"), "{why}");
}

#[test]
fn ollama_round_trip() {
    let (url, server) = serve_once(200, &ollama_reply(r#"{"characters":["ada"]}"#, "stop"));
    let ollama = backend("ollama:gemma", &ollama_at(&url)).unwrap();
    assert_eq!((ollama.name(), ollama.model()), ("ollama", "gemma"));
    let reply = ask(&*ollama).unwrap();
    assert_eq!(reply.json, json!({"characters": ["ada"]}));
    assert_eq!(reply.raw, r#"{"characters":["ada"]}"#);
    assert!(reply.seconds > 0.0, "{}", reply.seconds);
    assert_eq!(reply.cost_usd, None);

    let request = server.join().unwrap();
    let (line, body) = line_and_body(&request);
    assert_eq!(line, "POST /api/chat HTTP/1.1");
    assert_eq!(body["stream"], json!(false));
    assert_eq!(body["format"], schema());
    assert!(body.get("temperature").is_none(), "{body}");
    let expected = json!({
        "model": "gemma",
        "messages": messages(),
        "stream": false,
        "format": schema(),
        "think": false,
        "options": {"num_ctx": 32768, "num_predict": 8192},
    });
    assert_eq!(body, expected);
}

#[test]
fn ollama_cut_off_reply_is_an_error() {
    let (url, server) = serve_once(200, &ollama_reply(r#"{"characters":["ada","ada","#, "length"));
    let why = model_error(ask(&*backend("ollama:gemma", &ollama_at(&url)).unwrap()));
    assert!(why.contains("cut off"), "{why}");
    server.join().unwrap();
}

#[test]
fn openai_round_trip() {
    let (url, server) = serve_once(200, &openai_reply(r#"{"characters":[]}"#, "stop"));
    let openai = backend(&format!("openai:{url}/v1/#m"), &Settings::default()).unwrap();
    let reply = ask(&*openai).unwrap();
    assert_eq!(reply.json, json!({"characters": []}));
    assert_eq!(reply.raw, r#"{"characters":[]}"#);
    assert_eq!(reply.cost_usd, None);

    let request = server.join().unwrap();
    let (line, body) = line_and_body(&request);
    assert_eq!(line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(body["response_format"]["json_schema"]["strict"], json!(true));
    assert!(body.get("temperature").is_none(), "{body}");
    let expected = json!({
        "model": "m",
        "messages": messages(),
        "max_tokens": 8192,
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "events", "schema": schema(), "strict": true},
        },
    });
    assert_eq!(body, expected);
}

#[test]
fn openai_cut_off_reply_is_an_error() {
    let (url, server) = serve_once(200, &openai_reply(r#"{"characters":["ada","#, "length"));
    let why = model_error(ask(&*backend(&format!("openai:{url}#m"), &Settings::default()).unwrap()));
    assert!(why.contains("cut off"), "{why}");
    server.join().unwrap();
}

#[test]
fn openai_reply_with_no_choices_is_a_model_error() {
    let (url, server) = serve_once(200, r#"{"object":"chat.completion","choices":[]}"#);
    let why = model_error(ask(&*backend(&format!("openai:{url}#m"), &Settings::default()).unwrap()));
    assert!(why.contains("choices[0].message.content"), "{why}");
    server.join().unwrap();
}

#[test]
fn reply_of_the_wrong_shape_is_a_model_error() {
    for (body, cause) in [
        ("<html>not found</html>", "not JSON"),
        (r#"{"error":"no such model"}"#, "message.content"),
        (&ollama_reply("[]", "stop"), "not an object"),
        (&ollama_reply("The keeper and her gull.", "stop"), "not JSON"),
    ] {
        let (url, server) = serve_once(200, body);
        let why = model_error(ask(&*backend("ollama:gemma", &ollama_at(&url)).unwrap()));
        assert!(why.contains(cause), "{body}: {why}");
        server.join().unwrap();
    }
}

#[test]
fn http_error_status_is_a_model_error() {
    let (url, server) = serve_once(500, "boom");
    let why = model_error(ask(&*backend("ollama:gemma", &ollama_at(&url)).unwrap()));
    for said in ["ollama", &url, "500", "boom"] {
        assert!(why.contains(said), "{why} does not say {said:?}");
    }
    server.join().unwrap();
}

#[test]
fn refused_connection_is_a_model_error() {
    // The listener is gone by the end of the statement, so nothing listens on the port it was given.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}");
    let settings = Settings { timeout: Duration::from_secs(5), ..ollama_at(&url) };
    let why = model_error(ask(&*backend("ollama:gemma", &settings).unwrap()));
    assert!(why.contains(&url), "{why}");
}

#[test]
fn silent_server_times_out() {
    // The kernel completes the connection into the listener's backlog, so the request goes out and no reply
    // ever comes back.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let settings = Settings { timeout: Duration::from_millis(200), ..ollama_at(&url) };
    let start = Instant::now();
    let why = model_error(ask(&*backend("ollama:gemma", &settings).unwrap()));
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
    assert!(why.contains("timed out"), "{why}");
    drop(listener);
}
