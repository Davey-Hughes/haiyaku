//! The Claude CLI as a backend. A call runs the program once in print mode: the user prompt goes in on its stdin,
//! and its stdout is one JSON object that carries the model's reply and what the call cost.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Backend, Reply, Request, ending, excerpt, json_object};
use crate::{Error, Result};

/// How long a call sleeps between checks that the program has exited: nothing beside a reply that takes seconds.
const POLL: Duration = Duration::from_millis(10);
/// How long the pipes of a program that has exited by itself may stay open past the deadline. It has answered by
/// then, and a process it started can hold them a moment longer; a call that waited only for the time left would
/// count a program that exited just before the deadline as timed out.
const GRACE: Duration = Duration::from_secs(1);

/// A Claude model, reached through the `claude` program.
#[derive(Debug)]
pub(super) struct Claude {
    program: PathBuf,
    model: String,
    timeout: Duration,
}

/// What the program left when it exited.
struct Exit {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Claude {
    /// `model` through `program`, each call killed after `timeout`. A relative path of more than one component is
    /// made absolute from the working directory: the program runs in `/`, where the path would name another file
    /// or none. A bare name stays a name the `PATH` finds. [`Error::Io`] when the working directory cannot be
    /// read.
    pub(super) fn new(program: PathBuf, model: &str, timeout: Duration) -> Result<Claude> {
        let program = if program.components().count() > 1 { std::path::absolute(&program)? } else { program };
        Ok(Claude { program, model: model.into(), timeout })
    }

    /// The program started on `request`, its three streams piped.
    fn start(&self, request: &Request<'_>) -> io::Result<Child> {
        let schema = request.schema.to_string();
        Command::new(&self.program)
            // Without these flags the program loads the user's settings, hooks, tools, MCP servers and memory into
            // the call and keeps a session file of it; `/` holds no CLAUDE.md for it to find either. The model
            // reads the two prompts and nothing else.
            .args(["-p", "--model", &self.model, "--output-format", "json", "--tools", "", "--setting-sources", ""])
            .args(["--disable-slash-commands", "--strict-mcp-config", "--no-session-persistence"])
            .args(["--system-prompt", request.system, "--json-schema", &schema])
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }

    /// The program run on `request` until it exits, or killed and reaped once the timeout has passed.
    fn run(&self, request: &Request<'_>) -> std::result::Result<Exit, String> {
        // A timeout too long to add to the clock never runs out.
        let deadline = Instant::now().checked_add(self.timeout);
        let left = || until(deadline);
        let mut child = self.start(request).map_err(|e| format!("cannot start it: {e}"))?;
        // A chapter is larger than a pipe's buffer, and the program may write before it has read all of it, so the
        // prompt goes in from a thread of its own while this one waits. The thread is never joined, so a write that
        // blocks or fails cannot hold up the call: the exit status and stdout decide it.
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let user = request.user.to_owned();
        thread::spawn(move || {
            let _ = stdin.write_all(user.as_bytes());
        });
        let stdout = drain(child.stdout.take().expect("stdout is piped"));
        let stderr = drain(child.stderr.take().expect("stderr is piped"));
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if left() > Duration::ZERO => thread::sleep(POLL.min(left())),
                Ok(None) => {
                    stop(&mut child);
                    return Err(self.timed_out());
                }
                Err(e) => {
                    stop(&mut child);
                    return Err(format!("cannot wait for it: {e}"));
                }
            }
        };
        // A process the program started can hold its pipes open after it exits, so these waits are bounded too: by
        // the deadline, or by the grace when that is later.
        let collected_by = Instant::now().checked_add(left().max(GRACE));
        let stdout = self.collect(&stdout, until(collected_by), "stdout")?;
        let stderr = self.collect(&stderr, until(collected_by), "stderr")?;
        Ok(Exit { status, stdout, stderr })
    }

    /// The bytes `pipe` carried, once it has closed within `left`.
    fn collect(
        &self,
        pipe: &Receiver<io::Result<Vec<u8>>>,
        left: Duration,
        name: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        match pipe.recv_timeout(left) {
            Ok(read) => read.map_err(|e| format!("cannot read its {name}: {e}")),
            Err(RecvTimeoutError::Timeout) => Err(self.timed_out()),
            // The reading thread sends before it ends, so only a panic in it gets here.
            Err(RecvTimeoutError::Disconnected) => Err(format!("the thread reading its {name} stopped")),
        }
    }

    fn timed_out(&self) -> String {
        format!("timed out after {:?}", self.timeout)
    }
}

