//! The name rules of the link step: which of an earlier character's names make it a candidate for a later
//! character's name, and which of the two characters' names turn the candidate down. Every part is compared
//! reduced (see [`reduce`]), so case and punctuation do not keep two names apart.

use crate::extract::reduce;
use crate::log::Name;

/// `name` with its title and a leading "the" set aside: the words of its text, each reduced, less the words of
/// its title and less a first word "the", joined by one space. The title's words are set aside wherever they
/// stand, since a title may come after the name as well as before it ("Mrs. Marsh", "마시 부인").
pub(crate) fn bare(name: &Name) -> String {
    let title: Vec<String> = name.title.as_deref().map(|title| words(title).collect()).unwrap_or_default();
    let mut kept: Vec<String> = words(&name.text).filter(|word| !title.contains(word)).collect();
    // An article says nothing of who is meant, so "the old sailor" and "old sailor" are one description.
    if kept.first().is_some_and(|word| word == "the") {
        kept.remove(0);
    }
    kept.join(" ")
}

/// Whether `later` could name the character `earlier` names: they [`share`] a name, and their titles do not state
/// two sexes ([`other_sex`]).
pub(crate) fn matches(later: &Name, earlier: &Name) -> bool {
    !other_sex(later, earlier) && share(later, earlier)
}

/// Whether each of `later` and `earlier` has a title that states a sex, and the two sexes differ, as with "Mr.
/// Hale" and "Mrs. Hale".
pub(crate) fn other_sex(later: &Name, earlier: &Name) -> bool {
    later.title_sex.zip(earlier.title_sex).is_some_and(|(later, earlier)| later != earlier)
}

/// Whether `later` and `earlier` have the same given name, or the same surname, or the same [`bare`] form with
/// something left in it (a description, a nickname), whatever their titles say.
pub(crate) fn share(later: &Name, earlier: &Name) -> bool {
    let same = |label: fn(&Name) -> &Option<String>| {
        part_of(label(later)).is_some_and(|own| part_of(label(earlier)) == Some(own))
    };
    if same(|name| &name.given) || same(|name| &name.surname) {
        return true;
    }
    let whole = bare(later);
    !whole.is_empty() && whole == bare(earlier)
}

/// Whether `later`'s names and `earlier`'s are of two people: both sides have a given name and no given name of
/// either side is used by the other side's names, or likewise for surnames. A part is used by names when it is
/// one of their given names or surnames or a word of one of their texts, so a short form one side uses ("Tom")
/// keeps a fuller one on the other ("Thomas") from turning the candidate down.
pub(crate) fn conflict(later: &[&Name], earlier: &[&Name]) -> bool {
    apart(later, earlier, |name| &name.given) || apart(later, earlier, |name| &name.surname)
}

/// Whether both sides have the part `label` picks out of a name, and no such part of either side is used by the
/// other side's names.
fn apart(later: &[&Name], earlier: &[&Name], label: fn(&Name) -> &Option<String>) -> bool {
    let parts = |names: &[&Name]| -> Vec<String> { names.iter().filter_map(|name| part_of(label(name))).collect() };
    let (later_parts, earlier_parts) = (parts(later), parts(earlier));
    !later_parts.is_empty()
        && !earlier_parts.is_empty()
        && !later_parts.iter().any(|part| used(part, earlier))
        && !earlier_parts.iter().any(|part| used(part, later))
}

/// Whether `part`, reduced already, is a given name or a surname of one of `names`, or a word of one of their
/// texts.
fn used(part: &str, names: &[&Name]) -> bool {
    names.iter().any(|name| {
        [&name.given, &name.surname].into_iter().any(|label| part_of(label).as_deref() == Some(part))
            || words(&name.text).any(|word| word == part)
    })
}

/// A labelled part of a name, reduced. One that reduces to nothing is no part: it would equal any other such.
fn part_of(label: &Option<String>) -> Option<String> {
    label.as_deref().map(reduce).filter(|part| !part.is_empty())
}

