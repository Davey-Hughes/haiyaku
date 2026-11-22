//! The cast at a reader's position: the fold of what the database holds for the chapters the reader has
//! finished, in the book alone or with the books before it in a reading order of its series.

use rusqlite::Connection;

use crate::fold::{Cast, Folder};
use crate::store::{Link, finished_chapters, in_snapshot, read_events, read_links, read_series};
use crate::{Error, Result};

/// How much the cast at a position takes in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The book the reader is in, alone.
    Book,
    /// The book and those before it in a reading order of `series`, with the characters a link joins shown
    /// as one: the order of that name, or the series' default order for `None`.
    Series { series: String, order: Option<String> },
}

/// The characters a reader at `document` of `book` has met and what is known of them: the fold of the
/// chapters of `book` the reader has finished (see [`finished_chapters`]) and, in a series, of every book
/// before it in the order, each to its end.
///
/// A link joins its two characters once the reader has finished the chapter it holds from, so one found in
/// the chapter the reader is still in does not show.
///
/// The position, the series, the links and the events are all read from one state of the database: a book
/// that another connection rewrites during the call is in the cast as it was or as it has become, never as
/// some of each. Inside a transaction of the caller's, which stays open, that state is the transaction's.
///
/// # Errors
///
/// [`Error::NotFound`] when the book is not stored, when it has no such document, when the series or the
/// order is not stored, and when the order does not hold the book; [`Error::Invalid`] and [`Error::Sqlite`] as
/// [`read_events`] and [`read_links`] return them.
pub fn cast_at(conn: &Connection, book: &str, document: &str, scope: &Scope) -> Result<Cast> {
    // One snapshot, because the cut-off and the events it is applied to are separate statements: with a
    // book rewritten between them, the old log's count of finished chapters would cut the new log's
    // events, and where the new chapters start later that shows chapters the reader has not read.
    in_snapshot(conn, |conn| cast_in_snapshot(conn, book, document, scope))
}

fn cast_in_snapshot(conn: &Connection, book: &str, document: &str, scope: &Scope) -> Result<Cast> {
    // First, because it is what refuses a book that is not stored: such a book has no events, and would
    // otherwise fold to a cast with nobody in it.
    let finished = finished_chapters(conn, book, document)?;
    let mut folder = Folder::new();
    match scope {
        Scope::Book => fold_in(&mut folder, conn, book, Some(finished), &[])?,
        Scope::Series { series, order } => {
            let stored = read_series(conn, series)?;
            let order = match order {
                Some(name) => stored
                    .orders
                    .iter()
                    .find(|listed| listed.name == *name)
                    .ok_or_else(|| Error::NotFound(format!("order {name} of series {series}")))?,
                None => stored
                    .orders
                    .first()
                    .ok_or_else(|| Error::NotFound(format!("a reading order of series {series}")))?,
            };
            let Some(place) = order.books.iter().position(|listed| listed == book) else {
                return Err(Error::NotFound(format!("book {book} in order {} of series {series}", order.name)));
            };
            let links = read_links(conn, series, &order.name)?;
            for earlier in &order.books[..place] {
                fold_in(&mut folder, conn, earlier, None, &links)?;
            }
            fold_in(&mut folder, conn, book, Some(finished), &links)?;
        }
    }
    Ok(folder.finish())
}

/// Folds in the next book of the reading: its chapters up to `up_to`, or all of them for `None`, and those
/// of `links` that join one of its characters to an earlier book's from a chapter among them.
fn fold_in(folder: &mut Folder, conn: &Connection, book: &str, up_to: Option<u32>, links: &[Link]) -> Result<()> {
    folder.start_book(book);
    let mut due: Vec<&Link> = links
        .iter()
        .filter(|link| link.later.book == book && up_to.is_none_or(|finished| link.chapter <= finished))
        .collect();
    due.sort_by_key(|link| link.chapter);
    let mut due = due.into_iter().peekable();

    for event in read_events(conn, book, up_to)? {
        // A link is applied once its whole chapter is in, not at its block: a reader's position is a
        // finished chapter, so no cast is asked for between the two. By then the later character has been
        // introduced, which the merge needs, or it does nothing. The merge moves what the chapter said of
        // the later character before then, and takes its name where that is fuller than the earlier
        // character's, so a fuller name shows whether the text gave it before the link was applied or after.
        while let Some(link) = due.next_if(|link| link.chapter < event.chapter) {
            folder.merge(&link.earlier, &link.later);
        }
        folder.apply(&event);
    }
    for link in due {
        folder.merge(&link.earlier, &link.later);
    }
    Ok(())
}
