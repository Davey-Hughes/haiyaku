//! Writing a book's log to the database and reading it back.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use super::{in_snapshot, require_foreign_keys};
use crate::log::{
    BookLog, BookRecord, Chapter, Document, Entity, EntityKind, Event, EventBody, Name, Position, RelationKind, Sex,
};
use crate::{Error, Result};

/// Stores `log`, replacing what is stored for the book with the same `sha256`, in one transaction: on any
/// error the database is as it was.
///
/// Replacing a book keeps its place in every reading order and removes every link to or from it, in every
/// series: a link names characters by their ids, and the new log's ids need not mean the same characters.
/// Storing the log the book already has replaces nothing, and so keeps its links; the order `log` lists its
/// documents and its entities in makes no difference to that.
///
/// # Errors
///
/// [`Error::ForeignKeysOff`] when `PRAGMA foreign_keys` is off on `conn`. [`Error::Invalid`], with nothing
/// written, when the book's `sha256` is not 64 lowercase hex digits, and when the log breaks a rule that keeps
/// a reader's place and the chapter cut-off in step:
///
/// - the events' `seq` runs 1, 2, 3… and their chapters never go back;
/// - the chapters are numbered 1, 2, 3… in the order they are listed;
/// - taking the documents by `ordinal`, the chapters of those that have one never go back, and each is one
///   of the log's chapters;
/// - every chapter has a document;
/// - every event is in a document the log lists, and that document is in the event's chapter.
///
/// [`Error::Sqlite`] when the schema refuses a row, as it does an event that names an entity the log does not
/// list, and when `conn` is already inside a transaction.
pub fn write_book(conn: &mut Connection, log: &BookLog) -> Result<()> {
    // The foreign keys are what refuses a dangling reference below and what takes an event's child rows
    // with it.
    require_foreign_keys(conn)?;
    check_hash(&log.book.sha256)?;
    check_reading_order(&log.events)?;
    check_layout(log)?;

    let book = &log.book;
    let sha256 = book.sha256.as_str();
    // Immediate, so the write lock is taken before the stored log is read. A deferred transaction would
    // read first, and SQLite refuses at once to let a connection that has read wait for another writer:
    // the write would fail as busy where the caller's busy timeout should have waited. Dropping the
    // transaction on an error rolls it back.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Before the delete below: the links are dropped because a new log's ids may mean other characters,
    // and in the same log they mean the same ones. A caller that stores every sidecar it finds, changed or
    // not, would otherwise empty haiyaku_link.
    if is_stored(&tx, log)? {
        return Ok(());
    }

    // The old rows, each table before the ones it references; a new book has none. Deleting an event takes
    // its child rows by cascade.
    tx.execute("DELETE FROM haiyaku_link WHERE later_book = ?1 OR earlier_book = ?1", [sha256])?;
    for table in ["haiyaku_event", "haiyaku_document", "haiyaku_entity", "haiyaku_chapter"] {
        tx.execute(&format!("DELETE FROM {table} WHERE book = ?1"), [sha256])?;
    }
    // Updated in place, never deleted and inserted again: the rows of haiyaku_order_book that reference the
    // book would refuse the delete, and they are the book's place in its reading orders.
    tx.execute(
        "INSERT INTO haiyaku_book (sha256, title, language, backend, model, prompt_version, extracted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (sha256) DO UPDATE SET
            title = excluded.title, language = excluded.language, backend = excluded.backend,
            model = excluded.model, prompt_version = excluded.prompt_version, extracted_at = excluded.extracted_at",
        params![sha256, book.title, book.language, book.backend, book.model, book.prompt_version, book.extracted_at],
    )?;

    insert_chapters(&tx, sha256, &log.chapters)?;
    insert_documents(&tx, sha256, &log.documents)?;
    insert_entities(&tx, sha256, &log.entities)?;
    insert_events(&tx, sha256, &log.events)?;
    tx.commit()?;
    Ok(())
}

