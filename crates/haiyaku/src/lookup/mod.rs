//! Which characters a text names: the look-up behind a reader's tap on a word, given a cast and the text from
//! the start of the tapped word on. It reads nothing but the two, so the only names it knows are those the cast
//! carries, and a name the book gives only after the reader's position does not match.

mod particles;

use std::cmp::Reverse;
use std::iter;

use crate::fold::{Cast, Character};
use crate::log::Name;
use crate::words::{has_case, reduce, reduced_words, runs_on, stands_alone, word_spans, written_words};

/// Which of a character's names a mention matched, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NamePart {
    /// The name itself.
    Whole,
    /// The name's labelled given name.
    Given,
    /// The name's labelled surname.
    Surname,
}

/// A character a text names, and the match that names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Mention<'a> {
    pub character: &'a Character,
    /// The name that matched, or that carries the given name or surname that did: the shown name or an alias.
    pub name: &'a Name,
    pub part: NamePart,
    /// The match is `text[..len]`.
    pub len: usize,
}

/// The characters `text` names at its start, as far as `cast` knows them, most recently seen first.
///
/// `text` is meant to start at the word a reader tapped, and is read only as far as the longest name in the cast
/// has words. A word is a run of letters and digits, and words are compared reduced: lowercased, a final sigma
/// made a sigma. What comes before the first word, a quotation mark or a space, is passed over and is inside the
/// match, since a match is measured from the start of the text.
///
/// A character is tried under each of its names, the shown name and then its aliases in order, as a whole; then
/// under the labelled given name of each; then under the labelled surname of each. A given name or a surname is
/// taken from the name's own words: it is the first run of them that is the label's words, compared reduced, and it
/// is matched as the name writes it. So a surname labelled "Mcgregor" on Ada McGregor is matched as "McGregor", and
/// a given name labelled "Ada" on Adam Reed, where it is no word of the name, is not tried. A label of one word
/// that ends in a Hangul syllable, a Han character or a kana is tried as it is written when it is no run of the
/// name's words, as those scripts have no case and may write a name's parts without a space: 太郎 on 山田太郎.
/// Every rule below for a given name or a surname is put to the words so taken. A surname of one letter or one
/// syllable (한, which starts 한숨) is not tried. Nor is a name of one word tried as a whole when it is neither a
/// given name nor a surname (of [`Name::rank`] 0, as 엄마 or "Father" is), or when it has no given name and its
/// surname is of one letter or one syllable (an alias 한 labelled as the surname 한). A description has no part to
/// match alone, so it matches only from its first word: "the keeper" names the keeper, and "keeper" alone would be
/// a description of one word, which is left out.
///
/// A name of n words matches when the text's first n - 1 words are its first n - 1. Its last word must be the
/// text's next, except that when it ends in a Hangul syllable, a Han character or a kana the text's word need
/// only begin with it, since Korean and Japanese write a particle onto a name: what runs on is outside the match.
/// After a Hangul syllable what runs on must be one particle or two, each agreeing with the syllable before it;
/// after kana or a Han character it may be anything, as the crate has no word splitter for Japanese. So "Ron"
/// does not match "Ronald", 에이다 matches 에이다는 and 에이다에게는, and 단 does not match 단지. A name of one
/// word that has case, and that does not run on, must also be written in the case it is, or in capitals: "bill"
/// does not name Bill, "BILL" does. A match of two words or more, a surname of two words among them, is in any
/// case.
///
/// A surname whose last word has case also names its family with "s" on that word, or "es" when the word ends in
/// s, x, z, ch or sh: "Marshes" names every Marsh, "Holms" every Holm and "O'Dells" every O'Dell, but "Holmes"
/// names no Holm. The ending is held to the surname's rule for case: as written or in capitals for a surname of
/// one word, any case for one of two words or more. It is inside the match and is not a word of its own. In
/// Korean the particle 네 does the same (모건네).
///
/// Of two matches, the longer is the one of more words, or of as many words the one that covers more of the text (a
/// greater [`Mention::len`]), or of as many bytes too the one without a family's plural: "Fields" names Ann Fields
/// and not the family of Tom Field. A character's best match is its longest, and of equals the first in the order
/// above. Only the characters whose best match is the longest of all come back, each once, with that match: a text
/// that starts "Ned Marsh" names Ned Marsh and not every Marsh, and one that starts 山田太郎, a single word as it
/// is written without a space, names 山田太郎 and not every 山田. They come most recently seen first, by
/// [`Character::last`] in the order of [`Cast::books`], and those last seen in the same chapter in the order of the
/// cast.
pub fn characters_named<'a>(cast: &'a Cast, text: &str) -> Vec<Mention<'a>> {
    let candidates: Vec<_> = cast.characters.iter().map(|character| (character, candidates(character))).collect();
    let longest = candidates.iter().flat_map(|(_, of)| of).map(|candidate| candidate.written.len()).max().unwrap_or(0);
    let words: Vec<_> = word_spans(text)
        .take(longest)
        .map(|(start, written)| Word { start, written, reduced: reduce(written) })
        .collect();

    let best: Vec<_> = candidates.iter().filter_map(|(character, of)| best_match(character, of, &words)).collect();
    let most = best.iter().map(|(length, _)| *length).max();
    let mut mentions: Vec<_> =
        best.into_iter().filter(|(length, _)| Some(*length) == most).map(|(_, mention)| mention).collect();
    // Stable, so characters last seen in the same chapter keep the cast's order.
    mentions.sort_by_key(|mention| {
        let last = &mention.character.last;
        Reverse((cast.books.iter().position(|book| *book == last.book), last.chapter))
    });
    mentions
}

