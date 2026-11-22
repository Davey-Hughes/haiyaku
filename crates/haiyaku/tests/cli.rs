//! The `haiyaku` binary, run as a reader runs it. Each test gives it no environment but a home, a data and a
//! cache directory and an empty `PATH`, all inside the test's own temporary directory: whatever the binary
//! does, it cannot reach the owner's `claude`, `OLLAMA_HOST` or database.

#![cfg(feature = "cli")]

mod common;

use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::epub::{KEEPER_TOC, keeper_epub, keeper_replies, relation};
use haiyaku::log::EventBody;
use haiyaku::store::{read_book, sha256_file, write_book};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

/// A temporary directory to run the binary in. `home/`, `data/` and `cache/` are its `HOME`,
/// `XDG_DATA_HOME` and `XDG_CACHE_HOME`, none made until the binary makes it; `bin/`, empty, is its `PATH`;
/// `keeper.epub` is the book.
struct Sandbox {
    dir: TempDir,
    book: String,
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("bin")).unwrap();
        let book = keeper_epub(dir.path()).to_str().unwrap().to_owned();
        Sandbox { dir, book }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// The binary, to run in the sandbox with only the environment the sandbox gives it.
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_haiyaku"));
        command
            .env_clear()
            .env("HOME", self.path("home"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("PATH", self.path("bin"))
            .current_dir(self.dir.path());
        command
    }

    fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Output {
        self.command().args(args).output().unwrap()
    }

    /// `extract <book> --chapters <chapters> --backend <backend>`, then `more`.
    fn extract(&self, chapters: &str, backend: &str, more: &[&str]) -> Output {
        self.extract_with(self.command(), chapters, backend, more)
    }

    /// [`Sandbox::extract`], run by `command`, which may carry variables and options of its own.
    fn extract_with(&self, mut command: Command, chapters: &str, backend: &str, more: &[&str]) -> Output {
        command.args(["extract", &self.book, "--chapters", chapters, "--backend", backend]).args(more);
        command.output().unwrap()
    }

    /// The directory `name`, holding the first `count` of the keeper's replies as `001.json`, `002.json`, ...,
    /// as the spec `replay:<dir>` that names it.
    fn replies(&self, name: &str, count: usize) -> String {
        self.recorded(name, &keeper_replies()[..count])
    }

    /// The directory `name`, holding `replies` as `001.json`, `002.json`, ..., as the spec `replay:<dir>`
    /// that names it.
    fn recorded(&self, name: &str, replies: &[String]) -> String {
        let dir = self.path(name);
        fs::create_dir(&dir).unwrap();
        for (number, reply) in (1..).zip(replies) {
            fs::write(dir.join(format!("{number:03}.json")), reply).unwrap();
        }
        format!("replay:{}", dir.display())
    }

    fn sha256(&self) -> String {
        sha256_file(self.book.as_ref()).unwrap()
    }

    /// `show <book>`, then `more`.
    fn show(&self, more: &[&str]) -> Output {
        self.command().args(["show", &self.book]).args(more).output().unwrap()
    }

    /// How many chapters the database at `relative` holds for the book.
    fn chapters_stored(&self, relative: &str) -> usize {
        let conn = Connection::open(self.path(relative)).unwrap();
        read_book(&conn, &self.sha256()).unwrap().chapters.len()
    }
}

/// The text of a stream the binary wrote.
fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// The entries of `heading`'s section in what `show` printed: the indented lines under it.
#[track_caller]
fn section<'a>(stdout: &'a str, heading: &str) -> Vec<&'a str> {
    let mut lines = stdout.lines();
    assert!(lines.any(|line| line == heading), "no {heading} section: {stdout}");
    lines.take_while(|line| line.starts_with("  ")).map(str::trim).collect()
}

/// The help of `option` in `help`: its line and the lines under it, up to the next option's, as one line.
#[track_caller]
fn option_help(help: &str, option: &str) -> String {
    let mut lines = help.lines().skip_while(|line| !line.trim_start().starts_with(option));
    let first = lines.next().unwrap_or_else(|| panic!("no {option}: {help}"));
    let rest = lines.take_while(|line| !line.trim_start().starts_with('-'));
    std::iter::once(first).chain(rest).collect::<Vec<_>>().join(" ")
}

