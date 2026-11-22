//! Joins books extracted on their own into one series, in a chosen order.
//! A character of a later book is matched to one from an earlier book by
//! name, at the chapter whose text gives that name, so a link never arrives
//! before the text supports it. Its own name, equal to an earlier character's
//! once a leading "the" and titles are set aside, links at once. A weaker match
//! (a bare given name, an alias, part of a name) links only when the model,
//! shown the chapter's quotes, says the two are one person.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use crate::{
    Run, UnitRecord,
    events::{Character, Event, Graph},
    extract::alnum,
    llm::Backend,
};

/// Words that name a role rather than a person, too common to link on.
const ROLES: &[&str] = &["mum", "mom", "dad", "mother", "father", "boy", "girl"];

/// Titles set aside before names are compared, with the sex each implies.
const TITLES: &[(&str, Option<char>)] = &[
    ("mr", Some('m')),
    ("sir", Some('m')),
    ("lord", Some('m')),
    ("uncle", Some('m')),
    ("master", Some('m')),
    ("mrs", Some('f')),
    ("ms", Some('f')),
    ("miss", Some('f')),
    ("madam", Some('f')),
    ("madame", Some('f')),
    ("lady", Some('f')),
    ("dame", Some('f')),
    ("aunt", Some('f')),
    ("auntie", Some('f')),
    ("professor", None),
    ("prof", None),
    ("dr", None),
    ("doctor", None),
    ("captain", None),
];

const SYSTEM: &str = "You decide whether a character in a later book of a series is the same person as a \
character of an earlier book. Characters of one series often share a given name or a surname. Judge from \
the names and quotes given. Answer same or different, and copy the words from one of the quotes that best \
support the answer.";

struct Name {
    /// Lower-case alphanumeric words, without a leading "the" or titles.
    words: Vec<String>,
    /// From the title: "Mr. Weasley" and "Mrs. Weasley" are two people.
    sex: Option<char>,
    /// Every word is capitalised, so the first is a given name and the last a
    /// surname, which "Peeves the Poltergeist" or "the pale boy" are not.
    personal: bool,
}

fn parse(raw: &str) -> Option<Name> {
    let mut parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.first().is_some_and(|w| alnum(w) == "the") {
        parts.remove(0);
    }
    let mut sex = None;
    while let Some((_, s)) = parts.first().and_then(|w| {
        let w = alnum(w);
        TITLES.iter().find(|(t, _)| *t == w)
    }) {
        sex = sex.or(*s);
        parts.remove(0);
    }
    // Scripts without case count as capitalised.
    let capital = |w: &&str| w.chars().find(|c| c.is_alphanumeric()).is_some_and(|c| !c.is_lowercase());
    let words: Vec<String> = parts.iter().map(|w| alnum(w)).filter(|w| !w.is_empty()).collect();
    // An all lower-case name ("the elf") describes a role, not a person.
    if words.concat().chars().count() < 3
        || (words.len() == 1 && ROLES.contains(&words[0].as_str()))
        || !parts.iter().any(capital)
    {
        return None;
    }
    Some(Name { personal: parts.iter().all(capital), words, sex })
}

fn names_of(c: &Character) -> Vec<Name> {
    std::iter::once(&c.name).chain(&c.aliases).filter_map(|n| parse(n)).collect()
}

fn words(names: &[Name]) -> HashSet<&str> {
    names.iter().flat_map(|n| n.words.iter().map(String::as_str)).collect()
}

/// Given names and surnames, from the names that have both.
fn parts_of(names: &[Name]) -> (HashSet<&str>, HashSet<&str>) {
    let (mut given, mut surnames) = (HashSet::new(), HashSet::new());
    for n in names.iter().filter(|n| n.personal && n.words.len() >= 2) {
        // An initial ("T. M. Riddle") cannot tell two given names apart.
        if n.words[0].chars().count() > 1 {
            given.insert(n.words[0].as_str());
        }
        surnames.insert(n.words[n.words.len() - 1].as_str());
    }
    (given, surnames)
}

