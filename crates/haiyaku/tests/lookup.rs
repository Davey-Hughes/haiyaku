//! The look-up of the characters a text names at its start, over casts of invented names.

mod common;

use NamePart::{Given, Surname, Whole};
use common::{db, keeper, keepers_links, keepers_logs, keepers_return};
use haiyaku::fold::{Cast, EntityKey, fold_book};
use haiyaku::log::{BookLog, Event, EventBody, Name, Position, Sex};
use haiyaku::store::write_book;
use haiyaku::{NamePart, Scope, cast_at, characters_named, fold_series};

/// The cast of [`keeper`]'s book with these events in place of its own, each given with its chapter, for a reader
/// who has finished chapter 3.
fn cast_of(bodies: Vec<(u32, EventBody)>) -> Cast {
    let events = bodies.into_iter().zip(1..).map(|((chapter, body), seq)| Event {
        seq,
        chapter,
        at: Position { document: chapter, block: seq },
        quote: format!("quote {seq}"),
        body,
    });
    fold_book(&BookLog { events: events.collect(), ..keeper() }, 3)
}

/// What `text` names in `cast`: each mention as its character's id, the part that matched, the text of the
/// name that matched or carries that part, and the length of the match.
fn found<'a>(cast: &'a Cast, text: &str) -> Vec<(&'a str, NamePart, &'a str, usize)> {
    characters_named(cast, text)
        .into_iter()
        .map(|mention| (mention.character.key.id.as_str(), mention.part, mention.name.text.as_str(), mention.len))
        .collect()
}

fn name(text: &str, given: Option<&str>, surname: Option<&str>) -> Name {
    Name {
        text: text.into(),
        given: given.map(Into::into),
        surname: surname.map(Into::into),
        title: None,
        title_sex: None,
    }
}

fn titled(title: &str, title_sex: Option<Sex>, name: Name) -> Name {
    Name { title: Some(title.into()), title_sex, ..name }
}

fn introduced(entity: &str, name: Name) -> EventBody {
    EventBody::Introduced { entity: entity.into(), name }
}

fn alias(entity: &str, name: Name) -> EventBody {
    EventBody::Alias { entity: entity.into(), name }
}

fn appeared(entity: &str) -> EventBody {
    EventBody::Appeared { entity: entity.into() }
}

/// Ada is last seen in chapter 3, Ned in 2 and the others in 1. Ned Marsh shares his given name with Ned Hale
/// and his surname with Ada; "the keeper", "Father" and "Professor" are descriptions.
fn english() -> Cast {
    cast_of(vec![
        (1, introduced("ned", name("Ned Marsh", Some("Ned"), Some("Marsh")))),
        (1, introduced("ada", name("Ada Marsh", Some("Ada"), Some("Marsh")))),
        (1, introduced("neddy", name("Ned Hale", Some("Ned"), Some("Hale")))),
        (1, introduced("bill", name("Bill Reed", Some("Bill"), Some("Reed")))),
        (1, introduced("ron", name("Ron", Some("Ron"), None))),
        (1, introduced("keeper", name("the keeper", None, None))),
        (1, introduced("father", name("Father", None, None))),
        (1, introduced("professor", titled("Professor", None, name("Professor", None, None)))),
        (2, alias("ned", titled("Mr.", Some(Sex::Male), name("Mr. Marsh", None, Some("Marsh"))))),
        (3, appeared("ada")),
    ])
}

/// Ada is last seen in chapter 3, Ned in 2 and the others in 1. Ada and Ned share a surname, 단's given name and
/// 한's surname are one syllable, and 엄마 is a description.
fn korean() -> Cast {
    cast_of(vec![
        (1, introduced("ada", name("에이다 모건", Some("에이다"), Some("모건")))),
        (1, introduced("ned", name("네드 모건", Some("네드"), Some("모건")))),
        (1, introduced("dan", name("단", Some("단"), None))),
        (1, introduced("han", titled("선장", None, name("한 선장", None, Some("한"))))),
        (1, introduced("mom", name("엄마", None, None))),
        (2, alias("ned", titled("씨", Some(Sex::Male), name("모건 씨", None, Some("모건"))))),
        (3, appeared("ada")),
    ])
}

