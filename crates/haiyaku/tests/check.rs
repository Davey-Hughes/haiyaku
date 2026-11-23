//! A model's reply checked against its chapter: which of its characters become events, under which of the
//! program's ids, with which names and quotes. Each test on chapters of invented sentences and a reply written
//! by hand.

#![cfg(feature = "extract")]

mod common;

use std::collections::BTreeMap;

use common::epub::{chapter_text, name, relation, relevant_roster};
use common::texts;
use haiyaku::book::ChapterText;
use haiyaku::extract::{Outcome, Rejected, Roster, check, check_reveals, reveal_schema, schema};
use haiyaku::fold::{Cast, Folder};
use haiyaku::log::{Entity, EntityKind, Event, EventBody, Name, Position, RelationKind, Sex};
use serde_json::{Value, json};

/// A name with no part labelled, as a description is written.
fn description(text: &str) -> Value {
    name(text, None, None, None, None)
}

/// A character item of a reply.
fn character(reference: &str, name: Value, aliases: &[Value], evidence: &str) -> Value {
    json!({ "ref": reference, "name": name, "aliases": aliases, "evidence": evidence })
}

/// A reply of these characters, relations and same-as items.
fn reply_with(characters: &[Value], relations: &[Value], same_as: &[Value]) -> Value {
    json!({ "characters": characters, "relations": relations, "same_as": same_as })
}

/// A reply of these characters, with no relations and no same-as.
fn reply(characters: &[Value]) -> Value {
    reply_with(characters, &[], &[])
}

/// A same-as item of a reply: `other` is `reference`.
fn same_as(reference: &str, other: &str, evidence: &str) -> Value {
    json!({ "ref": reference, "other": other, "evidence": evidence })
}

/// A name as the log stores it.
fn stored(text: &str, given: Option<&str>, surname: Option<&str>, title: Option<&str>, title_sex: Option<Sex>) -> Name {
    Name {
        text: text.into(),
        given: given.map(Into::into),
        surname: surname.map(Into::into),
        title: title.map(Into::into),
        title_sex,
    }
}

fn at(document: u32, block: u32) -> Position {
    Position { document, block }
}

fn event(seq: u32, chapter: u32, at: Position, quote: &str, body: EventBody) -> Event {
    Event { seq, chapter, at, quote: quote.into(), body }
}

fn introduced(entity: &str, name: Name) -> EventBody {
    EventBody::Introduced { entity: entity.into(), name }
}

fn appeared(entity: &str) -> EventBody {
    EventBody::Appeared { entity: entity.into() }
}

fn named(entity: &str, name: Name) -> EventBody {
    EventBody::Named { entity: entity.into(), name }
}

fn alias(entity: &str, name: Name) -> EventBody {
    EventBody::Alias { entity: entity.into(), name }
}

fn related(from: &str, to: &str, kind: RelationKind, note: &str) -> EventBody {
    EventBody::Relation { from: from.into(), to: to.into(), kind, note: note.into() }
}

fn same(entity: &str, other: &str) -> EventBody {
    EventBody::SameAs { entity: entity.into(), other: other.into() }
}

fn character_entity(id: &str) -> Entity {
    Entity { id: id.into(), kind: EntityKind::Character }
}

fn bodies(events: &[Event]) -> Vec<EventBody> {
    events.iter().map(|event| event.body.clone()).collect()
}

fn rejected(reason: &'static str, item: Value) -> Rejected {
    Rejected { reason, item }
}

/// The cast every event of the roster adds up to.
fn cast(roster: &Roster) -> Cast {
    let mut folder = Folder::new();
    folder.start_book("book");
    for event in roster.events() {
        folder.apply(event);
    }
    folder.finish()
}

/// Every string an event holds: its quote, the ids it names, its note, and its name with the name's parts.
fn fields(event: &Event) -> Vec<&str> {
    let mut fields = vec![event.quote.as_str()];
    match &event.body {
        EventBody::Introduced { entity, name }
        | EventBody::Named { entity, name }
        | EventBody::Alias { entity, name } => {
            fields.push(entity);
            fields.push(&name.text);
            fields.extend([&name.given, &name.surname, &name.title].into_iter().flatten().map(String::as_str));
        }
        EventBody::Appeared { entity } => fields.push(entity),
        EventBody::Relation { from, to, note, .. } | EventBody::Overturned { from, to, note, .. } => {
            fields.extend([from, to, note].map(String::as_str));
        }
        EventBody::SameAs { entity, other } => fields.extend([entity, other].map(String::as_str)),
    }
    fields
}

/// The first chapter of a keeper's story: the keeper, her gull, and whose gull he is.
fn keeper_chapter() -> ChapterText {
    chapter_text(
        1,
        1,
        &[
            "At dusk the keeper climbed to the lamp.",
            "Pip was already on the rail, shouting at the sea.",
            "The keeper fed Pip, her gull, from her own plate.",
        ],
    )
}

fn pip() -> Value {
    name("Pip", Some("Pip"), None, None, None)
}

/// The characters of [`keeper_chapter`], called `keeper` and `dog`.
fn keeper_characters() -> [Value; 2] {
    [
        character("keeper", description("the keeper"), &[], "At dusk the keeper climbed to the lamp"),
        character("dog", pip(), &[], "Pip was already on the rail"),
    ]
}

/// The relation of [`keeper_chapter`]: the keeper owns her gull.
fn keeper_owns_pip() -> Value {
    relation("keeper", "dog", "owns", "her gull", "The keeper fed Pip, her gull")
}

/// A reply to [`keeper_chapter`] that calls the keeper `keeper` and the gull `dog`.
fn keeper_reply() -> Value {
    reply_with(&keeper_characters(), &[keeper_owns_pip()], &[])
}

fn ada_marsh() -> Value {
    name("Ada Marsh", Some("Ada"), Some("Marsh"), None, None)
}

fn mrs_marsh() -> Value {
    name("Mrs. Marsh", None, Some("Marsh"), Some("Mrs."), Some("female"))
}

#[test]
fn first_chapter_introduces() {
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &keeper_reply());

    let expected = [
        event(
            1,
            1,
            at(1, 0),
            "At dusk the keeper climbed to the lamp",
            introduced("c1", stored("the keeper", None, None, None, None)),
        ),
        event(
            2,
            1,
            at(1, 1),
            "Pip was already on the rail",
            introduced("c2", stored("Pip", Some("Pip"), None, None, None)),
        ),
        event(3, 1, at(1, 2), "The keeper fed Pip, her gull", related("c1", "c2", RelationKind::Owns, "her gull")),
    ];
    assert_eq!(outcome.events, expected);
    assert_eq!(roster.events(), expected);
    assert_eq!(outcome.kept, 3);
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    assert_eq!(roster.entities(), [character_entity("c1"), character_entity("c2")]);
}

#[test]
fn ids_are_the_programs() {
    let mut roster = Roster::new();
    check(&mut roster, &keeper_chapter(), &keeper_reply());

    assert_eq!(roster.events().len(), 3);
    let handles = ["keeper", "dog"];
    for entity in roster.entities() {
        assert!(!handles.contains(&entity.id.as_str()), "{entity:?}");
    }
    for event in roster.events() {
        assert!(fields(event).iter().all(|field| !handles.contains(field)), "{event:?}");
    }
}

#[test]
fn known_character_appears_once_a_chapter() {
    let ada = name("Ada", Some("Ada"), None, None, None);
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["Ada lit the lamp at dusk and watched the sea."]);
    check(&mut roster, &first, &reply(&[character("ada", ada.clone(), &[], "Ada lit the lamp at dusk")]));

    let second = chapter_text(
        2,
        2,
        &["In the morning Ada walked down to the landing stage.", "Ada read the letter twice on the steps."],
    );
    let outcome = check(
        &mut roster,
        &second,
        &reply(&[
            character("c1", ada.clone(), &[], "Ada walked down to the landing stage"),
            character("c1", ada, &[], "Ada read the letter twice on the steps"),
        ]),
    );

    assert_eq!(outcome.events, [event(2, 2, at(2, 0), "Ada walked down to the landing stage", appeared("c1"))]);
    assert_eq!(outcome.kept, 1);
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
}

#[test]
fn no_appeared_in_the_chapter_of_introduction() {
    let chapter = chapter_text(
        1,
        1,
        &["Ada Marsh lit the lamp at dusk.", "Good night, Mrs. Marsh, called the boy from the boat."],
    );
    let mut roster = Roster::new();
    // The handle again, then a second handle that her full name matches to her.
    let outcome = check(
        &mut roster,
        &chapter,
        &reply(&[
            character("ada", ada_marsh(), &[], "Ada Marsh lit the lamp at dusk"),
            character("ada", ada_marsh(), &[mrs_marsh()], "Good night, Mrs. Marsh, called the boy"),
            character("her", ada_marsh(), &[], "Ada Marsh lit the lamp at dusk"),
        ]),
    );

    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("Ada Marsh", Some("Ada"), Some("Marsh"), None, None)),
            alias("c1", stored("Mrs. Marsh", None, Some("Marsh"), Some("Mrs."), Some(Sex::Female))),
        ]
    );
    assert_eq!(outcome.notes.get("matched_by_name"), Some(&1));
    assert_eq!(outcome.kept, 2);
    assert_eq!(roster.entities(), [character_entity("c1")]);
}

