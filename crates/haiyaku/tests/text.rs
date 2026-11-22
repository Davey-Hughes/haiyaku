//! Where a chapter says a quote or a name, each test on a chapter of invented sentences.

#![cfg(feature = "extract")]

mod common;

use common::epub::chapter_text;
use haiyaku::extract::{MIN_QUOTE_WEIGHT, QuoteError, locate_name, locate_quote, weight};
use haiyaku::log::Position;

fn at(document: u32, block: u32) -> Position {
    Position { document, block }
}

#[test]
fn quote_ignores_case_spacing_and_punctuation() {
    let chapter = chapter_text(1, 0, &["\u{201c}The lamp at Gull Point burned all night,\u{201d} Ada said."]);
    assert_eq!(locate_quote(&chapter, "the LAMP, at gull-point,   burned all NIGHT"), Ok(at(0, 0)));
    assert_eq!(locate_quote(&chapter, "Gull Point burned all night, Ada said"), Ok(at(0, 0)));
}

#[test]
fn quote_with_ellipsis_matches_parts_in_order() {
    let chapter = chapter_text(1, 0, &["Ada climbed the stairs, counted every step, and lit the lamp at the top."]);
    assert_eq!(locate_quote(&chapter, "Ada climbed the stairs... and lit the lamp"), Ok(at(0, 0)));
    assert_eq!(locate_quote(&chapter, "Ada climbed the stairs \u{2026} lit the lamp at the top"), Ok(at(0, 0)));
    // Empty parts, from an ellipsis at an end or two in a row, are dropped rather than matched.
    assert_eq!(locate_quote(&chapter, "...counted every step......lit the lamp\u{2026}"), Ok(at(0, 0)));
}

#[test]
fn parts_out_of_order_are_not_found() {
    let chapter = chapter_text(1, 0, &["Ada climbed the stairs, counted every step, and lit the lamp at the top."]);
    assert_eq!(locate_quote(&chapter, "and lit the lamp... Ada climbed the stairs"), Err(QuoteError::NotFound));
    assert_eq!(
        locate_quote(&chapter, "lit the lamp at the top \u{2026} counted every step"),
        Err(QuoteError::NotFound)
    );
}

#[test]
fn quote_absent_is_not_found() {
    let chapter = chapter_text(1, 0, &["Ada climbed the stairs, counted every step, and lit the lamp at the top."]);
    assert_eq!(locate_quote(&chapter, "Ada sailed for the mainland"), Err(QuoteError::NotFound));
}

#[test]
fn quote_position_is_the_block_of_its_start() {
    let chapter = chapter_text(
        4,
        7,
        &[
            "Chapter Four",
            "The gulls woke Ada before dawn.",
            "She went down to the harbour.",
            "* * *",
            "The boat was gone.",
        ],
    );
    assert_eq!(locate_quote(&chapter, "She went down to the harbour"), Ok(at(7, 2)));
    assert_eq!(locate_quote(&chapter, "Ada before dawn. She went down"), Ok(at(7, 1)));
    assert_eq!(locate_quote(&chapter, "woke Ada before dawn ... the boat was gone"), Ok(at(7, 1)));
    // A block with no letters holds no quote: one that starts just after it is in the block after it.
    assert_eq!(locate_quote(&chapter, "The boat was gone"), Ok(at(7, 4)));
    // A part with no letters, here an opening quotation mark, is dropped rather than matched at the chapter's start.
    assert_eq!(locate_quote(&chapter, "\u{201c}... She went down to the harbour"), Ok(at(7, 2)));
}

#[test]
fn short_latin_quote_is_too_short() {
    let chapter = chapter_text(1, 0, &["Run now, Ada said, and the lamp went dark."]);
    assert_eq!(locate_quote(&chapter, "Run now"), Err(QuoteError::TooShort));
    assert_eq!(locate_quote(&chapter, "the lamp"), Err(QuoteError::TooShort));
    assert_eq!(weight("lamp went"), MIN_QUOTE_WEIGHT);
    assert_eq!(locate_quote(&chapter, "lamp went"), Ok(at(0, 0)));
    // The weight is counted before the search: an absent quote that is too short says so.
    assert_eq!(locate_quote(&chapter, "Run far"), Err(QuoteError::TooShort));
}

#[test]
fn three_hangul_syllables_are_enough() {
    let chapter = chapter_text(1, 0, &["바다 쪽에서 누군가 불렀다. \u{201c}지수야!\u{201d}"]);
    assert_eq!(locate_quote(&chapter, "지수야!"), Ok(at(0, 0)));
    assert_eq!(locate_quote(&chapter, "지수"), Err(QuoteError::TooShort));
}