#[test]
fn a_whole_name_and_a_given_name() {
    let e = english();
    assert_eq!(found(&e, "Ada Marsh climbed the stair."), [("ada", Whole, "Ada Marsh", 9)]);
    assert_eq!(found(&e, "Ada climbed"), [("ada", Given, "Ada Marsh", 3)]);
}

#[test]
fn a_shared_surname_finds_everyone_most_recent_first() {
    assert_eq!(
        found(&english(), "Marsh nodded."),
        [("ada", Surname, "Ada Marsh", 5), ("ned", Surname, "Ned Marsh", 5)]
    );
}

#[test]
fn a_possessive() {
    let e = english();
    assert_eq!(found(&e, "Ada's lamp"), [("ada", Given, "Ada Marsh", 3)]);
    assert_eq!(found(&e, "Ada Marsh's lamp"), [("ada", Whole, "Ada Marsh", 9)]);
}

#[test]
fn a_title_tapped_with_the_name_matches_as_a_whole() {
    assert_eq!(found(&english(), "Mr. Marsh rowed out."), [("ned", Whole, "Mr. Marsh", 9)]);
}

#[test]
fn a_name_does_not_match_the_start_of_a_longer_word() {
    let e = english();
    assert_eq!(found(&e, "Ronald came"), []);
    assert_eq!(found(&e, "Ron came"), [("ron", Whole, "Ron", 3)]);
}

#[test]
fn the_longest_match_wins() {
    let e = english();
    assert_eq!(found(&e, "Ned Marsh and Ada"), [("ned", Whole, "Ned Marsh", 9)]);
    assert_eq!(found(&e, "Ned came"), [("ned", Given, "Ned Marsh", 3), ("neddy", Given, "Ned Hale", 3)]);
}

#[test]
fn of_as_many_words_the_match_that_covers_more_wins() {
    // Written without spaces, a full name and its surname alone are both one word.
    let cast = cast_of(vec![
        (1, introduced("taro", name("山田太郎", Some("太郎"), Some("山田")))),
        (1, introduced("hanako", name("山田花子", Some("花子"), Some("山田")))),
    ]);
    assert_eq!(found(&cast, "山田太郎は笑った"), [("taro", Whole, "山田太郎", 12)]);
    assert_eq!(found(&cast, "山田は笑った"), [("taro", Surname, "山田太郎", 6), ("hanako", Surname, "山田花子", 6)]);
}

#[test]
fn a_one_word_match_keeps_the_names_case() {
    let e = english();
    assert_eq!(found(&e, "marsh birds"), []);
    assert_eq!(found(&e, "bill of sale"), []);
    assert_eq!(found(&e, "reed beds"), []);
    assert_eq!(found(&e, "Reed"), [("bill", Surname, "Bill Reed", 4)]);
}

#[test]
fn a_surname_is_matched_as_the_name_writes_it() {
    // The label is in another case than the name.
    let cast = cast_of(vec![(1, introduced("mcg", name("Ada McGregor", Some("Ada"), Some("Mcgregor"))))]);
    assert_eq!(found(&cast, "McGregor nodded"), [("mcg", Surname, "Ada McGregor", 8)]);
    assert_eq!(found(&cast, "McGregors"), [("mcg", Surname, "Ada McGregor", 9)]);
    assert_eq!(found(&cast, "Mcgregor"), []);
}

#[test]
fn a_label_that_is_no_run_of_the_names_words_is_not_tried() {
    // "Ada" lies inside the name, but only as part of the word "Adam".
    let cast = cast_of(vec![(1, introduced("adam", name("Adam Reed", Some("Ada"), Some("Reed"))))]);
    assert_eq!(found(&cast, "Ada came"), []);
    assert_eq!(found(&cast, "Reed"), [("adam", Surname, "Adam Reed", 4)]);
    assert_eq!(found(&cast, "Adam Reed"), [("adam", Whole, "Adam Reed", 9)]);
}

