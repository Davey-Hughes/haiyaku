//! Series, reading orders and links in the database, and the cast a reader has met at a document of a book.

mod common;

use common::{db, keeper, keepers, keepers_light, keepers_links, keepers_logs, keepers_return, texts};
use haiyaku::fold::{Cast, ChapterRef, Character, EntityKey, fold_book};
use haiyaku::log::{BookLog, Document, Entity, EntityKind, Position, RelationKind};
use haiyaku::store::{
    Holding, Link, LinkMethod, Order, Series, finished_chapters, put_links, put_series, read_book, read_links,
    read_series, series_holding, write_book,
};
use haiyaku::{Error, Scope, cast_at, fold_series};
use rusqlite::Connection;

/// Both books, their series and its links.
fn library() -> Connection {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    put_series(&mut conn, &keepers()).unwrap();
    put_links(&mut conn, "keepers", "published", &keepers_links(), &keepers_logs()).unwrap();
    conn
}

/// The hashes of the first book and of the second.
fn books() -> (String, String) {
    (keeper().book.sha256, keepers_return().book.sha256)
}

fn key(book: &str, id: &str) -> EntityKey {
    EntityKey { book: book.into(), id: id.into() }
}

fn in_series(order: Option<&str>) -> Scope {
    Scope::Series { series: "keepers".into(), order: order.map(Into::into) }
}

fn keys(cast: &Cast) -> Vec<&EntityKey> {
    cast.characters.iter().map(|character| &character.key).collect()
}

fn character<'a>(cast: &'a Cast, key: &EntityKey) -> &'a Character {
    cast.characters.iter().find(|character| character.key == *key).unwrap_or_else(|| panic!("no {key:?} in the cast"))
}

fn order(name: &str, books: &[&str]) -> Order {
    Order { name: name.into(), books: books.iter().map(|book| (*book).into()).collect() }
}

/// [`keepers`] with further orders after its own.
fn keepers_and(orders: &[Order]) -> Series {
    let mut series = keepers();
    series.orders.extend_from_slice(orders);
    series
}

/// A change to one field of a log.
type Change = fn(&mut BookLog);

fn not_found(result: Result<impl std::fmt::Debug, Error>, expected: &str) {
    assert!(matches!(&result, Err(Error::NotFound(what)) if what == expected), "{result:?}");
}

#[test]
fn finished_chapters_by_document() {
    let conn = library();
    let (_, second) = books();
    for (document, finished) in
        [("front.xhtml", 0), ("c1.xhtml", 0), ("c2.xhtml", 1), ("c3.xhtml", 2), ("back.xhtml", 3)]
    {
        assert_eq!(finished_chapters(&conn, &second, document).unwrap(), finished, "{document}");
    }
}

#[test]
fn chapter_is_finished_past_its_last_document() {
    let mut conn = db();
    let paths = [
        ("front.xhtml", None),
        ("c1a.xhtml", Some(1)),
        ("c1b.xhtml", Some(1)),
        ("c2.xhtml", Some(2)),
        ("c3.xhtml", Some(3)),
    ];
    let documents = (0..).zip(paths).map(|(ordinal, (path, chapter))| Document { ordinal, path: path.into(), chapter });
    let log = BookLog { documents: documents.collect(), events: Vec::new(), ..keeper() };
    write_book(&mut conn, &log).unwrap();
    for (document, finished) in [("c1a.xhtml", 0), ("c1b.xhtml", 0), ("c2.xhtml", 1), ("c3.xhtml", 2)] {
        assert_eq!(finished_chapters(&conn, &log.book.sha256, document).unwrap(), finished, "{document}");
    }
}

#[test]
fn unknown_document_is_not_found() {
    let conn = library();
    let (first, _) = books();
    let expected = format!("document c9.xhtml in book {first}");
    not_found(finished_chapters(&conn, &first, "c9.xhtml"), &expected);
    not_found(cast_at(&conn, &first, "c9.xhtml", &Scope::Book), &expected);
    not_found(cast_at(&conn, &first, "c9.xhtml", &in_series(None)), &expected);
}

