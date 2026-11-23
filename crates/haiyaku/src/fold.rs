//! The fold: what a run of events says about the characters and how they relate, once every event has been
//! taken in. It is pure and reads nothing but the events it is handed, so what a cast can show is decided
//! wholly by which events the caller passes: stop at the chapter the reader has finished, and nothing later
//! can reach the answer.

use std::collections::BTreeMap;

use crate::log::{BookLog, Event, EventBody, Name, RelationKind};

/// A character across books: an entity's id means nothing outside its book, so the book goes with it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityKey {
    pub book: String,
    pub id: String,
}

/// A chapter of a book. Which of two comes first is for the order the books were folded in to say, as
/// [`Cast::books`] gives it, so this has no ordering of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterRef {
    pub book: String,
    pub chapter: u32,
}

/// What the folded events say: the characters in the order the reader met them, each pair of them the text
/// relates, in the order the pairs first came up, and the books folded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cast {
    pub characters: Vec<Character>,
    pub relations: Vec<Relation>,
    /// Every book started, as a [`ChapterRef`] names it, in the order the books were started, a book with
    /// nothing of it read yet among them. It is what orders the chapters of different books: of two, the one
    /// whose book comes first here is the earlier.
    pub books: Vec<String>,
}

/// A character as the folded events leave it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Character {
    pub key: EntityKey,
    /// The best name known, with the labels of the event that gave it: the fullest by [`Name::rank`] that an
    /// `Introduced` or `Named` event has given it or a character merged into it, and never an alias. A name
    /// takes the place of the one shown only by being fuller, so of names equally full the character keeps
    /// the earliest of its own, and its own over that of a character merged into it.
    pub name: Name,
    /// Every other name it has carried, each text once, in the order the text first gave them, to it or to a
    /// character merged into it. A text given with different labels is here as the first event to give it
    /// any label (a given name, a surname, a title or a title's sex) gave it, and unlabelled where none did.
    pub aliases: Vec<Name>,
    pub first: ChapterRef,
    pub last: ChapterRef,
    /// The characters that turned out to be this one, in the order they were found to be.
    pub merged: Vec<EntityKey>,
}

/// What the text says of two characters, with `a < b`: a pair is held once, whichever way its facts run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Relation {
    pub a: EntityKey,
    pub b: EntityKey,
    /// In the order the text first stated them.
    pub facts: Vec<Fact>,
}

/// One thing the text says of a pair, however often it says it. An overturn closes it: the statements it covers
/// make one fact, marked with it, and a later statement of the same kind starts another.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Fact {
    pub kind: RelationKind,
    /// The end the fact runs from, for a kind that is [directed](RelationKind::directed); `None` for the
    /// others.
    pub from: Option<EntityKey>,
    /// The note of the first event that stated the fact, as `quote` is its quote.
    pub note: String,
    pub quote: String,
    pub first: ChapterRef,
    pub last: ChapterRef,
    /// Set when a later chapter showed the fact was never true; `None` for a fact that stands.
    pub overturned: Option<Overturn>,
}

/// The event that showed a fact was never true.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Overturn {
    /// The chapter whose overturned event covers the fact.
    pub at: ChapterRef,
    pub note: String,
    pub quote: String,
}

/// A chapter as the fold orders it: by the book's place in the order the books were started, then by the
/// chapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    book: usize,
    chapter: u32,
}

/// An event's place in the reading: `seq` orders the events of one chapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Origin {
    place: Place,
    seq: u32,
}

/// A character while events are still coming in.
#[derive(Debug)]
struct Member {
    /// The name shown, as the event that made it the one shown gave it. Another name replaces it only by
    /// outranking it, whether an event gives that name or a merged character brings it, so of equals the one
    /// already shown stays.
    name: Name,
    /// Every name carried, `name`'s text among them, each text once.
    names: Vec<Carried>,
    /// The earliest introduction among this character and those merged into it: where the reader first met
    /// it, which gives both its `first` and its place in the cast.
    introduced: Origin,
    last: Place,
    merged: Vec<EntityKey>,
}

impl Member {
    fn carry(&mut self, name: &Name, at: Origin) {
        self.take(Carried::new(name, at));
    }

