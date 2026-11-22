//! `cast_at` and `read_book` each read with several statements, and what they return has to be of one state
//! of the database, whatever another connection commits between two of those statements.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use common::{db, keeper};
use haiyaku::fold::fold_book;
use haiyaku::log::BookLog;
use haiyaku::store::{Order, Series, migrate, put_series, read_book, write_book};
use haiyaku::{Error, Scope, cast_at};
use rusqlite::Connection;
use tempfile::TempDir;

/// What became of the write made in the middle of the reader's call: `None` until it has been tried.
type Written = Arc<Mutex<Option<Result<(), String>>>>;

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

/// A connection to a new library file in `dir` that holds `keeper()` in a series of its own, set up so
/// that another connection stores `rewrite` in the middle of the next call that reads `table`.
///
/// Nothing outside a call can run between two of its statements, so the reader is made to stop itself. A
/// temporary view stands in for `table` and also reads a second database file, which another connection
/// holds locked. The first statement to read `table` finds that file busy and calls the reader's busy
/// handler, which stores `rewrite` through a third connection and then unlocks the file. That statement
/// had begun to read the library before it stopped, so the rewrite reaches the statements after it, unless
/// the whole call reads one snapshot. The library is in WAL mode, where a write can commit while a reader
/// is at work.
fn interrupted_reader(dir: &Path, table: &str, rewrite: BookLog) -> (Connection, Written) {
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
            *outcome = Some(write_book(&mut writer, &rewrite).map_err(|e| e.to_string()));
            gate.execute_batch("ROLLBACK").is_ok()
        }))
        .unwrap();
    (reader, written)
}

fn assert_written(written: &Written) {
    assert_eq!(*written.lock().unwrap(), Some(Ok(())), "the rewrite was not stored in the middle of the call");
}

#[test]
fn cast_at_reads_one_state_while_the_book_is_rewritten() {
    let dir = dir();
    // The series is read after the reader's position and before the events.
    let (reader, written) = interrupted_reader(dir.path(), "haiyaku_series", started_later());

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
    let (reader, written) = interrupted_reader(dir.path(), "haiyaku_chapter", started_later());

    let log = read_book(&reader, &hash()).unwrap();
    assert_written(&written);
    assert_eq!(log, keeper());
    assert_eq!(read_book(&reader, &hash()).unwrap(), started_later());
}

#[test]
fn reads_leave_a_callers_transaction_open() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute("UPDATE haiyaku_book SET title = 'The Lamp'", []).unwrap();

    // Each reads what the transaction has written and not yet committed.
    assert_eq!(read_book(&tx, &hash()).unwrap().book.title, "The Lamp");
    assert_eq!(cast_at(&tx, &hash(), "back.xhtml", &Scope::Book).unwrap(), fold_book(&keeper(), 3));
    // A failed read ends neither the transaction nor what it wrote.
    let unknown = "c".repeat(64);
    assert!(matches!(read_book(&tx, &unknown), Err(Error::NotFound(_))));
    assert!(matches!(cast_at(&tx, &unknown, "c1.xhtml", &Scope::Book), Err(Error::NotFound(_))));
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
    let unknown = "c".repeat(64);

    read_book(&conn, &hash()).unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(read_book(&conn, &unknown), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
    cast_at(&conn, &hash(), "back.xhtml", &Scope::Book).unwrap();
    assert!(conn.is_autocommit());
    assert!(matches!(cast_at(&conn, &unknown, "c1.xhtml", &Scope::Book), Err(Error::NotFound(_))));
    assert!(conn.is_autocommit());
}
