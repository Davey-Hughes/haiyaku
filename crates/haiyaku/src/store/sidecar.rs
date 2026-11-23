//! A sidecar: a SQLite file of its own that carries one book's log, or a series with every book of its orders
//! and their links, from one database to another. And the hash of a file, which is what ties a book's sidecar
//! to its book.

use std::cmp::Ordering;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, ffi};
use sha2::{Digest, Sha256};

use super::book::{check_hash, check_log, write_log};
use super::series::{check_links, check_series, write_links, write_series};
use super::{
    Link, SCHEMA_VERSION, Series, in_snapshot, migrate, put_links, put_series, read_book, read_links, read_series,
    require_foreign_keys, schema_version, write_book,
};
use crate::log::BookLog;
use crate::{Error, Result};

/// The SHA-256 of the file's bytes, in lowercase hex: the form a book's `sha256` is stored in.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    // Read in pieces: a book can be larger than is worth holding in memory at once.
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(hex(&hasher.finalize()))
}

/// `bytes` in lowercase hex, two digits a byte: the one spelling of a hash this crate writes.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Writes the book's log to a sidecar at `path`: a new SQLite file at [`SCHEMA_VERSION`] that holds this
/// book's rows and nothing else, neither a series nor a link.
///
/// The file is written under another name in `path`'s directory and renamed over `path` at the end, so a
/// reader finds the old sidecar or the whole new one, never half of one. A sidecar already at `path` is
/// replaced.
///
/// # Errors
///
/// [`Error::NotFound`] naming `book <sha256>` when no such book is stored, and then no file is created.
/// [`Error::Invalid`] when `path` has no file name, or the stored log breaks a rule of [`write_book`], which
/// writes the sidecar. [`Error::Io`] and [`Error::Sqlite`] when the file cannot be written; `path` is then
/// as it was.
pub fn export_book(conn: &Connection, sha256: &str, path: &Path) -> Result<()> {
    // Read before any file exists: an unknown book must leave nothing behind.
    let log = read_book(conn, sha256)?;
    // The code that writes a book to any database, so a sidecar cannot hold what an import would refuse.
    export(path, |sidecar| write_book(sidecar, &log))
}

/// Writes the series to a sidecar at `path`: a new SQLite file at [`SCHEMA_VERSION`] that holds every book its
/// orders hold, the series and each order's links, all read from one state of the database. A link names
/// characters by the ids of the logs it was made against, and those ids need not mean the same characters in
/// another extraction of the book, so the logs travel with the links.
///
/// The file is written as [`export_book`] writes one: under another name in `path`'s directory, then renamed
/// over `path`. A sidecar already at `path` is replaced.
///
/// # Errors
///
/// [`Error::NotFound`] naming `series <name>` when no such series is stored, and then no file is created.
/// [`Error::Invalid`] when `path` has no file name, or what is stored breaks a rule of [`write_book`],
/// [`put_series`] or [`put_links`], which write the sidecar. [`Error::Io`] and [`Error::Sqlite`] when the file
/// cannot be written; `path` is then as it was.
pub fn export_series(conn: &Connection, series: &str, path: &Path) -> Result<()> {
    // Read before any file exists: an unknown series must leave nothing behind.
    let whole = read_whole_series(conn, series)?;
    export(path, |sidecar| {
        // The code that writes to any database, so a sidecar cannot hold what an import would refuse.
        for log in &whole.logs {
            write_book(sidecar, log)?;
        }
        put_series(sidecar, &whole.series)?;
        for (order, links) in whole.series.orders.iter().zip(&whole.links) {
            // An order's links were made over its own books' logs, in its sequence, and put_links refuses others.
            let made_over: Vec<BookLog> = order
                .books
                .iter()
                .filter_map(|book| whole.logs.iter().find(|log| log.book.sha256 == *book))
                .cloned()
                .collect();
            put_links(sidecar, &whole.series.name, &order.name, links, &made_over)?;
        }
        Ok(())
    })
}

