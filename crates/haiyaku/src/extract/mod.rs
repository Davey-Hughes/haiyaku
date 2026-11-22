//! Extraction: two model calls per chapter, the first for what the chapter shows and the second for what it
//! reveals or overturns, with every item of each reply checked against the chapter's text before it enters the
//! log.

mod check;
mod prompt;
mod roster;
mod run;
mod text;

pub use check::{Outcome, Rejected, check, check_reveals, reveal_schema, schema};
pub use prompt::{PROMPT_VERSION, SYSTEM, prompt, reveal_prompt};
pub use roster::Roster;
pub use run::{Options, extract_book};
pub use text::{MIN_QUOTE_WEIGHT, QuoteError, locate_name, locate_quote, weight};
