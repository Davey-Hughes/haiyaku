//! The link step over one reading order: for each book after the first, each character it introduces, asked about
//! name by name against the characters of the books before it, until a reply links it or its names run out.

use std::cmp::Reverse;
use std::path::Path;

use serde_json::Value;

use super::names::{conflict, matches, share};
use super::prompt::{Candidate, SYSTEM, Sent, fits, line, prompt, reference, schema};
use crate::Result;
use crate::cast::fold_series;
use crate::extract::cache::{Cache, answer};
use crate::extract::{MIN_QUOTE_WEIGHT, quote_in, weight};
use crate::fold::{Cast, Character, EntityKey};
use crate::llm::{Backend, Reply, Request};
use crate::log::{BookLog, Event, EventBody, Name};
use crate::store::{Link, LinkMethod};
use crate::words::reduce;

/// What the calls of a [`link_order`] were and what they took, and which earlier characters were turned down
/// without a call.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// In the order made.
    pub calls: Vec<Call>,
    /// In the order found. An earlier character is in it once for each reason that turned it down for a later
    /// character, at the first of the later character's names that did; and not at all when a call for that later
    /// character listed it.
    pub turned_down: Vec<TurnedDown>,
    /// The calls' seconds summed, a reply taken from the cache at the seconds it recorded.
    pub seconds: f64,
    /// The calls' costs summed, a reply taken from the cache at the cost it recorded; `None` when any call states
    /// none, since the sum of the others would read as the whole.
    pub cost_usd: Option<f64>,
}

/// One call: the later character and the name that made it, the candidates it listed, and what the reply came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub later: EntityKey,
    /// The name that made the call, and its chapter.
    pub name: String,
    pub chapter: u32,
    /// Each candidate's key and shown name, in the order listed.
    pub candidates: Vec<(EntityKey, String)>,
    /// The ref the reply named, as written; `None` for none.
    pub answer: Option<String>,
    /// The reply's quote, as written.
    pub quote: String,
    pub verdict: Verdict,
}

/// A name of a later character, and the earlier characters it matches that were turned down: no call for the later
/// character lists them, so without this nothing would say why they were never asked about.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnedDown {
    pub later: EntityKey,
    /// The name, and its chapter.
    pub name: String,
    pub chapter: u32,
    /// Each earlier character turned down: its key, its shown name, and why.
    pub candidates: Vec<(EntityKey, String, Refusal)>,
}

/// Why an earlier character was turned down for a later character's name, with no call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Both characters have given names and neither uses one of the other's, or likewise for surnames, counting
    /// the later character's names to the end of the name's chapter.
    Conflict,
    /// Each name of the earlier character that shares a part with the name has a title of the other sex.
    OtherSex,
    /// The later character's book relates it, at any chapter, to another of its characters, and this run has linked
    /// `relative`, that character or one the book joins with it by a same-as, to the earlier character already:
    /// linked as well, the later character would be its own relative. A relation recorded on a character the book
    /// joins with the later one counts as the later one's, and a same-as counts at any chapter too. `name` is the
    /// relative's shown name in the cast of its book alone.
    Related { relative: EntityKey, name: String },
}

/// What a reply came to. Only [`Verdict::Linked`] makes a link. The others say why a reply made none, and are checked
/// in the order listed: the first that holds is the verdict, and a reply that none holds for is linked.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The later character is the candidate of this key.
    Linked(EntityKey),
    /// The reply named no candidate: its candidate is null, or a string that is empty once the whitespace around it
    /// is taken off, or reads `none` or `null` in either ASCII case.
    NoneChosen,
    /// The reply named a ref the call did not list. A ref is read without the whitespace around it and in either
    /// ASCII case, `E1` as `e1`, and in no looser form.
    NotListed,
    /// The quote weighs less than [`MIN_QUOTE_WEIGHT`].
    QuoteTooShort,
    /// What is left of the quote, reduced to its letters and digits, once every name of the later character and of
    /// the listed candidates is taken out of it, weighs less than [`MIN_QUOTE_WEIGHT`]: a name shows that two
    /// characters share it, not that they are one, and a particle or a word or two around it ("톰 리드의", "said
    /// Tom Reed") adds nothing to that.
    QuoteIsAName,
    /// The quote is in none of the quotes the call sent, each taken alone.
    QuoteNotFound,
}

