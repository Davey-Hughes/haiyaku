//! `cast_at`, `read_book`, `read_series`, `read_links` and `finished_chapters` each read with several
//! statements, and what they return has to be of one state of the database, whatever another connection
//! commits between two of those statements.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use common::{db, keeper, keepers, keepers_links, keepers_return};
use haiyaku::fold::fold_book;
use haiyaku::log::BookLog;
use haiyaku::store::{
    Order, Series, finished_chapters, migrate, put_links, put_series, read_book, read_links, read_series, write_book,
};
use haiyaku::{Error, Result, Scope, cast_at};
use rusqlite::{Connection, ErrorCode};
use tempfile::TempDir;

/// What became of the write made in the middle of the reader's call: `None` until it has been tried.
type Written = Arc<Mutex<Option<std::result::Result<(), String>>>>;

/// A directory for one test's files. Under target/, so they are never in the system's temporary directory.
fn dir() -> TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

fn hash() -> String {
    keeper().book.sha256
}

/// `keeper()` alone in a series, so that a cast in series scope reads the series between the reader's
/// position and the events.
fn alone() -> Series {
    Series {
        name: "keepers".into(),
        title: "The Keepers".into(),
        orders: vec![Order { name: "published".into(), books: vec![hash()] }],
    }
}

fn in_series() -> Scope {
    Scope::Series { series: "keepers".into(), order: None }
}

/// A second extraction of `keeper()` that starts one document later: its chapter 1 is c2.xhtml, with what
/// the first extraction had in chapter 2. A reader in c2.xhtml has finished chapter 1 of the first and no
/// chapter of this one.
fn started_later() -> BookLog {
    let mut log = keeper();
    log.book.extracted_at = "2026-02-01T00:00:00Z".into();
    log.chapters.pop();
    for (document, chapter) in log.documents.iter_mut().zip([None, None, Some(1), Some(2), None]) {
        document.chapter = chapter;
    }
    log.events.retain(|event| event.chapter > 1);
    for (seq, event) in (1..).zip(&mut log.events) {
        event.seq = seq;
        event.chapter -= 1;
    }
    log
}

/// `keeper()` extracted again without its front matter, so that every document comes one place earlier. A
/// reader in c2.xhtml has finished chapter 1 of this extraction, as of the first.
fn without_front_matter() -> BookLog {
    let mut log = keeper();
    log.book.extracted_at = "2026-02-01T00:00:00Z".into();
    log.documents.remove(0);
    for document in &mut log.documents {
        document.ordinal -= 1;
    }
    for event in &mut log.events {
        event.at.document -= 1;
    }
    log
}

/// `alone()` with its one order under another name.
fn reordered() -> Series {
    Series { orders: vec![Order { name: "chronological".into(), books: vec![hash()] }], ..alone() }
}

/// A connection to a new library file in `dir` that holds `keeper()` in a series of its own, set up so
/// that another connection makes the write `rewrite` in the middle of the next call that reads `table`.
///
/// Nothing outside a call can run between two of its statements, so the reader is made to stop itself. A
/// temporary view stands in for `table` and also reads a second database file, which another connection
/// holds locked. The first statement to read `table` finds that file busy and calls the reader's busy
/// handler, which runs `rewrite` on a third connection and then unlocks the file. That statement
/// had begun to read the library before it stopped, so the rewrite reaches the statements after it, unless
/// the whole call reads one snapshot. The library is in WAL mode, where a write can commit while a reader
/// is at work.
fn interrupted_reader(
    dir: &Path,
    table: &str,
    mut rewrite: impl FnMut(&mut Connection) -> Result<()> + Send + 'static,
) -> (Connection, Written) {
    let library = dir.join("library.db");
    let mut writer = Connection::open(&library).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&writer).unwrap();
    write_book(&mut writer, &keeper()).unwrap();
    put_series(&mut writer, &alone()).unwrap();

    let gate_file = dir.join("gate.db");
    let gate = Connection::open(&gate_file).unwrap();
    gate.execute_batch("CREATE TABLE held (x)").unwrap();

    let mut reader = Connection::open(&library).unwrap();
    reader.execute("ATTACH DATABASE ?1 AS gate", [gate_file.to_str().unwrap()]).unwrap();
    reader
        .execute_batch(&format!(
            "CREATE TEMP VIEW {table} AS
             SELECT * FROM main.{table} WHERE (SELECT count(*) FROM gate.held) >= 0"
        ))
        .unwrap();
    // After the reader has attached the file, which reads it.
    gate.execute_batch("BEGIN EXCLUSIVE").unwrap();

    let written = Written::default();
    let outcome = Arc::clone(&written);
    reader
        .busy_handler(Some(move |_| {
            let mut outcome = outcome.lock().unwrap();
            // Once only: a second call means the file is still locked, and giving up fails the read
            // rather than hanging the test.
            if outcome.is_some() {
                return false;
            }
            *outcome = Some(rewrite(&mut writer).map_err(|e| e.to_string()));
            gate.execute_batch("ROLLBACK").is_ok()
        }))
        .unwrap();
    (reader, written)
}