/// The text of the data script of a page `show --html` wrote: from its opening tag to the first `</script>`
/// after it, which is where a browser ends it too.
#[track_caller]
fn data_text(page: &str) -> &str {
    let open = r#"<script id="data" type="application/json">"#;
    let start = page.find(open).unwrap_or_else(|| panic!("no data script: {page}")) + open.len();
    let end = start + page[start..].find("</script>").unwrap_or_else(|| panic!("data script not closed: {page}"));
    &page[start..end]
}

/// The data of a page `show --html` wrote.
#[track_caller]
fn page_data(page: &str) -> Value {
    let data = data_text(page);
    serde_json::from_str(data).unwrap_or_else(|error| panic!("data is not JSON ({error}): {data}"))
}

/// Every name the characters of a page's step carry: the one each is shown under and its other names.
fn names(step: &Value) -> Vec<&str> {
    let characters = step["characters"].as_array().unwrap();
    let carried = characters
        .iter()
        .flat_map(|character| std::iter::once(&character["name"]).chain(character["aliases"].as_array().unwrap()));
    carried.map(|name| name.as_str().unwrap()).collect()
}

/// What the run wrote to stdout, once its status has been checked to be `status`.
#[track_caller]
fn status(output: &Output, status: i32) -> String {
    assert_eq!(output.status.code(), Some(status), "stderr: {}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn extract_without_chapters_lists_the_table_of_contents() {
    let sandbox = Sandbox::new();
    let stdout = status(&sandbox.run(&["extract", sandbox.book.as_str()]), 0);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), KEEPER_TOC.len() + 1, "{stdout}");
    for ((number, (label, _)), line) in (1..).zip(KEEPER_TOC).zip(&lines) {
        assert_eq!(line.split_whitespace().next(), Some(number.to_string().as_str()), "{stdout}");
        assert!(line.contains(label), "{stdout}");
        assert!(line.contains("1 document,"), "{stdout}");
    }
    // "About the author" and "The author of this story is invented too.", one line each.
    assert!(lines[3].contains("58 characters"), "{stdout}");
    assert!(lines[4].contains("--chapters"), "{stdout}");
    assert!(!sandbox.path("data").exists(), "the listing opens no database");
    assert!(!sandbox.path("cache").exists());
}

#[test]
fn default_database_follows_xdg_data_home() {
    let sandbox = Sandbox::new();
    let output = sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]);
    let stdout = status(&output, 0);
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
    let replies = sandbox.path("cache/haiyaku").join(sandbox.sha256()).join("replies");
    assert_eq!(fs::read_dir(&replies).unwrap().count(), 3);
    assert!(!sandbox.path("home").exists(), "with both XDG variables set, nothing goes under HOME");

    let stderr = text(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{stderr}");
    for (number, line) in (1..=3).zip(&lines) {
        assert!(line.starts_with(&format!("{number} (entry {number}) Chapter {number}")), "{stderr}");
    }
    // A replay states no cost, so the total cannot be known.
    assert!(stdout.contains("3 chapters"), "{stdout}");
    assert!(stdout.contains("cost unknown"), "{stdout}");
}