/// Chapters 1 to 3 of the keeper's story: she is "the keeper", then Ada Marsh, then Mrs. Marsh.
fn keeper_chapters() -> [ChapterText; 3] {
    [
        chapter_text(1, 1, &["At dusk the keeper climbed to the lamp."]),
        chapter_text(2, 2, &["The letter was addressed to Ada Marsh.", "She read it twice on the steps."]),
        chapter_text(
            3,
            3,
            &["Ada read the harbour list by the slipway.", "Good night, Mrs. Marsh, said the boy from the boat."],
        ),
    ]
}

/// The roster after chapters 1 and 2 of [`keeper_chapters`], and the outcome of chapter 2.
fn keeper_named() -> (Roster, Outcome) {
    let [first, second, _] = keeper_chapters();
    let mut roster = Roster::new();
    check(
        &mut roster,
        &first,
        &reply(&[character("keeper", description("the keeper"), &[], "At dusk the keeper climbed to the lamp")]),
    );
    let outcome =
        check(&mut roster, &second, &reply(&[character("c1", ada_marsh(), &[], "She read it twice on the steps")]));
    (roster, outcome)
}

#[test]
fn named_upgrades_a_description() {
    let (roster, outcome) = keeper_named();

    assert_eq!(
        outcome.events,
        [
            event(2, 2, at(2, 1), "She read it twice on the steps", appeared("c1")),
            event(
                3,
                2,
                at(2, 0),
                "Ada Marsh",
                named("c1", stored("Ada Marsh", Some("Ada"), Some("Marsh"), None, None))
            ),
        ]
    );
    let cast = cast(&roster);
    assert_eq!(cast.characters.len(), 1);
    assert_eq!(cast.characters[0].name.text, "Ada Marsh");
    assert_eq!(texts(&cast.characters[0].aliases), ["the keeper"]);
}

#[test]
fn lesser_name_becomes_an_alias() {
    let (mut roster, _) = keeper_named();
    let [.., third] = keeper_chapters();
    let outcome = check(
        &mut roster,
        &third,
        &reply(&[character("c1", mrs_marsh(), &[], "Ada read the harbour list by the slipway")]),
    );

    assert_eq!(
        outcome.events,
        [
            event(4, 3, at(3, 0), "Ada read the harbour list by the slipway", appeared("c1")),
            event(
                5,
                3,
                at(3, 1),
                "Mrs. Marsh",
                alias("c1", stored("Mrs. Marsh", None, Some("Marsh"), Some("Mrs."), Some(Sex::Female))),
            ),
        ]
    );
    let cast = cast(&roster);
    assert_eq!(cast.characters[0].name.text, "Ada Marsh");
    assert_eq!(texts(&cast.characters[0].aliases), ["the keeper", "Mrs. Marsh"]);
}

#[test]
fn name_outranking_the_shown_name_is_named() {
    let mut roster = Roster::new();
    let first = chapter_text(
        1,
        1,
        &["On the jetty stood the stranger, his coat dripping.", "Good evening, Mr. Reed, said the harbour master."],
    );
    let mr_reed = name("Mr. Reed", None, Some("Reed"), Some("Mr."), Some("male"));
    check(
        &mut roster,
        &first,
        &reply(&[character("stranger", description("the stranger"), &[mr_reed], "On the jetty stood the stranger")]),
    );

    // "Mr. Reed", given as an alias, outranks "the stranger" and is shown from chapter 1 on.
    assert_eq!(cast(&roster).characters[0].name.text, "Mr. Reed");

    let second = chapter_text(2, 2, &["Tom Reed took off his hat and shook the rain from it."]);
    let outcome = check(&mut roster, &second, &reply(&[character("c1", tom_reed(), &[], "Tom Reed took off his hat")]));

    // "Tom Reed" outranks "Mr. Reed", the name shown.
    assert_eq!(
        bodies(&outcome.events),
        [appeared("c1"), named("c1", stored("Tom Reed", Some("Tom"), Some("Reed"), None, None))]
    );
    assert_eq!(cast(&roster).characters[0].name.text, "Tom Reed");
}

#[test]
fn alias_outranking_the_shown_name_is_named() {
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["At noon the tall woman rowed in from the point."]);
    check(
        &mut roster,
        &first,
        &reply(&[character("woman", description("the tall woman"), &[], "At noon the tall woman rowed in")]),
    );

    let second = chapter_text(
        2,
        2,
        &[
            "Down at the harbour the tall woman counted the nets.",
            "Good day, Mrs. Marsh, called the ferryman.",
            "The children called her Widow Marsh, or just the widow.",
        ],
    );
    let evidence = "Down at the harbour the tall woman counted the nets";
    let widow_marsh = name("Widow Marsh", None, Some("Marsh"), Some("Widow"), Some("female"));
    let aliases = [mrs_marsh(), widow_marsh, description("the widow")];
    let outcome =
        check(&mut roster, &second, &reply(&[character("c1", description("the tall woman"), &aliases, evidence)]));

    // "Mrs. Marsh" outranks "the tall woman", the name shown until then. "Widow Marsh" ranks no higher than "Mrs.
    // Marsh", which is shown by the time it is weighed, and "the widow" ranks lower.
    assert_eq!(
        outcome.events,
        [
            event(2, 2, at(2, 0), evidence, appeared("c1")),
            event(
                3,
                2,
                at(2, 1),
                "Mrs. Marsh",
                named("c1", stored("Mrs. Marsh", None, Some("Marsh"), Some("Mrs."), Some(Sex::Female))),
            ),
            event(
                4,
                2,
                at(2, 2),
                "Widow Marsh",
                alias("c1", stored("Widow Marsh", None, Some("Marsh"), Some("Widow"), Some(Sex::Female))),
            ),
            event(5, 2, at(2, 2), "the widow", alias("c1", stored("the widow", None, None, None, None))),
        ]
    );
    let cast = cast(&roster);
    assert_eq!(cast.characters[0].name.text, "Mrs. Marsh");
    assert_eq!(texts(&cast.characters[0].aliases), ["the tall woman", "Widow Marsh", "the widow"]);
}

/// A roster that holds Ada Marsh as c1, with the alias Mrs. Marsh.
fn ada_roster() -> Roster {
    let mut roster = Roster::new();
    let chapter = chapter_text(
        1,
        1,
        &["Ada Marsh lit the lamp at dusk.", "Good night, Mrs. Marsh, called the boy from the boat."],
    );
    let outcome = check(
        &mut roster,
        &chapter,
        &reply(&[character("ada", ada_marsh(), &[mrs_marsh()], "Ada Marsh lit the lamp at dusk")]),
    );
    assert_eq!(outcome.events.len(), 2);
    roster
}

#[test]
fn new_character_with_a_known_alias_stays_new() {
    let mut roster = ada_roster();
    // Ada's husband's mother, also Mrs. Marsh.
    let chapter = chapter_text(
        2,
        2,
        &[
            "Old Mrs. Marsh came up the hill from the village with a basket of eggs.",
            "She had come to see her son's wife, as she did every Sunday.",
        ],
    );
    let outcome = check(
        &mut roster,
        &chapter,
        &reply_with(
            &[character("mother", mrs_marsh(), &[], "Old Mrs. Marsh came up the hill")],
            &[relation("mother", "c1", "family", "her son's wife", "She had come to see her son's wife")],
            &[],
        ),
    );

    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c2", stored("Mrs. Marsh", None, Some("Marsh"), Some("Mrs."), Some(Sex::Female))),
            related("c2", "c1", RelationKind::Family, "her son's wife"),
        ]
    );
    assert_eq!(outcome.notes.get("matched_by_name"), None);
    assert_eq!(roster.entities(), [character_entity("c1"), character_entity("c2")]);
}

#[test]
fn full_name_match_is_the_known_character() {
    let mut roster = ada_roster();
    let chapter = chapter_text(2, 2, &["Ada Marsh walked down to the harbour at noon."]);
    let outcome = check(
        &mut roster,
        &chapter,
        &reply(&[character("her", ada_marsh(), &[], "Ada Marsh walked down to the harbour")]),
    );

    assert_eq!(outcome.events, [event(3, 2, at(2, 0), "Ada Marsh walked down to the harbour", appeared("c1"))]);
    assert_eq!(outcome.notes.get("matched_by_name"), Some(&1));
    assert_eq!(roster.entities(), [character_entity("c1")]);
}

#[test]
fn a_known_full_name_among_the_aliases_is_the_known_character() {
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["Tom Reed rowed out past the rocks at dawn."]);
    check(&mut roster, &first, &reply(&[character("tom", tom_reed(), &[], "Tom Reed rowed out past the rocks")]));

    // The chapter holds "the stranger" first, which no one carries, and then Tom Reed, whom c1 carries.
    let item = character("him", description("the stranger"), &[tom_reed()], "On the jetty stood the stranger");
    let outcome = check(&mut roster, &stranger_chapter(), &reply(&[item]));

    assert_eq!(bodies(&outcome.events), [appeared("c1"), alias("c1", stored("the stranger", None, None, None, None))]);
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    assert_eq!(outcome.notes.get("matched_by_name"), Some(&1));
    assert_eq!(roster.entities(), [character_entity("c1")]);
}

