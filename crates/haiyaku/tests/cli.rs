//! The `haiyaku` binary, run as a reader runs it. Each test gives it no environment but a home, a data and a
//! cache directory and an empty `PATH`, all inside the test's own temporary directory: whatever the binary
//! does, it cannot reach the owner's `claude`, `OLLAMA_HOST` or database.

#![cfg(feature = "cli")]

mod common;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::epub::{KEEPER_TOC, Toc, keeper_documents, keeper_epub, keeper_replies, nav_map, ncx, relation};
use haiyaku::Error;
use haiyaku::fold::EntityKey;
use haiyaku::log::{BookLog, Entity, EntityKind, Event, EventBody, Name, Position, RelationKind, Sex};
use haiyaku::store::{
    Link, LinkMethod, Order, Series, import_book, import_series, put_links, read_book, read_links, read_series,
    sha256_file, write_book,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

/// A temporary directory to run the binary in. `home/`, `data/` and `cache/` are its `HOME`,
/// `XDG_DATA_HOME` and `XDG_CACHE_HOME`, none made until the binary makes it; `bin/`, empty, is its `PATH`;
/// `keeper.epub` is the book, and `return.epub` the book after it in the keepers' series.
struct Sandbox {
    dir: TempDir,
    book: String,
    sequel: String,
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("bin")).unwrap();
        let book = keeper_epub(dir.path()).to_str().unwrap().to_owned();
        // The keeper's documents under a package of its own, which is all it takes for another hash: a command
        // knows a book by the hash of its file and reads none of its text.
        let toc = Toc::Ncx(ncx(&nav_map(&KEEPER_TOC)));
        let sequel = common::epub::book(dir.path(), "return.epub", SEQUEL_METADATA, &keeper_documents(), &toc);
        Sandbox { dir, book, sequel: sequel.to_str().unwrap().to_owned() }
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

    fn sequel_sha256(&self) -> String {
        sha256_file(self.sequel.as_ref()).unwrap()
    }

    /// `show <book>`, then `more`.
    fn show(&self, more: &[&str]) -> Output {
        self.command().args(["show", &self.book]).args(more).output().unwrap()
    }

    /// Writes `log` straight into the default database as the log of the sandbox's book.
    fn store(&self, log: BookLog) {
        self.store_as(&self.book, log);
    }

    /// Writes `log` straight into the default database as the log of the EPUB at `book`. A command finds a book
    /// by the hash of its file and reads nothing else of it, so the book need not hold the log's text.
    fn store_as(&self, book: &str, mut log: BookLog) {
        log.book.sha256 = sha256_file(book.as_ref()).unwrap();
        fs::create_dir_all(self.path("data/haiyaku")).unwrap();
        let mut conn = self.database();
        haiyaku::store::migrate(&conn).unwrap();
        write_book(&mut conn, &log).unwrap();
    }

    /// The default database, with the foreign keys on that the library's writes need.
    fn database(&self) -> Connection {
        let conn = Connection::open(self.path("data/haiyaku/haiyaku.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn
    }

    /// The keepers' two books, stored as the logs of the sandbox's two EPUBs: "The Lighthouse Keeper" of
    /// `keeper.epub`, "The Keeper's Return" of `return.epub`.
    fn store_keepers(&self) {
        self.store_as(&self.book, common::keeper());
        self.store_as(&self.sequel, common::keepers_return());
    }

    /// [`common::keepers_links`], each end named by the hash of its book's EPUB in the sandbox.
    fn keepers_links(&self) -> Vec<Link> {
        let hashes = [
            (common::keeper().book.sha256, self.sha256()),
            (common::keepers_return().book.sha256, self.sequel_sha256()),
        ];
        let rehash = |key: &mut EntityKey| {
            key.book = hashes.iter().find(|(fixture, _)| *fixture == key.book).unwrap().1.clone();
        };
        let mut links = common::keepers_links();
        for link in &mut links {
            rehash(&mut link.later);
            rehash(&mut link.earlier);
        }
        links
    }

    /// The logs the default database holds for the keepers' two books, which links are made against.
    fn keepers_logs(&self) -> Vec<BookLog> {
        let conn = self.database();
        [self.sha256(), self.sequel_sha256()].iter().map(|book| read_book(&conn, book).unwrap()).collect()
    }

    /// The logs the default database holds for the books of the keepers' order `order`, in its sequence, which its
    /// links are made over.
    fn order_logs(&self, order: &str) -> Vec<BookLog> {
        let conn = self.database();
        let series = read_series(&conn, "keepers").unwrap();
        let books = &series.orders.iter().find(|stored| stored.name == order).unwrap().books;
        books.iter().map(|book| read_book(&conn, book).unwrap()).collect()
    }

    /// The keepers' series file: `keeper.epub` named from the file's folder, `return.epub` by its absolute
    /// path, and two orders, the books as published and the other way round.
    fn keepers_series(&self) -> String {
        let sequel = &self.sequel;
        format!(
            r#"name = "keepers"
title = "The Keepers"

[books]
keeper = "../keeper.epub"
return = '{sequel}'

[[order]]
name = "published"
books = ["keeper", "return"]

[[order]]
name = "sequel first"
books = ["return", "keeper"]
"#
        )
    }

    /// The links [`keepers_link_replies`] make in the order `published`: [`Sandbox::keepers_links`], each made by
    /// the model.
    fn keepers_linked(&self) -> Vec<Link> {
        let mut links = self.keepers_links();
        links[0].method = LinkMethod::Model { quote: "Ada watched the boat come in.".into() };
        links
    }

    /// The link [`sequel_first_replies`] make in the order `sequel first`: the first book's Ada is the second
    /// book's from the name its chapter 2 gives her.
    fn sequel_first_linked(&self) -> Vec<Link> {
        vec![Link {
            later: EntityKey { book: self.sha256(), id: "ada".into() },
            earlier: EntityKey { book: self.sequel_sha256(), id: "ada".into() },
            method: LinkMethod::Model { quote: "The letter was addressed to Ada Marsh.".into() },
            chapter: 2,
            at: Position { document: 2, block: 1 },
        }]
    }

    /// The stored links of the keepers' order `order`.
    fn links(&self, order: &str) -> Vec<Link> {
        read_links(&self.database(), "keepers", order).unwrap()
    }

    /// `link <file> --backend <backend>`, then `more`.
    fn link(&self, file: &str, backend: &str, more: &[&str]) -> Output {
        self.run(&[&["link", file, "--backend", backend][..], more].concat())
    }

    /// Writes `text` to [`SERIES_FILE`], and returns its path as a command is given it.
    fn series_file(&self, text: &str) -> String {
        fs::create_dir_all(self.path("series")).unwrap();
        fs::write(self.path(SERIES_FILE), text).unwrap();
        SERIES_FILE.to_owned()
    }

    /// How many chapters the database at `relative` holds for the book.
    fn chapters_stored(&self, relative: &str) -> usize {
        let conn = Connection::open(self.path(relative)).unwrap();
        read_book(&conn, &self.sha256()).unwrap().chapters.len()
    }

    /// Loads the keepers' series from its file into the default database, which holds both books already, and
    /// stores `links` as the order `published`'s; returns the file's path as a command is given it.
    fn load_keepers(&self, links: &[Link]) -> String {
        let file = self.series_file(&self.keepers_series());
        status(&self.run(&["series", &file]), 0);
        put_links(&mut self.database(), "keepers", "published", links, &self.keepers_logs()).unwrap();
        file
    }

    /// `show <the sequel> --series <file>`, then `more`.
    fn show_sequel(&self, file: &str, more: &[&str]) -> Output {
        self.run(&[&["show", &self.sequel, "--series", file][..], more].concat())
    }
}

/// What the package of [`Sandbox::sequel`] says of itself.
const SEQUEL_METADATA: &str = "
    <dc:identifier id=\"id\">urn:uuid:00000000-0000-4000-8000-000000000002</dc:identifier>
    <dc:title>The Keeper's Return</dc:title>
    <dc:language>en</dc:language>";

/// Where a test writes its series file, from the binary's working directory: in a folder of its own, so that a
/// book path read from the working directory in place of the file's folder finds no book. Its name is not the
/// series', so that a sidecar named after the file would show.
const SERIES_FILE: &str = "series/shelf.toml";

/// The note and the quote of the overturn [`keeper_with_overturn`] adds.
const OVERTURN_NOTE: &str = "the gull came with the lighthouse";
const OVERTURN_QUOTE: &str = "Pip had never been hers: the gull came with the lighthouse.";

/// The keeper's book with an overturn in chapter 3 of the fact chapter 1 states, that the keeper owns Pip.
fn keeper_with_overturn() -> BookLog {
    let mut log = common::keeper();
    let seq = log.events.last().unwrap().seq + 1;
    let body = EventBody::Overturned {
        from: "ada".into(),
        to: "pip".into(),
        kind: RelationKind::Owns,
        note: OVERTURN_NOTE.into(),
    };
    let at = Position { document: 3, block: 9 };
    log.events.push(Event { seq, chapter: 3, at, quote: OVERTURN_QUOTE.into(), body });
    log
}

/// A reply of the link step that names `candidate`, or none, and gives `quote`.
fn link_reply(candidate: Option<&str>, quote: &str) -> String {
    json!({ "candidate": candidate, "quote": quote }).to_string()
}

/// The replies that link the keepers' order `published`, as the link step's own tests record them: the second
/// book's Ada is the first book's, and its old sailor is the first book's Tom Reed.
fn keepers_link_replies() -> Vec<String> {
    vec![
        link_reply(Some("e1"), "Ada watched the boat come in."),
        link_reply(Some("e1"), "Call me Tom, said the old sailor."),
    ]
}

/// The replies for the keepers' order `sequel first`, whose later book is the first: its Ada is the second book's,
/// and nothing shows its Tom Reed to be the old sailor.
fn sequel_first_replies() -> Vec<String> {
    vec![link_reply(Some("e1"), "The letter was addressed to Ada Marsh."), link_reply(None, "")]
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
    let output = sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]);
    let stdout = status(&output, 0);
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
    let replies = sandbox.path("cache/haiyaku").join(sandbox.sha256()).join("replies");
    assert_eq!(fs::read_dir(&replies).unwrap().count(), 6, "two calls a chapter");
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
    let output = sandbox.extract("2-3", &sandbox.replies("replies", 6), &[]);
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
    let backend = sandbox.replies("replies", 6);
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
    let backend = sandbox.replies("replies", 6);
    let mut command = sandbox.command();
    command.arg("--db").arg(&db);
    status(&sandbox.extract_with(command, "1-3", &backend, &[]), 0);
    assert_eq!(sandbox.chapters_stored("elsewhere/books.db"), 3);
    assert!(!sandbox.path("data").exists());
}

