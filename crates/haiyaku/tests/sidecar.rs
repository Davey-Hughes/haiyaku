//! Sidecar files: a book exported to a file of its own and imported into another database, and the hash of
//! a file.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{db, keeper, keepers, keepers_links, keepers_return};
use haiyaku::Error;
use haiyaku::store::{
    SCHEMA_VERSION, export_book, import_book, put_links, put_series, read_book, read_links, schema_version,
    sha256_file, write_book,
};
use rusqlite::Connection;
use tempfile::TempDir;

/// A directory for one test's files. Under target/, so they are never in the system's temporary directory.
fn dir() -> TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// The names of the files in `dir`, in name order.
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

fn count(conn: &Connection, table: &str) -> u32 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

/// The hash of `keeper()`.
fn hash() -> String {
    keeper().book.sha256
}

/// `keeper()` exported to `keeper.haiyaku.db` in `dir`.
fn exported(dir: &Path) -> PathBuf {
    let mut source = db();
    write_book(&mut source, &keeper()).unwrap();
    let path = dir.join("keeper.haiyaku.db");
    export_book(&source, &hash(), &path).unwrap();
    path
}

/// A refused import stored nothing in `target`.
fn assert_no_book(target: &Connection) {
    for table in ["haiyaku_book", "haiyaku_chapter", "haiyaku_document", "haiyaku_entity", "haiyaku_event"] {
        assert_eq!(count(target, table), 0, "{table}");
    }
}

#[test]
fn export_then_import() {
    let dir = dir();
    let sidecar = exported(dir.path());

    let mut target = db();
    import_book(&mut target, &sidecar, &hash()).unwrap();
    assert_eq!(read_book(&target, &hash()).unwrap(), keeper());
}

#[test]
fn export_holds_the_one_book_and_no_series() {
    let dir = dir();
    let mut source = db();
    write_book(&mut source, &keeper()).unwrap();
    write_book(&mut source, &keepers_return()).unwrap();
    put_series(&mut source, &keepers()).unwrap();
    put_links(&mut source, "keepers", "published", &keepers_links()).unwrap();

    let second = keepers_return().book.sha256;
    let path = dir.path().join("return.haiyaku.db");
    export_book(&source, &second, &path).unwrap();
    // The temporary file the export wrote to is gone, and so is its journal.
    assert_eq!(files(dir.path()), ["return.haiyaku.db"]);

    let sidecar = Connection::open(&path).unwrap();
    assert_eq!(schema_version(&sidecar).unwrap(), Some(SCHEMA_VERSION));
    assert_eq!(read_book(&sidecar, &second).unwrap(), keepers_return());
    assert_eq!(count(&sidecar, "haiyaku_book"), 1);
    for table in ["haiyaku_series", "haiyaku_order", "haiyaku_order_book", "haiyaku_link"] {
        assert_eq!(count(&sidecar, table), 0, "{table}");
    }
}

#[test]
fn import_takes_the_expected_hash_in_any_letter_case() {
    let dir = dir();
    let sidecar = exported(dir.path());

    let mut target = db();
    import_book(&mut target, &sidecar, &hash().to_uppercase()).unwrap();
    assert_eq!(read_book(&target, &hash()).unwrap(), keeper());
}

#[test]
fn importing_an_unchanged_book_keeps_its_links() {
    let dir = dir();
    let mut library = db();
    write_book(&mut library, &keeper()).unwrap();
    write_book(&mut library, &keepers_return()).unwrap();
    put_series(&mut library, &keepers()).unwrap();
    put_links(&mut library, "keepers", "published", &keepers_links()).unwrap();

    // As a scan of the library's folder does: every sidecar it finds, changed or not.
    for (name, log) in [("keeper.haiyaku.db", keeper()), ("return.haiyaku.db", keepers_return())] {
        let path = dir.path().join(name);
        export_book(&library, &log.book.sha256, &path).unwrap();
        import_book(&mut library, &path, &log.book.sha256).unwrap();
        assert_eq!(read_links(&library, "keepers", "published").unwrap(), keepers_links(), "{name}");
    }
}

#[test]
fn import_refuses_other_hash() {
    let dir = dir();
    let sidecar = exported(dir.path());
    let other = "c".repeat(64);

    let mut target = db();
    let result = import_book(&mut target, &sidecar, &other);
    assert!(
        matches!(&result, Err(Error::HashMismatch { expected, found }) if *expected == other && *found == hash()),
        "{result:?}"
    );
    assert_no_book(&target);
}

#[test]
fn import_refuses_a_sidecar_with_two_books() {
    let dir = dir();
    let sidecar = exported(dir.path());
    let mut second = Connection::open(&sidecar).unwrap();
    second.pragma_update(None, "foreign_keys", "ON").unwrap();
    write_book(&mut second, &keepers_return()).unwrap();
    drop(second);

    let mut target = db();
    let result = import_book(&mut target, &sidecar, &hash());
    assert!(matches!(&result, Err(Error::HashMismatch { found, .. }) if found == "2 books"), "{result:?}");
    assert_no_book(&target);
}

#[test]
fn import_refuses_newer_schema() {
    let dir = dir();
    let sidecar = exported(dir.path());
    let newer = SCHEMA_VERSION + 1;
    Connection::open(&sidecar).unwrap().execute("UPDATE haiyaku_schema SET version = ?1", [newer]).unwrap();

    let mut target = db();
    let result = import_book(&mut target, &sidecar, &hash());
    assert!(
        matches!(result, Err(Error::SchemaTooNew { found, supported: SCHEMA_VERSION }) if found == newer),
        "{result:?}"
    );
    assert_no_book(&target);
}