#[test]
fn a_new_full_name_stays_new_beside_a_known_full_name_among_its_aliases() {
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["Tom Reed rowed out past the rocks at dawn."]);
    check(&mut roster, &first, &reply(&[character("tom", tom_reed(), &[], "Tom Reed rowed out past the rocks")]));

    // The chapter holds Hal Brook, whom no one carries, and Tom Reed, whom c1 carries.
    let second = chapter_text(2, 2, &["Hal Brook mended the nets while Tom Reed watched."]);
    let hal_brook = name("Hal Brook", Some("Hal"), Some("Brook"), None, None);
    let item = character("hal", hal_brook, &[tom_reed()], "Hal Brook mended the nets");
    let outcome = check(&mut roster, &second, &reply(&[item]));

    assert_eq!(
        bodies(&outcome.events),
        [introduced("c2", stored("Hal Brook", Some("Hal"), Some("Brook"), None, None))]
    );
    assert_eq!(outcome.rejected, [rejected("name_held_by_other", tom_reed())]);
    assert_eq!(outcome.notes.get("matched_by_name"), None);
    assert_eq!(roster.entities(), [character_entity("c1"), character_entity("c2")]);
}

#[test]
fn a_lone_given_name_is_matched_by_a_known_full_name_among_its_aliases() {
    let hal_brook = name("Hal Brook", Some("Hal"), Some("Brook"), None, None);
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["Hal Brook mended the nets on the slipway."]);
    check(&mut roster, &first, &reply(&[character("hal", hal_brook.clone(), &[], "Hal Brook mended the nets")]));

    // The chapter holds "Hal", which no one can carry as a full name, and then Hal Brook, whom c1 carries.
    let second = chapter_text(2, 2, &["Hal waved from the boat.", "Everyone on the quay knew Hal Brook."]);
    let item = character("him", name("Hal", Some("Hal"), None, None, None), &[hal_brook], "Hal waved from the boat");
    let outcome = check(&mut roster, &second, &reply(&[item]));

    assert_eq!(bodies(&outcome.events), [appeared("c1"), alias("c1", stored("Hal", Some("Hal"), None, None, None))]);
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    assert_eq!(outcome.notes.get("matched_by_name"), Some(&1));
    assert_eq!(roster.entities(), [character_entity("c1")]);
}

#[test]
fn a_form_of_a_known_full_name_matched_by_it_is_an_alias() {
    let hal_brook = name("Hal Brook", Some("Hal"), Some("Brook"), None, None);
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["Hal Brook mended the nets on the slipway."]);
    check(&mut roster, &first, &reply(&[character("hal", hal_brook, &[], "Hal Brook mended the nets")]));

    let second = chapter_text(2, 2, &["The clerk wrote Hal Timothy Brook in the harbour ledger."]);
    let longer = name("Hal Timothy Brook", Some("Hal"), Some("Brook"), None, None);
    let outcome =
        check(&mut roster, &second, &reply(&[character("him", longer, &[], "The clerk wrote Hal Timothy Brook")]));

    assert_eq!(
        bodies(&outcome.events),
        [appeared("c1"), alias("c1", stored("Hal Timothy Brook", Some("Hal"), Some("Brook"), None, None))]
    );
    assert_eq!(outcome.notes.get("matched_by_name"), Some(&1));
    assert_eq!(roster.entities(), [character_entity("c1")]);
}

#[test]
fn full_name_held_by_another_is_rejected() {
    let mut roster = Roster::new();
    let first =
        chapter_text(1, 1, &["The letter was addressed to Ada Marsh.", "At dusk the keeper climbed to the lamp."]);
    check(
        &mut roster,
        &first,
        &reply(&[
            character("ada", ada_marsh(), &[], "The letter was addressed to Ada Marsh"),
            character("keeper", description("the keeper"), &[], "At dusk the keeper climbed to the lamp"),
        ]),
    );

    let second = chapter_text(2, 2, &["The keeper said the name Ada Marsh aloud to the gulls."]);
    let outcome = check(&mut roster, &second, &reply(&[character("c2", ada_marsh(), &[], "The keeper said the name")]));

    assert_eq!(bodies(&outcome.events), [appeared("c2")]);
    assert_eq!(outcome.rejected, [rejected("name_held_by_other", ada_marsh())]);
    assert_eq!(outcome.kept, 1);
}

#[test]
fn empty_name_and_embedded_name_are_rejected() {
    let chapter = chapter_text(1, 1, &["He would walk to Alba before the tide turned."]);
    let empty = character("nobody", description(""), &[], "He would walk to Alba before the tide");
    let embedded =
        character("al", name("Al", Some("Al"), None, None, None), &[], "He would walk to Alba before the tide");
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &chapter, &reply(&[empty.clone(), embedded.clone()]));

    assert_eq!(outcome.rejected, [rejected("name_not_in_text", empty), rejected("name_not_in_text", embedded)]);
    assert!(outcome.events.is_empty(), "{:?}", outcome.events);
    assert_eq!(outcome.kept, 0);
    assert!(roster.entities().is_empty());
}

#[test]
fn listed_is_the_named_and_the_recent() {
    let (roster, chapter) = relevant_roster();
    let id = |text: &str| {
        roster
            .events()
            .iter()
            .find_map(|event| match &event.body {
                EventBody::Introduced { entity, name } if name.text == text => Some(entity.clone()),
                _ => None,
            })
            .unwrap()
    };

    let listed = roster.listed(&chapter);
    assert_eq!(listed, [id("Old Sal"), id("Mira")]);
    assert!(!listed.contains(&id("Bren Holt")));

    // A merged-away id is not listed in a chapter that names it, and its survivor is, by the name it took over:
    // c3, the stranger, is merged into c4, Tom Reed, and neither was seen in the two chapters before.
    let (roster, _) = stranger_roster(&[same_as("stranger", "tom", STRANGER_IS_TOM)]);
    let chapter = chapter_text(5, 5, &["The stranger came back on the evening boat."]);
    assert_eq!(roster.listed(&chapter), ["c4"]);
}

#[test]
fn listed_is_also_the_labelled_surname_in_the_chapter() {
    let one =
        chapter_text(1, 1, &["Mrs. Marsh lit the lamp at the window.", "Captain Holt tied the boat up at the jetty."]);
    // No part of Captain Holt's name is labelled a surname, so "Holt" in a later chapter does not list him.
    let captain = name("Captain Holt", None, None, Some("Captain"), Some("male"));
    let characters = [
        character("marsh", mrs_marsh(), &[], "Mrs. Marsh lit the lamp at the window"),
        character("holt", captain, &[], "Captain Holt tied the boat up at the jetty"),
    ];
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &one, &reply(&characters));
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);

    // Neither was seen in chapters 2 and 3, and chapter 4 writes neither name as the roster holds it.
    let four = chapter_text(4, 4, &["Ada Marsh and Jon Holt rowed out to the point."]);
    assert_eq!(roster.listed(&four), ["c1"]);
}

#[test]
fn a_cased_surname_lists_only_as_written_and_whole() {
    let one = chapter_text(1, 1, &["Nora Bell ran out onto the pitch."]);
    let nora = name("Nora Bell", Some("Nora"), Some("Bell"), None, None);
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &one, &reply(&[character("nora", nora, &[], "Nora Bell ran out onto the pitch")]));
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);

    // "bell" in lower case is a word of the language, not her surname.
    let bell = chapter_text(4, 4, &["The bell rang."]);
    assert!(roster.listed(&bell).is_empty());
    let caught = chapter_text(4, 4, &["Bell caught it."]);
    assert_eq!(roster.listed(&caught), ["c1"]);
}

#[test]
fn a_one_syllable_surname_never_lists_and_a_longer_one_runs_on() {
    let one = chapter_text(1, 1, &["한지수는 등대로 갔다.", "홀트 부인은 빵을 구웠다."]);
    let characters = [
        character("jisu", name("한지수", Some("지수"), Some("한"), None, None), &[], "한지수는 등대로 갔다"),
        character(
            "holt",
            name("홀트 부인", None, Some("홀트"), Some("부인"), Some("female")),
            &[],
            "홀트 부인은 빵을 구웠다",
        ),
    ];
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &one, &reply(&characters));
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);

    // 한 starts 한숨, a sigh; a surname of one syllable starts too many words to say who is meant.
    let sigh = chapter_text(4, 4, &["그는 한숨을 쉬었다."]);
    assert!(roster.listed(&sigh).is_empty());
    // A longer surname runs on into the particle written onto it, as a whole name does.
    let bren = chapter_text(4, 4, &["브렌 홀트가 웃었다."]);
    assert_eq!(roster.listed(&bren), ["c2"]);
}

