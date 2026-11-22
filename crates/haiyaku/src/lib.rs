//! haiyaku (配役, "the cast") stores what happens to a book's characters as a log of events in SQLite, and
//! answers who they are and how they relate as of the chapter a reader has reached, so that nothing from later
//! in the book shows. It opens no database of its own: every function takes the caller's
//! [`rusqlite::Connection`], and its tables, all named `haiyaku_...`, live beside the caller's own.

#[cfg(feature = "extract")]
pub mod book;
pub mod cast;
mod error;
#[cfg(feature = "extract")]
pub mod extract;
pub mod fold;
#[cfg(feature = "extract")]
pub mod llm;
pub mod log;
pub mod store;

pub use cast::{Scope, cast_at};
pub use error::{Error, Result};