#[test]
fn chapter_line_shows_its_number_and_its_entry() {
    let sandbox = Sandbox::new();
    let output = sandbox.extract("2-3", &sandbox.replies("replies", 3), &[]);
    status(&output, 0);
    let stderr = text(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert!(lines[0].starts_with("1 (entry 2) Chapter 2"), "{stderr}");
    assert!(lines[1].starts_with("2 (entry 3) Chapter 3"), "{stderr}");
}

#[test]
fn relative_xdg_value_is_ignored() {
    let sandbox = Sandbox::new();
    let backend = sandbox.replies("replies", 3);
    let mut command = sandbox.command();
    command.env("XDG_DATA_HOME", "rel");
    status(&sandbox.extract_with(command, "1-3", &backend, &[]), 0);
    assert_eq!(sandbox.chapters_stored("home/.local/share/haiyaku/haiyaku.db"), 3);
    assert!(!sandbox.path("rel").exists(), "a relative XDG_DATA_HOME is not read from the working directory");
}

#[test]
fn db_flag_names_the_database() {
    let sandbox = Sandbox::new();
    let db = sandbox.path("elsewhere/books.db");
    let backend = sandbox.replies("replies", 3);
    let mut command = sandbox.command();
    command.arg("--db").arg(&db);
    status(&sandbox.extract_with(command, "1-3", &backend, &[]), 0);
    assert_eq!(sandbox.chapters_stored("elsewhere/books.db"), 3);
    assert!(!sandbox.path("data").exists());
}

#[test]
fn no_cache_flag_writes_no_cache() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &["--no-cache"]), 0);
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
    assert!(!sandbox.path("cache").exists());
}

#[test]
fn no_cache_flag_reads_no_cache() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]), 0);
    let output = sandbox.extract("1-3", &sandbox.replies("empty", 0), &["--no-cache", "--replace"]);
    status(&output, 1);
    assert!(text(&output.stderr).contains("out of recorded replies"), "{}", text(&output.stderr));
}

#[test]
fn no_cache_help_says_what_it_leaves_out() {
    let sandbox = Sandbox::new();
    let help = status(&sandbox.run(&["extract", "--help"]), 0);
    let line = option_help(&help, "--no-cache");
    for said in ["reply", "rejected"] {
        assert!(line.contains(said), "{help}");
    }
}

#[test]
fn options_of_a_run_need_chapters() {
    let sandbox = Sandbox::new();
    for option in [&["--backend", "claude"][..], &["--timeout", "5"], &["--no-cache"], &["--replace"]] {
        let output = sandbox.run(&[&["extract", sandbox.book.as_str()][..], option].concat());
        status(&output, 2);
        assert!(text(&output.stderr).contains("--chapters"), "{option:?}: {}", text(&output.stderr));
    }
}

#[test]
fn timeout_and_ollama_host_reach_the_backend() {
    let sandbox = Sandbox::new();
    // A server that takes each request and never answers it, so only --timeout ends a call.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let (sender, requests) = mpsc::channel();
    std::thread::spawn(move || {
        let mut open = Vec::new();
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            open.push(stream);
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut command = sandbox.command();
    command.env("OLLAMA_HOST", &host).stdout(Stdio::piped()).stderr(Stdio::piped());
    command.args(["extract", &sandbox.book, "--chapters", "1", "--backend", "ollama:keeper", "--timeout", "1"]);
    let mut child = command.spawn().unwrap();
    // A call may take ten minutes by default: a run still going after this was not given --timeout.
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("--timeout 1 did not end the calls");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().unwrap();
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains(&format!("{host}/api/chat: timed out after 1s")), "{stderr}");
    let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(request.starts_with("POST /api/chat "), "{request}");
}

#[cfg(target_os = "linux")]
#[test]
fn progress_that_cannot_be_written_fails_once_the_log_is_stored() {
    let sandbox = Sandbox::new();
    // Every write to /dev/full fails, so no line reaches stderr.
    let mut command = sandbox.command();
    command.stderr(fs::OpenOptions::new().write(true).open("/dev/full").unwrap());
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("replies", 3), &[]);
    assert!(status(&output, 1).contains("3 chapters"));
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
}

#[test]
fn second_extract_uses_the_cache() {
    let sandbox = Sandbox::new();
    let backend = sandbox.replies("replies", 3);
    status(&sandbox.extract("1-3", &backend, &[]), 0);
    // The same directory, so the same cache key, with nothing left in it: only the cache can answer the run.
    for number in 1..=3 {
        fs::remove_file(sandbox.path("replies").join(format!("{number:03}.json"))).unwrap();
    }
    status(&sandbox.extract("1-3", &backend, &["--replace"]), 0);
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
}

