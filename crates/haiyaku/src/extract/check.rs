//! From a model's reply to events: every item checked against the chapter's text and the roster before it
//! enters the log, and every character given the program's own id.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::roster::Roster;
use super::text::{QuoteError, locate_name, locate_quote, reduce};
use crate::book::ChapterText;
use crate::log::{Event, EventBody, Name, Position, RelationKind, Sex};

/// The JSON schema of a reply, every field required, no extra properties.
pub fn schema() -> Value {
    let string = json!({ "type": "string" });
    let label = json!({ "type": ["string", "null"] });
    let name = object(json!({
        "text": string,
        "given": label,
        "surname": label,
        "title": label,
        "title_sex": { "type": ["string", "null"], "enum": ["male", "female", null] },
    }));
    let kind = json!({ "type": "string", "enum": RelationKind::ALL.map(RelationKind::as_str) });
    object(json!({
        "characters": array(object(json!({ "ref": string, "name": name, "aliases": array(name.clone()), "evidence": string }))),
        "relations": array(object(json!({ "from": string, "to": string, "kind": kind, "note": string, "evidence": string }))),
        "same_as": array(object(json!({ "ref": string, "other": string, "evidence": string }))),
    }))
}

/// The schema of an object with these properties, every one required and no other allowed.
fn object(properties: Value) -> Value {
    let required: Vec<String> =
        properties.as_object().map(|fields| fields.keys().cloned().collect()).unwrap_or_default();
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

fn array(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

/// What became of a reply.
#[derive(Debug)]
pub struct Outcome {
    /// This chapter's events, already appended to the roster.
    pub events: Vec<Event>,
    /// How many of the reply's items gave at least one event.
    pub kept: u32,
    /// The items turned down, and the single names turned down from items otherwise kept, in the order met.
    pub rejected: Vec<Rejected>,
    /// How often the check did each thing it notes to an item it did not turn down: a label cleared, a name or a
    /// quote replaced, a handle matched to a known character by its full name.
    pub notes: BTreeMap<&'static str, u32>,
}

/// What of a reply became no event: an item, one name of an item, a field that should hold items and does not,
/// or the whole reply when it is not an object.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected {
    pub reason: &'static str,
    /// As the reply wrote it.
    pub item: Value,
}

/// Checks a model's reply about `chapter` against the chapter's text and `roster`, the book so far, and appends
/// to the roster the events its sound items come to.
///
/// A `ref` names a known character only when it is one of the ids `roster.listed(chapter)` gave before the reply
/// changed the roster: the model was shown those and no others, so a made-up id beside the listed ones must not
/// land on the unlisted character that holds it. Any other `ref` is a handle for a new character, and means that
/// character only within this reply. Nothing the model calls a character reaches the log: every character gets
/// the next id of the program's own, `c1`, `c2`, ... in the order introduced.
///
/// A new character is introduced under the first of its names the chapter holds, and an item none of whose
/// names the chapter holds is turned down. When that name is a full name (a given name and a surname), the
/// handle stands for the character that already carries it, and when none does the name is a new character's,
/// whatever its aliases say, so a mistaken alias that is another character's full name does not pull the new
/// character into that one. A lesser name, a lone given name or surname or a description, cannot tell who it
/// is: the first of the item's names the chapter holds, tried in order, that is a full name a character carries
/// makes the handle stand for that character. Nothing less than a full name joins two, since a title, an alias
/// or a surname alone is shared within a family. A known character appears once a chapter, and not in the
/// chapter that introduced it.
///
/// Each name of an item that its character does not carry yet becomes a `Named` event when it is the item's
/// `name` and outranks ([`Name::rank`]) the name the character is shown under, and an `Alias` otherwise: the
/// fold shows a name by the same comparison, so an alias of equal rank never keeps a description shown. A full
/// name another character carries is turned down for this one, so no two characters carry one full name.
///
/// An introduction or an appearance is placed by the item's evidence; when the chapter does not hold it, by
/// the first of the item's names the chapter does hold, so a sound character does not fall over a quote the
/// model re-worded. A `Named` or `Alias` event quotes the name itself at the first block it stands in.
///
/// A relation or a same-as names its two characters as a character item does, and is turned down when either
/// is unknown or the two are one. A relation keeps the reply's direction. A same-as keeps the character shown
/// under the higher-ranked name, and of two ranked alike the one introduced first, and merges the other into
/// it: from then on the survivor carries the names of both. Each is placed by its own evidence and nothing
/// else, since where the chapter names a character says nothing of where it says the two are related or one.
///
/// A reply that is not a JSON object is turned down whole.
pub fn check(roster: &mut Roster, chapter: &ChapterText, reply: &Value) -> Outcome {
    let first = roster.events().len();
    let mut checker = Checker {
        listed: roster.listed(chapter),
        roster,
        chapter,
        handles: BTreeMap::new(),
        here: BTreeSet::new(),
        kept: 0,
        rejected: Vec::new(),
        notes: BTreeMap::new(),
        pending: Vec::new(),
    };
    if reply.is_object() {
        for item in checker.items(reply, "characters") {
            let verdict = checker.character(item);
            checker.settle(item, verdict);
        }
        for item in checker.items(reply, "relations") {
            let verdict = checker.relation(item);
            checker.settle(item, verdict);
        }
        for item in checker.items(reply, "same_as") {
            let verdict = checker.same_as(item);
            checker.settle(item, verdict);
        }
    } else {
        checker.reject("malformed", reply);
    }
    let Checker { roster, kept, rejected, notes, .. } = checker;
    Outcome { events: roster.events()[first..].to_vec(), kept, rejected, notes }
}

/// One reply on its way through [`check`].
struct Checker<'a> {
    roster: &'a mut Roster,
    chapter: &'a ChapterText,
    /// The ids the prompt listed, the only refs that name a known character.
    listed: Vec<String>,
    /// Each handle of this reply that stands for a character, and its id.
    handles: BTreeMap<String, String>,
    /// The characters introduced or appeared in this chapter.
    here: BTreeSet<String>,
    kept: u32,
    rejected: Vec<Rejected>,
    notes: BTreeMap<&'static str, u32>,
    /// What was noted on the item being checked, counted in `notes` once the item is not turned down.
    pending: Vec<&'static str>,
}

