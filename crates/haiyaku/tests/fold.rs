//! The fold of a book's events up to a finished chapter, on the invented books, with no database.

mod common;

use common::{keeper, keepers_light, keepers_return, texts};
use haiyaku::fold::{Cast, ChapterRef, Character, EntityKey, Fact, Folder, Relation, fold_book};
use haiyaku::log::{BookLog, Event, EventBody, Name, Position, RelationKind, Sex};

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

/// The cast of `earlier` read whole and then `later`, with the links, each of which makes its `other` of `later`
/// one with its `survivor` of `earlier`, applied in turn when chapter `linked` of `later` is in, as a cast at a
/// position applies them.
fn fold_linked(earlier: &BookLog, later: &BookLog, links: &[(&str, &str)], linked: u32) -> Cast {
    let mut folder = Folder::new();
    folder.start_book(&earlier.book.sha256);
    earlier.events.iter().for_each(|event| folder.apply(event));
    folder.start_book(&later.book.sha256);
    let (before, after): (Vec<_>, Vec<_>) = later.events.iter().partition(|event| event.chapter <= linked);
    before.into_iter().for_each(|event| folder.apply(event));
    for (survivor, other) in links {
        folder.merge(&key(earlier, survivor), &key(later, other));
    }
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

fn related(kind: RelationKind, from: &str, to: &str, note: &str) -> EventBody {
    EventBody::Relation { from: from.into(), to: to.into(), kind, note: note.into() }
}

fn overturned(kind: RelationKind, from: &str, to: &str, note: &str) -> EventBody {
    EventBody::Overturned { from: from.into(), to: to.into(), kind, note: note.into() }
}

/// `log` as far as a reader who has finished chapter `finished` has read it.
fn up_to(log: &BookLog, finished: u32) -> BookLog {
    BookLog { events: log.events.iter().filter(|event| event.chapter <= finished).cloned().collect(), ..log.clone() }
}

/// Each fact of the pair as its kind, its first and last chapters, and the chapter of the overturn that marks it.
fn spans(cast: &Cast, a: &EntityKey, b: &EntityKey) -> Vec<(RelationKind, ChapterRef, ChapterRef, Option<ChapterRef>)> {
    let span = |fact: &Fact| {
        let at = fact.overturned.as_ref().map(|overturn| overturn.at.clone());
        (fact.kind, fact.first.clone(), fact.last.clone(), at)
    };
    relation(cast, a, b).facts.iter().map(span).collect()
}

/// A fact's fields in the order `Fact` declares them, its overturn's as a tuple of their own.
type Fields =
    (RelationKind, Option<EntityKey>, String, String, ChapterRef, ChapterRef, Option<(ChapterRef, String, String)>);

/// Each fact of the pair as its [`Fields`].
fn facts(cast: &Cast, a: &EntityKey, b: &EntityKey) -> Vec<Fields> {
    let fields = |fact: &Fact| {
        let overturned = fact
            .overturned
            .as_ref()
            .map(|overturn| (overturn.at.clone(), overturn.note.clone(), overturn.quote.clone()));
        (
            fact.kind,
            fact.from.clone(),
            fact.note.clone(),
            fact.quote.clone(),
            fact.first.clone(),
            fact.last.clone(),
            overturned,
        )
    };
    relation(cast, a, b).facts.iter().map(fields).collect()
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
    assert_eq!(character(&cast, &ada).name.text, "the keeper");
    assert!(character(&cast, &ada).aliases.is_empty());

    let cast = fold_book(&k, 2);
    assert_eq!(character(&cast, &ada).name.text, "Ada Marsh");
    assert_eq!(texts(&character(&cast, &ada).aliases), ["the keeper"]);
}

#[test]
fn owns_is_directed() {
    let k = keeper();
    let (ada, pip) = (key(&k, "ada"), key(&k, "pip"));
    let cast = fold_book(&k, 1);
    assert_eq!(
        facts(&cast, &ada, &pip),
        [(
            RelationKind::Owns,
            Some(ada.clone()),
            "her gull".into(),
            "The keeper fed Pip, her gull, from her own plate.".into(),
            chapter(&k, 1),
            chapter(&k, 1),
            None,
        )]
    );
}

#[test]
fn stranger_separate_until_revealed() {
    let k = keeper();
    let cast = fold_book(&k, 2);
    assert_eq!(ids(&cast), ["ada", "pip", "stranger"]);
    assert_eq!(character(&cast, &key(&k, "stranger")).name.text, "the stranger");

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
    let tom_reed = character(&cast, &tom);
    assert_eq!(tom_reed.key, tom);
    assert_eq!(tom_reed.name.text, "Tom Reed");
    assert_eq!(texts(&tom_reed.aliases), ["the stranger"]);
    assert_eq!(tom_reed.first, chapter(&k, 2));
    assert_eq!(tom_reed.last, chapter(&k, 3));
    assert_eq!(tom_reed.merged, [key(&k, "stranger")]);

    assert_eq!(
        facts(&cast, &ada, &tom),
        [(
            RelationKind::Acquaintance,
            None,
            "met on the jetty".into(),
            "Ada nodded to the stranger, and he nodded back.".into(),
            chapter(&k, 2),
            chapter(&k, 3),
            None,
        )]
    );
    assert_eq!(cast.relations.len(), 2);
}

#[test]
fn alias_does_not_rename() {
    let k = keeper();
    let cast = fold_book(&k, 3);
    let ada = character(&cast, &key(&k, "ada"));
    assert_eq!(ada.name.text, "Ada Marsh");
    assert_eq!(texts(&ada.aliases), ["the keeper", "Mrs. Marsh"]);
}

#[test]
fn the_shown_name_keeps_its_labels() {
    let k = keeper();
    let ada = key(&k, "ada");
    assert_eq!(character(&fold_book(&k, 2), &ada).name, proper("Ada Marsh", Some("Ada"), Some("Marsh")));
    let mrs_marsh =
        Name { title: Some("Mrs.".into()), title_sex: Some(Sex::Female), ..proper("Mrs. Marsh", None, Some("Marsh")) };
    assert_eq!(character(&fold_book(&k, 3), &ada).aliases, [described("the keeper"), mrs_marsh]);
}

#[test]
fn a_name_of_equal_rank_does_not_replace_the_first() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (2, EventBody::Named { entity: "ada".into(), name: described("the lamp woman") }),
    ]);
    let cast = fold_book(&log, 2);
    let ada = character(&cast, &key(&log, "ada"));
    assert_eq!(ada.name.text, "the keeper");
    assert_eq!(texts(&ada.aliases), ["the lamp woman"]);
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
        facts(&cast, &key(&log, "ada"), &key(&log, "tom")),
        [
            (
                RelationKind::Acquaintance,
                None,
                "met on the jetty".into(),
                "quote 3".into(),
                chapter(&log, 1),
                chapter(&log, 2),
                None,
            ),
            (
                RelationKind::Ally,
                None,
                "mend the lamp together".into(),
                "quote 5".into(),
                chapter(&log, 2),
                chapter(&log, 2),
                None,
            ),
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
    assert_eq!(folded.name.text, "the boatman");
    assert_eq!(texts(&folded.aliases), ["the figure", "the stranger", "Old Tom"]);
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
        character(&cast, &tom).aliases.iter().map(|alias| alias.text.clone()).collect::<Vec<_>>()
    };

    // The stranger's names were on the page before any of Tom's, and "Old Tom" was the stranger's first.
    let first_seen = ["the stranger", "Old Tom", "the boatman", "the ferryman"];
    assert_eq!(aliases_merging_after(3), first_seen);
    assert_eq!(aliases_merging_after(6), first_seen);
}