#[test]
fn a_label_inside_a_word_of_kana_or_han_is_tried_as_written() {
    // Japanese writes a name's parts without a space, so 太郎 is no word of 山田太郎.
    let cast = cast_of(vec![(1, introduced("taro", name("山田太郎", Some("太郎"), Some("山田"))))]);
    assert_eq!(found(&cast, "太郎は"), [("taro", Given, "山田太郎", 6)]);
}

#[test]
fn a_word_in_capitals_is_any_case() {
    let e = english();
    assert_eq!(found(&e, "MARSH!"), [("ada", Surname, "Ada Marsh", 5), ("ned", Surname, "Ned Marsh", 5)]);
    assert_eq!(found(&e, "BILL"), [("bill", Given, "Bill Reed", 4)]);
    assert_eq!(found(&e, "ADA MARSH"), [("ada", Whole, "Ada Marsh", 9)]);
}

#[test]
fn a_description_matches_as_a_whole() {
    let e = english();
    assert_eq!(found(&e, "the keeper climbed"), [("keeper", Whole, "the keeper", 10)]);
    assert_eq!(found(&e, "The Keeper"), [("keeper", Whole, "the keeper", 10)]);
}

#[test]
fn a_one_word_description_does_not_match() {
    let e = english();
    assert_eq!(found(&e, "Father said"), []);
    assert_eq!(found(&e, "Professor said"), []);
    assert_eq!(found(&korean(), "엄마가 웃었다"), []);
}

#[test]
fn a_text_that_names_nobody() {
    assert_eq!(found(&english(), "The tide came in."), []);
}

#[test]
fn a_korean_name_takes_a_particle() {
    let k = korean();
    assert_eq!(found(&k, "에이다는 웃었다"), [("ada", Given, "에이다 모건", 9)]);
    assert_eq!(found(&k, "에이다 모건이 웃었다"), [("ada", Whole, "에이다 모건", 16)]);
}

#[test]
fn a_korean_title_after_the_name() {
    assert_eq!(found(&korean(), "모건 씨는"), [("ned", Whole, "모건 씨", 10)]);
}

#[test]
fn a_korean_surname_alone() {
    assert_eq!(found(&korean(), "모건이"), [("ada", Surname, "에이다 모건", 6), ("ned", Surname, "네드 모건", 6)]);
}

#[test]
fn a_one_syllable_surname_does_not_match_alone() {
    let k = korean();
    assert_eq!(found(&k, "한은"), []);
    assert_eq!(found(&k, "한숨을 쉬었다"), []);
    assert_eq!(found(&k, "한 선장은"), [("han", Whole, "한 선장", 10)]);
}

#[test]
fn a_one_word_name_that_is_a_one_syllable_surname_does_not_match_alone() {
    let cast = cast_of(vec![
        (1, introduced("han", titled("선장", None, name("한 선장", None, Some("한"))))),
        (1, alias("han", name("한", None, Some("한")))),
    ]);
    assert_eq!(found(&cast, "한은"), []);
    assert_eq!(found(&cast, "한 사람"), []);
    assert_eq!(found(&cast, "한 선장은"), [("han", Whole, "한 선장", 10)]);
}

#[test]
fn a_one_syllable_given_name_takes_a_particle() {
    assert_eq!(found(&korean(), "단은"), [("dan", Whole, "단", 3)]);
}

#[test]
fn a_one_syllable_given_name_runs_into_nothing_but_particles() {
    let k = korean();
    assert_eq!(found(&k, "단지 웃었다"), []);
    assert_eq!(found(&k, "단단히"), []);
}

#[test]
fn a_particle_agrees_with_the_syllable_before_it() {
    let k = korean();
    assert_eq!(found(&k, "단는"), []);
    assert_eq!(found(&k, "에이다은"), []);
}

#[test]
fn two_particles_at_most() {
    let k = korean();
    assert_eq!(found(&k, "에이다에게는"), [("ada", Given, "에이다 모건", 9)]);
    assert_eq!(found(&k, "에이다에게도만"), []);
}

#[test]
fn a_vocative_and_a_contraction_are_particles() {
    let k = korean();
    // 아 calls a name that ends in a final consonant, as 야 calls one that does not.
    assert_eq!(found(&k, "단아, 이리 와"), [("dan", Whole, "단", 3)]);
    assert_eq!(found(&k, "에이다아"), []);
    assert_eq!(found(&k, "에이다에겐"), [("ada", Given, "에이다 모건", 9)]);
    assert_eq!(found(&k, "에이다한텐"), [("ada", Given, "에이다 모건", 9)]);
}

