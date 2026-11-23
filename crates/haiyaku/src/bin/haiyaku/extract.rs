//! `haiyaku extract`: a book's table of contents, or a range of its entries extracted into the database.

use std::path::{Path, PathBuf};
use std::time::Duration;

use haiyaku::Error;
use haiyaku::book::Book;
use haiyaku::extract::{Options, extract_book};
use haiyaku::llm::{Settings, backend};
use haiyaku::log::Chapter;
use haiyaku::store::{read_book, write_book};
use rusqlite::Connection;

use crate::paths::{self, Env};
use crate::{Failure, clock, open_database, say, tell};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The book, an EPUB
    book: PathBuf,
    /// The entries of the table of contents to extract, as A-B or one number, numbered as the listing without
    /// this option numbers them. A run replaces the book's stored log, which takes --replace when there is one
    #[arg(long, value_name = "A-B", value_parser = entries)]
    chapters: Option<Entries>,
    /// The model: claude (Sonnet), claude:MODEL, ollama:MODEL, openai:BASE_URL#MODEL, or replay:DIR for the
    /// replies recorded in DIR. A replay hands out its replies in the order of the calls, so a rerun that finds
    /// chapters in the cache gives the first chapter it calls for the first reply, not the one recorded for
    /// that chapter: rerun a replay with --no-cache
    #[arg(long, value_name = "SPEC", default_value = "claude", requires = "chapters")]
    backend: String,
    /// How long one call to the model may take
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 600,
        value_parser = clap::value_parser!(u64).range(1..),
        requires = "chapters"
    )]
    timeout: u64,
    /// Keep no cache: no reply is read from it or kept in it, so every chapter calls the model, and no rejected
    /// items are written to it
    #[arg(long, requires = "chapters")]
    no_cache: bool,
    /// Extract a book the database already holds: the run replaces its stored log, and drops the book's series
    /// links, which name the old log's characters
    #[arg(long, requires = "chapters")]
    replace: bool,
}

/// Entries `first` to `last` of a table of contents, both included.
#[derive(Debug, Clone, Copy)]
struct Entries {
    first: u32,
    last: u32,
}

/// `A-B`, or one number `N` for `N-N`. Whether the book has those entries is checked against the book.
fn entries(text: &str) -> Result<Entries, String> {
    let (first, last) = text.split_once('-').unwrap_or((text, text));
    let number = |part: &str| part.trim().parse().map_err(|_| "expected A-B, as in 1-12, or one number".to_owned());
    Ok(Entries { first: number(first)?, last: number(last)? })
}

/// Lists `args.book`'s table of contents, or, with `--chapters`, extracts those entries into the database at
/// `db`, by default the one `env` points to.
pub fn run(args: &Args, db: Option<&Path>, env: Env<'_>) -> Result<(), Failure> {
    // The library's error says what is wrong with the file, not which file it is.
    let book = Book::open(&args.book).map_err(|error| Failure(format!("{}: {error}", args.book.display())))?;
    let Some(Entries { first, last }) = args.chapters else { return list(&book) };
    // Before anything is built or written, so that a range the book does not have costs nothing and leaves
    // nothing behind.
    book.chapters(first, last)?;
    let database = match db {
        Some(path) => path.to_owned(),
        None => paths::database(env)?,
    };
    let cache = if args.no_cache { None } else { Some(paths::cache(env)?) };
    // Opened, and the book looked for, before the backend is built: a database that cannot be written, or
    // that holds the book already, fails the run before a call is paid for.
    let mut conn = open_database(&database)?;
    if !args.replace && holds(&conn, &book.sha256)? {
        return Err(Failure(format!(
            "{} already holds {}: run with --replace to replace its stored log",
            database.display(),
            args.book.display()
        )));
    }
    let settings = Settings {
        timeout: Duration::from_secs(args.timeout),
        ollama_host: env("OLLAMA_HOST").map(|host| host.to_string_lossy().into_owned()),
        ..Settings::default()
    };
    let backend = backend(&args.backend, &settings)?;
    let extracted_at = clock::now();
    let options = Options { first, last, cache: cache.as_deref(), extracted_at: &extracted_at };
    // A progress line that cannot be written does not stop a run whose chapters are paid for: the first such
    // failure is held, and is the command's once the log is stored.
    let mut unsaid = None;
    let log = extract_book(&book, backend.as_ref(), &options, &mut |chapter| {
        if unsaid.is_none() {
            unsaid = tell(&progress(chapter)).err();
        }
    })?;
    // The run's one stored write, in one transaction: a run that fails before it leaves every book's rows as they were.
    write_book(&mut conn, &log)?;
    say(&totals(&log.chapters))?;
    unsaid.map_or(Ok(()), Err)
}

/// Whether the database holds a log of the book whose file hashes to `sha256`.
fn holds(conn: &Connection, sha256: &str) -> Result<bool, Failure> {
    match read_book(conn, sha256) {
        Ok(_) => Ok(true),
        Err(Error::NotFound(_)) => Ok(false),
        // Rows of the book that no longer read back as a log are still rows a run would replace.
        Err(Error::Invalid(_)) => Ok(true),
        Err(error) => Err(error.into()),
    }
}

/// A line for each entry of the table of contents, with its number, label, documents and characters of text,
/// then a line saying how to choose the entries to extract.
fn list(book: &Book) -> Result<(), Failure> {
    let count = u32::try_from(book.toc.len()).unwrap_or(u32::MAX);
    let width = count.to_string().len();
    let mut lines = Vec::new();
    for chapter in book.chapters(1, count)? {
        let documents = chapter.documents.len();
        let characters = chapter.text().chars().count();
        lines.push(format!(
            "{:>width$}  {}  ({}, {})",
            chapter.toc_entry,
            chapter.title,
            counted(documents, "document"),
            counted(characters, "character"),
        ));
    }
    lines.push("Pick the entries the story runs over, without front or back matter, with --chapters A-B.".into());
    say(&lines.join("\n"))
}

/// A finished chapter's line. Its number counts from the first entry extracted and its entry from the start
/// of the table of contents, which is what the listing shows, so the line gives both.
fn progress(chapter: &Chapter) -> String {
    format!(
        "{} (entry {}) {}: {:.1} s, {} kept, {} rejected, {}",
        chapter.number,
        chapter.toc_entry,
        chapter.title,
        chapter.seconds,
        chapter.kept,
        chapter.rejected,
        cost(chapter.cost_usd),
    )
}

/// What the run came to over all its chapters.
fn totals(chapters: &[Chapter]) -> String {
    let kept: u64 = chapters.iter().map(|chapter| u64::from(chapter.kept)).sum();
    let rejected: u64 = chapters.iter().map(|chapter| u64::from(chapter.rejected)).sum();
    let seconds: f64 = chapters.iter().map(|chapter| chapter.seconds).sum();
    // One chapter whose cost is not known makes the sum unknown: the costs that are known would read as the
    // whole.
    let cost_usd: Option<f64> = chapters.iter().map(|chapter| chapter.cost_usd).sum();
    format!(
        "{}, {kept} kept, {rejected} rejected, {seconds:.1} s, {}",
        counted(chapters.len(), "chapter"),
        cost(cost_usd)
    )
}

/// A cost as the run's totals write it: in dollars to the hundredth of a cent, or `cost unknown`.
pub fn cost(cost_usd: Option<f64>) -> String {
    cost_usd.map_or_else(|| "cost unknown".to_owned(), |usd| format!("${usd:.4}"))
}

/// `count` and `noun`, plural unless `count` is 1.
pub fn counted(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}
