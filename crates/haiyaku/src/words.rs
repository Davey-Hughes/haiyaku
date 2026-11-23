//! The word rules extraction, linking and the look-up share: what a word of a text is, how words are reduced to be
//! compared, and the tests a name's words are put to before they are looked for. Kept in one place so that the
//! callers cannot come to disagree on them.

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

/// The words of `text`, each with the byte offset it starts at. A word is a run of letters and digits (characters
/// for which [`char::is_alphanumeric`] holds); every other character ends one. This is the one definition of a
/// word the crate has, so every rule that compares words splits a text in the same places.
pub(crate) fn word_spans(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut chars = text.char_indices();
    std::iter::from_fn(move || {
        let (start, _) = chars.find(|&(_, c)| c.is_alphanumeric())?;
        let end = chars.find(|&(_, c)| !c.is_alphanumeric()).map_or(text.len(), |(end, _)| end);
        Some((start, &text[start..end]))
    })
}

/// The words of `text`, as written.
pub(crate) fn written_words(text: &str) -> Vec<&str> {
    word_spans(text).map(|(_, word)| word).collect()
}

/// The words of `text`, each reduced.
pub(crate) fn reduced_words(text: &str) -> impl Iterator<Item = String> {
    word_spans(text).map(|(_, word)| reduce(word))
}

/// Whether `c` is a Hangul syllable, a Han character or a kana, the scripts in which one character writes a
/// syllable or more. The ranges also hold marks and punctuation, which is harmless because the callers ask only
/// of letters and digits.
pub(crate) fn writes_a_syllable(c: char) -> bool {
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

/// Whether a word of a text may run on past `word`, the last word of a name, and still end the name: it may when
/// `word` ends in a Hangul syllable, a Han character or a kana, since in Korean and Japanese a particle is written
/// onto the name without a space.
pub(crate) fn runs_on(word: &str) -> bool {
    word.chars().next_back().is_some_and(writes_a_syllable)
}

/// Whether `text` has case: a letter or digit whose upper and lower case differ. A name with case can be told
/// from a common word by its capital ("Bell", "the bell"); one without has nothing to tell them apart.
pub(crate) fn has_case(text: &str) -> bool {
    text.chars().filter(|c| c.is_alphanumeric()).any(|c| c.to_uppercase().ne(c.to_lowercase()))
}

/// Whether `surname` is long enough to name its character standing alone: two letters or digits or more. A surname
/// of one letter or one syllable starts too many words to say who is meant (한 starts 한숨, a sigh); see O15 in
/// `docs/status.md`.
pub(crate) fn stands_alone(surname: &str) -> bool {
    surname.chars().filter(|c| c.is_alphanumeric()).count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_spans_give_each_word_and_its_offset() {
        let spans = |text| word_spans(text).collect::<Vec<_>>();
        assert_eq!(spans("\u{201c}Ada,\u{201d} said Mr. Marsh"), [(3, "Ada"), (11, "said"), (16, "Mr"), (20, "Marsh")]);
        assert_eq!(spans("에이다는 웃었다"), [(0, "에이다는"), (13, "웃었다")]);
        assert_eq!(spans("奈々は灯台へ行った。"), [(0, "奈々は灯台へ行った")]);
        assert_eq!(spans(""), []);
        assert_eq!(spans("\u{2014} !"), []);
    }
}