#[test]
fn bad_range_calls_no_model() {
    let sandbox = Sandbox::new();
    let output = sandbox.extract("3-9", &sandbox.replies("empty", 0), &[]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("haiyaku: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "the error is said once: {stderr}");
    assert!(stderr.contains("1-4"), "{stderr}");
    assert!(!sandbox.path("data").exists(), "no database");
    assert!(!sandbox.path("cache").exists(), "no cache");

    // A backend that cannot be built: the range is refused first.
    let missing = format!("replay:{}", sandbox.path("no-replies").display());
    let output = sandbox.extract("3-9", &missing, &[]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("1-4") && !stderr.contains("no-replies"), "{stderr}");
}

#[test]
fn failed_run_stores_no_book() {
    let sandbox = Sandbox::new();
    let output = sandbox.extract("1-3", &sandbox.replies("replies", 1), &[]);
    status(&output, 1);
    assert!(text(&output.stderr).contains("chapter 2"), "{}", text(&output.stderr));
    // The database is opened before the run, so it may be there, but it holds nothing of the book.
    let output = sandbox.show(&[]);
    status(&output, 1);
    assert!(text(&output.stderr).contains("has not been extracted"), "{}", text(&output.stderr));
}

#[test]
fn extract_of_a_stored_book_needs_replace() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]), 0);
    let before = status(&sandbox.show(&[]), 0);

    // No replies to hand out: a call to the model would fail as out of them.
    let output = sandbox.extract("1-2", &sandbox.replies("empty", 0), &[]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "no chapter was extracted: {stderr}");
    assert!(stderr.contains("--replace") && !stderr.contains("out of recorded replies"), "{stderr}");
    assert_eq!(status(&sandbox.show(&[]), 0), before);

    status(&sandbox.extract("1-2", &sandbox.replies("again", 2), &["--replace"]), 0);
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 2);
}

#[test]
fn database_that_cannot_be_made_fails_before_any_call() {
    let sandbox = Sandbox::new();
    // Under a file, where no directory can be made. No replies and no cache: a call would fail otherwise.
    let db = format!("{}/books.db", sandbox.book);
    let mut command = sandbox.command();
    command.args(["--db", &db]);
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("empty", 0), &["--no-cache"]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains(&sandbox.book) && !stderr.contains("out of recorded replies"), "{stderr}");
}

#[test]
fn read_only_database_fails_before_any_call() {
    let sandbox = Sandbox::new();
    let db = sandbox.path("read-only.db");
    haiyaku::store::migrate(&Connection::open(&db).unwrap()).unwrap();
    let mut permissions = fs::metadata(&db).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&db, permissions).unwrap();
    if fs::OpenOptions::new().write(true).open(&db).is_ok() {
        // A user who writes the file all the same, as root does, has a database the run can write.
        return;
    }
    let mut command = sandbox.command();
    command.arg("--db").arg(&db);
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("empty", 0), &["--no-cache"]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("read-only.db") && !stderr.contains("out of recorded replies"), "{stderr}");
}

/// Gives a directory its write permission back, so the sandbox can remove it however the test ends.
#[cfg(unix)]
struct Unlock(PathBuf);

#[cfg(unix)]
impl Drop for Unlock {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

#[cfg(unix)]
#[test]
fn database_in_a_directory_that_cannot_be_written_fails_before_any_call() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let dir = sandbox.path("locked");
    fs::create_dir(&dir).unwrap();
    let db = dir.join("locked.db");
    haiyaku::store::migrate(&Connection::open(&db).unwrap()).unwrap();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let _unlock = Unlock(dir.clone());
    if fs::File::create(dir.join("probe")).is_ok() {
        // A user who writes the directory all the same, as root does, has a database the run can write.
        eprintln!("skipped: {} can be written all the same, as by root", dir.display());
        return;
    }
    let mut command = sandbox.command();
    command.arg("--db").arg(&db);
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("empty", 0), &["--no-cache"]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("locked.db") && !stderr.contains("out of recorded replies"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn empty_database_in_a_directory_that_cannot_be_written_fails_before_any_call() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let dir = sandbox.path("locked");
    fs::create_dir(&dir).unwrap();
    // A file with no tables yet, so migrating it is a write.
    let db = dir.join("empty.db");
    fs::write(&db, "").unwrap();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let _unlock = Unlock(dir.clone());
    if fs::File::create(dir.join("probe")).is_ok() {
        // A user who writes the directory all the same, as root does, has a database the run can write.
        eprintln!("skipped: {} can be written all the same, as by root", dir.display());
        return;
    }
    let mut command = sandbox.command();
    command.arg("--db").arg(&db);
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("empty", 0), &["--no-cache"]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("empty.db: the database cannot be written") && !stderr.contains("out of recorded replies"),
        "{stderr}"
    );
}