#[test]
fn a_family_by_its_plural_surname() {
    let (e, k) = (english(), korean());
    assert_eq!(found(&e, "Marshes came"), [("ada", Surname, "Ada Marsh", 7), ("ned", Surname, "Ned Marsh", 7)]);
    assert_eq!(found(&e, "Reeds' boat"), [("bill", Surname, "Bill Reed", 5)]);
    assert_eq!(found(&e, "MARSHES"), [("ada", Surname, "Ada Marsh", 7), ("ned", Surname, "Ned Marsh", 7)]);
    assert_eq!(found(&e, "marshes"), []);
    assert_eq!(found(&k, "모건네"), [("ada", Surname, "에이다 모건", 6), ("ned", Surname, "네드 모건", 6)]);
}

#[test]
fn a_family_takes_es_only_after_s_x_z_ch_or_sh() {
    let cast = cast_of(vec![(1, introduced("holm", name("Ann Holm", Some("Ann"), Some("Holm"))))]);
    assert_eq!(found(&cast, "Holmes"), []);
    assert_eq!(found(&cast, "Holms"), [("holm", Surname, "Ann Holm", 5)]);
}

#[test]
fn a_surname_as_written_beats_a_familys_plural_of_as_many_bytes() {
    let cast = cast_of(vec![
        (1, introduced("tom", name("Tom Field", Some("Tom"), Some("Field")))),
        (1, introduced("ann", name("Ann Fields", Some("Ann"), Some("Fields")))),
    ]);
    assert_eq!(found(&cast, "Fields said"), [("ann", Surname, "Ann Fields", 6)]);
    assert_eq!(found(&cast, "Field said"), [("tom", Surname, "Tom Field", 5)]);
}

#[test]
fn a_given_name_names_no_family() {
    assert_eq!(found(&english(), "Adas"), []);
}

#[test]
fn kana_or_han_runs_on_as_extraction_has_it() {
    let nana = cast_of(vec![(1, introduced("nana", name("奈々", Some("奈々"), None)))]);
    assert_eq!(found(&nana, "奈々は灯台へ行った。"), [("nana", Whole, "奈々", 6)]);
}

#[test]
fn a_name_given_after_the_reader_does_not_match() {
    let mut conn = db();
    let log = keeper();
    write_book(&mut conn, &log).unwrap();
    let at = |document| cast_at(&conn, &log.book.sha256, document, &Scope::Book).unwrap();

    let in_two = at("c2.xhtml");
    assert_eq!(found(&in_two, "Ada Marsh"), []);
    assert_eq!(found(&in_two, "Ada"), []);
    assert_eq!(found(&in_two, "the keeper"), [("ada", Whole, "the keeper", 10)]);
    let in_three = at("c3.xhtml");
    assert_eq!(found(&in_three, "Ada"), [("ada", Given, "Ada Marsh", 3)]);
    assert_eq!(found(&in_three, "Tom Reed"), []);
    assert_eq!(found(&at("back.xhtml"), "Tom Reed"), [("tom", Whole, "Tom Reed", 8)]);
}

#[test]
fn most_recently_seen_first_by_the_order_of_the_books() {
    let (first, second) = (keeper().book.sha256, keepers_return().book.sha256);
    let ada = |book: &str| EntityKey { book: book.into(), id: "ada".into() };
    let named = |logs: &[BookLog]| -> Vec<EntityKey> {
        let cast = fold_series(logs, None, &[]);
        characters_named(&cast, "Ada").into_iter().map(|mention| mention.character.key.clone()).collect()
    };
    // The first book's Ada is last seen in its chapter 3, the second's in its chapter 2: neither the hash order
    // nor the chapter number alone gives both orders.
    assert_eq!(named(&[keeper(), keepers_return()]), [ada(&second), ada(&first)]);
    assert_eq!(named(&[keepers_return(), keeper()]), [ada(&first), ada(&second)]);
}

