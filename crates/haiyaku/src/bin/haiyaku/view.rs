//! `show --html`: one page that steps through the cast of an extracted book chapter by chapter, for checking
//! a run.

use haiyaku::fold::{Cast, fold_book};
use haiyaku::log::BookLog;
use serde_json::{Value, json};

/// The page, with `__DATA__` where the data goes. It asks no other host for anything: its style and its
/// script are in it, and it draws only the data embedded in it.
const PAGE: &str = include_str!("view.html");

/// The page for `log`: a step for each chapter from 0 to `last`, each the cast a reader has after it, and
/// the slider at chapter `at`.
pub fn page(log: &BookLog, last: u32, at: u32) -> String {
    let steps: Vec<Value> = (0..=last).map(|n| step(log, n)).collect();
    let data = json!({ "title": log.book.title, "at": at, "steps": steps });
    PAGE.replace("__DATA__", &script_text(&data.to_string()))
}

/// Chapter `n`, with its title and counts (`null` for 0, which is no chapter), and the cast after it. A
/// page holds one book, so a character's id alone names it.
fn step(log: &BookLog, n: u32) -> Value {
    let chapter = log.chapters.iter().find(|chapter| chapter.number == n);
    let Cast { characters, relations } = fold_book(log, n);
    let characters: Vec<Value> = characters
        .iter()
        .map(|character| {
            json!({
                "id": character.key.id,
                "name": character.name,
                "aliases": character.aliases,
                "first": character.first.chapter,
                "last": character.last.chapter,
                "merged": character.merged.iter().map(|key| &key.id).collect::<Vec<_>>(),
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
                        "from": fact.from.as_ref().map(|key| &key.id),
                        "note": fact.note,
                        "quote": fact.quote,
                        "first": fact.first.chapter,
                        "last": fact.last.chapter,
                    })
                })
                .collect();
            json!({ "a": relation.a.id, "b": relation.b.id, "facts": facts })
        })
        .collect();
    json!({
        "chapter": n,
        "title": chapter.map(|chapter| &chapter.title),
        "entry": chapter.map(|chapter| chapter.toc_entry),
        "kept": chapter.map(|chapter| chapter.kept),
        "rejected": chapter.map(|chapter| chapter.rejected),
        "characters": characters,
        "relations": relations,
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
