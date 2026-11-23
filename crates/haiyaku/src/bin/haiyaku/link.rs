//! `haiyaku link`: each reading order of a stored series linked with a model, its links stored and the calls that
//! made them reported.

use std::path::{Path, PathBuf};

use haiyaku::fold::EntityKey;
use haiyaku::link::{Call, Refusal, Report, TurnedDown, Verdict, link_order};
use haiyaku::llm::{Backend, Settings, backend};
use haiyaku::log::BookLog;
use haiyaku::store::{Link, Order, put_links, read_book};
use rusqlite::Connection;

use crate::extract::{cost, counted};
use crate::paths::{self, Env};
use crate::{Failure, open_existing_database_to_write, say, series, tell};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The series file, whose series `haiyaku series` has stored
    file: PathBuf,
    /// The one reading order to link, by its name; every order of the series by default
    #[arg(long, value_name = "NAME")]
    order: Option<String>,
    /// The model: claude (Sonnet), claude:MODEL, ollama:MODEL, openai:BASE_URL#MODEL, or replay:DIR for the
    /// replies recorded in DIR. A replay hands out its replies in the order of the calls, so a rerun that finds
    /// calls in the cache gives the first call it makes the first reply, not the one recorded for that call: rerun
    /// a replay with --no-cache
    #[arg(long, value_name = "SPEC", default_value = "claude")]
    backend: String,
    /// Keep no cache: no reply is read from it or kept in it, so every call goes to the model
    #[arg(long)]
    no_cache: bool,
}

/// Links each reading order of the series the file `args.file` names, or the one `--order` names, as the database at
/// `db`, by default the one `env` points to, holds it, and stores each order's links in place of those it had.
pub fn run(args: &Args, db: Option<&Path>, env: Env<'_>) -> Result<(), Failure> {
    let file = series::read(&args.file)?;
    let database = match db {
        Some(path) => path.to_owned(),
        None => paths::database(env)?,
    };
    let cache = if args.no_cache { None } else { Some(paths::cache(env)?) };
    // Checked before the backend is built, so that a run that can link nothing says why without needing a backend
    // that works.
    let Some(mut conn) = open_existing_database_to_write(&database)? else { return Err(series::not_stored(&file)) };
    let series = series::stored(&conn, &file)?;
    let orders: Vec<&Order> = match &args.order {
        None => series.orders.iter().collect(),
        Some(name) => vec![series::order(&file, &series, name)?],
    };
    let settings = Settings {
        ollama_host: env("OLLAMA_HOST").map(|host| host.to_string_lossy().into_owned()),
        ..Settings::default()
    };
    let backend = backend(&args.backend, &settings)?;
    // A progress line that cannot be written does not stop a run whose calls are paid for: the first such failure
    // is held, and is the command's once the links are stored.
    let mut unsaid = None;
    for order in orders {
        link_one_order(&mut conn, &series.name, order, backend.as_ref(), cache.as_deref(), &mut unsaid)?;
    }
    unsaid.map_or(Ok(()), Err)
}

/// Links `order` of the stored series `series` with `backend`, with replies kept in `cache` when there is one,
/// stores its links in place of those it had and prints its report. A progress line that cannot be written goes
/// into `unsaid` unless it holds one already. Nothing is stored for the order unless every call of it is answered
/// and the database still holds every log the calls were made from, and the order with the same books.
fn link_one_order(
    conn: &mut Connection,
    series: &str,
    order: &Order,
    backend: &dyn Backend,
    cache: Option<&Path>,
    unsaid: &mut Option<Failure>,
) -> Result<(), Failure> {
    // The library's error names the call or the book that failed, not the order, and a run links several.
    let in_order = |error: haiyaku::Error| Failure(format!("order {}: {error}", order.name));
    let logs = order.books.iter().map(|book| read_book(conn, book)).collect::<haiyaku::Result<Vec<_>>>();
    let logs = logs.map_err(in_order)?;
    let (links, report) = link_order(&logs, backend, cache, &mut |call| {
        if unsaid.is_none() {
            let at = place(&call.later, call.chapter, &call.name, &logs);
            *unsaid = tell(&format!("order {}: {at}: {}", order.name, verdict(call, &logs))).err();
        }
    })
    .map_err(in_order)?;
    put_links(conn, series, &order.name, &links, &logs).map_err(|error| not_stored(&order.name, error, &logs))?;
    say(&reported(&order.name, &logs, &links, &report))
}

/// Why `put_links` stored nothing for `order`, whose links were made against `logs`.
fn not_stored(order: &str, error: haiyaku::Error, logs: &[BookLog]) -> Failure {
    match error {
        // The owner knows the book by its title, and a rerun is the way out: it links against the log stored now.
        haiyaku::Error::LogReplaced { book } => Failure(format!(
            "order {order}: {} was extracted or imported again while linking ran, so the order's links were not \
             stored: run haiyaku link again",
            title(&book, logs)
        )),
        // A rerun links the order as it is stored now, or asks for haiyaku series when the file says otherwise.
        haiyaku::Error::OrderReplaced { series, .. } => Failure(format!(
            "order {order}: series {series} was stored again while linking ran, so the order's links were not stored: \
             run haiyaku link again"
        )),
        error => Failure(format!("order {order}: {error}")),
    }
}

