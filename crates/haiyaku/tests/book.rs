//! Writing a book's log and reading it back, through the public API, on in-memory databases.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{db, keeper, keeper_with_appearance, keepers_links, keepers_return};
use haiyaku::Error;
use haiyaku::log::{BookLog, Event, EventBody, Name, Position, RelationKind};
use haiyaku::store::{Order, Series, migrate, put_links, put_series, read_book, read_events, write_book};
use rusqlite::Connection;

fn count(conn: &Connection, table: &str) -> u32 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

fn seqs(events: &[Event]) -> Vec<u32> {
    events.iter().map(|event| event.seq).collect()
}

/// The name the fixture's event `seq` carries.
fn name_at(log: &BookLog, seq: usize) -> &Name {
    match &log.events[seq - 1].body {
        EventBody::Introduced { name, .. } | EventBody::Named { name, .. } | EventBody::Alias { name, .. } => name,
        other => panic!("event {seq} carries no name: {other:?}"),
    }
}

/// `keeper()` with an eleventh event, in chapter 3.
fn keeper_and(quote: &str, body: EventBody) -> BookLog {
    let mut log = keeper();
    log.events.push(Event { seq: 11, chapter: 3, at: Position { document: 3, block: 9 }, quote: quote.into(), body });
    log
}

/// `keeper()` with a relation to an entity the log never lists.
fn keeper_with_ghost() -> BookLog {
    keeper_and(
        "Ada would not speak of the ghost on the rocks.",
        EventBody::Relation {
            from: "ada".into(),
            to: "ghost".into(),
            kind: RelationKind::Enemy,
            note: "will not speak of it".into(),
        },
    )
}

/// `keeper_with_appearance()` with a twelfth event, in chapter 3: Pip was never Ada's gull, as event 3 had it.
fn keeper_with_overturn() -> BookLog {
    let mut log = keeper_with_appearance();
    log.events.push(Event {
        seq: 12,
        chapter: 3,
        at: Position { document: 3, block: 10 },
        quote: "Pip had never been her gull; he came to any keeper with bread.".into(),
        body: EventBody::Overturned {
            from: "ada".into(),
            to: "pip".into(),
            kind: RelationKind::Owns,
            note: "he comes to anyone with bread".into(),
        },
    });
    log
}

/// `write_book` refuses `log` as invalid, for a reason that says `why`, and stores none of it.
fn refused(log: &BookLog, why: &str) {
    let mut conn = db();
    let result = write_book(&mut conn, log);
    assert!(matches!(&result, Err(Error::Invalid(said)) if said.contains(why)), "{result:?}");
    assert!(matches!(read_book(&conn, &log.book.sha256), Err(Error::NotFound(_))));
    assert_eq!(count(&conn, "haiyaku_chapter") + count(&conn, "haiyaku_document") + count(&conn, "haiyaku_event"), 0);
}

/// `keeper()` with each chapter's number put through `renumber`, in its documents and events as well, so
/// that the numbers are all that is wrong with it.
fn keeper_renumbered(renumber: impl Fn(u32) -> u32) -> BookLog {
    let mut log = keeper();
    log.chapters.iter_mut().for_each(|chapter| chapter.number = renumber(chapter.number));
    log.documents.iter_mut().for_each(|document| document.chapter = document.chapter.map(&renumber));
    log.events.iter_mut().for_each(|event| event.chapter = renumber(event.chapter));
    log
}

fn is_foreign_key_failure(error: &Error) -> bool {
    // The extended code: the plain one is also what a failed CHECK returns.
    matches!(error, Error::Sqlite(e)
        if e.sqlite_error().map(|e| e.extended_code) == Some(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY))
}

#[test]
fn round_trip() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    assert_eq!(read_book(&conn, &keeper().book.sha256).unwrap(), keeper());
}

// The database is the caller's, and so is the encoding it stores text in.
#[test]
fn round_trip_in_a_utf16_database() {
    let mut conn = Connection::open_in_memory().unwrap();
    // Before the first table: a database keeps the encoding it was created with.
    conn.pragma_update(None, "encoding", "UTF-16le").unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&conn).unwrap();
    assert_eq!(conn.pragma_query_value(None, "encoding", |row| row.get::<_, String>(0)).unwrap(), "UTF-16le");

    write_book(&mut conn, &keeper()).unwrap();
    assert_eq!(read_book(&conn, &keeper().book.sha256).unwrap(), keeper());
}

