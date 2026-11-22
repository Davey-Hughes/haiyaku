//! The prompt a chapter is sent with and the part of the roster it carries, each on chapters of invented
//! sentences with a roster built from replies written by hand; and a whole book extracted, on invented EPUBs
//! with recorded replies.

#![cfg(feature = "extract")]

mod common;

use std::fs;
use std::path::Path;

use common::db;
use common::epub::{
    KEEPER_CHAPTERS, KEEPER_METADATA, Toc, book, chapter_text, keeper_epub, keeper_replies, name, nav_map, ncx,
    relation, relevant_roster, xhtml,
};
use haiyaku::book::{Book, ChapterText};
use haiyaku::extract::{Options, PROMPT_VERSION, Roster, check, extract_book, prompt};
use haiyaku::fold::Cast;
use haiyaku::llm::{Backend, Replay, Reply, Request};
use haiyaku::log::{BookLog, Chapter, EventBody, RelationKind};
use haiyaku::store::write_book;
use haiyaku::{Error, Result, Scope, cast_at};
use serde_json::{Value, json};
use tempfile::TempDir;

const CHARACTERS: &str = "Characters so far (ref | name | other names):";
const RELATIONS: &str = "Relations so far (from | kind | to | note):";

/// The lines under `heading` in `prompt`, up to the blank line that ends them, or `None` when the prompt has no
/// such heading.
fn section<'a>(prompt: &'a str, heading: &str) -> Option<Vec<&'a str>> {
    let start = prompt.find(&format!("{heading}\n"))? + heading.len() + 1;
    Some(prompt[start..].lines().take_while(|line| !line.is_empty()).collect())
}

/// A character item of a reply, with no aliases.
fn character(reference: &str, name: Value, evidence: &str) -> Value {
    json!({ "ref": reference, "name": name, "aliases": [], "evidence": evidence })
}

/// Checks `reply` to `chapter` into `roster`, and fails unless every item of it is kept.
fn keep_all(roster: &mut Roster, chapter: &ChapterText, reply: &Value) {
    let outcome = check(roster, chapter, reply);
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
}

#[test]
fn first_prompt_says_none_yet() {
    let prompt = prompt(&Roster::new(), &chapter_text(1, 1, &["At dusk the keeper climbed to the lamp."]));
    assert_eq!(section(&prompt, CHARACTERS), Some(vec!["(none yet)"]), "{prompt}");
    assert_eq!(section(&prompt, RELATIONS), None, "{prompt}");
}

#[test]
fn prompt_starts_with_the_chapter_and_ends_with_the_rules() {
    let mut chapter =
        chapter_text(2, 3, &["In the morning the post boat left a letter.", "She read it twice on the steps."]);
    chapter.title = "The Jetty".into();
    let prompt = prompt(&Roster::new(), &chapter);
    assert!(prompt.starts_with(&format!("Chapter 2: The Jetty\n\n{}\n\n{CHARACTERS}\n", chapter.text())), "{prompt}");
    // Every line after the rules' heading is a rule, so nothing of the chapter or the lists follows them.
    let (_, rules) = prompt.split_once("\n\nRules:\n").expect(&prompt);
    assert!(!rules.is_empty(), "{prompt}");
    assert!(rules.lines().all(|line| line.starts_with("- ") || line.starts_with("  - ")), "{prompt}");
}

#[test]
fn prompt_names_every_relation_kind() {
    let prompt = prompt(&Roster::new(), &chapter_text(1, 1, &["At dusk the keeper climbed to the lamp."]));
    let kinds = RelationKind::ALL.map(RelationKind::as_str).join(", ");
    assert!(prompt.contains(&kinds), "{prompt}");
}

/// The rule of `prompt` whose line starts with `start` once its indent is trimmed.
fn rule<'a>(prompt: &'a str, start: &str) -> &'a str {
    let (_, rules) = prompt.split_once("\n\nRules:\n").expect(prompt);
    rules.lines().map(str::trim_start).find(|line| line.starts_with(start)).expect(prompt)
}

#[test]
fn only_a_listed_personal_name_makes_a_character_the_listed_one() {
    let prompt = prompt(&Roster::new(), &chapter_text(1, 1, &["At dusk the keeper climbed to the lamp."]));
    // A description or a form of address fits many people, so being listed is no sign that it means this one.
    let rule = rule(&prompt, "- A character listed above");
    assert!(rule.contains("(a given name or a surname, not a description or a form of address)"), "{rule}");
    assert!(rule.contains("only when this chapter shows it is"), "{rule}");
}