/// The report of an order: a line for each call, a line for each earlier character turned down without a call, then
/// the order's line with its links, its calls, their seconds and their cost.
fn reported(order: &str, logs: &[BookLog], links: &[Link], report: &Report) -> String {
    let mut lines: Vec<String> = report.calls.iter().map(|call| call_line(call, logs)).collect();
    lines.extend(report.turned_down.iter().flat_map(|turned_down| turned_down_lines(turned_down, logs)));
    lines.push(format!(
        "order {order}: {}, {}, {:.1} s, {}",
        counted(links.len(), "link"),
        counted(report.calls.len(), "call"),
        report.seconds,
        cost(report.cost_usd)
    ));
    lines.join("\n")
}

/// A call's line in the report: where it was made, the candidates it listed, the answer, the verdict and the quote.
/// The answer is the ref the reply gave, as written, and the candidates are in the order the call listed them, which
/// is the order its refs count.
fn call_line(call: &Call, logs: &[BookLog]) -> String {
    let candidates: Vec<String> = call.candidates.iter().map(|(key, name)| character(key, name, logs)).collect();
    let answer = call.answer.as_deref().map_or_else(|| "none".to_owned(), one_line);
    // The quote ends the line, so it needs no marks around it, which a quote of speech would have inside it.
    let quote = if call.quote.is_empty() { "no quote".to_owned() } else { format!("quote: {}", one_line(&call.quote)) };
    let at = place(&call.later, call.chapter, &call.name, logs);
    format!("{at}: candidates {}; answer {answer}; {}; {quote}", candidates.join(", "), verdict(call, logs))
}

/// A line for each earlier character `turned_down` holds: where its later character's name turned it down, the
/// earlier character and why.
fn turned_down_lines(turned_down: &TurnedDown, logs: &[BookLog]) -> Vec<String> {
    let at = place(&turned_down.later, turned_down.chapter, &turned_down.name, logs);
    let line = |(key, name, refusal): &(EntityKey, String, Refusal)| {
        let why = match refusal {
            Refusal::Conflict => "their names conflict".to_owned(),
            Refusal::OtherSex => "a title of the other sex".to_owned(),
            Refusal::Related { relative, name } => {
                format!("related to {}, linked to it", character(relative, name, logs))
            }
        };
        format!("{at}: turned down {}; {why}", character(key, name, logs))
    };
    turned_down.candidates.iter().map(line).collect()
}

/// Where a call was made or a candidate turned down: the later character's book's title, the chapter and the name.
fn place(later: &EntityKey, chapter: u32, name: &str, logs: &[BookLog]) -> String {
    format!("{}, chapter {chapter}, \"{}\"", title(&later.book, logs), one_line(name))
}

/// An earlier character as the report names it: its shown name and its book's title, since two books can each
/// have a character of one name.
fn character(key: &EntityKey, name: &str, logs: &[BookLog]) -> String {
    format!("{} ({})", one_line(name), title(&key.book, logs))
}

/// The title of the book `sha256` among `logs`, or the hash for one that is not among them.
fn title<'a>(sha256: &'a str, logs: &'a [BookLog]) -> &'a str {
    logs.iter().find(|log| log.book.sha256 == sha256).map_or(sha256, |log| log.book.title.as_str())
}

/// What `call`'s reply came to.
fn verdict(call: &Call, logs: &[BookLog]) -> String {
    match &call.verdict {
        Verdict::Linked(key) => {
            let candidate = call.candidates.iter().find(|(candidate, _)| candidate == key);
            format!("linked to {}", character(key, candidate.map_or(key.id.as_str(), |(_, name)| name.as_str()), logs))
        }
        Verdict::NoneChosen => "none chosen".to_owned(),
        Verdict::NotListed => "answer not listed".to_owned(),
        Verdict::QuoteTooShort => "quote too short".to_owned(),
        Verdict::QuoteIsAName => "quote is a name".to_owned(),
        Verdict::QuoteNotFound => "quote not found".to_owned(),
    }
}

/// `text` on one line, each run of whitespace one space: a reply's quote or answer is the model's own text, and a
/// line break in it would start what reads as another call's line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use haiyaku::log::{BookLog, BookRecord};

    use super::not_stored;

    #[test]
    fn a_book_or_a_series_stored_again_asks_for_a_rerun() {
        let book = BookRecord {
            sha256: "e".repeat(64),
            title: "The Reed Light".into(),
            language: "en".into(),
            backend: "recorded".into(),
            model: "none".into(),
            prompt_version: 1,
            extracted_at: "2026-01-01T00:00:00Z".into(),
        };
        let logs = [BookLog { book, chapters: vec![], documents: vec![], entities: vec![], events: vec![] }];
        let failure = not_stored("published", haiyaku::Error::LogReplaced { book: "e".repeat(64) }, &logs);
        assert_eq!(
            failure.0,
            "order published: The Reed Light was extracted or imported again while linking ran, so the order's \
             links were not stored: run haiyaku link again"
        );
        let replaced = haiyaku::Error::OrderReplaced { series: "keepers".into(), order: "published".into() };
        assert_eq!(
            not_stored("published", replaced, &logs).0,
            "order published: series keepers was stored again while linking ran, so the order's links were not \
             stored: run haiyaku link again"
        );
        let other =
            not_stored("published", haiyaku::Error::NotFound("order published of series keepers".into()), &logs);
        assert_eq!(other.0, "order published: order published of series keepers not found");
    }
}
