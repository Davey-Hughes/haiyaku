//! The series file, which names a series, its books and the orders they can be read in; and `haiyaku series`,
//! which stores the series it names.
//!
//! The file is TOML, as the linking design's "The series file" lays it out:
//!
//! ```toml
//! name = "homer"
//! title = "Homer"
//!
//! [books]
//! iliad = "The Iliad.epub"
//! odyssey = "The Odyssey.epub"
//!
//! [[order]]
//! name = "composition"
//! books = ["iliad", "odyssey"]
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use haiyaku::Error;
use haiyaku::store::{Order, Series, put_series, read_series, sha256_file};
use rusqlite::Connection;
use toml::{Table, Value};

use crate::paths::{self, Env};
use crate::{Failure, open_existing_database_to_write, say};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The series file: a TOML file with the series' name and title, its books as [books] label = "path" with
    /// each path from the file's folder, and each reading order as [[order]] with its name and its books' labels
    file: PathBuf,
}

/// A series file, read and its books hashed.
#[derive(Debug)]
pub struct SeriesFile {
    /// The file's path as given, which every message about the file starts with.
    pub path: PathBuf,
    /// The series the file names, each book by the hash of its EPUB.
    pub series: Series,
    /// The path of each book an order holds, by its hash, for the messages that name a book.
    pub books: HashMap<String, PathBuf>,
}

/// Stores the series the file `args.file` names in the database at `db`, by default the one `env` points to,
/// in place of the stored series of its name, and says how many links that series loses; or, when the database
/// holds the file's series already, stores nothing and says so.
pub fn run(args: &Args, db: Option<&Path>, env: Env<'_>) -> Result<(), Failure> {
    let file = read(&args.file)?;
    let database = match db {
        Some(path) => path.to_owned(),
        None => paths::database(env)?,
    };
    let Some(mut conn) = open_existing_database_to_write(&database)? else { return Err(no_database(&file, &database)) };
    // Storing the series again would drop every link of its orders, which still hold: they were made over the same
    // books, and a book whose log has changed since has lost its links already.
    match read_series(&conn, &file.series.name) {
        Ok(stored) if stored == file.series => {
            return say(&format!("series {} is unchanged, and keeps its links", file.series.name));
        }
        Ok(_) | Err(Error::NotFound(_)) => {}
        Err(error) => return Err(error.into()),
    }
    let gone = put_series(&mut conn, &file.series).map_err(|error| {
        // The one book put_series refuses as not stored, it names by its hash; the owner knows it by its path.
        let not_stored = match &error {
            Error::NotFound(what) => file.books.iter().find(|(sha256, _)| *what == format!("book {sha256}")),
            _ => None,
        };
        match not_stored {
            Some((_, book)) => not_extracted(book, &database),
            None => Failure(format!("{}: {error}", file.path.display())),
        }
    })?;
    if gone == 0 {
        return Ok(());
    }
    let (count, are) = if gone == 1 { ("1 link".to_owned(), "is") } else { (format!("{gone} links"), "are") };
    say(&format!(
        "{count} of series {} {are} gone: haiyaku link {} makes them again",
        file.series.name,
        file.path.display()
    ))
}

/// Why the series of `file` cannot be stored in `database`, which is not there or holds no haiyaku tables: no book
/// has been extracted into it, the first the file's orders hold among them.
fn no_database(file: &SeriesFile, database: &Path) -> Failure {
    let first = file.series.orders.iter().flat_map(|order| &order.books).next();
    match first.and_then(|sha256| file.books.get(sha256)) {
        Some(book) => not_extracted(book, database),
        None => Failure(format!(
            "{}: series {} has no book, and {} holds no haiyaku database to store it in",
            file.path.display(),
            file.series.name,
            database.display()
        )),
    }
}

/// Why a series that holds the book at `book` cannot be stored in `database`.
fn not_extracted(book: &Path, database: &Path) -> Failure {
    Failure(format!("{} has not been extracted into {}", book.display(), database.display()))
}

/// The series as the database holds it, refused unless it holds it with the file's title and the file's orders,
/// each with the file's books in the file's order. The file is what the owner asked for: a series stored with other
/// books or orders would have a command link, export or show books the file does not name, in orders it does not
/// give, and one stored with another title would have export write that title into the sidecar.
pub fn stored(conn: &Connection, file: &SeriesFile) -> Result<Series, Failure> {
    match read_series(conn, &file.series.name) {
        Ok(series) if series.title == file.series.title && series.orders == file.series.orders => Ok(series),
        Ok(series) => {
            let other = if series.orders == file.series.orders { "another title" } else { "other orders" };
            Err(Failure(format!(
                "{}: the database holds series {} with {other} than the file: run haiyaku series {} first",
                file.path.display(),
                file.series.name,
                file.path.display()
            )))
        }
        Err(Error::NotFound(_)) => Err(not_stored(file)),
        Err(error) => Err(error.into()),
    }
}

/// The order named `name` of `series`, the series of `file` as the database holds it; refused, with the names of
/// its orders, when it has none of that name.
pub fn order<'a>(file: &SeriesFile, series: &'a Series, name: &str) -> Result<&'a Order, Failure> {
    series.orders.iter().find(|order| order.name == name).ok_or_else(|| {
        let names: Vec<String> = series.orders.iter().map(|order| format!("{:?}", order.name)).collect();
        Failure(format!(
            "{}: series {} has no order {name:?}: its orders are {}",
            file.path.display(),
            series.name,
            names.join(", ")
        ))
    })
}