#[test]
fn unknown_book_is_not_found() {
    let conn = library();
    let unknown = "c".repeat(64);
    let expected = format!("book {unknown}");
    not_found(finished_chapters(&conn, &unknown, "c1.xhtml"), &expected);
    // Not the empty cast its lack of events would fold to.
    not_found(cast_at(&conn, &unknown, "c1.xhtml", &Scope::Book), &expected);
    not_found(cast_at(&conn, &unknown, "c1.xhtml", &in_series(None)), &expected);
}

#[test]
fn series_holding_lists_orders_by_rank() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    let (first, second) = books();
    put_series(&mut conn, &keepers_and(&[order("chronological", &[&second, &first]), order("sequel", &[&second])]))
        .unwrap();
    // Rank order, not name order: "chronological" sorts before "published". "sequel" does not hold the book.
    let expected = vec![Holding { series: "keepers".into(), orders: vec!["published".into(), "chronological".into()] }];
    assert_eq!(series_holding(&conn, &first).unwrap(), expected);
}

#[test]
fn series_holding_lists_series_by_name() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    let (first, second) = books();
    put_series(&mut conn, &keepers_and(&[order("chronological", &[&second, &first])])).unwrap();
    let harbour =
        Series { name: "harbour".into(), title: "The Harbour".into(), orders: vec![order("main", &[&second])] };
    put_series(&mut conn, &harbour).unwrap();
    let expected = vec![
        Holding { series: "harbour".into(), orders: vec!["main".into()] },
        Holding { series: "keepers".into(), orders: vec!["published".into(), "chronological".into()] },
    ];
    assert_eq!(series_holding(&conn, &second).unwrap(), expected);
}

#[test]
fn series_holding_of_a_book_in_no_series_is_empty() {
    let mut conn = library();
    write_book(&mut conn, &keepers_light()).unwrap();
    assert_eq!(series_holding(&conn, &keepers_light().book.sha256).unwrap(), []);
}

#[test]
fn series_holding_needs_the_book_stored() {
    let conn = library();
    let unknown = "c".repeat(64);
    not_found(series_holding(&conn, &unknown), &format!("book {unknown}"));
}

#[test]
fn series_and_links_round_trip() {
    let mut conn = library();
    assert_eq!(read_series(&conn, "keepers").unwrap(), keepers());
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());

    // Links come back in reading order, whatever order they were put in.
    let reversed: Vec<Link> = keepers_links().into_iter().rev().collect();
    put_links(&mut conn, "keepers", "published", &reversed, &keepers_logs()).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());

    not_found(read_series(&conn, "lamplighters"), "series lamplighters");
    not_found(read_links(&conn, "keepers", "chronological"), "order chronological of series keepers");
}

#[test]
fn book_scope_ignores_links() {
    let conn = library();
    let (_, second) = books();
    let cast = cast_at(&conn, &second, "c3.xhtml", &Scope::Book).unwrap();
    assert_eq!(keys(&cast), [&key(&second, "ada"), &key(&second, "sailor")]);
    assert_eq!(cast, fold_book(&keepers_return(), 2));
}

#[test]
fn series_scope_links_at_introduction() {
    let conn = library();
    let (first, second) = books();
    let cast = cast_at(&conn, &second, "c2.xhtml", &in_series(None)).unwrap();
    assert_eq!(keys(&cast), [&key(&first, "ada"), &key(&first, "pip"), &key(&first, "tom"), &key(&second, "sailor")]);

    let ada = character(&cast, &key(&first, "ada"));
    assert_eq!(ada.merged, [key(&second, "ada")]);
    assert_eq!(ada.name.text, "Ada Marsh");
    assert_eq!(texts(&ada.aliases), ["the keeper", "Mrs. Marsh", "Ada"]);
    assert_eq!(ada.first, ChapterRef { book: first.clone(), chapter: 1 });
    assert_eq!(ada.last, ChapterRef { book: second.clone(), chapter: 1 });

    // All of the first book, and of the second nothing past its first chapter.
    let whole = fold_book(&keeper(), 3);
    for id in ["pip", "tom"] {
        assert_eq!(character(&cast, &key(&first, id)), character(&whole, &key(&first, id)));
    }
    assert_eq!(cast.relations, whole.relations);
}

