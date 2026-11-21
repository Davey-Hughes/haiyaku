//! One unit in, checked events out. The model only proposes: every item is
//! resolved against the roster and grounded in the unit's own text before it
//! becomes an event, and what fails is kept with its reason.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::events::{Event, Graph, Kind};

pub const SYSTEM: &str = "You record the characters of a book one chapter at a time, for a reader who has read up to the end of this chapter and no further. Reply with JSON only.";

pub fn schema() -> Value {
    let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["characters", "relations", "same_as"],
        "properties": {
            "characters": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["id", "name", "new", "aliases", "evidence"],
                "properties": {
                    "id": {"type": "string"}, "name": {"type": "string"}, "new": {"type": "boolean"},
                    "aliases": {"type": "array", "items": {"type": "string"}}, "evidence": {"type": "string"}
                }
            }},
            "relations": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["from", "to", "kind", "note", "evidence"],
                "properties": {
                    "from": {"type": "string"}, "to": {"type": "string"},
                    "kind": {"type": "string", "enum": kinds},
                    "note": {"type": "string"}, "evidence": {"type": "string"}
                }
            }},
            "same_as": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["id", "other", "evidence"],
                "properties": {"id": {"type": "string"}, "other": {"type": "string"}, "evidence": {"type": "string"}}
            }}
        }
    })
}