#[test]
fn failed_evidence_falls_back_to_the_name() {
    let mut roster = Roster::new();
    let first = chapter_text(
        1,
        1,
        &["Pip was already on the rail, shouting at the sea.", "The keeper fed Pip from her own plate."],
    );
    let fed = "The keeper fed Pip from her own plate";
    let outcome = check(
        &mut roster,
        &first,
        &reply_with(
            &[
                character("dog", pip(), &[], "Pip sat on the fence and shouted at the waves"),
                character("keeper", description("the keeper"), &[], fed),
            ],
            &[relation("dog", "keeper", "friend", "she feeds him", fed)],
            &[],
        ),
    );
    assert_eq!(
        outcome.events,
        [
            event(1, 1, at(1, 0), "Pip", introduced("c1", stored("Pip", Some("Pip"), None, None, None))),
            event(2, 1, at(1, 1), fed, introduced("c2", stored("the keeper", None, None, None, None))),
            // The character whose evidence failed keeps its relation.
            event(3, 1, at(1, 1), fed, related("c1", "c2", RelationKind::Friend, "she feeds him")),
        ]
    );
    assert_eq!(outcome.notes.get("evidence_replaced"), Some(&1));
    assert_eq!(outcome.kept, 3);

    // A known character's appearance falls back the same way, here from a quote too short to place.
    let second = chapter_text(2, 2, &["The wind dropped at noon.", "Then the gull Pip came down to the jetty."]);
    let outcome = check(&mut roster, &second, &reply(&[character("c1", pip(), &[], "Pip came")]));
    assert_eq!(outcome.events, [event(4, 2, at(2, 1), "Pip", appeared("c1"))]);
    assert_eq!(outcome.notes.get("evidence_replaced"), Some(&1));
}

#[test]
fn known_character_with_neither_evidence_nor_name_is_rejected() {
    let ada = name("Ada", Some("Ada"), None, None, None);
    let mut roster = Roster::new();
    let first = chapter_text(1, 1, &["Ada lit the lamp at dusk and watched the sea."]);
    check(&mut roster, &first, &reply(&[character("ada", ada.clone(), &[], "Ada lit the lamp at dusk")]));

    let second = chapter_text(2, 2, &["The tide came in fast over the causeway."]);
    let absent = character("c1", ada.clone(), &[], "Ada rowed out to the rocks");
    let short = character("c1", ada, &[], "The tide");
    let outcome = check(&mut roster, &second, &reply(&[absent.clone(), short.clone()]));

    assert_eq!(outcome.rejected, [rejected("evidence_not_found", absent), rejected("evidence_too_short", short)]);
    assert!(outcome.events.is_empty(), "{:?}", outcome.events);
    assert_eq!(outcome.kept, 0);
}

#[test]
fn character_known_only_by_an_alias_is_introduced_under_it() {
    let chapter = chapter_text(
        1,
        1,
        &["Mum was asleep in the chair by the stove.", "Bren put a blanket over her and let her sleep."],
    );
    let mrs_holt = name("Mrs. Holt", None, Some("Holt"), Some("Mrs."), Some("female"));
    let blanket = "Bren put a blanket over her";
    let mut roster = Roster::new();
    let outcome = check(
        &mut roster,
        &chapter,
        &reply_with(
            &[
                character("mother", mrs_holt.clone(), &[description("Mum")], "Mum was asleep in the chair"),
                character("son", name("Bren", Some("Bren"), None, None, None), &[], blanket),
            ],
            &[relation("mother", "son", "family", "her son", blanket)],
            &[],
        ),
    );

    assert_eq!(
        outcome.events,
        [
            event(
                1,
                1,
                at(1, 0),
                "Mum was asleep in the chair",
                introduced("c1", stored("Mum", None, None, None, None))
            ),
            event(2, 1, at(1, 1), blanket, introduced("c2", stored("Bren", Some("Bren"), None, None, None))),
            event(3, 1, at(1, 1), blanket, related("c1", "c2", RelationKind::Family, "her son")),
        ]
    );
    assert_eq!(outcome.notes.get("name_replaced_by_alias"), Some(&1));
    // The name the chapter does not say is turned down on its own.
    assert_eq!(outcome.rejected, [rejected("alias_not_in_text", mrs_holt)]);
    assert_eq!(outcome.kept, 3);
}

#[test]
fn label_outside_the_name_is_cleared() {
    let chapter = chapter_text(1, 1, &["Pip was already on the rail."]);
    let mut roster = Roster::new();
    let pip_marsh = name("Pip", None, Some("Marsh"), None, None);
    let outcome =
        check(&mut roster, &chapter, &reply(&[character("dog", pip_marsh, &[], "Pip was already on the rail")]));

    assert_eq!(bodies(&outcome.events), [introduced("c1", stored("Pip", None, None, None, None))]);
    assert_eq!(outcome.notes.get("label_not_in_name"), Some(&1));
}

#[test]
fn title_sex_needs_a_title() {
    let chapter = chapter_text(1, 1, &["Pip watched Rook come up the stair."]);
    let mut roster = Roster::new();
    let evidence = "Pip watched Rook come up the stair";
    let outcome = check(
        &mut roster,
        &chapter,
        &reply(&[
            // No title at all, then a title the name does not hold, which is cleared first.
            character("dog", name("Pip", Some("Pip"), None, None, Some("male")), &[], evidence),
            character("rook", name("Rook", None, Some("Rook"), Some("Captain"), Some("male")), &[], evidence),
        ]),
    );

    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("Pip", Some("Pip"), None, None, None)),
            introduced("c2", stored("Rook", None, Some("Rook"), None, None)),
        ]
    );
    assert_eq!(outcome.notes.get("label_not_in_name"), Some(&1));
}

#[test]
fn korean_name_with_particle_and_trailing_title() {
    let chapter = chapter_text(1, 1, &["한지수는 등대로 갔다. 한 씨는 웃었다."]);
    let mut roster = Roster::new();
    let outcome = check(
        &mut roster,
        &chapter,
        &reply(&[character(
            "jisu",
            name("한지수", Some("지수"), Some("한"), None, None),
            &[name("한 씨", None, Some("한"), Some("씨"), Some("male"))],
            "한지수는 등대로 갔다",
        )]),
    );

    assert_eq!(
        outcome.events,
        [
            event(
                1,
                1,
                at(1, 0),
                "한지수는 등대로 갔다",
                introduced("c1", stored("한지수", Some("지수"), Some("한"), None, None))
            ),
            event(2, 1, at(1, 0), "한 씨", alias("c1", stored("한 씨", None, Some("한"), Some("씨"), Some(Sex::Male)))),
        ]
    );
    assert!(outcome.notes.is_empty(), "{:?}", outcome.notes);
}

#[test]
fn malformed_items_are_rejected_one_by_one() {
    let evidence = "At dusk the keeper climbed to the lamp";
    let no_title_sex = json!({ "text": "Pip", "given": "Pip", "surname": null, "title": null });
    let broken = [
        json!({ "ref": "x", "aliases": [], "evidence": evidence }),
        json!({ "ref": 7, "name": pip(), "aliases": [], "evidence": evidence }),
        json!({ "ref": "x", "name": pip(), "aliases": "Pip", "evidence": evidence }),
        json!({ "ref": "x", "name": pip(), "aliases": [], "evidence": null }),
        json!({ "ref": "x", "name": no_title_sex, "aliases": [], "evidence": evidence }),
        json!({ "ref": "x", "name": name("Pip", Some("Pip"), None, None, Some("gull")), "aliases": [], "evidence": evidence }),
        json!({ "ref": "x", "name": pip(), "aliases": [{ "text": 3 }], "evidence": evidence }),
        json!("Pip"),
    ];
    let [keeper, dog] = keeper_characters();
    let mut characters = vec![keeper];
    characters.extend(broken.iter().cloned());
    characters.push(dog);
    let mut broken_relation = keeper_owns_pip();
    broken_relation["from"] = json!(7);
    let broken_same_as = json!({ "ref": "keeper", "other": null, "evidence": evidence });
    let mut roster = Roster::new();
    let outcome = check(
        &mut roster,
        &keeper_chapter(),
        &reply_with(&characters, &[broken_relation.clone(), keeper_owns_pip()], std::slice::from_ref(&broken_same_as)),
    );

    let mut expected: Vec<Rejected> = broken.into_iter().map(|item| rejected("malformed", item)).collect();
    expected.extend([rejected("malformed", broken_relation), rejected("malformed", broken_same_as)]);
    assert_eq!(outcome.rejected, expected);
    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("the keeper", None, None, None, None)),
            introduced("c2", stored("Pip", Some("Pip"), None, None, None)),
            related("c1", "c2", RelationKind::Owns, "her gull"),
        ]
    );
    assert_eq!(outcome.kept, 3);
}

#[test]
fn missing_array_is_empty() {
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &json!({}));
    assert!(outcome.events.is_empty() && outcome.rejected.is_empty() && outcome.notes.is_empty(), "{outcome:?}");
    assert_eq!(outcome.kept, 0);

    let keeper = character("keeper", description("the keeper"), &[], "At dusk the keeper climbed to the lamp");
    let outcome = check(&mut roster, &keeper_chapter(), &json!({ "characters": [keeper] }));
    assert_eq!(bodies(&outcome.events), [introduced("c1", stored("the keeper", None, None, None, None))]);
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
}

