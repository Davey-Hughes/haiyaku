//! `show --html`: one page that steps through a cast chapter by chapter, for checking a run: the cast of an
//! extracted book, or with `--series` that of a reading order of a series up to a book.

use std::slice;

use haiyaku::fold::{Cast, EntityKey};
use haiyaku::fold_series;
use haiyaku::log::{BookLog, Chapter, EventBody};
use haiyaku::store::{Link, LinkMethod};
use serde_json::{Value, json};

/// The page, with `__DATA__` where the data goes. It asks no other host for anything: its style and its
/// script are in it, and it draws only the data embedded in it.
const PAGE: &str = include_str!("view.html");

/// The page for `log`: a step for each chapter from 0 to the last extracted, each the cast a reader has after
/// it, and the slider at chapter `at`.
pub fn page(log: &BookLog, at: u32) -> String {
    let books = Books { logs: slice::from_ref(log), series: false };
    let mut steps = vec![books.step(0, 0, &[])];
    steps.extend(log.chapters.iter().map(|chapter| books.step(0, chapter.number, &[])));
    render(&books, steps, at as usize)
}

/// The page for a reading order of a series up to a book: `logs` are the order's books up to that one, which is
/// the last, and `links` the order's links. A step for nothing read, then one for each chapter of each book, each
/// the cast a reader has after it; each step also lists the links applied by then. The slider starts at the
/// step of chapter `at` of the last book.
pub fn series_page(logs: &[BookLog], links: &[Link], at: u32) -> String {
    let books = Books { logs, series: true };
    let mut steps = vec![books.step(0, 0, links)];
    for (index, log) in logs.iter().enumerate() {
        steps.extend(log.chapters.iter().map(|chapter| books.step(index, chapter.number, links)));
    }
    // A stored book's chapters are numbered 1, 2, 3... (`write_book` stores no other), so chapter `n` of the
    // last book is `n` steps past the last chapter of the book before it, or past the step for nothing read.
    let before: usize = logs.iter().rev().skip(1).map(|log| log.chapters.len()).sum();
    render(&books, steps, before + at as usize)
}

/// The page with its data: the title of the book shown, the titles of the page's books, the steps, and the
/// index of the step the slider starts at.
fn render(books: &Books<'_>, steps: Vec<Value>, at: usize) -> String {
    let titles: Vec<&str> = books.logs.iter().map(|log| log.book.title.as_str()).collect();
    let shown = titles.last().copied().unwrap_or_default();
    let data = json!({ "title": shown, "books": titles, "at": at, "steps": steps });
    PAGE.replace("__DATA__", &script_text(&data.to_string()))
}

/// The books of a page, in reading order, which its data names by their index here.
struct Books<'a> {
    logs: &'a [BookLog],
    /// Whether the page is a series'. An entity's id is only its book's, so on a series page an id carries the
    /// book's index, and two books' `c1` are two characters; a page of one book keeps the log's ids, and its
    /// steps carry neither a book nor links.
    series: bool,
}

