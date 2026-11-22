//! The fold of a book's events up to a finished chapter, on the invented books, with no database.

mod common;

use common::{keeper, keepers_light, keepers_return};
use haiyaku::fold::{Cast, ChapterRef, Character, EntityKey, Fact, Folder, Relation, fold_book};
use haiyaku::log::{BookLog, Event, EventBody, Name, Position, RelationKind};

fn key(log: &BookLog, id: &str) -> EntityKey {
    EntityKey { book: log.book.sha256.clone(), id: id.into() }
}

fn chapter(log: &BookLog, chapter: u32) -> ChapterRef {
    ChapterRef { book: log.book.sha256.clone(), chapter }
}

fn ids(cast: &Cast) -> Vec<&str> {
    cast.characters.iter().map(|character| character.key.id.as_str()).collect()
}

fn character<'a>(cast: &'a Cast, key: &EntityKey) -> &'a Character {
    cast.characters.iter().find(|character| character.key == *key).unwrap_or_else(|| panic!("no {key:?} in the cast"))
}

/// The relation of the two, given in the order the cast holds a pair.
fn relation<'a>(cast: &'a Cast, a: &EntityKey, b: &EntityKey) -> &'a Relation {
    cast.relations
        .iter()
        .find(|relation| relation.a == *a && relation.b == *b)
        .unwrap_or_else(|| panic!("no pair {a:?}, {b:?}"))
}

/// keeper() with these events for its own: the fold reads a log's events and the hash of its book, nothing else.
fn keeper_with(bodies: Vec<(u32, EventBody)>) -> BookLog {
    with_events(keeper(), bodies)
}

/// `book` with these events, each given with its chapter, for its own.
fn with_events(book: BookLog, bodies: Vec<(u32, EventBody)>) -> BookLog {
    let events = bodies.into_iter().zip(1..).map(|((chapter, body), seq)| Event {
        seq,
        chapter,
        at: Position { document: chapter, block: seq },
        quote: format!("quote {seq}"),
        body,
    });
    BookLog { events: events.collect(), ..book }
}

/// The cast of `earlier` read whole and then `later`, with the link that makes `other` of `later` one with
/// `survivor` of `earlier` applied when chapter `linked` of `later` is in, as a cast at a position applies it.
fn fold_linked(earlier: &BookLog, later: &BookLog, survivor: &str, other: &str, linked: u32) -> Cast {
    let mut folder = Folder::new();
    folder.start_book(&earlier.book.sha256);
    earlier.events.iter().for_each(|event| folder.apply(event));
    folder.start_book(&later.book.sha256);
    let (before, after): (Vec<_>, Vec<_>) = later.events.iter().partition(|event| event.chapter <= linked);
    before.into_iter().for_each(|event| folder.apply(event));
    folder.merge(&key(earlier, survivor), &key(later, other));
    after.into_iter().for_each(|event| folder.apply(event));
    folder.finish()
}

fn described(text: &str) -> Name {
    Name { text: text.into(), given: None, surname: None, title: None, title_sex: None }
}

/// A name with as many of a given name and a surname as are passed: what its rank counts.
fn proper(text: &str, given: Option<&str>, surname: Option<&str>) -> Name {
    Name { given: given.map(Into::into), surname: surname.map(Into::into), ..described(text) }
}

fn introduced(entity: &str, text: &str) -> EventBody {
    EventBody::Introduced { entity: entity.into(), name: described(text) }
}

fn same_as(entity: &str, other: &str) -> EventBody {
    EventBody::SameAs { entity: entity.into(), other: other.into() }
}

fn acquainted(from: &str, to: &str, note: &str) -> EventBody {
    EventBody::Relation { from: from.into(), to: to.into(), kind: RelationKind::Acquaintance, note: note.into() }
}

#[test]
fn nothing_before_chapter_one() {
    let cast = fold_book(&keeper(), 0);
    assert!(cast.characters.is_empty());
    assert!(cast.relations.is_empty());
}

#[test]
fn description_until_named() {
    let k = keeper();
    let ada = key(&k, "ada");

    let cast = fold_book(&k, 1);
    assert_eq!(character(&cast, &ada).name, "the keeper");
    assert!(character(&cast, &ada).aliases.is_empty());

    let cast = fold_book(&k, 2);
    assert_eq!(character(&cast, &ada).name, "Ada Marsh");
    assert_eq!(character(&cast, &ada).aliases, ["the keeper"]);
}

