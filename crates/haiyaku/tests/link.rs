//! The link step on invented books with recorded replies: which names of a later character it asks about, what a
//! call sends, which replies make a link, and what a failed call and the reply cache leave behind.

#![cfg(feature = "extract")]

mod common;

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use common::{db, keeper, keepers, keepers_light, keepers_links, keepers_return};
use haiyaku::fold::EntityKey;
use haiyaku::link::{Call, Refusal, Report, TurnedDown, Verdict, link_order};
use haiyaku::llm::{Backend, Replay, Reply, Request};
use haiyaku::log::{BookLog, Entity, EntityKind, Event, EventBody, Name, Position, RelationKind, Sex};
use haiyaku::store::{Link, LinkMethod, put_links, put_series, read_links, write_book};
use haiyaku::{Error, Result, fold_series};
use serde_json::{Value, json};
use tempfile::TempDir;

/// A reply that names `candidate` and gives `quote`.
fn reply(candidate: &str, quote: &str) -> String {
    json!({ "candidate": candidate, "quote": quote }).to_string()
}

/// A reply that names no candidate.
fn none() -> String {
    json!({ "candidate": null, "quote": "" }).to_string()
}

/// The replies that link the keepers: the second book's Ada is the first book's, and its old sailor is the first
/// book's Tom Reed.
fn keepers_replies() -> Vec<String> {
    vec![reply("e1", "Ada watched the boat come in."), reply("e1", "Call me Tom, said the old sailor.")]
}

/// `logs`, one reading order, linked with `backend`, with replies kept in `cache` when there is one.
fn link(logs: &[BookLog], backend: &dyn Backend, cache: Option<&Path>) -> Result<(Vec<Link>, Report)> {
    link_order(logs, backend, cache, &mut |_| {})
}

fn key(log: &BookLog, id: &str) -> EntityKey {
    EntityKey { book: log.book.sha256.clone(), id: id.into() }
}

/// The verdicts of the calls of `report`, in the order made.
fn verdicts(report: &Report) -> Vec<Verdict> {
    report.calls.iter().map(|call| call.verdict.clone()).collect()
}

/// A book of three chapters laid out as the keepers' books are, whose hash is `hash_digit` 64 times over, with
/// the characters `ids` and `events`.
fn book(hash_digit: char, title: &str, ids: &[&str], events: Vec<Event>) -> BookLog {
    let mut log = keepers_return();
    log.book.sha256 = hash_digit.to_string().repeat(64);
    log.book.title = title.into();
    log.entities = ids.iter().map(|id| Entity { id: (*id).into(), kind: EntityKind::Character }).collect();
    log.events = events;
    log
}

/// An event of `chapter`, in the document that holds it.
fn event(seq: u32, chapter: u32, block: u32, quote: &str, body: EventBody) -> Event {
    Event { seq, chapter, at: Position { document: chapter, block }, quote: quote.into(), body }
}

fn name(text: &str, given: Option<&str>, surname: Option<&str>) -> Name {
    Name {
        text: text.into(),
        given: given.map(Into::into),
        surname: surname.map(Into::into),
        title: None,
        title_sex: None,
    }
}

/// A name with a title, which states `sex` when it is `Some`.
fn titled(text: &str, given: Option<&str>, surname: Option<&str>, title: &str, sex: Option<Sex>) -> Name {
    Name { title: Some(title.into()), title_sex: sex, ..name(text, given, surname) }
}

fn introduced(entity: &str, name: Name) -> EventBody {
    EventBody::Introduced { entity: entity.into(), name }
}

fn alias(entity: &str, name: Name) -> EventBody {
    EventBody::Alias { entity: entity.into(), name }
}

fn appeared(entity: &str) -> EventBody {
    EventBody::Appeared { entity: entity.into() }
}

/// A backend that answers from a replay of its replies, with a time and a cost for each call as a model's backend
/// reports them, 1.5 s and 0.25 USD, and keeps what each call asked.
#[derive(Debug)]
struct Recorder {
    replay: Replay,
    asked: RefCell<Vec<Asked>>,
}

/// What one call asked: its system text, its prompt and its schema.
#[derive(Debug)]
struct Asked {
    system: String,
    user: String,
    schema: Value,
}

impl Recorder {
    fn new(replies: Vec<String>) -> Recorder {
        Recorder { replay: Replay::new(replies), asked: RefCell::default() }
    }
}

impl Backend for Recorder {
    fn name(&self) -> &str {
        self.replay.name()
    }