#[test]
fn two_characters_of_one_full_name_both_come_back() {
    let cast = cast_of(vec![
        (1, introduced("elder", name("Tom Reed", Some("Tom"), Some("Reed")))),
        (2, introduced("younger", name("Tom Reed", Some("Tom"), Some("Reed")))),
    ]);
    assert_eq!(found(&cast, "Tom Reed"), [("younger", Whole, "Tom Reed", 8), ("elder", Whole, "Tom Reed", 8)]);
}

#[test]
fn characters_last_seen_in_one_chapter_keep_the_casts_order() {
    // Tom is introduced before Bill, against the order of their ids.
    let cast = cast_of(vec![
        (1, introduced("tom", name("Tom Reed", Some("Tom"), Some("Reed")))),
        (1, introduced("bill", name("Bill Reed", Some("Bill"), Some("Reed")))),
    ]);
    assert_eq!(found(&cast, "Reed"), [("tom", Surname, "Tom Reed", 4), ("bill", Surname, "Bill Reed", 4)]);
}

#[test]
fn a_merged_characters_name_finds_the_survivor() {
    assert_eq!(found(&fold_book(&keeper(), 3), "the stranger"), [("tom", Whole, "the stranger", 12)]);

    let series = fold_series(&keepers_logs(), None, &keepers_links());
    let tom = EntityKey { book: keeper().book.sha256, id: "tom".into() };
    let named: Vec<_> = characters_named(&series, "the old sailor")
        .into_iter()
        .map(|mention| (&mention.character.key, mention.part, mention.name.text.as_str(), mention.len))
        .collect();
    assert_eq!(named, [(&tom, Whole, "the old sailor", 14)]);
}

#[test]
fn a_character_comes_back_once() {
    let cast = cast_of(vec![
        (1, introduced("ada", name("Ada Marsh", Some("Ada"), Some("Marsh")))),
        (2, alias("ada", name("Ada", Some("Ada"), None))),
    ]);
    assert_eq!(found(&cast, "Ada"), [("ada", Whole, "Ada", 3)]);
}

#[test]
fn punctuation_before_the_name_is_passed_over() {
    let e = english();
    assert_eq!(found(&e, "\u{201c}Ada,\u{201d} she said"), [("ada", Given, "Ada Marsh", 6)]);
    assert_eq!(found(&e, "  Ada"), [("ada", Given, "Ada Marsh", 5)]);
}

#[test]
fn no_word_names_nobody() {
    let cast = cast_of(vec![
        (1, introduced("dash", name("\u{2014}", Some("\u{2014}"), None))),
        (1, introduced("ada", name("Ada Marsh", Some("Ada"), Some("Marsh")))),
    ]);
    assert_eq!(found(&cast, ""), []);
    assert_eq!(found(&cast, "\u{2014} !"), []);
    assert_eq!(found(&cast, "\u{2014} Ada"), [("ada", Given, "Ada Marsh", 7)]);
}

#[test]
fn a_name_with_an_apostrophe_is_two_words() {
    let cast = cast_of(vec![(1, introduced("bran", name("Bran O'Dell", Some("Bran"), Some("O'Dell"))))]);
    assert_eq!(found(&cast, "O'Dell's boat"), [("bran", Surname, "Bran O'Dell", 6)]);
    assert_eq!(found(&cast, "o'dell"), [("bran", Surname, "Bran O'Dell", 6)]);
    assert_eq!(found(&cast, "O\u{2019}Dell"), [("bran", Surname, "Bran O'Dell", 8)]);
    assert_eq!(found(&cast, "Bran O'Dell"), [("bran", Whole, "Bran O'Dell", 11)]);
}

#[test]
fn a_family_by_the_plural_of_a_surname_of_two_words() {
    let cast = cast_of(vec![(1, introduced("bran", name("Bran O'Dell", Some("Bran"), Some("O'Dell"))))]);
    assert_eq!(found(&cast, "O'Dells' boat"), [("bran", Surname, "Bran O'Dell", 7)]);
    assert_eq!(found(&cast, "o'dells"), [("bran", Surname, "Bran O'Dell", 7)]);
    assert_eq!(found(&cast, "O'Dellses"), []);
}
