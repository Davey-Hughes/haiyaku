//! The invented books the integration tests share, a database to put them in, and a one-shot HTTP server.

// Each test binary compiles this module for itself and uses only some of it.
#![allow(dead_code)]

#[cfg(feature = "extract")]
pub mod epub;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

use haiyaku::fold::EntityKey;
use haiyaku::log::{
    BookLog, BookRecord, Chapter, Document, Entity, EntityKind, Event, EventBody, Name, Position, RelationKind, Sex,
};
use haiyaku::store::{Link, LinkMethod, Order, Series};
use rusqlite::{Connection, params};

/// An in-memory database with foreign keys on and the haiyaku tables in it.
pub fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    haiyaku::store::migrate(&conn).unwrap();
    conn
}

/// A server on a free port of 127.0.0.1 that answers one request with `status` and `body`, and hands back the
/// request it read: the request line, the headers and the body. The first value is its `http://127.0.0.1:<port>`.
pub fn serve_once(status: u16, body: &str) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let reason = if (200..300).contains(&status) { "OK" } else { "Error" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    );
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let request = read_request(&stream);
        (&stream).write_all(response.as_bytes()).unwrap();
        request
    });
    (url, server)
}

/// One request off `stream`, read to the end of the body its `content-length` gives.
fn read_request(stream: &TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
        request.push_str(&line);
        if line == "\r\n" || line.is_empty() {
            break;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    request + &String::from_utf8(body).unwrap()
}

/// "The Lighthouse Keeper": the keeper is named in chapter 2, and the stranger of chapter 2 turns out in
/// chapter 3 to be Tom Reed.
pub fn keeper() -> BookLog {
    BookLog {
        book: record('a', "The Lighthouse Keeper", "2026-01-01T00:00:00Z"),
        chapters: vec![
            chapter(1, "The Lamp", 1.5, None, 3, 0),
            chapter(2, "The Jetty", 2.25, Some(0.01), 3, 1),
            chapter(3, "The Evening Boat", 3.0, Some(0.02), 4, 0),
        ],
        documents: documents(),
        entities: characters(&["ada", "pip", "stranger", "tom"]),
        events: vec![
            event(
                1,
                1,
                0,
                "At dusk the keeper climbed to the lamp.",
                introduced("ada", name("the keeper", None, None)),
            ),
            event(
                2,
                1,
                2,
                "Pip was already on the rail, shouting at the sea.",
                introduced("pip", name("Pip", Some("Pip"), None)),
            ),
            event(
                3,
                1,
                3,
                "The keeper fed Pip, her gull, from her own plate.",
                relation("ada", "pip", RelationKind::Owns, "her gull"),
            ),
            event(
                4,
                2,
                1,
                "The letter was addressed to Ada Marsh.",
                EventBody::Named { entity: "ada".into(), name: name("Ada Marsh", Some("Ada"), Some("Marsh")) },
            ),
            event(
                5,
                2,
                4,
                "On the jetty stood the stranger, his coat dripping.",
                introduced("stranger", name("the stranger", None, None)),
            ),
            event(
                6,
                2,
                5,
                "Ada nodded to the stranger, and he nodded back.",
                relation("ada", "stranger", RelationKind::Acquaintance, "met on the jetty"),
            ),
            event(
                7,
                3,
                0,
                "The harbour list had one name for the evening boat: Tom Reed.",
                introduced("tom", name("Tom Reed", Some("Tom"), Some("Reed"))),
            ),
            event(
                8,
                3,
                3,
                "The stranger took off his hat, and it was Tom Reed.",
                EventBody::SameAs { entity: "tom".into(), other: "stranger".into() },
            ),
            event(
                9,
                3,
                4,
                "Ada and Tom Reed had been friends since school.",
                relation("ada", "tom", RelationKind::Acquaintance, "old friends"),
            ),
            event(
                10,
                3,
                7,
                "Good night, Mrs. Marsh, said the boy from the boat.",
                EventBody::Alias {
                    entity: "ada".into(),
                    name: Name {
                        text: "Mrs. Marsh".into(),
                        given: None,
                        surname: Some("Marsh".into()),
                        title: Some("Mrs.".into()),
                        title_sex: Some(Sex::Female),
                    },
                },
            ),
        ],
    }
}

/// [`keeper`] with an eleventh event, in chapter 3, of the one type its story has no use for: a log with an event
/// of every type that version 1 of the schema stores.
pub fn keeper_with_appearance() -> BookLog {
    let mut log = keeper();
    log.events.push(event(11, 3, 9, "Pip slept through the storm.", EventBody::Appeared { entity: "pip".into() }));
    log
}

/// The tables of version 1 of the schema, as a database made before version 2 has them.
const VERSION_ONE: &str = include_str!("../../src/store/schema/001_initial.sql");

/// Makes `conn`'s main database one that a haiyaku at schema version 1 made and stored `log` in. The rows go
/// in by hand: `write_book` reads the stored log back first, through tables such a database does not have.
///
/// # Panics
///
/// When `log` holds an overturned event, which version 1 cannot store.
pub fn version_one(conn: &Connection, log: &BookLog) {
    conn.execute_batch(VERSION_ONE).unwrap();
    // The version row as `migrate` writes it.
    conn.execute_batch(
        "CREATE TABLE main.haiyaku_schema (version INTEGER NOT NULL); INSERT INTO main.haiyaku_schema (version) VALUES (1)",
    )
    .unwrap();
    let (book, sha256) = (&log.book, log.book.sha256.as_str());
    conn.execute(
        "INSERT INTO haiyaku_book VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![sha256, book.title, book.language, book.backend, book.model, book.prompt_version, book.extracted_at],
    )
    .unwrap();
    for c in &log.chapters {
        conn.execute(
            "INSERT INTO haiyaku_chapter VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![sha256, c.number, c.toc_entry, c.title, c.seconds, c.cost_usd, c.kept, c.rejected],
        )
        .unwrap();
    }
    for d in &log.documents {
        conn.execute(
            "INSERT INTO haiyaku_document VALUES (?1, ?2, ?3, ?4)",
            params![sha256, d.ordinal, d.path, d.chapter],
        )
        .unwrap();
    }
    for e in &log.entities {
        let kind = match e.kind {
            EntityKind::Character => "character",
        };
        conn.execute("INSERT INTO haiyaku_entity VALUES (?1, ?2, ?3)", params![sha256, e.id, kind]).unwrap();
    }
    for e in &log.events {
        let event_type = match &e.body {
            EventBody::Introduced { .. } => "introduced",
            EventBody::Named { .. } => "named",
            EventBody::Alias { .. } => "alias",
            EventBody::Appeared { .. } => "appeared",
            EventBody::Relation { .. } => "relation",
            EventBody::SameAs { .. } => "same_as",
            EventBody::Overturned { .. } => panic!("event {} is overturned, which version 1 cannot store", e.seq),
        };
        conn.execute(
            "INSERT INTO haiyaku_event VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![sha256, e.seq, e.chapter, e.at.document, e.at.block, event_type, e.quote],
        )
        .unwrap();
        match &e.body {
            EventBody::Introduced { entity, name }
            | EventBody::Named { entity, name }
            | EventBody::Alias { entity, name } => {
                let sex = name.title_sex.map(|sex| match sex {
                    Sex::Male => "male",
                    Sex::Female => "female",
                });
                let values = params![sha256, e.seq, entity, name.text, name.given, name.surname, name.title, sex];
                conn.execute("INSERT INTO haiyaku_name VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", values)
            }
            EventBody::Appeared { entity } => {
                conn.execute("INSERT INTO haiyaku_appearance VALUES (?1, ?2, ?3)", params![sha256, e.seq, entity])
            }
            EventBody::Relation { from, to, kind, note } => conn.execute(
                "INSERT INTO haiyaku_relation VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![sha256, e.seq, from, to, kind.as_str(), note],
            ),
            EventBody::SameAs { entity, other } => conn
                .execute("INSERT INTO haiyaku_same_as VALUES (?1, ?2, ?3, ?4)", params![sha256, e.seq, entity, other]),
            EventBody::Overturned { .. } => unreachable!("refused above"),
        }
        .unwrap();
    }
}

/// "The Keeper's Return", the book after [`keeper`]: its Ada is the first book's, and its old sailor is the
/// first book's Tom Reed.
pub fn keepers_return() -> BookLog {
    BookLog {
        book: record('b', "The Keeper's Return", "2026-01-02T00:00:00Z"),
        chapters: vec![
            chapter(1, "The Boat", 2.0, Some(0.01), 2, 0),
            chapter(2, "The Lamp Room", 1.75, None, 1, 1),
            chapter(3, "The Name", 2.5, Some(0.03), 1, 0),
        ],
        documents: documents(),
        entities: characters(&["ada", "sailor"]),
        events: vec![
            event(1, 1, 0, "Ada watched the boat come in.", introduced("ada", name("Ada", Some("Ada"), None))),
            event(
                2,
                1,
                1,
                "At the tiller sat the old sailor, grey as the water.",
                introduced("sailor", name("the old sailor", None, None)),
            ),
            event(
                3,
                2,
                2,
                "Ada and the old sailor mended the lamp together.",
                relation("ada", "sailor", RelationKind::Ally, "mend the lamp together"),
            ),
            event(
                4,
                3,
                6,
                "Call me Tom, said the old sailor.",
                EventBody::Alias { entity: "sailor".into(), name: name("Tom", Some("Tom"), None) },
            ),
        ],
    }
}

/// "The Keeper's Light", the book after [`keepers_return`]: its captain is that book's old sailor, and so the
/// first book's Tom Reed.
pub fn keepers_light() -> BookLog {
    BookLog {
        book: record('d', "The Keeper's Light", "2026-01-03T00:00:00Z"),
        chapters: vec![
            chapter(1, "The Wreck", 1.0, None, 1, 0),
            chapter(2, "The Tide", 1.25, Some(0.01), 1, 0),
            chapter(3, "The Light", 1.5, None, 1, 0),
        ],
        documents: documents(),
        entities: characters(&["captain", "girl"]),
        events: vec![
            event(
                1,
                1,
                0,
                "Captain Reed had the wreck in his glass before anyone.",
                introduced(
                    "captain",
                    Name {
                        text: "Captain Reed".into(),
                        given: None,
                        surname: Some("Reed".into()),
                        title: Some("Captain".into()),
                        title_sex: None,
                    },
                ),
            ),
            event(
                2,
                2,
                1,
                "The lamplighter's girl ran down to the tide line.",
                introduced("girl", name("the lamplighter's girl", None, None)),
            ),
            event(
                3,
                3,
                2,
                "Captain Reed carried the girl up to the light.",
                relation("captain", "girl", RelationKind::Ally, "carries her up to the light"),
            ),
        ],
    }
}

/// The series of the two books, in the one order they came out in.
pub fn keepers() -> Series {
    Series {
        name: "keepers".into(),
        title: "The Keepers".into(),
        orders: vec![Order {
            name: "published".into(),
            books: vec![keeper().book.sha256, keepers_return().book.sha256],
        }],
    }
}

/// The links of [`keepers`]' order. Ada is linked on her name where the second book introduces her; the old
/// sailor only where its third chapter calls him Tom.
pub fn keepers_links() -> Vec<Link> {
    let (first, second) = (keeper().book.sha256, keepers_return().book.sha256);
    let key = |book: &str, id: &str| EntityKey { book: book.into(), id: id.into() };
    vec![
        Link {
            later: key(&second, "ada"),
            earlier: key(&first, "ada"),
            method: LinkMethod::Name,
            chapter: 1,
            at: Position { document: 1, block: 0 },
        },
        Link {
            later: key(&second, "sailor"),
            earlier: key(&first, "tom"),
            method: LinkMethod::Model { quote: "Call me Tom, said the old sailor.".into() },
            chapter: 3,
            at: Position { document: 3, block: 6 },
        },
    ]
}

/// The book whose hash is `hash_digit` 64 times over. No fixture takes `c`: it is the tests' book that is
/// not stored.
fn record(hash_digit: char, title: &str, extracted_at: &str) -> BookRecord {
    BookRecord {
        sha256: hash_digit.to_string().repeat(64),
        title: title.into(),
        language: "en".into(),
        backend: "recorded".into(),
        model: "none".into(),
        prompt_version: 1,
        extracted_at: extracted_at.into(),
    }
}

/// Chapter `number`, which is also its entry in the table of contents: entry 0 is the front matter.
fn chapter(number: u32, title: &str, seconds: f64, cost_usd: Option<f64>, kept: u32, rejected: u32) -> Chapter {
    Chapter { number, toc_entry: number, title: title.into(), seconds, cost_usd, kept, rejected }
}

/// Both books' layout: document `n` holds chapter `n`, between front and back matter in no chapter.
fn documents() -> Vec<Document> {
    vec![
        Document { ordinal: 0, path: "front.xhtml".into(), chapter: None },
        Document { ordinal: 1, path: "c1.xhtml".into(), chapter: Some(1) },
        Document { ordinal: 2, path: "c2.xhtml".into(), chapter: Some(2) },
        Document { ordinal: 3, path: "c3.xhtml".into(), chapter: Some(3) },
        Document { ordinal: 4, path: "back.xhtml".into(), chapter: None },
    ]
}

fn characters(ids: &[&str]) -> Vec<Entity> {
    ids.iter().map(|id| Entity { id: (*id).into(), kind: EntityKind::Character }).collect()
}

/// An event of `chapter`, in the document that holds it (see [`documents`]).
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

fn introduced(entity: &str, name: Name) -> EventBody {
    EventBody::Introduced { entity: entity.into(), name }
}

fn relation(from: &str, to: &str, kind: RelationKind, note: &str) -> EventBody {
    EventBody::Relation { from: from.into(), to: to.into(), kind, note: note.into() }
}