pub fn prompt(graph: &Graph, number: usize, title: &str, text: &str) -> String {
    let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
    let mut p = String::new();
    p.push_str(&format!(
        "Rules:
- Use only what this chapter's text says. You may know this book: ignore everything you know about it beyond the text below. Never state a name, identity, relationship or fact that the text has not shown by the end of this chapter.
- characters: every named character who appears, speaks or is described in this chapter. Include an unnamed character only if they matter to what happens; leave out bystanders and crowds (a secretary, passers-by, people in the street) and people only mentioned in passing.
  - A character already in the roster keeps its roster id, with \"new\": false.
  - A character not in the roster gets a new id (lowercase words joined by hyphens) and \"new\": true. Its name is the fullest name this chapter gives, copied exactly; an unnamed character gets a short description from the text.
  - aliases: other names, nicknames and titles this chapter uses for the character (for example \"Mr. Smith\", \"the Headmaster\"), each copied exactly from the text. Never a description, a common noun or a pronoun.
- relations: relationships this chapter states or clearly shows. from and to are ids from the roster or your characters list. kind is one of: {kinds}. note is a few words (for example \"uncle\", \"best friend\"). The order of from and to matters only for mentor (from regularly teaches or guides to; not a single act of help) and serves (from serves to).
- same_as: only when this chapter reveals that two characters (from the roster or your characters list) are the same person. Both are ids; id is the one to keep.
- evidence: a passage of 5 to 25 words copied exactly from this chapter that shows the item. Never paraphrase it.

",
        kinds = kinds.join(", ")
    ));
    p.push_str("Roster so far (id | name | aliases):\n");
    if graph.characters.is_empty() {
        p.push_str("(empty: this is the first chapter)\n");
    }
    for c in graph.characters.values() {
        p.push_str(&format!("{} | {} | {}\n", c.id, c.name, c.aliases.join(", ")));
    }
    if !graph.edges.is_empty() {
        p.push_str("\nRelations so far (from | kind | to | note):\n");
        for e in graph.edges.values() {
            for f in &e.facts {
                let to = if f.from == e.a { &e.b } else { &e.a };
                p.push_str(&format!("{} | {} | {} | {}\n", f.from, f.kind.name(), to, f.note));
            }
        }
    }
    p.push_str(&format!("\nChapter {number}: {title}\n\n{text}\n"));
    p
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rejected {
    pub reason: String,
    pub item: Value,
}

#[derive(Default)]
pub struct Outcome {
    pub events: Vec<Event>,
    pub rejected: Vec<Rejected>,
    pub counts: BTreeMap<String, u32>,
}

impl Outcome {
    fn count(&mut self, what: &str) {
        *self.counts.entry(what.to_string()).or_default() += 1;
    }
    fn reject(&mut self, reason: &str, item: &Value) {
        self.count(reason);
        self.rejected.push(Rejected { reason: reason.to_string(), item: item.clone() });
    }
}

#[derive(Deserialize)]
struct CharItem {
    id: String,
    name: String,
    #[serde(default)]
    new: bool,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    evidence: String,
}

#[derive(Deserialize)]
struct RelItem {
    from: String,
    to: String,
    kind: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    evidence: String,
}

#[derive(Deserialize)]
struct SameItem {
    id: String,
    other: String,
    #[serde(default)]
    evidence: String,
}

/// Letters and digits only, lowercased: quotes, dashes, spacing and
/// punctuation vary between the book and a model's copy of it, the words do not.
pub fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

const PRONOUNS: &[&str] = &["he", "him", "his", "she", "her", "hers", "they", "them", "you", "i", "me", "it"];

fn grounded_quote(evidence: &str, hay: &str) -> Result<(), &'static str> {
    let evidence = evidence.replace('…', "...");
    let parts: Vec<String> = evidence.split("...").map(alnum).filter(|p| !p.is_empty()).collect();
    if parts.iter().map(|p| p.chars().count()).sum::<usize>() < 8 {
        return Err("evidence_too_short");
    }
    let mut from = 0;
    for part in parts {
        match hay[from..].find(&part) {
            Some(i) => from += i + part.len(),
            None => return Err("evidence_not_found"),
        }
    }
    Ok(())
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn check(graph: &Graph, text: &str, reply: &Value) -> Outcome {
    let mut out = Outcome::default();
    let hay = alnum(text);
    // Every name and alias already known, to catch a duplicate declared under a new id.
    let mut by_name: HashMap<String, String> = HashMap::new();
    for c in graph.characters.values() {
        for n in std::iter::once(&c.name).chain(&c.aliases) {
            by_name.entry(alnum(n)).or_insert_with(|| c.id.clone());
        }
    }
    let mut local: HashMap<String, String> = HashMap::new();
    let mut here: HashSet<String> = HashSet::new();
    let mut known_alias: HashSet<(String, String)> = HashSet::new();
    for c in graph.characters.values() {
        for a in std::iter::once(&c.name).chain(&c.aliases) {
            known_alias.insert((c.id.clone(), alnum(a)));
        }
    }
    let resolve = |id: &str, local: &HashMap<String, String>| -> Option<String> {
        let c = graph.canonical(id);
        if graph.characters.contains_key(&c) {
            return Some(c);
        }
        local.get(id).cloned()
    };
    // Relations and reveals sometimes name a character where the id belongs.
    let resolve_or_name =
        |id: &str, local: &HashMap<String, String>, by_name: &HashMap<String, String>, out: &mut Outcome| {
            resolve(id, local).or_else(|| by_name.get(&alnum(id)).cloned().inspect(|_| out.count("id_given_as_name")))
        };

    for item in reply["characters"].as_array().into_iter().flatten() {
        let Ok(c) = serde_json::from_value::<CharItem>(item.clone()) else {
            out.reject("malformed", item);
            continue;
        };
        if let Err(why) = grounded_quote(&c.evidence, &hay) {
            out.reject(why, item);
            continue;
        }
        let existing = match resolve(&c.id, &local) {
            Some(id) => {
                if c.new && !local.values().any(|v| *v == id) {
                    out.count("new_but_in_roster");
                }
                Some(id)
            }
            None => by_name.get(&alnum(&c.name)).cloned().inspect(|_| out.count("matched_by_name")),
        };
        let id = match existing {
            Some(id) => {
                local.insert(c.id.clone(), id.clone());
                if here.insert(id.clone()) && graph.characters.contains_key(&id) {
                    out.events.push(Event::Appeared { id: id.clone() });
                }
                id
            }
            None => {
                let grounded: Vec<&String> =
                    std::iter::once(&c.name).chain(&c.aliases).filter(|n| hay.contains(&alnum(n))).collect();
                let Some(name) = grounded.first().map(|n| (*n).clone()) else {
                    out.reject("name_not_in_text", item);
                    continue;
                };
                if name != c.name {
                    out.count("name_replaced_by_alias");
                }
                if !c.new {
                    out.count("undeclared_new");
                }
                let mut id = slug(&c.id);
                if id.is_empty() {
                    id = slug(&name);
                }
                while graph.characters.contains_key(&id) || local.values().any(|v| *v == id) {
                    id.push('_');
                }
                local.insert(c.id.clone(), id.clone());
                here.insert(id.clone());
                by_name.insert(alnum(&name), id.clone());
                known_alias.insert((id.clone(), alnum(&name)));
                out.events.push(Event::Introduced { id: id.clone(), name, evidence: c.evidence.clone() });
                id
            }
        };
        for a in &c.aliases {
            let key = alnum(a);
            if key.is_empty() || PRONOUNS.contains(&key.as_str()) {
                out.count("alias_pronoun");
                continue;
            }
            if !hay.contains(&key) {
                out.reject("alias_not_in_text", &json!({"id": id, "alias": a}));
                continue;
            }
            if known_alias.insert((id.clone(), key.clone())) {
                by_name.entry(key).or_insert_with(|| id.clone());
                out.events.push(Event::Alias { id: id.clone(), alias: a.clone() });
            }
        }
    }

    for item in reply["relations"].as_array().into_iter().flatten() {
        let Ok(r) = serde_json::from_value::<RelItem>(item.clone()) else {
            out.reject("malformed", item);
            continue;
        };
        let Some(kind) = Kind::parse(&r.kind) else {
            out.reject("bad_kind", item);
            continue;
        };
        let (Some(from), Some(to)) =
            (resolve_or_name(&r.from, &local, &by_name, &mut out), resolve_or_name(&r.to, &local, &by_name, &mut out))
        else {
            out.reject("unknown_id", item);
            continue;
        };
        if from == to {
            out.reject("self_relation", item);
            continue;
        }
        if let Err(why) = grounded_quote(&r.evidence, &hay) {
            out.reject(why, item);
            continue;
        }
        out.events.push(Event::Relation { from, to, kind, note: r.note, evidence: r.evidence });
    }

    for item in reply["same_as"].as_array().into_iter().flatten() {
        let Ok(s) = serde_json::from_value::<SameItem>(item.clone()) else {
            out.reject("malformed", item);
            continue;
        };
        let (Some(id), Some(other)) =
            (resolve_or_name(&s.id, &local, &by_name, &mut out), resolve_or_name(&s.other, &local, &by_name, &mut out))
        else {
            out.reject("unknown_id", item);
            continue;
        };
        if id == other {
            out.reject("self_same_as", item);
            continue;
        }
        if let Err(why) = grounded_quote(&s.evidence, &hay) {
            out.reject(why, item);
            continue;
        }
        out.events.push(Event::SameAs { id, other, evidence: s.evidence });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_match_across_punctuation_and_ellipses() {
        let hay = alnum("“Who’s left the gate open?” Mara asked the ferryman. Nobody answered her.");
        assert!(grounded_quote("\"Who's left the gate open?\" Mara asked the ferryman.", &hay).is_ok());
        assert!(grounded_quote("Who's left the gate open ... Nobody answered her", &hay).is_ok());
        assert_eq!(grounded_quote("Who left the gate open, Mara asked", &hay), Err("evidence_not_found"));
        assert_eq!(grounded_quote("Nobody answered ... Who's left the gate", &hay), Err("evidence_not_found"));
    }
}