#[test]
fn no_cache_flag_writes_no_cache() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &["--no-cache"]), 0);
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
    assert!(!sandbox.path("cache").exists());
}

#[test]
fn no_cache_flag_reads_no_cache() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]), 0);
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
    let output = sandbox.extract_with(command, "1-3", &sandbox.replies("replies", 6), &[]);
    assert!(status(&output, 1).contains("3 chapters"));
    assert_eq!(sandbox.chapters_stored("data/haiyaku/haiyaku.db"), 3);
}

#[test]
fn second_extract_uses_the_cache() {
    let sandbox = Sandbox::new();
    let backend = sandbox.replies("replies", 6);
    status(&sandbox.extract("1-3", &backend, &[]), 0);
    // The same directory, so the same cache key, with nothing left in it: only the cache can answer the run.
    for number in 1..=6 {
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
    let output = sandbox.extract("1-3", &sandbox.replies("replies", 2), &[]);
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
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]), 0);
    let before = status(&sandbox.show(&[]), 0);

    // No replies to hand out: a call to the model would fail as out of them.
    let output = sandbox.extract("1-2", &sandbox.replies("empty", 0), &[]);
    status(&output, 1);
    let stderr = text(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "no chapter was extracted: {stderr}");
    assert!(stderr.contains("--replace") && !stderr.contains("out of recorded replies"), "{stderr}");
    assert_eq!(status(&sandbox.show(&[]), 0), before);

    status(&sandbox.extract("1-2", &sandbox.replies("again", 4), &["--replace"]), 0);
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
    for command in ["extract", "link"] {
        let help = status(&sandbox.run(&[command, "--help"]), 0);
        let backend = option_help(&help, "--backend");
        for said in ["order", "--no-cache"] {
            assert!(backend.contains(said), "{command}: {help}");
        }
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
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]), 0);

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
    let replies = [first.to_string(), keeper_replies().swap_remove(1)];
    status(&sandbox.extract("1", &sandbox.recorded("replies", &replies), &[]), 0);
    let stdout = status(&sandbox.show(&[]), 0);
    assert_eq!(section(&stdout, "Relations"), ["Pip -> the keeper: serves, feeds from her plate"], "{stdout}");
}