/// Both sides have a given name, or both a surname, and neither side ever
/// uses one of the other's. So "Lucius Malfoy" is not "Draco Malfoy" though
/// both are called "Mr. Malfoy", while the nickname "Perfect Percy" does not
/// count against "Percy Weasley".
fn clash(a: &[Name], b: &[Name]) -> bool {
    let (wa, wb) = (words(a), words(b));
    let ((ga, sa), (gb, sb)) = (parts_of(a), parts_of(b));
    let differ = |x: &HashSet<&str>, y: &HashSet<&str>| {
        !x.is_empty() && !y.is_empty() && !x.iter().any(|w| wb.contains(w)) && !y.iter().any(|w| wa.contains(w))
    };
    differ(&ga, &gb) || differ(&sa, &sb)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Weak,
    Strong,
}

/// How well the later character's names match the earlier character's. With
/// `own`, `later[0]` is its own name. Only that name, equal to one of the
/// earlier character's, is strong, and not when it is a bare given name, which
/// many people share.
fn level(later: &[Name], own: bool, earlier: &[Name]) -> Option<Level> {
    let (ga, _) = parts_of(later);
    let (gb, _) = parts_of(earlier);
    let mut best = None;
    for (i, a) in later.iter().enumerate() {
        for b in earlier {
            if a.sex.is_some() && b.sex.is_some() && a.sex != b.sex {
                continue;
            }
            let l = if a.words == b.words {
                let w = a.words[0].as_str();
                let bare = a.words.len() == 1 && (ga.contains(w) || gb.contains(w));
                if own && i == 0 && !bare { Level::Strong } else { Level::Weak }
            } else if a.personal && b.personal && (a.words.starts_with(&b.words) || b.words.starts_with(&a.words)) {
                Level::Weak
            } else {
                continue;
            };
            best = best.max(Some(l));
        }
    }
    best
}

/// The quotes of one chapter about `id`, after `out`.
fn quotes(events: &[Event], id: &str, mut out: Vec<String>) -> Vec<String> {
    for e in events {
        let q = match e {
            Event::Introduced { id: i, evidence, .. } if i == id => evidence,
            Event::Relation { from, to, evidence, .. } if from == id || to == id => evidence,
            Event::SameAs { id: i, other, evidence } if i == id || other == id => evidence,
            _ => continue,
        };
        if !q.is_empty() && !out.contains(q) {
            out.push(q.clone());
        }
    }
    out.truncate(8);
    out
}

fn name_of<'a>(series: &'a Graph, id: &'a str) -> &'a str {
    series.characters.get(id).map_or(id, |c| c.name.as_str())
}

/// A later book's character as the model sees it.
struct Later<'a> {
    /// The series id it has, or will have if it stays unlinked.
    sid: String,
    chapter: &'a str,
    /// Its own name first.
    names: Vec<String>,
    quotes: Vec<String>,
}

enum Found {
    /// The earlier character, and the model's quote when the model decided.
    One(String, Option<String>),
    Many(Vec<String>),
    None,
}

/// Puts weak matches to the model, each pair once.
struct Judge<'a> {
    llm: &'a Backend,
    asked: HashSet<(String, String)>,
    calls: usize,
    failed: usize,
    seconds: f64,
    cost: f64,
}