    fn model(&self) -> &str {
        self.replay.model()
    }

    fn call(&self, request: &Request<'_>) -> Result<Reply> {
        let asked = Asked { system: request.system.into(), user: request.user.into(), schema: request.schema.clone() };
        self.asked.borrow_mut().push(asked);
        Ok(Reply { seconds: 1.5, cost_usd: Some(0.25), ..self.replay.call(request)? })
    }
}

#[test]
fn links_the_keepers() {
    let logs = [keeper(), keepers_return()];
    let (links, report) = link(&logs, &Replay::new(keepers_replies()), None).unwrap();
    let mut linked = keepers_links();
    linked[0].method = LinkMethod::Model { quote: "Ada watched the boat come in.".into() };
    assert_eq!(links, linked);

    let asked: Vec<(&str, u32)> = report.calls.iter().map(|call| (call.name.as_str(), call.chapter)).collect();
    // The sailor's introduction, a description, matches no name of the first book: only "Tom" brings a candidate.
    assert_eq!(asked, [("Ada", 1), ("Tom", 3)]);
    let tom = Call {
        later: key(&logs[1], "sailor"),
        name: "Tom".into(),
        chapter: 3,
        candidates: vec![(key(&logs[0], "tom"), "Tom Reed".into())],
        answer: Some("e1".into()),
        quote: "Call me Tom, said the old sailor.".into(),
        verdict: Verdict::Linked(key(&logs[0], "tom")),
    };
    assert_eq!(report.calls[1], tom);

    let mut conn = db();
    for log in &logs {
        write_book(&mut conn, log).unwrap();
    }
    put_series(&mut conn, &keepers()).unwrap();
    put_links(&mut conn, "keepers", "published", &links, &logs).unwrap();
    assert_eq!(read_links(&conn, "keepers", "published").unwrap(), links);
}

/// The keepers' call for the old sailor's "Tom": the system text, a line `---`, the prompt, a line `---` and the
/// schema.
fn link_call() -> String {
    let recorder = Recorder::new(keepers_replies());
    link(&[keeper(), keepers_return()], &recorder, None).unwrap();
    let asked = recorder.asked.into_inner();
    let Asked { system, user, schema } = &asked[1];
    format!("{system}\n---\n{user}---\n{schema}\n")
}

#[test]
fn link_call_is_byte_for_byte() {
    // A call's text is part of the key its reply is kept under, so a change to it asks every call again: it is made
    // on purpose, with this file written again.
    assert_eq!(link_call(), include_str!("golden/link_call.txt"));
}

#[test]
fn namesakes_stay_apart() {
    let father = book(
        'e',
        "The Old Light",
        &["father"],
        vec![event(
            1,
            1,
            0,
            "Tom Reed had kept the light for forty years before his son was born.",
            introduced("father", name("Tom Reed", Some("Tom"), Some("Reed"))),
        )],
    );
    let logs = [keeper(), father];
    let (links, report) = link(&logs, &Replay::new(vec![none()]), None).unwrap();
    assert_eq!(links, []);
    assert_eq!(verdicts(&report), [Verdict::NoneChosen]);
    assert_eq!(report.calls[0].answer, None);

    let cast = fold_series(&logs, None, &links);
    let named: Vec<&EntityKey> = cast
        .characters
        .iter()
        .filter(|character| character.name.text == "Tom Reed")
        .map(|character| &character.key)
        .collect();
    assert_eq!(named, [&key(&logs[0], "tom"), &key(&logs[1], "father")]);
}

#[test]
fn conflicting_given_names_make_no_call() {
    let ned = book(
        'e',
        "The Marsh Boy",
        &["ned"],
        vec![event(
            1,
            1,
            0,
            "Ned Marsh rowed out to the rock.",
            introduced("ned", name("Ned Marsh", Some("Ned"), Some("Marsh"))),
        )],
    );
    let logs = [keeper(), ned];
    let (links, report) = link(&logs, &Replay::new(vec![]), None).unwrap();
    assert_eq!((links, report.calls), (vec![], vec![]));
    let ada = (key(&logs[0], "ada"), "Ada Marsh".to_owned(), Refusal::Conflict);
    let turned_down =
        TurnedDown { later: key(&logs[1], "ned"), name: "Ned Marsh".into(), chapter: 1, candidates: vec![ada] };
    assert_eq!(report.turned_down, [turned_down]);
}