#[test]
fn appeared_round_trips() {
    let mut conn = db();
    let log = keeper_with_appearance();
    write_book(&mut conn, &log).unwrap();
    assert_eq!(read_book(&conn, &log.book.sha256).unwrap(), log);
}

#[test]
fn overturned_round_trips() {
    let mut conn = db();
    let log = keeper_with_overturn();
    write_book(&mut conn, &log).unwrap();
    assert_eq!(read_book(&conn, &log.book.sha256).unwrap(), log);
}

#[test]
fn two_books_keep_apart() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    // Both have an entity "ada" and events from seq 1.
    assert_eq!(read_book(&conn, &keeper().book.sha256).unwrap(), keeper());
    assert_eq!(read_book(&conn, &keepers_return().book.sha256).unwrap(), keepers_return());
}

#[test]
fn read_events_cuts_at_chapter() {
    let mut conn = db();
    let log = keeper();
    write_book(&mut conn, &log).unwrap();
    let sha256 = &log.book.sha256;

    assert_eq!(read_events(&conn, sha256, Some(1)).unwrap(), log.events[..3]);
    assert_eq!(seqs(&read_events(&conn, sha256, Some(2)).unwrap()), [1, 2, 3, 4, 5, 6]);
    assert_eq!(read_events(&conn, sha256, None).unwrap(), log.events);
    assert_eq!(read_events(&conn, sha256, Some(0)).unwrap(), []);
}

#[test]
fn unknown_book_is_not_found() {
    let conn = db();
    let sha256 = keeper().book.sha256;
    assert!(matches!(read_book(&conn, &sha256), Err(Error::NotFound(what)) if what == format!("book {sha256}")));
    assert_eq!(read_events(&conn, &sha256, None).unwrap(), []);
}

#[test]
fn write_requires_foreign_keys() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    migrate(&conn).unwrap();
    assert!(matches!(write_book(&mut conn, &keeper()), Err(Error::ForeignKeysOff)));
    assert!(matches!(read_book(&conn, &keeper().book.sha256), Err(Error::NotFound(_))));
}

#[test]
fn relation_to_unknown_entity_is_refused() {
    let mut conn = db();
    let refused = write_book(&mut conn, &keeper_with_ghost()).unwrap_err();
    assert!(is_foreign_key_failure(&refused), "{refused}");
    assert!(matches!(read_book(&conn, &keeper().book.sha256), Err(Error::NotFound(_))));
    // Not only the book row: the ten events written before the refused one went with it.
    assert_eq!(count(&conn, "haiyaku_event"), 0);
}

#[test]
fn failed_rewrite_keeps_old_log() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    let mut rewrite = keeper_with_ghost();
    rewrite.book.title = "The Lighthouse Keeper, revised".into();
    // Refused at its last event, after the old log was deleted and the new one all but written.
    let refused = write_book(&mut conn, &rewrite).unwrap_err();
    assert!(is_foreign_key_failure(&refused), "{refused}");
    assert_eq!(read_book(&conn, &keeper().book.sha256).unwrap(), keeper());
}

// SQLite asks a connection whether to wait for a lock only while the connection holds none itself: one that
// has read and then finds the write lock taken is refused at once, since two such would wait for each other.
// So a write that reads the stored log first waits for another writer only if it took the write lock before
// that read, and the busy handler being asked is what shows it did.
#[test]
fn write_takes_the_write_lock_before_it_reads_the_stored_log() {
    for journal in ["DELETE", "WAL"] {
        // Under target/, so the file is never in the system's temporary directory.
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let path = dir.path().join("library.db");
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", journal).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&conn).unwrap();
        write_book(&mut conn, &keeper()).unwrap();

        // Another connection is in the middle of a write to the stored book, and commits it when the
        // connection under test is first asked whether to wait.
        let other = Connection::open(&path).unwrap();
        other.execute_batch("BEGIN IMMEDIATE; UPDATE haiyaku_book SET title = 'The Lamp'").unwrap();
        let asked = Arc::new(AtomicU32::new(0));
        let calls = Arc::clone(&asked);
        conn.busy_handler(Some(move |_| {
            calls.fetch_add(1, Ordering::Relaxed) == 0 && other.execute_batch("COMMIT").is_ok()
        }))
        .unwrap();

        let mut rewrite = keeper();
        rewrite.book.title = "The Lighthouse Keeper, revised".into();
        let written = write_book(&mut conn, &rewrite);
        assert!(written.is_ok(), "{journal}: {written:?}");
        assert_eq!(asked.load(Ordering::Relaxed), 1, "{journal}");
        assert_eq!(read_book(&conn, &rewrite.book.sha256).unwrap(), rewrite, "{journal}");
    }
}