#[test]
fn field_that_is_not_an_array_is_rejected() {
    let keeper = character("keeper", description("the keeper"), &[], "At dusk the keeper climbed to the lamp");
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &json!({ "characters": keeper }));

    assert_eq!(outcome.rejected, [rejected("malformed", keeper)]);
    assert!(outcome.events.is_empty(), "{:?}", outcome.events);
}

#[test]
fn reply_that_is_not_an_object_is_rejected() {
    let [keeper, _] = keeper_characters();
    for reply in [json!([keeper]), json!("the keeper"), Value::Null] {
        let mut roster = Roster::new();
        let outcome = check(&mut roster, &keeper_chapter(), &reply);

        assert_eq!(outcome.rejected, [rejected("malformed", reply)]);
        assert!(outcome.events.is_empty(), "{:?}", outcome.events);
        assert_eq!(outcome.kept, 0);
    }
}

#[test]
fn notes_count_only_items_not_turned_down() {
    // Each name labels a surname its text does not hold, which is cleared and noted.
    let ghost = character(
        "ghost",
        name("the ghost", None, Some("Marsh"), None, None),
        &[],
        "At dusk the keeper climbed to the lamp",
    );
    let dog = character("dog", name("Pip", None, Some("Marsh"), None, None), &[], "Pip was already on the rail");
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &reply(&[ghost.clone(), dog]));
    assert_eq!(outcome.rejected, [rejected("name_not_in_text", ghost)]);
    assert_eq!(outcome.notes, BTreeMap::from([("label_not_in_name", 1)]));

    let second = chapter_text(2, 2, &["The tide came in fast over the causeway."]);
    let absent = character("c1", name("Pip", None, Some("Marsh"), None, None), &[], "Pip rowed out to the rocks");
    let outcome = check(&mut roster, &second, &reply(std::slice::from_ref(&absent)));
    assert_eq!(outcome.rejected, [rejected("evidence_not_found", absent)]);
    assert!(outcome.notes.is_empty(), "{:?}", outcome.notes);
}

/// Chapter 2 after [`keeper_chapter`]: the stranger on the jetty is Tom Reed.
fn stranger_chapter() -> ChapterText {
    chapter_text(2, 2, &["On the jetty stood the stranger, his coat dripping.", "He said his name was Tom Reed."])
}

/// The quote of [`stranger_chapter`] that says the stranger is Tom Reed.
const STRANGER_IS_TOM: &str = "He said his name was Tom Reed";

fn tom_reed() -> Value {
    name("Tom Reed", Some("Tom"), Some("Reed"), None, None)
}

/// The roster after [`keeper_chapter`] and [`stranger_chapter`], whose reply makes the stranger c3 and Tom Reed c4
/// and holds these same-as items, and the outcome of the stranger chapter.
fn stranger_roster(same_as: &[Value]) -> (Roster, Outcome) {
    let mut roster = Roster::new();
    check(&mut roster, &keeper_chapter(), &keeper_reply());
    let characters = [
        character("stranger", description("the stranger"), &[], "On the jetty stood the stranger"),
        character("tom", tom_reed(), &[], STRANGER_IS_TOM),
    ];
    let outcome = check(&mut roster, &stranger_chapter(), &reply_with(&characters, &[], same_as));
    (roster, outcome)
}

fn thomas_reed() -> Value {
    name("Thomas Reed", Some("Thomas"), Some("Reed"), None, None)
}

/// The quote of [`reed_roster`]'s chapter that says Tom Reed is Thomas Reed.
const REGISTER: &str = "The register named him Thomas Reed";

/// The roster after a chapter whose reply introduces Tom Reed as c1 and Thomas Reed as c2 and holds `same_as`,
/// and that chapter's outcome.
fn reed_roster(same_as: Value) -> (Roster, Outcome) {
    let chapter = chapter_text(
        1,
        1,
        &[
            "Tom Reed rowed out past the rocks at dawn.",
            "The register named him Thomas Reed, born in the year of the storm.",
        ],
    );
    let characters = [
        character("tom", tom_reed(), &[], "Tom Reed rowed out past the rocks"),
        character("thomas", thomas_reed(), &[], REGISTER),
    ];
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &chapter, &reply_with(&characters, &[], &[same_as]));
    (roster, outcome)
}

#[test]
fn survivor_is_the_better_named() {
    for item in [same_as("stranger", "tom", STRANGER_IS_TOM), same_as("tom", "stranger", STRANGER_IS_TOM)] {
        let (_, outcome) = stranger_roster(&[item]);
        assert_eq!(
            outcome.events,
            [
                event(
                    4,
                    2,
                    at(2, 0),
                    "On the jetty stood the stranger",
                    introduced("c3", stored("the stranger", None, None, None, None)),
                ),
                event(
                    5,
                    2,
                    at(2, 1),
                    STRANGER_IS_TOM,
                    introduced("c4", stored("Tom Reed", Some("Tom"), Some("Reed"), None, None))
                ),
                event(6, 2, at(2, 1), STRANGER_IS_TOM, same("c4", "c3")),
            ]
        );
        assert_eq!(outcome.kept, 3);
    }

    // Tom Reed and Thomas Reed rank alike, so the lower number survives, whichever way the reply writes them.
    for item in [same_as("tom", "thomas", REGISTER), same_as("thomas", "tom", REGISTER)] {
        let (_, outcome) = reed_roster(item);
        assert_eq!(outcome.events.last().map(|event| &event.body), Some(&same("c1", "c2")));
    }
}

#[test]
fn full_name_match_lands_on_the_survivor() {
    let (mut roster, outcome) = reed_roster(same_as("thomas", "tom", REGISTER));
    assert_eq!(outcome.events.last().map(|event| &event.body), Some(&same("c1", "c2")));

    let chapter = chapter_text(2, 2, &["Thomas Reed came home on the evening tide."]);
    let evidence = "Thomas Reed came home on the evening tide";
    let outcome = check(&mut roster, &chapter, &reply(&[character("him", thomas_reed(), &[], evidence)]));

    // Thomas Reed is c2's own name, and c2 is merged into c1.
    assert_eq!(outcome.events, [event(4, 2, at(2, 0), evidence, appeared("c1"))]);
    assert_eq!(outcome.notes.get("matched_by_name"), Some(&1));
}

#[test]
fn unlisted_id_is_a_handle() {
    let (mut roster, _) = stranger_roster(&[same_as("stranger", "tom", STRANGER_IS_TOM)]);
    let chapter = chapter_text(
        5,
        5,
        &["The stranger came back on the evening boat.", "Wen the ferryman tied her up and walked home."],
    );
    // c3 is merged into c4, and c1, the keeper, was last seen in chapter 1 and is not named here.
    assert_eq!(roster.listed(&chapter), ["c4"]);

    let outcome = check(
        &mut roster,
        &chapter,
        &reply(&[
            character("c3", description("the stranger"), &[], "The stranger came back on the evening boat"),
            character("c1", name("Wen", Some("Wen"), None, None, None), &[], "Wen the ferryman tied her up"),
        ]),
    );

    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c5", stored("the stranger", None, None, None, None)),
            introduced("c6", stored("Wen", Some("Wen"), None, None, None)),
        ]
    );
}

#[test]
fn character_nowhere_in_the_text_is_rejected_with_its_relations() {
    let [keeper, _] = keeper_characters();
    let ghost = character("ghost", description("the ghost"), &[], "At dusk the keeper climbed to the lamp");
    let haunts = relation("ghost", "keeper", "enemy", "haunts her", "At dusk the keeper climbed to the lamp");
    let fears = relation("keeper", "ghost", "enemy", "fears it", "At dusk the keeper climbed to the lamp");
    let mut roster = Roster::new();
    let outcome = check(
        &mut roster,
        &keeper_chapter(),
        &reply_with(&[keeper, ghost.clone()], &[haunts.clone(), fears.clone()], &[]),
    );

    assert_eq!(
        outcome.rejected,
        [rejected("name_not_in_text", ghost), rejected("unknown_ref", haunts), rejected("unknown_ref", fears)]
    );
    assert_eq!(bodies(&outcome.events), [introduced("c1", stored("the keeper", None, None, None, None))]);
    assert_eq!(outcome.kept, 1);
}

#[test]
fn relation_needs_its_own_evidence() {
    // Both ends are in the chapter, but only its own quote places a relation.
    let absent = relation("keeper", "dog", "owns", "her gull", "The keeper kept Pip in a cage");
    let short = relation("keeper", "dog", "owns", "her gull", "Pip");
    let mut roster = Roster::new();
    let outcome =
        check(&mut roster, &keeper_chapter(), &reply_with(&keeper_characters(), &[absent.clone(), short.clone()], &[]));

    assert_eq!(outcome.rejected, [rejected("evidence_not_found", absent), rejected("evidence_too_short", short)]);
    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("the keeper", None, None, None, None)),
            introduced("c2", stored("Pip", Some("Pip"), None, None, None)),
        ]
    );
    assert!(outcome.notes.is_empty(), "{:?}", outcome.notes);
}