#[test]
fn link_waits_for_its_chapter() {
    let conn = library();
    let (first, second) = books();
    let (tom, sailor) = (key(&first, "tom"), key(&second, "sailor"));

    // The reader is in the chapter that calls the sailor Tom, and has not finished it.
    let cast = cast_at(&conn, &second, "c3.xhtml", &in_series(None)).unwrap();
    assert_eq!(character(&cast, &sailor).name.text, "the old sailor");
    assert_eq!(character(&cast, &tom).merged, [key(&first, "stranger")]);

    let cast = cast_at(&conn, &second, "back.xhtml", &in_series(None)).unwrap();
    assert_eq!(keys(&cast), [&key(&first, "ada"), &key(&first, "pip"), &tom]);
    let tom_reed = character(&cast, &tom);
    assert_eq!(tom_reed.name.text, "Tom Reed");
    assert_eq!(texts(&tom_reed.aliases), ["the stranger", "the old sailor", "Tom"]);
    assert_eq!(tom_reed.merged, [key(&first, "stranger"), sailor]);
    assert_eq!(tom_reed.last, ChapterRef { book: second.clone(), chapter: 3 });

    // What the second book says of Ada and the sailor is now said of Ada Marsh and Tom Reed.
    let pair = cast.relations.iter().find(|relation| relation.a == key(&first, "ada") && relation.b == tom).unwrap();
    let ally = pair.facts.iter().find(|fact| fact.kind == RelationKind::Ally).unwrap();
    assert_eq!(ally.first, ChapterRef { book: second, chapter: 2 });
}

#[test]
fn links_of_a_book_read_whole_are_applied() {
    let mut conn = db();
    for log in [keeper(), keepers_return(), keepers_light()] {
        write_book(&mut conn, &log).unwrap();
    }
    let (first, second) = books();
    let third = keepers_light().book.sha256;
    let three = Series { orders: vec![order("published", &[&first, &second, &third])], ..keepers() };
    put_series(&mut conn, &three).unwrap();
    // The third book's captain is linked to the second's sailor, whom only the last chapter of the second
    // links to the first book's Tom Reed.
    let mut links = keepers_links();
    links.push(Link {
        later: key(&third, "captain"),
        earlier: key(&second, "sailor"),
        method: LinkMethod::Name,
        chapter: 1,
        at: Position { document: 1, block: 0 },
    });
    put_links(&mut conn, "keepers", "published", &links, &[keeper(), keepers_return(), keepers_light()]).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), links);
    let (ada, pip, tom) = (key(&first, "ada"), key(&first, "pip"), key(&first, "tom"));
    let (stranger, sailor, captain) = (key(&first, "stranger"), key(&second, "sailor"), key(&third, "captain"));

    // In the third book's first chapter the second book is in whole, and every link of it with it.
    let cast = cast_at(&conn, &third, "c1.xhtml", &in_series(None)).unwrap();
    assert_eq!(keys(&cast), [&ada, &pip, &tom]);
    assert_eq!(character(&cast, &tom).merged, [stranger.clone(), sailor.clone()]);
    let second_read = cast_at(&conn, &second, "back.xhtml", &in_series(None)).unwrap();
    assert_eq!(cast.characters, second_read.characters);
    assert_eq!(cast.relations, second_read.relations);
    assert_eq!(cast.books, [first.as_str(), second.as_str(), third.as_str()]);

    // Past that chapter the captain is Tom Reed as well, through the sailor.
    let cast = cast_at(&conn, &third, "c2.xhtml", &in_series(None)).unwrap();
    assert_eq!(keys(&cast), [&ada, &pip, &tom]);
    let tom_reed = character(&cast, &tom);
    assert_eq!(tom_reed.name.text, "Tom Reed");
    assert_eq!(texts(&tom_reed.aliases), ["the stranger", "the old sailor", "Tom", "Captain Reed"]);
    assert_eq!(tom_reed.merged, [stranger, sailor, captain]);
    assert_eq!(tom_reed.last, ChapterRef { book: third, chapter: 1 });
}

