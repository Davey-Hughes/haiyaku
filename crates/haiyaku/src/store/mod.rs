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
    match work(conn) {
        Ok(done) => {
            conn.execute_batch("RELEASE haiyaku")?;
            Ok(done)
        }
        Err(e) => {
            // ROLLBACK TO undoes the work but keeps the savepoint, hence the RELEASE. If these fail, SQLite
            // has already rolled the whole transaction back (as it does when the disk fills), and the
            // work's error is still the one the caller needs.
            let _ = conn.execute_batch("ROLLBACK TO haiyaku; RELEASE haiyaku");
            Err(e)
        }
    }
}

/// [`in_transaction`] for work that only reads and takes several statements to do it: each statement on its
/// own would see whatever was committed just before it, and their results need not belong together. Deferred,
/// so the call locks no more than its reads do.
pub(crate) fn in_snapshot<T>(conn: &Connection, read: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    in_transaction(conn, TransactionBehavior::Deferred, read)
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