/// How many of a later character's quotes a call sends at most: its first, which introduces it, and the latest
/// before the end of the name's chapter. A long book's main character is named in hundreds of events, and every
/// call for it would otherwise carry them all.
const QUOTES: usize = 12;

/// The links of one reading order, `logs` its books in reading order, and the report of the calls that made them.
///
/// For each book after the first, the earlier characters are those of the books before it, each folded to its end
/// and joined by the links made so far ([`fold_series`]). Each character the book introduces is taken alone, on its
/// own names in order. An earlier character is a candidate for one of those names when one of its names matches it
/// and its names do not conflict with the later character's names to the end of the name's chapter, unless the links
/// made so far join it to a character the later book relates to the later one, either of the two counting with the
/// characters the book joins with it by a same-as ([`Refusal::Related`]). A name that brings a candidate no earlier
/// call for the character listed gets a call listing every candidate it brings; any other name gets none.
///
/// A reply links the character to the candidate it names only when its quote weighs [`MIN_QUOTE_WEIGHT`] or more,
/// does so still with the names of the later character and of the candidates taken out of it, and is found in one
/// of the later character's quotes the call sent. The link holds from the name that made the call, and ends the
/// character's calls, so a character gets one link at most. Every link is a [`LinkMethod::Model`] link.
///
/// An earlier character turned down for a name goes into the report's `turned_down`, unless a call for the same later
/// character lists it.
///
/// A call sends nothing from past the end of its name's chapter. Each goes through the reply cache of the later
/// book in `cache`, under `<cache>/<book sha256>/`, so that a run started again pays only for the calls no earlier
/// run finished. `progress` is given each call once its verdict is known.
///
/// # Errors
///
/// [`Error::Model`](crate::Error::Model) naming the later book's title, the chapter and the name when a call fails
/// twice in a row, which a reply that does not fit the schema does; [`Error::Io`](crate::Error::Io) when the cache
/// cannot be read or written. Either ends the step with no links: the replies kept so far stay in the cache.
pub fn link_order(
    logs: &[BookLog],
    backend: &dyn Backend,
    cache: Option<&Path>,
    progress: &mut dyn FnMut(&Call),
) -> Result<(Vec<Link>, Report)> {
    let schema = schema();
    let schema_text = schema.to_string();
    let mut links = Vec::new();
    let mut report = Report { calls: Vec::new(), turned_down: Vec::new(), seconds: 0.0, cost_usd: Some(0.0) };
    for (place, book) in logs.iter().enumerate().skip(1) {
        let before = &logs[..place];
        let cast = fold_series(before, None, &links);
        let earlier: Vec<Earlier<'_>> =
            cast.characters.iter().map(|character| Earlier::new(character, before)).collect();
        // Names a relative in a turn-down as the later book's own cast shows it.
        let own = fold_series(std::slice::from_ref(book), None, &[]);
        let cache = cache.map(|dir| Cache::new(dir, &book.book.sha256));
        let ask = |sent: &Sent, what: &str| {
            let user = prompt(sent);
            let request = Request { system: SYSTEM, user: &user, schema: &schema };
            answer(backend, &request, &schema_text, cache.as_ref(), what, fits)
        };
        for id in introduced(book) {
            let later = Later::new(book, id);
            let related = later.related(&links, &own);
            links.extend(link_character(&later, &earlier, &related, &ask, &mut report, progress)?);
        }
    }
    Ok((links, report))
}