#[test]
fn front_matter_shows_only_earlier_books() {
    let conn = library();
    let (first, second) = books();
    let cast = cast_at(&conn, &second, "front.xhtml", &in_series(None)).unwrap();
    let whole = fold_book(&keeper(), 3);
    assert_eq!(cast.characters, whole.characters);
    assert_eq!(cast.relations, whole.relations);
    assert_eq!(cast.books, [first, second]);
}

#[test]
fn the_cast_lists_the_books_it_folded() {
    let (first, second) = books();
    assert_eq!(fold_book(&keeper(), 0).books, [first.as_str()]);
    let both = fold_series(&[keeper(), keepers_return()], Some(1), &keepers_links());
    assert_eq!(both.books, [first.as_str(), second.as_str()]);
    // In the front matter nothing of the second book is read, and it is listed all the same.
    let front = cast_at(&library(), &second, "front.xhtml", &in_series(None)).unwrap();
    assert_eq!(front.books, [first.as_str(), second.as_str()]);
    assert!(fold_series(&[], None, &[]).books.is_empty());
}

#[test]
fn rewriting_a_book_drops_its_links() {
    let mut conn = library();
    let mut rewrite = keepers_return();
    rewrite.book.extracted_at = "2026-03-01T00:00:00Z".into();
    write_book(&mut conn, &rewrite).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), []);
    assert_eq!(read_series(&conn, "keepers").unwrap(), keepers());
}

#[test]
fn storing_the_log_a_book_has_keeps_its_links() {
    let mut conn = library();
    // Each book is the earlier book of a link or the later one.
    for mut log in [keeper(), keepers_return()] {
        write_book(&mut conn, &log).unwrap();
        assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());

        // The same log still, with its documents and entities listed in another order than they are read in.
        log.documents.reverse();
        log.entities.reverse();
        write_book(&mut conn, &log).unwrap();
        assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
    }
    assert_eq!(read_book(&conn, &keeper().book.sha256).unwrap(), keeper());
    assert_eq!(read_book(&conn, &keepers_return().book.sha256).unwrap(), keepers_return());
}

#[test]
fn a_log_that_differs_in_one_field_drops_the_links() {
    let changes: [(&str, Change); 6] = [
        ("the record", |log| log.book.model = "another".into()),
        ("a chapter", |log| log.chapters[0].seconds += 0.5),
        ("a document", |log| log.documents[4].path = "afterword.xhtml".into()),
        ("an entity", |log| log.entities.push(Entity { id: "tom".into(), kind: EntityKind::Character })),
        ("an event's block", |log| log.events[0].at.block += 1),
        ("an event's body", |log| log.events[3].quote.push('!')),
    ];
    for (what, change) in changes {
        let mut conn = library();
        let mut log = keepers_return();
        change(&mut log);
        write_book(&mut conn, &log).unwrap();
        assert_eq!(read_links(&conn, "keepers", "published").unwrap(), [], "{what}");
        assert_eq!(read_book(&conn, &log.book.sha256).unwrap(), log, "{what}");
    }
}