#[test]
fn a_title_of_the_other_sex_makes_no_call() {
    let mrs = titled("Mrs. Hale", None, Some("Hale"), "Mrs.", Some(Sex::Female));
    let earlier = book(
        'e',
        "The Hale House",
        &["mrs"],
        vec![event(1, 1, 0, "Mrs. Hale kept the house.", introduced("mrs", mrs))],
    );
    let mr = titled("Mr. Hale", None, Some("Hale"), "Mr.", Some(Sex::Male));
    let later =
        book('f', "The Hale Boat", &["mr"], vec![event(1, 1, 0, "Mr. Hale kept the boat.", introduced("mr", mr))]);
    let logs = [earlier, later];
    let (links, report) = link(&logs, &Replay::new(vec![]), None).unwrap();
    assert_eq!((links, report.calls), (vec![], vec![]));
    let mrs = (key(&logs[0], "mrs"), "Mrs. Hale".to_owned(), Refusal::OtherSex);
    let turned_down =
        TurnedDown { later: key(&logs[1], "mr"), name: "Mr. Hale".into(), chapter: 1, candidates: vec![mrs] };
    assert_eq!(report.turned_down, [turned_down]);
}

#[test]
fn conflict_counts_names_up_to_the_names_chapter() {
    let mr = titled("Mr. Marsh", None, Some("Marsh"), "Mr.", Some(Sex::Male));
    let ned = book(
        'e',
        "The Marsh Boy",
        &["ned"],
        vec![
            event(1, 1, 0, "Mr. Marsh rowed out to the rock.", introduced("ned", mr)),
            event(
                2,
                3,
                0,
                "His mother had called him Ned Marsh.",
                EventBody::Named { entity: "ned".into(), name: name("Ned Marsh", Some("Ned"), Some("Marsh")) },
            ),
        ],
    );
    let logs = [keeper(), ned];
    let replay = Replay::new(vec![none()]);
    let (links, report) = link(&logs, &replay, None).unwrap();
    assert_eq!(links, []);
    // In chapter 1 nothing rules Ada Marsh out, though by chapter 3 the two given names would.
    let [call] = report.calls.as_slice() else { panic!("{:#?}", report.calls) };
    assert_eq!((call.name.as_str(), call.chapter), ("Mr. Marsh", 1));
    assert_eq!(call.candidates, [(key(&logs[0], "ada"), "Ada Marsh".to_owned())]);
    assert_eq!(replay.remaining(), 0);
    // A call listed her, so the report does not say she was turned down.
    assert_eq!(report.turned_down, []);
}

#[test]
fn a_character_a_call_lists_is_not_reported_turned_down() {
    let earlier = book(
        'e',
        "The Reed Light",
        &["tom"],
        vec![event(
            1,
            1,
            0,
            "Tom Reed kept the light through the long winter.",
            introduced("tom", name("Tom Reed", Some("Tom"), Some("Reed"))),
        )],
    );
    let thomas = || name("Thomas Reed", Some("Thomas"), Some("Reed"));
    let later = |hash_digit: char, first: Name, second: Name| {
        let events = vec![
            event(1, 1, 0, "Reed came up the jetty steps with his bag.", introduced("reed", first)),
            event(2, 3, 0, "Reed signed the harbour book in his own hand.", alias("reed", second)),
        ];
        book(hash_digit, "The Reed Return", &["reed"], events)
    };
    // Thomas Reed's given name turns Tom Reed down until "Tom" is a name of his too; and the other way round, a
    // call lists Tom Reed for "Reed" before "Thomas Reed" would turn him down.
    for later in
        [later('f', thomas(), name("Tom", Some("Tom"), None)), later('f', name("Reed", None, Some("Reed")), thomas())]
    {
        let logs = [earlier.clone(), later];
        let (links, report) = link(&logs, &Replay::new(vec![none()]), None).unwrap();
        assert_eq!(links, []);
        let [call] = report.calls.as_slice() else { panic!("{:#?}", report.calls) };
        assert_eq!(call.candidates, [(key(&logs[0], "tom"), "Tom Reed".to_owned())]);
        assert_eq!(report.turned_down, [], "{:?}", call.name);
    }
}

