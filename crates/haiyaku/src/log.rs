//! A book's log as values in memory: the record of the book, its chapters, documents and entities, and its
//! events in reading order. [`crate::store`] writes a log to the database and reads it back.

/// Everything stored about one extracted book.
#[derive(Debug, Clone, PartialEq)]
pub struct BookLog {
    pub book: BookRecord,
    /// In reading order, numbered 1, 2, 3…, each with at least one document.
    pub chapters: Vec<Chapter>,
    /// Taken by `ordinal`, the chapters of those that have one never go back, and each is one of `chapters`.
    pub documents: Vec<Document>,
    /// Every entity an event names.
    pub entities: Vec<Entity>,
    /// In reading order: `seq` runs 1, 2, 3… and the chapters never go back. Each is in one of `documents`,
    /// and in that document's chapter.
    ///
    /// [`crate::store::write_book`] refuses a log that breaks one of these rules, here or on `chapters` and
    /// `documents`: they are what lets a reader's document stand for the chapters they have finished.
    pub events: Vec<Event>,
}

/// What a book is and how its log was made.
#[derive(Debug, Clone, PartialEq)]
pub struct BookRecord {
    /// The SHA-256 of the EPUB file, in lowercase hex: what identifies the book everywhere.
    pub sha256: String,
    pub title: String,
    pub language: String,
    pub backend: String,
    pub model: String,
    pub prompt_version: u32,
    /// RFC 3339, in UTC.
    pub extracted_at: String,
}

/// A chapter chosen for extraction, and what its extraction took.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    /// From 1, in reading order.
    pub number: u32,
    /// The chapter's index in the book's table of contents.
    pub toc_entry: u32,
    pub title: String,
    pub seconds: f64,
    /// `None` when the backend reports no cost.
    pub cost_usd: Option<f64>,
    /// How many of the model's items became events.
    pub kept: u32,
    /// How many the program turned down.
    pub rejected: u32,
}

/// A document of the EPUB's spine.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Spine order, from 0.
    pub ordinal: u32,
    pub path: String,
    /// `None` for a document outside every chosen chapter: front matter, back matter.
    pub chapter: Option<u32>,
}

/// Something the events are about. Its `id` is unique within its book and means nothing outside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: String,
    pub kind: EntityKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Character,
}

impl EntityKind {
    /// The name stored in SQL.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Character => "character",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "character" => Some(Self::Character),
            _ => None,
        }
    }
}

/// A block of a document: where a quote starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    /// The document's `ordinal`.
    pub document: u32,
    pub block: u32,
}

/// One thing the text says, with the words that say it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// From 1, in reading order through the whole book.
    pub seq: u32,
    pub chapter: u32,
    /// The block holding the start of `quote`.
    pub at: Position,
    /// The evidence, in the book's own words.
    pub quote: String,
    pub body: EventBody,
}

/// What an event says. Entities are named by their ids.
#[derive(Debug, Clone, PartialEq)]
pub enum EventBody {
    /// A new entity, under the name the text first gives it.
    Introduced { entity: String, name: Name },
    /// The text now gives a fuller name.
    Named { entity: String, name: Name },
    /// Another name or form of address.
    Alias { entity: String, name: Name },
    /// The entity is present in this chapter.
    Appeared { entity: String },
    /// A fact about two entities. For a kind that is [directed](RelationKind::directed) it runs from `from`
    /// to `to`; for the others the two are in no order.
    Relation { from: String, to: String, kind: RelationKind, note: String },
    /// Two entities turn out to be one: `other` is `entity`.
    SameAs { entity: String, other: String },
    /// The chapter shows that the relation of `kind` between the two, recorded before this event, was never
    /// true; for a [directed](RelationKind::directed) kind it runs from `from` to `to`, for the others the two
    /// are in no order.
    Overturned { from: String, to: String, kind: RelationKind, note: String },
}

/// A name as the text gives it, and the parts of it that could be told apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    pub text: String,
    pub given: Option<String>,
    pub surname: Option<String>,
    pub title: Option<String>,
    /// The sex a title states, as "Mrs." does.
    pub title_sex: Option<Sex>,
}

impl Name {
    /// How much of a proper name this is: 2 with a given name and a surname, 1 with one of them, 0 with
    /// neither, which is a description ("the keeper"). A title adds nothing.
    pub fn rank(&self) -> u8 {
        u8::from(self.given.is_some()) + u8::from(self.surname.is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

impl Sex {
    /// The name stored in SQL.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Male => "male",
            Self::Female => "female",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "male" => Some(Self::Male),
            "female" => Some(Self::Female),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    Family,
    Romance,
    Friend,
    Ally,
    Rival,
    Enemy,
    Mentor,
    Serves,
    Colleague,
    Acquaintance,
    Owns,
}

impl RelationKind {
    /// Every kind, in the order a reply's schema and the prompt list them.
    pub const ALL: [Self; 11] = [
        Self::Family,
        Self::Romance,
        Self::Friend,
        Self::Ally,
        Self::Rival,
        Self::Enemy,
        Self::Mentor,
        Self::Serves,
        Self::Colleague,
        Self::Acquaintance,
        Self::Owns,
    ];

    /// Whether a relation of this kind runs from one entity to the other (a mentor of, a servant of, an
    /// owner of) rather than holding between the two alike.
    pub fn directed(self) -> bool {
        matches!(self, Self::Mentor | Self::Serves | Self::Owns)
    }

    /// The name stored in SQL, one of those the `CHECK`s of `haiyaku_relation` and `haiyaku_overturn` list: a
    /// name that is not there cannot be written.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Family => "family",
            Self::Romance => "romance",
            Self::Friend => "friend",
            Self::Ally => "ally",
            Self::Rival => "rival",
            Self::Enemy => "enemy",
            Self::Mentor => "mentor",
            Self::Serves => "serves",
            Self::Colleague => "colleague",
            Self::Acquaintance => "acquaintance",
            Self::Owns => "owns",
        }
    }

    /// The kind [`as_str`](Self::as_str) names, or `None` for any other string.
    pub fn parse(name: &str) -> Option<Self> {
        // Through `as_str`, so the two cannot come to disagree on a name.
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}