#[test]
fn link_against_order_is_invalid() {
    let mut conn = library();
    let (first, second) = books();
    let backwards = Link {
        later: key(&first, "ada"),
        earlier: key(&second, "ada"),
        method: LinkMethod::Name,
        chapter: 1,
        at: Position { document: 1, block: 0 },
    };
    let mut links = keepers_links();
    links.truncate(1);
    links.push(backwards);
    assert!(matches!(put_links(&mut conn, "keepers", "published", &links, &keepers_logs()), Err(Error::Invalid(_))));
    // The refused call changed nothing, the link before the bad one included.
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn link_to_a_book_outside_the_order_is_invalid() {
    let mut conn = library();
    let (_, second) = books();
    put_series(&mut conn, &keepers_and(&[order("solo", &[&second])])).unwrap();
    assert!(matches!(
        put_links(&mut conn, "keepers", "solo", &keepers_links(), &[keepers_return()]),
        Err(Error::Invalid(_))
    ));
    not_found(
        put_links(&mut conn, "keepers", "chronological", &keepers_links(), &keepers_logs()),
        "order chronological of series keepers",
    );
}

#[test]
fn link_at_a_document_outside_its_chapter_is_invalid() {
    let mut conn = library();
    // The sailor's link holds from chapter 3, in document 3. In turn: a chapter the reader finishes sooner, a
    // document of another chapter, the back matter, which is in no chapter, and a document the book lacks.
    let cases = [
        (1, 3, "that document is in chapter 3"),
        (3, 2, "that document is in chapter 2"),
        (3, 4, "that document is in no chapter"),
        (3, 9, "the book has no such document"),
    ];
    for (chapter, document, reason) in cases {
        let mut links = keepers_links();
        links[1].chapter = chapter;
        links[1].at.document = document;
        let refused = put_links(&mut conn, "keepers", "published", &links, &keepers_logs());
        assert!(matches!(&refused, Err(Error::Invalid(why)) if why.ends_with(reason)), "{refused:?}");
        assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
    }
}

#[test]
fn link_to_an_unknown_character_is_refused() {
    let mut conn = library();
    let never = |id: &str| format!("to {id} of book {}, which that book never introduces", books().0);

    // The first book has no such entity.
    let mut links = keepers_links();
    links[1].earlier.id = "harbourmaster".into();
    let refused = put_links(&mut conn, "keepers", "published", &links, &keepers_logs());
    assert!(matches!(&refused, Err(Error::Invalid(why)) if why.ends_with(&never("harbourmaster"))), "{refused:?}");
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());

    // Nor has the second the one it is linked from.
    let mut links = keepers_links();
    links[1].later.id = "boatswain".into();
    let refused = put_links(&mut conn, "keepers", "published", &links, &keepers_logs());
    assert!(
        matches!(&refused, Err(Error::Invalid(why)) if why.contains("has not introduced boatswain")),
        "{refused:?}"
    );
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn link_to_a_character_the_earlier_book_never_introduces_is_invalid() {
    let mut conn = db();
    // The first book lists a harbourmaster and has no event that introduces one, so no cast ever holds him.
    let mut first = keeper();
    first.entities.push(Entity { id: "harbourmaster".into(), kind: EntityKind::Character });
    write_book(&mut conn, &first).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    put_series(&mut conn, &keepers()).unwrap();

    let mut links = keepers_links();
    links[1].earlier.id = "harbourmaster".into();
    let refused = put_links(&mut conn, "keepers", "published", &links, &[first, keepers_return()]);
    let said = |why: &str| why.contains("to harbourmaster") && why.ends_with("which that book never introduces");
    assert!(matches!(&refused, Err(Error::Invalid(why)) if said(why)), "{refused:?}");
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), []);
}

#[test]
fn link_from_a_chapter_before_the_later_character_is_introduced_is_invalid() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    // The sailor comes into the second book in its chapter 2 here.
    let mut second = keepers_return();
    second.events[1].chapter = 2;
    second.events[1].at.document = 2;
    write_book(&mut conn, &second).unwrap();
    put_series(&mut conn, &keepers()).unwrap();
    let logs = [keeper(), second];

    // His link holds from chapter 1: the fold would try it at the end of that chapter, find no sailor, and
    // never try again.
    let mut links = keepers_links();
    links[1].chapter = 1;
    links[1].at = Position { document: 1, block: 0 };
    let refused = put_links(&mut conn, "keepers", "published", &links, &logs);
    let said = |why: &str| why.contains("holds from chapter 1") && why.contains("has not introduced sailor");
    assert!(matches!(&refused, Err(Error::Invalid(why)) if said(why)), "{refused:?}");
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), []);

    // From the chapter that introduces him it is stored.
    links[1].chapter = 2;
    links[1].at.document = 2;
    put_links(&mut conn, "keepers", "published", &links, &logs).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), links);
}