#[test]
fn write_inside_a_callers_transaction_is_refused_and_leaves_it_open() {
    let mut conn = db();
    conn.execute_batch("BEGIN; INSERT INTO haiyaku_series (name, title) VALUES ('keepers', 'The Keepers')").unwrap();

    let refused = write_book(&mut conn, &keeper());
    assert!(matches!(&refused, Err(Error::Sqlite(e)) if e.to_string().contains("within a transaction")), "{refused:?}");
    // The transaction is still the caller's to end, with what it wrote and nothing of the book.
    assert!(!conn.is_autocommit());
    conn.execute_batch("COMMIT").unwrap();
    assert_eq!(count(&conn, "haiyaku_series"), 1);
    assert!(matches!(read_book(&conn, &keeper().book.sha256), Err(Error::NotFound(_))));
}

#[test]
fn rewrite_replaces_the_log_drops_its_links_and_keeps_its_place() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    let (first, second) = (keeper().book.sha256, keepers_return().book.sha256);
    let in_both = |name: &str| Order { name: name.into(), books: vec![first.clone(), second.clone()] };
    let series = Series {
        name: "keepers".into(),
        title: "The Keepers".into(),
        orders: vec![in_both("published"), in_both("reissued")],
    };
    put_series(&mut conn, &series).unwrap();
    // One link in each order, so a delete that covered a single order would leave the other's behind.
    let links = keepers_links();

    // The links name the first book as their earlier book and the second as their later book.
    for mut rewrite in [keeper(), keepers_return()] {
        put_links(&mut conn, "keepers", "published", &links[..1]).unwrap();
        put_links(&mut conn, "keepers", "reissued", &links[1..]).unwrap();
        assert_eq!(count(&conn, "haiyaku_link"), 2);
        rewrite.book.title.push_str(", revised");
        rewrite.events.pop();

        write_book(&mut conn, &rewrite).unwrap();
        assert_eq!(read_book(&conn, &rewrite.book.sha256).unwrap(), rewrite);
        assert_eq!(count(&conn, "haiyaku_link"), 0);
        assert_eq!(count(&conn, "haiyaku_order_book"), 4);
    }
}

#[test]
fn hash_that_is_not_64_lowercase_hex_digits_is_invalid() {
    let hashes = [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        format!("{}g", "a".repeat(63)),
        // 64 bytes, and 64 characters, that are not hex digits.
        "é".repeat(32),
        "é".repeat(64),
        String::new(),
    ];
    for sha256 in hashes {
        let mut log = keeper();
        log.book.sha256 = sha256;
        refused(&log, "64 lowercase hex digits");
    }
}

#[test]
fn seq_out_of_order_is_invalid() {
    let mut conn = db();
    for seqs in [[2, 1], [1, 3], [0, 1], [1, 1]] {
        let mut log = keeper();
        log.events.truncate(2);
        log.events[0].seq = seqs[0];
        log.events[1].seq = seqs[1];
        assert!(matches!(write_book(&mut conn, &log), Err(Error::Invalid(why)) if why.contains("seq")), "{seqs:?}");
        assert!(matches!(read_book(&conn, &log.book.sha256), Err(Error::NotFound(_))));
    }
}

#[test]
fn chapters_going_back_is_invalid() {
    let mut conn = db();
    let mut log = keeper();
    // The last event of chapter 2, moved back into chapter 1 and its document.
    log.events[5].chapter = 1;
    log.events[5].at.document = 1;
    assert!(matches!(write_book(&mut conn, &log), Err(Error::Invalid(why)) if why.contains("after an event")));
    assert!(matches!(read_book(&conn, &log.book.sha256), Err(Error::NotFound(_))));
}

#[test]
fn chapters_numbered_from_zero_are_invalid() {
    refused(&keeper_renumbered(|number| number - 1), "chapter 1 of the log has number 0");
}

#[test]
fn gap_in_the_chapter_numbers_is_invalid() {
    refused(&keeper_renumbered(|number| if number == 3 { 4 } else { number }), "chapter 3 of the log has number 4");
}

#[test]
fn documents_out_of_chapter_order_are_invalid() {
    // Chapters 1 and 2 change documents, and their events go with them.
    let mut log = keeper();
    log.documents[1].chapter = Some(2);
    log.documents[2].chapter = Some(1);
    for event in &mut log.events[..6] {
        event.at.document = 3 - event.chapter;
    }
    refused(&log, "document c2.xhtml is in chapter 1, after a document in chapter 2");
}