#[test]
fn database_in_use_by_another_run_fails_before_any_call() {
    let sandbox = Sandbox::new();
    let db = sandbox.path("busy.db");
    let other_run = Connection::open(&db).unwrap();
    haiyaku::store::migrate(&other_run).unwrap();
    // The write lock a run's write takes, held for the whole of this run: it waits out its busy timeout and
    // fails, with no race to win or lose.
    other_run.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut command = sandbox.command();
    command.arg("--db").arg(&db);
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("empty", 0), &["--no-cache"]);
    other_run.execute_batch("ROLLBACK").unwrap();
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("busy.db: the database is in use by another run")
            && !stderr.contains("out of recorded replies"),
        "{stderr}"
    );
}

#[test]
fn backend_help_says_to_rerun_a_replay_without_the_cache() {
    let sandbox = Sandbox::new();
    let help = status(&sandbox.run(&["extract", "--help"]), 0);
    let backend = option_help(&help, "--backend");
    for said in ["order", "--no-cache"] {
        assert!(backend.contains(said), "{help}");
    }
}

#[test]
fn replace_help_says_what_a_run_replaces() {
    let sandbox = Sandbox::new();
    let help = status(&sandbox.run(&["extract", "--help"]), 0);
    let replace = option_help(&help, "--replace");
    for said in ["stored log", "series links"] {
        assert!(replace.contains(said), "{help}");
    }
    let chapters = option_help(&help, "--chapters");
    assert!(chapters.contains("stored log") && chapters.contains("--replace"), "{help}");
}

#[test]
fn missing_book_is_named() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["extract", "no-such-book.epub"]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("haiyaku: no-such-book.epub: "), "{stderr}");
}

#[test]
fn chapters_that_are_not_a_range_are_a_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.extract("two", &sandbox.replies("empty", 0), &[]);
    status(&output, 2);
    assert!(!sandbox.path("data").exists());
}

#[test]
fn extract_then_show() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]), 0);

    let stdout = status(&sandbox.show(&[]), 0);
    // The stranger of chapter 2 is Tom Reed, and was met in chapter 2.
    assert_eq!(
        section(&stdout, "Characters"),
        ["Ada Marsh (the keeper, Mrs. Marsh), ch 1-3", "Pip, ch 1-1", "Tom Reed (the stranger), ch 2-3"],
        "{stdout}"
    );
    assert_eq!(
        section(&stdout, "Relations"),
        ["Ada Marsh -> Pip: owns, her gull", "Ada Marsh - Tom Reed: acquaintance, met on the jetty"],
        "{stdout}"
    );
    assert_eq!(status(&sandbox.show(&["--at", "3"]), 0), stdout, "by default, the cast after the last chapter");

    let stdout = status(&sandbox.show(&["--at", "1"]), 0);
    assert_eq!(section(&stdout, "Characters"), ["the keeper, ch 1-1", "Pip, ch 1-1"], "{stdout}");
    assert_eq!(section(&stdout, "Relations"), ["the keeper -> Pip: owns, her gull"], "{stdout}");
    for later in ["Ada", "Marsh", "Tom", "Reed", "stranger"] {
        assert!(!stdout.contains(later), "{later} is not known after chapter 1: {stdout}");
    }

    let stdout = status(&sandbox.show(&["--at", "0"]), 0);
    assert!(section(&stdout, "Characters").is_empty(), "{stdout}");
    assert!(section(&stdout, "Relations").is_empty(), "{stdout}");
}