#[test]
fn links_made_against_a_log_stored_again_since_are_refused() {
    let mut conn = db();
    let read = [keeper(), keepers_return(), keepers_light()];
    for log in &read {
        write_book(&mut conn, log).unwrap();
    }
    let (first, second) = books();
    let third = keepers_light().book.sha256;
    let three = Series { orders: vec![order("published", &[&first, &second, &third])], ..keepers() };
    put_series(&mut conn, &three).unwrap();
    put_links(&mut conn, "keepers", "published", &keepers_links(), &read).unwrap();
    // Made against the three logs: the third book's captain is the second's sailor.
    let mut links = keepers_links();
    links.push(Link {
        later: key(&third, "captain"),
        earlier: key(&second, "sailor"),
        method: LinkMethod::Name,
        chapter: 1,
        at: Position { document: 1, block: 0 },
    });

    // The third book is extracted again while the links are made.
    let mut again = keepers_light();
    again.book.extracted_at = "2026-03-01T00:00:00Z".into();
    write_book(&mut conn, &again).unwrap();
    let replaced = |refused: &Result<(), Error>| matches!(refused, Err(Error::LogReplaced { book }) if *book == third);
    let refused = put_links(&mut conn, "keepers", "published", &links, &read);
    assert!(replaced(&refused), "{refused:?}");
    // Nothing is stored, and the links between the other two books are as they were.
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
    // A log no link names counts as well: the step that read it found none of its characters to link.
    let refused = put_links(&mut conn, "keepers", "published", &keepers_links()[..1], &read);
    assert!(replaced(&refused), "{refused:?}");
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());

    // Made against the logs stored now, they are stored.
    put_links(&mut conn, "keepers", "published", &links, &[keeper(), keepers_return(), again]).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), links);
}

#[test]
fn links_made_over_an_order_stored_again_since_are_refused() {
    let mut conn = db();
    for log in [keeper(), keepers_return(), keepers_light()] {
        write_book(&mut conn, &log).unwrap();
    }
    let (first, second) = books();
    let third = keepers_light().book.sha256;
    let stored_as = |books: &[&str]| Series { orders: vec![order("published", books)], ..keepers() };
    put_series(&mut conn, &stored_as(&[&first, &third])).unwrap();
    // Made over the first and the third book: the third book's captain is the first's Tom Reed.
    let read = [keeper(), keepers_light()];
    let links = [Link {
        later: key(&third, "captain"),
        earlier: key(&first, "tom"),
        method: LinkMethod::Name,
        chapter: 1,
        at: Position { document: 1, block: 0 },
    }];
    put_links(&mut conn, "keepers", "published", &links, &read).unwrap();

    // The order is stored again with the second book between the two while the links are made, so they never
    // weighed the second book's characters.
    put_series(&mut conn, &stored_as(&[&first, &second, &third])).unwrap();
    let replaced = |refused: &Result<(), Error>| {
        let named = |series: &str, order: &str| (series, order) == ("keepers", "published");
        matches!(refused, Err(Error::OrderReplaced { series, order }) if named(series, order))
    };
    let refused = put_links(&mut conn, "keepers", "published", &links, &read);
    assert!(replaced(&refused), "{refused:?}");
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), []);

    // The same books in another sequence are another order as well.
    put_series(&mut conn, &stored_as(&[&third, &first])).unwrap();
    let refused = put_links(&mut conn, "keepers", "published", &[], &read);
    assert!(replaced(&refused), "{refused:?}");
}