#[test]
fn show_marks_an_overturned_fact() {
    let sandbox = Sandbox::new();
    sandbox.store(keeper_with_overturn());
    let stdout = status(&sandbox.show(&[]), 0);
    let marked = format!("Ada Marsh -> Pip: owns, her gull [overturned in ch 3: {OVERTURN_NOTE}]");
    assert_eq!(
        section(&stdout, "Relations"),
        [marked.as_str(), "Ada Marsh - Tom Reed: acquaintance, met on the jetty"],
        "{stdout}"
    );
}

#[test]
fn show_before_the_overturn_prints_the_fact_unmarked() {
    let sandbox = Sandbox::new();
    sandbox.store(keeper_with_overturn());
    let stdout = status(&sandbox.show(&["--at", "2"]), 0);
    assert_eq!(
        section(&stdout, "Relations"),
        ["Ada Marsh -> Pip: owns, her gull", "Ada Marsh - the stranger: acquaintance, met on the jetty"],
        "{stdout}"
    );
}

#[test]
fn show_prints_a_marked_fact_beside_the_same_kind_stated_again() {
    let sandbox = Sandbox::new();
    // The keeper's book with chapter 1's fact, that the keeper owns Pip, overturned in chapter 2 and stated again,
    // with its own quote, in chapter 3.
    let mut log = common::keeper();
    let overturn = EventBody::Overturned {
        from: "ada".into(),
        to: "pip".into(),
        kind: RelationKind::Owns,
        note: OVERTURN_NOTE.into(),
    };
    let at = |document, block| Position { document, block };
    let overturned = Event { seq: 0, chapter: 2, at: at(2, 7), quote: OVERTURN_QUOTE.into(), body: overturn };
    let owns = EventBody::Relation {
        from: "ada".into(),
        to: "pip".into(),
        kind: RelationKind::Owns,
        note: "given to her for good".into(),
    };
    let quote = "The harbourmaster gave the gull to her for good.";
    let again = Event { seq: 0, chapter: 3, at: at(3, 9), quote: quote.into(), body: owns };
    // The overturn goes after chapter 2's last event and the statement after chapter 3's, and the events are
    // numbered again in that order.
    log.events.insert(6, overturned);
    log.events.push(again);
    for (event, seq) in log.events.iter_mut().zip(1..) {
        event.seq = seq;
    }
    sandbox.store(log);

    let stdout = status(&sandbox.show(&[]), 0);
    let marked = format!("Ada Marsh -> Pip: owns, her gull [overturned in ch 2: {OVERTURN_NOTE}]");
    assert_eq!(
        section(&stdout, "Relations"),
        [
            marked.as_str(),
            "Ada Marsh -> Pip: owns, given to her for good",
            "Ada Marsh - Tom Reed: acquaintance, met on the jetty"
        ],
        "{stdout}"
    );
}

#[test]
fn failed_extract_leaves_database_unchanged() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]), 0);
    let before = status(&sandbox.show(&[]), 0);
    assert!(before.contains("Tom Reed"), "{before}");

    let output = sandbox.extract("1-3", &sandbox.replies("one", 2), &["--no-cache", "--replace"]);
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
    status(&sandbox.extract("2-3", &sandbox.replies("replies", 6), &[]), 0);
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
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]), 0);
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
    // One book: the data names it, and an id is the log's own, with no book in it.
    assert_eq!(data["books"], json!([data["title"]]), "{data}");
    for character in steps[3]["characters"].as_array().unwrap() {
        assert_eq!((&character["firstBook"], &character["lastBook"]), (&json!(0), &json!(0)), "{data}");
        assert!(!character["id"].as_str().unwrap().contains(':'), "{data}");
    }
    assert!(!names(&steps[1]).contains(&"Ada Marsh"), "the keeper is named in chapter 2: {data}");
    assert!(names(&steps[3]).contains(&"Ada Marsh"), "{data}");
}