impl Judge<'_> {
    fn find(&mut self, series: &Graph, before: usize, later: &Later, own: bool, lines: &mut Vec<String>) -> Found {
        let own = own && parse(&later.names[0]).is_some();
        let mine: Vec<Name> = later.names.iter().filter_map(|n| parse(n)).collect();
        let (mut strong, mut weak) = (Vec::new(), Vec::new());
        for c in series.characters.values().filter(|c| c.first < before) {
            let theirs = names_of(c);
            if clash(&mine, &theirs) {
                continue;
            }
            match level(&mine, own, &theirs) {
                Some(Level::Strong) => strong.push(c),
                Some(Level::Weak) => weak.push(c),
                None => {}
            }
        }
        match strong.len() {
            0 => {}
            1 => return Found::One(strong[0].id.clone(), None),
            _ => return Found::Many(strong.iter().map(|c| c.id.clone()).collect()),
        }
        let mut same = Vec::new();
        for c in weak {
            if self.asked.insert((later.sid.clone(), c.id.clone()))
                && let Some(quote) = self.ask(later, c, lines)
            {
                same.push((c.id.clone(), quote));
            }
        }
        match same.len() {
            0 => Found::None,
            1 => {
                let (id, quote) = same.remove(0);
                Found::One(id, Some(quote))
            }
            _ => Found::Many(same.into_iter().map(|(id, _)| id).collect()),
        }
    }

    fn ask(&mut self, later: &Later, earlier: &Character, lines: &mut Vec<String>) -> Option<String> {
        let also = |n: &[String]| if n.is_empty() { "-".to_string() } else { n.join("; ") };
        let user = format!(
            "Later book, {}:\n{}\nAlso called: {}\nQuotes from the later book:\n{}\n\n\
             Earlier book:\n{}\nAlso called: {}\nQuote where it first appears:\n- \"{}\"\n\n\
             Is the later book's character the same person as the earlier book's?",
            later.chapter,
            later.names[0],
            also(&later.names[1..]),
            later.quotes.iter().map(|q| format!("- \"{q}\"")).collect::<Vec<_>>().join("\n"),
            earlier.name,
            also(&earlier.aliases),
            earlier.evidence,
        );
        let schema = json!({
            "type": "object",
            "properties": {
                "verdict": {"type": "string", "enum": ["same", "different"]},
                "quote": {"type": "string"},
            },
            "required": ["verdict", "quote"],
            "additionalProperties": false,
        });
        let pair = format!("{}: {} -> {}", later.chapter, later.names[0], earlier.name);
        match self.llm.call(SYSTEM, &user, &schema) {
            Err(e) => {
                self.failed += 1;
                lines.push(format!("error  {pair}: {e:#}"));
                None
            }
            Ok(r) => {
                self.calls += 1;
                self.seconds += r.seconds;
                self.cost += r.cost_usd.unwrap_or(0.0);
                let verdict = r.json["verdict"].as_str().unwrap_or("?");
                let quote = r.json["quote"].as_str().unwrap_or("").to_string();
                let q = alnum(&quote);
                // A link rests on words the reader can find in the text shown.
                let grounded = !q.is_empty()
                    && later.quotes.iter().chain(std::iter::once(&earlier.evidence)).any(|g| alnum(g).contains(&q));
                let note = if verdict == "same" && !grounded { ", quote not in the text: no link" } else { "" };
                lines.push(format!("model  {pair}: {verdict}{note} ({:.0} s) \"{quote}\"", r.seconds));
                (verdict == "same" && grounded).then_some(quote)
            }
        }
    }
}