impl Backend for Claude {
    fn name(&self) -> &str {
        "claude"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn call(&self, request: &Request<'_>) -> Result<Reply> {
        let start = Instant::now();
        let fail = |cause: String| Error::Model(format!("claude ({}): {cause}", self.program.display()));
        let exit = self.run(request).map_err(fail)?;
        let (json, raw, cost_usd) = reply(&exit).map_err(fail)?;
        Ok(Reply { json, raw, seconds: start.elapsed().as_secs_f64(), cost_usd })
    }
}

/// The model's reply out of what the program printed: the JSON object, the text it came as, and what the call cost.
fn reply(exit: &Exit) -> std::result::Result<(Value, String, Option<f64>), String> {
    let stdout = String::from_utf8_lossy(&exit.stdout);
    let printed = printed_object(&stdout);
    // The program exits non-zero when it reports an error, and what it reported says more than its stderr.
    if let Ok(printed) = &printed
        && printed["is_error"] == true
    {
        let said = printed["result"].as_str().map_or_else(|| printed.to_string(), str::to_owned);
        return Err(format!("it reported an error: {}", excerpt(&said)));
    }
    if !exit.status.success() {
        let stderr = String::from_utf8_lossy(&exit.stderr);
        return Err(match stderr.trim() {
            "" => format!("it failed ({})", exit.status),
            said => format!("it failed ({}): {}", exit.status, ending(said)),
        });
    }
    let printed = printed?;
    let (json, raw) = match &printed["structured_output"] {
        Value::Null => {
            let raw = printed["result"].as_str().ok_or("its output has neither structured_output nor a result")?;
            (json_object(raw)?, raw.to_owned())
        }
        json @ Value::Object(_) => (json.clone(), json.to_string()),
        _ => return Err("its structured_output is not an object".into()),
    };
    Ok((json, raw, printed["total_cost_usd"].as_f64()))
}

/// The JSON object the program printed, or what it printed instead.
fn printed_object(stdout: &str) -> std::result::Result<Value, String> {
    match serde_json::from_str(stdout) {
        Ok(printed @ Value::Object(_)) => Ok(printed),
        _ if stdout.trim().is_empty() => Err("it printed nothing".into()),
        Ok(_) => Err(format!("it printed JSON that is not an object: {}", excerpt(stdout))),
        Err(e) => Err(format!("it printed something that is not JSON ({e}): {}", excerpt(stdout))),
    }
}

/// A thread that reads `pipe` to its end, and the channel the bytes come back on. Reading each pipe from its own
/// thread keeps the program from blocking on a full one while the call waits on the other.
fn drain(mut pipe: impl Read + Send + 'static) -> Receiver<io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = pipe.read_to_end(&mut bytes).map(|_| bytes);
        // The receiver is gone once the call has given up on the program, and then nothing wants the bytes.
        let _ = sender.send(read);
    });
    receiver
}

/// The time from now until `end`, which `None` puts past any wait.
fn until(end: Option<Instant>) -> Duration {
    end.map_or(Duration::MAX, |end| end.saturating_duration_since(Instant::now()))
}

/// Kills `child` and reaps it, so it leaves no zombie. Only the process started is killed: it stays in this
/// program's process group, where a Ctrl-C at the terminal reaches it too, so what it started is out of reach (see
/// [`Settings::claude_program`](super::Settings::claude_program)). An error from either means the child is gone
/// already, and there is nothing left to do.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