#[test]
fn of_one_text_the_first_labelled_name_is_kept_whenever_the_merge_comes() {
    let alias = |entity: &str, name: Name| EventBody::Alias { entity: entity.into(), name };
    let mrs_marsh =
        Name { title: Some("Mrs.".into()), title_sex: Some(Sex::Female), ..proper("Mrs. Marsh", None, Some("Marsh")) };
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, alias("ada", described("Marsh"))),
        (2, EventBody::Introduced { entity: "widow".into(), name: mrs_marsh.clone() }),
        (2, alias("widow", proper("Marsh", None, Some("Marsh")))),
        (3, same_as("ada", "widow")),
        (3, alias("ada", proper("Marsh", Some("Marsh"), None))),
    ]);
    let (ada, widow) = (key(&log, "ada"), key(&log, "widow"));
    // Merged after every other event, Ada already holds her own labelled "Marsh" when the widow's earlier one
    // arrives; merged by the same-as, she holds the widow's when her own arrives.
    let merged_last = {
        let mut folder = Folder::new();
        folder.start_book(&log.book.sha256);
        let unmerged = log.events.iter().filter(|event| !matches!(event.body, EventBody::SameAs { .. }));
        unmerged.for_each(|event| folder.apply(event));
        folder.merge(&ada, &widow);
        folder.finish()
    };

    for cast in [fold_book(&log, 3), merged_last] {
        let folded = character(&cast, &ada);
        assert_eq!(folded.name, mrs_marsh);
        assert_eq!(folded.aliases, [described("the keeper"), proper("Marsh", None, Some("Marsh"))]);
    }
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
    let ada_marsh = character(&cast, &ada);
    assert_eq!(ada_marsh.key, ada);
    assert_eq!(ada_marsh.name.text, "Ada Marsh");
    assert_eq!(texts(&ada_marsh.aliases), ["the keeper", "Mrs. Marsh", "Ada"]);
    assert_eq!(ada_marsh.first, chapter(&k, 1));
    assert_eq!(ada_marsh.last, chapter(&r, 2));
    assert_eq!(ada_marsh.merged, [key(&r, "ada")]);
    let folded = character(&cast, &tom);
    assert_eq!(folded.name.text, "Tom Reed");
    assert_eq!(texts(&folded.aliases), ["the stranger", "the old sailor", "Tom"]);
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
        let cast = fold_linked(&k, &r, &[("tom", "sailor")], 1);

        let keys: Vec<_> = cast.characters.iter().map(|character| &character.key).collect();
        assert_eq!(keys, [&key(&k, "ada"), &key(&k, "pip"), &key(&k, "tom")], "{entity} is {other}");
        let tom = character(&cast, &key(&k, "tom"));
        assert_eq!(tom.name.text, "Tom Reed");
        assert_eq!(texts(&tom.aliases), ["the stranger", "the old sailor", "the captain"]);
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
        let cast = fold_linked(&k, &r, &[("stranger", "sailor")], 1);

        assert_eq!(cast.characters.len(), 1);
        let stranger = character(&cast, &key(&k, "stranger"));
        assert_eq!(stranger.name.text, "Tom Reed", "named in chapter {named_in}");
        assert_eq!(texts(&stranger.aliases), ["the stranger", "the old sailor"], "named in chapter {named_in}");
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
        (tom.name.text.clone(), tom.aliases.iter().map(|alias| alias.text.clone()).collect::<Vec<_>>())
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
    assert_eq!(tom_reed.name.text, "Tom Reed");
    assert_eq!(texts(&tom_reed.aliases), ["the stranger", "the old sailor", "the captain", "the mate"]);
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