#[test]
fn owns_is_directed() {
    let k = keeper();
    let (ada, pip) = (key(&k, "ada"), key(&k, "pip"));
    let cast = fold_book(&k, 1);
    assert_eq!(
        relation(&cast, &ada, &pip).facts,
        [Fact {
            kind: RelationKind::Owns,
            from: Some(ada.clone()),
            note: "her gull".into(),
            quote: "The keeper fed Pip, her gull, from her own plate.".into(),
            first: chapter(&k, 1),
            last: chapter(&k, 1),
        }]
    );
}

#[test]
fn stranger_separate_until_revealed() {
    let k = keeper();
    let cast = fold_book(&k, 2);
    assert_eq!(ids(&cast), ["ada", "pip", "stranger"]);
    assert_eq!(character(&cast, &key(&k, "stranger")).name, "the stranger");

    let facts = &relation(&cast, &key(&k, "ada"), &key(&k, "stranger")).facts;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].kind, RelationKind::Acquaintance);
    assert_eq!(facts[0].from, None);
    assert_eq!(cast.relations.len(), 2);
}

#[test]
fn same_as_merges() {
    let k = keeper();
    let (ada, tom) = (key(&k, "ada"), key(&k, "tom"));
    let cast = fold_book(&k, 3);
    assert_eq!(ids(&cast), ["ada", "pip", "tom"]);
    assert_eq!(
        *character(&cast, &tom),
        Character {
            key: tom.clone(),
            name: "Tom Reed".into(),
            aliases: vec!["the stranger".into()],
            first: chapter(&k, 2),
            last: chapter(&k, 3),
            merged: vec![key(&k, "stranger")],
        }
    );

    assert_eq!(
        relation(&cast, &ada, &tom).facts,
        [Fact {
            kind: RelationKind::Acquaintance,
            from: None,
            note: "met on the jetty".into(),
            quote: "Ada nodded to the stranger, and he nodded back.".into(),
            first: chapter(&k, 2),
            last: chapter(&k, 3),
        }]
    );
    assert_eq!(cast.relations.len(), 2);
}

#[test]
fn alias_does_not_rename() {
    let k = keeper();
    let cast = fold_book(&k, 3);
    let ada = character(&cast, &key(&k, "ada"));
    assert_eq!(ada.name, "Ada Marsh");
    assert_eq!(ada.aliases, ["the keeper", "Mrs. Marsh"]);
}

#[test]
fn a_name_of_equal_rank_does_not_replace_the_first() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (2, EventBody::Named { entity: "ada".into(), name: described("the lamp woman") }),
    ]);
    let cast = fold_book(&log, 2);
    let ada = character(&cast, &key(&log, "ada"));
    assert_eq!(ada.name, "the keeper");
    assert_eq!(ada.aliases, ["the lamp woman"]);
}

#[test]
fn the_cast_is_in_the_order_the_reader_met_it() {
    let log = keeper_with(vec![
        (1, introduced("pip", "the gull")),
        (1, introduced("tom", "the boatman")),
        (1, acquainted("tom", "pip", "throws him scraps")),
        (2, introduced("ada", "the keeper")),
        (2, acquainted("ada", "pip", "shares her plate")),
    ]);
    let cast = fold_book(&log, 2);
    assert_eq!(ids(&cast), ["pip", "tom", "ada"]);
    let pairs: Vec<_> = cast.relations.iter().map(|pair| (pair.a.id.as_str(), pair.b.id.as_str())).collect();
    assert_eq!(pairs, [("pip", "tom"), ("ada", "pip")]);
}

#[test]
fn merge_drops_self_pair() {
    let log = keeper_with(vec![
        (1, introduced("stranger", "the stranger")),
        (1, introduced("tom", "the boatman")),
        (
            1,
            EventBody::Relation {
                from: "stranger".into(),
                to: "tom".into(),
                kind: RelationKind::Rival,
                note: "said to hate each other".into(),
            },
        ),
        (2, same_as("tom", "stranger")),
    ]);
    assert_eq!(fold_book(&log, 1).relations.len(), 1);

    let cast = fold_book(&log, 2);
    assert_eq!(ids(&cast), ["tom"]);
    assert!(cast.relations.is_empty());
}

