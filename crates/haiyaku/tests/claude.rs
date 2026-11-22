//! The Claude backend against fake `claude` programs: shell scripts that print what the real one prints, so no
//! test runs it. What a call passes the program, what it makes of the output, and how each failure comes back.

#![cfg(all(feature = "extract", unix))]

use std::fmt::Debug;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use haiyaku::llm::{Reply, Request, Settings, backend};
use haiyaku::{Error, Result};
use serde_json::{Value, json};
use tempfile::TempDir;

const SYSTEM: &str = "List the characters.\nName each one once.";
const USER: &str = "The keeper climbed to the lamp.\nPip shouted at the sea.";
/// What the real program prints for a call that worked, give or take fields the backend does not read.
const ANSWERED: &str =
    r#"{"type":"result","is_error":false,"structured_output":{"characters":[]},"total_cost_usd":0.5}"#;
/// Larger than a pipe's buffer, so a call that wrote the prompt before reading the output would deadlock.
const MIB: usize = 1 << 20;

/// Held by each test while it writes its program and runs it. A file open for writing in one thread is open in
/// every child another thread forks until that child execs, and running the file then fails with "Text file
/// busy"; taking turns keeps a script's writing and another test's fork apart.
static TURN: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A fake `claude`: an `sh` script in a directory of its own, where the script finds the directory as `$dir`.
struct Fake {
    dir: TempDir,
    _turn: MutexGuard<'static, ()>,
}

impl Fake {
    fn new(body: &str) -> Fake {
        Fake::new_in(&std::env::temp_dir(), body)
    }

    /// A fake in a directory of its own inside `parent`.
    fn new_in(parent: &Path, body: &str) -> Fake {
        let turn = turn();
        let dir = TempDir::new_in(parent).unwrap();
        let script = dir.path().join("claude");
        fs::write(&script, format!("#!/bin/sh\ndir={}\n{body}\n", quoted(&dir.path().display().to_string()))).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        Fake { dir, _turn: turn }
    }

    /// A fake that prints `stdout` and exits 0, without reading its stdin.
    fn printing(stdout: &str) -> Fake {
        Fake::new(&format!("printf '%s' {}", quoted(stdout)))
    }

    /// Default settings with this fake as the program.
    fn settings(&self) -> Settings {
        Settings { claude_program: self.dir.path().join("claude"), ..Settings::default() }
    }

    /// A file the script wrote into its directory.
    fn wrote(&self, name: &str) -> String {
        fs::read_to_string(self.dir.path().join(name)).unwrap()
    }
}

/// `text` as one word of `sh`, whatever quotes it holds.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The path that names `to` from the directory `from`, both absolute.
fn relative(from: &Path, to: &Path) -> PathBuf {
    let shared = from.components().zip(to.components()).take_while(|(from, to)| from == to).count();
    from.components().skip(shared).map(|_| Component::ParentDir).chain(to.components().skip(shared)).collect()
}

fn schema() -> Value {
    json!({"type": "object", "properties": {"characters": {"type": "array"}}, "required": ["characters"]})
}

/// The `claude` backend's reply to `user`, run as `settings` say.
fn ask(settings: &Settings, user: &str) -> Result<Reply> {
    backend("claude", settings)?.call(&Request { system: SYSTEM, user, schema: &schema() })
}

fn model_error<T: Debug>(result: Result<T>) -> String {
    match result {
        Err(Error::Model(why)) => why,
        other => panic!("expected Error::Model, got {other:?}"),
    }
}

#[test]
fn structured_output_is_the_reply() {
    let fake = Fake::printing(ANSWERED);
    let reply = ask(&fake.settings(), USER).unwrap();
    assert_eq!(reply.json, json!({"characters": []}));
    assert_eq!(serde_json::from_str::<Value>(&reply.raw).unwrap(), reply.json);
    assert_eq!(reply.cost_usd, Some(0.5));
}