#[test]
fn import_leaves_sidecar_untouched() {
    let dir = dir();
    let sidecar = exported(dir.path());
    // As on a read-only share: an import that tried to write would fail here.
    let mut permissions = fs::metadata(&sidecar).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&sidecar, permissions).unwrap();
    let before = fs::read(&sidecar).unwrap();

    import_book(&mut db(), &sidecar, &hash()).unwrap();
    assert!(fs::read(&sidecar).unwrap() == before, "the import changed the sidecar's bytes");
    assert_eq!(files(dir.path()), ["keeper.haiyaku.db"]);
}

// Version 1 is the first schema, so the only older file there can be is one from before any table. Migrated,
// it holds no book, which is enough to show that the migration ran, and ran on a copy.
#[test]
fn import_migrates_an_older_sidecar_in_memory() {
    let dir = dir();
    let sidecar = dir.path().join("old.haiyaku.db");
    Connection::open(&sidecar)
        .unwrap()
        .execute_batch("CREATE TABLE haiyaku_schema (version INTEGER NOT NULL); INSERT INTO haiyaku_schema VALUES (0)")
        .unwrap();
    let before = fs::read(&sidecar).unwrap();

    let mut target = db();
    let result = import_book(&mut target, &sidecar, &hash());
    // Not SQLite's "no such table": the book was looked for in tables the migration made.
    assert!(matches!(&result, Err(Error::HashMismatch { found, .. }) if found == "no book"), "{result:?}");
    assert!(fs::read(&sidecar).unwrap() == before, "the import changed the sidecar's bytes");
    assert_eq!(files(dir.path()), ["old.haiyaku.db"]);
    assert_no_book(&target);
}

#[test]
fn import_refuses_a_database_that_is_not_a_sidecar() {
    let dir = dir();
    let notes = dir.path().join("notes.db");
    Connection::open(&notes).unwrap().execute_batch("CREATE TABLE note (text TEXT NOT NULL)").unwrap();

    let mut target = db();
    let result = import_book(&mut target, &notes, &hash());
    assert!(matches!(&result, Err(Error::Invalid(why)) if why.contains("not a haiyaku sidecar")), "{result:?}");
    assert_no_book(&target);
}

#[test]
fn import_refuses_a_file_that_is_not_a_database() {
    let dir = dir();
    let letter = dir.path().join("letter.txt");
    fs::write(&letter, "Dear Ada, the lamp is out again.").unwrap();

    let mut target = db();
    let result = import_book(&mut target, &letter, &hash());
    assert!(matches!(result, Err(Error::Sqlite(_))), "{result:?}");
    assert_no_book(&target);
}

// A connection that could write would make the missing file, an empty database, where the sidecar should be.
#[test]
fn import_of_a_missing_sidecar_creates_no_file() {
    let dir = dir();
    let mut target = db();
    let result = import_book(&mut target, &dir.path().join("gone.haiyaku.db"), &hash());
    assert!(matches!(result, Err(Error::Sqlite(_))), "{result:?}");
    assert!(files(dir.path()).is_empty());
    assert_no_book(&target);
}

#[test]
fn export_replaces_existing_file() {
    let dir = dir();
    let mut source = db();
    write_book(&mut source, &keeper()).unwrap();
    let path = dir.path().join("keeper.haiyaku.db");
    export_book(&source, &hash(), &path).unwrap();

    let mut retitled = keeper();
    retitled.book.title = "The Lighthouse Keeper, Revised".into();
    write_book(&mut source, &retitled).unwrap();
    export_book(&source, &hash(), &path).unwrap();
    assert_eq!(files(dir.path()), ["keeper.haiyaku.db"]);

    let mut target = db();
    import_book(&mut target, &path, &hash()).unwrap();
    assert_eq!(read_book(&target, &hash()).unwrap(), retitled);
}

#[test]
fn export_unknown_book_writes_nothing() {
    let dir = dir();
    let unknown = "c".repeat(64);
    let result = export_book(&db(), &unknown, &dir.path().join("nobody.haiyaku.db"));
    assert!(matches!(&result, Err(Error::NotFound(what)) if what.contains(&unknown)), "{result:?}");
    assert!(files(dir.path()).is_empty());
}

#[test]
fn failed_export_leaves_no_temporary_file() {
    let dir = dir();
    let mut source = db();
    write_book(&mut source, &keeper()).unwrap();
    // A directory is in the way, so the rename at the end fails.
    let taken = dir.path().join("keeper.haiyaku.db");
    fs::create_dir(&taken).unwrap();

    let result = export_book(&source, &hash(), &taken);
    assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
    assert_eq!(files(dir.path()), ["keeper.haiyaku.db"]);
    assert!(taken.is_dir());
}

#[test]
fn sha256_of_abc() {
    let dir = dir();
    let path = dir.path().join("abc.txt");
    fs::write(&path, "abc").unwrap();
    assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

// FIPS 180's long test vector: a file that takes more than one read to hash.
#[test]
fn sha256_of_a_million_a() {
    let dir = dir();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a".repeat(1_000_000)).unwrap();
    assert_eq!(sha256_file(&path).unwrap(), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
}

#[test]
fn sha256_of_a_missing_file_is_an_io_error() {
    let dir = dir();
    assert!(matches!(sha256_file(&dir.path().join("gone.epub")), Err(Error::Io(_))));
}
