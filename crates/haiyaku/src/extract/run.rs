//! A whole book extracted: for each chapter in order its two calls, each with its prompt and the check of its
//! reply, with a cache of replies that lets a run that stopped start again where it stopped.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::cache::{Cache, answer, replace};
use super::check::{Rejected, check, check_reveals, reveal_schema, schema};
use super::prompt::{PROMPT_VERSION, SYSTEM, prompt, reveal_prompt};
use super::roster::Roster;
use crate::Result;
use crate::book::Book;
use crate::llm::{Backend, Request};
use crate::log::{BookLog, BookRecord, Chapter};

/// Which chapters [`extract_book`] extracts, and where it keeps what it is told.
#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// The first entry of the book's table of contents to extract, counted from 1.
    pub first: u32,
    /// The last entry to extract, itself included.
    pub last: u32,
    /// The directory a book's replies are kept in, under `<book sha256>/replies/`, so that a run started again
    /// calls no model for a call already answered; and under `<book sha256>/rejected/<nnn>.json`, what the
    /// checks of both calls for the chapter at entry `nnn` of the table of contents turned down, each marked with
    /// its call, and what the first call's check noted, an empty record for a chapter without text. A file is
    /// named by the entry and not by the chapter's place in the run, so it holds the record of the latest run over
    /// that entry, whatever range that run covered, and a run over other entries leaves it as it is. `None` reads
    /// and writes nothing.
    pub cache: Option<&'a Path>,
    /// When the book was extracted, in RFC 3339 and UTC: what [`BookRecord::extracted_at`] records.
    pub extracted_at: &'a str,
}