pub fn link(runs: &[Run], llm: &Backend) -> (Run, Vec<String>) {
    let mut series = Graph::default();
    let mut units = Vec::new();
    let mut lines = Vec::new();
    let mut judge = Judge { llm, asked: HashSet::new(), calls: 0, failed: 0, seconds: 0.0, cost: 0.0 };
    for (b, run) in runs.iter().enumerate() {
        let offset = (b + 1) * 1000;
        let ns = format!("b{}:", b + 1);
        let mut map: HashMap<String, String> = HashMap::new();
        let mut unlinked: HashSet<String> = HashSet::new();
        let (mut linked, mut fresh, mut ambiguous) = (0, 0, 0);
        for u in &run.units {
            let unit = offset + u.index;
            let title = format!("B{} {}", b + 1, u.title);
            let mut aliases: HashMap<&str, Vec<&str>> = HashMap::new();
            for e in &u.events {
                if let Event::Alias { id, alias } = e {
                    aliases.entry(id.as_str()).or_default().push(alias.as_str());
                }
            }
            let mut events = Vec::new();
            let get =
                |map: &HashMap<String, String>, id: &str| map.get(id).cloned().unwrap_or_else(|| format!("{ns}{id}"));
            for e in &u.events {
                let start = events.len();
                match e {
                    Event::Introduced { id, name, evidence } => {
                        let sid = format!("{ns}{id}");
                        let found = if b == 0 {
                            Found::None
                        } else {
                            let own = aliases.get(id.as_str()).cloned().unwrap_or_default();
                            let later = Later {
                                sid: sid.clone(),
                                chapter: &title,
                                names: std::iter::once(name.as_str()).chain(own).map(String::from).collect(),
                                quotes: quotes(&u.events, id, vec![]),
                            };
                            judge.find(&series, offset, &later, true, &mut lines)
                        };
                        match found {
                            Found::One(earlier, quote) => {
                                let how = if quote.is_some() { "model" } else { "name" };
                                lines.push(format!("link   {title}: {name} -> {} ({how})", name_of(&series, &earlier)));
                                linked += 1;
                                map.insert(id.clone(), earlier.clone());
                                events.push(Event::Appeared { id: earlier });
                            }
                            other => {
                                if let Found::Many(c) = &other {
                                    ambiguous += 1;
                                    lines.push(format!("ambig  {title}: {name} -> {}", c.join(", ")));
                                }
                                if b > 0 {
                                    fresh += 1;
                                    lines.push(format!("new    {title}: {name}"));
                                }
                                map.insert(id.clone(), sid.clone());
                                unlinked.insert(sid.clone());
                                events.push(Event::Introduced {
                                    id: sid,
                                    name: name.clone(),
                                    evidence: evidence.clone(),
                                });
                            }
                        }
                    }
                    Event::Appeared { id } => events.push(Event::Appeared { id: get(&map, id) }),
                    Event::Alias { id, alias } => {
                        let sid = get(&map, id);
                        // A new character of this book whose later alias names
                        // an earlier book's character may be that character.
                        // One merged into another (same_as) has left the graph.
                        if b > 0
                            && unlinked.contains(&sid)
                            && let Some(c) = series.characters.get(&sid)
                        {
                            let later = Later {
                                sid: sid.clone(),
                                chapter: &title,
                                names: std::iter::once(&c.name).chain(&c.aliases).chain([alias]).cloned().collect(),
                                quotes: quotes(&u.events, id, vec![c.evidence.clone()]),
                            };
                            if let Found::One(earlier, quote) = judge.find(&series, offset, &later, false, &mut lines) {
                                lines.push(format!(
                                    "late   {title}: {} -> {} (alias {alias})",
                                    c.name,
                                    name_of(&series, &earlier)
                                ));
                                unlinked.remove(&sid);
                                linked += 1;
                                fresh -= 1;
                                events.push(Event::SameAs {
                                    id: earlier.clone(),
                                    other: sid.clone(),
                                    evidence: quote.unwrap_or_else(|| format!("alias {alias}")),
                                });
                                map.insert(id.clone(), earlier);
                            }
                        }
                        events.push(Event::Alias { id: get(&map, id), alias: alias.clone() });
                    }
                    Event::Relation { from, to, kind, note, evidence } => events.push(Event::Relation {
                        from: get(&map, from),
                        to: get(&map, to),
                        kind: *kind,
                        note: note.clone(),
                        evidence: evidence.clone(),
                    }),
                    Event::SameAs { id, other, evidence } => events.push(Event::SameAs {
                        id: get(&map, id),
                        other: get(&map, other),
                        evidence: evidence.clone(),
                    }),
                }
                for e in &events[start..] {
                    series.apply(unit, e);
                }
            }
            units.push(UnitRecord {
                index: unit,
                title,
                chars: u.chars,
                seconds: u.seconds,
                prompt_tokens: u.prompt_tokens,
                output_tokens: u.output_tokens,
                cost_usd: u.cost_usd,
                events,
                rejected: u.rejected.clone(),
                counts: u.counts.clone(),
                error: u.error.clone(),
            });
        }
        if b > 0 {
            lines.push(format!("book {}: {linked} linked, {fresh} new, {ambiguous} ambiguous", b + 1));
        }
    }
    lines.push(format!(
        "model: {} calls, {} failed, {:.0} s, {:.4} USD",
        judge.calls, judge.failed, judge.seconds, judge.cost
    ));
    let book = runs.iter().map(|r| r.book.as_str()).collect::<Vec<_>>().join(" + ");
    let backend = runs.first().map_or(String::new(), |r| r.backend.clone());
    (Run { book, backend, units, after: vec![] }, lines)
}
