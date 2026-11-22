//! What extraction knows of a book so far, taken wholly from the events it has kept: the characters it has
//! given ids, the names each carries, the chapters each was seen in, and which turned out to be one.

use std::cmp::Reverse;
use std::collections::HashSet;

use super::text::{locate_name, locate_surname, reduce};
use crate::book::ChapterText;
use crate::log::{Entity, EntityKind, Event, EventBody, Name, Position, RelationKind};

/// What extraction knows of the book so far: the characters, their names, the merges, and every event kept.
#[derive(Debug, Default)]
pub struct Roster {
    events: Vec<Event>,
    /// In the order introduced, so the one at index `i` is `c{i + 1}`.
    characters: Vec<Character>,
}

/// A relation the roster holds ([`Roster::held`]), its ends the survivors of the ends it was stated with. For a
/// [directed](RelationKind::directed) kind it runs from `from` to `to`; for the others the two are in no order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Held<'a> {
    pub(crate) from: &'a str,
    pub(crate) to: &'a str,
    pub(crate) kind: RelationKind,
    pub(crate) note: &'a str,
}

impl Held<'_> {
    /// Whether this is the fact a relation of `kind` from `from` to `to` states ([`fact`]).
    pub(crate) fn is_fact(&self, from: &str, kind: RelationKind, to: &str) -> bool {
        fact(self.from, self.kind, self.to) == fact(from, kind, to)
    }
}

/// One id given out, and what its own events say of it. A character merged away keeps its record, and what the
/// roster answers about a character it answers for the survivor ([`Roster::survivor`]), from the records of
/// the survivor and of every character merged into it, but for the name it is shown under, which is the
/// survivor's own.
#[derive(Debug)]
struct Character {
    id: String,
    /// Every name its events gave it, in the order given.
    names: Vec<Name>,
    /// The name it is shown under, as the fold picks it: the name it was introduced under, replaced by a `Named`
    /// name, or by the name a character merged into it was shown under, only when that one outranks it
    /// ([`Name::rank`]), so of two alike the one already shown stays. Only a survivor's is read and kept up to date.
    shown: Name,
    /// The latest chapter an event names it in.
    last: u32,
    /// The character a same-as merged it into, which may have been merged since.
    merged_into: Option<usize>,
}

impl Roster {
    pub fn new() -> Roster {
        Roster::default()
    }

    /// Every event kept, in order: `seq` runs from 1.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Every id given out, merged ones included, in the order given.
    pub fn entities(&self) -> Vec<Entity> {
        self.characters
            .iter()
            .map(|character| Entity { id: character.id.clone(), kind: EntityKind::Character })
            .collect()
    }

    /// The ids a prompt for this chapter lists, in id order: each surviving character one of whose names, or a
    /// surname labelled in one of them, stands in the chapter, or that was last seen in one of the two chapters
    /// before it. A merged-away id is never listed; its survivor carries its names and its chapters. A surname
    /// stands in the chapter as `locate_surname` finds it: of more than one letter or syllable, and in its own
    /// case where its script has case.
    ///
    /// The surname reaches a character the chapter names in a form it has not carried yet: one met as "Mrs.
    /// Marsh" and written "Ada Marsh" here would otherwise go unlisted, and the model would give it a second id.
    pub fn listed(&self, chapter: &ChapterText) -> Vec<String> {
        let number = chapter.number;
        (0..self.characters.len())
            .filter(|&index| self.characters[index].merged_into.is_none())
            .filter(|&root| {
                let last = self.members(root).map(|member| member.last).max().unwrap_or(0);
                let recent = last < number && number <= last.saturating_add(2);
                recent
                    || self.names(root).any(|name| {
                        let surname = name.surname.as_deref();
                        locate_name(chapter, &name.text).is_some()
                            || surname.is_some_and(|surname| locate_surname(chapter, surname).is_some())
                    })
            })
            .map(|root| self.characters[root].id.clone())
            .collect()
    }

    /// The id that stands for `id`'s character now: `id` itself, or the survivor of the same-as that merged it
    /// away, followed through every later merge.
    pub(crate) fn survivor<'a>(&'a self, id: &'a str) -> &'a str {
        match self.index(id) {
            Some(index) => &self.characters[self.root(index)].id,
            None => id,
        }
    }

