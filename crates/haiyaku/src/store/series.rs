//! Series, their reading orders and the links between the books of an order, written to the database and
//! read back; and how much of a book a reader at one of its documents has finished.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::book::{is_stored, unknown};
use super::{in_snapshot, require_foreign_keys};
use crate::fold::EntityKey;
use crate::log::{BookLog, Position};
use crate::{Error, Result};

/// Books that share characters, and the orders they can be read in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Series {
    pub name: String,
    pub title: String,
    /// At least one. The first is the default.
    pub orders: Vec<Order>,
}

/// One order to read the books of a series in. It need not hold every book of the series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub name: String,
    /// The books' `sha256`, in reading order.
    pub books: Vec<String>,
}

/// A character of a later book of a reading order is the same person as a character of an earlier one.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub later: EntityKey,
    pub earlier: EntityKey,
    pub method: LinkMethod,
    /// The chapter of the later book that holds `at`: the two are one for a reader who has finished it.
    pub chapter: u32,
    /// The block of the later book from which the link holds.
    pub at: Position,
}

/// How a link was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkMethod {
    /// The two carry the same name.
    Name,
    /// A model judged them the same, and gave this quote as its evidence.
    Model { quote: String },
}

/// Stores `series`, replacing the series of the same name, in one transaction: on any error the database is
/// as it was. Returns how many links the series it replaced had, which are gone with it; 0 when none was stored.
///
/// Replacing a series removes every link of every one of its orders, those of an order that comes back
/// unchanged included: a link is only known to hold for the list of books it was made against.
///
/// # Errors
///
/// [`Error::ForeignKeysOff`] when `PRAGMA foreign_keys` is off on `conn`; [`Error::Invalid`] when the series
/// has no order, or two orders of one name, or an order lists a book twice; [`Error::NotFound`] naming
/// `book <sha256>` when an order lists a book that is not stored; [`Error::Sqlite`] when a statement fails,
/// and when `conn` is already inside a transaction.
pub fn put_series(conn: &mut Connection, series: &Series) -> Result<u32> {
    // The foreign keys are what takes the old orders, their books and their links with the series' row.
    require_foreign_keys(conn)?;
    check_series(series)?;
    // Immediate, so that the links counted are the ones the replacement takes: in a deferred transaction another
    // connection could store an order's links between the count and the write. Dropping the transaction on an
    // error rolls it back.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let gone = tx.query_row("SELECT count(*) FROM haiyaku_link WHERE series = ?1", [&series.name], |row| row.get(0))?;
    write_series(&tx, series)?;
    tx.commit()?;
    Ok(gone)
}

/// The rules of [`put_series`] that need nothing from the database.
pub(super) fn check_series(series: &Series) -> Result<()> {
    if series.orders.is_empty() {
        return Err(Error::Invalid(format!("series {} has no reading order", series.name)));
    }
    for (listed, order) in series.orders.iter().enumerate() {
        // The schema would refuse the second as well, with an error that names its columns and not the order.
        if series.orders[..listed].iter().any(|before| before.name == order.name) {
            return Err(Error::Invalid(format!("series {} lists two orders named {}", series.name, order.name)));
        }
        let twice = order.books.iter().enumerate().find(|(listed, book)| order.books[..*listed].contains(book));
        if let Some((_, book)) = twice {
            return Err(Error::Invalid(format!(
                "order {} of series {} lists book {book} twice",
                order.name, series.name
            )));
        }
    }
    Ok(())
}

/// [`put_series`] inside the transaction its caller holds. The caller has checked that the foreign keys are on and
/// that `series` passes [`check_series`].
pub(super) fn write_series(conn: &Connection, series: &Series) -> Result<()> {
    conn.execute("DELETE FROM haiyaku_series WHERE name = ?1", [&series.name])?;
    conn.execute("INSERT INTO haiyaku_series (name, title) VALUES (?1, ?2)", [&series.name, &series.title])?;
    insert_orders(conn, series)
}

/// The series as [`put_series`] stored it, its orders by rank.
///
/// All of it is read from one state of the database: a series that another connection replaces during the
/// call comes back as it was or as it has become, never as some of each. Inside a transaction of the
/// caller's, which stays open, that state is the transaction's. On a connection outside a transaction, the
/// read is refused with [`Error::Sqlite`] while a write statement of the caller's is still in progress on it.
///
/// # Errors
///
/// [`Error::NotFound`] naming `series <name>` when no such series is stored; [`Error::Sqlite`] when the
/// database cannot be read.
pub fn read_series(conn: &Connection, name: &str) -> Result<Series> {
    // One snapshot, because the orders and the books of each are separate statements: with the series
    // replaced between them, an order it no longer has would come back with no book, or with the books
    // another order now has under its name.
    in_snapshot(conn, |conn| series_in_snapshot(conn, name))
}

