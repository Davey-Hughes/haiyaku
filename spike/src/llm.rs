//! Backends: Ollama's native API (structured output, thinking off), any
//! OpenAI-compatible server, and the `claude` CLI. Adapted from subcheck.

use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub enum Backend {
    Ollama { url: String, model: String },
    OpenAi { url: String, model: String },
    Claude { model: String },
}

pub struct Reply {
    pub json: Value,
    pub raw: String,
    pub seconds: f64,
    pub prompt_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

/// Room for a long chapter plus the roster; a cut-off reply is invalid JSON.
const NUM_CTX: u64 = 32768;
const MAX_OUTPUT: u64 = 8192;

impl Backend {
    /// `ollama:<model>`, `openai:<base url>#<model>`, `claude` or `claude:<model>`.
    pub fn parse(s: &str) -> Result<Backend> {
        if let Some(model) = s.strip_prefix("ollama:") {
            let url = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".into());
            return Ok(Backend::Ollama { url, model: model.into() });
        }
        if let Some(rest) = s.strip_prefix("openai:") {
            let (url, model) = rest.split_once('#').unwrap_or((rest, ""));
            return Ok(Backend::OpenAi { url: url.trim_end_matches('/').into(), model: model.into() });
        }
        if s == "claude" {
            return Ok(Backend::Claude { model: "sonnet".into() });
        }
        if let Some(model) = s.strip_prefix("claude:") {
            return Ok(Backend::Claude { model: model.into() });
        }
        bail!("unknown backend {s:?}: use ollama:<model>, openai:<url>#<model>, claude or claude:<model>")
    }

    pub fn call(&self, system: &str, user: &str, schema: &Value) -> Result<Reply> {
        let start = Instant::now();
        let mut reply = match self {
            Backend::Ollama { url, model } => ollama(url, model, system, user, schema)?,
            Backend::OpenAi { url, model } => openai(url, model, system, user, schema)?,
            Backend::Claude { model } => claude(model, system, user, schema)?,
        };
        reply.seconds = start.elapsed().as_secs_f64();
        Ok(reply)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(1800))).build().into()
}

fn ollama(url: &str, model: &str, system: &str, user: &str, schema: &Value) -> Result<Reply> {
    let body = json!({
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "stream": false,
        "format": schema,
        "think": false,
        // The model's own sampling defaults: greedy decoding under a JSON
        // schema can repeat one array item until the output limit.
        "options": {"num_ctx": NUM_CTX, "num_predict": MAX_OUTPUT},
    });
    let v: Value = agent().post(format!("{url}/api/chat")).send_json(&body)?.body_mut().read_json()?;
    let raw = v["message"]["content"].as_str().context("Ollama reply has no message content")?.to_string();
    if v["done_reason"] == "length" {
        let tail: String = raw.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        bail!("reply cut off at {MAX_OUTPUT} tokens, ending: {tail}");
    }
    Ok(Reply {
        json: serde_json::from_str(&raw).context("reply is not JSON")?,
        raw,
        seconds: 0.0,
        prompt_tokens: v["prompt_eval_count"].as_u64(),
        output_tokens: v["eval_count"].as_u64(),
        cost_usd: None,
    })
}

fn openai(url: &str, model: &str, system: &str, user: &str, schema: &Value) -> Result<Reply> {
    let body = json!({
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "temperature": 0,
        "max_tokens": MAX_OUTPUT,
        "response_format": {"type": "json_schema", "json_schema": {"name": "events", "schema": schema, "strict": true}},
        "chat_template_kwargs": {"enable_thinking": false},
    });
    let v: Value = agent().post(format!("{url}/chat/completions")).send_json(&body)?.body_mut().read_json()?;
    let choice = &v["choices"][0];
    if choice["finish_reason"] == "length" {
        bail!("reply cut off at {MAX_OUTPUT} tokens");
    }
    let raw = choice["message"]["content"].as_str().context("reply has no message content")?.to_string();
    Ok(Reply {
        json: serde_json::from_str(&raw).context("reply is not JSON")?,
        raw,
        seconds: 0.0,
        prompt_tokens: v["usage"]["prompt_tokens"].as_u64(),
        output_tokens: v["usage"]["completion_tokens"].as_u64(),
        cost_usd: None,
    })
}

fn claude(model: &str, system: &str, user: &str, schema: &Value) -> Result<Reply> {
    // Without these flags a headless call loads the user's hooks, skills,
    // CLAUDE.md and memory into its context. The root directory holds no
    // CLAUDE.md for it to find either.
    let mut child = Command::new("claude")
        .args(["-p", "--model", model, "--output-format", "json", "--tools", "", "--setting-sources", ""])
        .args(["--disable-slash-commands", "--strict-mcp-config", "--no-session-persistence"])
        .args(["--system-prompt", system, "--json-schema", &schema.to_string()])
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running claude")?;
    child.stdin.take().context("claude stdin")?.write_all(user.as_bytes())?;
    let out = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(&stdout).with_context(|| {
        format!("claude printed no JSON (exit {:?}): {}", out.status.code(), String::from_utf8_lossy(&out.stderr))
    })?;
    if v["is_error"] == true {
        bail!("claude: {}", v["result"]);
    }
    let usage = &v["usage"];
    let input = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
        .iter()
        .filter_map(|k| usage[*k].as_u64())
        .sum::<u64>();
    let json = match &v["structured_output"] {
        Value::Null => serde_json::from_str(v["result"].as_str().unwrap_or("")).context("reply is not JSON")?,
        s => s.clone(),
    };
    Ok(Reply {
        raw: json.to_string(),
        json,
        seconds: 0.0,
        prompt_tokens: Some(input),
        output_tokens: usage["output_tokens"].as_u64(),
        cost_usd: v["total_cost_usd"].as_f64(),
    })
}