#[test]
fn a_listed_name_is_spared_the_run_of_words_rule() {
    let prompt = prompt(&Roster::new(), &chapter_text(1, 1, &["At dusk the keeper climbed to the lamp."]));
    // The check takes a listed name the character carries whether or not this chapter writes it, so the rule that
    // a name stands in this chapter holds only for a name the model gives from it.
    let rule = rule(&prompt, "- In each name and alias");
    assert!(
        rule.contains("as the list writes it for a listed name. A name you give from this chapter is one run"),
        "{rule}"
    );
}

#[test]
fn a_listed_pair_gains_a_relation_of_another_kind() {
    let prompt = prompt(&Roster::new(), &chapter_text(1, 1, &["At dusk the keeper climbed to the lamp."]));
    // A listed relation must not hold back one of another kind that this chapter reveals, even a contrary one.
    let rule = rule(&prompt, "- relations:");
    let further = "A pair already listed above gains a further relation when this chapter reveals one of another kind, \
                   even one that contradicts what is listed: give it as well.";
    assert!(rule.ends_with(further), "{rule}");
}

#[test]
fn prompt_carries_only_the_relevant_roster() {
    let (roster, chapter) = relevant_roster();
    let prompt = prompt(&roster, &chapter);
    // Old Sal is named in the chapter and Mira was seen in the chapter before it; Bren Holt is neither, so
    // neither he nor his relation with Old Sal is listed.
    assert_eq!(section(&prompt, CHARACTERS), Some(vec!["c2 | Old Sal |", "c3 | Mira |"]), "{prompt}");
    assert!(!prompt.contains("Bren"), "{prompt}");
    assert_eq!(section(&prompt, RELATIONS), None, "{prompt}");
}

#[test]
fn prompt_lists_a_relation_through_its_survivors() {
    let mut roster = Roster::new();
    let one = chapter_text(
        1,
        1,
        &[
            "Ada Marsh kept the lamp at Gull Point.",
            "The stranger came up the path at dusk.",
            "Ada gave the stranger a cup of water at the door.",
        ],
    );
    let reply = json!({
        "characters": [
            character(
                "new1",
                name("Ada Marsh", Some("Ada"), Some("Marsh"), None, None),
                "Ada Marsh kept the lamp at Gull Point",
            ),
            character("new2", name("the stranger", None, None, None, None), "The stranger came up the path at dusk"),
        ],
        "relations": [
            relation("new1", "new2", "acquaintance", "gave him water", "Ada gave the stranger a cup of water"),
        ],
        "same_as": [],
    });
    keep_all(&mut roster, &one, &reply);
    let two = chapter_text(
        2,
        2,
        &[
            "Tom Reed came in on the evening boat.",
            "Ada knew Tom Reed from school.",
            "Tom Reed and the stranger walked up the hill together.",
            "The stranger took off his hat, and it was Tom Reed.",
        ],
    );
    let reply = json!({
        "characters": [
            character(
                "new1",
                name("Tom Reed", Some("Tom"), Some("Reed"), None, None),
                "Tom Reed came in on the evening boat",
            ),
        ],
        "relations": [
            relation("c1", "new1", "acquaintance", "from school", "Ada knew Tom Reed from school"),
            relation("c2", "new1", "ally", "walked together", "Tom Reed and the stranger walked up the hill"),
        ],
        "same_as": [{ "ref": "new1", "other": "c2", "evidence": "The stranger took off his hat, and it was Tom Reed" }],
    });
    keep_all(&mut roster, &two, &reply);

    let prompt = prompt(&roster, &chapter_text(3, 3, &["Ada Marsh and Tom Reed rowed out to the point."]));
    // The stranger, c2, is now Tom Reed, c3: the acquaintance from c1 to c2 is the one from c1 to c3, listed
    // once under the note first given, and the ally between c2 and c3 is between c3 and itself.
    assert_eq!(
        section(&prompt, CHARACTERS),
        Some(vec!["c1 | Ada Marsh |", "c3 | Tom Reed | the stranger"]),
        "{prompt}"
    );
    assert_eq!(section(&prompt, RELATIONS), Some(vec!["c1 | acquaintance | c3 | gave him water"]), "{prompt}");
}

