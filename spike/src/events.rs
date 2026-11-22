//! The event log and its fold. No I/O, so this part can become the
//! wasm-compatible core: the graph at unit N is the fold of the events of
//! units up to N, which is what keeps it spoiler-safe.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
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
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Family,
        Kind::Romance,
        Kind::Friend,
        Kind::Ally,
        Kind::Rival,
        Kind::Enemy,
        Kind::Mentor,
        Kind::Serves,
        Kind::Colleague,
        Kind::Acquaintance,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Family => "family",
            Kind::Romance => "romance",
            Kind::Friend => "friend",
            Kind::Ally => "ally",
            Kind::Rival => "rival",
            Kind::Enemy => "enemy",
            Kind::Mentor => "mentor",
            Kind::Serves => "serves",
            Kind::Colleague => "colleague",
            Kind::Acquaintance => "acquaintance",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        let s = s.trim().to_ascii_lowercase();
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }

    /// Mentor and serves read from `from` to `to`; every other kind is mutual.
    pub fn directed(self) -> bool {
        matches!(self, Kind::Mentor | Kind::Serves)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Introduced { id: String, name: String, evidence: String },
    Appeared { id: String },
    Alias { id: String, alias: String },
    Relation { from: String, to: String, kind: Kind, note: String, evidence: String },
    SameAs { id: String, other: String, evidence: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub first: usize,
    pub seen: Vec<usize>,
    pub evidence: String,
    pub merged: Vec<String>,
}

/// One kind of relation between a pair, from the unit it was first stated to
/// the last unit that stated it again.
#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub kind: Kind,
    pub from: String,
    pub note: String,
    pub first: usize,
    pub last: usize,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Edge {
    pub a: String,
    pub b: String,
    pub facts: Vec<Fact>,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub characters: BTreeMap<String, Character>,
    pub edges: BTreeMap<(String, String), Edge>,
    redirect: HashMap<String, String>,
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

impl Graph {
    pub fn fold<'a>(units: impl IntoIterator<Item = (usize, &'a [Event])>) -> Graph {
        let mut g = Graph::default();
        for (unit, events) in units {
            for e in events {
                g.apply(unit, e);
            }
        }
        g
    }

    /// Follows `same_as` merges to the id that survived them.
    pub fn canonical(&self, id: &str) -> String {
        let mut id = id;
        while let Some(next) = self.redirect.get(id) {
            id = next;
        }
        id.to_string()
    }

    pub fn apply(&mut self, unit: usize, e: &Event) {
        match e {
            Event::Introduced { id, name, evidence } => {
                self.characters.entry(id.clone()).or_insert_with(|| Character {
                    id: id.clone(),
                    name: name.clone(),
                    aliases: vec![],
                    first: unit,
                    seen: vec![unit],
                    evidence: evidence.clone(),
                    merged: vec![],
                });
            }
            Event::Appeared { id } => {
                let id = self.canonical(id);
                if let Some(c) = self.characters.get_mut(&id)
                    && c.seen.last() != Some(&unit)
                {
                    c.seen.push(unit);
                }
            }
            Event::Alias { id, alias } => {
                let id = self.canonical(id);
                if let Some(c) = self.characters.get_mut(&id)
                    && *alias != c.name
                    && !c.aliases.contains(alias)
                {
                    c.aliases.push(alias.clone());
                }
            }
            Event::Relation { from, to, kind, note, evidence } => {
                let (from, to) = (self.canonical(from), self.canonical(to));
                if from == to || !self.characters.contains_key(&from) || !self.characters.contains_key(&to) {
                    return;
                }
                let key = pair(&from, &to);
                let edge = self.edges.entry(key.clone()).or_insert_with(|| Edge { a: key.0, b: key.1, facts: vec![] });
                let fact = Fact {
                    kind: *kind,
                    from: from.clone(),
                    note: note.clone(),
                    first: unit,
                    last: unit,
                    evidence: evidence.clone(),
                };
                add_fact(edge, fact);
            }
            Event::SameAs { id, other, .. } => self.merge(&self.canonical(id), &self.canonical(other)),
        }
    }

    fn merge(&mut self, keep: &str, gone: &str) {
        if keep == gone || !self.characters.contains_key(keep) {
            return;
        }
        let Some(g) = self.characters.remove(gone) else { return };
        if let Some(k) = self.characters.get_mut(keep) {
            for a in std::iter::once(g.name.clone()).chain(g.aliases) {
                if a != k.name && !k.aliases.contains(&a) {
                    k.aliases.push(a);
                }
            }
            k.seen.extend(g.seen);
            k.seen.sort_unstable();
            k.seen.dedup();
            k.first = k.first.min(g.first);
            k.merged.push(g.name);
        }
        self.redirect.insert(gone.to_string(), keep.to_string());
        let moved: Vec<_> = self.edges.keys().filter(|(a, b)| a == gone || b == gone).cloned().collect();
        for key in moved {
            let Some(edge) = self.edges.remove(&key) else { continue };
            let swap = |s: &str| if s == gone { keep.to_string() } else { s.to_string() };
            let (a, b) = (swap(&key.0), swap(&key.1));
            if a == b {
                continue;
            }
            let nk = pair(&a, &b);
            let target = self.edges.entry(nk.clone()).or_insert_with(|| Edge { a: nk.0, b: nk.1, facts: vec![] });
            for mut f in edge.facts {
                f.from = swap(&f.from);
                add_fact(target, f);
            }
        }
    }
}

fn add_fact(edge: &mut Edge, fact: Fact) {
    let same = edge.facts.iter_mut().find(|f| f.kind == fact.kind && (!fact.kind.directed() || f.from == fact.from));
    match same {
        Some(f) => {
            f.first = f.first.min(fact.first);
            f.last = f.last.max(fact.last);
            if f.note.is_empty() {
                f.note = fact.note;
            }
        }
        None => edge.facts.push(fact),
    }
}