#[test]
fn a_reply_may_hold_a_single_quote() {
    let fake = Fake::printing(r#"{"is_error":false,"structured_output":{"characters":["Pip's keeper"]}}"#);
    assert_eq!(ask(&fake.settings(), USER).unwrap().json, json!({"characters": ["Pip's keeper"]}));
}

#[test]
fn result_string_is_parsed_without_structured_output() {
    let result = r#"{"characters":["ada"]}"#;
    let fake = Fake::printing(&json!({"is_error": false, "result": result, "total_cost_usd": 0.25}).to_string());
    let reply = ask(&fake.settings(), USER).unwrap();
    assert_eq!(reply.json, json!({"characters": ["ada"]}));
    assert_eq!(reply.raw, result);
    assert_eq!(reply.cost_usd, Some(0.25));
}

#[test]
fn is_error_fails() {
    // The real program exits 1 when it reports an error; what it reported is the cause either way.
    for code in [0, 1] {
        let stdout = r#"{"is_error":true,"result":"the model is overloaded"}"#;
        let fake = Fake::new(&format!("printf '%s' '{stdout}'\nexit {code}"));
        let why = model_error(ask(&fake.settings(), USER));
        assert!(why.contains("the model is overloaded"), "{why}");
    }
}

#[test]
fn output_that_is_not_json_fails() {
    let printed = [
        ("", "printed nothing"),
        ("Hello", "not JSON"),
        ("[]", "not an object"),
        (r#"{"is_error":false,"result":"not json"}"#, "not JSON"),
        (r#"{"is_error":false,"result":"[]"}"#, "not an object"),
        (r#"{"is_error":false,"structured_output":[]}"#, "structured_output is not an object"),
        (r#"{"is_error":false}"#, "neither structured_output nor a result"),
    ];
    for (stdout, cause) in printed {
        let fake = Fake::printing(stdout);
        let why = model_error(ask(&fake.settings(), USER));
        assert!(why.contains(cause), "{stdout:?}: {why}");
    }
}

#[test]
fn nonzero_exit_fails_with_stderr() {
    // Lines of noise first, so the cause is only in the end of stderr.
    let fake = Fake::new("yes 'loading' | head -n 1000 >&2\necho 'no login' >&2\nexit 3");
    let why = model_error(ask(&fake.settings(), USER));
    assert!(why.contains("no login"), "{why}");
    assert!(why.contains("exit status: 3"), "{why}");
    assert!(why.len() < 1000, "{} bytes: {why}", why.len());
}

#[test]
fn prompt_on_stdin_and_flags_passed() {
    let fake = Fake::new(&format!(
        "for arg in \"$@\"; do printf '%s\\0' \"$arg\"; done > \"$dir/args\"\ncat > \"$dir/stdin\"\npwd > \"$dir/pwd\"\n\
         printf '%s' '{ANSWERED}'"
    ));
    ask(&fake.settings(), USER).unwrap();
    let args = fake.wrote("args");
    let args: Vec<&str> = args.strip_suffix('\0').unwrap().split('\0').collect();
    let flags = [
        "-p",
        "--model",
        "sonnet",
        "--output-format",
        "json",
        "--tools",
        "",
        "--setting-sources",
        "",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--system-prompt",
        SYSTEM,
        "--json-schema",
    ];
    assert_eq!(args[..flags.len()], flags);
    assert_eq!(args.len(), flags.len() + 1, "{args:?}");
    assert_eq!(serde_json::from_str::<Value>(args[flags.len()]).unwrap(), schema());
    assert_eq!(fake.wrote("stdin"), USER);
    assert_eq!(fake.wrote("pwd").trim_end(), "/");
}

#[test]
fn large_prompt_does_not_deadlock() {
    // The script writes a reply larger than a pipe's buffer before it reads a byte of the prompt.
    let fake = Fake::new(&format!(
        "printf '%s' '{{\"is_error\":false,\"structured_output\":{{\"characters\":[]}},\"padding\":\"'\n\
         head -c {MIB} /dev/zero | tr '\\0' x\nprintf '%s' '\"}}'\nwc -c > \"$dir/read\""
    ));
    // A deadlock blocks the call before its own timeout can act, so the test waits for the call from outside it.
    let settings = fake.settings();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || sender.send(ask(&settings, &"x".repeat(MIB))));
    let reply = receiver.recv_timeout(Duration::from_secs(20)).expect("the call deadlocked").unwrap();
    assert_eq!(reply.json, json!({"characters": []}));
    assert_eq!(fake.wrote("read").trim().parse::<usize>().unwrap(), MIB);
}

#[test]
fn timeout_kills_the_child() {
    // The script becomes the sleep, which reads none of the prompt: the prompt is larger than a pipe's buffer, so
    // the thread that writes it is still blocked when the timeout kills the sleep.
    let fake = Fake::new("echo $$ > \"$dir/pid\"\nexec sleep 30");
    let settings = Settings { timeout: Duration::from_millis(200), ..fake.settings() };
    let start = Instant::now();
    let why = model_error(ask(&settings, &"x".repeat(MIB)));
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
    assert!(why.contains("timed out"), "{why}");
    #[cfg(target_os = "linux")]
    {
        // A process killed but not reaped would still have its entry.
        let pid = fake.wrote("pid");
        let pid = pid.trim();
        assert!(!Path::new("/proc").join(pid).exists(), "the script, pid {pid}, is still there");
    }
}

#[test]
fn a_program_that_exits_in_time_is_no_timeout_while_what_it_started_holds_its_pipes() {
    // The script answers and exits at once, well before the deadline, and the sleep it leaves holds its pipes past
    // the deadline but not past the grace after the exit: a call that collected for the time left alone would time
    // out.
    let fake = Fake::new(&format!("sleep 0.7 &\nprintf '%s' {}", quoted(ANSWERED)));
    let settings = Settings { timeout: Duration::from_millis(500), ..fake.settings() };
    assert_eq!(ask(&settings, USER).unwrap().json, json!({"characters": []}));
}

#[test]
fn a_relative_program_is_found_from_the_working_directory() {
    // Inside the target directory, which "/" does not hold: the program runs in "/", where the relative path
    // would name nothing.
    let fake = Fake::new_in(Path::new(env!("CARGO_TARGET_TMPDIR")), &format!("printf '%s' {}", quoted(ANSWERED)));
    let program = relative(&std::env::current_dir().unwrap(), &fake.dir.path().join("claude"));
    assert!(program.is_relative() && program.components().count() > 1, "{}", program.display());
    let settings = Settings { claude_program: program, ..Settings::default() };
    assert_eq!(ask(&settings, USER).unwrap().json, json!({"characters": []}));
}

#[test]
fn missing_program_is_a_model_error() {
    // Starting a program forks even when the program is missing.
    let _turn = turn();
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("no-such-claude");
    let settings = Settings { claude_program: missing.clone(), ..Settings::default() };
    let why = model_error(ask(&settings, USER));
    assert!(why.contains(&missing.display().to_string()), "{why}");
    // A bare name is looked up on the PATH, so it is not made a path from the working directory.
    let bare = "no-such-claude-for-haiyaku-tests";
    let settings = Settings { claude_program: bare.into(), ..Settings::default() };
    let why = model_error(ask(&settings, USER));
    assert!(why.contains(&format!("({bare})")), "{why}");
}