#[test]
fn directed_fact_runs_from_its_source() {
    let sandbox = Sandbox::new();
    // Chapter 1's relation turned round, so that it runs from Pip, the later character and the greater id, to
    // the keeper: the pair is held the other way round.
    let mut first: Value = serde_json::from_str(&keeper_replies()[0]).unwrap();
    let quote = "The keeper fed Pip, her gull, from her own plate.";
    first["relations"] = json!([relation("new2", "new1", "serves", "feeds from her plate", quote)]);
    status(&sandbox.extract("1", &sandbox.recorded("replies", &[first.to_string()]), &[]), 0);
    let stdout = status(&sandbox.show(&[]), 0);
    assert_eq!(section(&stdout, "Relations"), ["Pip -> the keeper: serves, feeds from her plate"], "{stdout}");
}

#[test]
fn failed_extract_leaves_database_unchanged() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]), 0);
    let before = status(&sandbox.show(&[]), 0);
    assert!(before.contains("Tom Reed"), "{before}");

    let output = sandbox.extract("1-3", &sandbox.replies("one", 1), &["--no-cache", "--replace"]);
    status(&output, 1);
    assert_eq!(status(&sandbox.show(&[]), 0), before);
}

#[test]
fn show_before_extract_fails() {
    let sandbox = Sandbox::new();
    let output = sandbox.show(&[]);
    assert!(status(&output, 1).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("haiyaku: ") && stderr.contains("has not been extracted"), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "the error is said once: {stderr}");
    assert!(!sandbox.path("data").exists(), "show leaves no empty database behind");

    // A database that holds other books, or none, does not hold this one either.
    fs::create_dir_all(sandbox.path("data/haiyaku")).unwrap();
    haiyaku::store::migrate(&Connection::open(sandbox.path("data/haiyaku/haiyaku.db")).unwrap()).unwrap();
    let output = sandbox.show(&[]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("haiyaku: ") && stderr.contains("has not been extracted"), "{stderr}");
}

#[test]
fn show_writes_nothing_into_a_database_without_haiyaku_tables() {
    let sandbox = Sandbox::new();
    let show = |db: &PathBuf| {
        let output = sandbox.command().arg("--db").arg(db).args(["show", &sandbox.book]).output().unwrap();
        assert!(status(&output, 1).is_empty());
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with("haiyaku: ") && stderr.contains("has not been extracted"), "{stderr}");
    };
    let empty = sandbox.path("empty.db");
    fs::write(&empty, "").unwrap();
    show(&empty);
    assert_eq!(fs::metadata(&empty).unwrap().len(), 0);

    let other = sandbox.path("other.db");
    Connection::open(&other).unwrap().execute_batch("CREATE TABLE other (x)").unwrap();
    show(&other);
    let conn = Connection::open(&other).unwrap();
    let mut tables = conn.prepare("SELECT name FROM sqlite_master").unwrap();
    let tables: Vec<String> = tables.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(tables, ["other"], "another program's database is left as it was");
}

#[test]
fn show_missing_book_is_named() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["show", "no-such-book.epub"]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("haiyaku: no-such-book.epub: "), "{stderr}");
}

