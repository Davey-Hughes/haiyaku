//! A sidecar: one book's log in a SQLite file of its own, which carries it from one database to another.
//! And the hash of a file, which is what ties a sidecar to its book.

use std::cmp::Ordering;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags, ffi};
use sha2::{Digest, Sha256};

use super::book::check_hash;
use super::{SCHEMA_VERSION, migrate, read_book, schema_version, write_book};
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
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
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
    let temporary = create_beside(path)?;
    let written = write_sidecar(&temporary, &log).and_then(|()| Ok(fs::rename(&temporary, path)?));
    if written.is_err() {
        // The export's own error is the one the caller needs; a file that will not go away is a leftover
        // that `create_beside` steps over.
        let _ = fs::remove_file(&temporary);
    }
    written
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
    // Without the create flag a missing file is an error rather than a new, empty database on the share.
    let file =
        Connection::open_with_flags(sidecar, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let version = schema_version(&file)?.ok_or_else(|| {
        Error::Invalid(format!("{} is not a haiyaku sidecar: it has no haiyaku_schema table", sidecar.display()))
    })?;
    let source = match version.cmp(&SCHEMA_VERSION) {
        Ordering::Greater => return Err(Error::SchemaTooNew { found: version, supported: SCHEMA_VERSION }),
        Ordering::Less => migrated_copy(&file)?,
        Ordering::Equal => file,
    };
    let log = only_book(&source, expected_sha256)?;
    write_book(conn, &log)
}

/// Creates an empty file beside `path` for the export to write to, and returns its path. Beside it, because
/// a rename replaces `path` in one step only within a file system.
fn create_beside(path: &Path) -> Result<PathBuf> {
    let name =
        path.file_name().ok_or_else(|| Error::Invalid(format!("{} names no file to export to", path.display())))?;
    let mut attempt = 0u32;
    loop {
        let mut temporary = name.to_os_string();
        temporary.push(format!(".{attempt}.tmp"));
        let temporary = path.with_file_name(temporary);
        // Created here and not by SQLite, which opens a file that is already there: a name that another
        // export holds, or that a killed one left behind, must never be written into.
        match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(_) => return Ok(temporary),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Makes the empty `file` a database at [`SCHEMA_VERSION`] that holds `log`.
fn write_sidecar(file: &Path, log: &BookLog) -> Result<()> {
    let mut sidecar = Connection::open(file)?;
    sidecar.pragma_update(None, "foreign_keys", true)?;
    migrate(&sidecar)?;
    // The code that writes a book to any database, so a sidecar cannot hold what an import would refuse.
    write_book(&mut sidecar, log)?;
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
