//! The `haiyaku` command. Its `extract` lists a book's table of contents, or extracts a range of its entries
//! with a model into the database; its `show` prints the cast an extracted book has after a chapter, or writes
//! a page that steps through it chapter by chapter.
//!
//! The paths, the environment and every line of output are the binary's: the library reads no environment
//! variable and prints nothing.

mod clock;
mod extract;
mod paths;
mod show;
mod view;

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, TransactionBehavior};

/// Spoiler-safe character events from books.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// The database of extracted books, in place of $XDG_DATA_HOME/haiyaku/haiyaku.db
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List a book's table of contents, or extract a range of its entries into the database
    Extract(extract::Args),
    /// Print the characters of an extracted book and how they relate, as a reader knows them after a chapter, or
    /// write a page that steps through them chapter by chapter
    Show(show::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match &cli.command {
        Command::Extract(args) => extract::run(args, cli.db.as_deref(), &env),
        Command::Show(args) => show::run(args, cli.db.as_deref(), &env),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            // A stderr that cannot be written to leaves nowhere to say it, and the status still does.
            let _ = tell(&format!("haiyaku: {failure}"));
            ExitCode::FAILURE
        }
    }
}

/// The process's environment variable `name`. The one place the program reads its environment: everything
/// else is handed a lookup, which a test gives its own variables.
fn env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// Why a command failed. `main` says it once on stderr, as `haiyaku: <message>`, and exits with status 1;
/// a usage error is clap's, with clap's status 2.
#[derive(Debug)]
struct Failure(String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<haiyaku::Error> for Failure {
    fn from(error: haiyaku::Error) -> Self {
        Failure(error.to_string())
    }
}

/// The database at `path`, made with its directory when missing, shown to take a write, and set up as every
/// command sets it up.
fn open_database(path: &Path) -> Result<Connection, Failure> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|error| Failure(format!("{}: {error}", dir.display())))?;
    }
    let conn = Connection::open(path).map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    // SQLite opens a file it may not write read-only, without an error, and the first write would then fail
    // only once the work it stores is done.
    if conn.is_readonly(rusqlite::MAIN_DB).map_err(haiyaku::Error::from)? {
        return Err(Failure(format!("{}: the database is read-only", path.display())));
    }
    // A file SQLite may write still takes no write in a directory it may not write, where it cannot make the
    // journal a write needs, nor while another run holds the write lock. Migrating a current schema writes
    // nothing, and migrating an older one or an empty file would fail with SQLite's bare error, so the write is
    // tried first: it says which of the two it is, before the run pays for the work it would store.
    try_write(&conn).map_err(|error| write_failure(path, &error))?;
    set_up(conn)
}

/// Why the database at `path` took no write. Another run's write lock outlasting the busy timeout is no fault of
/// the database, and saying it cannot be written would send the user to its permissions.
fn write_failure(path: &Path, error: &rusqlite::Error) -> Failure {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            Failure(format!("{}: the database is in use by another run", path.display()))
        }
        _ => Failure(format!("{}: the database cannot be written: {error}", path.display())),
    }
}

/// A write to the main database, rolled back so the file is left as it was. Making a table writes a page of the
/// file, which SQLite first keeps in the journal, so a database that cannot have a journal fails here. The table
/// is its own, so a file haiyaku has not migrated takes the write as a migrated one does.
fn try_write(conn: &Connection) -> rusqlite::Result<()> {
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    transaction.execute_batch("CREATE TABLE main.haiyaku_write_probe (x)")?;
    transaction.rollback()
}

/// The database at `path`, set up as every command sets it up, or `None` when there is no file there or
/// no haiyaku tables in it. For a command that only reads: opening a database that is not there would leave
/// an empty one behind, and its directory with it.
fn open_existing_database(path: &Path) -> Result<Option<Connection>, Failure> {
    if !path.try_exists().map_err(|error| Failure(format!("{}: {error}", path.display())))? {
        return Ok(None);
    }
    // Without CREATE, a file removed since the check above is an error rather than made again empty.
    let mut flags = OpenFlags::default();
    flags.remove(OpenFlags::SQLITE_OPEN_CREATE);
    let conn =
        Connection::open_with_flags(path, flags).map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    // A file haiyaku never migrated holds no book, and migrating it would write haiyaku's tables into an empty
    // file or another program's database. One at an older version is migrated: the reads expect the current
    // schema.
    if haiyaku::store::schema_version(&conn)?.is_none() {
        return Ok(None);
    }
    set_up(conn).map(Some)
}

/// Foreign keys on, which the library's writes refuse to go without, and haiyaku's tables brought up to date.
fn set_up(conn: Connection) -> Result<Connection, Failure> {
    conn.pragma_update(None, "foreign_keys", "ON").map_err(haiyaku::Error::from)?;
    haiyaku::store::migrate(&conn)?;
    Ok(conn)
}

/// Writes `text` and a newline to stdout. A pipe the reader closed early, as `head` does, is a failure to
/// report, where `println!` would panic.
fn say(text: &str) -> Result<(), Failure> {
    writeln!(io::stdout().lock(), "{text}").map_err(|error| Failure(format!("stdout: {error}")))
}

/// Writes `text` and a newline to stderr, failing as [`say`] does where `eprintln!` would panic.
fn tell(text: &str) -> Result<(), Failure> {
    writeln!(io::stderr().lock(), "{text}").map_err(|error| Failure(format!("stderr: {error}")))
}