#[test]
fn weights() {
    assert_eq!(weight("Ada 12"), 5);
    assert_eq!(weight("지수"), 6);
    assert_eq!(weight("漢字かな"), 12);
    assert_eq!(weight("\u{2014}!"), 0);
    assert_eq!(weight("奈々"), 6);
}

#[test]
fn halfwidth_kana_weigh_as_their_fullwidth_forms() {
    // A halfwidth voiced or semi-voiced syllable is a kana and a sound mark, which marks the kana before it.
    assert_eq!(weight("ﾘｭｳｼﾞ"), weight("リュウジ"));
    assert_eq!(weight("ｶﾞﾗｽﾉﾄﾞｱｦｱｹﾀ ﾊﾟﾝﾔ"), weight("ガラスノドアヲアケタ パンヤ"));
}

#[test]
fn name_needs_whole_words() {
    assert_eq!(locate_name(&chapter_text(1, 0, &["He would walk to Alba."]), "Al"), None);
    assert_eq!(locate_name(&chapter_text(1, 0, &["Ada's lamp burned."]), "Ada"), Some(at(0, 0)));
}

#[test]
fn empty_name_is_absent() {
    let chapter = chapter_text(1, 0, &["Ada said \u{2014} nothing."]);
    assert_eq!(locate_name(&chapter, ""), None);
    assert_eq!(locate_name(&chapter, "\u{2014}"), None);
}

#[test]
fn multiword_name_matches_consecutive_words() {
    assert_eq!(
        locate_name(&chapter_text(1, 0, &["\u{201c}Here,\u{201d} said Ada Marsh."]), "Ada Marsh"),
        Some(at(0, 0))
    );
    assert_eq!(
        locate_name(&chapter_text(1, 0, &["\u{201c}Here,\u{201d} said Ada Marsh."]), "ada MARSH"),
        Some(at(0, 0))
    );
    assert_eq!(locate_name(&chapter_text(1, 0, &["Ada went to the marsh."]), "Ada Marsh"), None);
    // Names do not cross blocks.
    assert_eq!(
        locate_name(&chapter_text(1, 0, &["The lamp was lit by Ada", "Marsh, who said nothing."]), "Ada Marsh"),
        None
    );
}

#[test]
fn korean_name_matches_before_a_particle() {
    let chapter = chapter_text(1, 0, &["한지수는 등대로 갔다"]);
    assert_eq!(locate_name(&chapter, "한지수"), Some(at(0, 0)));
    assert_eq!(locate_name(&chapter, "지수"), None);
    assert_eq!(locate_name(&chapter_text(1, 0, &["한 씨는 웃었다"]), "한 씨"), Some(at(0, 0)));
}

#[test]
fn han_and_kana_names_match_before_a_particle() {
    // A name that ends in the iteration mark, or in a halfwidth voiced sound mark, runs on into its particle as
    // one that ends in an ideograph or a kana does.
    assert_eq!(locate_name(&chapter_text(1, 0, &["奈々は灯台へ行った。"]), "奈々"), Some(at(0, 0)));
    assert_eq!(locate_name(&chapter_text(1, 0, &["ﾘｭｳｼﾞは灯台へ行った。"]), "ﾘｭｳｼﾞ"), Some(at(0, 0)));
    assert_eq!(locate_name(&chapter_text(1, 0, &["ﾀﾝﾎﾟﾎﾟが鳴いた。"]), "ﾀﾝﾎﾟﾎﾟ"), Some(at(0, 0)));
}

#[test]
fn a_final_sigma_matches_a_capital_sigma() {
    let capitals = chapter_text(1, 0, &["Ο ΝΙΚΟΣ ΑΝΕΒΗΚΕ ΣΤΟ ΦΑΡΟ."]);
    assert_eq!(locate_quote(&capitals, "ο Νικος ανεβηκε στο φαρο"), Ok(at(0, 0)));
    let lower = chapter_text(1, 0, &["Ο Νικος ανεβηκε στο φαρο."]);
    assert_eq!(locate_quote(&lower, "Ο ΝΙΚΟΣ ΑΝΕΒΗΚΕ"), Ok(at(0, 0)));
}

#[test]
fn name_position_is_its_first_block() {
    let chapter = chapter_text(3, 5, &["The tide came in.", "Ada Marsh lit the lamp.", "Then Ada Marsh slept."]);
    assert_eq!(locate_name(&chapter, "Ada Marsh"), Some(at(5, 1)));
}
