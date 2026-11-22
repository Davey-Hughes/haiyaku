//! Where a chapter says a quote or a name. A quote is matched on its letters and digits alone, so a reply that
//! changes the case, the spacing or the punctuation of the book still finds it; a name is matched on whole
//! words, so it is not found inside a longer word.

use crate::book::ChapterText;
use crate::log::Position;

/// The weight (see [`weight`]) a quote needs to be evidence: eight Latin letters, enough for a fragment of a
/// sentence, or three Hangul syllables, enough for a name called out. A lighter quote could stand almost anywhere
/// in a chapter, and so says nothing of where the item it backs is found.
pub const MIN_QUOTE_WEIGHT: u32 = 8;

/// Why a quote is not found in a chapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteError {
    /// Its parts weigh less than [`MIN_QUOTE_WEIGHT`] together.
    TooShort,
    /// A part is not in the chapter, or not after the part before it.
    NotFound,
}

/// The block holding the start of `quote`, if the chapter says it.
///
/// The quote is split at `...` and `…`, and the parts with no letter or digit are dropped. Each part, reduced
/// to its letters and digits and lowercased, has to stand in the chapter's text reduced the same way, after the
/// part before it. The chapter's blocks are run together for this, so a quote may cross a paragraph break; the
/// block is the one holding the first letter of the first part.
///
/// # Errors
///
/// [`QuoteError::TooShort`] when the parts weigh less than [`MIN_QUOTE_WEIGHT`] together, whether or not the
/// chapter says them; [`QuoteError::NotFound`] when it does not say them in that order.
pub fn locate_quote(chapter: &ChapterText, quote: &str) -> Result<Position, QuoteError> {
    // A part with nothing left once reduced would match anywhere, and as the first part would place the quote
    // wherever the search starts.
    let parts: Vec<String> =
        quote.split("...").flat_map(|part| part.split('…')).map(reduce).filter(|part| !part.is_empty()).collect();
    if parts.iter().map(|part| weight(part)).sum::<u32>() < MIN_QUOTE_WEIGHT {
        return Err(QuoteError::TooShort);
    }
    let (first, rest) = parts.split_first().ok_or(QuoteError::TooShort)?;

    let mut text = String::new();
    // Where each block's reduced text starts in `text`, rising with the blocks.
    let mut starts = Vec::with_capacity(chapter.blocks.len());
    for block in &chapter.blocks {
        starts.push(text.len());
        text.push_str(&reduce(&block.text));
    }

    let start = text.find(first.as_str()).ok_or(QuoteError::NotFound)?;
    let mut end = start + first.len();
    for part in rest {
        end += text[end..].find(part.as_str()).ok_or(QuoteError::NotFound)? + part.len();
    }
    // The last block that starts at or before the match. A block with no letter or digit starts where the next
    // one does, so taking the last such start passes over it to the block the match is in.
    let block = starts.partition_point(|&block_start| block_start <= start) - 1;
    Ok(chapter.blocks[block].at.clone())
}

/// The first block in which `name` stands as whole words.
///
/// A text's words are its runs of letters and digits, lowercased, and a name with no words is in no block. The
/// name is in a block when its words are consecutive words of the block, each equal, except that the block's
/// word may run on past the name's last word when that word ends in a Hangul syllable, a Han character or a kana:
/// in Korean and Japanese a particle is written onto the name without a space. A name does not cross blocks.
pub fn locate_name(chapter: &ChapterText, name: &str) -> Option<Position> {
    let name = words(name);
    let (last, before) = name.split_last()?;
    let runs_on = last.chars().next_back().is_some_and(writes_a_syllable);
    let ends_the_name = |word: &String| if runs_on { word.starts_with(last.as_str()) } else { word == last };
    chapter
        .blocks
        .iter()
        .find(|block| {
            words(&block.text)
                .windows(name.len())
                .any(|run| run[..before.len()] == *before && ends_the_name(&run[before.len()]))
        })
        .map(|block| block.at.clone())
}

/// The first block in which `surname`, standing alone, names the character who carries it.
///
/// A surname of one letter or one syllable is in no block: it starts too many words to say who is meant (한
/// starts 한숨, a sigh). One with case (it has a letter whose upper and lower case differ) has to stand as whole
/// words written in its own case, since lowercased it is often a word of the language ("Bell", "the bell"). Any
/// other is found as [`locate_name`] finds a name, so a particle may be written onto it.
pub(crate) fn locate_surname(chapter: &ChapterText, surname: &str) -> Option<Position> {
    let letters: Vec<char> = surname.chars().filter(|c| c.is_alphanumeric()).collect();
    if letters.len() < 2 {
        return None;
    }
    if !letters.iter().any(|c| c.to_uppercase().ne(c.to_lowercase())) {
        return locate_name(chapter, surname);
    }
    let surname = written_words(surname);
    chapter
        .blocks
        .iter()
        .find(|block| written_words(&block.text).windows(surname.len()).any(|run| run == surname))
        .map(|block| block.at.clone())
}

/// How much of a sentence `text` carries: 3 for each Hangul syllable, Han character or kana, which writes a
/// syllable or a word on its own, and 1 for any other letter or digit. The halfwidth voiced and semi-voiced sound
/// marks weigh nothing: each marks the kana before it, which is counted, so a halfwidth syllable written in two
/// characters weighs what its fullwidth form does.
pub fn weight(text: &str) -> u32 {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| match c {
            '\u{FF9E}' | '\u{FF9F}' => 0,
            c if writes_a_syllable(c) => 3,
            _ => 1,
        })
        .sum()
}

/// The letters and digits of `text`, lowercased, each final sigma made a sigma.
///
/// Lowercased one letter at a time, a capital sigma is always `σ`, since nothing says where its word ends, while
/// lower-case text writes `ς` at the end of a word: without the second step a quote in capitals would not match
/// the same words in lower case. There is no Unicode normalisation, which would need tables the crate does not
/// carry: NFD is out of scope, so a letter written as a base and a combining mark does not match the same letter
/// written as one character.
pub(crate) fn reduce(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ς' { 'σ' } else { c })
        .collect()
}

/// The runs of letters and digits of `text`, lowercased.
fn words(text: &str) -> Vec<String> {
    written_words(text).into_iter().map(str::to_lowercase).collect()
}

/// The runs of letters and digits of `text`, as written.
fn written_words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect()
}

/// Whether `c` is a Hangul syllable, a Han character or a kana, the scripts in which one character writes a
/// syllable or more. The ranges also hold marks and punctuation, which is harmless because the callers ask only
/// of letters and digits.
fn writes_a_syllable(c: char) -> bool {
    matches!(
        c,
        // Hangul syllables.
        '\u{AC00}'..='\u{D7A3}'
        // Hiragana, katakana and its phonetic extensions, halfwidth katakana with its two sound marks, which end
        // a syllable written in two characters, and the kana supplements.
        | '\u{3040}'..='\u{30FF}'
        | '\u{31F0}'..='\u{31FF}'
        | '\u{FF66}'..='\u{FF9F}'
        | '\u{1B000}'..='\u{1B16F}'
        // CJK unified ideographs, extension A, the compatibility ideographs, the two planes of extensions, and
        // the iteration mark, which writes the ideograph before it again.
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{3FFFF}'
        | '\u{3005}'
    )
}
