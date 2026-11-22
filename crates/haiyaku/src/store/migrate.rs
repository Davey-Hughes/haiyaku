//! The schema's version, and the runner that brings a database up to it.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use super::in_transaction;
use crate::{Error, Result};

/// The schema version this build reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Migration `i` takes the schema from version `i` to version `i + 1`. A file in this list never changes
/// once a database has run it: that database is past it and would never see the change.
const MIGRATIONS: &[&str] = &[include_str!("schema/001_initial.sql")];

// `migrate_with` takes the newest version from the length of its list, and callers compare against
// `SCHEMA_VERSION`: a migration added without the bump must not build.
const _: () = assert!(MIGRATIONS.len() == SCHEMA_VERSION as usize);

/// Brings the haiyaku tables in `conn`'s main database up to [`SCHEMA_VERSION`], creating them where there
/// are none; an attached or a temporary database is neither looked at nor changed. Calling it again changes
/// nothing, and a database already at that version is only read: the call takes no write lock on it.
///
/// What a call changes commits together or not at all. On a connection that is inside a transaction of the
/// caller's, as in a step of the caller's own migrations, the changes are made under a savepoint: they commit
/// or roll back with that transaction, and a failure undoes them alone and leaves the caller's writes.
///
/// `PRAGMA foreign_keys` stays as the caller set it, and `PRAGMA user_version`, which is the caller's, is
/// never read or written.
///
/// # Errors
///
/// [`Error::SchemaTooNew`] when a later haiyaku wrote the database, which is left as it was;
/// [`Error::Invalid`] when the version table has lost its row; [`Error::Sqlite`] when a statement fails.
pub fn migrate(conn: &Connection) -> Result<()> {
    migrate_with(conn, MIGRATIONS)
}

/// The version of the haiyaku schema in `conn`'s main database, or `None` where [`migrate`] has never run.
///
/// # Errors
///
/// [`Error::Invalid`] when the version table has lost its row; [`Error::Sqlite`] when the database cannot be
/// read.
pub fn schema_version(conn: &Connection) -> Result<Option<u32>> {
    // `main` by name, here and wherever the version table is read or written: a bare name also finds the
    // table of a temporary or an attached database, a sidecar's for one, and the migrations, which make
    // their tables in the main database, would then be skipped with nothing made.
    if !conn.table_exists(Some("main"), "haiyaku_schema")? {
        return Ok(None);
    }
    let version = conn.query_row("SELECT version FROM main.haiyaku_schema", [], |row| row.get(0)).optional()?;
    // Not version 0: the tables of some version are there, and the first migration would fail on them
    // with an error that says nothing about the row.
    match version {
        Some(version) => Ok(Some(version)),
        None => Err(Error::Invalid("the haiyaku_schema table has no row, so the schema's version is unknown".into())),
    }
}

/// [`migrate`] over a list the caller supplies, so a test can run lists of other lengths, and ones that
/// fail, without touching the real schema.
fn migrate_with(conn: &Connection, migrations: &[&str]) -> Result<()> {
    let newest = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    // Read before locking. An up-to-date database is what nearly every call finds, and the call then takes
    // no write lock, so it does not wait for another connection's write transaction to end. It is a read
    // like any other: outside WAL mode it can still find the database busy while a writer commits.
    if pending(conn, newest)?.is_none() {
        return Ok(());
    }
    // Immediate takes the write lock before `apply_pending` reads the version again. Of two connections
    // migrating one file the second then waits and finds nothing pending; a deferred transaction would let
    // both read the old version and run the same migration.
    in_transaction(conn, TransactionBehavior::Immediate, |conn| apply_pending(conn, migrations, newest))
}

/// The stored version, 0 for none, when migrations after it are pending; `None` when the database is at
/// `newest`.
fn pending(conn: &Connection, newest: u32) -> Result<Option<u32>> {
    let current = schema_version(conn)?.unwrap_or(0);
    if current > newest {
        return Err(Error::SchemaTooNew { found: current, supported: newest });
    }
    Ok((current < newest).then_some(current))
}

/// Runs the pending migrations and stores `newest`, inside the transaction or savepoint its caller holds.
fn apply_pending(conn: &Connection, migrations: &[&str], newest: u32) -> Result<()> {
    // Read again: another connection may have migrated between the first read and the lock.
    let Some(current) = pending(conn, newest)? else {
        return Ok(());
    };
    for sql in migrations.iter().skip(current as usize) {
        conn.execute_batch(sql)?;
    }
    // Delete and insert rather than update: it leaves exactly one row whether the table is new or old.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS main.haiyaku_schema (version INTEGER NOT NULL); DELETE FROM main.haiyaku_schema",
    )?;
    conn.execute("INSERT INTO main.haiyaku_schema (version) VALUES (?1)", [newest])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_migrations_only() {
        let conn = Connection::open_in_memory().unwrap();
        let (one, two) = ("CREATE TABLE one (x)", "CREATE TABLE two (x)");
        migrate_with(&conn, &[one, two]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), Some(2));

        // `one` and `two` fail on a table that exists, so this passes only if neither runs again.
        migrate_with(&conn, &[one, two, "CREATE TABLE three (x)"]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), Some(3));
        assert!(conn.table_exists(None, "three").unwrap());
    }

    #[test]
    fn failed_migration_leaves_nothing_behind() {
        let conn = Connection::open_in_memory().unwrap();
        let failed = migrate_with(&conn, &["CREATE TABLE one (x)", "CREATE TABLE two (x); CREATE TABLE one (x)"]);
        assert!(matches!(failed, Err(Error::Sqlite(_))));
        assert_eq!(schema_version(&conn).unwrap(), None);
        assert!(!conn.table_exists(None, "one").unwrap());
        assert!(!conn.table_exists(None, "two").unwrap());
        // The connection is usable again: the failed run did not leave its transaction open.
        assert!(conn.is_autocommit());
    }

    #[test]
    fn failed_migration_in_a_callers_transaction_keeps_the_callers_writes() {
        let conn = Connection::open_in_memory().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute_batch("CREATE TABLE callers (x); INSERT INTO callers VALUES (1)").unwrap();

        let failed = migrate_with(&tx, &["CREATE TABLE one (x)", "CREATE TABLE two (x); CREATE TABLE one (x)"]);
        // The second migration's own failure, not a refusal to start.
        assert!(matches!(failed, Err(Error::Sqlite(e)) if e.to_string().contains("already exists")));
        // The caller's transaction is still open, without what the failed run wrote.
        assert!(!tx.is_autocommit());
        assert_eq!(schema_version(&tx).unwrap(), None);
        assert!(!tx.table_exists(None, "one").unwrap());
        assert!(!tx.table_exists(None, "two").unwrap());

        tx.commit().unwrap();
        let kept: u32 = conn.query_row("SELECT count(*) FROM callers", [], |row| row.get(0)).unwrap();
        assert_eq!(kept, 1);
    }
}
