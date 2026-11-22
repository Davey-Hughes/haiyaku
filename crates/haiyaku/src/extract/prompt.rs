//! What the model is sent for one chapter: the chapter's text, the part of the roster the chapter needs, and the
//! rules of a reply.

use std::collections::HashSet;

use super::roster::Roster;
use crate::book::ChapterText;
use crate::log::{EventBody, RelationKind};

/// The version of [`SYSTEM`] and of [`prompt`]'s wording, recorded with each extracted book so that its log
/// says which prompt wrote it: raise it whenever either changes.
pub const PROMPT_VERSION: u32 = 4;

/// The system text of every chapter's call.
pub const SYSTEM: &str = "You record the characters of a book one chapter at a time, for a reader who has read to \
                          the end of this chapter and no further, and you reply with JSON only.";

/// The prompt for `chapter`, given `roster`, the book before it: the chapter's title and text, the characters
/// [`Roster::listed`] gives for the chapter and the relations between them, then the rules of a reply.
///
/// The chapter, the longest part by far, comes first and the rules last, where a model given a long document
/// follows instructions best.
///
/// It lists the ids [`check`](super::check) takes as known characters and no others, so the two agree on which
/// ids the model was shown. A character is listed under the name it is shown under and its other names, in id
/// order.
pub fn prompt(roster: &Roster, chapter: &ChapterText) -> String {
    let listed = roster.listed(chapter);
    let mut prompt = format!("Chapter {}: {}\n\n{}\n", chapter.number, chapter.title, chapter.text());
    prompt.push_str("\nCharacters so far (ref | name | other names):\n");
    if listed.is_empty() {
        prompt.push_str("(none yet)\n");
    }
    for id in &listed {
        if let Some((name, others)) = roster.names_of(id) {
            prompt.push_str(&row(&[id, name, &others.join(", ")]));
        }
    }
    let relations = relations(roster, &listed);
    if !relations.is_empty() {
        prompt.push_str("\nRelations so far (from | kind | to | note):\n");
        for relation in &relations {
            prompt.push_str(&row(relation));
        }
    }
    prompt.push('\n');
    prompt.push_str(&rules());
    prompt
}

/// The rules of a reply: what it holds and how each of its fields is written.
fn rules() -> String {
    let kinds = RelationKind::ALL.map(RelationKind::as_str).join(", ");
    format!(
        "Rules:
- Use only what this chapter's text and the lists above say. You may know this book: ignore everything you know \
about it beyond them. Never state a name, identity, relationship or fact that they have not shown by the end of \
this chapter.
- characters: every named character who appears, speaks or is described in this chapter. Include an unnamed \
character only if they matter to what happens; leave out bystanders and crowds (a secretary, passers-by, people \
in the street) and people only mentioned in passing.
  - A character listed above under \"Characters so far\" keeps its ref. A character called by a personal name \
listed there (a given name or a surname, not a description or a form of address) is that listed character, unless \
this chapter shows it is someone else; one called by a listed description or form of address (\"the stranger\", \
\"Mum\") is that listed character only when this chapter shows it is. Any other character gets a ref of its own \
that is not listed (new1, new2, ...).
  - name: for a listed character, the name listed for it, unless this chapter gives it a fuller one: then that \
name. For a new character, the fullest name this chapter gives; an unnamed character gets a short description \
copied from the text.
  - aliases: the other names and forms of address this chapter uses for the character (for example \"Mr. Smith\", \
\"the Headmaster\"). Never a pronoun.
  - In each name and alias, text is the name copied exactly as this chapter writes it, or as the list writes it \
for a listed name. A name you give from this chapter is one run of words that stands in it in that order and \
form: never a name joined from parts the chapter writes apart (when the chapter writes \"Ann and Joe Bell\", \
Ann's name is \"Ann\"), never \"Given Surname\" built from \"Surname, Given\", and never a description in your own \
words (\"a man in a grey coat\" as the chapter has it, not \"the grey-coated stranger\"). given, surname and title \
label the parts of it that are a given name, a surname and a title, and are null when it has no such part; a \
title may follow the name. A description (\"the tall woman\", \"엄마\") has no labels. title_sex is \"male\" or \
\"female\" only when the title itself tells it, and null otherwise.
- relations: relationships this chapter states or clearly shows. from and to are refs listed above or in your \
characters list. kind is one of: {kinds}. note is a few words (for example \"uncle\", \"best friend\"). \
The order of from and to matters only for mentor, serves and owns, which run from the mentor, the servant and the \
owner (mentor: from regularly teaches or guides to; not a single act of help). A pair already listed above gains \
a further relation when this chapter reveals one of another kind, even one that contradicts what is listed: give \
it as well.
- same_as: only when this chapter reveals that two characters (listed above or in your characters list) are one. \
ref and other are the refs of two different characters: a character that only appears again is not a same_as.
- evidence: a phrase or a sentence copied exactly from this chapter, character for character, that shows the \
item. Never paraphrase it: never a name where the text has a pronoun, never two passages joined with \"...\" or any \
other mark, never a word or its ending changed.
"
    )
}

/// The relations between two of the `listed` characters that `roster`'s events state, each as its from, kind, to
/// and note.
///
/// Each end is taken for its survivor first ([`Roster::survivor`]), so a merge does not drop a relation from
/// every later prompt, and a relation whose two ends are now one character is not listed. A fact is listed once,
/// in the order first stated and under the note first given, as the fold holds it.
fn relations<'a>(roster: &'a Roster, listed: &[String]) -> Vec<[&'a str; 4]> {
    let is_listed = |id: &str| listed.iter().any(|listed| listed == id);
    let mut facts = HashSet::new();
    let mut relations = Vec::new();
    for event in roster.events() {
        let EventBody::Relation { from, to, kind, note } = &event.body else { continue };
        let (from, to) = (roster.survivor(from), roster.survivor(to));
        if from == to || !is_listed(from) || !is_listed(to) {
            continue;
        }
        // A kind that is not directed holds between the two alike, so its fact is one whichever way it is stated.
        let fact = if kind.directed() || from < to { (kind.as_str(), from, to) } else { (kind.as_str(), to, from) };
        if facts.insert(fact) {
            relations.push([from, kind.as_str(), to, note.as_str()]);
        }
    }
    relations
}

/// One line of a list: its cells, each as [`cell`] writes it, joined by ` | `, with no space left at its end when
/// the last cell is empty.
fn row(cells: &[&str]) -> String {
    let cells: Vec<String> = cells.iter().map(|text| cell(text)).collect();
    format!("{}\n", cells.join(" | ").trim_end())
}

/// `text` as a cell of a list: each `|` made `/`, and each run of line breaks one space. A bar would read as the
/// end of the cell and a line break as the end of the row, so the model would take one name or note for two. A
/// name is matched on its letters and digits alone, so a model that copies the cell as written still names it.
fn cell(text: &str) -> String {
    let mut cell = String::with_capacity(text.len());
    let mut after_break = false;
    for c in text.chars() {
        let breaks = matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}');
        if !breaks {
            cell.push(if c == '|' { '/' } else { c });
        } else if !after_break {
            cell.push(' ');
        }
        after_break = breaks;
    }
    cell
}