#[test]
fn overturn_marks_the_fact_stated_before_it() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, related(RelationKind::Enemy, "ada", "tom", "quarrel over the light")),
        (3, overturned(RelationKind::Enemy, "tom", "ada", "the quarrel was staged for the harbourmaster")),
    ]);
    let (ada, tom) = (key(&log, "ada"), key(&log, "tom"));
    let fact = |overturned| -> Fields {
        (
            RelationKind::Enemy,
            None,
            "quarrel over the light".into(),
            "quote 3".into(),
            chapter(&log, 1),
            chapter(&log, 1),
            overturned,
        )
    };
    let overturn = (chapter(&log, 3), "the quarrel was staged for the harbourmaster".into(), "quote 4".into());

    assert_eq!(facts(&fold_book(&log, 3), &ada, &tom), [fact(Some(overturn))]);
    // Before the overturning chapter the fact stands.
    assert_eq!(facts(&fold_book(&log, 2), &ada, &tom), [fact(None)]);
}

#[test]
fn statement_after_an_overturn_makes_a_new_current_fact() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, related(RelationKind::Ally, "ada", "tom", "mend the lamp together")),
        (2, overturned(RelationKind::Ally, "ada", "tom", "he broke the lamp himself")),
        (3, related(RelationKind::Ally, "tom", "ada", "row out to the wreck together")),
    ]);
    let ch = |n| chapter(&log, n);
    let cast = fold_book(&log, 3);
    assert_eq!(
        spans(&cast, &key(&log, "ada"), &key(&log, "tom")),
        [(RelationKind::Ally, ch(1), ch(1), Some(ch(2))), (RelationKind::Ally, ch(3), ch(3), None)]
    );
    assert_eq!(relation(&cast, &key(&log, "ada"), &key(&log, "tom")).facts[1].note, "row out to the wreck together");
}

