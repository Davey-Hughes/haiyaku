//! The schema and its migration runner through the public API, on in-memory databases.

use haiyaku::Error;
use haiyaku::store::{SCHEMA_VERSION, migrate, schema_version};
use rusqlite::Connection;

/// In name order, as `tables` returns them.
const TABLES: [&str; 14] = [
    "haiyaku_appearance",
    "haiyaku_book",
    "haiyaku_chapter",
    "haiyaku_document",
    "haiyaku_entity",
    "haiyaku_event",
    "haiyaku_link",
    "haiyaku_name",
    "haiyaku_order",
    "haiyaku_order_book",
    "haiyaku_relation",
    "haiyaku_same_as",
    "haiyaku_schema",
    "haiyaku_series",
];

const KEEPER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RETURN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// The tables of the connection's main database.
fn tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn.prepare("SELECT name FROM main.sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
    stmt.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
}

fn count(conn: &Connection, table: &str) -> u32 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

/// A migrated database with foreign keys on and at least one row in every table: two invented books that
/// share a character, in one series with one reading order and a link between them.
fn filled() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&conn).unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO haiyaku_book VALUES
            ('{KEEPER}', 'The Keeper', 'en', 'recorded', 'none', 1, '2026-01-01T00:00:00Z'),
            ('{RETURN}', 'The Keeper''s Return', 'en', 'recorded', 'none', 1, '2026-01-02T00:00:00Z');
         INSERT INTO haiyaku_chapter VALUES
            ('{KEEPER}', 1, 0, 'The Lamp', 1.5, NULL, 4, 0),
            ('{RETURN}', 1, 0, 'The Boat', 2.0, 0.01, 1, 1);
         INSERT INTO haiyaku_document VALUES
            ('{KEEPER}', 0, 'cover.xhtml', NULL),
            ('{KEEPER}', 1, 'ch1.xhtml', 1),
            ('{RETURN}', 0, 'ch1.xhtml', 1);
         INSERT INTO haiyaku_entity VALUES
            ('{KEEPER}', 'ada', 'character'),
            ('{KEEPER}', 'tom', 'character'),
            ('{RETURN}', 'ada', 'character');
         INSERT INTO haiyaku_event VALUES
            ('{KEEPER}', 1, 1, 1, 0, 'introduced', 'Ada lit the lamp.'),
            ('{KEEPER}', 2, 1, 1, 0, 'appeared', 'Ada lit the lamp.'),
            ('{KEEPER}', 3, 1, 1, 2, 'relation', 'Tom had rowed with her since spring.'),
            ('{KEEPER}', 4, 1, 1, 5, 'same_as', 'The rower was Tom.');
         INSERT INTO haiyaku_name VALUES ('{KEEPER}', 1, 'ada', 'Miss Ada', 'Ada', NULL, 'Miss', 'female');
         INSERT INTO haiyaku_appearance VALUES ('{KEEPER}', 2, 'ada');
         INSERT INTO haiyaku_relation VALUES ('{KEEPER}', 3, 'ada', 'tom', 'friend', 'they row together');
         INSERT INTO haiyaku_same_as VALUES ('{KEEPER}', 4, 'ada', 'tom');
         INSERT INTO haiyaku_series VALUES ('keepers', 'The Keepers');
         INSERT INTO haiyaku_order VALUES ('keepers', 'published', 0);
         INSERT INTO haiyaku_order_book VALUES
            ('keepers', 'published', 0, '{KEEPER}'),
            ('keepers', 'published', 1, '{RETURN}');
         INSERT INTO haiyaku_link VALUES
            ('keepers', 'published', '{RETURN}', 'ada', '{KEEPER}', 'ada', 'name', NULL, 1, 0, 0);"
    ))
    .unwrap();
    conn
}

#[test]
fn migrate_creates_every_table() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    assert_eq!(tables(&conn), TABLES);
    assert_eq!(schema_version(&conn).unwrap(), Some(1));
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn migrate_twice_is_a_no_op() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    migrate(&conn).unwrap();
    assert_eq!(tables(&conn), TABLES);
    assert_eq!(schema_version(&conn).unwrap(), Some(1));
    assert_eq!(count(&conn, "haiyaku_schema"), 1);
}

#[test]
fn migrate_leaves_user_version_alone() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", 31).unwrap();
    migrate(&conn).unwrap();
    let user_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
    assert_eq!(user_version, 31);
}

#[test]
fn migrate_leaves_foreign_keys_as_set() {
    for on in [true, false] {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", on).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(tables(&conn), TABLES);
        let after: bool = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0)).unwrap();
        assert_eq!(after, on);
    }
}

#[test]
fn newer_schema_is_refused() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    conn.execute("UPDATE haiyaku_schema SET version = 2", []).unwrap();
    assert!(matches!(migrate(&conn), Err(Error::SchemaTooNew { found: 2, supported: 1 })));
    assert_eq!(schema_version(&conn).unwrap(), Some(2));
}