#[test]
fn a_link_to_a_book_whose_log_is_not_given_is_invalid() {
    let mut conn = library();
    let (_, second) = books();
    let refused = put_links(&mut conn, "keepers", "published", &keepers_links()[..1], &[keeper()]);
    let said = |why: &str| why.contains(&format!("ada of book {second}"));
    assert!(matches!(&refused, Err(Error::Invalid(why)) if said(why)), "{refused:?}");
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn two_links_for_one_later_character_are_invalid() {
    let mut conn = library();
    let (first, second) = books();
    // The second book's Ada is the first's Ada already, and here the first's Tom as well.
    let twice = Link { earlier: key(&first, "tom"), ..keepers_links()[0].clone() };
    let mut links = keepers_links();
    links.push(twice);

    let refused = put_links(&mut conn, "keepers", "published", &links, &keepers_logs());
    assert!(
        matches!(&refused, Err(Error::Invalid(why)) if why.contains(&format!("two links for ada of book {second}"))),
        "{refused:?}"
    );
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn put_links_replaces_one_orders_links() {
    let mut conn = library();
    let (first, second) = books();
    put_series(&mut conn, &keepers_and(&[order("reissued", &[&first, &second])])).unwrap();
    for name in ["published", "reissued"] {
        put_links(&mut conn, "keepers", name, &keepers_links(), &keepers_logs()).unwrap();
    }
    put_links(&mut conn, "keepers", "published", &keepers_links()[..1], &keepers_logs()).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links()[..1]);
    assert_eq!(read_links(&conn, "keepers", "reissued").unwrap(), keepers_links());
}

#[test]
fn put_series_refuses_unknown_book() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    let (_, second) = books();
    not_found(put_series(&mut conn, &keepers()), &format!("book {second}"));
    not_found(read_series(&conn, "keepers"), "series keepers");
}

#[test]
fn put_series_replaces_the_orders_and_drops_their_links() {
    let mut conn = library();
    let (first, second) = books();
    let revised = Series {
        name: "keepers".into(),
        title: "The Keepers, collected".into(),
        orders: vec![order("solo", &[&second]), order("published", &[&first, &second])],
    };
    put_series(&mut conn, &revised).unwrap();
    assert_eq!(read_series(&conn, "keepers").unwrap(), revised);
    // The order kept its name and its books, and its links went all the same: they were made against the
    // series as it was.
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), []);
}

#[test]
fn put_series_says_how_many_links_it_drops() {
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &keepers_return()).unwrap();
    let (first, second) = books();
    let both = keepers_and(&[order("reissued", &[&first, &second])]);
    assert_eq!(put_series(&mut conn, &both).unwrap(), 0, "a series stored for the first time replaces none");
    // Another series' links are not the keepers', and stay.
    let lighthouse = Series { name: "lighthouse".into(), ..keepers() };
    put_series(&mut conn, &lighthouse).unwrap();
    put_links(&mut conn, "lighthouse", "published", &keepers_links(), &keepers_logs()).unwrap();
    put_links(&mut conn, "keepers", "published", &keepers_links(), &keepers_logs()).unwrap();
    put_links(&mut conn, "keepers", "reissued", &keepers_links()[..1], &keepers_logs()).unwrap();

    assert_eq!(put_series(&mut conn, &both).unwrap(), 3, "the links of each of its orders");
    assert_eq!(put_series(&mut conn, &both).unwrap(), 0, "the links went with the series it replaced");
    assert_eq!(read_links(&conn, "lighthouse", "published").unwrap(), keepers_links());
}

#[test]
fn series_without_an_order_or_with_a_book_twice_is_invalid() {
    let mut conn = library();
    let (first, second) = books();
    let no_order = Series { orders: Vec::new(), ..keepers() };
    let twice = Series { orders: vec![order("published", &[&first, &second, &first])], ..keepers() };
    for refused in [no_order, twice] {
        assert!(matches!(put_series(&mut conn, &refused), Err(Error::Invalid(_))), "{refused:?}");
    }
    assert_eq!(read_series(&conn, "keepers").unwrap(), keepers());
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn two_orders_of_one_name_are_invalid() {
    let mut conn = library();
    let (_, second) = books();
    let refused = put_series(&mut conn, &keepers_and(&[order("published", &[&second])]));
    assert!(matches!(&refused, Err(Error::Invalid(why)) if why.contains("two orders named published")), "{refused:?}");
    assert_eq!(read_series(&conn, "keepers").unwrap(), keepers());
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn series_writes_require_foreign_keys() {
    let mut conn = library();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    assert!(matches!(put_series(&mut conn, &keepers()), Err(Error::ForeignKeysOff)));
    assert!(matches!(put_links(&mut conn, "keepers", "published", &[], &[]), Err(Error::ForeignKeysOff)));
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), keepers_links());
}