/// The link of `later` to one of `earlier`, if a reply makes one: its names are asked about in order, as
/// [`link_order`] says, each call with `ask`, until a call links it or its names run out. `related` holds the
/// earlier characters turned down for `later` whatever its name, as [`Later::related`] gives them. Each call goes
/// into `report` and to `progress`, and each earlier character turned down that no call listed into `report`.
fn link_character(
    later: &Later<'_>,
    earlier: &[Earlier<'_>],
    related: &[(EntityKey, Refusal)],
    ask: &dyn Fn(&Sent, &str) -> Result<Reply>,
    report: &mut Report,
    progress: &mut dyn FnMut(&Call),
) -> Result<Option<Link>> {
    // The candidates the calls for this character have listed so far, and the earlier characters turned down.
    let mut listed: Vec<&EntityKey> = Vec::new();
    let mut refused: Vec<TurnedDown> = Vec::new();
    let mut linked = None;
    for (event, name) in &later.names {
        let chapter = event.chapter;
        let so_far: Vec<&Name> = later.names_to(chapter).collect();
        let mut candidates: Vec<&Earlier<'_>> = Vec::new();
        let mut turned_down = Vec::new();
        for candidate in earlier {
            match weigh(name, &so_far, candidate, related) {
                Weighed::Apart => {}
                Weighed::Candidate => candidates.push(candidate),
                Weighed::TurnedDown(refusal) => {
                    let key = &candidate.character.key;
                    // Once for each reason: a turn-down for another reason than the one reported says something
                    // the first does not, as a relative's link does after a title of the other sex.
                    let reported = |earlier: &TurnedDown| {
                        earlier.candidates.iter().any(|(other, _, why)| other == key && *why == refusal)
                    };
                    if !refused.iter().any(reported) {
                        turned_down.push((key.clone(), candidate.character.name.text.clone(), refusal));
                    }
                }
            }
        }
        if !turned_down.is_empty() {
            refused.push(TurnedDown {
                later: later.key.clone(),
                name: name.text.clone(),
                chapter,
                candidates: turned_down,
            });
        }
        if candidates.iter().all(|candidate| listed.contains(&&candidate.character.key)) {
            continue;
        }
        for candidate in &candidates {
            if !listed.contains(&&candidate.character.key) {
                listed.push(&candidate.character.key);
            }
        }

        let title = &later.book.book.title;
        let sent = Sent {
            book: line(title),
            chapter,
            name: line(&name.text),
            names: once(so_far.iter().map(|name| line(&name.text))),
            quotes: later.quotes_to(chapter),
            candidates: candidates.iter().map(|candidate| candidate.listed()).collect(),
        };
        let reply = ask(&sent, &format!("{title}, chapter {chapter}, link call for the name {:?}", name.text))?;
        report.seconds += reply.seconds;
        report.cost_usd = report.cost_usd.zip(reply.cost_usd).map(|(sum, cost)| sum + cost);

        // `fits` has seen to it that the candidate is a string or null and the quote a string.
        let chosen = reply.json.get("candidate").and_then(Value::as_str).map(str::to_owned);
        let quote = reply.json.get("quote").and_then(Value::as_str).unwrap_or_default().to_owned();
        let verdict = verdict(chosen.as_deref(), &quote, &candidates, later, &sent.quotes);
        linked = match &verdict {
            Verdict::Linked(earlier) => Some(Link {
                later: later.key.clone(),
                earlier: earlier.clone(),
                method: LinkMethod::Model { quote: quote.clone() },
                chapter,
                at: event.at.clone(),
            }),
            _ => None,
        };
        let call = Call {
            later: later.key.clone(),
            name: name.text.clone(),
            chapter,
            candidates: candidates
                .iter()
                .map(|candidate| (candidate.character.key.clone(), candidate.character.name.text.clone()))
                .collect(),
            answer: chosen,
            quote,
            verdict,
        };
        progress(&call);
        report.calls.push(call);
        if linked.is_some() {
            break;
        }
    }
    // A candidate one name turns down and another brings to a call was weighed by the model after all: a line saying
    // it was turned down would read as if it never was.
    for turned_down in &mut refused {
        turned_down.candidates.retain(|(key, _, _)| !listed.contains(&key));
    }
    report.turned_down.extend(refused.into_iter().filter(|turned_down| !turned_down.candidates.is_empty()));
    Ok(linked)
}

/// What the rules make of an earlier character for a later character's name.
enum Weighed {
    /// No name of it shares a part with the name.
    Apart,
    Candidate,
    TurnedDown(Refusal),
}

/// What the rules make of `candidate` for `name`, a name of a later character whose names by the end of its
/// chapter are `so_far` and for which `related` turns down the earlier characters it holds.
fn weigh(name: &Name, so_far: &[&Name], candidate: &Earlier<'_>, related: &[(EntityKey, Refusal)]) -> Weighed {
    if candidate.names.iter().any(|earlier| matches(name, earlier)) {
        let key = &candidate.character.key;
        if conflict(so_far, &candidate.names) {
            Weighed::TurnedDown(Refusal::Conflict)
        } else if let Some((_, refusal)) = related.iter().find(|(linked, _)| linked == key) {
            Weighed::TurnedDown(refusal.clone())
        } else {
            Weighed::Candidate
        }
    } else if candidate.names.iter().any(|earlier| share(name, earlier)) {
        // No name of it matches, so each that shares a part with the name has a title of the other sex.
        Weighed::TurnedDown(Refusal::OtherSex)
    } else {
        Weighed::Apart
    }
}

/// What the reply that named `chosen` with `quote` comes to, for a call about `later` that listed `candidates` and
/// sent `sent` as the later character's quotes.
fn verdict(
    chosen: Option<&str>,
    quote: &str,
    candidates: &[&Earlier<'_>],
    later: &Later<'_>,
    sent: &[String],
) -> Verdict {
    let Some(chosen) = chosen else { return Verdict::NoneChosen };
    // A model may capitalise the ref or pad it, and either still names one candidate. A bare number or a ref with a
    // mark after it is not read as one: a guess at what such a reply meant could name a candidate it did not.
    let chosen = chosen.trim();
    // A model asked for null may write the word, or leave the string empty, and means none all the same: read as a
    // ref, the report would say it named a candidate the call never listed.
    if chosen.is_empty() || ["none", "null"].iter().any(|word| chosen.eq_ignore_ascii_case(word)) {
        return Verdict::NoneChosen;
    }
    let listed = |place: usize| reference(place).eq_ignore_ascii_case(chosen);
    let Some((_, candidate)) = candidates.iter().enumerate().find(|(place, _)| listed(*place)) else {
        return Verdict::NotListed;
    };
    if weight(quote) < MIN_QUOTE_WEIGHT {
        return Verdict::QuoteTooShort;
    }
    let mut names: Vec<String> = later
        .names
        .iter()
        .map(|(_, name)| *name)
        .chain(candidates.iter().flat_map(|listed| listed.names.iter().copied()))
        .map(|name| reduce(&name.text))
        .filter(|name| !name.is_empty())
        .collect();
    // The longest first: a shorter name inside a longer one ("Tom" in "Tom Reed") taken out first would leave
    // the rest of the longer one to count as words of the quote.
    names.sort_by_key(|name| Reverse(name.len()));
    let rest = names.iter().fold(reduce(quote), |rest, name| rest.replace(name.as_str(), ""));
    if weight(&rest) < MIN_QUOTE_WEIGHT {
        return Verdict::QuoteIsAName;
    }
    // One sent quote at a time: two run together could make a quote that neither holds.
    if !sent.iter().any(|text| quote_in(text, quote).is_ok()) {
        return Verdict::QuoteNotFound;
    }
    Verdict::Linked(candidate.character.key.clone())
}

/// A character of the books before the one being linked, with what the name rules weigh and what a call lists.
struct Earlier<'a> {
    character: &'a Character,
    /// The name of every `Introduced`, `Named` and `Alias` event of it or of a character merged into it, so that a
    /// name carried by any of them makes it a candidate and is counted in the conflict rule.
    names: Vec<&'a Name>,
    /// The title of its key's book.
    book: &'a str,
    /// The quote of its key's own introduction.
    introduction: &'a str,
}

impl<'a> Earlier<'a> {
    fn new(character: &'a Character, before: &'a [BookLog]) -> Earlier<'a> {
        let key = &character.key;
        let is_it = |book: &str, id: &str| {
            [key].into_iter().chain(&character.merged).any(|merged| merged.book == book && merged.id == id)
        };
        let mut names = Vec::new();
        for log in before {
            for event in &log.events {
                if let Some((id, name)) = name_of(event)
                    && is_it(&log.book.sha256, id)
                {
                    names.push(name);
                }
            }
        }
        let own = before.iter().find(|log| log.book.sha256 == key.book);
        let introduction = own.and_then(|log| {
            log.events.iter().find_map(|event| match &event.body {
                EventBody::Introduced { entity, .. } if *entity == key.id => Some(event.quote.as_str()),
                _ => None,
            })
        });
        Earlier {
            character,
            names,
            book: own.map_or("", |log| log.book.title.as_str()),
            introduction: introduction.unwrap_or_default(),
        }
    }

    /// The character as a call lists it.
    fn listed(&self) -> Candidate {
        Candidate {
            book: line(self.book),
            name: line(&self.character.name.text),
            others: self.character.aliases.iter().map(|other| line(&other.text)).collect(),
            introduction: line(self.introduction),
        }
    }
}

