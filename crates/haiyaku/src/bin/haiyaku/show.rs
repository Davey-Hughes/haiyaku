//! `haiyaku show`: the cast an extracted book has after a chapter, alone or with `--series` after the books
//! before it in a reading order of its series, drawn as text, or with `--html` as a page that steps through it
//! chapter by chapter.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use haiyaku::Error;
use haiyaku::fold::{Cast, ChapterRef, EntityKey};
use haiyaku::fold_series;
use haiyaku::log::BookLog;
use haiyaku::store::{Link, read_book, read_links, sha256_file};
use rusqlite::Connection;

use crate::paths::{self, Env};
use crate::series::{self, SeriesFile};
use crate::{Failure, open_existing_database, say, view};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The book, an EPUB that has been extracted
    book: PathBuf,
    /// The cast after this chapter, 0 for none read yet; by default after the last chapter extracted. Chapters
    /// count from the first entry extracted, not from the table of contents: after extract --chapters 5-9,
    /// chapter 1 is entry 5. With --series, a chapter of this book, read after every book before it. With --html,
    /// the chapter the page opens at
    #[arg(long, value_name = "CHAPTER")]
    at: Option<u32>,
    /// Write a page to FILE that steps through the cast chapter by chapter, from none read to the last chapter
    /// extracted, in place of printing it. The page is for checking a run, not for a reader: it holds every
    /// character and every quote up to the last chapter extracted, whatever --at says
    #[arg(long, value_name = "FILE")]
    html: Option<PathBuf>,
    /// The series file of a series stored with haiyaku series: the cast is that of a reader who has read the books
    /// before this one in a reading order of the series, whole, with the characters the order's links join shown
    /// as one; and the page steps through every chapter of those books first
    #[arg(long, value_name = "FILE")]
    series: Option<PathBuf>,
    /// The reading order of --series, by its name; by default the series' first
    #[arg(long, value_name = "NAME", requires = "series")]
    order: Option<String>,
}

/// Prints the cast of `args.book` after the chapter `--at` names, alone or after the books before it in a reading
/// order of `--series`, from the database at `db`, by default the one `env` points to; or, with `--html`, writes
/// the page of every chapter's cast to the file it names.
pub fn run(args: &Args, db: Option<&Path>, env: Env<'_>) -> Result<(), Failure> {
    // A book is stored under the hash of its file, so the file, not its name, says which book it is.
    let sha256 = sha256_file(&args.book).map_err(|error| Failure(format!("{}: {error}", args.book.display())))?;
    let file = args.series.as_deref().map(series::read).transpose()?;
    let database = match db {
        Some(path) => path.to_owned(),
        None => paths::database(env)?,
    };
    let not_extracted =
        || Failure(format!("{} has not been extracted into {}", args.book.display(), database.display()));
    let Some(mut conn) = open_existing_database(&database)? else { return Err(not_extracted()) };
    // The books in reading order, the one shown last, and the links between them.
    let (logs, links) = match &file {
        Some(file) => in_series(&mut conn, file, args.order.as_deref(), &sha256, &args.book)?,
        None => match read_book(&conn, &sha256) {
            Ok(log) => (vec![log], Vec::new()),
            Err(Error::NotFound(_)) => return Err(not_extracted()),
            Err(error) => return Err(error.into()),
        },
    };
    let Some(shown) = logs.last() else { return Err(not_extracted()) };
    let last = shown.chapters.last().map_or(0, |chapter| chapter.number);
    let at = args.at.unwrap_or(last);
    if at > last {
        return Err(Failure(format!(
            "--at {at} is past the last chapter extracted from {}: --at takes 0-{last}",
            args.book.display()
        )));
    }
    match (&args.html, &file) {
        (Some(page), None) => write_page(page, &view::page(shown, at)),
        (Some(page), Some(_)) => write_page(page, &view::series_page(&logs, &links, at)),
        (None, _) => say(&draw(&fold_series(&logs, Some(at), &links), &Chapters::new(&logs))),
    }
}

/// The logs of the books of the reading order `order` of the series `file` names, or of its first order, up to
/// and including the book `sha256`, whose path is `book`, and the order's links, all read from one state of the
/// database.
fn in_series(
    conn: &mut Connection,
    file: &SeriesFile,
    order: Option<&str>,
    sha256: &str,
    book: &Path,
) -> Result<(Vec<BookLog>, Vec<Link>), Failure> {
    // One read transaction: each read on its own sees what was committed just before it, and a book extracted again
    // between the logs and the links would have the old log's links folded into the new log's characters. Dropped
    // at the end, it rolls back, which for a transaction that wrote nothing only ends it.
    let tx = conn.transaction().map_err(haiyaku::Error::from)?;
    let conn = &*tx;
    let stored = series::stored(conn, file)?;
    let order = match order {
        Some(name) => series::order(file, &stored, name)?,
        None => stored
            .orders
            .first()
            .ok_or_else(|| Failure(format!("{}: series {} has no reading order", file.path.display(), stored.name)))?,
    };
    let Some(place) = order.books.iter().position(|listed| listed == sha256) else {
        return Err(Failure(format!(
            "{}: order {:?} of series {} does not hold {}",
            file.path.display(),
            order.name,
            stored.name,
            book.display()
        )));
    };
    let logs =
        order.books[..=place].iter().map(|listed| read_book(conn, listed)).collect::<haiyaku::Result<Vec<_>>>()?;
    Ok((logs, read_links(conn, &stored.name, &order.name)?))
}

