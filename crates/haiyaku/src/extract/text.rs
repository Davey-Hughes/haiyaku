//! Where a chapter says a quote or a name. A quote is matched on its letters and digits alone, so a reply that
//! changes the case, the spacing or the punctuation of the book still finds it; a name is matched on whole
//! words, so it is not found inside a longer word.

use crate::book::ChapterText;
use crate::log::Position;
use crate::words::{has_case, reduce, runs_on, stands_alone, writes_a_syllable, written_words};

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
    let mut text = String::new();
    // Where each block's reduced text starts in `text`, rising with the blocks.
    let mut starts = Vec::with_capacity(chapter.blocks.len());
    for block in &chapter.blocks {
        starts.push(text.len());
        text.push_str(&reduce(&block.text));
    }

    let start = find_quote(&text, quote)?;
    // The last block that starts at or before the match. A block with no letter or digit starts where the next
    // one does, so taking the last such start passes over it to the block the match is in.
    let block = starts.partition_point(|&block_start| block_start <= start) - 1;
    Ok(chapter.blocks[block].at.clone())
}

/// Whether `text` holds `quote`, as [`locate_quote`] finds a quote in a chapter whose one block is `text`.
///
/// # Errors
///
/// Those of [`locate_quote`].
pub(crate) fn quote_in(text: &str, quote: &str) -> Result<(), QuoteError> {
    find_quote(&reduce(text), quote).map(|_| ())
}

/// Where `quote` starts in `text`, which is reduced already, by the rule [`locate_quote`] states. It is the
/// whole of that rule, shared with [`quote_in`] so that the two cannot come to disagree.
fn find_quote(text: &str, quote: &str) -> Result<usize, QuoteError> {
    // A part with nothing left once reduced would match anywhere, and as the first part would place the quote
    // wherever the search starts.
    let parts: Vec<String> =
        quote.split("...").flat_map(|part| part.split('…')).map(reduce).filter(|part| !part.is_empty()).collect();
    if parts.iter().map(|part| weight(part)).sum::<u32>() < MIN_QUOTE_WEIGHT {
        return Err(QuoteError::TooShort);
    }
    let (first, rest) = parts.split_first().ok_or(QuoteError::TooShort)?;

    let start = text.find(first.as_str()).ok_or(QuoteError::NotFound)?;
    let mut end = start + first.len();
    for part in rest {
        end += text[end..].find(part.as_str()).ok_or(QuoteError::NotFound)? + part.len();
    }
    Ok(start)
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
    let may_run_on = runs_on(last);
    let ends_the_name = |word: &String| if may_run_on { word.starts_with(last.as_str()) } else { word == last };
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
    if !stands_alone(surname) {
        return None;
    }
    if !has_case(surname) {
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

/// The runs of letters and digits of `text`, lowercased. Lowercased as a whole and not reduced: `str::to_lowercase`
/// keeps a final `ς` where [`reduce`] makes it `σ`, so reducing here would change which names extraction finds
/// in Greek.
fn words(text: &str) -> Vec<String> {
    written_words(text).into_iter().map(str::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::PlacedBlock;

    #[test]
    fn quote_in_finds_what_locate_quote_finds() {
        let text = "\u{201c}The lamp at Gull Point burned all night,\u{201d} Ada said, and counted every step.";
        let chapter = ChapterText {
            number: 1,
            toc_entry: 1,
            title: "Chapter 1".into(),
            documents: 0..1,
            blocks: vec![PlacedBlock { at: Position { document: 0, block: 0 }, text: text.into() }],
        };
        for (quote, holds) in [
            ("the LAMP, at gull-point,   burned all NIGHT", Ok(())),
            ("The lamp... counted every step", Ok(())),
            ("Gull Point \u{2026} Ada said...", Ok(())),
            ("Ada", Err(QuoteError::TooShort)),
            ("Ada... said", Err(QuoteError::TooShort)),
            ("Ada sailed for the mainland", Err(QuoteError::NotFound)),
            ("counted every step... The lamp at Gull Point", Err(QuoteError::NotFound)),
        ] {
            assert_eq!(quote_in(text, quote), holds, "{quote}");
            assert_eq!(locate_quote(&chapter, quote).map(|_| ()), holds, "{quote}");
        }
    }
}