#[test]
fn self_relation_rejected() {
    let mut roster = ada_roster();
    let chapter = chapter_text(
        2,
        2,
        &["Ada Marsh walked down to the harbour at noon.", "She talked to herself all the way down the hill."],
    );
    let evidence = "She talked to herself all the way";
    let itself = relation("c1", "c1", "friend", "her own company", evidence);
    // `her` is matched to c1 by her full name, so this one too runs from c1 to c1.
    let through_handle = relation("her", "c1", "friend", "her own company", evidence);
    let outcome = check(
        &mut roster,
        &chapter,
        &reply_with(
            &[character("her", ada_marsh(), &[], "Ada Marsh walked down to the harbour")],
            &[itself.clone(), through_handle.clone()],
            &[],
        ),
    );

    assert_eq!(outcome.rejected, [rejected("self_relation", itself), rejected("self_relation", through_handle)]);
    assert_eq!(bodies(&outcome.events), [appeared("c1")]);
}

#[test]
fn a_relation_is_turned_down_for_the_first_rule_it_breaks() {
    // Each item breaks the rule its reason names and every rule after it, in the order unknown_ref,
    // self_relation, bad_kind, then the evidence.
    let absent = "The keeper never once left the mainland";
    let unknown = relation("ghost", "ghost", "pet", "her gull", absent);
    let itself = relation("keeper", "keeper", "pet", "her gull", absent);
    let kind = relation("keeper", "dog", "pet", "her gull", absent);
    let evidence = relation("keeper", "dog", "owns", "her gull", absent);
    let relations = [unknown.clone(), itself.clone(), kind.clone(), evidence.clone()];
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &reply_with(&keeper_characters(), &relations, &[]));

    assert_eq!(
        outcome.rejected,
        [
            rejected("unknown_ref", unknown),
            rejected("self_relation", itself),
            rejected("bad_kind", kind),
            rejected("evidence_not_found", evidence),
        ]
    );
    assert_eq!(outcome.kept, 2);
}

#[test]
fn an_id_not_listed_is_no_end_of_a_relation() {
    let mut roster = Roster::new();
    check(&mut roster, &keeper_chapter(), &keeper_reply());
    let chapter = chapter_text(5, 5, &["Wen the ferryman tied up the boat and walked home."]);
    // c1 and c2 exist, but were last seen in chapter 1 and neither is named here.
    assert!(roster.listed(&chapter).is_empty());

    let evidence = "Wen the ferryman tied up the boat";
    let wen = character("wen", name("Wen", Some("Wen"), None, None, None), &[], evidence);
    let from = relation("c1", "wen", "friend", "ferries her", evidence);
    let to = relation("wen", "c2", "owns", "his gull", evidence);
    let outcome = check(&mut roster, &chapter, &reply_with(&[wen], &[from.clone(), to.clone()], &[]));

    assert_eq!(outcome.rejected, [rejected("unknown_ref", from), rejected("unknown_ref", to)]);
    assert_eq!(bodies(&outcome.events), [introduced("c3", stored("Wen", Some("Wen"), None, None, None))]);
}

#[test]
fn kind_is_trimmed_and_lowercased() {
    let owns = relation("keeper", "dog", " Owns ", "her gull", "The keeper fed Pip, her gull");
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &reply_with(&keeper_characters(), &[owns], &[]));

    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    assert_eq!(bodies(&outcome.events)[2..], [related("c1", "c2", RelationKind::Owns, "her gull")]);
}

#[test]
fn note_is_trimmed_and_its_whitespace_made_one_space() {
    let owns = relation("keeper", "dog", "owns", " her\r\n  gull\t", "The keeper fed Pip, her gull");
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &keeper_chapter(), &reply_with(&keeper_characters(), &[owns], &[]));

    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    assert_eq!(bodies(&outcome.events)[2..], [related("c1", "c2", RelationKind::Owns, "her gull")]);
}

#[test]
fn unknown_kind_rejected() {
    let pet = relation("keeper", "dog", "pet", "her gull", "The keeper fed Pip, her gull");
    let mut roster = Roster::new();
    let outcome =
        check(&mut roster, &keeper_chapter(), &reply_with(&keeper_characters(), std::slice::from_ref(&pet), &[]));

    assert_eq!(outcome.rejected, [rejected("bad_kind", pet)]);
    assert_eq!(outcome.events.len(), 2);
    assert_eq!(outcome.kept, 2);
}

#[test]
fn same_as_needs_two_characters_and_its_own_evidence() {
    let unknown = same_as("stranger", "ghost", STRANGER_IS_TOM);
    let itself = same_as("tom", "tom", STRANGER_IS_TOM);
    let unplaced = same_as("stranger", "tom", "He said his name was Tom Reed of Dunmore");
    let sound = same_as("stranger", "tom", STRANGER_IS_TOM);
    // Once the two are one, a second same-as of them names one character twice.
    let again = same_as("tom", "stranger", STRANGER_IS_TOM);
    let (_, outcome) = stranger_roster(&[unknown.clone(), itself.clone(), unplaced.clone(), sound, again.clone()]);

    assert_eq!(
        outcome.rejected,
        [
            rejected("unknown_ref", unknown),
            rejected("self_same_as", itself),
            rejected("evidence_not_found", unplaced),
            rejected("self_same_as", again),
        ]
    );
    assert_eq!(bodies(&outcome.events)[2..], [same("c4", "c3")]);
    assert_eq!(outcome.kept, 3);
}

/// The steward and the duke of [`steward_chapter`], called `steward` and `duke`.
fn steward_characters() -> [Value; 2] {
    [
        character("steward", description("the steward"), &[], "At first light the steward bowed low"),
        character("duke", description("the duke"), &[], "bowed low before the duke"),
    ]
}

/// A chapter in which the steward serves the duke.
fn steward_chapter() -> ChapterText {
    chapter_text(
        1,
        1,
        &["At first light the steward bowed low before the duke.", "The steward had served the duke all his life."],
    )
}

#[test]
fn same_as_between_characters_related_earlier_is_refused() {
    let serves = relation("steward", "duke", "serves", "his steward", "The steward had served the duke all his life");
    let first = reply_with(&steward_characters(), &[serves], &[]);
    let second = chapter_text(2, 2, &["Under the mask the steward wore the face of the duke."]);
    let evidence = "Under the mask the steward wore the face of the duke";
    // The relation runs from c1 to c2, and a same-as either way round is refused.
    for item in [same_as("c1", "c2", evidence), same_as("c2", "c1", evidence)] {
        let mut roster = Roster::new();
        check(&mut roster, &steward_chapter(), &first);
        let outcome = check(&mut roster, &second, &reply_with(&[], &[], std::slice::from_ref(&item)));

        assert_eq!(outcome.rejected, [rejected("related_pair", item)]);
        assert!(outcome.events.is_empty());
        assert_eq!(cast(&roster).characters.len(), 2);
    }
}

#[test]
fn same_as_between_characters_related_in_the_same_reply_is_refused() {
    let item = same_as("dog", "keeper", "The keeper fed Pip, her gull");
    let mut roster = Roster::new();
    let outcome = check(
        &mut roster,
        &keeper_chapter(),
        &reply_with(&keeper_characters(), &[keeper_owns_pip()], std::slice::from_ref(&item)),
    );

    assert_eq!(outcome.rejected, [rejected("related_pair", item)]);
    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("the keeper", None, None, None, None)),
            introduced("c2", stored("Pip", Some("Pip"), None, None, None)),
            related("c1", "c2", RelationKind::Owns, "her gull"),
        ]
    );
    assert_eq!(outcome.kept, 3);
}

#[test]
fn same_as_is_refused_when_the_same_reply_relates_them_with_a_bad_quote() {
    // Turned down for its quote, the relation still says the text tells of two people.
    let item = relation("keeper", "dog", "owns", "her gull", "The keeper bought Pip at the harbour market");
    let same = same_as("dog", "keeper", "The keeper fed Pip, her gull");
    let mut roster = Roster::new();
    let reply = reply_with(&keeper_characters(), std::slice::from_ref(&item), std::slice::from_ref(&same));
    let outcome = check(&mut roster, &keeper_chapter(), &reply);

    assert_eq!(outcome.rejected, [rejected("evidence_not_found", item), rejected("related_pair", same)]);
    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("the keeper", None, None, None, None)),
            introduced("c2", stored("Pip", Some("Pip"), None, None, None)),
        ]
    );
    assert_eq!(outcome.kept, 2);
}

#[test]
fn same_as_is_kept_when_the_only_relation_item_has_an_unknown_kind() {
    // A kind outside the list states no relation the check knows, so the item does not say the two are two people.
    let item = relation("keeper", "dog", "pet", "her gull", "The keeper fed Pip, her gull");
    let mut roster = Roster::new();
    let reply = reply_with(
        &keeper_characters(),
        std::slice::from_ref(&item),
        &[same_as("dog", "keeper", "The keeper fed Pip, her gull")],
    );
    let outcome = check(&mut roster, &keeper_chapter(), &reply);

    assert_eq!(outcome.rejected, [rejected("bad_kind", item)]);
    assert_eq!(
        bodies(&outcome.events),
        [
            introduced("c1", stored("the keeper", None, None, None, None)),
            introduced("c2", stored("Pip", Some("Pip"), None, None, None)),
            same("c2", "c1"),
        ]
    );
    assert_eq!(outcome.kept, 3);
}