#[test]
fn overturn_of_a_directed_kind_keeps_its_direction() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, related(RelationKind::Serves, "ada", "tom", "keeps the light for his boat")),
        (1, related(RelationKind::Serves, "tom", "ada", "rows her mail over")),
        (2, overturned(RelationKind::Serves, "ada", "tom", "the light was never lit for him")),
    ]);
    let (ada, tom) = (key(&log, "ada"), key(&log, "tom"));
    let cast = fold_book(&log, 2);
    let marks: Vec<_> = relation(&cast, &ada, &tom)
        .facts
        .iter()
        .map(|fact| (fact.from.as_ref(), fact.overturned.as_ref().map(|overturn| overturn.at.chapter)))
        .collect();
    assert_eq!(marks, [(Some(&ada), Some(2)), (Some(&tom), None)]);
}

#[test]
fn overturn_covers_a_statement_a_later_same_as_brings() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, introduced("stranger", "the stranger")),
        (1, related(RelationKind::Ally, "ada", "stranger", "keeps watch with her")),
        (2, related(RelationKind::Ally, "tom", "ada", "mend the lamp together")),
        (3, overturned(RelationKind::Ally, "ada", "tom", "he came to wreck the light")),
        (4, same_as("tom", "stranger")),
    ]);
    let ch = |n| chapter(&log, n);
    let cast = fold_book(&log, 4);
    // The stranger's statement was the same belief, made before the overturn under another id.
    assert_eq!(spans(&cast, &key(&log, "ada"), &key(&log, "tom")), [(RelationKind::Ally, ch(1), ch(2), Some(ch(3)))]);
    assert_eq!(relation(&cast, &key(&log, "ada"), &key(&log, "tom")).facts[0].note, "keeps watch with her");
}

#[test]
fn statement_joined_after_the_overturn_stays_current() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, introduced("stranger", "the stranger")),
        (1, related(RelationKind::Ally, "ada", "tom", "mend the lamp together")),
        (3, overturned(RelationKind::Ally, "tom", "ada", "he came to wreck the light")),
        (4, related(RelationKind::Ally, "ada", "stranger", "pulls her from the water")),
        (5, same_as("tom", "stranger")),
    ]);
    let ch = |n| chapter(&log, n);
    assert_eq!(
        spans(&fold_book(&log, 5), &key(&log, "ada"), &key(&log, "tom")),
        [(RelationKind::Ally, ch(1), ch(1), Some(ch(3))), (RelationKind::Ally, ch(4), ch(4), None)]
    );
}

#[test]
fn overturn_without_a_statement_marks_nothing() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, introduced("pip", "the gull")),
        (1, acquainted("ada", "tom", "met on the jetty")),
        // Nothing of its kind on the pair to cover, and a pair with nothing at all.
        (2, overturned(RelationKind::Ally, "ada", "tom", "never on her side")),
        (2, overturned(RelationKind::Owns, "ada", "pip", "the gull is nobody's")),
        // An overturn never reaches forward.
        (3, related(RelationKind::Ally, "tom", "ada", "mend the lamp together")),
    ]);
    let ch = |n| chapter(&log, n);
    let cast = fold_book(&log, 3);
    assert_eq!(
        spans(&cast, &key(&log, "ada"), &key(&log, "tom")),
        [(RelationKind::Acquaintance, ch(1), ch(1), None), (RelationKind::Ally, ch(3), ch(3), None)]
    );
    assert_eq!(cast.relations.len(), 1);
}

#[test]
fn an_overturn_naming_a_character_not_yet_met_is_passed_over() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("stranger", "the stranger")),
        (1, related(RelationKind::Ally, "ada", "stranger", "keeps watch with her")),
        (2, overturned(RelationKind::Ally, "ada", "tom", "he came to wreck the light")),
        (3, introduced("tom", "the boatman")),
        (3, same_as("tom", "stranger")),
    ]);
    let ch = |n| chapter(&log, n);
    assert_eq!(
        spans(&fold_book(&log, 3), &key(&log, "ada"), &key(&log, "tom")),
        [(RelationKind::Ally, ch(1), ch(1), None)]
    );
}