#[test]
fn show_past_the_last_chapter_fails() {
    let sandbox = Sandbox::new();
    // Entries 2 and 3 of the contents are the log's chapters 1 and 2.
    status(&sandbox.extract("2-3", &sandbox.replies("replies", 3), &[]), 0);
    status(&sandbox.show(&["--at", "2"]), 0);
    let output = sandbox.show(&["--at", "3"]);
    assert!(status(&output, 1).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("haiyaku: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "the error is said once: {stderr}");
    assert!(stderr.contains("0-2"), "{stderr}");
}

#[test]
fn at_help_says_where_chapters_count_from() {
    let sandbox = Sandbox::new();
    let help = status(&sandbox.run(&["show", "--help"]), 0);
    let at = option_help(&help, "--at");
    for said in ["first entry extracted", "table of contents"] {
        assert!(at.contains(said), "{help}");
    }
}

#[test]
fn show_html_writes_a_self_contained_page() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]), 0);
    let file = sandbox.path("keeper.html");
    let stdout = status(&sandbox.show(&["--html", file.to_str().unwrap()]), 0);
    assert!(stdout.is_empty(), "the page goes to the file: {stdout}");

    let page = fs::read_to_string(&file).unwrap();
    for remote in [r#"src="http"#, r#"href="http"#, "url(http", "@import", "fetch("] {
        assert!(!page.contains(remote), "the page asks another host for something: {remote}");
    }
    let data = page_data(&page);
    let steps = data["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 4, "a step for each chapter from 0 to 3: {data}");
    assert_eq!(steps[2]["title"], "Chapter 2", "{data}");
    assert!(!names(&steps[1]).contains(&"Ada Marsh"), "the keeper is named in chapter 2: {data}");
    assert!(names(&steps[3]).contains(&"Ada Marsh"), "{data}");
}

#[test]
fn html_data_cannot_close_its_script() {
    let sandbox = Sandbox::new();
    // show finds a book by the hash of its file and reads nothing else of it, so any file can stand for the
    // book of a log written straight into the database.
    let book = sandbox.path("small.epub");
    fs::write(&book, "A small file that stands for a book.").unwrap();
    let mut log = common::keeper();
    log.book.sha256 = sha256_file(&book).unwrap();
    // The cast carries quotes on relation facts only. `<!--<script>` would make a browser pass over the
    // `</script>` that ends the data.
    let quote = "The keeper fed Pip, her gull.</script><p> <!--<script>";
    let event = log.events.iter_mut().find(|event| matches!(event.body, EventBody::Relation { .. })).unwrap();
    event.quote = quote.to_owned();
    fs::create_dir_all(sandbox.path("data/haiyaku")).unwrap();
    let mut conn = Connection::open(sandbox.path("data/haiyaku/haiyaku.db")).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    haiyaku::store::migrate(&conn).unwrap();
    write_book(&mut conn, &log).unwrap();
    drop(conn);

    let file = sandbox.path("page.html");
    let args: [&OsStr; 4] = ["show".as_ref(), book.as_os_str(), "--html".as_ref(), file.as_os_str()];
    status(&sandbox.run(&args), 0);
    let page = fs::read_to_string(&file).unwrap();
    let raw = data_text(&page);
    assert!(!raw.contains("</") && !raw.contains("<!--"), "{raw}");
    let data = page_data(&page);
    let quotes: Vec<&str> = data["steps"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|step| step["relations"].as_array().unwrap())
        .flat_map(|relation| relation["facts"].as_array().unwrap())
        .map(|fact| fact["quote"].as_str().unwrap())
        .collect();
    assert!(quotes.contains(&quote), "{data}");
}

#[test]
fn html_page_starts_at_the_chapter_at_names() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 3), &[]), 0);
    let start = |name: &str, more: &[&str]| {
        let file = sandbox.path(name);
        status(&sandbox.show(&[&["--html", file.to_str().unwrap()][..], more].concat()), 0);
        page_data(&fs::read_to_string(&file).unwrap())["at"].clone()
    };
    assert_eq!(start("last.html", &[]), 3, "by default, the last chapter extracted");
    assert_eq!(start("first.html", &["--at", "1"]), 1);

    let file = sandbox.path("past.html");
    status(&sandbox.show(&["--html", file.to_str().unwrap(), "--at", "4"]), 1);
    assert!(!file.exists(), "a refused --at writes no page");
}

#[test]
fn html_help_says_the_page_is_for_checking_a_run() {
    let sandbox = Sandbox::new();
    let help = status(&sandbox.run(&["show", "--help"]), 0);
    let html = option_help(&help, "--html");
    for said in ["checking a run", "not for a reader"] {
        assert!(html.contains(said), "{help}");
    }
}
