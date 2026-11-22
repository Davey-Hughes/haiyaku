//! `haiyaku show`: the cast an extracted book has after a chapter, drawn as text, or with `--html` as a page
//! that steps through it chapter by chapter.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use haiyaku::Error;
use haiyaku::fold::{Cast, EntityKey, fold_book};
use haiyaku::store::{read_book, sha256_file};

use crate::paths::{self, Env};
use crate::{Failure, open_existing_database, say, view};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The book, an EPUB that has been extracted
    book: PathBuf,
    /// The cast after this chapter, 0 for none read yet; by default after the last chapter extracted. Chapters
    /// count from the first entry extracted, not from the table of contents: after extract --chapters 5-9,
    /// chapter 1 is entry 5. With --html, the chapter the page opens at
    #[arg(long, value_name = "CHAPTER")]
    at: Option<u32>,
    /// Write a page to FILE that steps through the cast chapter by chapter, from none read to the last chapter
    /// extracted, in place of printing it. The page is for checking a run, not for a reader: it holds every
    /// character and every quote up to the last chapter extracted, whatever --at says
    #[arg(long, value_name = "FILE")]
    html: Option<PathBuf>,
}

/// Prints the cast of `args.book` after the chapter `--at` names, from the database at `db`, by default the
/// one `env` points to; or, with `--html`, writes the page of every chapter's cast to the file it names.
pub fn run(args: &Args, db: Option<&Path>, env: Env<'_>) -> Result<(), Failure> {
    // A book is stored under the hash of its file, so the file, not its name, says which book it is.
    let sha256 = sha256_file(&args.book).map_err(|error| Failure(format!("{}: {error}", args.book.display())))?;
    let database = match db {
        Some(path) => path.to_owned(),
        None => paths::database(env)?,
    };
    let not_extracted =
        || Failure(format!("{} has not been extracted into {}", args.book.display(), database.display()));
    let Some(conn) = open_existing_database(&database)? else { return Err(not_extracted()) };
    let log = match read_book(&conn, &sha256) {
        Ok(log) => log,
        Err(Error::NotFound(_)) => return Err(not_extracted()),
        Err(error) => return Err(error.into()),
    };
    let last = log.chapters.last().map_or(0, |chapter| chapter.number);
    let at = args.at.unwrap_or(last);
    if at > last {
        return Err(Failure(format!(
            "--at {at} is past the last chapter extracted from {}: --at takes 0-{last}",
            args.book.display()
        )));
    }
    match &args.html {
        Some(page) => {
            fs::write(page, view::page(&log, last, at)).map_err(|error| Failure(format!("{}: {error}", page.display())))
        }
        None => say(&draw(&fold_book(&log, at))),
    }
}

/// `cast` as lines of text: a `Characters` section, a line each with the name, the other names in
/// parentheses and the chapters from the first to the last that names the character; then a `Relations`
/// section, a line per fact with its two ends, its kind and its note, and for a fact a later chapter overturned,
/// that chapter and the overturn's note in brackets.
fn draw(cast: &Cast) -> String {
    let names: HashMap<&EntityKey, &str> =
        cast.characters.iter().map(|character| (&character.key, character.name.as_str())).collect();
    // Every end of a relation is a character of the cast; should one not be, its id stands in, where indexing
    // would panic over a line of output.
    let name = |key: &EntityKey| names.get(key).map_or_else(|| key.id.clone(), |name| (*name).to_owned());

    let mut lines = vec!["Characters".to_owned()];
    for character in &cast.characters {
        let aliases =
            if character.aliases.is_empty() { String::new() } else { format!(" ({})", character.aliases.join(", ")) };
        lines.push(format!("  {}{aliases}, ch {}-{}", character.name, character.first.chapter, character.last.chapter));
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
                format!(" [overturned in ch {}{overturn_note}]", overturn.at.chapter)
            });
            lines.push(format!("  {ends}: {}{note}{overturned}", fact.kind.as_str()));
        }
    }
    lines.join("\n")
}