/// The log of table-of-contents entries `options.first` to `options.last` of `book`: each a chapter, in order,
/// sent to `backend` twice. The first call carries its [`prompt`] and the [`schema`], and the reply's items are
/// [`check`]ed against the chapter and the chapters before it; the second carries its [`reveal_prompt`], made
/// from the roster that check left, and the [`reveal_schema`], and the reply is checked with
/// [`check_reveals`], so its events follow the first reply's. A chapter's row sums both calls' seconds, kept and
/// rejected counts and costs, and gives no cost when either call states none. `progress` is given each
/// chapter's row once the chapter is done.
///
/// A chapter without text is sent to no model: its row keeps nothing and costs 0, which is known. Either call
/// that fails with [`Error::Model`](crate::Error::Model), which is how a backend reports any failure, is made
/// once more, and a second failure in a row ends the run. With a cache (see [`Options::cache`]) a reply already
/// kept is taken in place of a call, with the seconds and cost it recorded, so a run started again reports what
/// the whole book took and pays only for the calls no earlier run finished.
///
/// It writes to no database: the caller stores the log, so a run that fails leaves the database as it was.
///
/// # Errors
///
/// [`Error::Invalid`](crate::Error::Invalid) when the entries are not in the table of contents, before any call.
/// [`Error::Model`](crate::Error::Model) naming the chapter's number and title and which of its calls failed,
/// with the cause, when one fails twice. [`Error::Io`](crate::Error::Io) when the cache cannot be read or
/// written.
pub fn extract_book(
    book: &Book,
    backend: &dyn Backend,
    options: &Options<'_>,
    progress: &mut dyn FnMut(&Chapter),
) -> Result<BookLog> {
    let chapters = book.chapters(options.first, options.last)?;
    let cache = options.cache.map(|dir| Cache::new(dir, &book.sha256));
    let schema = schema();
    let schema_text = schema.to_string();
    let reveal_schema = reveal_schema();
    let reveal_schema_text = reveal_schema.to_string();
    let mut roster = Roster::new();
    let mut rows = Vec::with_capacity(chapters.len());
    for chapter in &chapters {
        let (seconds, cost_usd, kept, rejected) = if chapter.blocks.is_empty() {
            // Nothing a model said of a chapter with no text could be checked against it, so the call could only
            // cost.
            if let Some(cache) = &cache {
                // An empty record, so that what an earlier run wrote for this entry does not stand for this one.
                put_rejected(cache, chapter.toc_entry, &[], &[], &BTreeMap::new())?;
            }
            (0.0, Some(0.0), 0, 0)
        } else {
            let what = format!("chapter {} ({})", chapter.number, chapter.title);
            let user = prompt(&roster, chapter);
            let request = Request { system: SYSTEM, user: &user, schema: &schema };
            // Any object fits: the checks turn a reply's items down one by one, so one bad item costs that item
            // and not the call.
            let first = format!("{what}, first call");
            let reply = answer(backend, &request, &schema_text, cache.as_ref(), &first, Value::is_object)?;
            let outcome = check(&mut roster, chapter, &reply.json);
            let user = reveal_prompt(&roster, chapter);
            let request = Request { system: SYSTEM, user: &user, schema: &reveal_schema };
            let second = format!("{what}, reveal call");
            let reveal = answer(backend, &request, &reveal_schema_text, cache.as_ref(), &second, Value::is_object)?;
            let revealed = check_reveals(&mut roster, chapter, &reveal.json);
            if let Some(cache) = &cache {
                put_rejected(cache, chapter.toc_entry, &outcome.rejected, &revealed.rejected, &outcome.notes)?;
            }
            let rejected = outcome.rejected.len().saturating_add(revealed.rejected.len());
            (
                reply.seconds + reveal.seconds,
                sum_costs(reply.cost_usd, reveal.cost_usd),
                outcome.kept.saturating_add(revealed.kept),
                u32::try_from(rejected).unwrap_or(u32::MAX),
            )
        };
        let row = Chapter {
            number: chapter.number,
            toc_entry: chapter.toc_entry,
            title: chapter.title.clone(),
            seconds,
            cost_usd,
            kept,
            rejected,
        };
        progress(&row);
        rows.push(row);
    }
    Ok(BookLog {
        book: BookRecord {
            sha256: book.sha256.clone(),
            title: book.title.clone(),
            language: book.language.clone(),
            backend: backend.name().to_owned(),
            model: backend.model().to_owned(),
            prompt_version: PROMPT_VERSION,
            extracted_at: options.extracted_at.to_owned(),
        },
        chapters: rows,
        documents: book.document_rows(&chapters),
        entities: roster.entities(),
        events: roster.events().to_vec(),
    })
}

/// Writes to `cache` what the checks of the chapter at entry `toc_entry` turned down, the first call's and then
/// the second's, each marked with its call, and `notes`, over what an earlier run wrote.
fn put_rejected(
    cache: &Cache,
    toc_entry: u32,
    first: &[Rejected],
    reveal: &[Rejected],
    notes: &BTreeMap<&str, u32>,
) -> Result<()> {
    let rejected: Vec<Value> = [("first", first), ("reveal", reveal)]
        .into_iter()
        .flat_map(|(call, rejected)| {
            rejected
                .iter()
                .map(move |rejected| json!({ "call": call, "reason": rejected.reason, "item": rejected.item }))
        })
        .collect();
    let written = json!({ "rejected": rejected, "notes": notes });
    replace(&cache.dir().join("rejected").join(format!("{toc_entry:03}.json")), &format!("{written:#}"))
}

/// What a chapter's two calls cost together, or `None` when either states no cost: the cost of one alone would
/// read as the whole chapter's.
fn sum_costs(first: Option<f64>, second: Option<f64>) -> Option<f64> {
    Some(first? + second?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cost_is_known_only_when_both_calls_state_one() {
        assert_eq!(sum_costs(None, None), None);
        assert_eq!(sum_costs(Some(0.25), None), None);
        assert_eq!(sum_costs(None, Some(0.25)), None);
        assert_eq!(sum_costs(Some(0.25), Some(0.5)), Some(0.75));
    }
}