fn series_in_snapshot(conn: &Connection, name: &str) -> Result<Series> {
    let title = conn
        .query_row("SELECT title FROM haiyaku_series WHERE name = ?1", [name], |row| row.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("series {name}")))?;
    let orders: Vec<String> = conn
        .prepare("SELECT name FROM haiyaku_order WHERE series = ?1 ORDER BY rank")?
        .query_map([name], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let orders = orders.into_iter().map(|order| Ok(Order { books: books_in(conn, name, &order)?, name: order }));
    Ok(Series { name: name.to_owned(), title, orders: orders.collect::<Result<_>>()? })
}

/// A series that holds a book, and those of its reading orders that do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    pub series: String,
    /// By rank: the series' default order first, when it holds the book.
    pub orders: Vec<String>,
}

/// The series that have a reading order holding `book`, by name, each with the names of its orders that hold it,
/// by rank. A stored book that no series holds gives an empty list.
///
/// Which series and which order to read the book in is the caller's choice; nothing here ranks one above another.
///
/// All of it is read from one state of the database, as [`read_series`] reads: a series that another
/// connection replaces during the call comes back as it was or as it has become.
///
/// # Errors
///
/// [`Error::NotFound`] naming `book <sha256>` when the book is not stored; [`Error::Sqlite`] when the database
/// cannot be read.
pub fn series_holding(conn: &Connection, book: &str) -> Result<Vec<Holding>> {
    // One snapshot, because the rows and the check that the book is stored are separate statements: with the
    // book removed between them, a book that is not stored would come back as one in no series.
    in_snapshot(conn, |conn| holding_in_snapshot(conn, book))
}

fn holding_in_snapshot(conn: &Connection, book: &str) -> Result<Vec<Holding>> {
    if !book_is_stored(conn, book)? {
        return Err(Error::NotFound(format!("book {book}")));
    }
    let mut stmt = conn.prepare(
        "SELECT o.series, o.name
         FROM haiyaku_order o
         WHERE EXISTS (SELECT 1 FROM haiyaku_order_book b
                       WHERE b.series = o.series AND b.order_name = o.name AND b.book = ?1)
         ORDER BY o.series, o.rank",
    )?;
    let mut rows = stmt.query([book])?;
    let mut held: Vec<Holding> = Vec::new();
    while let Some(row) = rows.next()? {
        let (series, order): (String, String) = (row.get(0)?, row.get(1)?);
        match held.last_mut() {
            Some(last) if last.series == series => last.orders.push(order),
            _ => held.push(Holding { series, orders: vec![order] }),
        }
    }
    Ok(held)
}

/// Stores `links` as the links of one reading order, replacing those it had, in one transaction: on any error
/// the database is as it was.
///
/// `logs` are the logs the links were made against, one for each book of the order, in its sequence. Nothing is
/// stored unless the database still holds each of them, as [`write_book`](super::write_book) would find it holds a
/// log it is given, and the order still lists their books, in that sequence: a link names characters by ids, and a
/// log stored since may give those ids to other characters; and links made over other books may skip a book whose
/// characters they were never weighed against, or join a character to one of a book that now comes after it.
///
/// # Errors
///
/// [`Error::ForeignKeysOff`] when `PRAGMA foreign_keys` is off on `conn`; [`Error::LogReplaced`] naming the
/// first of `logs` the database holds another log of, or none; [`Error::NotFound`] naming
/// `order <order> of series <series>` when no such order is stored; [`Error::OrderReplaced`] when the order
/// lists other books than `logs` are the logs of, or the same in another sequence; [`Error::Invalid`] when a
/// link names a book none of `logs` is the log of, when two of the links are for the same later character, when a
/// link's later book does not come after its earlier book in the order, which covers a book the order does not
/// hold, and when a link's `at` names a document the later book does not have or one that is not in the link's
/// `chapter`, when the later book has not introduced the later character by the end of that chapter, and when the
/// earlier book never introduces the earlier one, which covers an entity its book does not have; [`Error::Sqlite`]
/// when the schema refuses a row, and when `conn` is already inside a transaction.
pub fn put_links(conn: &mut Connection, series: &str, order: &str, links: &[Link], logs: &[BookLog]) -> Result<()> {
    // The checks of each link refuse a link to a book, a document or an entity that is not there. The foreign
    // keys stand behind them: with the keys on, a case the checks miss is an error and not a dangling link.
    require_foreign_keys(conn)?;
    check_made_against(links, logs)?;
    check_links(series, order, links)?;
    // Immediate, so that the order and the logs the links are checked against are the ones they are stored with:
    // a deferred transaction would let another connection replace the series or store a book again between the
    // check and the insert.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Before the checks of each link, which would otherwise weigh the ids against a log they were not made from, and
    // the books' places against an order the links were not made over.
    require_stored(&tx, logs)?;
    require_made_over(&tx, series, order, logs)?;
    write_links(&tx, series, order, links)?;
    tx.commit()?;
    Ok(())
}

