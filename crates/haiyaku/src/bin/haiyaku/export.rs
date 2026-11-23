//! `haiyaku export`: a sidecar, the file that carries a book's log, or a series with its books and links, to
//! another database.

use std::path::{Path, PathBuf};

use haiyaku::Error;
use haiyaku::store::{export_book, export_series, sha256_file};

use crate::paths::{self, Env};
use crate::{Failure, open_existing_database, say, series};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// An extracted book, an EPUB, whose sidecar is <stem>.haiyaku.db beside it; or a series file stored with
    /// haiyaku series, whose sidecar, named after the series, is <name>.series.haiyaku.db beside the file
    path: PathBuf,
}

/// Writes the sidecar of the book or the series file at `args.path`, from the database at `db`, by default
/// the one `env` points to, and prints the path it wrote.
pub fn run(args: &Args, db: Option<&Path>, env: Env<'_>) -> Result<(), Failure> {
    let database = match db {
        Some(path) => path.to_owned(),
        None => paths::database(env)?,
    };
    let is_epub = args.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("epub"));
    let sidecar = if is_epub { book(&args.path, &database)? } else { series(&args.path, &database)? };
    say(&sidecar.display().to_string())
}

/// Writes the sidecar of the book at `path`, `<stem>.haiyaku.db` beside it, and returns its path.
fn book(path: &Path, database: &Path) -> Result<PathBuf, Failure> {
    // A book is stored under the hash of its file, so the file, not its name, says which book it is.
    let sha256 = sha256_file(path).map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    let not_extracted = || Failure(format!("{} has not been extracted into {}", path.display(), database.display()));
    let Some(conn) = open_existing_database(database)? else { return Err(not_extracted()) };
    let sidecar = path.with_extension("haiyaku.db");
    match export_book(&conn, &sha256, &sidecar) {
        Ok(()) => Ok(sidecar),
        Err(Error::NotFound(_)) => Err(not_extracted()),
        Err(error) => Err(Failure(format!("{}: {error}", sidecar.display()))),
    }
}

/// Writes the sidecar of the series the file at `path` names, `<name>.series.haiyaku.db` beside the file, and
/// returns its path.
fn series(path: &Path, database: &Path) -> Result<PathBuf, Failure> {
    let file = series::read(path)?;
    let Some(conn) = open_existing_database(database)? else { return Err(series::not_stored(&file)) };
    let stored = series::stored(&conn, &file)?;
    // Books often share the series file's folder, and a series named as one of their files would otherwise share
    // its sidecar's path, each export writing over the other's.
    let sidecar = path.with_file_name(format!("{}.series.haiyaku.db", stored.name));
    export_series(&conn, &stored.name, &sidecar).map_err(|error| Failure(format!("{}: {error}", sidecar.display())))?;
    Ok(sidecar)
}