/// The runs of letters and digits of `text`, each reduced.
fn words(text: &str) -> impl Iterator<Item = String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).map(reduce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Sex;

    fn name(text: &str, given: Option<&str>, surname: Option<&str>) -> Name {
        Name {
            text: text.into(),
            given: given.map(Into::into),
            surname: surname.map(Into::into),
            title: None,
            title_sex: None,
        }
    }

    fn titled(name: Name, title: &str, sex: Option<Sex>) -> Name {
        Name { title: Some(title.into()), title_sex: sex, ..name }
    }

    #[test]
    fn bare_sets_aside_the_title_and_a_leading_the() {
        assert_eq!(bare(&name("the old sailor", None, None)), "old sailor");
        assert_eq!(bare(&titled(name("Mrs. Marsh", None, Some("Marsh")), "Mrs.", Some(Sex::Female))), "marsh");
        assert_eq!(bare(&titled(name("Captain Reed", None, Some("Reed")), "Captain", None)), "reed");
        // A title is set aside only when it is labelled, and "the" only as the first word.
        assert_eq!(bare(&name("Captain Reed", None, Some("Reed"))), "captain reed");
        assert_eq!(bare(&name("Keeper of the Keys", None, None)), "keeper of the keys");
    }

    #[test]
    fn korean_title_after_the_name_is_set_aside() {
        let madam = titled(name("마시 부인", None, Some("마시")), "부인", Some(Sex::Female));
        assert_eq!(bare(&madam), "마시");
        assert!(matches(&madam, &name("에이다 마시", Some("에이다"), Some("마시"))));
        assert!(!matches(&madam, &titled(name("마시 씨", None, Some("마시")), "씨", Some(Sex::Male))));
    }

    #[test]
    fn names_match_on_a_part_or_whole() {
        let ada = name("Ada Marsh", Some("Ada"), Some("Marsh"));
        assert!(matches(&name("Ada", Some("Ada"), None), &ada));
        assert!(matches(&name("Ned Marsh", Some("Ned"), Some("Marsh")), &ada));
        assert!(matches(&name("The Old Sailor", None, None), &name("old sailor", None, None)));

        assert!(!matches(&name("Ned Hale", Some("Ned"), Some("Hale")), &ada));
        assert!(!matches(&name("the old sailor", None, None), &name("the old keeper", None, None)));
        // Parts with no letter or digit, and bare forms with nothing left, are nothing to share.
        assert!(!matches(&name("Ada", Some("."), None), &name("Ned", Some("."), None)));
        let sir = || titled(name("Sir", None, None), "Sir", Some(Sex::Male));
        assert!(!matches(&sir(), &sir()));

        let mr = titled(name("Mr. Hale", None, Some("Hale")), "Mr.", Some(Sex::Male));
        let mrs = titled(name("Mrs. Hale", None, Some("Hale")), "Mrs.", Some(Sex::Female));
        assert!(!matches(&mr, &mrs));
        assert!(!matches(&mrs, &mr));
        // A sex stated on one side only turns nothing down.
        assert!(matches(&mr, &name("Tom Hale", Some("Tom"), Some("Hale"))));
        // The two names share a surname; only the titles keep them apart.
        assert!(share(&mr, &mrs) && other_sex(&mr, &mrs));
    }

    #[test]
    fn conflict_needs_a_part_on_both_sides() {
        let ada = name("Ada Marsh", Some("Ada"), Some("Marsh"));
        let mrs = titled(name("Mrs. Marsh", None, Some("Marsh")), "Mrs.", Some(Sex::Female));
        let earlier = [&ada, &mrs];
        assert!(conflict(&[&name("Ned Marsh", Some("Ned"), Some("Marsh"))], &earlier));
        assert!(conflict(&[&name("Ada Hale", Some("Ada"), Some("Hale"))], &earlier));
        assert!(!conflict(&[&titled(name("Mr. Marsh", None, Some("Marsh")), "Mr.", Some(Sex::Male))], &earlier));
    }

    #[test]
    fn a_part_the_other_side_uses_is_no_conflict() {
        let thomas = name("Thomas Reed", Some("Thomas"), Some("Reed"));
        let tom_reed = name("Tom Reed", Some("Tom"), Some("Reed"));
        assert!(conflict(&[&thomas], &[&tom_reed]));
        // "Tom" uses the earlier given name, labelled as one or only a word of the text.
        assert!(!conflict(&[&thomas, &name("Tom", Some("Tom"), None)], &[&tom_reed]));
        assert!(!conflict(&[&thomas, &name("Tom", None, None)], &[&tom_reed]));
        assert!(!conflict(&[&tom_reed], &[&thomas, &name("Tom", None, None)]));
    }
}