/// Writes `text` to the file `page`.
fn write_page(page: &Path, text: &str) -> Result<(), Failure> {
    fs::write(page, text).map_err(|error| Failure(format!("{}: {error}", page.display())))
}

/// How `show` writes a chapter: `ch <n>` in the book shown, `<title> ch <n>` in another.
struct Chapters<'a> {
    /// The hash of the book shown.
    shown: &'a str,
    /// Each book's title, by its hash.
    titles: HashMap<&'a str, &'a str>,
}

impl<'a> Chapters<'a> {
    /// For `logs` in reading order, the book shown last.
    fn new(logs: &'a [BookLog]) -> Self {
        let titles = logs.iter().map(|log| (log.book.sha256.as_str(), log.book.title.as_str())).collect();
        Chapters { shown: logs.last().map_or("", |log| log.book.sha256.as_str()), titles }
    }

    /// What comes before a chapter's `ch`: nothing in the book shown, the book's title and a space in another.
    fn book(&self, book: &str) -> String {
        if book == self.shown {
            return String::new();
        }
        // Every chapter of a cast is of a book it was folded from; should one not be, its hash stands in, where
        // indexing would panic over a line of output.
        format!("{} ", self.titles.get(book).copied().unwrap_or(book))
    }

    fn one(&self, chapter: &ChapterRef) -> String {
        format!("{}ch {}", self.book(&chapter.book), chapter.chapter)
    }

    /// The chapters from `first` to `last`: `ch 2-5` in one book, its title before it when that is not the book
    /// shown, and `<first> to <last>` across two.
    fn span(&self, first: &ChapterRef, last: &ChapterRef) -> String {
        if first.book == last.book {
            format!("{}ch {}-{}", self.book(&first.book), first.chapter, last.chapter)
        } else {
            format!("{} to {}", self.one(first), self.one(last))
        }
    }
}

/// `cast` as lines of text: a `Characters` section, a line each with the name, the other names in
/// parentheses and the chapters from the first to the last that names the character; then a `Relations`
/// section, a line per fact with its two ends, its kind and its note, and for a fact a later chapter overturned,
/// that chapter and the overturn's note in brackets. Each chapter is written as `chapters` writes it.
fn draw(cast: &Cast, chapters: &Chapters<'_>) -> String {
    let names: HashMap<&EntityKey, &str> =
        cast.characters.iter().map(|character| (&character.key, character.name.as_str())).collect();
    // Every end of a relation is a character of the cast; should one not be, its id stands in, where indexing
    // would panic over a line of output.
    let name = |key: &EntityKey| names.get(key).map_or_else(|| key.id.clone(), |name| (*name).to_owned());

    let mut lines = vec!["Characters".to_owned()];
    for character in &cast.characters {
        let aliases =
            if character.aliases.is_empty() { String::new() } else { format!(" ({})", character.aliases.join(", ")) };
        let span = chapters.span(&character.first, &character.last);
        lines.push(format!("  {}{aliases}, {span}", character.name));
    }
    lines.push(String::new());
    lines.push("Relations".to_owned());
    for relation in &cast.relations {
        for fact in &relation.facts {
            // The pair is held in key order, which says nothing about who did what: only a directed fact's
            // `from` gives the order of its ends.
            let ends = match &fact.from {
                Some(from) => {
                    let to = if *from == relation.a { &relation.b } else { &relation.a };
                    format!("{} -> {}", name(from), name(to))
                }
                None => format!("{} - {}", name(&relation.a), name(&relation.b)),
            };
            let note = if fact.note.is_empty() { String::new() } else { format!(", {}", fact.note) };
            let overturned = fact.overturned.as_ref().map_or_else(String::new, |overturn| {
                let overturn_note =
                    if overturn.note.is_empty() { String::new() } else { format!(": {}", overturn.note) };
                format!(" [overturned in {}{overturn_note}]", chapters.one(&overturn.at))
            });
            lines.push(format!("  {ends}: {}{note}{overturned}", fact.kind.as_str()));
        }
    }
    lines.join("\n")
}
