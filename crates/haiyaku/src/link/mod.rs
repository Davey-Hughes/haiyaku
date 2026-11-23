//! Linking the books of a series: which character of a later book is a character of a book before it in a
//! reading order. A later character's names find the earlier characters it could be, and turn down those its
//! names rule out (the design's "The link step", step 3); a model call then chooses among those left, and its
//! answer counts only on a quote found in what the call sent.

mod names;
mod prompt;
mod run;

pub use run::{Call, Refusal, Report, TurnedDown, Verdict, link_order};