/// The whole log of the book: its chapters by number, its documents by ordinal, its entities by id and its
/// events by `seq`.
///
/// All of it is read from one state of the database: a book that another connection rewrites during the call
/// comes back as it was or as it has become, never as some of each. Inside a transaction of the caller's,
/// which stays open, that state is the transaction's. On a connection outside a transaction, the read is
/// refused with [`Error::Sqlite`] while a write statement of the caller's is still in progress on it.
///
/// # Errors
///
/// [`Error::NotFound`] naming `book <sha256>` when no such book is stored; [`Error::Invalid`] when an event
/// has lost the row that holds its body; [`Error::Sqlite`] when the database cannot be read.
pub fn read_book(conn: &Connection, sha256: &str) -> Result<BookLog> {
    // One snapshot, because the log is read table by table: with the book rewritten between two of those
    // reads, the chapters of one extraction would come back with the events of another, and an export
    // would write that to a sidecar.
    in_snapshot(conn, |conn| read_log(conn, sha256))
}

/// [`read_book`] inside the transaction its caller holds.
fn read_log(conn: &Connection, sha256: &str) -> Result<BookLog> {
    let book = conn
        .query_row(
            "SELECT sha256, title, language, backend, model, prompt_version, extracted_at
             FROM haiyaku_book WHERE sha256 = ?1",
            [sha256],
            |row| {
                Ok(BookRecord {
                    sha256: row.get(0)?,
                    title: row.get(1)?,
                    language: row.get(2)?,
                    backend: row.get(3)?,
                    model: row.get(4)?,
                    prompt_version: row.get(5)?,
                    extracted_at: row.get(6)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("book {sha256}")))?;

    let chapters = conn
        .prepare(
            "SELECT number, toc_entry, title, seconds, cost_usd, kept, rejected
             FROM haiyaku_chapter WHERE book = ?1 ORDER BY number",
        )?
        .query_map([sha256], |row| {
            Ok(Chapter {
                number: row.get(0)?,
                toc_entry: row.get(1)?,
                title: row.get(2)?,
                seconds: row.get(3)?,
                cost_usd: row.get(4)?,
                kept: row.get(5)?,
                rejected: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let documents = conn
        .prepare("SELECT ordinal, path, chapter FROM haiyaku_document WHERE book = ?1 ORDER BY ordinal")?
        .query_map([sha256], |row| Ok(Document { ordinal: row.get(0)?, path: row.get(1)?, chapter: row.get(2)? }))?
        .collect::<rusqlite::Result<_>>()?;

    let mut entities = Vec::new();
    let mut stmt = conn.prepare("SELECT id, kind FROM haiyaku_entity WHERE book = ?1 ORDER BY id")?;
    let mut rows = stmt.query([sha256])?;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(1)?;
        entities.push(Entity { id: row.get(0)?, kind: known(EntityKind::parse(&kind), "entity kind", &kind)? });
    }

    Ok(BookLog { book, chapters, documents, entities, events: read_events(conn, sha256, None)? })
}

/// The book's events in `seq` order: those of chapters up to and including `up_to_chapter`, or all of them
/// for `None`. A book that is not stored has no events, which is not an error here.
///
/// # Errors
///
/// [`Error::Invalid`] when an event has lost the row that holds its body; [`Error::Sqlite`] when the database
/// cannot be read.
pub fn read_events(conn: &Connection, sha256: &str, up_to_chapter: Option<u32>) -> Result<Vec<Event>> {
    // An event has a row in exactly one of the five child tables, so four of the joins find nothing, and
    // the fifth finding nothing either is the broken log `event_from_row` reports.
    let mut stmt = conn.prepare(
        "SELECT e.seq, e.chapter, e.document, e.block, e.type, e.quote,
                n.entity AS name_entity, n.text, n.given, n.surname, n.title, n.title_sex,
                a.entity AS appearance_entity,
                r.from_entity, r.to_entity, r.kind, r.note,
                s.entity AS same_as_entity, s.other,
                o.from_entity AS overturn_from, o.to_entity AS overturn_to, o.kind AS overturn_kind,
                o.note AS overturn_note
         FROM haiyaku_event e
         LEFT JOIN haiyaku_name n ON n.book = e.book AND n.seq = e.seq
         LEFT JOIN haiyaku_appearance a ON a.book = e.book AND a.seq = e.seq
         LEFT JOIN haiyaku_relation r ON r.book = e.book AND r.seq = e.seq
         LEFT JOIN haiyaku_same_as s ON s.book = e.book AND s.seq = e.seq
         LEFT JOIN haiyaku_overturn o ON o.book = e.book AND o.seq = e.seq
         WHERE e.book = ?1 AND (?2 IS NULL OR e.chapter <= ?2)
         ORDER BY e.seq",
    )?;
    let mut rows = stmt.query(params![sha256, up_to_chapter])?;
    let mut events = Vec::new();
    while let Some(row) = rows.next()? {
        events.push(event_from_row(row, sha256)?);
    }
    Ok(events)
}

/// Whether the log stored for the book is `log`, through the code that reads a book back, so that this and
/// [`read_book`] cannot come to disagree on what a stored log is.
fn is_stored(conn: &Connection, log: &BookLog) -> Result<bool> {
    let stored = match read_log(conn, &log.book.sha256) {
        Ok(stored) => stored,
        // No log, or one that cannot be read back, which the write replaces.
        Err(Error::NotFound(_) | Error::Invalid(_)) => return Ok(false),
        Err(e) => return Err(e),
    };
    // A log may list its documents and its entities in any order, and they are read back by ordinal and by
    // id, so each is compared in that order. The chapters and the events are in theirs, or the checks
    // would have refused the log.
    let mut documents: Vec<&Document> = log.documents.iter().collect();
    documents.sort_by_key(|document| document.ordinal);
    let mut entities: Vec<&Entity> = log.entities.iter().collect();
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(stored.book == log.book
        && stored.chapters == log.chapters
        && stored.events == log.events
        && stored.documents.iter().eq(documents)
        && stored.entities.iter().eq(entities))
}

/// The hash is the book's key in every table and is compared as text, so it has one spelling: in another
/// letter case the same file would be stored as a second book, and looked up, not found.
pub(super) fn check_hash(sha256: &str) -> Result<()> {
    if sha256.len() == 64 && sha256.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("the book's sha256 is {sha256:?}, and it must be 64 lowercase hex digits")))
    }
}

/// The fold reads events by `seq` and cuts them off at a chapter, so `seq` order has to be reading order:
/// an event stored out of place would show a reader something from a chapter not yet read.
fn check_reading_order(events: &[Event]) -> Result<()> {
    let mut chapter = 0;
    for (expected, event) in (1u32..).zip(events) {
        if event.seq != expected {
            return Err(Error::Invalid(format!(
                "event {expected} of the log has seq {}, and seq must run 1, 2, 3… in the order of the events",
                event.seq
            )));
        }
        if event.chapter < chapter {
            return Err(Error::Invalid(format!(
                "event {expected} is in chapter {}, after an event in chapter {chapter}",
                event.chapter
            )));
        }
        chapter = event.chapter;
    }
    Ok(())
}

/// A reader's place is a document and what they are shown is cut off at a chapter, so the two have to agree:
/// the count of chapters whose documents the reader is past is taken for the number of the last chapter
/// finished (see [`super::finished_chapters`]), and an event shows once its chapter is finished. A log where
/// numbers, documents and events come apart would show a reader something from a chapter not yet read.
fn check_layout(log: &BookLog) -> Result<()> {
    for (expected, chapter) in (1u32..).zip(&log.chapters) {
        if chapter.number != expected {
            return Err(Error::Invalid(format!(
                "chapter {expected} of the log has number {}, and the numbers must run 1, 2, 3… in reading order",
                chapter.number
            )));
        }
    }
    let last = log.chapters.last().map_or(0, |chapter| chapter.number);

    let by_ordinal: BTreeMap<u32, &Document> =
        log.documents.iter().map(|document| (document.ordinal, document)).collect();
    let mut reached = 0;
    for document in by_ordinal.values() {
        let Some(chapter) = document.chapter else { continue };
        if !(1..=last).contains(&chapter) {
            return Err(Error::Invalid(format!(
                "document {} is in chapter {chapter}, which the log does not list",
                document.path
            )));
        }
        if chapter < reached {
            return Err(Error::Invalid(format!(
                "document {} is in chapter {chapter}, after a document in chapter {reached}",
                document.path
            )));
        }
        reached = chapter;
    }
    let has_document = |number: &u32| log.documents.iter().any(|document| document.chapter == Some(*number));
    if let Some(empty) = (1..=last).find(|number| !has_document(number)) {
        return Err(Error::Invalid(format!("chapter {empty} has no document")));
    }

    for event in &log.events {
        let Some(document) = by_ordinal.get(&event.at.document) else {
            return Err(Error::Invalid(format!(
                "event {} is in document {}, which the log does not list",
                event.seq, event.at.document
            )));
        };
        if document.chapter != Some(event.chapter) {
            let held = document.chapter.map_or_else(|| "no chapter".to_owned(), |chapter| format!("chapter {chapter}"));
            return Err(Error::Invalid(format!(
                "event {} is in chapter {}, and its document {} is in {held}",
                event.seq, event.chapter, document.path
            )));
        }
    }
    Ok(())
}

fn insert_chapters(conn: &Connection, book: &str, chapters: &[Chapter]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO haiyaku_chapter (book, number, toc_entry, title, seconds, cost_usd, kept, rejected)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for c in chapters {
        stmt.execute(params![book, c.number, c.toc_entry, c.title, c.seconds, c.cost_usd, c.kept, c.rejected])?;
    }
    Ok(())
}

fn insert_documents(conn: &Connection, book: &str, documents: &[Document]) -> Result<()> {
    let mut stmt =
        conn.prepare("INSERT INTO haiyaku_document (book, ordinal, path, chapter) VALUES (?1, ?2, ?3, ?4)")?;
    for d in documents {
        stmt.execute(params![book, d.ordinal, d.path, d.chapter])?;
    }
    Ok(())
}

fn insert_entities(conn: &Connection, book: &str, entities: &[Entity]) -> Result<()> {
    let mut stmt = conn.prepare("INSERT INTO haiyaku_entity (book, id, kind) VALUES (?1, ?2, ?3)")?;
    for e in entities {
        stmt.execute(params![book, e.id, e.kind.as_str()])?;
    }
    Ok(())
}

/// Each event's row, then the row of its body in the child table for its type.
fn insert_events(conn: &Connection, book: &str, events: &[Event]) -> Result<()> {
    let mut event = conn.prepare(
        "INSERT INTO haiyaku_event (book, seq, chapter, document, block, type, quote)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut name = conn.prepare(
        "INSERT INTO haiyaku_name (book, seq, entity, text, given, surname, title, title_sex)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut appearance = conn.prepare("INSERT INTO haiyaku_appearance (book, seq, entity) VALUES (?1, ?2, ?3)")?;
    let mut relation = conn.prepare(
        "INSERT INTO haiyaku_relation (book, seq, from_entity, to_entity, kind, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    let mut same_as = conn.prepare("INSERT INTO haiyaku_same_as (book, seq, entity, other) VALUES (?1, ?2, ?3, ?4)")?;
    let mut overturn = conn.prepare(
        "INSERT INTO haiyaku_overturn (book, seq, from_entity, to_entity, kind, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for e in events {
        event.execute(params![book, e.seq, e.chapter, e.at.document, e.at.block, event_type(&e.body), e.quote])?;
        match &e.body {
            EventBody::Introduced { entity, name: n }
            | EventBody::Named { entity, name: n }
            | EventBody::Alias { entity, name: n } => name.execute(params![
                book,
                e.seq,
                entity,
                n.text,
                n.given,
                n.surname,
                n.title,
                n.title_sex.map(Sex::as_str)
            ])?,
            EventBody::Appeared { entity } => appearance.execute(params![book, e.seq, entity])?,
            EventBody::Relation { from, to, kind, note } => {
                relation.execute(params![book, e.seq, from, to, kind.as_str(), note])?
            }
            EventBody::SameAs { entity, other } => same_as.execute(params![book, e.seq, entity, other])?,
            EventBody::Overturned { from, to, kind, note } => {
                overturn.execute(params![book, e.seq, from, to, kind.as_str(), note])?
            }
        };
    }
    Ok(())
}

/// The `type` of the event's row, one of those the `CHECK` of `haiyaku_event` lists.
fn event_type(body: &EventBody) -> &'static str {
    match body {
        EventBody::Introduced { .. } => "introduced",
        EventBody::Named { .. } => "named",
        EventBody::Alias { .. } => "alias",
        EventBody::Appeared { .. } => "appeared",
        EventBody::Relation { .. } => "relation",
        EventBody::SameAs { .. } => "same_as",
        EventBody::Overturned { .. } => "overturned",
    }
}

/// An event from a row of the query in [`read_events`].
fn event_from_row(row: &Row<'_>, book: &str) -> Result<Event> {
    let seq: u32 = row.get("seq")?;
    let event_type: String = row.get("type")?;
    // The first column read from a child table is NOT NULL there, so it is NULL only when the join found no
    // row for the event.
    let body_in = |table: &str, column: &str| -> Result<String> {
        row.get::<_, Option<String>>(column)?
            .ok_or_else(|| Error::Invalid(format!("event {seq} of book {book} has no row in {table}")))
    };
    let body = match event_type.as_str() {
        "introduced" | "named" | "alias" => {
            let entity = body_in("haiyaku_name", "name_entity")?;
            let title_sex: Option<String> = row.get("title_sex")?;
            let name = Name {
                text: row.get("text")?,
                given: row.get("given")?,
                surname: row.get("surname")?,
                title: row.get("title")?,
                title_sex: title_sex.map(|sex| known(Sex::parse(&sex), "sex", &sex)).transpose()?,
            };
            match event_type.as_str() {
                "introduced" => EventBody::Introduced { entity, name },
                "named" => EventBody::Named { entity, name },
                _ => EventBody::Alias { entity, name },
            }
        }
        "appeared" => EventBody::Appeared { entity: body_in("haiyaku_appearance", "appearance_entity")? },
        "relation" => {
            let from = body_in("haiyaku_relation", "from_entity")?;
            let kind: String = row.get("kind")?;
            EventBody::Relation {
                from,
                to: row.get("to_entity")?,
                kind: known(RelationKind::parse(&kind), "relation kind", &kind)?,
                note: row.get("note")?,
            }
        }
        "same_as" => {
            EventBody::SameAs { entity: body_in("haiyaku_same_as", "same_as_entity")?, other: row.get("other")? }
        }
        "overturned" => {
            let from = body_in("haiyaku_overturn", "overturn_from")?;
            let kind: String = row.get("overturn_kind")?;
            EventBody::Overturned {
                from,
                to: row.get("overturn_to")?,
                kind: known(RelationKind::parse(&kind), "relation kind", &kind)?,
                note: row.get("overturn_note")?,
            }
        }
        other => return Err(unknown("event type", other)),
    };
    Ok(Event {
        seq,
        chapter: row.get("chapter")?,
        at: Position { document: row.get("document")?, block: row.get("block")? },
        quote: row.get("quote")?,
        body,
    })
}

/// `parsed`, or the error for a stored name this build has no variant for. The schema's CHECKs list the same
/// names as the enums, so this is a database whose tables were not made by this crate's migrations.
fn known<T>(parsed: Option<T>, what: &str, stored: &str) -> Result<T> {
    parsed.ok_or_else(|| unknown(what, stored))
}

pub(super) fn unknown(what: &str, stored: &str) -> Error {
    Error::Invalid(format!("the database holds the {what} {stored:?}, which this build does not know"))
}