/// What became of one item: `Ok(true)` when it gave an event, `Ok(false)` when it gave none and is not turned
/// down (a known character listed again with nothing new), and `Err` with the reason it is turned down.
type Verdict = Result<bool, &'static str>;

impl Checker<'_> {
    /// The items of the array at `key`: none when the reply has no such field, and none, with the field turned
    /// down, when it is not an array.
    fn items<'r>(&mut self, reply: &'r Value, key: &str) -> &'r [Value] {
        match reply.get(key) {
            None => &[],
            Some(Value::Array(items)) => items,
            Some(other) => {
                self.reject("malformed", other);
                &[]
            }
        }
    }

    fn character(&mut self, item: &Value) -> Verdict {
        let parsed = CharacterItem::parse(item).ok_or("malformed")?;
        let names: Vec<Name> = parsed.names.iter().map(|(name, _)| self.tidy(name)).collect();
        // The first of the item's names the chapter holds, by its place among them, and where it stands.
        let first =
            names.iter().enumerate().find_map(|(index, name)| Some((index, locate_name(self.chapter, &name.text)?)));

        // The character the item is about when it is a known one, and the name that chose it or introduces it
        // when the ref is a handle.
        let (known, chosen) = match self.known(parsed.reference) {
            Some(id) => (Some(id), None),
            None => {
                let Some((chosen, _)) = first else { return Err("name_not_in_text") };
                if chosen > 0 {
                    self.note("name_replaced_by_alias");
                }
                // The chosen name's holder comes first; only a full name, a given name and a surname, can have
                // one. A chosen full name that no one holds is a new character's, whatever its aliases say: a
                // mistaken alias that is another character's full name would otherwise pull the new character
                // into that one. A lesser name, a lone given name or surname or a description, cannot tell who it
                // is, so it is matched through its aliases: a known character's full name among them would
                // otherwise introduce a second character, and then be turned down for it as held by the first.
                let chosen_name = &names[chosen];
                let holder = match self.roster.holder(chosen_name) {
                    Some(holder) => Some(holder),
                    None if chosen_name.rank() < 2 => names
                        .iter()
                        .filter(|name| locate_name(self.chapter, &name.text).is_some())
                        .find_map(|name| self.roster.holder(name)),
                    None => None,
                }
                .map(str::to_owned);
                if holder.is_some() {
                    self.note("matched_by_name");
                }
                (holder, Some(chosen))
            }
        };
        let introduces = known.is_none();
        let id = known.unwrap_or_else(|| self.roster.next_id());

        let mut gave = false;
        if !self.here.contains(&id) {
            let fallback = first.as_ref().map(|(index, at)| (&names[*index], at));
            let (quote, at) = self.quote(parsed.evidence, fallback).map_err(quote_rejection)?;
            let body = match chosen.filter(|_| introduces) {
                Some(chosen) => EventBody::Introduced { entity: id.clone(), name: names[chosen].clone() },
                None => EventBody::Appeared { entity: id.clone() },
            };
            self.roster.push(self.chapter.number, at, quote, body);
            self.here.insert(id.clone());
            gave = true;
        }
        if chosen.is_some() {
            self.handles.insert(parsed.reference.to_owned(), id.clone());
        }

        for (index, (name, (_, written))) in names.iter().zip(&parsed.names).enumerate() {
            // The name an introduction gave is carried by now. The name that matched a known character may not
            // be, since only its given name and surname are compared: "Hal Timothy Brook" matches "Hal Brook",
            // and is given to the character as any other name it lacks.
            if self.roster.carries(&id, &name.text) {
                continue;
            }
            let Some(at) = locate_name(self.chapter, &name.text) else {
                self.reject("alias_not_in_text", written);
                continue;
            };
            if self.roster.held_by_other(&id, name) {
                self.reject("name_held_by_other", written);
                continue;
            }
            let (entity, quote) = (id.clone(), name.text.clone());
            let body = if index == 0 && name.rank() > self.roster.shown_rank(&id) {
                EventBody::Named { entity, name: name.clone() }
            } else {
                EventBody::Alias { entity, name: name.clone() }
            };
            self.roster.push(self.chapter.number, at, quote, body);
            gave = true;
        }
        Ok(gave)
    }

    /// A relation item: both its characters known and not one, its kind one of [`RelationKind::ALL`] once
    /// trimmed and lowercased, and its evidence in the chapter. Its note is kept trimmed, with each run of
    /// whitespace, line breaks included, made one space: later prompts list it as a cell of one line.
    fn relation(&mut self, item: &Value) -> Verdict {
        let [from, to, kind, note, evidence] =
            strings(item, ["from", "to", "kind", "note", "evidence"]).ok_or("malformed")?;
        let (Some(from), Some(to)) = (self.known(from), self.known(to)) else { return Err("unknown_ref") };
        if from == to {
            return Err("self_relation");
        }
        let kind = RelationKind::parse(&kind.trim().to_lowercase()).ok_or("bad_kind")?;
        let at = locate_quote(self.chapter, evidence).map_err(quote_rejection)?;
        let note = note.split_whitespace().collect::<Vec<_>>().join(" ");
        let body = EventBody::Relation { from, to, kind, note };
        self.roster.push(self.chapter.number, at, evidence.to_owned(), body);
        Ok(true)
    }

    /// A same-as item: both its characters known and not one, and its evidence in the chapter. It merges the
    /// worse named of the two into the better ([`Roster::better_named`]).
    fn same_as(&mut self, item: &Value) -> Verdict {
        let [reference, other, evidence] = strings(item, ["ref", "other", "evidence"]).ok_or("malformed")?;
        let (Some(a), Some(b)) = (self.known(reference), self.known(other)) else { return Err("unknown_ref") };
        if a == b {
            return Err("self_same_as");
        }
        let at = locate_quote(self.chapter, evidence).map_err(quote_rejection)?;
        let (entity, other) = self.roster.better_named(&a, &b);
        let body = EventBody::SameAs { entity: entity.to_owned(), other: other.to_owned() };
        self.roster.push(self.chapter.number, at, evidence.to_owned(), body);
        Ok(true)
    }

    /// Takes in `item`'s verdict: the item kept when it gave an event, or turned down with its reason. What was
    /// noted on it counts only when it is not turned down.
    fn settle(&mut self, item: &Value, verdict: Verdict) {
        let pending = std::mem::take(&mut self.pending);
        match verdict {
            Ok(gave) => {
                self.kept += u32::from(gave);
                for note in pending {
                    *self.notes.entry(note).or_default() += 1;
                }
            }
            Err(reason) => self.reject(reason, item),
        }
    }

    /// The character `reference` names when it is a known one: an id the prompt listed, or a handle this reply
    /// has already given a character.
    fn known(&self, reference: &str) -> Option<String> {
        let id = match self.listed.iter().find(|listed| *listed == reference) {
            Some(listed) => listed,
            None => self.handles.get(reference)?,
        };
        Some(self.roster.survivor(id).to_owned())
    }

    /// `name` with its text trimmed, each labelled part that its text does not hold cleared, and the sex of a
    /// title kept only beside the title.
    fn tidy(&mut self, name: &Name) -> Name {
        let text = name.text.trim();
        let whole = reduce(text);
        let mut label = |part: &Option<String>| {
            let part = part.as_deref()?.trim();
            let reduced = reduce(part);
            if !reduced.is_empty() && whole.contains(&reduced) {
                Some(part.to_owned())
            } else {
                self.note("label_not_in_name");
                None
            }
        };
        let (given, surname, title) = (label(&name.given), label(&name.surname), label(&name.title));
        let title_sex = name.title_sex.filter(|_| title.is_some());
        Name { text: text.to_owned(), given, surname, title, title_sex }
    }

    /// The quote and block of an introduction or an appearance: the evidence where the chapter holds it, else
    /// `fallback`, a name of the item as the reply writes it and the block it first stands in.
    fn quote(
        &mut self,
        evidence: &str,
        fallback: Option<(&Name, &Position)>,
    ) -> Result<(String, Position), QuoteError> {
        match locate_quote(self.chapter, evidence) {
            Ok(at) => Ok((evidence.to_owned(), at)),
            Err(error) => {
                let (name, at) = fallback.ok_or(error)?;
                self.note("evidence_replaced");
                Ok((name.text.clone(), at.clone()))
            }
        }
    }

    fn reject(&mut self, reason: &'static str, item: &Value) {
        self.rejected.push(Rejected { reason, item: item.clone() });
    }

    fn note(&mut self, note: &'static str) {
        self.pending.push(note);
    }
}