fn assert_written(written: &Written) {
    assert_eq!(*written.lock().unwrap(), Some(Ok(())), "the write was not made in the middle of the call");
}

#[test]
fn cast_at_reads_one_state_while_the_book_is_rewritten() {
    let dir = dir();
    // The series is read after the reader's position and before the events.
    let (reader, written) =
        interrupted_reader(dir.path(), "haiyaku_series", |writer| write_book(writer, &started_later()));

    let cast = cast_at(&reader, &hash(), "c2.xhtml", &in_series()).unwrap();
    assert_written(&written);
    // Not the first extraction's cut-off on the second's events, which would show the chapter the reader
    // is still in.
    assert_eq!(cast, fold_book(&keeper(), 1));
    // The next call reads the rewrite, of which the reader has finished nothing.
    assert_eq!(cast_at(&reader, &hash(), "c2.xhtml", &in_series()).unwrap(), fold_book(&started_later(), 0));
}

#[test]
fn read_book_reads_one_state_while_the_book_is_rewritten() {
    let dir = dir();
    // The chapters are read after the book's record and before its documents, entities and events.
    let (reader, written) =
        interrupted_reader(dir.path(), "haiyaku_chapter", |writer| write_book(writer, &started_later()));

    let log = read_book(&reader, &hash()).unwrap();
    assert_written(&written);
    assert_eq!(log, keeper());
    assert_eq!(read_book(&reader, &hash()).unwrap(), started_later());
}

#[test]
fn read_series_reads_one_state_while_the_series_is_replaced() {
    let dir = dir();
    // The orders are read after the series' title and before the books of each.
    let (reader, written) = interrupted_reader(dir.path(), "haiyaku_order", |writer| put_series(writer, &reordered()));

    let series = read_series(&reader, "keepers").unwrap();
    assert_written(&written);
    // Not the first series' order with the books the second has under that name, which are none.
    assert_eq!(series, alone());
    assert_eq!(read_series(&reader, "keepers").unwrap(), reordered());
}

#[test]
fn read_links_reads_one_state_while_the_series_is_replaced() {
    let dir = dir();
    // The order is looked for first, and its links are read after.
    let (reader, written) = interrupted_reader(dir.path(), "haiyaku_order", |writer| put_series(writer, &reordered()));
    // Links take two books, so the library gets the second and the series of both before the read.
    let mut library = Connection::open(dir.path().join("library.db")).unwrap();
    library.pragma_update(None, "foreign_keys", "ON").unwrap();
    write_book(&mut library, &keepers_return()).unwrap();
    put_series(&mut library, &keepers()).unwrap();
    put_links(&mut library, "keepers", "published", &keepers_links()).unwrap();

    let links = read_links(&reader, "keepers", "published").unwrap();
    assert_written(&written);
    // Not the first series' order found and then no link read for it: the second series has no such order,
    // and that is an error and not an order without links.
    assert_eq!(links, keepers_links());
    let gone = read_links(&reader, "keepers", "published");
    assert!(matches!(&gone, Err(Error::NotFound(what)) if what == "order published of series keepers"), "{gone:?}");
}

