//! The one error type every function in this crate returns.

use std::fmt;

/// What a haiyaku function returns.
pub type Result<T> = std::result::Result<T, Error>;

/// Why a haiyaku function failed.
#[derive(Debug)]
pub enum Error {
    /// SQLite failed a statement, a constraint of the schema included.
    Sqlite(rusqlite::Error),
    /// A file could not be read or written.
    Io(std::io::Error),
    /// Nothing is stored under what was looked up; the string names it, as in `book <sha256>`.
    NotFound(String),
    /// Input that breaks a rule this crate states; the string says which.
    Invalid(String),
    /// The connection has `PRAGMA foreign_keys` off, and a write relies on the schema's foreign keys to
    /// refuse a dangling reference and to cascade a delete.
    ForeignKeysOff,
    /// The database was written by a later haiyaku than this one, which cannot know what its tables mean.
    SchemaTooNew { found: u32, supported: u32 },
    /// A file holds a different book from the one asked for. `found` is the hash of the one book the file
    /// holds, or says how many there are when that is not one: `no book`, `2 books`.
    HashMismatch { expected: String, found: String },
    // Epub and Model are built only by code behind the `extract` feature, and are variants with every set
    // of features: an enum whose shape depends on features breaks a caller's `match`.
    /// A file cannot be read as an EPUB; the string says what is wrong with it, and where.
    Epub(String),
    /// A model backend failed, timed out, or answered with something that is not the JSON asked for; the
    /// string says which.
    Model(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The cause is in the message as well as in `source`: a caller that prints only the outermost
            // error would otherwise show "sqlite" and nothing about what went wrong.
            Self::Sqlite(e) => write!(f, "sqlite: {e}"),
            Self::Io(e) => write!(f, "i/o: {e}"),
            Self::NotFound(what) => write!(f, "{what} not found"),
            Self::Invalid(why) => write!(f, "invalid input: {why}"),
            Self::ForeignKeysOff => {
                f.write_str("PRAGMA foreign_keys is off on this connection, and haiyaku writes need it on")
            }
            Self::SchemaTooNew { found, supported } => {
                write!(f, "the haiyaku schema is version {found}, and this build supports up to version {supported}")
            }
            Self::HashMismatch { expected, found } => {
                write!(f, "expected the book with sha256 {expected}, found {found}")
            }
            Self::Epub(what) => write!(f, "EPUB: {what}"),
            Self::Model(what) => write!(f, "model: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::NotFound(_)
            | Self::Invalid(_)
            | Self::ForeignKeysOff
            | Self::SchemaTooNew { .. }
            | Self::HashMismatch { .. }
            | Self::Epub(_)
            | Self::Model(_) => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    /// One of each variant that has no cause, with the text its message has to carry.
    fn without_a_cause() -> [(Error, Vec<&'static str>); 7] {
        [
            (Error::NotFound("book abc".into()), vec!["book abc"]),
            (Error::Invalid("seq must run 1, 2, 3".into()), vec!["seq must run 1, 2, 3"]),
            (Error::ForeignKeysOff, vec!["foreign_keys"]),
            (Error::SchemaTooNew { found: 7, supported: 3 }, vec!["version 7", "version 3"]),
            (Error::HashMismatch { expected: "abc".into(), found: "2 books".into() }, vec!["abc", "2 books"]),
            (Error::Epub("no table of contents".into()), vec!["no table of contents"]),
            (Error::Model("timed out after 600 s".into()), vec!["timed out after 600 s"]),
        ]
    }

    fn io() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no sidecar on the share")
    }

    #[test]
    fn display_names_each_variants_own_data() {
        for (error, carried) in without_a_cause() {
            let message = error.to_string();
            for text in carried {
                assert!(message.contains(text), "{error:?} says {message:?}, without {text:?}");
            }
        }
        // The two with a cause say what it was, for a caller that prints the outermost error alone.
        let cause = rusqlite::Error::QueryReturnedNoRows;
        assert_eq!(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows).to_string(), format!("sqlite: {cause}"));
        assert_eq!(Error::Io(io()).to_string(), "i/o: no sidecar on the share");
    }

    #[test]
    fn source_is_the_cause_of_sqlite_and_io_alone() {
        let sqlite = Error::Sqlite(rusqlite::Error::QueryReturnedNoRows);
        let cause = sqlite.source().expect("a SQLite error has its cause");
        assert!(matches!(cause.downcast_ref(), Some(rusqlite::Error::QueryReturnedNoRows)));

        let read = Error::Io(io());
        let cause = read.source().expect("an i/o error has its cause");
        assert_eq!(
            cause.downcast_ref::<std::io::Error>().map(std::io::Error::kind),
            Some(std::io::ErrorKind::NotFound)
        );

        for (error, _) in without_a_cause() {
            assert!(error.source().is_none(), "{error:?}");
        }
    }

    #[test]
    fn epub_and_model_errors_say_what_failed() {
        let epub = Error::Epub("no table of contents".into());
        let message = epub.to_string();
        assert!(message.contains("EPUB"), "{message:?}");
        assert!(message.contains("no table of contents"), "{message:?}");
        assert!(epub.source().is_none());

        let model = Error::Model("timed out after 600 s".into());
        let message = model.to_string();
        assert!(message.contains("model"), "{message:?}");
        assert!(message.contains("timed out after 600 s"), "{message:?}");
        assert!(model.source().is_none());
    }

    #[test]
    fn from_gives_the_variant_of_its_cause() {
        assert!(matches!(
            Error::from(rusqlite::Error::QueryReturnedNoRows),
            Error::Sqlite(rusqlite::Error::QueryReturnedNoRows)
        ));
        assert!(matches!(Error::from(io()), Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound));
    }
}