    /// The relations the roster holds, as both calls' prompts list them and an overturn is checked against them:
    /// each fact ([`fact`]) stated since the last overturn of it, once, under the note first given since then, in
    /// the order first stated since then.
    ///
    /// Each end is taken for its survivor first ([`Roster::survivor`]), the ends of an overturn too, so a merge does
    /// not drop a relation, and a relation whose two ends are now one character is held by none. An overturn covers
    /// every statement of its fact made before it, as the fold has it: the fact leaves the list until a later
    /// chapter states it again.
    pub(crate) fn held(&self) -> Vec<Held<'_>> {
        // Each overturn's fact, with its place in the log: it covers a statement placed before it.
        let overturns: Vec<(usize, (&str, &str, &str))> = self
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.body {
                EventBody::Overturned { from, to, kind, .. } => {
                    Some((index, fact(self.survivor(from), *kind, self.survivor(to))))
                }
                _ => None,
            })
            .collect();
        let mut facts = HashSet::new();
        let mut held = Vec::new();
        for (index, event) in self.events.iter().enumerate() {
            let EventBody::Relation { from, to, kind, note } = &event.body else { continue };
            let (from, to) = (self.survivor(from), self.survivor(to));
            let stated = fact(from, *kind, to);
            let covered = overturns.iter().any(|(at, overturned)| *at > index && *overturned == stated);
            if from != to && !covered && facts.insert(stated) {
                held.push(Held { from, to, kind: *kind, note });
            }
        }
        held
    }

    /// The id the next character introduced is given.
    pub(crate) fn next_id(&self) -> String {
        format!("c{}", self.characters.len() + 1)
    }

    /// The surviving character that carries a full name with `name`'s given name and surname, the lowest id when
    /// several do; `None` also when `name` is not a full name.
    pub(crate) fn holder(&self, name: &Name) -> Option<&str> {
        self.holders(name).min().map(|root| self.characters[root].id.as_str())
    }

    /// Whether a surviving character other than `id`'s carries a full name with `name`'s given name and surname.
    pub(crate) fn held_by_other(&self, id: &str, name: &Name) -> bool {
        let own = self.index(id).map(|index| self.root(index));
        self.holders(name).any(|root| Some(root) != own)
    }

    /// Whether `id`'s character carries a name whose text is `text`, compared reduced.
    pub(crate) fn carries(&self, id: &str, text: &str) -> bool {
        let text = reduce(text);
        self.index(id).is_some_and(|index| self.names(self.root(index)).any(|name| reduce(&name.text) == text))
    }

    /// `a` and `b`, two surviving characters, in the order a same-as names them: the one it keeps, then the one
    /// it merges away. It keeps the one shown under the higher-ranked name ([`Name::rank`]), and of two ranked
    /// alike the one introduced first, so "the stranger" joins Tom Reed and not the other way round.
    pub(crate) fn better_named<'a>(&self, a: &'a str, b: &'a str) -> (&'a str, &'a str) {
        let standing = |id: &str| (Reverse(self.shown_rank(id)), self.index(id));
        if standing(b) < standing(a) { (b, a) } else { (a, b) }
    }

    /// The rank of the name `id`'s character is shown under now.
    pub(crate) fn shown_rank(&self, id: &str) -> u8 {
        self.index(id).map_or(0, |index| self.characters[self.root(index)].shown.rank())
    }

    /// The name `id`'s character is shown under now, and every other name it carries, each once: its own and
    /// those of the characters merged into it, the earliest introduced first and each one's in the order given.
    /// `None` for an id never given out.
    pub(crate) fn names_of(&self, id: &str) -> Option<(&str, Vec<&str>)> {
        let root = self.root(self.index(id)?);
        let shown = self.characters[root].shown.text.as_str();
        let mut others: Vec<&str> = Vec::new();
        for name in self.names(root) {
            if name.text != shown && !others.contains(&name.text.as_str()) {
                others.push(&name.text);
            }
        }
        Some((shown, others))
    }

    /// Appends an event of chapter `chapter`, numbered after the last, and takes in what it says. An
    /// `Introduced` event gives out [`Roster::next_id`] and no other id: the character at index `i` is
    /// `c{i + 1}`, so an id given out of turn would be given out again.
    pub(crate) fn push(&mut self, chapter: u32, at: Position, quote: String, body: EventBody) {
        match &body {
            EventBody::Introduced { entity, name } => {
                debug_assert_eq!(*entity, self.next_id(), "an introduction gives out the next id");
                self.characters.push(Character {
                    id: entity.clone(),
                    names: vec![name.clone()],
                    shown: name.clone(),
                    last: chapter,
                    merged_into: None,
                });
            }
            EventBody::Named { entity, name } => self.give(entity, name, true),
            EventBody::Alias { entity, name } => self.give(entity, name, false),
            EventBody::Appeared { .. } | EventBody::Relation { .. } | EventBody::Overturned { .. } => {}
            EventBody::SameAs { entity, other } => {
                if let (Some(survivor), Some(other)) = (self.index(entity), self.index(other)) {
                    let (survivor, other) = (self.root(survivor), self.root(other));
                    if survivor != other {
                        self.characters[other].merged_into = Some(survivor);
                        if self.characters[other].shown.rank() > self.characters[survivor].shown.rank() {
                            self.characters[survivor].shown = self.characters[other].shown.clone();
                        }
                    }
                }
            }
        }
        for id in named_ids(&body) {
            if let Some(index) = self.index(id) {
                let character = &mut self.characters[index];
                character.last = character.last.max(chapter);
            }
        }
        let seq = self.events.last().map_or(1, |event| event.seq + 1);
        self.events.push(Event { seq, chapter, at, quote, body });
    }

    /// Adds `name` to the names of `id`'s own record. A name it may be `shown` under replaces the one its
    /// character is shown under when it outranks that one.
    fn give(&mut self, id: &str, name: &Name, shown: bool) {
        if let Some(index) = self.index(id) {
            let root = self.root(index);
            if shown && name.rank() > self.characters[root].shown.rank() {
                self.characters[root].shown = name.clone();
            }
            self.characters[index].names.push(name.clone());
        }
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.characters.iter().position(|character| character.id == id)
    }

    /// The index of the character that stands for the one at `index` now.
    fn root(&self, mut index: usize) -> usize {
        while let Some(next) = self.characters[index].merged_into {
            index = next;
        }
        index
    }

    /// The characters the one at `root` stands for: itself and every character merged into it.
    fn members(&self, root: usize) -> impl Iterator<Item = &Character> {
        (0..self.characters.len()).filter(move |&index| self.root(index) == root).map(|index| &self.characters[index])
    }

    /// Every name the character at `root` carries, its own and those of the characters merged into it.
    fn names(&self, root: usize) -> impl Iterator<Item = &Name> {
        self.members(root).flat_map(|member| &member.names)
    }

    /// The surviving characters that carry a full name with `name`'s given name and surname, compared reduced; none
    /// when `name` is not a full name.
    fn holders(&self, name: &Name) -> impl Iterator<Item = usize> {
        let wanted = full_name(name);
        (0..self.characters.len())
            .filter(move |&index| {
                wanted.is_some() && self.characters[index].names.iter().any(|carried| full_name(carried) == wanted)
            })
            .map(|index| self.root(index))
    }
}