/// The reason a quote that is not placed gives for turning its item down.
fn quote_rejection(error: QuoteError) -> &'static str {
    match error {
        QuoteError::TooShort => "evidence_too_short",
        QuoteError::NotFound => "evidence_not_found",
    }
}

/// The strings at `keys` in `item`, or `None` when one is missing or is not a string.
fn strings<'a, const N: usize>(item: &'a Value, keys: [&str; N]) -> Option<[&'a str; N]> {
    let mut found = [""; N];
    for (slot, key) in found.iter_mut().zip(keys) {
        *slot = item.get(key)?.as_str()?;
    }
    Some(found)
}

/// A character item as the reply writes it.
struct CharacterItem<'a> {
    reference: &'a str,
    /// Its `name`, then its `aliases` in order, each with the JSON it was read from.
    names: Vec<(Name, &'a Value)>,
    evidence: &'a str,
}

impl<'a> CharacterItem<'a> {
    /// `None` when a field is missing or of the wrong type, its names' fields included.
    fn parse(item: &'a Value) -> Option<Self> {
        let name = item.get("name")?;
        let mut names = vec![(parse_name(name)?, name)];
        for alias in item.get("aliases")?.as_array()? {
            names.push((parse_name(alias)?, alias));
        }
        Some(Self { reference: item.get("ref")?.as_str()?, names, evidence: item.get("evidence")?.as_str()? })
    }
}

/// A name as the reply writes it, or `None` when a field is missing or of the wrong type.
fn parse_name(value: &Value) -> Option<Name> {
    let label = |key: &str| match value.get(key)? {
        Value::Null => Some(None),
        Value::String(part) => Some(Some(part.clone())),
        _ => None,
    };
    let title_sex = match value.get("title_sex")? {
        Value::Null => None,
        Value::String(sex) => Some(Sex::parse(sex)?),
        _ => return None,
    };
    Some(Name {
        text: value.get("text")?.as_str()?.to_owned(),
        given: label("given")?,
        surname: label("surname")?,
        title: label("title")?,
        title_sex,
    })
}