#[test]
fn same_as_through_a_merged_id_is_refused() {
    let chapter = chapter_text(
        1,
        1,
        &[
            "At first light the steward bowed low before the duke.",
            "Orlan Vey had ruled the valley for thirty years.",
            "The duke was Orlan Vey, and the steward had served him all his life.",
        ],
    );
    let orlan = name("Orlan Vey", Some("Orlan"), Some("Vey"), None, None);
    let [steward, duke] = steward_characters();
    let characters = [steward, character("orlan", orlan, &[], "Orlan Vey had ruled the valley"), duke];
    let evidence = "The duke was Orlan Vey";
    let serves = |to: &str| relation("steward", to, "serves", "his steward", "the steward had served him all his life");
    // The steward is c1, Orlan Vey c2 and the duke c3, merged into c2 by the first same-as. The relation is the
    // steward's to c2, or to c3 and so to c2 once c3 is merged; either way a same-as of the steward and either id
    // is one of c1 and c2.
    for (relation, item) in [
        (serves("orlan"), same_as("steward", "duke", evidence)),
        (serves("duke"), same_as("steward", "orlan", evidence)),
    ] {
        let mut roster = Roster::new();
        let items = [same_as("duke", "orlan", evidence), item.clone()];
        let outcome = check(&mut roster, &chapter, &reply_with(&characters, &[relation], &items));

        assert_eq!(outcome.rejected, [rejected("related_pair", item)]);
        assert_eq!(outcome.events.last().map(|event| &event.body), Some(&same("c2", "c3")));
    }
}

#[test]
fn schema_lists_every_relation_kind() {
    let kinds = &schema()["properties"]["relations"]["items"]["properties"]["kind"]["enum"];
    assert_eq!(*kinds, json!(RelationKind::ALL.map(RelationKind::as_str)));
}

#[test]
fn schema_requires_every_field_and_no_other() {
    /// Each object schema within `schema`, itself first.
    fn objects(schema: &Value, found: &mut Vec<Value>) {
        if schema["type"] == "object" {
            found.push(schema.clone());
        }
        match schema {
            Value::Object(map) => map.values().for_each(|value| objects(value, found)),
            Value::Array(values) => values.iter().for_each(|value| objects(value, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    objects(&schema(), &mut found);

    // The reply, a character, a name, a relation and a same-as; a name is written out for `name` and for
    // `aliases`.
    assert_eq!(found.len(), 6);
    for object in found {
        let mut properties: Vec<&str> = object["properties"].as_object().unwrap().keys().map(String::as_str).collect();
        let mut required: Vec<&str> =
            object["required"].as_array().unwrap().iter().map(|key| key.as_str().unwrap()).collect();
        properties.sort_unstable();
        required.sort_unstable();
        assert_eq!(properties, required, "{object}");
        assert_eq!(object["additionalProperties"], false, "{object}");
    }
}

#[test]
fn reveal_schema_requires_both_lists() {
    /// The keys of `object`'s properties and its required keys, each sorted.
    fn keys(object: &Value) -> (Vec<&str>, Vec<&str>) {
        let mut properties: Vec<&str> = object["properties"].as_object().unwrap().keys().map(String::as_str).collect();
        let mut required: Vec<&str> =
            object["required"].as_array().unwrap().iter().map(|key| key.as_str().unwrap()).collect();
        properties.sort_unstable();
        required.sort_unstable();
        (properties, required)
    }
    let reveal = reveal_schema();
    let lists = vec!["overturned", "relations"];
    assert_eq!(keys(&reveal), (lists.clone(), lists));
    assert_eq!(reveal["additionalProperties"], false);
    // A relation is the first call's relation item, and an overturned item is written as one.
    let relation = &schema()["properties"]["relations"]["items"];
    assert_eq!(reveal["properties"]["relations"], json!({ "type": "array", "items": relation }));
    let item = &reveal["properties"]["overturned"]["items"];
    let fields = vec!["evidence", "from", "kind", "note", "to"];
    assert_eq!(keys(item), (fields.clone(), fields));
    assert_eq!(item["additionalProperties"], false);
    assert_eq!(item["properties"]["kind"]["enum"], json!(RelationKind::ALL.map(RelationKind::as_str)));
    for field in ["from", "to", "note", "evidence"] {
        assert_eq!(item["properties"][field], json!({ "type": "string" }), "{field}");
    }
}

/// The warden and Corin of [`warden_chapter`], called `warden` and `corin`.
fn warden_characters() -> [Value; 2] {
    [
        character("warden", description("the warden"), &[], "The warden barred the gate against Corin"),
        character("corin", name("Corin", Some("Corin"), None, None, None), &[], "Corin swore that the warden"),
    ]
}

/// A chapter in which Corin takes the warden for his enemy, and the warden teaches Corin.
fn warden_chapter() -> ChapterText {
    chapter_text(
        1,
        1,
        &[
            "The warden barred the gate against Corin at dusk.",
            "Corin swore that the warden was his enemy for life.",
            "Every morning the warden taught Corin the sword in the yard.",
        ],
    )
}

/// The enemy relation of [`warden_chapter`], from the warden to Corin.
fn warden_enemy() -> Value {
    relation("warden", "corin", "enemy", "sworn enemy", "Corin swore that the warden was his enemy for life")
}

/// The roster after [`warden_chapter`], whose reply makes the warden c1 and Corin c2 and states `relations`,
/// every item kept.
fn warden_roster(relations: &[Value]) -> Roster {
    let mut roster = Roster::new();
    let outcome = check(&mut roster, &warden_chapter(), &reply_with(&warden_characters(), relations, &[]));
    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    roster
}

/// The chapter after [`warden_chapter`]: the warden was never Corin's enemy, and never his teacher.
fn unbarred_chapter() -> ChapterText {
    chapter_text(
        2,
        2,
        &[
            "That night the warden left the gate unbarred for Corin.",
            "He had barred it only to keep the guard from finding Corin.",
            "The lessons in the yard had only ever been a cover for their talks.",
            "The warden had stood with Corin against the guard from the start.",
        ],
    )
}

/// The quote of [`unbarred_chapter`] that shows the warden was never Corin's enemy.
const NEVER_ENEMIES: &str = "He had barred it only to keep the guard from finding Corin";

/// The quote of [`unbarred_chapter`] that shows the warden never taught Corin.
const NO_LESSONS: &str = "The lessons in the yard had only ever been a cover";

/// The quote of [`unbarred_chapter`] that shows the warden is Corin's ally.
const ALLIES: &str = "The warden had stood with Corin against the guard";

/// A reply to the second call: these relations and overturned items.
fn reveals(relations: &[Value], overturned: &[Value]) -> Value {
    json!({ "relations": relations, "overturned": overturned })
}

/// An overturned item of a reply to the second call, which is written as a relation item is.
fn overturned(from: &str, to: &str, kind: &str, note: &str, evidence: &str) -> Value {
    relation(from, to, kind, note, evidence)
}

fn overturn(from: &str, to: &str, kind: RelationKind, note: &str) -> EventBody {
    EventBody::Overturned { from: from.into(), to: to.into(), kind, note: note.into() }
}

#[test]
fn overturn_of_a_held_relation_is_kept() {
    let mut roster = warden_roster(&[warden_enemy()]);
    // Written from Corin to the warden: an enemy runs no way round, so this is the relation stated from the warden
    // to Corin.
    let item = overturned("c2", "c1", "enemy", " was hiding\n him ", NEVER_ENEMIES);
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], &[item]));

    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    let body = overturn("c2", "c1", RelationKind::Enemy, "was hiding him");
    assert_eq!(outcome.events, [event(4, 2, at(2, 1), NEVER_ENEMIES, body)]);
    assert_eq!(outcome.kept, 1);
}

#[test]
fn overturn_of_an_unheld_relation_is_rejected() {
    let friend = overturned("c1", "c2", "friend", "never friends", NEVER_ENEMIES);
    let enemy = overturned("c1", "c2", "enemy", "never enemies", NEVER_ENEMIES);
    // The roster holds an enemy and no friend; and then no relation at all.
    for (relations, item) in [(vec![warden_enemy()], friend), (vec![], enemy)] {
        let mut roster = warden_roster(&relations);
        let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], std::slice::from_ref(&item)));

        assert_eq!(outcome.rejected, [rejected("not_recorded", item)]);
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.kept, 0);
    }
}

#[test]
fn second_overturn_is_rejected() {
    let mut roster = warden_roster(&[warden_enemy()]);
    let first = overturned("c1", "c2", "enemy", "never enemies", NEVER_ENEMIES);
    // The first covers the relation, so the roster holds none for the second, either way round.
    let second = overturned("c2", "c1", "enemy", "never enemies", NEVER_ENEMIES);
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], &[first, second.clone()]));

    assert_eq!(outcome.rejected, [rejected("not_recorded", second)]);
    assert_eq!(bodies(&outcome.events), [overturn("c1", "c2", RelationKind::Enemy, "never enemies")]);
    assert_eq!(outcome.kept, 1);
}