#[test]
fn a_merge_keeps_the_earlier_statement_of_a_fact() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("stranger", "the stranger")),
        (1, acquainted("ada", "stranger", "met on the jetty")),
        (2, introduced("tom", "the boatman")),
        (
            2,
            EventBody::Relation {
                from: "tom".into(),
                to: "ada".into(),
                kind: RelationKind::Ally,
                note: "mend the lamp together".into(),
            },
        ),
        (2, acquainted("tom", "ada", "old friends")),
        (3, same_as("tom", "stranger")),
    ]);
    let cast = fold_book(&log, 3);
    assert_eq!(
        relation(&cast, &key(&log, "ada"), &key(&log, "tom")).facts,
        [
            Fact {
                kind: RelationKind::Acquaintance,
                from: None,
                note: "met on the jetty".into(),
                quote: "quote 3".into(),
                first: chapter(&log, 1),
                last: chapter(&log, 2),
            },
            Fact {
                kind: RelationKind::Ally,
                from: None,
                note: "mend the lamp together".into(),
                quote: "quote 5".into(),
                first: chapter(&log, 2),
                last: chapter(&log, 2),
            },
        ]
    );
}

#[test]
fn an_entity_never_introduced_is_passed_over() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (2, acquainted("ada", "ghost", "will not speak of it")),
        (3, same_as("ghost", "ada")),
        (3, EventBody::Appeared { entity: "ghost".into() }),
    ]);
    let cast = fold_book(&log, 3);
    assert_eq!(ids(&cast), ["ada"]);
    assert!(character(&cast, &key(&log, "ada")).merged.is_empty());
    assert_eq!(character(&cast, &key(&log, "ada")).last, chapter(&log, 3));
    assert!(cast.relations.is_empty());
}

#[test]
fn an_id_merged_twice_over_reaches_the_last_survivor() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("figure", "the figure")),
        (
            1,
            EventBody::Relation {
                from: "figure".into(),
                to: "ada".into(),
                kind: RelationKind::Serves,
                note: "rows her mail over".into(),
            },
        ),
        (2, introduced("stranger", "the stranger")),
        (2, same_as("stranger", "figure")),
        (3, introduced("tom", "the boatman")),
        (3, same_as("tom", "stranger")),
        (3, EventBody::Alias { entity: "figure".into(), name: described("Old Tom") }),
    ]);
    let (ada, tom) = (key(&log, "ada"), key(&log, "tom"));
    let cast = fold_book(&log, 3);

    // Tom sorts by the chapter the reader first met him in, as the figure.
    assert_eq!(ids(&cast), ["ada", "tom"]);
    let folded = character(&cast, &tom);
    assert_eq!(folded.name, "the boatman");
    assert_eq!(folded.aliases, ["the figure", "the stranger", "Old Tom"]);
    assert_eq!(folded.merged, [key(&log, "figure"), key(&log, "stranger")]);
    assert_eq!(folded.first, chapter(&log, 1));
    assert_eq!(relation(&cast, &ada, &tom).facts[0].from, Some(tom.clone()));
}

#[test]
fn aliases_are_in_the_order_first_seen_whenever_the_merge_comes() {
    let alias = |entity: &str, text: &str| EventBody::Alias { entity: entity.into(), name: described(text) };
    let tom_reed = Name { given: Some("Tom".into()), surname: Some("Reed".into()), ..described("Tom Reed") };
    let log = keeper_with(vec![
        (1, introduced("stranger", "the stranger")),
        (1, alias("stranger", "Old Tom")),
        (2, introduced("tom", "the boatman")),
        (2, alias("tom", "the ferryman")),
        (2, alias("tom", "Old Tom")),
        (3, EventBody::Named { entity: "tom".into(), name: tom_reed }),
    ]);
    let (tom, stranger) = (key(&log, "tom"), key(&log, "stranger"));
    let aliases_merging_after = |events: usize| {
        let mut folder = Folder::new();
        folder.start_book(&log.book.sha256);
        for event in &log.events {
            folder.apply(event);
            if event.seq as usize == events {
                folder.merge(&tom, &stranger);
            }
        }
        let cast = folder.finish();
        character(&cast, &tom).aliases.clone()
    };

    // The stranger's names were on the page before any of Tom's, and "Old Tom" was the stranger's first.
    let first_seen = ["the stranger", "Old Tom", "the boatman", "the ferryman"];
    assert_eq!(aliases_merging_after(3), first_seen);
    assert_eq!(aliases_merging_after(6), first_seen);
}

#[test]
fn a_relation_between_an_id_and_its_survivor_adds_no_pair() {
    let log = keeper_with(vec![
        (1, introduced("stranger", "the stranger")),
        (1, introduced("tom", "the boatman")),
        (2, same_as("tom", "stranger")),
        (3, acquainted("stranger", "tom", "seen together on the jetty")),
    ]);
    let cast = fold_book(&log, 3);
    assert_eq!(ids(&cast), ["tom"]);
    assert_eq!(character(&cast, &key(&log, "tom")).last, chapter(&log, 3));
    assert!(cast.relations.is_empty());
}