/// Why a command that needs the series of `file` stored cannot go on: the database does not hold it.
pub fn not_stored(file: &SeriesFile) -> Failure {
    Failure(format!(
        "{}: the database holds no series {}: run haiyaku series {} first",
        file.path.display(),
        file.series.name,
        file.path.display()
    ))
}

/// The series file at `path`, with each book an order holds hashed. A label no order uses is passed over, so
/// its book need not be there.
pub fn read(path: &Path) -> Result<SeriesFile, Failure> {
    let failure = |why: String| Failure(format!("{}: {why}", path.display()));
    let text = fs::read_to_string(path).map_err(|error| failure(error.to_string()))?;
    let top: Table = text.parse().map_err(|error: toml::de::Error| failure(error.to_string()))?;
    let layout = layout(&top).map_err(failure)?;
    // A path is the file's folder's, so the file names the same books from any working directory.
    let folder = path.parent().unwrap_or(Path::new(""));
    let mut hashes: HashMap<&str, String> = HashMap::new();
    let mut books = HashMap::new();
    let mut orders = Vec::new();
    for (name, labels) in layout.orders {
        let mut order = Order { name: name.to_owned(), books: Vec::new() };
        for label in labels {
            let sha256 = match hashes.get(label) {
                Some(sha256) => sha256.clone(),
                None => {
                    let book = folder.join(layout.books[label]);
                    let sha256 = sha256_file(&book).map_err(|error| failure(format!("{}: {error}", book.display())))?;
                    hashes.insert(label, sha256.clone());
                    books.insert(sha256.clone(), book);
                    sha256
                }
            };
            order.books.push(sha256);
        }
        orders.push(order);
    }
    let series = Series { name: layout.name.to_owned(), title: layout.title.to_owned(), orders };
    Ok(SeriesFile { path: path.to_owned(), series, books })
}

/// What a series file says, before its books are hashed.
struct Layout<'a> {
    name: &'a str,
    title: &'a str,
    /// Each book's path as written, by its label.
    books: HashMap<&'a str, &'a str>,
    /// Each order's name and its books' labels, every label one of `books`.
    orders: Vec<(&'a str, Vec<&'a str>)>,
}

/// The layout of the file whose top-level table is `top`, or why it is not a series file's.
fn layout(top: &Table) -> Result<Layout<'_>, String> {
    only(top, &["name", "title", "books", "order"], "")?;
    let name = string(top, "name", "")?;
    // `export` names the series' sidecar after it, beside the file: an empty name would make a hidden file, and one
    // with a separator or "." or ".." would write in another folder.
    if !matches!(Path::new(name).components().collect::<Vec<_>>()[..], [Component::Normal(only)] if only == name) {
        return Err(format!("\"name\" must be one plain file name, with no folder in it, and is {name:?}"));
    }
    let title = string(top, "title", "")?;
    let books = field(top, "books", "")?;
    let Value::Table(labelled) = books else {
        return Err(wrong("", "books", "a table of labels and paths, written [books]", books));
    };
    let mut books = HashMap::new();
    for (label, path) in labelled {
        let Value::String(path) = path else {
            return Err(wrong("[books] ", label, "a string, the path of the book", path));
        };
        books.insert(label.as_str(), path.as_str());
    }
    let listed = field(top, "order", "")?;
    let Value::Array(listed) = listed else {
        return Err(wrong("", "order", "an array of tables, written [[order]]", listed));
    };
    let mut orders = Vec::new();
    for (number, order) in (1..).zip(listed) {
        let place = format!("[[order]] {number}: ");
        let Value::Table(order) = order else {
            return Err(format!("{place}is {}, where an order is a table", kind(order)));
        };
        only(order, &["name", "books"], &place)?;
        let name = string(order, "name", &place)?;
        let labels = field(order, "books", &place)?;
        let Value::Array(labels) = labels else {
            return Err(wrong(&place, "books", "an array of labels from [books]", labels));
        };
        let mut held = Vec::new();
        for label in labels {
            match label {
                Value::String(label) if books.contains_key(label.as_str()) => held.push(label.as_str()),
                Value::String(label) => return Err(format!("{place}\"{label}\" is not a label of [books]")),
                other => return Err(format!("{place}\"books\" holds {}, where a label is a string", kind(other))),
            }
        }
        orders.push((name, held));
    }
    Ok(Layout { name, title, books, orders })
}

/// Refuses a key of `table` that is not one of `known`. `place` starts a message, and says which table it is.
fn only(table: &Table, known: &[&str], place: &str) -> Result<(), String> {
    match table.keys().find(|key| !known.contains(&key.as_str())) {
        Some(key) => Err(format!("{place}unknown key \"{key}\"")),
        None => Ok(()),
    }
}

/// The value of `key` in `table`, which has to have one.
fn field<'a>(table: &'a Table, key: &str, place: &str) -> Result<&'a Value, String> {
    table.get(key).ok_or_else(|| format!("{place}\"{key}\" is missing"))
}

/// The string at `key` in `table`.
fn string<'a>(table: &'a Table, key: &str, place: &str) -> Result<&'a str, String> {
    match field(table, key, place)? {
        Value::String(text) => Ok(text),
        other => Err(wrong(place, key, "a string", other)),
    }
}

/// Why `found`, the value at `key`, is not `wanted`.
fn wrong(place: &str, key: &str, wanted: &str, found: &Value) -> String {
    format!("{place}\"{key}\" must be {wanted}, and is {}", kind(found))
}

/// What `value` is, as a message says it: `a string`, `an integer`.
fn kind(value: &Value) -> String {
    let name = value.type_str();
    let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{article} {name}")
}