/// A character of the book being linked, taken alone: a same-as inside its book is the fold's to apply at its own
/// chapter, so a call never weighs a name that only its partner carries.
struct Later<'a> {
    book: &'a BookLog,
    key: EntityKey,
    /// Its own `Introduced`, `Named` and `Alias` events, in `seq` order, each with the name it gives.
    names: Vec<(&'a Event, &'a Name)>,
}

impl<'a> Later<'a> {
    fn new(book: &'a BookLog, id: &str) -> Later<'a> {
        let names = book
            .events
            .iter()
            .filter_map(|event| name_of(event).filter(|(entity, _)| *entity == id).map(|(_, name)| (event, name)));
        Later { book, key: EntityKey { book: book.book.sha256.clone(), id: id.to_owned() }, names: names.collect() }
    }

    /// Its names given by the end of `chapter`.
    fn names_to(&self, chapter: u32) -> impl Iterator<Item = &'a Name> {
        self.names.iter().filter(move |(event, _)| event.chapter <= chapter).map(|(_, name)| *name)
    }

    /// The earlier characters that `links` join a relative of it to, each with the [`Refusal::Related`] that turns
    /// it down, naming the relative as `own`, the cast of its book alone, shows it. A relative is a character its
    /// book relates to it at any chapter, or one `own` joins with such a character; a relation recorded on a
    /// character `own` joins with it is its own. The whole book is extracted before it is linked, and a turn-down
    /// shows a reader nothing, so a relation or a same-as recorded after the name that would make the call keeps the
    /// two apart as well. Only a link of its own book counts, and such a link is one this run has made before
    /// reaching it.
    fn related(&self, links: &[Link], own: &Cast) -> Vec<(EntityKey, Refusal)> {
        // Every id that stands for one character: the extraction may record a relation under one of a character's
        // ids and the run link it under another, and the fold makes them one person all the same.
        let itself = joined(own, &self.key.id);
        let others = self.book.events.iter().filter_map(|event| relative(event, &itself));
        // Two ids of the character itself are not two people, whatever the book says of them.
        let relatives = once(others.filter(|other| !itself.contains(other)).flat_map(|other| joined(own, other)));
        let of_relative =
            |link: &&Link| link.later.book == self.key.book && relatives.contains(&link.later.id.as_str());
        let shown = |key: &EntityKey| {
            character_of(own, &key.id).map_or_else(|| key.id.clone(), |character| character.name.text.clone())
        };
        let refusal = |link: &Link| Refusal::Related { relative: link.later.clone(), name: shown(&link.later) };
        links.iter().filter(of_relative).map(|link| (link.earlier.clone(), refusal(link))).collect()
    }