#[test]
fn a_directed_kind_holds_a_fact_each_way() {
    let serves = |from: &str, to: &str, note: &str| EventBody::Relation {
        from: from.into(),
        to: to.into(),
        kind: RelationKind::Serves,
        note: note.into(),
    };
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, serves("tom", "ada", "rows her mail over")),
        (2, serves("ada", "tom", "keeps the light for his boat")),
        (3, serves("tom", "ada", "brings her coal")),
    ]);
    let (ada, tom) = (key(&log, "ada"), key(&log, "tom"));
    let cast = fold_book(&log, 3);
    let facts = &relation(&cast, &ada, &tom).facts;
    let each_way: Vec<_> =
        facts.iter().map(|fact| (fact.from.as_ref(), fact.note.as_str(), fact.last.chapter)).collect();
    assert_eq!(each_way, [(Some(&tom), "rows her mail over", 3), (Some(&ada), "keeps the light for his boat", 2)]);
}

#[test]
fn a_link_merges_across_books() {
    let (k, r) = (keeper(), keepers_return());
    let mut folder = Folder::new();
    folder.start_book(&k.book.sha256);
    k.events.iter().for_each(|event| folder.apply(event));
    folder.start_book(&r.book.sha256);
    // One link holds from the later book's first chapter, so that book's later events name an id that no
    // longer stands for itself; the other comes after everything its character did there.
    let (met, after) = r.events.split_at(2);
    met.iter().for_each(|event| folder.apply(event));
    folder.merge(&key(&k, "tom"), &key(&r, "sailor"));
    after.iter().for_each(|event| folder.apply(event));
    folder.merge(&key(&k, "ada"), &key(&r, "ada"));
    let cast = folder.finish();

    let (ada, tom) = (key(&k, "ada"), key(&k, "tom"));
    assert_eq!(ids(&cast), ["ada", "pip", "tom"]);
    assert_eq!(
        *character(&cast, &ada),
        Character {
            key: ada.clone(),
            name: "Ada Marsh".into(),
            aliases: vec!["the keeper".into(), "Mrs. Marsh".into(), "Ada".into()],
            first: chapter(&k, 1),
            last: chapter(&r, 2),
            merged: vec![key(&r, "ada")],
        }
    );
    let folded = character(&cast, &tom);
    assert_eq!(folded.name, "Tom Reed");
    assert_eq!(folded.aliases, ["the stranger", "the old sailor", "Tom"]);
    assert_eq!(folded.merged, [key(&k, "stranger"), key(&r, "sailor")]);
    assert_eq!(folded.last, chapter(&r, 3));

    let kinds: Vec<_> = relation(&cast, &ada, &tom).facts.iter().map(|fact| (fact.kind, fact.first.clone())).collect();
    assert_eq!(kinds, [(RelationKind::Acquaintance, chapter(&k, 2)), (RelationKind::Ally, chapter(&r, 2))]);
}

#[test]
fn a_same_as_keeps_the_earlier_books_character() {
    let k = keeper();
    // Either way round: whichever side the event calls `entity`, the first book's Tom Reed is who the series
    // has known since that book.
    for (entity, other) in [("captain", "sailor"), ("sailor", "captain")] {
        let r = with_events(
            keepers_return(),
            vec![
                (1, introduced("sailor", "the old sailor")),
                (3, introduced("captain", "the captain")),
                (3, same_as(entity, other)),
            ],
        );
        // The sailor is linked to Tom Reed from the later book's first chapter.
        let cast = fold_linked(&k, &r, "tom", "sailor", 1);

        let keys: Vec<_> = cast.characters.iter().map(|character| &character.key).collect();
        assert_eq!(keys, [&key(&k, "ada"), &key(&k, "pip"), &key(&k, "tom")], "{entity} is {other}");
        let tom = character(&cast, &key(&k, "tom"));
        assert_eq!(tom.name, "Tom Reed");
        assert_eq!(tom.aliases, ["the stranger", "the old sailor", "the captain"]);
        assert_eq!(tom.merged, [key(&k, "stranger"), key(&r, "sailor"), key(&r, "captain")]);
        assert_eq!(tom.last, chapter(&r, 3));
    }
}