/// Refuses a link that names a book none of `logs` is the log of: nothing would say which log its ids are from.
fn check_made_against(links: &[Link], logs: &[BookLog]) -> Result<()> {
    let given = |key: &&EntityKey| logs.iter().any(|log| log.book.sha256 == key.book);
    match links.iter().flat_map(|link| [&link.later, &link.earlier]).find(|key| !given(key)) {
        Some(key) => Err(Error::Invalid(format!(
            "a link names {} of book {}, and none of the logs it was made against is that book's",
            key.id, key.book
        ))),
        None => Ok(()),
    }
}

/// Refuses unless the database holds each of `logs`.
fn require_stored(conn: &Connection, logs: &[BookLog]) -> Result<()> {
    for log in logs {
        if !is_stored(conn, log)? {
            return Err(Error::LogReplaced { book: log.book.sha256.clone() });
        }
    }
    Ok(())
}

/// Refuses unless the order is stored and lists the books of `logs`, in their sequence.
fn require_made_over(conn: &Connection, series: &str, order: &str, logs: &[BookLog]) -> Result<()> {
    require_order(conn, series, order)?;
    if books_in(conn, series, order)?.iter().eq(logs.iter().map(|log| &log.book.sha256)) {
        Ok(())
    } else {
        Err(Error::OrderReplaced { series: series.to_owned(), order: order.to_owned() })
    }
}

/// The rules of [`put_links`] that need nothing from the database.
pub(super) fn check_links(series: &str, order: &str, links: &[Link]) -> Result<()> {
    // The schema would refuse the second as well, with an error that names its columns and not the character.
    let mut linked = HashSet::new();
    if let Some(Link { later, .. }) = links.iter().find(|link| !linked.insert(&link.later)) {
        return Err(Error::Invalid(format!(
            "order {order} of series {series} is given two links for {} of book {}",
            later.id, later.book
        )));
    }
    Ok(())
}