#[test]
fn html_data_cannot_close_its_script() {
    let sandbox = Sandbox::new();
    let mut log = common::keeper();
    // The cast carries quotes on relation facts only. `<!--<script>` would make a browser pass over the
    // `</script>` that ends the data.
    let quote = "The keeper fed Pip, her gull.</script><p> <!--<script>";
    let event = log.events.iter_mut().find(|event| matches!(event.body, EventBody::Relation { .. })).unwrap();
    event.quote = quote.to_owned();
    sandbox.store(log);

    let file = sandbox.path("page.html");
    status(&sandbox.show(&["--html", file.to_str().unwrap()]), 0);
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
fn show_html_carries_the_overturn() {
    let sandbox = Sandbox::new();
    sandbox.store(keeper_with_overturn());
    let file = sandbox.path("page.html");
    status(&sandbox.show(&["--html", file.to_str().unwrap()]), 0);
    let data = page_data(&fs::read_to_string(&file).unwrap());
    // Owning is the one fact of the keeper and Pip.
    let owns = |step: usize| {
        let relations = data["steps"][step]["relations"].as_array().unwrap();
        let pair = relations.iter().find(|relation| relation["a"] == "ada" && relation["b"] == "pip").unwrap();
        assert_eq!(pair["facts"].as_array().unwrap().len(), 1, "{data}");
        pair["facts"][0].clone()
    };
    assert_eq!(owns(2).get("overturned"), Some(&Value::Null), "not yet overturned after chapter 2: {data}");
    let overturn = json!({ "chapter": 3, "book": 0, "note": OVERTURN_NOTE, "quote": OVERTURN_QUOTE });
    assert_eq!(owns(3)["overturned"], overturn, "{data}");
}

#[test]
fn html_page_starts_at_the_chapter_at_names() {
    let sandbox = Sandbox::new();
    status(&sandbox.extract("1-3", &sandbox.replies("replies", 6), &[]), 0);
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

#[test]
fn series_stores_the_files_series() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.series_file(&sandbox.keepers_series());
    assert_eq!(status(&sandbox.run(&["series", &file]), 0), "", "a series that had no links says nothing");
    let (first, second) = (sandbox.sha256(), sandbox.sequel_sha256());
    let expected = Series {
        name: "keepers".into(),
        title: "The Keepers".into(),
        orders: vec![
            Order { name: "published".into(), books: vec![first.clone(), second.clone()] },
            Order { name: "sequel first".into(), books: vec![second, first] },
        ],
    };
    assert_eq!(read_series(&sandbox.database(), "keepers").unwrap(), expected);
}

#[test]
fn series_file_errors_name_the_file() {
    let sandbox = Sandbox::new();
    sandbox.store(common::keeper());
    let good = sandbox.keepers_series();
    let stores_no_series = |case: &str| {
        let read = read_series(&sandbox.database(), "keepers");
        assert!(matches!(read, Err(Error::NotFound(_))), "{case} stores no series: {read:?}");
    };

    // The sequel has not been extracted: the message names its EPUB, and the database it is not in.
    let file = sandbox.series_file(&good);
    let output = sandbox.run(&["series", &file]);
    assert!(status(&output, 1).is_empty());
    let database = sandbox.path("data/haiyaku/haiyaku.db");
    let expected = format!("haiyaku: {} has not been extracted into {}\n", sandbox.sequel, database.display());
    assert_eq!(text(&output.stderr), expected);
    stores_no_series("a book not extracted");

    sandbox.store_as(&sandbox.sequel, common::keepers_return());
    let missing = sandbox.path("missing.epub").to_str().unwrap().to_owned();
    // Each case: what is wrong, the file, and a path its message names besides the file's.
    let cases = [
        ("an unknown key at the top", format!("colour = \"blue\"\n{good}"), None),
        // Under [books] every key is a label and its value a path: a table there is a key it does not know.
        ("an unknown key in [books]", format!("{good}\n[books.shelf]\nrow = 2\n"), None),
        ("an unknown key in an order", format!("{good}colour = \"blue\"\n"), None),
        ("a title that is not a string", good.replace(r#"title = "The Keepers""#, "title = 3"), None),
        ("a book path that is not a string", good.replace(r#""../keeper.epub""#, "1"), None),
        ("a label [books] lacks", good.replace(r#"["return", "keeper"]"#, r#"["return", "lamp"]"#), None),
        ("no order", good[..good.find("[[order]]").unwrap()].to_owned(), None),
        ("two orders of one name", good.replace("sequel first", "published"), None),
        // The series' sidecar is named after it, beside the file.
        ("an empty name", good.replace(r#"name = "keepers""#, r#"name = """#), None),
        ("a name with a folder in it", good.replace(r#"name = "keepers""#, r#"name = "shelf/keepers""#), None),
        ("a name that leaves the folder", good.replace(r#"name = "keepers""#, r#"name = "../keepers""#), None),
        ("a name that is the folder", good.replace(r#"name = "keepers""#, r#"name = ".""#), None),
        ("a missing book file", good.replace(&sandbox.sequel, &missing), Some(&missing)),
    ];
    for (case, written, book) in cases {
        assert_ne!(written, good, "{case}");
        let file = sandbox.series_file(&written);
        let output = sandbox.run(&["series", &file]);
        assert!(status(&output, 1).is_empty(), "{case}");
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with(&format!("haiyaku: {file}: ")), "{case}: {stderr}");
        assert!(book.is_none_or(|book| stderr.contains(book.as_str())), "{case}: {stderr}");
        stores_no_series(case);
    }
}

#[test]
fn series_and_link_make_no_database() {
    let sandbox = Sandbox::new();
    let file = sandbox.series_file(&sandbox.keepers_series());
    let backend = sandbox.recorded("unused", &[]);
    let refused = |db: Option<&PathBuf>| {
        let run = |args: &[&str]| {
            let mut command = sandbox.command();
            if let Some(db) = db {
                command.arg("--db").arg(db);
            }
            let output = command.args(args).output().unwrap();
            assert!(status(&output, 1).is_empty(), "{args:?}");
            text(&output.stderr)
        };
        let database = db.cloned().unwrap_or_else(|| sandbox.path("data/haiyaku/haiyaku.db"));
        // Nothing is extracted into a database that is not there: the message names the first book of the file.
        let stderr = run(&["series", &file]);
        let expected = format!("keeper.epub has not been extracted into {}\n", database.display());
        assert!(stderr.starts_with("haiyaku: ") && stderr.ends_with(&expected), "{stderr}");
        let stderr = run(&["link", &file, "--backend", &backend]);
        assert_eq!(
            stderr,
            format!("haiyaku: {file}: the database holds no series keepers: run haiyaku series {file} first\n")
        );
    };

    refused(None);
    assert!(!sandbox.path("data").exists(), "no database, and no folder for it");
    let elsewhere = sandbox.path("elsewhere/haiyaku.db");
    refused(Some(&elsewhere));
    assert!(!sandbox.path("elsewhere").exists(), "no database, and no folder for it");
    // A file without haiyaku tables holds no book either, and is not written to.
    let empty = sandbox.path("empty.db");
    fs::write(&empty, "").unwrap();
    refused(Some(&empty));
    assert_eq!(fs::metadata(&empty).unwrap().len(), 0);

    // A series whose orders hold no book is not stored in a database that is not there either.
    let bookless = sandbox.series_file(
        "name = \"keepers\"\ntitle = \"The Keepers\"\n[books]\n[[order]]\nname = \"published\"\nbooks = []\n",
    );
    let output = sandbox.run(&["series", &bookless]);
    assert!(status(&output, 1).is_empty());
    assert!(text(&output.stderr).starts_with(&format!("haiyaku: {bookless}: ")), "{}", text(&output.stderr));
    assert!(!sandbox.path("data").exists());
}

#[test]
fn series_keeps_the_links_of_a_series_it_has_already_and_counts_the_links_it_drops() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let good = sandbox.keepers_series();
    let file = sandbox.series_file(&good);
    status(&sandbox.run(&["series", &file]), 0);
    let mut conn = sandbox.database();
    put_links(&mut conn, "keepers", "published", &sandbox.keepers_links(), &sandbox.order_logs("published")).unwrap();
    let sequel_first = sandbox.sequel_first_linked();
    put_links(&mut conn, "keepers", "sequel first", &sequel_first, &sandbox.order_logs("sequel first")).unwrap();

    // The file as stored: nothing is stored again, and the links stay.
    let stdout = status(&sandbox.run(&["series", &file]), 0);
    assert_eq!(stdout, "series keepers is unchanged, and keeps its links\n");
    assert_eq!(sandbox.links("published"), sandbox.keepers_links());
    assert_eq!(sandbox.links("sequel first"), sequel_first);

    // Another title: the series is stored again, and every order's links are gone.
    sandbox.series_file(&good.replace("The Keepers", "The Keepers, collected"));
    let stdout = status(&sandbox.run(&["series", &file]), 0);
    assert_eq!(stdout, format!("3 links of series keepers are gone: haiyaku link {file} makes them again\n"));
    assert_eq!(sandbox.links("published"), []);
    assert_eq!(sandbox.links("sequel first"), []);
}

#[test]
fn export_writes_a_book_sidecar_beside_it() {
    let sandbox = Sandbox::new();
    sandbox.store(common::keeper());
    let stdout = status(&sandbox.run(&["export", &sandbox.book]), 0);
    let sidecar = sandbox.path("keeper.haiyaku.db");
    assert_eq!(stdout, format!("{}\n", sidecar.display()));
    let mut conn = common::db();
    import_book(&mut conn, &sidecar, &sandbox.sha256()).unwrap();
    let stored = read_book(&sandbox.database(), &sandbox.sha256()).unwrap();
    assert_eq!(read_book(&conn, &sandbox.sha256()).unwrap(), stored);

    // An EPUB is known by its extension, in any case.
    let shouted = sandbox.path("lighthouse.EPUB");
    fs::copy(&sandbox.book, &shouted).unwrap();
    let stdout = status(&sandbox.run(&["export", shouted.to_str().unwrap()]), 0);
    let sidecar = sandbox.path("lighthouse.haiyaku.db");
    assert_eq!(stdout, format!("{}\n", sidecar.display()));
    assert!(sidecar.exists());

    // A book that has not been extracted is named by its path, and gets no sidecar.
    let output = sandbox.run(&["export", &sandbox.sequel]);
    assert!(status(&output, 1).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with(&format!("haiyaku: {} has not been extracted", sandbox.sequel)), "{stderr}");
    assert!(!sandbox.path("return.haiyaku.db").exists());
}

#[test]
fn export_writes_a_series_sidecar_beside_the_file() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);
    let links = sandbox.keepers_links();
    put_links(&mut sandbox.database(), "keepers", "published", &links, &sandbox.keepers_logs()).unwrap();

    let stdout = status(&sandbox.run(&["export", &file]), 0);
    // Named after the series, not the file.
    assert_eq!(stdout, "series/keepers.series.haiyaku.db\n");
    let mut conn = common::db();
    let series = import_series(&mut conn, &sandbox.path("series/keepers.series.haiyaku.db")).unwrap();
    let stored = sandbox.database();
    assert_eq!(series, read_series(&stored, "keepers").unwrap());
    for book in [sandbox.sha256(), sandbox.sequel_sha256()] {
        assert_eq!(read_book(&conn, &book).unwrap(), read_book(&stored, &book).unwrap());
    }
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), links);
}

#[test]
fn a_series_named_as_a_book_beside_it_has_a_sidecar_of_its_own() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    // Beside the first book, and named as its file is.
    let file = "keeper.toml";
    let text = sandbox.keepers_series().replace(r#"name = "keepers""#, r#"name = "keeper""#);
    fs::write(sandbox.path(file), text.replace("../keeper.epub", "keeper.epub")).unwrap();
    status(&sandbox.run(&["series", file]), 0);

    let book = sandbox.path("keeper.haiyaku.db");
    assert_eq!(status(&sandbox.run(&["export", &sandbox.book]), 0), format!("{}\n", book.display()));
    assert_eq!(status(&sandbox.run(&["export", file]), 0), "keeper.series.haiyaku.db\n");
    // Each sidecar is still the one its export wrote.
    import_book(&mut common::db(), &book, &sandbox.sha256()).unwrap();
    let series = import_series(&mut common::db(), &sandbox.path("keeper.series.haiyaku.db")).unwrap();
    assert_eq!(series.name, "keeper");
}

#[test]
fn export_refuses_a_file_the_database_differs_from() {
    let sandbox = Sandbox::new();
    let good = sandbox.keepers_series();
    let file = sandbox.series_file(&good);
    let refused = |case: &str| {
        let output = sandbox.run(&["export", &file]);
        assert!(status(&output, 1).is_empty(), "{case}");
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with(&format!("haiyaku: {file}: ")), "{case}: {stderr}");
        assert!(stderr.contains(&format!("haiyaku series {file}")), "{case}: {stderr}");
        assert!(!sandbox.path("series/keepers.series.haiyaku.db").exists(), "{case}");
    };
    refused("no database");
    assert!(!sandbox.path("data").exists(), "export leaves no empty database behind");
    sandbox.store_keepers();
    refused("a database without the series");

    status(&sandbox.run(&["series", &file]), 0);
    let cases = [
        ("a book left out of an order", good.replace(r#"["return", "keeper"]"#, r#"["return"]"#)),
        (
            "the books of an order the other way round",
            good.replace(r#"["return", "keeper"]"#, r#"["keeper", "return"]"#),
        ),
        ("an order of another name", good.replace("sequel first", "backwards")),
        ("an order fewer", good[..good.rfind("[[order]]").unwrap()].to_owned()),
        // The sidecar carries the title, so a stored title the file has changed would go out under the old one.
        ("another title", good.replace(r#"title = "The Keepers""#, r#"title = "The Lighthouse Keepers""#)),
    ];
    for (case, written) in cases {
        assert_ne!(written, good, "{case}");
        sandbox.series_file(&written);
        refused(case);
    }
}

#[test]
fn link_stores_each_orders_links() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);
    // By rank: an order linked out of turn would be given the other order's replies.
    let backend = sandbox.recorded("replies", &[keepers_link_replies(), sequel_first_replies()].concat());
    let output = sandbox.link(&file, &backend, &[]);
    let stdout = status(&output, 0);
    assert_eq!(sandbox.links("published"), sandbox.keepers_linked());
    assert_eq!(sandbox.links("sequel first"), sandbox.sequel_first_linked());

    // Each candidate, and the one linked to, with its book's title.
    let report = [
        r#"The Keeper's Return, chapter 1, "Ada": candidates Ada Marsh (The Lighthouse Keeper); answer e1; linked to Ada Marsh (The Lighthouse Keeper); quote: Ada watched the boat come in."#,
        r#"The Keeper's Return, chapter 3, "Tom": candidates Tom Reed (The Lighthouse Keeper); answer e1; linked to Tom Reed (The Lighthouse Keeper); quote: Call me Tom, said the old sailor."#,
        "order published: 2 links, 2 calls, 0.0 s, cost unknown",
        r#"The Lighthouse Keeper, chapter 2, "Ada Marsh": candidates Ada (The Keeper's Return); answer e1; linked to Ada (The Keeper's Return); quote: The letter was addressed to Ada Marsh."#,
        r#"The Lighthouse Keeper, chapter 3, "Tom Reed": candidates the old sailor (The Keeper's Return); answer none; none chosen; no quote"#,
        "order sequel first: 1 link, 2 calls, 0.0 s, cost unknown",
    ];
    assert_eq!(stdout.lines().collect::<Vec<_>>(), report);
    let progress = [
        r#"order published: The Keeper's Return, chapter 1, "Ada": linked to Ada Marsh (The Lighthouse Keeper)"#,
        r#"order published: The Keeper's Return, chapter 3, "Tom": linked to Tom Reed (The Lighthouse Keeper)"#,
        r#"order sequel first: The Lighthouse Keeper, chapter 2, "Ada Marsh": linked to Ada (The Keeper's Return)"#,
        r#"order sequel first: The Lighthouse Keeper, chapter 3, "Tom Reed": none chosen"#,
    ];
    assert_eq!(text(&output.stderr).lines().collect::<Vec<_>>(), progress);
}

#[test]
fn link_reports_what_its_name_rules_turn_down() {
    let sandbox = Sandbox::new();
    sandbox.store(common::keeper());
    // The second book's one character shares Ada Marsh's surname, and their given names rule each other out.
    let mut sequel = common::keepers_return();
    sequel.entities = vec![Entity { id: "ned".into(), kind: EntityKind::Character }];
    let ned = Name {
        text: "Ned Marsh".into(),
        given: Some("Ned".into()),
        surname: Some("Marsh".into()),
        title: None,
        title_sex: None,
    };
    let body = EventBody::Introduced { entity: "ned".into(), name: ned };
    let at = Position { document: 1, block: 0 };
    sequel.events = vec![Event { seq: 1, chapter: 1, at, quote: "Ned Marsh rowed out to the rock.".into(), body }];
    sandbox.store_as(&sandbox.sequel, sequel);
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);

    let output = sandbox.link(&file, &sandbox.recorded("none", &[]), &[]);
    let stdout = status(&output, 0);
    let lines: Vec<&str> = stdout.lines().collect();
    let [published, published_total, sequel_first, sequel_first_total] = lines[..] else { panic!("{stdout}") };
    // In the order that reads the first book first, Ada Marsh's "Mrs. Marsh" of chapter 3 turns Ned Marsh down
    // again, and is not said twice.
    assert_eq!(
        [published, sequel_first],
        [
            r#"The Keeper's Return, chapter 1, "Ned Marsh": turned down Ada Marsh (The Lighthouse Keeper); their names conflict"#,
            r#"The Lighthouse Keeper, chapter 2, "Ada Marsh": turned down Ned Marsh (The Keeper's Return); their names conflict"#,
        ]
    );
    assert!(published_total.starts_with("order published: 0 links, 0 calls, "), "{stdout}");
    assert!(sequel_first_total.starts_with("order sequel first: 0 links, 0 calls, "), "{stdout}");
    assert_eq!(text(&output.stderr), "", "no call, so no progress");
}

#[test]
fn link_reports_a_candidate_linked_to_a_relative() {
    let sandbox = Sandbox::new();
    sandbox.store(common::keeper());
    // The second book's Ada is linked to Ada Marsh; her father's name makes Ada Marsh his candidate too.
    let mut sequel = common::keepers_return();
    sequel.entities = ["ada", "father"].map(|id| Entity { id: id.into(), kind: EntityKind::Character }).into();
    let ada = Name { text: "Ada".into(), given: Some("Ada".into()), surname: None, title: None, title_sex: None };
    let father = Name {
        text: "Mr. Marsh".into(),
        given: None,
        surname: Some("Marsh".into()),
        title: Some("Mr.".into()),
        title_sex: Some(Sex::Male),
    };
    let family =
        EventBody::Relation { from: "father".into(), to: "ada".into(), kind: RelationKind::Family, note: "".into() };
    let events = [
        (1, "Ada came back to the light in spring.", EventBody::Introduced { entity: "ada".into(), name: ada }),
        (2, "Mr. Marsh mended nets on the jetty.", EventBody::Introduced { entity: "father".into(), name: father }),
        (2, "Mr. Marsh was waiting for his daughter.", family),
    ];
    sequel.events = (1..)
        .zip(events)
        .map(|(seq, (chapter, quote, body))| Event {
            seq,
            chapter,
            at: Position { document: chapter, block: seq },
            quote: quote.into(),
            body,
        })
        .collect();
    sandbox.store_as(&sandbox.sequel, sequel);
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);

    let replies = [
        link_reply(Some("e1"), "Ada came back to the light in spring."),
        link_reply(Some("e1"), "Mr. Marsh mended nets on the jetty."),
    ];
    let output = sandbox.link(&file, &sandbox.recorded("replies", &replies), &["--order", "published"]);
    let stdout = status(&output, 0);
    let report = [
        r#"The Keeper's Return, chapter 1, "Ada": candidates Ada Marsh (The Lighthouse Keeper); answer e1; linked to Ada Marsh (The Lighthouse Keeper); quote: Ada came back to the light in spring."#,
        r#"The Keeper's Return, chapter 2, "Mr. Marsh": turned down Ada Marsh (The Lighthouse Keeper); related to Ada (The Keeper's Return), linked to it"#,
        "order published: 1 link, 1 call, 0.0 s, cost unknown",
    ];
    assert_eq!(stdout.lines().collect::<Vec<_>>(), report);
}

#[test]
fn link_order_option_links_one_order() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);
    // The order ranked second: a run that took the first, or every order, would give it the wrong replies.
    let backend = sandbox.recorded("replies", &sequel_first_replies());
    let stdout = status(&sandbox.link(&file, &backend, &["--order", "sequel first"]), 0);
    assert_eq!(sandbox.links("sequel first"), sandbox.sequel_first_linked());
    assert_eq!(sandbox.links("published"), []);
    assert_eq!(stdout.lines().last(), Some("order sequel first: 1 link, 2 calls, 0.0 s, cost unknown"));
    assert!(!stdout.contains("order published"), "{stdout}");

    // An order the series lacks is named, and nothing is linked.
    let output = sandbox.link(&file, &sandbox.recorded("unused", &[]), &["--order", "backwards"]);
    assert!(status(&output, 1).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with(&format!("haiyaku: {file}: ")), "{stderr}");
    assert!(stderr.contains(r#""backwards""#) && !stderr.contains("recorded replies"), "{stderr}");
    assert_eq!(sandbox.links("sequel first"), sandbox.sequel_first_linked());
}

#[test]
fn link_refuses_a_file_the_database_differs_from() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let good = sandbox.keepers_series();
    let file = sandbox.series_file(&good);
    let backend = sandbox.recorded("replies", &keepers_link_replies());
    let refused = |case: &str| {
        let output = sandbox.link(&file, &backend, &[]);
        assert!(status(&output, 1).is_empty(), "{case}");
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with(&format!("haiyaku: {file}: ")), "{case}: {stderr}");
        assert!(stderr.contains(&format!("haiyaku series {file}")), "{case}: {stderr}");
        // A run that made a call would have kept its reply.
        assert!(!sandbox.path("cache").exists(), "{case}");
    };
    refused("a database without the series");
    assert!(matches!(read_series(&sandbox.database(), "keepers"), Err(Error::NotFound(_))));

    status(&sandbox.run(&["series", &file]), 0);
    let links = sandbox.keepers_links();
    put_links(&mut sandbox.database(), "keepers", "published", &links, &sandbox.keepers_logs()).unwrap();
    sandbox.series_file(&good.replace("sequel first", "backwards"));
    refused("an order of another name");
    assert_eq!(sandbox.links("published"), links);
    assert_eq!(sandbox.links("sequel first"), []);
}

#[test]
fn failed_link_keeps_the_orders_links() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);
    // Each order holds a link by name, which the step never makes, so that links a run stored in their place would
    // show.
    let published = sandbox.keepers_links();
    let mut sequel_first = sandbox.sequel_first_linked();
    sequel_first[0].method = LinkMethod::Name;
    let mut conn = sandbox.database();
    put_links(&mut conn, "keepers", "published", &published, &sandbox.keepers_logs()).unwrap();
    put_links(&mut conn, "keepers", "sequel first", &sequel_first, &sandbox.order_logs("sequel first")).unwrap();

    // The first order's second call finds no reply: the order stores nothing, and the run goes no further.
    let output = sandbox.link(&file, &sandbox.recorded("one", &keepers_link_replies()[..1]), &[]);
    assert!(status(&output, 1).is_empty());
    let stderr = text(&output.stderr);
    let failure = stderr.lines().last().unwrap();
    assert!(failure.starts_with("haiyaku: order published: ") && failure.contains(r#""Tom""#), "{stderr}");
    assert_eq!(sandbox.links("published"), published);
    assert_eq!(sandbox.links("sequel first"), sequel_first);

    // The first order is linked and stored; the second finds no reply, and keeps the links it had.
    let output = sandbox.link(&file, &sandbox.recorded("two", &keepers_link_replies()), &[]);
    let stdout = status(&output, 1);
    assert_eq!(stdout.lines().last(), Some("order published: 2 links, 2 calls, 0.0 s, cost unknown"));
    assert!(text(&output.stderr).contains("haiyaku: order sequel first: "), "{}", text(&output.stderr));
    assert_eq!(sandbox.links("published"), sandbox.keepers_linked());
    assert_eq!(sandbox.links("sequel first"), sequel_first);
}

#[test]
fn second_link_uses_the_cache() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.series_file(&sandbox.keepers_series());
    status(&sandbox.run(&["series", &file]), 0);
    let replies = [keepers_link_replies(), sequel_first_replies()].concat();
    status(&sandbox.link(&file, &sandbox.recorded("uncached", &replies), &["--no-cache"]), 0);
    assert!(!sandbox.path("cache").exists());

    let backend = sandbox.recorded("replies", &replies);
    let first = status(&sandbox.link(&file, &backend, &[]), 0);
    // The same directory, so the same cache key, with nothing left in it: only the cache can answer the run.
    for number in 1..=replies.len() {
        fs::remove_file(sandbox.path("replies").join(format!("{number:03}.json"))).unwrap();
    }
    let mut conn = sandbox.database();
    for order in ["published", "sequel first"] {
        put_links(&mut conn, "keepers", order, &[], &sandbox.order_logs(order)).unwrap();
    }
    assert_eq!(status(&sandbox.link(&file, &backend, &[]), 0), first);
    assert_eq!(sandbox.links("published"), sandbox.keepers_linked());
    assert_eq!(sandbox.links("sequel first"), sandbox.sequel_first_linked());
}

#[test]
fn show_series_prints_the_series_cast() {
    let sandbox = Sandbox::new();
    sandbox.store(keeper_with_overturn());
    sandbox.store_as(&sandbox.sequel, common::keepers_return());
    let file = sandbox.load_keepers(&sandbox.keepers_linked());

    let stdout = status(&sandbox.show_sequel(&file, &["--at", "2"]), 0);
    // Ada is one character from the second book's chapter 1, where her link holds; the sailor's link holds only
    // from chapter 3. A chapter of the first book is named with its title.
    assert_eq!(
        section(&stdout, "Characters"),
        [
            "Ada Marsh (the keeper, Mrs. Marsh, Ada), The Lighthouse Keeper ch 1 to ch 2",
            "Pip, The Lighthouse Keeper ch 1-3",
            "Tom Reed (the stranger), The Lighthouse Keeper ch 2-3",
            "the old sailor, ch 1-2",
        ],
        "{stdout}"
    );
    let marked =
        format!("Ada Marsh -> Pip: owns, her gull [overturned in The Lighthouse Keeper ch 3: {OVERTURN_NOTE}]");
    assert!(section(&stdout, "Relations").contains(&marked.as_str()), "{stdout}");

    // The order that reads the second book first: it is alone in its cast, and the order has no links.
    let stdout = status(&sandbox.show_sequel(&file, &["--order", "sequel first", "--at", "2"]), 0);
    assert_eq!(section(&stdout, "Characters"), ["Ada, ch 1-2", "the old sailor, ch 1-2"], "{stdout}");
}

#[test]
fn show_series_at_the_last_chapter_applies_its_link() {
    let sandbox = Sandbox::new();
    sandbox.store(common::keeper());
    let mut sequel = common::keepers_return();
    // Chapter 3's document ends the spine, so no reader's document is past it: only a fold by chapter number
    // counts that chapter, and the sailor's link in it, as read.
    sequel.documents.retain(|document| document.path != "back.xhtml");
    sandbox.store_as(&sandbox.sequel, sequel);
    let file = sandbox.load_keepers(&sandbox.keepers_linked());

    let stdout = status(&sandbox.show_sequel(&file, &["--at", "3"]), 0);
    assert_eq!(
        section(&stdout, "Characters"),
        [
            "Ada Marsh (the keeper, Mrs. Marsh, Ada), The Lighthouse Keeper ch 1 to ch 2",
            "Pip, The Lighthouse Keeper ch 1-1",
            "Tom Reed (the stranger, the old sailor, Tom), The Lighthouse Keeper ch 2 to ch 3",
        ],
        "{stdout}"
    );
}

#[test]
fn show_series_html_steps_through_the_order() {
    let sandbox = Sandbox::new();
    sandbox.store(keeper_with_overturn());
    sandbox.store_as(&sandbox.sequel, common::keepers_return());
    // The sailor's link alone, so that both books' `ada` are in the cast at once.
    let file = sandbox.load_keepers(&sandbox.keepers_linked()[1..]);
    let write = |name: &str, more: &[&str]| {
        let page = sandbox.path(name);
        status(&sandbox.show_sequel(&file, &[&["--html", page.to_str().unwrap()][..], more].concat()), 0);
        page_data(&fs::read_to_string(&page).unwrap())
    };
    let data = write("series.html", &[]);
    assert_eq!(data["books"], json!(["The Lighthouse Keeper", "The Keeper's Return"]), "{data}");
    assert_eq!(data["title"], "The Keeper's Return", "{data}");
    let steps = data["steps"].as_array().unwrap();
    // Each step's book and chapter: nothing read, then each chapter of each book.
    let places: Vec<Value> = steps.iter().map(|step| json!([step["book"], step["chapter"]])).collect();
    assert_eq!(places, [[0, 0], [0, 1], [0, 2], [0, 3], [1, 1], [1, 2], [1, 3]].map(|place| json!(place)), "{data}");
    assert_eq!(data["at"], 6, "by default, the book's last chapter extracted: {data}");
    assert_eq!(write("start.html", &["--at", "0"])["at"], 3, "none of the book read is the first book whole");
    assert_eq!(write("first.html", &["--at", "1"])["at"], 4);

    for step in steps {
        let ids: Vec<&str> =
            step["characters"].as_array().unwrap().iter().map(|character| character["id"].as_str().unwrap()).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len(), "an id twice: {step}");
    }
    let character = |step: usize, id: &str| {
        let characters = steps[step]["characters"].as_array().unwrap();
        characters.iter().find(|character| character["id"] == id).unwrap_or_else(|| panic!("no {id}: {data}"))
    };
    // In the second book's first chapter: each book's Ada, first met in her own book.
    assert_eq!(character(4, "0:ada")["firstBook"], 0, "{data}");
    assert_eq!(character(4, "1:ada")["firstBook"], 1, "{data}");
    assert_eq!(character(4, "0:pip")["lastBook"], 0, "{data}");
    let fact = |step: usize, a: &str, b: &str| {
        let relations = steps[step]["relations"].as_array().unwrap();
        let pair = relations.iter().find(|relation| relation["a"] == a && relation["b"] == b);
        pair.unwrap_or_else(|| panic!("no {a} and {b}: {data}"))["facts"][0].clone()
    };
    // A fact of the first book, seen from the second, keeps its book, and so does the overturn that marks it.
    let owns = fact(4, "0:ada", "0:pip");
    assert_eq!((&owns["firstBook"], &owns["overturned"]["book"]), (&json!(0), &json!(0)), "{data}");
    assert_eq!(fact(5, "1:ada", "1:sailor")["firstBook"], 1, "{data}");

    let sailor = json!([{
        "later": "1:sailor",
        "name": "the old sailor",
        "earlier": "0:tom",
        "book": 1,
        "chapter": 3,
        "quote": "Call me Tom, said the old sailor.",
    }]);
    assert_eq!(steps[6]["links"], sailor, "{data}");
    assert_eq!(steps[5]["links"], json!([]), "the sailor's link holds from chapter 3: {data}");
    assert!(character(6, "0:tom")["merged"].as_array().unwrap().contains(&json!("1:sailor")), "{data}");
}

#[test]
fn show_series_html_of_the_first_book_has_no_links() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    let file = sandbox.load_keepers(&sandbox.keepers_linked());
    let page = sandbox.path("first.html");
    let output = sandbox.run(&["show", &sandbox.book, "--series", &file, "--html", page.to_str().unwrap()]);
    status(&output, 0);
    let page = fs::read_to_string(&page).unwrap();
    let data = page_data(&page);
    assert_eq!(data["books"], json!(["The Lighthouse Keeper"]), "{data}");
    // Both of the order's links are the second book's, which a reader of the first has not reached.
    for step in data["steps"].as_array().unwrap() {
        assert_eq!(step["links"], json!([]), "{step}");
    }
    assert!(!data_text(&page).contains(r#""1:"#), "no id of a second book: {data}");
}

#[test]
fn show_series_needs_the_book_in_the_order() {
    let sandbox = Sandbox::new();
    sandbox.store_keepers();
    // The default order holds the first book alone; the other holds both.
    let file = sandbox.series_file(&sandbox.keepers_series().replace(r#"["keeper", "return"]"#, r#"["keeper"]"#));
    status(&sandbox.run(&["series", &file]), 0);

    let output = sandbox.show_sequel(&file, &[]);
    assert!(status(&output, 1).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with(&format!("haiyaku: {file}: ")), "{stderr}");
    assert!(stderr.contains(r#""published""#) && stderr.contains(&sandbox.sequel), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "the error is said once: {stderr}");

    status(&sandbox.show_sequel(&file, &["--order", "sequel first"]), 0);
}

#[test]
fn order_without_series_is_a_usage_error() {
    let sandbox = Sandbox::new();
    sandbox.store(common::keeper());
    let output = sandbox.show(&["--order", "published"]);
    assert!(status(&output, 2).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("--series"), "{stderr}");
}