#[test]
fn a_fuller_name_survives_a_link_whichever_chapter_gives_it() {
    // The earlier book never learns who the stranger is.
    let k = keeper_with(vec![(1, introduced("stranger", "the stranger"))]);
    let tom_reed = proper("Tom Reed", Some("Tom"), Some("Reed"));
    // The link holds from the later book's first chapter. The text names the sailor in that chapter, so
    // before the link is applied, or in the next, so after it.
    for named_in in [1, 2] {
        let r = with_events(
            keepers_return(),
            vec![
                (1, introduced("sailor", "the old sailor")),
                (named_in, EventBody::Named { entity: "sailor".into(), name: tom_reed.clone() }),
            ],
        );
        let cast = fold_linked(&k, &r, "stranger", "sailor", 1);

        assert_eq!(cast.characters.len(), 1);
        let stranger = character(&cast, &key(&k, "stranger"));
        assert_eq!(stranger.name, "Tom Reed", "named in chapter {named_in}");
        assert_eq!(stranger.aliases, ["the stranger", "the old sailor"], "named in chapter {named_in}");
    }
}

#[test]
fn a_survivor_takes_a_merged_characters_name_only_when_it_is_fuller() {
    let named = |text: &str, given: Option<&str>, surname: Option<&str>| EventBody::Introduced {
        entity: "stranger".into(),
        name: proper(text, given, surname),
    };
    let survivor = EventBody::Introduced { entity: "tom".into(), name: proper("Tom", Some("Tom"), None) };
    let folded = |merged: EventBody| {
        let log = keeper_with(vec![(1, merged), (2, survivor.clone()), (3, same_as("tom", "stranger"))]);
        let cast = fold_book(&log, 3);
        let tom = character(&cast, &key(&log, "tom"));
        (tom.name.clone(), tom.aliases.clone())
    };

    assert_eq!(
        folded(named("Thomas Reed", Some("Thomas"), Some("Reed"))),
        ("Thomas Reed".into(), vec!["Tom".to_owned()])
    );
    // A name as full as its own does not replace it, and neither does a lesser one.
    assert_eq!(folded(named("Mr. Reed", None, Some("Reed"))), ("Tom".into(), vec!["Mr. Reed".to_owned()]));
    assert_eq!(folded(named("the stranger", None, None)), ("Tom".into(), vec!["the stranger".to_owned()]));
}

/// Three books read one after the other: the first has Tom Reed, the second an old sailor, and the third a
/// captain and a mate who are one from its first chapter, the captain merged into the mate.
fn three_books() -> [BookLog; 3] {
    let second = with_events(keepers_return(), vec![(1, introduced("sailor", "the old sailor"))]);
    let third = with_events(
        keepers_light(),
        vec![
            (1, introduced("captain", "the captain")),
            (1, introduced("mate", "the mate")),
            (1, same_as("mate", "captain")),
        ],
    );
    [keeper(), second, third]
}

/// The cast of `books` read whole, with `links` applied in that order once the last book is in: each joins
/// its second character to its first, as a link joins the later to the earlier.
fn fold_then_link(books: &[BookLog; 3], links: &[(&EntityKey, &EntityKey)]) -> Cast {
    let mut folder = Folder::new();
    for book in books {
        folder.start_book(&book.book.sha256);
        book.events.iter().for_each(|event| folder.apply(event));
    }
    for (earlier, later) in links {
        folder.merge(earlier, later);
    }
    folder.finish()
}

#[test]
fn a_link_keeps_the_earliest_books_character_when_its_later_side_already_stands_for_it() {
    let books = three_books();
    let [first, second, third] = &books;
    let (tom, sailor) = (key(first, "tom"), key(second, "sailor"));
    let (captain, mate) = (key(third, "captain"), key(third, "mate"));

    // The mate is linked to the first book's Tom Reed, so the captain stands for him too by the time the
    // captain's own link, to the second book's sailor, is applied.
    let cast = fold_then_link(&books, &[(&tom, &mate), (&sailor, &captain)]);
    let keys: Vec<_> = cast.characters.iter().map(|character| &character.key).collect();
    assert_eq!(keys, [&key(first, "ada"), &key(first, "pip"), &tom]);
    let tom_reed = character(&cast, &tom);
    assert_eq!(tom_reed.name, "Tom Reed");
    assert_eq!(tom_reed.aliases, ["the stranger", "the old sailor", "the captain", "the mate"]);
    assert_eq!(tom_reed.merged, [key(first, "stranger"), captain, mate, sailor]);
}

#[test]
fn links_due_together_give_one_cast_in_either_order() {
    let books = three_books();
    let [first, second, third] = &books;
    let to_tom = (&key(first, "tom"), &key(third, "mate"));
    let to_sailor = (&key(second, "sailor"), &key(third, "captain"));
    assert_eq!(fold_then_link(&books, &[to_tom, to_sailor]), fold_then_link(&books, &[to_sailor, to_tom]));
}