impl Books<'_> {
    /// The index of the book `sha256` among the page's, if it is one of them.
    fn find(&self, sha256: &str) -> Option<usize> {
        self.logs.iter().position(|log| log.book.sha256 == sha256)
    }

    /// The index of the book of a key or a chapter of a cast the page folded.
    ///
    /// # Panics
    ///
    /// When the page does not hold the book, which such a cast cannot name: the fold names only the books it is
    /// handed, and the page hands it only its own.
    fn index(&self, sha256: &str) -> usize {
        self.find(sha256).expect("a cast names only the books it was folded from")
    }

    /// The id of a character as the data gives it.
    fn id(&self, key: &EntityKey) -> String {
        if self.series { format!("{}:{}", self.index(&key.book), key.id) } else { key.id.clone() }
    }

    /// Chapter `n` of the book at `book`, as the cast a reader has after it, with its title and counts (`null`
    /// for 0, which is no chapter). Every chapter in it has its book beside it, as the index of that book; a
    /// fact's `overturned` is `null` for a fact that stands.
    fn step(&self, book: usize, n: u32, links: &[Link]) -> Value {
        let chapter: Option<&Chapter> = self.logs[book].chapters.iter().find(|chapter| chapter.number == n);
        let Cast { characters, relations } = fold_series(&self.logs[..=book], Some(n), links);
        let characters: Vec<Value> = characters
            .iter()
            .map(|character| {
                json!({
                    "id": self.id(&character.key),
                    "name": character.name,
                    "aliases": character.aliases,
                    "first": character.first.chapter,
                    "firstBook": self.index(&character.first.book),
                    "last": character.last.chapter,
                    "lastBook": self.index(&character.last.book),
                    "merged": character.merged.iter().map(|key| self.id(key)).collect::<Vec<_>>(),
                })
            })
            .collect();
        let relations: Vec<Value> = relations
            .iter()
            .map(|relation| {
                let facts: Vec<Value> = relation
                    .facts
                    .iter()
                    .map(|fact| {
                        json!({
                            "kind": fact.kind.as_str(),
                            "from": fact.from.as_ref().map(|key| self.id(key)),
                            "note": fact.note,
                            "quote": fact.quote,
                            "first": fact.first.chapter,
                            "firstBook": self.index(&fact.first.book),
                            "last": fact.last.chapter,
                            "lastBook": self.index(&fact.last.book),
                            "overturned": fact.overturned.as_ref().map(|overturn| {
                                json!({
                                    "chapter": overturn.at.chapter,
                                    "book": self.index(&overturn.at.book),
                                    "note": overturn.note,
                                    "quote": overturn.quote,
                                })
                            }),
                        })
                    })
                    .collect();
                json!({ "a": self.id(&relation.a), "b": self.id(&relation.b), "facts": facts })
            })
            .collect();
        let mut step = json!({
            "chapter": n,
            "title": chapter.map(|chapter| &chapter.title),
            "entry": chapter.map(|chapter| chapter.toc_entry),
            "kept": chapter.map(|chapter| chapter.kept),
            "rejected": chapter.map(|chapter| chapter.rejected),
            "characters": characters,
            "relations": relations,
        });
        if self.series {
            step["book"] = json!(book);
            step["links"] = self.applied(book, n, links);
        }
        step
    }

    /// The links of `links` that a reader after chapter `n` of the book at `book` has had applied: those of a book
    /// before it, and its own from that chapter or an earlier one. Each joins `later` into `earlier` from chapter
    /// `chapter` of the book at `book`, which is `later`'s; `name` is the name `later` is introduced under, since
    /// a link found at its introduction merges it before any step shows it alone; `quote` is the model's evidence,
    /// `null` for a link made on the name.
    fn applied(&self, book: usize, n: u32, links: &[Link]) -> Value {
        let applied: Vec<Value> = links
            .iter()
            .filter_map(|link| {
                // An order's links include those of its books after the page's last, which the page never reads.
                let later = self.find(&link.later.book)?;
                (later < book || (later == book && link.chapter <= n)).then(|| {
                    let quote = match &link.method {
                        LinkMethod::Model { quote } => Some(quote),
                        LinkMethod::Name => None,
                    };
                    json!({
                        "later": self.id(&link.later),
                        "name": introduced_as(&self.logs[later], &link.later.id),
                        "earlier": self.id(&link.earlier),
                        "book": later,
                        "chapter": link.chapter,
                        "quote": quote,
                    })
                })
            })
            .collect();
        Value::Array(applied)
    }
}

/// The name the entity `id` of `log` is first introduced under; `null` for one `log` never introduces, which a
/// stored link cannot name (`put_links` refuses it).
fn introduced_as<'a>(log: &'a BookLog, id: &str) -> Option<&'a str> {
    log.events.iter().find_map(|event| match &event.body {
        EventBody::Introduced { entity, name } if entity == id => Some(name.text.as_str()),
        _ => None,
    })
}

/// `json` made safe to be the text of a `<script>` element. The browser ends that element at the first
/// `</script` in it, whatever the script's own quoting says, and passes over the end tag once a `<!--` and a
/// `<script` have come before it; a quote from a book may hold either. Neither can survive here: `\/` and
/// `\u003c` are JSON's own escapes for `/` and `<`, which stand only inside strings in JSON, so the text
/// parses to the same data.
fn script_text(json: &str) -> String {
    json.replace("</", "<\\/").replace("<!--", "\\u003c!--")
}