/// A word of the text: the byte offset it starts at, the word as written, and reduced.
#[derive(Debug)]
struct Word<'t> {
    start: usize,
    written: &'t str,
    reduced: String,
}

/// A name, or a labelled part of one, that a character may be named by.
#[derive(Debug)]
struct Candidate<'a> {
    name: &'a Name,
    part: NamePart,
    /// Its words as written, never empty.
    written: Vec<&'a str>,
    /// The same words, reduced.
    reduced: Vec<String>,
}

/// What `character` may be named by, in the order that settles which of its longest matches is reported: each
/// of its names whole, the shown name first and then the aliases in order; the given name of each; the surname
/// of each.
fn candidates(character: &Character) -> Vec<Candidate<'_>> {
    let names = || iter::once(&character.name).chain(&character.aliases);
    let whole = names().map(|name| (name, NamePart::Whole, Some(name.text.as_str())));
    let given = names().map(|name| (name, NamePart::Given, name.given.as_deref()));
    let surname = names().map(|name| (name, NamePart::Surname, name.surname.as_deref()));
    whole
        .chain(given)
        .chain(surname)
        .filter_map(|(name, part, label)| {
            let (written, reduced) = words_of(name, label?)?;
            Some(Candidate { name, part, written, reduced })
        })
        // One letter or one syllable starts too many words to say who is meant (O15).
        .filter(|candidate| candidate.part != NamePart::Surname || stands_alone(&candidate.written.concat()))
        // A one-word description (엄마, "Father") is a common word far more often than it names the character,
        // and a wrong card over every 엄마 costs more than a missing one. A one-syllable surname is held back from
        // matching alone for the same reason, as 한 and 이 are among the commonest words in Korean (O15); a name
        // of one word that is only that surname (an alias 한 labelled as the surname 한) would bring the card
        // back as a whole name.
        .filter(|candidate| {
            candidate.part != NamePart::Whole || candidate.written.len() > 1 || !too_common_alone(candidate.name)
        })
        .collect()
}

/// The words of `name` that `label` is, as the name writes them and reduced: the first run of the name's words
/// that, reduced, are the label's words, or else, for a label of one word that ends in a Hangul syllable, a Han
/// character or a kana, the label's own word. `None` otherwise, or when the label has no words, which keeps
/// [`Candidate::written`] never empty. For the name's own text that is every word it has.
///
/// Extraction keeps a label whose reduced text lies anywhere inside the reduced name, and keeps it as the model
/// wrote it, so a label may differ from the name in case ("Mcgregor" on Ada McGregor) or be part of one of its
/// words ("Ada" on Adam Reed). Matched as written, the first would miss the case rule of a one-word match
/// ("McGregor" would name nobody) and the second would name Adam Reed at every "Ada".
fn words_of<'a>(name: &'a Name, label: &'a str) -> Option<(Vec<&'a str>, Vec<String>)> {
    let (written, reduced): (Vec<_>, Vec<_>) = (written_words(label), reduced_words(label).collect());
    if reduced.is_empty() {
        return None;
    }
    let words = written_words(&name.text);
    let name_reduced: Vec<_> = reduced_words(&name.text).collect();
    match name_reduced.windows(reduced.len()).position(|run| run == reduced.as_slice()) {
        Some(at) => Some((words[at..at + reduced.len()].to_vec(), reduced)),
        // Korean and Japanese have no case and may write a name's parts without a space (山田太郎), so a label in
        // those scripts that lies inside a word of the name is a part of it, and its own spelling is the name's.
        None if written.len() == 1 && runs_on(written[0]) => Some((written, reduced)),
        None => None,
    }
}

/// Whether `name`, of one word, is too common a word to be tried as a whole: a description, or a surname of one
/// letter or one syllable with no given name beside it, the given name and the surname as [`words_of`] takes them
/// from the name.
fn too_common_alone(name: &Name) -> bool {
    let given = name.given.as_deref().and_then(|label| words_of(name, label));
    let surname = name.surname.as_deref().and_then(|label| words_of(name, label));
    name.rank() == 0 || (given.is_none() && surname.is_some_and(|(surname, _)| !stands_alone(&surname.concat())))
}