#[test]
fn a_cell_holds_no_bar_and_no_line_break() {
    let mut roster = Roster::new();
    let one = chapter_text(
        1,
        1,
        &["Ada Marsh kept the lamp at Gull Point.", "The keeper and Tom Reed were old friends from school."],
    );
    let friends = "The keeper and Tom Reed were old friends";
    let reply = json!({
        "characters": [
            {
                "ref": "new1",
                "name": name("Ada | Marsh", Some("Ada"), Some("Marsh"), None, None),
                "aliases": [name("the\nkeeper", None, None, None, None)],
                "evidence": "Ada Marsh kept the lamp at Gull Point",
            },
            character("new2", name("Tom Reed", Some("Tom"), Some("Reed"), None, None), "Tom Reed were old friends"),
        ],
        "relations": [relation("new1", "new2", "friend", "old friend |\r\n\nfrom school", friends)],
        "same_as": [],
    });
    keep_all(&mut roster, &one, &reply);

    let prompt = prompt(&roster, &chapter_text(2, 2, &["Ada Marsh and Tom Reed rowed out to the point."]));
    assert_eq!(
        section(&prompt, CHARACTERS),
        Some(vec!["c1 | Ada / Marsh | the keeper", "c2 | Tom Reed |"]),
        "{prompt}"
    );
    assert_eq!(section(&prompt, RELATIONS), Some(vec!["c1 | friend | c2 | old friend / from school"]), "{prompt}");
}

/// When "The Keeper"'s extractions are made.
const EXTRACTED_AT: &str = "2026-11-21T12:00:00Z";

/// "The Keeper", written to `dir` and opened.
fn keeper(dir: &TempDir) -> Book {
    Book::open(&keeper_epub(dir.path())).unwrap()
}

/// Entries `first` to `last` of `book` extracted with `replay`, with replies kept in `cache` when there is one.
fn extract(book: &Book, replay: &Replay, first: u32, last: u32, cache: Option<&Path>) -> Result<BookLog> {
    let options = Options { first, last, cache, extracted_at: EXTRACTED_AT };
    extract_book(book, replay, &options, &mut |_| {})
}

/// The cast a reader at `document` sees, with `log` the one book stored.
fn cast(log: &BookLog, document: &str) -> Cast {
    let mut conn = db();
    write_book(&mut conn, log).unwrap();
    cast_at(&conn, &log.book.sha256, document, &Scope::Book).unwrap()
}

/// The names the characters of `cast` are shown under, sorted.
fn names(cast: &Cast) -> Vec<&str> {
    let mut names: Vec<&str> = cast.characters.iter().map(|character| character.name.as_str()).collect();
    names.sort_unstable();
    names
}

/// The message of a run that failed with [`Error::Model`], failing on any other result.
fn model_error(run: Result<BookLog>) -> String {
    match run {
        Err(Error::Model(message)) => message,
        other => panic!("expected a model error, got {other:?}"),
    }
}

#[test]
fn extracts_the_keeper() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    let log = extract(&book, &Replay::new(keeper_replies()), 1, 3, None).unwrap();

    let record = &log.book;
    assert_eq!(record.sha256, book.sha256);
    assert_eq!((record.title.as_str(), record.language.as_str()), ("The Keeper", "en"));
    assert_eq!((record.backend.as_str(), record.model.as_str()), ("replay", "none"));
    assert_eq!(record.prompt_version, PROMPT_VERSION);
    assert_eq!(record.extracted_at, EXTRACTED_AT);
    assert_eq!(log.chapters.iter().map(|chapter| chapter.toc_entry).collect::<Vec<_>>(), [1, 2, 3]);
    for chapter in &log.chapters {
        assert!(chapter.kept > 0, "{chapter:?}");
        assert_eq!(chapter.rejected, 0, "{chapter:?}");
    }
    let chapters: Vec<Option<u32>> = log.documents.iter().map(|document| document.chapter).collect();
    assert_eq!(chapters, [None, Some(1), Some(2), Some(3), None]);

    let mut conn = db();
    write_book(&mut conn, &log).unwrap();
    let after_one = cast_at(&conn, &book.sha256, "OEBPS/c2.xhtml", &Scope::Book).unwrap();
    assert_eq!(names(&after_one), ["Pip", "the keeper"]);
    let whole = cast_at(&conn, &book.sha256, "OEBPS/back.xhtml", &Scope::Book).unwrap();
    let shown = |wanted: &str| whole.characters.iter().find(|character| character.name == wanted);
    let ada = shown("Ada Marsh").unwrap_or_else(|| panic!("{whole:#?}"));
    assert_eq!(ada.aliases, ["the keeper", "Mrs. Marsh"]);
    let tom = shown("Tom Reed").unwrap_or_else(|| panic!("{whole:#?}"));
    assert_eq!(tom.merged.len(), 1, "{tom:?}");
    assert!(shown("the stranger").is_none(), "{whole:#?}");
}