/// Stores the book of the sidecar at `sidecar` in `conn`, as [`write_book`] does: in one transaction, in
/// place of what is stored for that book, and under the same rules.
///
/// The sidecar is opened read-only and never written to, so it can sit on a share that is mounted
/// read-only. One written at an older schema version is copied into memory and migrated there.
///
/// # Errors
///
/// Nothing is written to `conn` on any of them.
///
/// - [`Error::Invalid`] when `expected_sha256` is not 64 lowercase hex digits, the one form [`write_book`]
///   stores a hash in; when the file is a database with no haiyaku tables, or its version table has lost
///   its row; and when its log breaks a rule of [`write_book`].
/// - [`Error::HashMismatch`] unless the sidecar holds exactly one book and its `sha256` is
///   `expected_sha256`: the sidecar is for another edition of the book. `found` is that book's hash, or
///   `no book`, or how many books there are.
/// - [`Error::SchemaTooNew`] when a later haiyaku wrote the sidecar.
/// - [`Error::ForeignKeysOff`] when `PRAGMA foreign_keys` is off on `conn`.
/// - [`Error::Sqlite`] when the file is missing or is not a SQLite database, and when `conn` refuses a row.
pub fn import_book(conn: &mut Connection, sidecar: &Path, expected_sha256: &str) -> Result<()> {
    // Every read looks a book up by its hash as given, so a hash taken here in another spelling would
    // import the book and then find it nowhere.
    check_hash(expected_sha256)?;
    let source = open_sidecar(sidecar)?;
    let log = only_book(&source, expected_sha256)?;
    write_book(conn, &log)
}

/// Stores the series of the sidecar at `sidecar` in `conn`, with its books and its links, in one transaction.
/// Returns the series stored.
///
/// Each book is stored as [`write_book`] stores it, in the order the series' orders first list them: a log
/// that differs from the one stored for its book replaces it and drops every stored link to or from that book,
/// in every series, while the log the book already has keeps them. Then the series replaces the stored one of
/// its name, as [`put_series`] does, and each order's links are stored as [`put_links`] stores them: the
/// links that arrive are the ones made against the logs that arrive with them.
///
/// The sidecar is opened read-only and never written to, as by [`import_book`], and one written at an older
/// schema version is copied into memory and migrated there.
///
/// # Errors
///
/// Nothing is written to `conn` on any of them.
///
/// - [`Error::ForeignKeysOff`] when `PRAGMA foreign_keys` is off on `conn`.
/// - [`Error::Invalid`] when the file is a database with no haiyaku tables, or its version table has lost its
///   row; when it does not hold exactly one series, or an order of it lists a book it does not hold; and when
///   a log, the series or a link breaks a rule of [`write_book`], [`put_series`] or [`put_links`].
/// - [`Error::SchemaTooNew`] when a later haiyaku wrote the sidecar.
/// - [`Error::Sqlite`] when the file is missing or is not a SQLite database, when `conn` refuses a row, and
///   when `conn` is already inside a transaction.
pub fn import_series(conn: &mut Connection, sidecar: &Path) -> Result<Series> {
    // The writes below rely on the foreign keys to take the child rows of a replaced log's events, and a replaced
    // series' orders with their books and links. A replaced log's links are not among them: `write_log` deletes
    // those itself.
    require_foreign_keys(conn)?;
    let source = open_sidecar(sidecar)?;
    let whole = in_snapshot(&source, |source| read_whole_series(source, &only_series(source, sidecar)?)).map_err(
        |e| match e {
            // The series was just found, and its orders with it, so what is not found is a book an order lists.
            // The schema refuses that in a file written with the foreign keys on.
            Error::NotFound(what) => {
                Error::Invalid(format!("{} lists {what} in an order and does not hold it", sidecar.display()))
            }
            e => e,
        },
    )?;
    // Before the write lock is taken, so a sidecar that breaks one of these keeps no other writer waiting.
    for log in &whole.logs {
        check_log(log)?;
    }
    check_series(&whole.series)?;
    for (order, links) in whole.series.orders.iter().zip(&whole.links) {
        check_links(&whole.series.name, &order.name, links)?;
    }

    // Immediate, as in `write_book`: the write lock before the stored logs are read. Dropping the transaction on
    // an error rolls back every book, the series and the links with it.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for log in &whole.logs {
        write_log(&tx, log)?;
    }
    write_series(&tx, &whole.series)?;
    for (order, links) in whole.series.orders.iter().zip(&whole.links) {
        write_links(&tx, &whole.series.name, &order.name, links)?;
    }
    tx.commit()?;
    Ok(whole.series)
}

/// A series and what its sidecar carries with it.
struct WholeSeries {
    series: Series,
    /// The log of every book of the series' orders, in the order they first list them.
    logs: Vec<BookLog>,
    /// Each order's links, in the order of `series.orders`.
    links: Vec<Vec<Link>>,
}

/// The series `name` and what its sidecar carries with it, read from one state of the database: links read
/// beside another extraction's log would name its characters by ids that may now be others'.
fn read_whole_series(conn: &Connection, name: &str) -> Result<WholeSeries> {
    in_snapshot(conn, |conn| {
        let series = read_series(conn, name)?;
        let mut books: Vec<&str> = Vec::new();
        for book in series.orders.iter().flat_map(|order| &order.books) {
            if !books.contains(&book.as_str()) {
                books.push(book);
            }
        }
        let logs = books.into_iter().map(|sha256| read_book(conn, sha256)).collect::<Result<_>>()?;
        let links = series.orders.iter().map(|order| read_links(conn, name, &order.name)).collect::<Result<_>>()?;
        Ok(WholeSeries { series, logs, links })
    })
}