#[test]
fn default_order_is_rank_zero() {
    let mut conn = library();
    let (first, second) = books();
    put_series(&mut conn, &keepers_and(&[order("solo", &[&second])])).unwrap();
    put_links(&mut conn, "keepers", "published", &keepers_links(), &keepers_logs()).unwrap();

    let default = cast_at(&conn, &second, "back.xhtml", &in_series(None)).unwrap();
    assert_eq!(default, cast_at(&conn, &second, "back.xhtml", &in_series(Some("published"))).unwrap());
    assert_eq!(keys(&default), [&key(&first, "ada"), &key(&first, "pip"), &key(&first, "tom")]);
    assert_eq!(
        cast_at(&conn, &second, "back.xhtml", &in_series(Some("solo"))).unwrap(),
        fold_book(&keepers_return(), 3)
    );
}

#[test]
fn series_scope_needs_the_book_in_the_order() {
    let mut conn = library();
    let (first, second) = books();
    put_series(&mut conn, &keepers_and(&[order("solo", &[&second])])).unwrap();
    not_found(
        cast_at(&conn, &first, "c1.xhtml", &in_series(Some("solo"))),
        &format!("book {first} in order solo of series keepers"),
    );
    not_found(
        cast_at(&conn, &first, "c1.xhtml", &in_series(Some("chronological"))),
        "order chronological of series keepers",
    );
    let elsewhere = Scope::Series { series: "lamplighters".into(), order: None };
    not_found(cast_at(&conn, &first, "c1.xhtml", &elsewhere), "series lamplighters");
}

#[test]
fn fold_series_is_cast_at_by_chapter() {
    let conn = library();
    let (_, second) = books();
    let logs = [keeper(), keepers_return()];
    for finished in 0..=2 {
        let document = format!("c{}.xhtml", finished + 1);
        assert_eq!(
            fold_series(&logs, Some(finished), &keepers_links()),
            cast_at(&conn, &second, &document, &in_series(None)).unwrap(),
            "{document}"
        );
    }
    assert_eq!(
        fold_series(&logs, None, &keepers_links()),
        cast_at(&conn, &second, "back.xhtml", &in_series(None)).unwrap()
    );
}

#[test]
fn fold_series_of_one_book_is_fold_book() {
    for finished in 0..=3 {
        assert_eq!(fold_series(&[keeper()], Some(finished), &[]), fold_book(&keeper(), finished), "{finished}");
    }
}

#[test]
fn fold_series_counts_a_chapter_that_ends_the_spine() {
    // The second book ends on its last chapter, so no document of it is past that chapter.
    let mut second = keepers_return();
    second.documents.retain(|document| document.path != "back.xhtml");
    let mut conn = db();
    write_book(&mut conn, &keeper()).unwrap();
    write_book(&mut conn, &second).unwrap();
    put_series(&mut conn, &keepers()).unwrap();
    put_links(&mut conn, "keepers", "published", &keepers_links(), &[keeper(), second.clone()]).unwrap();
    let (first, _) = books();
    let (tom, sailor) = (key(&first, "tom"), key(&second.book.sha256, "sailor"));

    for document in &second.documents {
        let cast = cast_at(&conn, &second.book.sha256, &document.path, &in_series(None)).unwrap();
        assert!(!character(&cast, &tom).merged.contains(&sailor), "{}", document.path);
    }

    let cast = fold_series(&[keeper(), second], Some(3), &keepers_links());
    assert_eq!(keys(&cast), [&key(&first, "ada"), &key(&first, "pip"), &tom]);
    assert_eq!(character(&cast, &tom).merged, [key(&first, "stranger"), sailor]);
}

#[test]
fn fold_series_passes_over_links_of_books_it_is_not_given() {
    assert_eq!(fold_series(&[keeper()], None, &keepers_links()), fold_book(&keeper(), 3));
}