#[test]
fn documents_after_the_range_show_the_whole_extracted_cast() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    let replies = keeper_replies();
    let two = extract(&book, &Replay::new(replies[..2].to_vec()), 1, 2, None).unwrap();
    // A reader at the start of chapter 3 of the whole book has finished chapter 2 and no more.
    let after_two = cast(&extract(&book, &Replay::new(replies), 1, 3, None).unwrap(), "OEBPS/c3.xhtml");
    assert_eq!(names(&after_two), ["Ada Marsh", "Pip", "the stranger"]);

    assert_eq!(cast(&two, "OEBPS/c3.xhtml"), after_two);
    assert_eq!(cast(&two, "OEBPS/back.xhtml"), after_two);
}

#[test]
fn progress_is_called_once_per_chapter() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    let options = Options { first: 1, last: 3, cache: None, extracted_at: EXTRACTED_AT };
    let mut seen: Vec<Chapter> = Vec::new();
    let log = extract_book(&book, &Replay::new(keeper_replies()), &options, &mut |chapter| seen.push(chapter.clone()))
        .unwrap();
    assert_eq!(seen, log.chapters);

    // Each chapter as it is done, so a run that fails has shown the chapters before the one that failed.
    let mut seen: Vec<u32> = Vec::new();
    let failed = extract_book(&book, &Replay::new(keeper_replies()[..2].to_vec()), &options, &mut |chapter| {
        seen.push(chapter.number);
    });
    assert!(failed.is_err());
    assert_eq!(seen, [1, 2]);
}

#[test]
fn one_retry_on_a_model_error() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    let mut replies = vec!["not json".to_owned()];
    replies.extend(keeper_replies());
    let replay = Replay::new(replies);
    let log = extract(&book, &replay, 1, 3, None).unwrap();
    assert_eq!(replay.remaining(), 0);
    assert_eq!(log, extract(&book, &Replay::new(keeper_replies()), 1, 3, None).unwrap());

    let replay = Replay::new(vec!["x".into(), "y".into(), keeper_replies().swap_remove(0)]);
    let message = model_error(extract(&book, &replay, 1, 3, None));
    // The chapter's number and title, and the cause of the second failure.
    assert!(message.contains("chapter 1") && message.contains("Chapter 1"), "{message}");
    assert!(message.contains("reply 2"), "{message}");
    assert_eq!(replay.remaining(), 1, "a second failure ends the run with no third call");
}

/// A replay that reports a time and a cost for each call, as a model's backend does.
#[derive(Debug)]
struct Timed(Replay);

impl Backend for Timed {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn model(&self) -> &str {
        self.0.model()
    }

    fn call(&self, request: &Request<'_>) -> Result<Reply> {
        Ok(Reply { seconds: 1.5, cost_usd: Some(0.25), ..self.0.call(request)? })
    }
}

#[test]
fn second_run_calls_nothing() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let book = keeper(&dir);
    let options = Options { first: 1, last: 3, cache: Some(cache.path()), extracted_at: EXTRACTED_AT };
    let first = extract_book(&book, &Timed(Replay::new(keeper_replies())), &options, &mut |_| {}).unwrap();
    assert!(first.chapters.iter().all(|chapter| chapter.seconds == 1.5 && chapter.cost_usd == Some(0.25)));
    // The second run reports the time and the cost the first one recorded.
    let second = extract(&book, &Replay::new(vec![]), 1, 3, Some(cache.path())).unwrap();
    assert_eq!(second, first);
    // One file a reply, and no temporary file left beside them.
    let replies = cache.path().join(&book.sha256).join("replies");
    assert_eq!(fs::read_dir(replies).unwrap().count(), 3);
}

#[test]
fn reply_file_that_does_not_parse_is_ignored() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let book = keeper(&dir);
    let first = extract(&book, &Replay::new(keeper_replies()), 1, 3, Some(cache.path())).unwrap();
    for file in fs::read_dir(cache.path().join(&book.sha256).join("replies")).unwrap() {
        fs::write(file.unwrap().path(), "{\"raw\": ").unwrap();
    }
    let replay = Replay::new(keeper_replies());
    assert_eq!(extract(&book, &replay, 1, 3, Some(cache.path())).unwrap(), first);
    assert_eq!(replay.remaining(), 0, "every chapter called again");
    assert_eq!(extract(&book, &Replay::new(vec![]), 1, 3, Some(cache.path())).unwrap(), first, "and kept again");
}

#[test]
fn resume_after_failure() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let book = keeper(&dir);
    let replies = keeper_replies();
    let message = model_error(extract(&book, &Replay::new(replies[..2].to_vec()), 1, 3, Some(cache.path())));
    assert!(message.contains("chapter 3"), "{message}");

    let replay = Replay::new(replies[2..].to_vec());
    let resumed = extract(&book, &replay, 1, 3, Some(cache.path())).unwrap();
    assert_eq!(replay.remaining(), 0);
    assert_eq!(resumed, extract(&book, &Replay::new(replies), 1, 3, None).unwrap());
}