    /// Takes a name given to the character or brought by one merged into it. Of each text it keeps the
    /// earliest event to give it and the earliest to label it, whatever order they arrive in.
    fn take(&mut self, carried: Carried) {
        let Some(held) = self.names.iter_mut().find(|held| held.name.text == carried.name.text) else {
            self.names.push(carried);
            return;
        };
        held.first = held.first.min(carried.first);
        if let Some(at) = carried.labelled
            && held.labelled.is_none_or(|labelled| at < labelled)
        {
            held.name = carried.name;
            held.labelled = Some(at);
        }
    }

    /// Takes the name of one of the character's own `Introduced` or `Named` events. Such a name, and the one
    /// a character merged into this one was shown under, are the only names that can become the one it is
    /// shown under.
    fn own(&mut self, name: &Name, at: Origin) {
        if name.rank() > self.name.rank() {
            self.name.clone_from(name);
        }
        self.carry(name, at);
    }
}

/// A name's text as a character carries it. Both of its events are the earliest by the order of the events,
/// not by when the name reached the character: otherwise where a merge is applied would change the order of
/// the aliases and the labels they show.
#[derive(Debug)]
struct Carried {
    /// The event that first gave the text, to the character or to one merged into it: what orders the aliases.
    first: Origin,
    /// The text with the labels of the event at `labelled`, or with none while no event has labelled it.
    name: Name,
    /// The first event to give the text with any label.
    labelled: Option<Origin>,
}

impl Carried {
    fn new(name: &Name, at: Origin) -> Self {
        let labelled =
            name.given.is_some() || name.surname.is_some() || name.title.is_some() || name.title_sex.is_some();
        Self { first: at, name: name.clone(), labelled: labelled.then_some(at) }
    }
}

/// What one event says of a pair: that a fact holds, or, as an overturn, that it never held.
#[derive(Debug)]
struct Claim {
    kind: RelationKind,
    /// As a fact's `from`.
    from: Option<EntityKey>,
    note: String,
    quote: String,
    origin: Origin,
}

/// What the events say of one pair, an entry per event. They are combined into facts only in `finish`: until
/// then a merge can still bring a statement made before an overturn, or an overturn made after statements,
/// and so change which statements make one fact.
#[derive(Debug, Default)]
struct Pair {
    stated: Vec<Claim>,
    overturned: Vec<Claim>,
}

impl Pair {
    /// The facts the statements make, in the order of their first statements. Each overturn covers the
    /// statements of its kind, and for a directed kind of its direction, made before it that no earlier overturn
    /// covers; they make one fact, marked with it. Those after the last overturn make the fact that stands.
    fn facts(mut self, chapter: impl Fn(Place) -> ChapterRef) -> Vec<Fact> {
        self.stated.sort_by_key(|claim| claim.origin);
        self.overturned.sort_by_key(|claim| claim.origin);
        // Each fact with the overturn that covers it, by its index in `overturned`.
        let mut facts: Vec<(Option<usize>, Fact)> = Vec::new();
        for claim in self.stated {
            // Only a directed kind has a `from`, so this matches the kind, and the direction for those.
            let alike = |held: &Claim| held.kind == claim.kind && held.from == claim.from;
            let cover = self.overturned.iter().position(|overturn| alike(overturn) && claim.origin < overturn.origin);
            let at = chapter(claim.origin.place);
            let same = |(covered, fact): &&mut (Option<usize>, Fact)| {
                *covered == cover && fact.kind == claim.kind && fact.from == claim.from
            };
            match facts.iter_mut().find(same) {
                // The statements come in order, so this one is the latest yet.
                Some((_, fact)) => fact.last = at,
                None => {
                    let overturned = cover.map(|index| {
                        let Claim { origin, note, quote, .. } = &self.overturned[index];
                        Overturn { at: chapter(origin.place), note: note.clone(), quote: quote.clone() }
                    });
                    let Claim { kind, from, note, quote, .. } = claim;
                    facts.push((cover, Fact { kind, from, note, quote, first: at.clone(), last: at, overturned }));
                }
            }
        }
        facts.into_iter().map(|(_, fact)| fact).collect()
    }
}

/// Folds events one at a time, across the books of a series.
#[derive(Debug, Default)]
pub struct Folder {
    /// In the order started: a [`Place`] holds an index into this.
    books: Vec<String>,
    /// The characters that stand for themselves. One merged away is a key of `merged_into` instead.
    members: BTreeMap<EntityKey, Member>,
    /// By pair, the lesser key first, and only between members.
    relations: BTreeMap<(EntityKey, EntityKey), Pair>,
    /// Each character merged away, and the one it was merged into, which may itself have been merged since.
    merged_into: BTreeMap<EntityKey, EntityKey>,
}