/// Writes a sidecar to `path`: `fill` writes its rows to a new file beside `path`, which is then renamed
/// over `path`, so a reader finds the old sidecar or the whole new one, never half of one.
fn export(path: &Path, fill: impl FnOnce(&mut Connection) -> Result<()>) -> Result<()> {
    let temporary = create_beside(path)?;
    let written = write_sidecar(&temporary, fill).and_then(|()| Ok(fs::rename(&temporary, path)?));
    if written.is_err() {
        // The export's own error is the one the caller needs; a file that will not go away is a leftover
        // that `create_beside` steps over.
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// The sidecar's database at [`SCHEMA_VERSION`]: the file itself, opened read-only, or a copy migrated in memory
/// from an older version.
fn open_sidecar(sidecar: &Path) -> Result<Connection> {
    // Without the create flag a missing file is an error rather than a new, empty database on the share.
    let file =
        Connection::open_with_flags(sidecar, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let version = schema_version(&file)?.ok_or_else(|| {
        Error::Invalid(format!("{} is not a haiyaku sidecar: it has no haiyaku_schema table", sidecar.display()))
    })?;
    match version.cmp(&SCHEMA_VERSION) {
        Ordering::Greater => Err(Error::SchemaTooNew { found: version, supported: SCHEMA_VERSION }),
        Ordering::Less => migrated_copy(&file),
        Ordering::Equal => Ok(file),
    }
}

/// Creates an empty file beside `path`, for a write that is then renamed over `path`, and returns its path.
/// Beside it, because a rename replaces `path` in one step only within a file system.
pub(crate) fn create_beside(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| Error::Invalid(format!("{} names no file", path.display())))?;
    let mut attempt = 0u32;
    loop {
        let mut temporary = name.to_os_string();
        temporary.push(format!(".{attempt}.tmp"));
        let temporary = path.with_file_name(temporary);
        // Created here and not by the writer, which may open a file that is already there as SQLite does: a
        // name that another write holds, or that a killed one left behind, must never be written into.
        match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(_) => return Ok(temporary),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Makes the empty `file` a database at [`SCHEMA_VERSION`], with the foreign keys on, that holds what `fill`
/// writes to it.
fn write_sidecar(file: &Path, fill: impl FnOnce(&mut Connection) -> Result<()>) -> Result<()> {
    let mut sidecar = Connection::open(file)?;
    sidecar.pragma_update(None, "foreign_keys", true)?;
    migrate(&sidecar)?;
    fill(&mut sidecar)?;
    // Closed before the rename, and by hand: a drop would swallow the error of a close that failed.
    sidecar.close().map_err(|(_, e)| e)?;
    Ok(())
}

/// The sidecar's database, copied into memory and brought to [`SCHEMA_VERSION`] there: a migration writes,
/// and the file must stay as it is.
fn migrated_copy(file: &Connection) -> Result<Connection> {
    let mut copy = Connection::open_in_memory()?;
    // As on every database a migration runs against: one that moves rows relies on the foreign keys to
    // refuse what it gets wrong.
    copy.pragma_update(None, "foreign_keys", true)?;
    // -1 copies every page in the one step, so a step that is not the last is one that could not read the
    // file: a writer held it locked for longer than the connection waits.
    if Backup::new(file, &mut copy)?.step(-1)? != StepResult::Done {
        return Err(rusqlite::Error::SqliteFailure(ffi::Error::new(ffi::SQLITE_BUSY), None).into());
    }
    migrate(&copy)?;
    Ok(copy)
}

/// The log of the one book in `sidecar`, which has to be the book `expected`.
fn only_book(sidecar: &Connection, expected: &str) -> Result<BookLog> {
    let books: Vec<String> = sidecar
        .prepare("SELECT sha256 FROM haiyaku_book ORDER BY sha256")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let found = match books.as_slice() {
        [only] if only == expected => return read_book(sidecar, expected),
        [only] => only.clone(),
        [] => "no book".to_owned(),
        several => format!("{} books", several.len()),
    };
    Err(Error::HashMismatch { expected: expected.to_owned(), found })
}

/// The name of the one series in `source`, the database of the sidecar at `sidecar`.
fn only_series(source: &Connection, sidecar: &Path) -> Result<String> {
    let names: Vec<String> = source
        .prepare("SELECT name FROM haiyaku_series ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    match names.as_slice() {
        [only] => Ok(only.clone()),
        held => Err(Error::Invalid(format!(
            "{} holds {} series, and a series sidecar holds exactly one",
            sidecar.display(),
            held.len()
        ))),
    }
}
