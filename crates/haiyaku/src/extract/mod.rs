//! Extraction: one model call per chapter, with every item of the reply checked against the chapter's text
//! before it enters the log.

mod check;
mod prompt;
mod roster;
mod run;
mod text;

pub use check::{Outcome, Rejected, check, schema};
pub use prompt::{PROMPT_VERSION, SYSTEM, prompt};
pub use roster::Roster;
pub use run::{Options, extract_book};
pub use text::{MIN_QUOTE_WEIGHT, QuoteError, locate_name, locate_quote, weight};
