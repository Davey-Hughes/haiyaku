//! What the model is sent for one chapter: the chapter's text, the part of the roster the chapter needs, and the
//! rules of a reply.

use super::roster::Roster;
use crate::book::ChapterText;
use crate::log::RelationKind;

/// The version of [`SYSTEM`] and of the wording of [`prompt`] and [`reveal_prompt`], recorded with each extracted
/// book so that its log says which prompts wrote it: raise it whenever one of them changes.
pub const PROMPT_VERSION: u32 = 5;

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
    let mut prompt = heading(chapter);
    prompt.push_str(&lists(roster, chapter));
    prompt.push('\n');
    prompt.push_str(&rules());
    prompt
}

/// The second call's prompt for `chapter`, given `roster` once the first call's reply for the chapter is checked:
/// the chapter as [`prompt`] carries it, a line that the reader has finished it, the characters and relations in
/// [`prompt`]'s lists, then the task and the rules of a reply. Its schema is
/// [`reveal_schema`](super::reveal_schema), and [`check_reveals`](super::check_reveals) checks the reply.
///
/// The first call records what the chapter shows, and records a reveal only some of the time (O11): this one asks
/// only for what the chapter reveals or overturns (the design's "The second call"). It lists
/// the characters [`Roster::listed`] gives for the chapter after the first call's check, so the ones that call
/// introduced are among them, and the relations between them that the roster holds, the first call's included:
/// the ones an overturn may name.
pub fn reveal_prompt(roster: &Roster, chapter: &ChapterText) -> String {
    let mut prompt = heading(chapter);
    prompt.push_str("\nThe reader has just finished this chapter.\n");
    prompt.push_str(&lists(roster, chapter));
    prompt.push('\n');
    prompt.push_str(REVEAL_TASK);
    prompt.push('\n');
    prompt.push_str(&reveal_rules());
    prompt
}

/// The chapter's title and text.
fn heading(chapter: &ChapterText) -> String {
    format!("Chapter {}: {}\n\n{}\n", chapter.number, chapter.title, chapter.text())
}

/// The characters [`Roster::listed`] gives for `chapter`, and the relations between them, each list under its
/// heading.
fn lists(roster: &Roster, chapter: &ChapterText) -> String {
    let listed = roster.listed(chapter);
    let mut lists = String::from("\nCharacters so far (ref | name | other names):\n");
    if listed.is_empty() {
        lists.push_str("(none yet)\n");
    }
    for id in &listed {
        if let Some((name, others)) = roster.names_of(id) {
            lists.push_str(&row(&[id, name, &others.join(", ")]));
        }
    }
    let relations = relations(roster, &listed);
    if !relations.is_empty() {
        lists.push_str("\nRelations so far (from | kind | to | note):\n");
        for relation in &relations {
            lists.push_str(&row(relation));
        }
    }
    lists
}

/// How the order of a relation's two characters reads, in both calls' rules alike.
const DIRECTION: &str = "The order of from and to matters only for mentor, serves and owns, which run from the \
mentor, the servant and the owner (mentor: from regularly teaches or guides to; not a single act of help).";

/// How evidence is copied, in both calls' rules alike.
const EVIDENCE: &str = "evidence: a phrase or a sentence copied exactly from this chapter, character for character, \
that shows the item. Never paraphrase it: never a name where the text has a pronoun, never two passages joined with \
\"...\" or any other mark, never a word or its ending changed.";

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
{DIRECTION} A pair already listed above gains \
a further relation when this chapter reveals one of another kind, even one that contradicts what is listed: give \
it as well.
- same_as: only when this chapter reveals that two characters (listed above or in your characters list) are one. \
ref and other are the refs of two different characters: a character that only appears again is not a same_as.
- {EVIDENCE}
"
    )
}

/// The second call's task, a paragraph a line, word for word as the design gives it ("The second call", item 5).
/// The wording must not drift: the second call's prompt is part of the key its reply is cached under, so a changed
/// word asks every chapter again, and a golden test pins the prompt byte for byte.
const REVEAL_TASK: &str = "Read the chapter again. This step looks only for what the chapter reveals or overturns \
about these characters; another step already recorded what the chapter shows in passing.

relations: a relation between two of these characters that this chapter reveals and that the list above does not \
hold in any wording, such as a hidden allegiance, a secret act, or a relation of another kind for a pair already \
listed. Leave out what the chapter only shows in passing.

overturned: a relation in the list above that this chapter shows was not true, such as a character who seemed \
hostile but was protecting someone. Name it exactly as the list gives it: the same from, to and kind. Leave out a \
relation that changes later in the story but held when it was recorded.
";

/// The rules of a reply to the second call: what it holds and how each field of its items is written.
fn reveal_rules() -> String {
    let kinds = RelationKind::ALL.map(RelationKind::as_str).join(", ");
    format!(
        "Rules:
- Use only this chapter's text, read to its end, and the lists above. You may know this book and the series it \
belongs to: ignore everything you know of them beyond this chapter.
- from and to are refs from the characters list above.
- kind is one of: {kinds}. {DIRECTION}
- note is a few words on what this chapter shows (for example \"hid him from the guard\").
- {EVIDENCE}
- relations and overturned are empty lists when this chapter reveals or overturns nothing.
"
    )
}

/// The relations [`Roster::held`] holds between two of the `listed` characters, each as its from, kind, to and
/// note. A relation an overturn covers is left out: listed unmarked, it would invite the first call to state it
/// again, which would make it stand, and marked, it would change the list's format.
fn relations<'a>(roster: &'a Roster, listed: &[String]) -> Vec<[&'a str; 4]> {
    let is_listed = |id: &str| listed.iter().any(|listed| listed == id);
    roster
        .held()
        .into_iter()
        .filter(|held| is_listed(held.from) && is_listed(held.to))
        .map(|held| [held.from, held.kind.as_str(), held.to, held.note])
        .collect()
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