#[test]
fn document_in_an_unlisted_chapter_is_invalid() {
    let mut log = keeper();
    log.documents[4].chapter = Some(4);
    refused(&log, "document back.xhtml is in chapter 4, which the log does not list");
}

#[test]
fn chapter_without_a_document_is_invalid() {
    // Chapter 2 loses its document, and the log the events that were in it and after it.
    let mut log = keeper();
    log.documents[2].chapter = None;
    log.events.truncate(3);
    refused(&log, "chapter 2 has no document");
}

#[test]
fn event_in_another_chapter_than_its_document_is_invalid() {
    // The last event stays in chapter 3 and moves to the document of chapter 2.
    let mut log = keeper();
    log.events[9].at.document = 2;
    refused(&log, "event 10 is in chapter 3, and its document c2.xhtml is in chapter 2");
}

#[test]
fn event_in_a_document_without_a_chapter_is_invalid() {
    let mut log = keeper();
    log.events[9].at.document = 4;
    refused(&log, "event 10 is in chapter 3, and its document back.xhtml is in no chapter");
    log.events[9].at.document = 9;
    refused(&log, "event 10 is in document 9, which the log does not list");
}

#[test]
fn event_without_its_child_row_is_invalid() {
    let log = keeper_with_overturn();
    for (table, seq) in [
        ("haiyaku_name", 1),
        ("haiyaku_relation", 3),
        ("haiyaku_same_as", 8),
        ("haiyaku_appearance", 11),
        ("haiyaku_overturn", 12),
    ] {
        let mut conn = db();
        write_book(&mut conn, &log).unwrap();
        conn.execute(&format!("DELETE FROM {table} WHERE seq = ?1"), [seq]).unwrap();
        let read = read_book(&conn, &log.book.sha256);
        assert!(matches!(&read, Err(Error::Invalid(why)) if why.contains(table)), "{table}: {read:?}");
    }
}

#[test]
fn name_rank() {
    let log = keeper();
    assert_eq!(name_at(&log, 1).rank(), 0, "a description");
    assert_eq!(name_at(&log, 2).rank(), 1, "a given name");
    assert_eq!(name_at(&log, 4).rank(), 2, "a given name and a surname");
    assert_eq!(name_at(&log, 10).rank(), 1, "a surname, and a title that does not count");
}

#[test]
fn relation_kinds_are_the_schemas() {
    // The names in the CHECKs of haiyaku_relation and haiyaku_overturn, in their order.
    let kinds = [
        (RelationKind::Family, "family"),
        (RelationKind::Romance, "romance"),
        (RelationKind::Friend, "friend"),
        (RelationKind::Ally, "ally"),
        (RelationKind::Rival, "rival"),
        (RelationKind::Enemy, "enemy"),
        (RelationKind::Mentor, "mentor"),
        (RelationKind::Serves, "serves"),
        (RelationKind::Colleague, "colleague"),
        (RelationKind::Acquaintance, "acquaintance"),
        (RelationKind::Owns, "owns"),
    ];
    // Ada and Pip introduced, then one relation between them of every kind, each overturned after it.
    let mut log = keeper();
    log.events.truncate(2);
    let event = |seq, body| Event {
        seq,
        chapter: 1,
        at: Position { document: 1, block: seq },
        quote: "The keeper and Pip shared the rail.".into(),
        body,
    };
    for (seq, (kind, name)) in (3..).step_by(2).zip(kinds) {
        assert_eq!(kind.as_str(), name);
        assert_eq!(RelationKind::parse(name), Some(kind));
        let (from, to, note) = ("ada".to_owned(), "pip".to_owned(), name.to_owned());
        log.events
            .push(event(seq, EventBody::Relation { from: from.clone(), to: to.clone(), kind, note: note.clone() }));
        log.events.push(event(seq + 1, EventBody::Overturned { from, to, kind, note }));
    }
    assert_eq!(RelationKind::parse("nemesis"), None);
    let directed: Vec<_> = kinds.iter().filter(|(kind, _)| kind.directed()).map(|(_, name)| *name).collect();
    assert_eq!(directed, ["mentor", "serves", "owns"]);

    // The tables' CHECKs take each name, and each reads back as the kind it was written from.
    let mut conn = db();
    write_book(&mut conn, &log).unwrap();
    assert_eq!(read_book(&conn, &log.book.sha256).unwrap(), log);
}