/// [`put_links`] inside the transaction its caller holds. The caller has checked that the foreign keys are on and
/// that `links` pass [`check_links`]; each link is checked against the database here, in the transaction that
/// stores it.
pub(super) fn write_links(conn: &Connection, series: &str, order: &str, links: &[Link]) -> Result<()> {
    require_order(conn, series, order)?;
    let books = books_in(conn, series, order)?;
    let place = |book: &str| books.iter().position(|listed| listed == book);
    let mut chapter_of = conn.prepare("SELECT chapter FROM haiyaku_document WHERE book = ?1 AND ordinal = ?2")?;
    let mut introduced = conn.prepare(
        "SELECT EXISTS (
            SELECT 1 FROM haiyaku_event e JOIN haiyaku_name n ON n.book = e.book AND n.seq = e.seq
            WHERE e.book = ?1 AND n.entity = ?2 AND e.type = 'introduced' AND (?3 IS NULL OR e.chapter <= ?3))",
    )?;
    for link in links {
        let (later, earlier) = (&link.later, &link.earlier);
        // A book the order does not hold has no place in it, so it comes neither before nor after another.
        if !matches!((place(&later.book), place(&earlier.book)), (Some(later), Some(earlier)) if later > earlier) {
            return Err(Error::Invalid(format!(
                "a link joins {} of book {} to {} of book {}, which does not come before it in order {order} of \
                 series {series}",
                later.id, later.book, earlier.id, earlier.book
            )));
        }
        // A link counts once its chapter is finished, so the chapter has to be the one that holds `at`: with
        // a lower one the two would be shown as one before the reader reaches the passage that says so.
        let held: Option<Option<u32>> =
            chapter_of.query_row(params![later.book, link.at.document], |row| row.get(0)).optional()?;
        if held != Some(Some(link.chapter)) {
            let found = match held {
                None => "the book has no such document".to_owned(),
                Some(None) => "that document is in no chapter".to_owned(),
                Some(Some(chapter)) => format!("that document is in chapter {chapter}"),
            };
            return Err(Error::Invalid(format!(
                "the link of {} of book {} holds from document {} in chapter {}, and {found}",
                later.id, later.book, link.at.document, link.chapter
            )));
        }
        // The fold tries a link once, when its chapter is in, and joins only two characters it has met by
        // then: the later book's up to that chapter, the earlier book's anywhere in it, since that book is
        // read whole. A link that fails this is never applied, and the two stay apart with no error.
        if !introduced.query_row(params![later.book, later.id, link.chapter], |row| row.get(0))? {
            return Err(Error::Invalid(format!(
                "the link of {} of book {} holds from chapter {}, and the book has not introduced {} by the end of \
                 that chapter",
                later.id, later.book, link.chapter, later.id
            )));
        }
        if !introduced.query_row(params![earlier.book, earlier.id, None::<u32>], |row| row.get(0))? {
            return Err(Error::Invalid(format!(
                "a link joins {} of book {} to {} of book {}, which that book never introduces",
                later.id, later.book, earlier.id, earlier.book
            )));
        }
    }
    conn.execute("DELETE FROM haiyaku_link WHERE series = ?1 AND order_name = ?2", [series, order])?;
    insert_links(conn, series, order, links)
}

/// The links of one reading order, in the order a reader comes to them: by the later book's place in the
/// order, then by where in it the link holds.
///
/// The order and its links are read from one state of the database: for a series that another connection
/// replaces during the call, they are the links the order had, or the order is not found. Inside a
/// transaction of the caller's, which stays open, that state is the transaction's. On a connection outside a
/// transaction, the read is refused with [`Error::Sqlite`] while a write statement of the caller's is still in
/// progress on it.
///
/// # Errors
///
/// [`Error::NotFound`] naming `order <order> of series <series>` when no such order is stored;
/// [`Error::Invalid`] when a link was made in a way this build does not know; [`Error::Sqlite`] when the
/// database cannot be read.
pub fn read_links(conn: &Connection, series: &str, order: &str) -> Result<Vec<Link>> {
    // One snapshot, because the order is looked for in one statement and its links are read in another:
    // with the series replaced between them, an order that is gone would come back as one without links.
    in_snapshot(conn, |conn| links_in_snapshot(conn, series, order))
}

fn links_in_snapshot(conn: &Connection, series: &str, order: &str) -> Result<Vec<Link>> {
    require_order(conn, series, order)?;
    let mut stmt = conn.prepare(
        "SELECT l.later_book, l.later_entity, l.earlier_book, l.earlier_entity, l.method, l.quote,
                l.chapter, l.document, l.block
         FROM haiyaku_link l
         LEFT JOIN haiyaku_order_book b
            ON b.series = l.series AND b.order_name = l.order_name AND b.book = l.later_book
         WHERE l.series = ?1 AND l.order_name = ?2
         ORDER BY b.position, l.chapter, l.document, l.block, l.later_entity",
    )?;
    let mut rows = stmt.query([series, order])?;
    let mut links = Vec::new();
    while let Some(row) = rows.next()? {
        let method: String = row.get("method")?;
        let method = match method.as_str() {
            "name" => LinkMethod::Name,
            // The schema's CHECK gives every such link a quote.
            "model" => LinkMethod::Model { quote: row.get("quote")? },
            other => return Err(unknown("link method", other)),
        };
        links.push(Link {
            later: EntityKey { book: row.get("later_book")?, id: row.get("later_entity")? },
            earlier: EntityKey { book: row.get("earlier_book")?, id: row.get("earlier_entity")? },
            method,
            chapter: row.get("chapter")?,
            at: Position { document: row.get("document")?, block: row.get("block")? },
        });
    }
    Ok(links)
}