#[test]
fn a_character_turned_down_again_for_another_reason_is_reported_again() {
    let mr = || titled("Mr. Grey", None, Some("Grey"), "Mr.", Some(Sex::Male));
    let earlier = book(
        'e',
        "The Grey House",
        &["grey"],
        vec![event(1, 1, 0, "Mr. Grey built the house.", introduced("grey", mr()))],
    );
    let mrs = titled("Mrs. Grey", None, Some("Grey"), "Mrs.", Some(Sex::Female));
    let family =
        EventBody::Relation { from: "mother".into(), to: "father".into(), kind: RelationKind::Family, note: "".into() };
    let later = book(
        'f',
        "The Grey Return",
        &["father", "mother"],
        vec![
            event(1, 1, 0, "Mr. Grey came home to the house he had built.", introduced("father", mr())),
            event(2, 2, 0, "Mrs. Grey met him at the gate.", introduced("mother", mrs)),
            event(3, 2, 1, "She had waited for her husband all summer.", family),
            event(4, 3, 0, "Grey, the harbourmaster called her.", alias("mother", name("Grey", None, Some("Grey")))),
        ],
    );
    let logs = [earlier, later];
    let replay = Replay::new(vec![reply("e1", "Mr. Grey came home to the house he had built.")]);
    let (links, report) = link(&logs, &replay, None).unwrap();
    let (grey, father, mother) = (key(&logs[0], "grey"), key(&logs[1], "father"), key(&logs[1], "mother"));
    assert_eq!(joined(&links), [(&father, &grey)]);
    // Mrs. Grey's title turns Mr. Grey down first; her name "Grey" would make him a candidate, and her husband's
    // link turns him down again.
    let turned_down = |name: &str, chapter: u32, refusal: Refusal| TurnedDown {
        later: mother.clone(),
        name: name.into(),
        chapter,
        candidates: vec![(grey.clone(), "Mr. Grey".into(), refusal)],
    };
    let related = Refusal::Related { relative: father.clone(), name: "Mr. Grey".into() };
    assert_eq!(report.turned_down, [turned_down("Mrs. Grey", 2, Refusal::OtherSex), turned_down("Grey", 3, related)]);
}

/// The Greys: an earlier book's Silas Grey, and a later book whose Silas, introduced in chapter 1, and Mr. Grey,
/// introduced in chapter 2, each have a name that makes Silas Grey a candidate; `between`, when there is one, is an
/// event of the later book at the chapter given that names the two.
fn greys(between: Option<(u32, EventBody)>) -> [BookLog; 2] {
    let earlier = book(
        'e',
        "The Grey House",
        &["grey"],
        vec![event(
            1,
            1,
            0,
            "Silas Grey built the grey house on the hill.",
            introduced("grey", name("Silas Grey", Some("Silas"), Some("Grey"))),
        )],
    );
    let mr = titled("Mr. Grey", None, Some("Grey"), "Mr.", Some(Sex::Male));
    let mut events = vec![
        event(
            1,
            1,
            0,
            "Silas came home to the grey house on the hill.",
            introduced("silas", name("Silas", Some("Silas"), None)),
        ),
        event(2, 2, 0, "Mr. Grey had kept the same coat for twenty winters.", introduced("father", mr)),
    ];
    if let Some((chapter, body)) = between {
        events.push(event(3, chapter, 1, "Mr. Grey had never once called Silas his son.", body));
    }
    [earlier, book('f', "The Grey Return", &["silas", "father"], events)]
}

/// The replies that link each of the Greys' later characters to Silas Grey, in the order of their calls.
fn greys_replies() -> Vec<String> {
    vec![
        reply("e1", "Silas came home to the grey house on the hill."),
        reply("e1", "Mr. Grey had kept the same coat for twenty winters."),
    ]
}

/// The later character and the earlier character of each of `links`.
fn joined(links: &[Link]) -> Vec<(&EntityKey, &EntityKey)> {
    links.iter().map(|link| (&link.later, &link.earlier)).collect()
}

/// Links `logs`, the Greys with an event or two more that relate Mr. Grey to Silas, and checks that Silas Grey, whom
/// the run links Silas to first, is turned down for Mr. Grey with no call.
fn turned_down_as_related(logs: [BookLog; 2]) {
    let replay = Replay::new(greys_replies());
    let (links, report) = link(&logs, &replay, None).unwrap();
    let (grey, silas) = (key(&logs[0], "grey"), key(&logs[1], "silas"));
    assert_eq!(joined(&links), [(&silas, &grey)]);
    assert_eq!(replay.remaining(), 1, "Mr. Grey's name made a call");
    let related = Refusal::Related { relative: silas, name: "Silas".into() };
    let turned_down = TurnedDown {
        later: key(&logs[1], "father"),
        name: "Mr. Grey".into(),
        chapter: 2,
        candidates: vec![(grey, "Silas Grey".into(), related)],
    };
    assert_eq!(report.turned_down, [turned_down]);
}