    /// The quotes of the events of its book to the end of `chapter` that name it, each text once, in `seq` order;
    /// past [`QUOTES`] of them, the first and the latest.
    fn quotes_to(&self, chapter: u32) -> Vec<String> {
        let id = &self.key.id;
        let events = self.book.events.iter().filter(|event| event.chapter <= chapter && names_it(event, id));
        let mut quotes = once(events.map(|event| line(&event.quote)));
        if quotes.len() > QUOTES {
            quotes.drain(1..quotes.len() - (QUOTES - 1));
        }
        quotes
    }
}

/// The ids of the characters `book` introduces, in the order of their first introductions.
fn introduced(book: &BookLog) -> Vec<&str> {
    let ids = book.events.iter().filter_map(|event| match &event.body {
        EventBody::Introduced { entity, .. } => Some(entity.as_str()),
        _ => None,
    });
    once(ids)
}

/// The entity and the name of an `Introduced`, `Named` or `Alias` event.
fn name_of(event: &Event) -> Option<(&str, &Name)> {
    match &event.body {
        EventBody::Introduced { entity, name }
        | EventBody::Named { entity, name }
        | EventBody::Alias { entity, name } => Some((entity, name)),
        _ => None,
    }
}

/// Whether `event` names the entity `id`, as its subject or at either end of a relation, an overturn or a same-as.
fn names_it(event: &Event, id: &str) -> bool {
    match &event.body {
        EventBody::Introduced { entity, .. }
        | EventBody::Named { entity, .. }
        | EventBody::Alias { entity, .. }
        | EventBody::Appeared { entity } => entity == id,
        EventBody::Relation { from, to, .. } | EventBody::Overturned { from, to, .. } => from == id || to == id,
        EventBody::SameAs { entity, other } => entity == id || other == id,
    }
}