/// How many of the book's chapters a reader at `document` has finished: those whose last document comes
/// before it. A chapter the reader is still in does not count, however far into it they are.
///
/// [`write_book`](super::write_book) stores no book unless its chapters are numbered 1, 2, 3… in the order of
/// their documents, each with a document, so this is also the number of the last chapter finished, and 0
/// before the end of the first.
///
/// The document and the chapters are read from one state of the database: for a book that another connection
/// rewrites during the call, the count is the one of the log as it was or as it has become. Inside a
/// transaction of the caller's, which stays open, that state is the transaction's. On a connection outside a
/// transaction, the read is refused with [`Error::Sqlite`] while a write statement of the caller's is still in
/// progress on it.
///
/// # Errors
///
/// [`Error::NotFound`] naming `document <path> in book <sha256>` when the book has no document at that path,
/// or `book <sha256>` when the book is not stored at all; [`Error::Sqlite`] when the database cannot be read.
pub fn finished_chapters(conn: &Connection, book: &str, document: &str) -> Result<u32> {
    // One snapshot, because the document's place and the chapters that end before it are separate
    // statements: with the book rewritten between them, the place in the old log would be counted against
    // the chapters of the new one, and where those start earlier that counts a chapter the reader is still in.
    in_snapshot(conn, |conn| finished_in_snapshot(conn, book, document))
}

fn finished_in_snapshot(conn: &Connection, book: &str, document: &str) -> Result<u32> {
    let ordinal: Option<u32> = conn
        .query_row("SELECT ordinal FROM haiyaku_document WHERE book = ?1 AND path = ?2", [book, document], |row| {
            row.get(0)
        })
        .optional()?;
    let Some(ordinal) = ordinal else {
        return Err(Error::NotFound(if book_is_stored(conn, book)? {
            format!("document {document} in book {book}")
        } else {
            format!("book {book}")
        }));
    };
    let finished = conn.query_row(
        "SELECT count(*)
         FROM (SELECT max(ordinal) AS last FROM haiyaku_document
               WHERE book = ?1 AND chapter IS NOT NULL GROUP BY chapter)
         WHERE last < ?2",
        params![book, ordinal],
        |row| row.get(0),
    )?;
    Ok(finished)
}

fn book_is_stored(conn: &Connection, sha256: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM haiyaku_book WHERE sha256 = ?1)", [sha256], |row| row.get(0))?)
}

fn require_order(conn: &Connection, series: &str, order: &str) -> Result<()> {
    let stored: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM haiyaku_order WHERE series = ?1 AND name = ?2)",
        [series, order],
        |row| row.get(0),
    )?;
    if stored { Ok(()) } else { Err(Error::NotFound(format!("order {order} of series {series}"))) }
}

/// The books of the order, in reading order; none for an order that is not stored.
fn books_in(conn: &Connection, series: &str, order: &str) -> Result<Vec<String>> {
    let books = conn
        .prepare("SELECT book FROM haiyaku_order_book WHERE series = ?1 AND order_name = ?2 ORDER BY position")?
        .query_map([series, order], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(books)
}

/// The series' orders, ranked as listed, each with its books.
fn insert_orders(conn: &Connection, series: &Series) -> Result<()> {
    let mut order = conn.prepare("INSERT INTO haiyaku_order (series, name, rank) VALUES (?1, ?2, ?3)")?;
    let mut book =
        conn.prepare("INSERT INTO haiyaku_order_book (series, order_name, position, book) VALUES (?1, ?2, ?3, ?4)")?;
    for (rank, listed) in (0u32..).zip(&series.orders) {
        order.execute(params![series.name, listed.name, rank])?;
        for (position, sha256) in (0u32..).zip(&listed.books) {
            // The schema would refuse the row as well, with an error that does not say which book.
            if !book_is_stored(conn, sha256)? {
                return Err(Error::NotFound(format!("book {sha256}")));
            }
            book.execute(params![series.name, listed.name, position, sha256])?;
        }
    }
    Ok(())
}

fn insert_links(conn: &Connection, series: &str, order: &str, links: &[Link]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO haiyaku_link (series, order_name, later_book, later_entity, earlier_book, earlier_entity,
                                   method, quote, chapter, document, block)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for link in links {
        let (method, quote) = match &link.method {
            LinkMethod::Name => ("name", None),
            LinkMethod::Model { quote } => ("model", Some(quote)),
        };
        stmt.execute(params![
            series,
            order,
            link.later.book,
            link.later.id,
            link.earlier.book,
            link.earlier.id,
            method,
            quote,
            link.chapter,
            link.at.document,
            link.at.block
        ])?;
    }
    Ok(())
}