#[test]
fn a_candidate_linked_to_a_relative_makes_no_call() {
    let family =
        EventBody::Relation { from: "father".into(), to: "silas".into(), kind: RelationKind::Family, note: "".into() };
    turned_down_as_related(greys(Some((2, family))));
}

#[test]
fn a_relation_of_a_later_chapter_turns_the_candidate_down_too() {
    let family =
        EventBody::Relation { from: "father".into(), to: "silas".into(), kind: RelationKind::Family, note: "".into() };
    turned_down_as_related(greys(Some((3, family))));
    // An overturned relation still names two people.
    let overturned = EventBody::Overturned {
        from: "silas".into(),
        to: "father".into(),
        kind: RelationKind::Family,
        note: "".into(),
    };
    turned_down_as_related(greys(Some((3, overturned))));
}

/// The Greys, the later book with `more` besides Silas's introduction in chapter 1 and Mr. Grey's in chapter 2: each
/// event of the chapter it gives, after the events already in that chapter, with `others` the characters they
/// introduce.
fn greys_with(others: &[&str], more: Vec<(u32, &str, EventBody)>) -> [BookLog; 2] {
    let [earlier, mut later] = greys(None);
    for (chapter, quote, body) in more {
        let block = u32::try_from(later.events.iter().filter(|event| event.chapter == chapter).count()).unwrap();
        later.events.push(event(0, chapter, block, quote, body));
    }
    // In reading order, as a stored log is, and numbered in it. The sort is stable, so a chapter keeps its order.
    later.events.sort_by_key(|event| event.chapter);
    for (seq, event) in (1..).zip(&mut later.events) {
        event.seq = seq;
    }
    later.entities.extend(others.iter().map(|id| Entity { id: (*id).into(), kind: EntityKind::Character }));
    [earlier, later]
}

#[test]
fn a_relation_of_a_character_the_later_one_is_joined_with_turns_the_candidate_down() {
    // The harbourmaster is Silas's father, and turns out to be Mr. Grey: Mr. Grey is Silas's father, though no
    // relation names him.
    let harbourmaster = introduced("elder", name("the harbourmaster", None, None));
    let family =
        EventBody::Relation { from: "elder".into(), to: "silas".into(), kind: RelationKind::Family, note: "".into() };
    let same = EventBody::SameAs { entity: "elder".into(), other: "father".into() };
    turned_down_as_related(greys_with(
        &["elder"],
        vec![
            (1, "The harbourmaster had watched the boy grow up.", harbourmaster),
            (1, "The harbourmaster had never once called Silas his son.", family),
            (3, "The harbourmaster, it turned out, was Mr. Grey.", same),
        ],
    ));
}

#[test]
fn a_relative_joined_with_the_linked_character_turns_the_candidate_down() {
    // The boy is Silas, and Mr. Grey is the boy's father: the link made for Silas is the boy's.
    let boy = introduced("boy", name("the boy", None, None));
    let same = EventBody::SameAs { entity: "silas".into(), other: "boy".into() };
    let family =
        EventBody::Relation { from: "father".into(), to: "boy".into(), kind: RelationKind::Family, note: "".into() };
    turned_down_as_related(greys_with(
        &["boy"],
        vec![
            (1, "A boy ran up the hill ahead of the cart.", boy),
            (1, "The boy was Silas, home at last.", same),
            (3, "Mr. Grey had never once called the boy his son.", family),
        ],
    ));
}

#[test]
fn two_later_characters_that_no_relation_joins_both_link() {
    // An extraction that makes two characters of one is still repaired: both link to the earlier character.
    let logs = greys(None);
    let replay = Replay::new(greys_replies());
    let (links, report) = link(&logs, &replay, None).unwrap();
    let grey = key(&logs[0], "grey");
    assert_eq!(joined(&links), [(&key(&logs[1], "silas"), &grey), (&key(&logs[1], "father"), &grey)]);
    assert_eq!((replay.remaining(), report.turned_down), (0, vec![]));
}