impl Folder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The events applied from here on are `book`'s, and the ids in them are its ids.
    ///
    /// Books must be started in reading order; chapters within a book in order. Starting a book a second
    /// time is the caller's error: it takes a second, later place in the order, so its chapters no longer
    /// compare as one book's, and `first`, `last` and the order of the cast come out wrong.
    pub fn start_book(&mut self, book: &str) {
        self.books.push(book.to_owned());
    }

    /// Takes in one event of the book last started. An entity that has not been introduced is passed over,
    /// and a relation, an overturn or a merge with one does nothing. Introducing a character again names it,
    /// as a `Named` event would.
    ///
    /// A `SameAs` keeps `entity` and merges `other` into it. Where the two stand for characters of
    /// different books, as one does once a link has joined it to an earlier book's, the character of the
    /// book started first is kept instead.
    ///
    /// # Panics
    ///
    /// When no book has been started: an entity's id names nothing without its book.
    pub fn apply(&mut self, event: &Event) {
        let book = self.books.len().checked_sub(1).expect("Folder::start_book comes before Folder::apply");
        let at = Origin { place: Place { book, chapter: event.chapter }, seq: event.seq };
        match &event.body {
            EventBody::Introduced { entity, name } => {
                let key = self.key(book, entity);
                match self.seen(&key, at.place) {
                    Some(member) => member.own(name, at),
                    None => {
                        let member = Member {
                            name: name.clone(),
                            names: vec![Carried::new(name, at)],
                            introduced: at,
                            last: at.place,
                            merged: Vec::new(),
                        };
                        self.members.insert(key, member);
                    }
                }
            }
            EventBody::Named { entity, name } => {
                if let Some(member) = self.seen(&self.key(book, entity), at.place) {
                    member.own(name, at);
                }
            }
            EventBody::Alias { entity, name } => {
                if let Some(member) = self.seen(&self.key(book, entity), at.place) {
                    member.carry(name, at);
                }
            }
            EventBody::Appeared { entity } => {
                self.seen(&self.key(book, entity), at.place);
            }
            EventBody::Relation { from, to, kind, note } | EventBody::Overturned { from, to, kind, note } => {
                let (from, to) = (self.key(book, from), self.key(book, to));
                let known = [&from, &to].map(|key| self.seen(key, at.place).is_some());
                if known == [true, true] && from != to {
                    let claim = Claim {
                        kind: *kind,
                        from: kind.directed().then(|| from.clone()),
                        note: note.clone(),
                        quote: event.quote.clone(),
                        origin: at,
                    };
                    let pair = self.pair(&from, &to);
                    if matches!(event.body, EventBody::Overturned { .. }) {
                        pair.overturned.push(claim);
                    } else {
                        pair.stated.push(claim);
                    }
                }
            }
            EventBody::SameAs { entity, other } => {
                let (entity, other) = (self.key(book, entity), self.key(book, other));
                self.seen(&entity, at.place);
                self.seen(&other, at.place);
                self.merge(&entity, &other);
            }
        }
    }

    /// Makes `other` one with `survivor`, which takes its names, its relations and their overturns, and the ids
    /// merged into it, and is shown under its name where that name is the fuller of the two. From here on an
    /// event that names `other` is about `survivor`.
    ///
    /// Each of the two is first taken for the character it stands for after the merges so far. Where those
    /// are characters of different books, the one whose book was started first survives, whichever argument
    /// it came in as; within one book it is the first argument.
    ///
    /// Does nothing unless both are in the cast already and are not yet one. A link between books is
    /// therefore applied after the later book's character has been introduced: applied sooner it is lost,
    /// and the two stay apart.
    pub fn merge(&mut self, survivor: &EntityKey, other: &EntityKey) {
        let (mut survivor, mut other) = (self.resolve(survivor.clone()), self.resolve(other.clone()));
        if survivor == other {
            return;
        }
        // Either side may stand, through an earlier merge, for a character of an earlier book than the id
        // it was named by, and that character is the one the series has known the longest: merged away, it
        // would change its key in the middle of a later book, and which of two links came first would
        // decide who is left.
        if self.started(&other) < self.started(&survivor) {
            std::mem::swap(&mut survivor, &mut other);
        }
        let Some(gone) = self.members.remove(&other) else { return };
        let Some(kept) = self.members.get_mut(&survivor) else {
            self.members.insert(other, gone);
            return;
        };
        for carried in gone.names {
            kept.take(carried);
        }
        // The best name known is the fuller of the two, whichever of them the text gave it to and whether
        // that was before the merge or after: a name that comes after reaches the survivor through `own`,
        // under the same rule.
        if gone.name.rank() > kept.name.rank() {
            kept.name = gone.name;
        }
        kept.merged.extend(gone.merged);
        kept.merged.push(other.clone());
        kept.introduced = kept.introduced.min(gone.introduced);
        kept.last = kept.last.max(gone.last);

        let mut moved = Vec::new();
        self.relations.retain(|(a, b), pair| {
            let partner = if *a == other {
                b
            } else if *b == other {
                a
            } else {
                return true;
            };
            moved.push((partner.clone(), std::mem::take(pair)));
            false
        });
        for (partner, mut pair) in moved {
            // The pair of the two that are now one: a character has no relation with itself.
            if partner == survivor {
                continue;
            }
            for claim in pair.stated.iter_mut().chain(&mut pair.overturned) {
                if claim.from.as_ref() == Some(&other) {
                    claim.from = Some(survivor.clone());
                }
            }
            let held = self.pair(&survivor, &partner);
            held.stated.append(&mut pair.stated);
            held.overturned.append(&mut pair.overturned);
        }
        self.merged_into.insert(other, survivor);
    }

    /// The cast the events taken in add up to.
    pub fn finish(self) -> Cast {
        let Self { books, members, relations, .. } = self;
        let chapter = |place: Place| ChapterRef { book: books[place.book].clone(), chapter: place.chapter };

        // The sorts are stable over the maps' order, which is what settles a tie: pairs first related in the
        // same chapter come out by `(a, b)`.
        let mut members: Vec<_> = members.into_iter().collect();
        members.sort_by_key(|(_, member)| member.introduced);
        let characters = members.into_iter().map(|(key, member)| {
            let Member { name, mut names, introduced, last, merged } = member;
            names.sort_by_key(|carried| carried.first);
            let aliases =
                names.into_iter().map(|carried| carried.name).filter(|carried| carried.text != name.text).collect();
            Character { key, name, aliases, first: chapter(introduced.place), last: chapter(last), merged }
        });
        let characters = characters.collect();

        // A pair with overturns and no statement has no fact to show.
        let mut pairs: Vec<_> = relations
            .into_iter()
            .filter_map(|(pair, held)| Some((held.stated.iter().map(|claim| claim.origin).min()?.place, pair, held)))
            .collect();
        pairs.sort_by_key(|(first, ..)| *first);
        let relations = pairs.into_iter().map(|(_, (a, b), held)| Relation { a, b, facts: held.facts(chapter) });
        let relations = relations.collect();

        Cast { characters, relations, books }
    }

    /// The character `id` stands for in the book at `book`: itself, or the survivor of the merges that took
    /// it.
    fn key(&self, book: usize, id: &str) -> EntityKey {
        self.resolve(EntityKey { book: self.books[book].clone(), id: id.to_owned() })
    }

    /// The place of the character's book in the order the books were started.
    fn started(&self, key: &EntityKey) -> Option<usize> {
        self.books.iter().position(|book| *book == key.book)
    }

    fn resolve(&self, mut key: EntityKey) -> EntityKey {
        while let Some(survivor) = self.merged_into.get(&key) {
            key = survivor.clone();
        }
        key
    }

    /// The character an event at `place` names, if it has been introduced. Every event reaches a character
    /// through here, which is what keeps `last` at the latest chapter that names it.
    fn seen(&mut self, key: &EntityKey, place: Place) -> Option<&mut Member> {
        let member = self.members.get_mut(key)?;
        member.last = member.last.max(place);
        Some(member)
    }

    /// What the events so far say of the pair of `x` and `y`, whichever way round they are named.
    fn pair(&mut self, x: &EntityKey, y: &EntityKey) -> &mut Pair {
        let pair = if x <= y { (x.clone(), y.clone()) } else { (y.clone(), x.clone()) };
        self.relations.entry(pair).or_default()
    }
}

/// The cast of one book for a reader who has finished chapter `finished`: the fold of the events of the
/// chapters up to it, and of no later one.
pub fn fold_book(log: &BookLog, finished: u32) -> Cast {
    let mut folder = Folder::new();
    folder.start_book(&log.book.sha256);
    for event in log.events.iter().filter(|event| event.chapter <= finished) {
        folder.apply(event);
    }
    folder.finish()
}