#[test]
fn each_overturn_of_a_kind_marks_the_statements_before_it() {
    let log = keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, related(RelationKind::Ally, "ada", "tom", "mend the lamp together")),
        (2, overturned(RelationKind::Ally, "ada", "tom", "he broke the lamp himself")),
        (3, related(RelationKind::Ally, "tom", "ada", "row out to the wreck together")),
        (4, overturned(RelationKind::Ally, "tom", "ada", "he holed the boat himself")),
    ]);
    let ch = |n| chapter(&log, n);
    // The second overturn covers only the statement the first left standing.
    assert_eq!(
        spans(&fold_book(&log, 4), &key(&log, "ada"), &key(&log, "tom")),
        [(RelationKind::Ally, ch(1), ch(1), Some(ch(2))), (RelationKind::Ally, ch(3), ch(3), Some(ch(4)))]
    );
}

#[test]
fn a_directed_overturn_reaches_a_statement_through_a_merge_either_way_round() {
    // Either id may be the one merged away: the overturn's `from` is re-keyed when Tom is, the statement's when
    // the stranger is, and either way the two run the same way once more.
    for (survivor, other) in [("stranger", "tom"), ("tom", "stranger")] {
        let log = keeper_with(vec![
            (1, introduced("ada", "the keeper")),
            (1, introduced("stranger", "the stranger")),
            (1, introduced("tom", "the boatman")),
            (1, related(RelationKind::Serves, "stranger", "ada", "carries her lamp oil")),
            (3, overturned(RelationKind::Serves, "tom", "ada", "the oil was for his own boat")),
            (4, same_as(survivor, other)),
        ]);
        let (ada, kept) = (key(&log, "ada"), key(&log, survivor));
        let cast = fold_book(&log, 4);
        let marks: Vec<_> = relation(&cast, &ada, &kept)
            .facts
            .iter()
            .map(|fact| (fact.kind, fact.from.as_ref(), fact.overturned.as_ref().map(|overturn| overturn.at.chapter)))
            .collect();
        assert_eq!(marks, [(RelationKind::Serves, Some(&kept), Some(3))], "{survivor} survives");
    }
}

/// The earlier book of a series that states Ada and Tom are allies in its first chapter.
fn allies() -> BookLog {
    keeper_with(vec![
        (1, introduced("ada", "the keeper")),
        (1, introduced("tom", "the boatman")),
        (1, related(RelationKind::Ally, "ada", "tom", "mend the lamp together")),
    ])
}

/// A later book that meets Ada and the old sailor in its first chapter and overturns their alliance in
/// chapter `overturned_in`.
fn allies_overturned(overturned_in: u32) -> BookLog {
    with_events(
        keepers_return(),
        vec![
            (1, introduced("ada", "Ada")),
            (1, introduced("sailor", "the old sailor")),
            (overturned_in, overturned(RelationKind::Ally, "sailor", "ada", "he came to wreck the light")),
        ],
    )
}

/// Ada and the old sailor of the later book are the earlier book's Ada and Tom.
const ALLIES_LINKED: [(&str, &str); 2] = [("ada", "ada"), ("tom", "sailor")];

#[test]
fn overturn_reaches_an_earlier_book_through_a_link() {
    let (k, r) = (allies(), allies_overturned(2));
    let (ada, tom) = (key(&k, "ada"), key(&k, "tom"));

    let cast = fold_linked(&k, &up_to(&r, 2), &ALLIES_LINKED, 1);
    assert_eq!(spans(&cast, &ada, &tom), [(RelationKind::Ally, chapter(&k, 1), chapter(&k, 1), Some(chapter(&r, 2)))]);

    let cast = fold_linked(&k, &up_to(&r, 1), &ALLIES_LINKED, 1);
    assert_eq!(spans(&cast, &ada, &tom), [(RelationKind::Ally, chapter(&k, 1), chapter(&k, 1), None)]);
}

#[test]
fn link_after_an_overturn_still_brings_the_fact_under_it() {
    let (k, r) = (allies(), allies_overturned(1));
    // The overturn is taken in while the later book's two still stand apart from the earlier book's, with
    // nothing of their own to cover.
    let cast = fold_linked(&k, &up_to(&r, 2), &ALLIES_LINKED, 2);
    assert_eq!(
        spans(&cast, &key(&k, "ada"), &key(&k, "tom")),
        [(RelationKind::Ally, chapter(&k, 1), chapter(&k, 1), Some(chapter(&r, 1)))]
    );
}