/// A book of three entries whose second is a plate, a document with a picture and no text.
fn plated(dir: &TempDir) -> Book {
    let [one, two, _] = KEEPER_CHAPTERS;
    let plate = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Part Two</title></head>
<body><div><img src="plate.png" alt=""/></div></body>
</html>"#;
    let documents = vec![
        ("c1.xhtml", xhtml("Chapter 1", one).into_bytes()),
        ("plate.xhtml", plate.as_bytes().to_vec()),
        ("c2.xhtml", xhtml("Chapter 2", two).into_bytes()),
    ];
    let toc =
        Toc::Ncx(ncx(&nav_map(&[("Chapter 1", "c1.xhtml"), ("Part Two", "plate.xhtml"), ("Chapter 2", "c2.xhtml")])));
    let plated = Book::open(&book(dir.path(), "plate.epub", KEEPER_METADATA, &documents, &toc)).unwrap();
    assert!(plated.chapters(2, 2).unwrap()[0].blocks.is_empty(), "the plate has no text");
    plated
}

#[test]
fn chapter_without_text_calls_no_model() {
    let dir = TempDir::new().unwrap();
    let plated = plated(&dir);

    let replay = Replay::new(keeper_replies()[..2].to_vec());
    let log = extract(&plated, &replay, 1, 3, None).unwrap();
    assert_eq!(replay.remaining(), 0);
    let [first, plate, last] = log.chapters.as_slice() else { panic!("{:?}", log.chapters) };
    assert_eq!((plate.seconds, plate.cost_usd, plate.kept, plate.rejected), (0.0, Some(0.0), 0, 0));
    for chapter in [first, last] {
        assert!(chapter.kept > 0 && chapter.rejected == 0, "{chapter:?}");
    }
}

#[test]
fn rejected_items_go_to_the_cache() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let book = keeper(&dir);
    let mut reply: Value = serde_json::from_str(&keeper_replies()[0]).unwrap();
    let stray = relation("new1", "new9", "friend", "watches the tide", "Below the gallery the rocks were black");
    reply["relations"].as_array_mut().unwrap().push(stray.clone());
    let log = extract(&book, &Replay::new(vec![reply.to_string()]), 1, 1, Some(cache.path())).unwrap();

    assert_eq!(log.chapters[0].rejected, 1);
    let stray_kept = log
        .events
        .iter()
        .any(|event| matches!(&event.body, EventBody::Relation { note, .. } if note == "watches the tide"));
    assert!(!stray_kept, "{:#?}", log.events);
    let path = cache.path().join(&book.sha256).join("rejected").join("001.json");
    let written: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(written["rejected"], json!([{ "reason": "unknown_ref", "item": stray }]), "{written}");
    assert!(written["notes"].is_object(), "{written}");
}

#[test]
fn rejected_files_are_named_by_contents_entry() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let book = keeper(&dir);
    let mut reply: Value = serde_json::from_str(&keeper_replies()[0]).unwrap();
    let stray = relation("new1", "new9", "friend", "watches the tide", "Below the gallery the rocks were black");
    reply["relations"].as_array_mut().unwrap().push(stray.clone());
    extract(&book, &Replay::new(vec![reply.to_string()]), 1, 1, Some(cache.path())).unwrap();
    // Entry 2 is the first chapter of this run, so a file named by the run's chapter would be entry 1's.
    extract(&book, &Replay::new(keeper_replies()[1..2].to_vec()), 2, 2, Some(cache.path())).unwrap();

    let rejected = cache.path().join(&book.sha256).join("rejected");
    let read = |name: &str| -> Value { serde_json::from_slice(&fs::read(rejected.join(name)).unwrap()).unwrap() };
    assert_eq!(read("001.json")["rejected"], json!([{ "reason": "unknown_ref", "item": stray }]));
    assert!(read("002.json")["notes"].is_object());
}

#[test]
fn chapter_without_text_writes_an_empty_rejected_file() {
    let (dir, cache) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let plated = plated(&dir);
    let path = cache.path().join(&plated.sha256).join("rejected").join("002.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, r#"{ "rejected": [{ "reason": "unknown_ref", "item": {} }], "notes": {} }"#).unwrap();

    extract(&plated, &Replay::new(keeper_replies()[..2].to_vec()), 1, 3, Some(cache.path())).unwrap();
    let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(written, json!({ "rejected": [], "notes": {} }));
}