#[test]
fn next_name_lists_every_candidate_its_name_brings() {
    let earlier = book(
        'e',
        "The Marsh Light",
        &["ada", "ned"],
        vec![
            event(
                1,
                1,
                0,
                "Ada Marsh kept the light at Gull Point.",
                introduced("ada", name("Ada Marsh", Some("Ada"), Some("Marsh"))),
            ),
            event(2, 1, 1, "The keeper trimmed the wick at dusk.", alias("ada", name("the keeper", None, None))),
            event(
                3,
                2,
                0,
                "Ned Marsh, her brother, rowed out to the rock.",
                introduced("ned", name("Ned Marsh", Some("Ned"), Some("Marsh"))),
            ),
        ],
    );
    let later = book(
        'f',
        "The Marsh Return",
        &["keeper"],
        vec![
            event(
                1,
                1,
                0,
                "The keeper came down to meet the boat.",
                introduced("keeper", name("the keeper", None, None)),
            ),
            event(
                2,
                3,
                4,
                "Marsh, said the harbourmaster, you are wanted.",
                alias("keeper", name("Marsh", None, Some("Marsh"))),
            ),
        ],
    );
    let logs = [earlier, later];
    let replies = vec![none(), reply("e2", "The keeper came down to meet the boat.")];
    let (links, report) = link(&logs, &Replay::new(replies), None).unwrap();

    let (ada, ned) = ((key(&logs[0], "ada"), "Ada Marsh".to_owned()), (key(&logs[0], "ned"), "Ned Marsh".to_owned()));
    let asked: Vec<&[(EntityKey, String)]> = report.calls.iter().map(|call| call.candidates.as_slice()).collect();
    assert_eq!(asked, [[ada.clone()].as_slice(), [ada, ned.clone()].as_slice()]);
    assert_eq!(verdicts(&report), [Verdict::NoneChosen, Verdict::Linked(ned.0.clone())]);
    let held = Link {
        later: key(&logs[1], "keeper"),
        earlier: ned.0,
        method: LinkMethod::Model { quote: "The keeper came down to meet the boat.".into() },
        chapter: 3,
        at: Position { document: 3, block: 4 },
    };
    assert_eq!(links, [held]);
}

#[test]
fn quotes_stop_at_the_names_chapter() {
    let quote = |seq: u32| format!("The old sailor said his piece, line {seq:02}.");
    // Fifteen events name the sailor by the end of chapter 2, where "Tom" brings a candidate, and three more after.
    let events = (1..=18)
        .map(|seq| {
            let chapter = match seq {
                1..=8 => 1,
                9..=15 => 2,
                _ => 3,
            };
            let body = match seq {
                1 => introduced("sailor", name("the old sailor", None, None)),
                12 => alias("sailor", name("Tom", Some("Tom"), None)),
                _ => appeared("sailor"),
            };
            event(seq, chapter, seq, &quote(seq), body)
        })
        .collect();
    let later = book('e', "The Old Sailor", &["sailor"], events);
    let recorder = Recorder::new(vec![none()]);
    link(&[keeper(), later], &recorder, None).unwrap();

    let asked = recorder.asked.into_inner();
    let [Asked { user, .. }] = asked.as_slice() else { panic!("{asked:#?}") };
    let sent: Vec<u32> = (1..=18).filter(|seq| user.contains(&quote(*seq))).collect();
    assert_eq!(sent, [1, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15], "{user}");
    let at: Vec<usize> = sent.iter().map(|seq| user.find(&quote(*seq)).unwrap()).collect();
    assert!(at.is_sorted(), "{user}");
}

/// A quote of the captain's that his one call sends.
const CAPTAIN_SENT: &str = "He tied up at the jetty and shouted for the keeper.";

/// The first keepers' book and a later book whose Captain Tom Reed makes one call, at chapter 1, listing the first
/// book's Tom Reed as `e1` and sending [`CAPTAIN_SENT`] among his quotes.
fn captain_reed() -> [BookLog; 2] {
    let captain = titled("Captain Tom Reed", Some("Tom"), Some("Reed"), "Captain", None);
    let later = book(
        'e',
        "The Reed Boat",
        &["captain"],
        vec![
            event(1, 1, 0, "Captain Tom Reed brought the boat round the point.", introduced("captain", captain)),
            event(2, 1, 1, CAPTAIN_SENT, appeared("captain")),
            event(3, 2, 0, "The captain sailed home before the storm.", appeared("captain")),
        ],
    );
    [keeper(), later]
}