/// The entity at the other end of `event` when it is a relation or an overturn with one of `ids` at one end: an
/// overturned relation still names two people.
fn relative<'e>(event: &'e Event, ids: &[&str]) -> Option<&'e str> {
    match &event.body {
        EventBody::Relation { from, to, .. } | EventBody::Overturned { from, to, .. } => {
            if ids.contains(&from.as_str()) {
                Some(to)
            } else if ids.contains(&to.as_str()) {
                Some(from)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The character of `own`, the cast of one book alone, that the id `id` of that book stands for.
fn character_of<'c>(own: &'c Cast, id: &str) -> Option<&'c Character> {
    let is_it = |key: &EntityKey| key.id == id;
    own.characters.iter().find(|character| is_it(&character.key) || character.merged.iter().any(is_it))
}

/// The ids of one book that `own`, the cast of that book alone, makes one character with `id`, `id` among them;
/// `id` alone when `own` does not have it.
fn joined<'c>(own: &'c Cast, id: &'c str) -> Vec<&'c str> {
    match character_of(own, id) {
        Some(character) => [&character.key].into_iter().chain(&character.merged).map(|key| key.id.as_str()).collect(),
        None => vec![id],
    }
}

/// `items` in order, each once.
fn once<T: PartialEq>(items: impl Iterator<Item = T>) -> Vec<T> {
    let mut kept = Vec::new();
    for item in items {
        if !kept.contains(&item) {
            kept.push(item);
        }
    }
    kept
}