/// How long a match is, as the longest match is judged: in words, then in bytes, then a match without a family's
/// plural over one with it. The derived order compares the fields in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Length {
    words: usize,
    /// The match is `text[..bytes]`.
    bytes: usize,
    /// A family's plural counts toward `bytes`, so at "Fields" the Field family would tie with Ann Fields, whose
    /// surname the word is as written.
    without_plural: bool,
}

/// The match of `character` at the start of the text whose first words are `words`, with its length: of the
/// candidates that match, the first of the longest.
fn best_match<'a>(
    character: &'a Character,
    candidates: &[Candidate<'a>],
    words: &[Word],
) -> Option<(Length, Mention<'a>)> {
    candidates
        .iter()
        .filter_map(|candidate| {
            let length = candidate.length_in(words)?;
            let mention = Mention { character, name: candidate.name, part: candidate.part, len: length.bytes };
            Some((length, mention))
        })
        // `min_by_key` keeps the first of equals, which is the tie-break the candidates' order sets.
        .min_by_key(|(length, _)| Reverse(*length))
}

impl Candidate<'_> {
    /// The length of the match this candidate makes, when it starts the text whose first words are `words`.
    fn length_in(&self, words: &[Word]) -> Option<Length> {
        let (last, before) = words.get(..self.written.len())?.split_last()?;
        if before.iter().zip(&self.reduced).any(|(word, reduced)| word.reduced != *reduced) {
            return None;
        }
        let (covered, plural) = self.ends_in(last)?;
        Some(Length { words: self.written.len(), bytes: last.start + covered, without_plural: !plural })
    }

    /// How many bytes of `word`, the text's word in the place of the candidate's last, the match covers, and
    /// whether they end in a family's plural; `None` when the candidate does not end there. Every rule for the last
    /// word is here or in [`Self::endings`]: run-on, the particles after a Hangul syllable, case, and a family's
    /// plural.
    fn ends_in(&self, word: &Word) -> Option<(usize, bool)> {
        let (last, reduced) = (*self.written.last()?, self.reduced.last()?);
        if runs_on(last) {
            // The text's word begins with the name's last word: its first characters, as many as that word has.
            let mut boundaries = word.written.char_indices().map(|(at, _)| at).chain(iter::once(word.written.len()));
            let end = boundaries.nth(last.chars().count())?;
            // A tap lands on whatever word the reader touched, so after a Hangul syllable only particles may run
            // on, or 단 would name 단지; after kana or a Han character anything may, until a word splitter (O9).
            let (head, rest) = word.written.split_at(end);
            let ends_here = reduce(head) == *reduced
                && (rest.is_empty() || !particles::ends_in_hangul(last) || particles::follow(last, rest));
            ends_here.then_some((end, false))
        } else if self.written.len() == 1 && has_case(last) {
            // One word with case has only its capital to tell the name ("Bill") from the common word ("bill");
            // a word in capitals, as in a heading or a shout, shows no case to go by. A family's plural is held to
            // the same case.
            let ending = self.endings().iter().copied().find(|ending| {
                word.written.strip_suffix(ending) == Some(last)
                    || (in_capitals(word.written) && word.reduced.strip_suffix(ending) == Some(reduced.as_str()))
            })?;
            Some((word.written.len(), !ending.is_empty()))
        } else {
            // Two words or more tell a name from common words without its case, so they compare reduced, a family's
            // plural on a surname of two words or more among them ("the o'dells"); so does a word without case,
            // which has none to go by.
            let ending = self
                .endings()
                .iter()
                .copied()
                .find(|ending| word.reduced.strip_suffix(ending) == Some(reduced.as_str()))?;
            Some((word.written.len(), !ending.is_empty()))
        }
    }

    /// The endings the text's word may add to the candidate's last word: none but a family's plural, on a surname
    /// whose last word has case. A surname with "s" on names the family ("the Reeds"), and with "es" only after s,
    /// x, z, ch or sh ("the Marshes"), as English spells the plural: "Holmes" is a name of its own, not the Holms.
    /// The ending is judged on the reduced word, so that a surname written in capitals ends as it does in lower
    /// case.
    fn endings(&self) -> &'static [&'static str] {
        let family = self.part == NamePart::Surname && self.written.last().is_some_and(|last| has_case(last));
        let sibilant =
            self.reduced.last().is_some_and(|last| ["s", "x", "z", "ch", "sh"].iter().any(|end| last.ends_with(end)));
        if !family {
            &[""]
        } else if sibilant {
            &["", "s", "es"]
        } else {
            &["", "s"]
        }
    }
}

/// Whether `word` is written in capitals: it has an upper-case letter and no lower-case one.
fn in_capitals(word: &str) -> bool {
    word.chars().any(char::is_uppercase) && !word.chars().any(char::is_lowercase)
}