#[test]
fn finished_chapters_reads_one_state_while_the_book_is_rewritten() {
    let dir = dir();
    // The document's place is read first, and the chapters that end before it are counted after.
    let (reader, written) =
        interrupted_reader(dir.path(), "haiyaku_document", |writer| write_book(writer, &without_front_matter()));

    let finished = finished_chapters(&reader, &hash(), "c2.xhtml").unwrap();
    assert_written(&written);
    // Not the document's place in the first extraction counted against the chapters of the second, which
    // makes 2: the chapter the reader is still in, finished in neither.
    assert_eq!(finished, 1);
    assert_eq!(finished_chapters(&reader, &hash(), "c2.xhtml").unwrap(), 1);
}

/// One of the calls that read with several statements, with its name for a failed assertion.
type Reader = (&'static str, fn(&Connection) -> Result<()>);

/// Each of them on what `keeper()` and `alone()` have stored.
fn readers() -> [Reader; 5] {
    [
        ("read_book", |conn| read_book(conn, &hash()).map(drop)),
        ("cast_at", |conn| cast_at(conn, &hash(), "back.xhtml", &in_series()).map(drop)),
        ("read_series", |conn| read_series(conn, "keepers").map(drop)),
        ("read_links", |conn| read_links(conn, "keepers", "published").map(drop)),
        ("finished_chapters", |conn| finished_chapters(conn, &hash(), "back.xhtml").map(drop)),
    ]
}

/// Each of them on something that is not stored.
fn readers_of_nothing() -> [Reader; 5] {
    [
        ("read_book", |conn| read_book(conn, &"c".repeat(64)).map(drop)),
        ("cast_at", |conn| cast_at(conn, &"c".repeat(64), "c1.xhtml", &Scope::Book).map(drop)),
        ("read_series", |conn| read_series(conn, "lamps").map(drop)),
        ("read_links", |conn| read_links(conn, "keepers", "reissued").map(drop)),
        ("finished_chapters", |conn| finished_chapters(conn, &"c".repeat(64), "c1.xhtml").map(drop)),
    ]
}

/// `keeper()` in a series of its own, and an empty table of the caller's beside them.
fn library_with_lamps() -> Connection {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    put_series(&mut conn, &alone()).unwrap();
    conn.execute_batch("CREATE TABLE lamp (id INTEGER PRIMARY KEY)").unwrap();
    conn
}

fn lamps(conn: &Connection) -> u32 {
    conn.query_row("SELECT count(*) FROM lamp", [], |row| row.get(0)).unwrap()
}

#[test]
fn a_read_outside_a_transaction_is_refused_while_a_write_of_the_callers_is_in_progress() {
    let conn = library_with_lamps();
    for (name, read) in readers() {
        conn.execute("DELETE FROM lamp", []).unwrap();
        let mut insert = conn.prepare("INSERT INTO lamp VALUES (1), (2) RETURNING id").unwrap();
        let mut inserted = insert.query([]).unwrap();
        assert_eq!(inserted.next().unwrap().map(|row| row.get_unwrap::<_, u32>(0)), Some(1));

        let refused = read(&conn);
        assert!(
            matches!(&refused, Err(Error::Sqlite(e)) if e.sqlite_error_code() == Some(ErrorCode::DatabaseBusy)),
            "{name}: {refused:?}"
        );
        // The statement is the caller's to finish, and what it wrote is still there.
        assert_eq!(inserted.next().unwrap().map(|row| row.get_unwrap::<_, u32>(0)), Some(2), "{name}");
        assert!(inserted.next().unwrap().is_none(), "{name}");
        drop(inserted);
        assert!(conn.is_autocommit(), "{name}");
        assert_eq!(lamps(&conn), 2, "{name}");
    }
}

#[test]
fn a_read_in_a_callers_transaction_runs_while_a_write_of_the_callers_is_in_progress() {
    let conn = library_with_lamps();
    let tx = conn.unchecked_transaction().unwrap();
    let mut insert = tx.prepare("INSERT INTO lamp VALUES (1), (2) RETURNING id").unwrap();
    let mut inserted = insert.query([]).unwrap();
    assert_eq!(inserted.next().unwrap().map(|row| row.get_unwrap::<_, u32>(0)), Some(1));

    assert_eq!(read_series(&tx, "keepers").unwrap(), alone());
    for (name, read) in readers() {
        read(&tx).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
    assert_eq!(inserted.next().unwrap().map(|row| row.get_unwrap::<_, u32>(0)), Some(2));
    assert!(inserted.next().unwrap().is_none());
    drop(inserted);
    drop(insert);
    tx.commit().unwrap();
    assert_eq!(lamps(&conn), 2);
}

#[test]
fn a_failed_read_in_a_callers_transaction_leaves_the_callers_cursors_open() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    put_series(&mut conn, &alone()).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    // A table made in the transaction: rolling back to a savepoint after one ends every cursor that is open.
    tx.execute_batch(
        "CREATE TABLE lamp (id INTEGER PRIMARY KEY); INSERT INTO lamp VALUES (1), (2), (3), (4), (5), (6)",
    )
    .unwrap();
    let mut select = tx.prepare("SELECT id FROM lamp ORDER BY id").unwrap();
    let mut selected = select.query([]).unwrap();
    assert_eq!(selected.next().unwrap().map(|row| row.get_unwrap::<_, u32>(0)), Some(1));

    for (lamp, (name, read)) in (2..).zip(readers_of_nothing()) {
        let missing = read(&tx);
        assert!(matches!(missing, Err(Error::NotFound(_))), "{name}: {missing:?}");
        let next = selected.next().unwrap_or_else(|e| panic!("after {name}: {e:?}"));
        assert_eq!(next.map(|row| row.get_unwrap::<_, u32>(0)), Some(lamp), "{name}");
    }
    assert!(!tx.is_autocommit());
}

#[test]
fn reads_leave_a_callers_transaction_open() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    put_series(&mut conn, &alone()).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute("UPDATE haiyaku_book SET title = 'The Lamp'", []).unwrap();

    // Each reads what the transaction has written and not yet committed.
    assert_eq!(read_book(&tx, &hash()).unwrap().book.title, "The Lamp");
    assert_eq!(cast_at(&tx, &hash(), "back.xhtml", &Scope::Book).unwrap(), fold_book(&keeper(), 3));
    tx.execute("UPDATE haiyaku_series SET title = 'The Lamps'", []).unwrap();
    assert_eq!(read_series(&tx, "keepers").unwrap().title, "The Lamps");
    assert_eq!(read_links(&tx, "keepers", "published").unwrap(), []);
    tx.execute("UPDATE haiyaku_document SET path = 'end.xhtml' WHERE path = 'back.xhtml'", []).unwrap();
    assert_eq!(finished_chapters(&tx, &hash(), "end.xhtml").unwrap(), 3);
    // A read inside a read, as the cast's of the series is.
    assert_eq!(cast_at(&tx, &hash(), "end.xhtml", &in_series()).unwrap(), fold_book(&keeper(), 3));
    // A failed read ends neither the transaction nor what it wrote.
    let unknown = "c".repeat(64);
    assert!(matches!(read_book(&tx, &unknown), Err(Error::NotFound(_))));
    assert!(matches!(cast_at(&tx, &unknown, "c1.xhtml", &Scope::Book), Err(Error::NotFound(_))));
    assert!(matches!(read_series(&tx, "lamps"), Err(Error::NotFound(_))));
    assert!(matches!(read_links(&tx, "keepers", "reissued"), Err(Error::NotFound(_))));
    assert!(matches!(finished_chapters(&tx, &unknown, "c1.xhtml"), Err(Error::NotFound(_))));
    assert!(!tx.is_autocommit());
    assert_eq!(read_book(&tx, &hash()).unwrap().book.title, "The Lamp");

    // It is still the caller's to end.
    tx.rollback().unwrap();
    assert_eq!(read_book(&conn, &hash()).unwrap(), keeper());
}

#[test]
fn reads_leave_no_transaction_of_their_own_open() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    put_series(&mut conn, &alone()).unwrap();
    let unknown = "c".repeat(64);

    read_book(&conn, &hash()).unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(read_book(&conn, &unknown), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
    cast_at(&conn, &hash(), "back.xhtml", &Scope::Book).unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(cast_at(&conn, &unknown, "c1.xhtml", &Scope::Book), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
    read_series(&conn, "keepers").unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(read_series(&conn, "lamps"), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
    read_links(&conn, "keepers", "published").unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(read_links(&conn, "keepers", "reissued"), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
    finished_chapters(&conn, &hash(), "back.xhtml").unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(finished_chapters(&conn, &unknown, "c1.xhtml"), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
}
