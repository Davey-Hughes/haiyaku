//! The database schema and its migrations; a book's log, and a series with its reading orders and links,
//! written to it and read back; and the sidecar file that carries a book's log to another database.

mod book;
mod migrate;
mod series;
mod sidecar;

use rusqlite::{Connection, Transaction, TransactionBehavior};

pub use book::{read_book, read_events, write_book};
pub use migrate::{SCHEMA_VERSION, migrate, schema_version};
pub use series::{Link, LinkMethod, Order, Series, finished_chapters, put_links, put_series, read_links, read_series};
pub use sidecar::{export_book, import_book, sha256_file};

use crate::{Error, Result};

/// Runs `work` inside one transaction on `conn`: all it reads is one state of the database, whatever other
/// connections commit while it runs, and what it writes commits together or not at all.
///
/// On a connection in autocommit mode the transaction is this call's own, begun as `behavior` says. On one
/// that is inside a transaction of the caller's, where BEGIN is an error, `work` runs under a savepoint: it
/// reads what that transaction reads, its writes commit or roll back with it, and an error undoes them alone
/// and leaves the transaction open.
pub(crate) fn in_transaction<T>(
    conn: &Connection,
    behavior: TransactionBehavior,
    work: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    if conn.is_autocommit() {
        // Dropping the transaction on an error rolls it back.
        let tx = Transaction::new_unchecked(conn, behavior)?;
        let done = work(&tx)?;
        tx.commit()?;
        return Ok(done);
    }
    conn.execute_batch("SAVEPOINT haiyaku")?;
    let done = work(conn).and_then(|done| {
        conn.execute_batch("RELEASE haiyaku")?;
        Ok(done)
    });
    if done.is_err() {
        // A RELEASE that failed has left the savepoint and the work under it in the caller's transaction,
        // as an error of the work has. ROLLBACK TO undoes the work but keeps the savepoint, hence the
        // RELEASE after it. Where these fail as well, SQLite has already rolled the whole transaction back
        // (as it does when the disk fills) or still refuses what it refused before, and the first error is
        // the one the caller needs.
        let _ = conn.execute_batch("ROLLBACK TO haiyaku; RELEASE haiyaku");
    }
    done
}

/// Runs `read`, which only reads and takes several statements to do it, on one state of the database: each
/// statement on its own would see whatever was committed just before it, and their results need not belong
/// together.
///
/// On a connection outside a transaction, the read is refused with SQLite's busy error while a write
/// statement of the caller's is still in progress on it, an `INSERT ... RETURNING` with rows left to return
/// for one.
pub(crate) fn in_snapshot<T>(conn: &Connection, read: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        // The caller's transaction is one state of the database already, and a read has nothing to undo. A
        // savepoint would only cost the caller: SQLite opens none while a write statement of the caller's is
        // in progress, and rolling back to one after a failed read can end the caller's open cursors.
        return read(conn);
    }
    // A savepoint and not BEGIN, though outside a transaction either starts one that reads a single state
    // and locks no more than its reads do. With a write statement of the caller's in progress, BEGIN works
    // and the COMMIT is then refused, and the way out of that is a rollback that takes the caller's write
    // with it. A savepoint is refused at once, before the read has begun and with nothing lost.
    conn.execute_batch("SAVEPOINT haiyaku")?;
    let done = read(conn);
    // Whether the read failed or not: there is nothing to undo, and the RELEASE is what ends the transaction.
    match conn.execute_batch("RELEASE haiyaku") {
        Ok(()) => done,
        Err(e) => {
            // The transaction is this call's own and must not outlive it. A failed read's error is the one
            // the caller needs.
            let _ = conn.execute_batch("ROLLBACK");
            done.and(Err(e.into()))
        }
    }
}

/// Refuses a connection with `PRAGMA foreign_keys` off. A write checks this first when it leaves a dangling
/// reference for the schema to refuse, or old rows for a cascade to take: with the foreign keys off it would
/// store something broken without an error.
fn require_foreign_keys(conn: &Connection) -> Result<()> {
    if conn.pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))? {
        Ok(())
    } else {
        Err(Error::ForeignKeysOff)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::ffi;

    use super::*;

    /// A database of lamps and their wicks. A wick's lamp is only looked for when the transaction commits.
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE lamp (id INTEGER PRIMARY KEY);
             CREATE TABLE wick (lamp INTEGER NOT NULL REFERENCES lamp (id) DEFERRABLE INITIALLY DEFERRED);",
        )
        .unwrap();
        conn
    }

    fn count(conn: &Connection, table: &str) -> u32 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
    }

    fn extended_code(result: &Result<()>) -> Option<i32> {
        match result {
            Err(Error::Sqlite(e)) => e.sqlite_error().map(|e| e.extended_code),
            _ => None,
        }
    }

    #[test]
    fn a_failed_commit_leaves_no_transaction_open() {
        let conn = db();
        let result = in_transaction(&conn, TransactionBehavior::Immediate, |conn| {
            conn.execute("INSERT INTO wick VALUES (1)", [])?;
            Ok(())
        });
        assert_eq!(extended_code(&result), Some(ffi::SQLITE_CONSTRAINT_FOREIGNKEY), "{result:?}");
        assert!(conn.is_autocommit());
        assert_eq!(count(&conn, "wick"), 0);
    }

    #[test]
    fn a_read_whose_release_fails_leaves_no_transaction_open() {
        let conn = db();
        let mut unfinished = conn.prepare("INSERT INTO lamp VALUES (1), (2) RETURNING id").unwrap();
        let mut rows = None;
        let result = in_snapshot(&conn, |_| {
            // SQLite releases no savepoint under a statement that writes and has rows left to return.
            let mut returned = unfinished.query([])?;
            returned.next()?;
            rows = Some(returned);
            Ok(())
        });
        assert_eq!(extended_code(&result), Some(ffi::SQLITE_BUSY), "{result:?}");
        drop(rows);
        assert!(conn.is_autocommit());
        // The way out of the transaction took the statement's rows with it.
        assert_eq!(count(&conn, "lamp"), 0);
    }

    #[test]
    fn a_failed_release_undoes_the_work_and_leaves_the_callers_transaction_open() {
        let conn = db();
        conn.execute_batch("BEGIN; INSERT INTO lamp VALUES (1)").unwrap();
        let mut unfinished = conn.prepare("INSERT INTO lamp VALUES (2), (3) RETURNING id").unwrap();
        let mut rows = None;
        let result = in_transaction(&conn, TransactionBehavior::Deferred, |conn| {
            conn.execute("INSERT INTO lamp VALUES (4)", [])?;
            // SQLite releases no savepoint under a statement that writes and has rows left to return.
            let mut returned = unfinished.query([])?;
            returned.next()?;
            rows = Some(returned);
            Ok(())
        });
        assert_eq!(extended_code(&result), Some(ffi::SQLITE_BUSY), "{result:?}");
        drop(rows);
        // The savepoint itself outlives the call here: the statement that refused the RELEASE is still
        // unfinished when it is tried again. What the call can do is undo the work.
        assert_eq!(count(&conn, "lamp"), 1);
        assert!(!conn.is_autocommit());
        conn.execute_batch("COMMIT").unwrap();
        assert_eq!(count(&conn, "lamp"), 1);
    }
}