#[test]
fn migrate_in_a_rolled_back_transaction_leaves_no_tables() {
    let conn = Connection::open_in_memory().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    migrate(&tx).unwrap();
    assert_eq!(schema_version(&tx).unwrap(), Some(1));
    tx.rollback().unwrap();
    assert!(tables(&conn).is_empty());
    assert_eq!(schema_version(&conn).unwrap(), None);
}

#[test]
fn migrate_in_a_committed_transaction_stays() {
    let conn = Connection::open_in_memory().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    migrate(&tx).unwrap();
    tx.commit().unwrap();
    assert_eq!(tables(&conn), TABLES);
    assert_eq!(schema_version(&conn).unwrap(), Some(1));
}

#[test]
fn up_to_date_database_takes_no_write_lock() {
    // Under target/, so the file is never in the system's temporary directory.
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let path = dir.path().join("current.db");
    let writer = Connection::open(&path).unwrap();
    migrate(&writer).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();

    let other = Connection::open(&path).unwrap();
    // Fail at once on the writer's lock rather than wait for it.
    other.busy_timeout(std::time::Duration::ZERO).unwrap();
    migrate(&other).unwrap();
    assert_eq!(schema_version(&other).unwrap(), Some(1));
}

#[test]
fn migrate_looks_only_at_the_main_database() {
    // Under target/, so the file is never in the system's temporary directory.
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let side = dir.path().join("side.db");
    migrate(&Connection::open(&side).unwrap()).unwrap();

    // A database with no haiyaku tables of its own and a migrated one attached to it, as a sidecar might be.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("ATTACH DATABASE ?1 AS side", [side.to_str().unwrap()]).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), None);
    migrate(&conn).unwrap();
    assert_eq!(tables(&conn), TABLES);
    assert_eq!(schema_version(&conn).unwrap(), Some(1));
    assert_eq!(count(&conn, "main.haiyaku_schema"), 1);
    assert_eq!(count(&conn, "side.haiyaku_schema"), 1);
}

#[test]
fn a_temporary_table_of_the_name_is_not_the_schema() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TEMP TABLE haiyaku_schema (version INTEGER NOT NULL);
         INSERT INTO temp.haiyaku_schema VALUES (9)",
    )
    .unwrap();
    assert_eq!(schema_version(&conn).unwrap(), None);
    migrate(&conn).unwrap();
    assert_eq!(tables(&conn), TABLES);
    assert_eq!(schema_version(&conn).unwrap(), Some(1));
    // The version went into the main database's table and left the temporary one alone.
    let temporary: u32 = conn.query_row("SELECT version FROM temp.haiyaku_schema", [], |row| row.get(0)).unwrap();
    assert_eq!(temporary, 9);
}

#[test]
fn schema_table_without_its_row_is_refused() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    conn.execute("DELETE FROM haiyaku_schema", []).unwrap();
    assert!(matches!(migrate(&conn), Err(Error::Invalid(why)) if why.contains("haiyaku_schema")));
    assert!(matches!(schema_version(&conn), Err(Error::Invalid(why)) if why.contains("haiyaku_schema")));
    assert_eq!(tables(&conn), TABLES);
}

#[test]
fn fresh_connection_has_no_version() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(schema_version(&conn).unwrap(), None);
    assert!(tables(&conn).is_empty());
}

// SQLite checks that a foreign key names a real key of its parent only when a row is written, so a
// mistyped reference in the schema passes `migrate` and shows here.
#[test]
fn every_table_takes_a_row_with_foreign_keys_on() {
    let conn = filled();
    for table in TABLES {
        assert!(count(&conn, table) > 0, "{table} is empty");
    }
}

#[test]
fn relation_to_an_entity_never_introduced_is_refused() {
    let conn = filled();
    conn.execute(
        "INSERT INTO haiyaku_event VALUES (?1, 5, 1, 1, 7, 'relation', 'She wrote to the harbour master.')",
        [KEEPER],
    )
    .unwrap();
    let dangling = conn
        .execute(
            "INSERT INTO haiyaku_relation VALUES (?1, 5, 'ada', 'harbour-master', 'acquaintance', 'she writes to him')",
            [KEEPER],
        )
        .unwrap_err();
    // The extended code: the plain one is also what a failed CHECK returns.
    assert_eq!(dangling.sqlite_error().map(|e| e.extended_code), Some(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY));
    assert_eq!(count(&conn, "haiyaku_relation"), 1);
}

#[test]
fn deleting_events_takes_their_child_rows() {
    let conn = filled();
    conn.execute("DELETE FROM haiyaku_event WHERE book = ?1", [KEEPER]).unwrap();
    for table in ["haiyaku_name", "haiyaku_appearance", "haiyaku_relation", "haiyaku_same_as"] {
        assert_eq!(count(&conn, table), 0, "{table}");
    }
}

#[test]
fn deleting_a_series_takes_its_orders_and_links() {
    let conn = filled();
    conn.execute("DELETE FROM haiyaku_series WHERE name = 'keepers'", []).unwrap();
    for table in ["haiyaku_order", "haiyaku_order_book", "haiyaku_link"] {
        assert_eq!(count(&conn, table), 0, "{table}");
    }
    assert_eq!(count(&conn, "haiyaku_book"), 2);
}