#[test]
fn overturn_of_a_directed_kind_needs_its_direction() {
    let mentor = relation("warden", "corin", "mentor", "teaches him the sword", "the warden taught Corin the sword");
    let mut roster = warden_roster(&[mentor]);
    // The warden was Corin's mentor, not Corin the warden's.
    let backwards = overturned("c2", "c1", "mentor", "no lessons", NO_LESSONS);
    let forwards = overturned("c1", "c2", "mentor", "no lessons", NO_LESSONS);
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], &[backwards.clone(), forwards]));

    assert_eq!(outcome.rejected, [rejected("not_recorded", backwards)]);
    assert_eq!(bodies(&outcome.events), [overturn("c1", "c2", RelationKind::Mentor, "no lessons")]);
}

#[test]
fn overturn_needs_its_own_evidence() {
    let mut roster = warden_roster(&[warden_enemy()]);
    // The quote that stated the relation is in the chapter before, not in this one.
    let stated = overturned("c1", "c2", "enemy", "never enemies", "Corin swore that the warden was his enemy for life");
    let short = overturned("c1", "c2", "enemy", "never enemies", "Corin");
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], &[stated.clone(), short.clone()]));

    assert_eq!(outcome.rejected, [rejected("evidence_not_found", stated), rejected("evidence_too_short", short)]);
    assert!(outcome.events.is_empty());
}

#[test]
fn an_overturn_is_turned_down_for_the_first_rule_it_breaks() {
    // Each item breaks the rule its reason names and every rule after it, in the order malformed, unknown_ref,
    // self_overturn, bad_kind, not_recorded, then the evidence.
    let absent = "The warden never once opened the gate";
    let malformed = json!({ "from": "ghost", "to": "ghost", "kind": "pet", "note": "never" });
    let unknown = overturned("ghost", "ghost", "pet", "never", absent);
    let itself = overturned("c1", "c1", "pet", "never", absent);
    let kind = overturned("c1", "c2", "pet", "never", absent);
    let unrecorded = overturned("c1", "c2", "friend", "never", absent);
    let evidence = overturned("c1", "c2", "enemy", "never", absent);
    let items =
        [malformed.clone(), unknown.clone(), itself.clone(), kind.clone(), unrecorded.clone(), evidence.clone()];
    let mut roster = warden_roster(&[warden_enemy()]);
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], &items));

    assert_eq!(
        outcome.rejected,
        [
            rejected("malformed", malformed),
            rejected("unknown_ref", unknown),
            rejected("self_overturn", itself),
            rejected("bad_kind", kind),
            rejected("not_recorded", unrecorded),
            rejected("evidence_not_found", evidence),
        ]
    );
    assert!(outcome.events.is_empty());
}

#[test]
fn overturn_of_a_merged_pair_is_rejected() {
    // The stranger, c3, is merged into Tom Reed, c4: the two are one character, which the prompt lists as c4 alone.
    let (mut roster, _) = stranger_roster(&[same_as("stranger", "tom", STRANGER_IS_TOM)]);
    let chapter = stranger_chapter();
    assert_eq!(roster.listed(&chapter), ["c1", "c2", "c4"]);
    let one = overturned("c4", "c4", "enemy", "never enemies", STRANGER_IS_TOM);
    let merged_away = overturned("c4", "c3", "enemy", "never enemies", STRANGER_IS_TOM);
    let outcome = check_reveals(&mut roster, &chapter, &reveals(&[], &[one.clone(), merged_away.clone()]));

    assert_eq!(outcome.rejected, [rejected("self_overturn", one), rejected("unknown_ref", merged_away)]);
    assert!(outcome.events.is_empty());
}

#[test]
fn unlisted_ref_in_a_reveal_is_unknown() {
    let mut roster = warden_roster(&[warden_enemy()]);
    let chapter = chapter_text(5, 5, &["The warden walked the wall alone in the rain."]);
    // Corin, c2, was last seen in chapter 1 and is not named here; and the second call introduces no one, so a
    // ref of its own is no one.
    assert_eq!(roster.listed(&chapter), ["c1"]);
    let evidence = "The warden walked the wall alone";
    let unlisted = overturned("c1", "c2", "enemy", "never enemies", evidence);
    let new = relation("c1", "new1", "friend", "walks with him", evidence);
    let outcome =
        check_reveals(&mut roster, &chapter, &reveals(std::slice::from_ref(&new), std::slice::from_ref(&unlisted)));

    assert_eq!(outcome.rejected, [rejected("unknown_ref", unlisted), rejected("unknown_ref", new)]);
    assert!(outcome.events.is_empty());
}

#[test]
fn overturn_cannot_cover_its_own_replys_relation() {
    let mut roster = warden_roster(&[]);
    let ally = relation("c1", "c2", "ally", "stood with him", ALLIES);
    let overturn_item = overturned("c1", "c2", "ally", "never allies", NEVER_ENEMIES);
    // The overturned items are checked first, so the relation this reply reveals is not there to overturn.
    let outcome =
        check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[ally], std::slice::from_ref(&overturn_item)));

    assert_eq!(outcome.rejected, [rejected("not_recorded", overturn_item)]);
    assert_eq!(bodies(&outcome.events), [related("c1", "c2", RelationKind::Ally, "stood with him")]);
    assert_eq!(outcome.kept, 1);
}

#[test]
fn a_reveal_relation_restating_its_own_overturn_is_turned_down() {
    let mentor = relation("warden", "corin", "mentor", "teaches him the sword", "the warden taught Corin the sword");
    let mut roster = warden_roster(&[warden_enemy(), mentor]);
    let overturns = [
        overturned("c1", "c2", "enemy", "never enemies", NEVER_ENEMIES),
        overturned("c1", "c2", "mentor", "no lessons", NO_LESSONS),
        // Turned down, so it overturns nothing.
        overturned("c1", "c2", "friend", "never friends", NEVER_ENEMIES),
    ];
    // An enemy runs no way round, so either order restates the overturned fact; a mentor runs one way, so only the
    // same way does, and the other way round is a fact of its own.
    let enemy = relation("c2", "c1", "enemy", "enemies after all", ALLIES);
    let mentor_again = relation("c1", "c2", "mentor", "teaches him", ALLIES);
    let mentor_back = relation("c2", "c1", "mentor", "teaches him", ALLIES);
    let friend = relation("c1", "c2", "friend", "stood with him", ALLIES);
    let relations = [enemy.clone(), mentor_again.clone(), mentor_back, friend];
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&relations, &overturns));

    assert_eq!(
        outcome.rejected,
        [
            rejected("not_recorded", overturns[2].clone()),
            rejected("overturned_in_reply", enemy),
            rejected("overturned_in_reply", mentor_again),
        ]
    );
    assert_eq!(
        bodies(&outcome.events),
        [
            overturn("c1", "c2", RelationKind::Enemy, "never enemies"),
            overturn("c1", "c2", RelationKind::Mentor, "no lessons"),
            related("c2", "c1", RelationKind::Mentor, "teaches him"),
            related("c1", "c2", RelationKind::Friend, "stood with him"),
        ]
    );
    assert_eq!(outcome.kept, 4);
}

#[test]
fn same_as_between_characters_whose_relation_was_overturned_is_refused() {
    let mut roster = warden_roster(&[warden_enemy()]);
    let overturn_item = overturned("c1", "c2", "enemy", "never enemies", NEVER_ENEMIES);
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reveals(&[], &[overturn_item]));
    assert_eq!(bodies(&outcome.events), [overturn("c1", "c2", RelationKind::Enemy, "never enemies")]);

    let chapter =
        chapter_text(3, 3, &["At the gate the warden lowered his hood, and the face beneath it was Corin's."]);
    assert_eq!(roster.listed(&chapter), ["c1", "c2"]);
    let item = same_as("c1", "c2", "the face beneath it was Corin's");
    let outcome = check(&mut roster, &chapter, &reply_with(&[], &[], std::slice::from_ref(&item)));

    // The overturn says the two were never enemies, not that they are one: the text still told of two.
    assert_eq!(outcome.rejected, [rejected("related_pair", item)]);
    assert!(outcome.events.is_empty());
}

#[test]
fn reveal_reply_with_the_first_calls_shape_keeps_its_relations() {
    let mut roster = warden_roster(&[]);
    let guard = character("new1", description("the guard"), &[], "keep the guard from finding Corin");
    let ally = relation("c1", "c2", "ally", "stood with him", ALLIES);
    // No overturned items, and characters and same-as items, which a second call's reply does not hold.
    let outcome = check_reveals(&mut roster, &unbarred_chapter(), &reply_with(&[guard], &[ally], &[]));

    assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
    assert_eq!(bodies(&outcome.events), [related("c1", "c2", RelationKind::Ally, "stood with him")]);
    assert_eq!(outcome.kept, 1);
}