/// A name's given name and surname, reduced, when it has both.
fn full_name(name: &Name) -> Option<(String, String)> {
    Some((reduce(name.given.as_deref()?), reduce(name.surname.as_deref()?)))
}

/// The fact a relation of `kind` from `from` to `to` states, as a key: a kind that is not directed holds between the
/// two alike, so its fact is one whichever way round it is stated, as the fold keeps it.
pub(crate) fn fact<'a>(from: &'a str, kind: RelationKind, to: &'a str) -> (&'static str, &'a str, &'a str) {
    if kind.directed() || from < to { (kind.as_str(), from, to) } else { (kind.as_str(), to, from) }
}

/// The ids an event names.
fn named_ids(body: &EventBody) -> Vec<&str> {
    match body {
        EventBody::Introduced { entity, .. }
        | EventBody::Named { entity, .. }
        | EventBody::Alias { entity, .. }
        | EventBody::Appeared { entity } => vec![entity],
        EventBody::Relation { from, to, .. } | EventBody::Overturned { from, to, .. } => vec![from, to],
        EventBody::SameAs { entity, other } => vec![entity, other],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fold::Folder;

    fn name(text: &str, given: Option<&str>, surname: Option<&str>) -> Name {
        Name {
            text: text.into(),
            given: given.map(Into::into),
            surname: surname.map(Into::into),
            title: None,
            title_sex: None,
        }
    }

    /// A same-as that keeps the worse named of two shows the survivor under the other's name, and a name given
    /// to a character before the merge stays among its other names, as the fold has them.
    #[test]
    fn names_are_the_folds() {
        let mut roster = Roster::new();
        let mut push = |chapter, body| roster.push(chapter, Position { document: 1, block: 0 }, "quote".into(), body);
        push(1, EventBody::Introduced { entity: "c1".into(), name: name("the stranger", None, None) });
        push(1, EventBody::Introduced { entity: "c2".into(), name: name("Tom Reed", Some("Tom"), Some("Reed")) });
        push(1, EventBody::Named { entity: "c1".into(), name: name("Tom", Some("Tom"), None) });
        push(2, EventBody::SameAs { entity: "c1".into(), other: "c2".into() });

        let mut folder = Folder::new();
        folder.start_book("book");
        roster.events().iter().for_each(|event| folder.apply(event));
        let cast = folder.finish();
        let [character] = cast.characters.as_slice() else { panic!("{cast:?}") };
        let aliases: Vec<&str> = character.aliases.iter().map(String::as_str).collect();
        assert_eq!(roster.names_of("c1"), Some((character.name.as_str(), aliases.clone())));
        assert_eq!(roster.names_of("c2"), Some((character.name.as_str(), aliases)));
        assert_eq!(character.name, "Tom Reed");
    }

    /// An id given out of turn would leave `next_id` to give it out a second time.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the next id")]
    fn introduction_gives_out_the_next_id() {
        let name = Name { text: "Pip".into(), given: Some("Pip".into()), surname: None, title: None, title_sex: None };
        let body = EventBody::Introduced { entity: "c2".into(), name };
        Roster::new().push(1, Position { document: 1, block: 0 }, "Pip".into(), body);
    }
}