#[test]
fn quote_not_sent_links_nothing() {
    let logs = captain_reed();
    let sent = CAPTAIN_SENT;
    for (answer, verdict) in [
        (reply("e1", "The captain sailed home before the storm."), Verdict::QuoteNotFound),
        (reply("e1", "round the point. He tied up at the jetty"), Verdict::QuoteNotFound),
        (reply("e1", "Tom"), Verdict::QuoteTooShort),
        (reply("e1", "Captain Tom Reed"), Verdict::QuoteIsAName),
        (reply("e9", sent), Verdict::NotListed),
    ] {
        let (links, report) = link(&logs, &Replay::new(vec![answer.clone()]), None).unwrap();
        assert_eq!((links, verdicts(&report)), (vec![], vec![verdict]), "{answer}");
    }
    // The same call links on a quote it sent, or on part of one.
    for quote in [sent, "tied up at the jetty"] {
        let (links, report) = link(&logs, &Replay::new(vec![reply("e1", quote)]), None).unwrap();
        assert_eq!(verdicts(&report), [Verdict::Linked(key(&logs[0], "tom"))], "{quote}");
        assert_eq!(links.len(), 1, "{quote}");
    }
}

#[test]
fn a_ref_is_read_without_its_case_and_the_spaces_around_it() {
    let logs = captain_reed();
    let tom = key(&logs[0], "tom");
    for (chosen, verdict) in [
        ("E1", Verdict::Linked(tom.clone())),
        (" e1 ", Verdict::Linked(tom.clone())),
        ("\te1\n", Verdict::Linked(tom)),
        // Nothing further from the ref stands for it.
        ("1", Verdict::NotListed),
        ("e1.", Verdict::NotListed),
        ("e 1", Verdict::NotListed),
        ("ｅ1", Verdict::NotListed),
    ] {
        let (_, report) = link(&logs, &Replay::new(vec![reply(chosen, CAPTAIN_SENT)]), None).unwrap();
        let [call] = report.calls.as_slice() else { panic!("{:#?}", report.calls) };
        assert_eq!(call.verdict, verdict, "{chosen:?}");
        // The report keeps the ref as the reply wrote it.
        assert_eq!(call.answer.as_deref(), Some(chosen));
    }
}

#[test]
fn a_reply_that_writes_out_none_chooses_none() {
    let logs = captain_reed();
    for chosen in ["", " ", "none", "None", " NONE\n", "null", "Null", "\tnull "] {
        let (links, report) = link(&logs, &Replay::new(vec![reply(chosen, "")]), None).unwrap();
        let [call] = report.calls.as_slice() else { panic!("{:#?}", report.calls) };
        assert_eq!(call.verdict, Verdict::NoneChosen, "{chosen:?}");
        assert_eq!(links, [], "{chosen:?}");
        assert_eq!(call.answer.as_deref(), Some(chosen));
    }
}

#[test]
fn quote_that_is_a_name_and_little_more_links_nothing() {
    let reed = name("Tom Reed", Some("Tom"), Some("Reed"));
    let later = book(
        'e',
        "The Reed Boat",
        &["reed"],
        vec![
            event(1, 1, 0, "Tom Reed's boat came round the point at noon.", introduced("reed", reed)),
            event(2, 1, 1, "Home at last, said Tom Reed.", appeared("reed")),
            event(3, 1, 2, "On the jetty they still called him the stranger.", appeared("reed")),
            event(4, 1, 3, "Call me Tom, he told the harbourmaster.", alias("reed", name("Tom", Some("Tom"), None))),
        ],
    );
    let logs = [keeper(), later];
    let answered = |logs: &[BookLog], quote: &str| {
        let (links, report) = link(logs, &Replay::new(vec![reply("e1", quote)]), None).unwrap();
        (links.len(), verdicts(&report))
    };
    let tom = key(&logs[0], "tom");
    // Each quote is in what the call sends, and weighs enough on its own: only its names keep it from linking.
    // "Tom" is taken out of "said Tom Reed" only after "Tom Reed", or "Reed" would be left to count as a word. "the
    // stranger" is a name of the candidate alone, which the first book's Tom Reed carries from his same-as.
    let quotes = ["Tom Reed's", "said Tom Reed", "the stranger", "Tom Reed's boat came round the point"];
    let a_name = (0, vec![Verdict::QuoteIsAName]);
    assert_eq!(
        quotes.map(|quote| answered(&logs, quote)),
        [a_name.clone(), a_name.clone(), a_name.clone(), (1, vec![Verdict::Linked(tom)])]
    );

    // In Korean a particle is written onto the name, as 의 is here.
    let korean = |hash_digit: char, title: &str, quote: &str| {
        let tom = name("톰 리드", Some("톰"), Some("리드"));
        book(hash_digit, title, &["tom"], vec![event(1, 1, 0, quote, introduced("tom", tom))])
    };
    let logs = [
        korean('e', "등대지기", "저녁 배의 명단에는 톰 리드라는 이름 하나뿐이었다."),
        korean('f', "등대지기의 귀환", "톰 리드의 배가 정오에 곶을 돌아 들어왔다."),
    ];
    assert_eq!(
        ["톰 리드의", "톰 리드의 배가 정오에"].map(|quote| answered(&logs, quote)),
        [a_name, (1, vec![Verdict::Linked(key(&logs[0], "tom"))])]
    );
}

