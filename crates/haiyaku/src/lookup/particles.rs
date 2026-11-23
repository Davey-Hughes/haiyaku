//! The particles Korean writes onto a name, for the look-up only: after a name that ends in a Hangul syllable, a
//! tap may land on any word that starts with the name, so only these may run on (the design's "Particles, for the
//! look-up only"). Extraction, which looks for names it already expects, admits any run-on.

/// What a particle needs of the syllable written before it, by that syllable's final consonant.
#[derive(Debug, Clone, Copy)]
enum Needs {
    /// A final consonant.
    Consonant,
    /// A final consonant that is not ㄹ: the 으로-forms, since 로 itself follows a final ㄹ.
    ConsonantNotRieul,
    /// No final consonant.
    Vowel,
    /// No final consonant, or ㄹ: the 로-forms.
    VowelOrRieul,
    /// Any syllable.
    Either,
}

/// taiyaku's dictionary's list of particles, copied so that haiyaku depends on nothing of taiyaku's, with these
/// added after it: 네 for a family (모건네, the Morgans); the vocative 아, which calls a name that ends in a final
/// consonant as 야 calls one that does not (단아, 에이다야); and contractions common in dialogue (에겐 for 에게는).
const PARTICLES: [(&str, Needs); 83] = [
    ("은", Needs::Consonant),
    ("이", Needs::Consonant),
    ("을", Needs::Consonant),
    ("과", Needs::Consonant),
    ("이랑", Needs::Consonant),
    ("이나", Needs::Consonant),
    ("이야", Needs::Consonant),
    ("이에요", Needs::Consonant),
    ("이었다", Needs::Consonant),
    ("이라고", Needs::Consonant),
    ("이었고", Needs::Consonant),
    ("이라는", Needs::Consonant),
    ("이라도", Needs::Consonant),
    ("이었는데", Needs::Consonant),
    ("으로", Needs::ConsonantNotRieul),
    ("으로는", Needs::ConsonantNotRieul),
    ("으로서", Needs::ConsonantNotRieul),
    ("으로써", Needs::ConsonantNotRieul),
    ("는", Needs::Vowel),
    ("가", Needs::Vowel),
    ("를", Needs::Vowel),
    ("와", Needs::Vowel),
    ("랑", Needs::Vowel),
    ("나", Needs::Vowel),
    ("다", Needs::Vowel),
    ("야", Needs::Vowel),
    ("예요", Needs::Vowel),
    ("라고", Needs::Vowel),
    ("였다", Needs::Vowel),
    ("였고", Needs::Vowel),
    ("라는", Needs::Vowel),
    ("라도", Needs::Vowel),
    ("였는데", Needs::Vowel),
    ("로", Needs::VowelOrRieul),
    ("로는", Needs::VowelOrRieul),
    ("로서", Needs::VowelOrRieul),
    ("로써", Needs::VowelOrRieul),
    ("의", Needs::Either),
    ("에", Needs::Either),
    ("께", Needs::Either),
    ("도", Needs::Either),
    ("만", Needs::Either),
    ("들", Needs::Either),
    ("인", Needs::Either),
    ("이다", Needs::Either),
    ("에서", Needs::Either),
    ("에게", Needs::Either),
    ("한테", Needs::Either),
    ("께서", Needs::Either),
    ("까지", Needs::Either),
    ("부터", Needs::Either),
    ("처럼", Needs::Either),
    ("보다", Needs::Either),
    ("하고", Needs::Either),
    ("마다", Needs::Either),
    ("조차", Needs::Either),
    ("밖에", Needs::Either),
    ("들은", Needs::Either),
    ("들이", Needs::Either),
    ("들을", Needs::Either),
    ("들의", Needs::Either),
    ("이고", Needs::Either),
    ("만큼", Needs::Either),
    ("대로", Needs::Either),
    ("입니다", Needs::Either),
    ("에서는", Needs::Either),
    ("에서도", Needs::Either),
    ("에게서", Needs::Either),
    ("에게는", Needs::Either),
    ("한테서", Needs::Either),
    ("들에게", Needs::Either),
    ("네", Needs::Either),
    ("아", Needs::Consonant),
    ("이란", Needs::Consonant),
    ("이며", Needs::Consonant),
    ("이면", Needs::Consonant),
    ("란", Needs::Vowel),
    ("며", Needs::Vowel),
    ("면", Needs::Vowel),
    ("엔", Needs::Either),
    ("에겐", Needs::Either),
    ("한텐", Needs::Either),
    ("에선", Needs::Either),
];

/// Whether `word` ends in a Hangul syllable, after which only particles may run on.
pub(super) fn ends_in_hangul(word: &str) -> bool {
    word.chars().next_back().and_then(final_consonant).is_some()
}

/// Whether `rest` is one or two particles that may be written after `word`, each agreeing with the syllable
/// before it: the last of `word` for the first, the last of the first for the second.
pub(super) fn follow(word: &str, rest: &str) -> bool {
    // Every particle that starts `rest` is tried, since one may start another (이 and 이에요).
    particles_after(word).any(|first| {
        rest.strip_prefix(first)
            .is_some_and(|after| after.is_empty() || particles_after(first).any(|second| after == second))
    })
}

/// The particles that may be written after `word`: none when it does not end in a Hangul syllable.
fn particles_after(word: &str) -> impl Iterator<Item = &'static str> {
    let last = word.chars().next_back().and_then(final_consonant);
    PARTICLES
        .iter()
        .filter(move |(_, needs)| last.is_some_and(|last| agrees(*needs, last)))
        .map(|(particle, _)| *particle)
}

/// The final consonant of `c` when it is a Hangul syllable, as Unicode orders them: 0 for none, 8 for ㄹ.
fn final_consonant(c: char) -> Option<u32> {
    matches!(c, '\u{AC00}'..='\u{D7A3}').then(|| (u32::from(c) - 0xAC00) % 28)
}

/// Whether a particle that needs `needs` may follow a syllable whose final consonant is `last`.
fn agrees(needs: Needs, last: u32) -> bool {
    match needs {
        Needs::Consonant => last != 0,
        Needs::ConsonantNotRieul => last != 0 && last != 8,
        Needs::Vowel => last == 0,
        Needs::VowelOrRieul => last == 0 || last == 8,
        Needs::Either => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_particle_agrees_with_the_final_consonant_before_it() {
        // 로 follows ㄹ and 으로 does not; 으로 follows any other final consonant, and never a syllable without one.
        assert!(follow("빌", "로"));
        assert!(!follow("빌", "으로"));
        assert!(follow("단", "으로"));
        assert!(!follow("에이다", "으로"));
        // A second particle agrees with the first's last syllable: 은 needs a final consonant, and 게 has none.
        assert!(!follow("에이다", "에게은"));
    }

    #[test]
    fn the_vocative_and_the_contractions_agree_with_the_syllable_before_them() {
        // 아 and 이면 follow a final consonant, 면 a syllable without one.
        assert!(follow("단", "아"));
        assert!(!follow("에이다", "아"));
        assert!(follow("단", "이면"));
        assert!(follow("에이다", "면"));
    }
}