#[test]
fn third_book_links_through_the_joined_character() {
    let logs = [keeper(), keepers_return(), keepers_light()];
    let mut replies = keepers_replies();
    replies.push(reply("e1", "Captain Reed had the wreck in his glass before anyone."));
    let recorder = Recorder::new(replies);
    let (links, report) = link(&logs, &recorder, None).unwrap();
    assert_eq!(recorder.replay.remaining(), 0);

    let tom = key(&logs[0], "tom");
    let captain = &report.calls[2];
    assert_eq!(
        (&captain.later, captain.candidates.as_slice()),
        (&key(&logs[2], "captain"), [(tom.clone(), "Tom Reed".into())].as_slice())
    );
    let user = &recorder.asked.borrow()[2].user;
    for other in ["the stranger", "the old sailor", "Tom"] {
        assert!(user.contains(&format!("\nother name: {other}\n")), "{other}: {user}");
    }
    assert_eq!((&links[2].later, &links[2].earlier), (&key(&logs[2], "captain"), &tom));

    let cast = fold_series(&logs, None, &links);
    let joined: Vec<&Vec<EntityKey>> = cast
        .characters
        .iter()
        .filter(|character| !character.merged.is_empty())
        .map(|character| &character.merged)
        .collect();
    let toms = vec![key(&logs[0], "stranger"), key(&logs[1], "sailor"), key(&logs[2], "captain")];
    assert_eq!(joined, [&vec![key(&logs[1], "ada")], &toms]);
}

#[test]
fn failed_call_returns_no_links_and_a_rerun_asks_only_the_rest() {
    let cache = TempDir::new().unwrap();
    let logs = [keeper(), keepers_return()];
    let replies = keepers_replies();
    let failed = link(&logs, &Replay::new(replies[..1].to_vec()), Some(cache.path()));
    let Err(Error::Model(message)) = failed else { panic!("expected a model error, got {failed:?}") };
    for part in ["The Keeper's Return", "chapter 3", "Tom"] {
        assert!(message.contains(part), "{message}");
    }

    let replay = Replay::new(replies[1..].to_vec());
    let (links, _) = link(&logs, &replay, Some(cache.path())).unwrap();
    assert_eq!(replay.remaining(), 0);
    assert_eq!(links, link(&logs, &Replay::new(keepers_replies()), None).unwrap().0);
    assert_eq!(links.len(), 2);
}

#[test]
fn reply_that_does_not_fit_is_not_kept() {
    let cache = TempDir::new().unwrap();
    let logs = [keeper(), keepers_return()];
    let unfit = json!({ "candidate": "e1" }).to_string();
    let failed = link(&logs, &Replay::new(vec![unfit.clone(), unfit]), Some(cache.path()));
    assert!(matches!(failed, Err(Error::Model(_))), "{failed:?}");
    let replies = cache.path().join(&logs[1].book.sha256).join("replies");
    assert_eq!(fs::read_dir(replies).map(Iterator::count).unwrap_or(0), 0);

    let replay = Replay::new(keepers_replies());
    let (links, _) = link(&logs, &replay, Some(cache.path())).unwrap();
    assert_eq!((links.len(), replay.remaining()), (2, 0));
}

#[test]
fn report_sums_the_calls() {
    let cache = TempDir::new().unwrap();
    let logs = [keeper(), keepers_return()];
    let mut seen = Vec::new();
    let (_, report) =
        link_order(&logs, &Recorder::new(keepers_replies()), Some(cache.path()), &mut |call| seen.push(call.clone()))
            .unwrap();
    assert_eq!((report.seconds, report.cost_usd), (3.0, Some(0.5)));
    assert_eq!(seen, report.calls);
    // A rerun takes both replies from the cache, at the time and cost they recorded.
    let (_, rerun) = link(&logs, &Replay::new(vec![]), Some(cache.path())).unwrap();
    assert_eq!(rerun, report);

    let (_, replayed) = link(&logs, &Replay::new(keepers_replies()), None).unwrap();
    assert_eq!((replayed.seconds, replayed.cost_usd), (0.0, None));
}
