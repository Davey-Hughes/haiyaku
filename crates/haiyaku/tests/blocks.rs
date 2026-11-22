//! XHTML documents as heading and paragraph blocks, each test on a literal document of invented text.

#![cfg(feature = "extract")]

use haiyaku::book::{Block, BlockKind, blocks};

fn heading(text: &str) -> Block {
    Block { kind: BlockKind::Heading, text: text.into() }
}

fn paragraph(text: &str) -> Block {
    Block { kind: BlockKind::Paragraph, text: text.into() }
}

/// The blocks of a document whose body holds `inner`.
fn in_body(inner: &str) -> Vec<Block> {
    blocks(&format!("<html><head><title>Invented</title></head><body>{inner}</body></html>"))
}

#[test]
fn paragraphs_and_headings() {
    assert_eq!(
        blocks("<body><h1>One</h1><p>First.</p><p>Second.</p></body>"),
        [heading("One"), paragraph("First."), paragraph("Second.")]
    );
}

#[test]
fn head_script_style_and_ruby_readings_are_skipped() {
    let document = "<head><title>T</title></head>\
        <body><p>漢<rt>かん</rt>字<rp>(</rp></p><script>x</script><style>y</style></body>";
    assert_eq!(blocks(document), [paragraph("漢字")]);
}

#[test]
fn soft_hyphens_dropped_and_space_collapsed() {
    assert_eq!(blocks("<body><p>ex&shy;am&#173;ple  \n text&nbsp;here</p></body>"), [paragraph("example text here")]);
    // Written as the characters themselves, and at the edges of a block, where a space is trimmed.
    assert_eq!(
        in_body("<p>\u{a0} lamp\u{ad}light \u{ad} on\u{a0}\u{a0}the\tsea \u{a0}</p>"),
        [paragraph("lamplight on the sea")]
    );
}

#[test]
fn named_entities_decoded() {
    assert_eq!(
        in_body("<p>caf&eacute; &ldquo;na&iuml;ve&rdquo; &amp; &#233; &#xE9;</p>"),
        [paragraph("café “naïve” & é é")]
    );
}

#[test]
fn unknown_entity_and_bare_ampersand_stay() {
    assert_eq!(in_body("<p>&bogus; R&D</p>"), [paragraph("&bogus; R&D")]);
}

#[test]
fn a_bare_less_than_stays_text() {
    assert_eq!(in_body("<p>1 < 2 &lt; 3 <</p>"), [paragraph("1 < 2 < 3 <")]);
    // An escaped tag is text too: references are decoded after the markup is found, not before.
    assert_eq!(in_body("<p>write &lt;p&gt; for a paragraph</p>"), [paragraph("write <p> for a paragraph")]);
}

#[test]
fn malformed_markup_keeps_text() {
    assert_eq!(blocks("<body><p>One<p>Two</div><b>Three"), [paragraph("One"), paragraph("Two"), paragraph("Three")]);
}

#[test]
fn an_unclosed_head_or_ruby_reading_keeps_the_text_after_it() {
    assert_eq!(blocks("<head><title>T<body><p>kept</p></body>"), [paragraph("kept")]);
    assert_eq!(in_body("<p><ruby>漢<rt>かん</ruby>字</p><p>next</p>"), [paragraph("漢字"), paragraph("next")]);
    assert_eq!(in_body("<p>漢<rp>(<rt>かん</p><p>next</p>"), [paragraph("漢"), paragraph("next")]);
}

#[test]
fn a_self_closing_tag_opens_nothing() {
    assert_eq!(in_body(r#"<script src="a.js"/><p>kept</p>"#), [paragraph("kept")]);
    assert_eq!(in_body("<h2/><p>no heading</p>"), [paragraph("no heading")]);
}

#[test]
fn prefixed_and_uppercase_tags() {
    assert_eq!(blocks("<xhtml:BODY><xhtml:P>Hi</xhtml:P></xhtml:BODY>"), [paragraph("Hi")]);
}

#[test]
fn br_splits_a_paragraph() {
    assert_eq!(blocks("<body><p>a<br/>b</p></body>"), [paragraph("a"), paragraph("b")]);
}

#[test]
fn every_block_tag_ends_a_block_where_it_opens_and_where_it_closes() {
    let tags = [
        "p",
        "div",
        "br",
        "hr",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "li",
        "tr",
        "blockquote",
        "section",
        "article",
        "dd",
        "dt",
        "table",
        "td",
        "th",
        "caption",
        "figcaption",
        "aside",
        "nav",
        "header",
        "footer",
        "pre",
        "ul",
        "ol",
        "dl",
        "figure",
        "main",
    ];
    for tag in tags {
        let inside = if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") { heading("b") } else { paragraph("b") };
        assert_eq!(in_body(&format!("a<{tag}>b</{tag}>c")), [paragraph("a"), inside, paragraph("c")], "{tag}");
    }
}

#[test]
fn neighbouring_cells_are_blocks_of_their_own() {
    assert_eq!(
        in_body("<table><tr><td>Ada</td><td>Marsh</td></tr><tr><th>Tom</th><th>Reed</th></tr></table>"),
        [paragraph("Ada"), paragraph("Marsh"), paragraph("Tom"), paragraph("Reed")]
    );
    assert_eq!(in_body("<ul><li>lamp</li></ul><ol><li>stair</li></ol>"), [paragraph("lamp"), paragraph("stair")]);
}

#[test]
fn an_unclosed_heading_ends_where_a_block_opens_after_its_text() {
    assert_eq!(in_body("<h2>One<p>Two</p>"), [heading("One"), paragraph("Two")]);
    assert_eq!(
        in_body("<h1>Chapter One<div>The keeper climbed.</div><div>Pip shouted.</div>"),
        [heading("Chapter One"), paragraph("The keeper climbed."), paragraph("Pip shouted.")]
    );
    assert_eq!(
        in_body("<h1><p>Chapter One</p><p>The keeper climbed.</p>"),
        [heading("Chapter One"), paragraph("The keeper climbed.")]
    );
    assert_eq!(
        in_body("<h1>Title<h2>Part<p>Text.</p><div>More.</div>"),
        [heading("Title"), heading("Part"), paragraph("Text."), paragraph("More.")]
    );
    // A line break holds nothing, however it is written: the heading goes on after it.
    assert_eq!(
        in_body("<h1>Chapter 1<br/>The Lamp</h1><p>Text.</p>"),
        [heading("Chapter 1"), heading("The Lamp"), paragraph("Text.")]
    );
    assert_eq!(in_body("<h1>Chapter 1<br></br>The Lamp</h1>"), [heading("Chapter 1"), heading("The Lamp")]);
}

#[test]
fn a_block_that_opens_before_a_headings_text_is_part_of_it() {
    assert_eq!(
        in_body(r#"<h1 class="ch"><div>Chapter One</div></h1><p>Text.</p>"#),
        [heading("Chapter One"), paragraph("Text.")]
    );
    assert_eq!(in_body("<h1><p>Chapter One</p></h1><p>Text.</p>"), [heading("Chapter One"), paragraph("Text.")]);
    assert_eq!(in_body("<h1>\n <div>\n Chapter One</div></h1>"), [heading("Chapter One")]);
    // Once the heading has text, a block that opens is one of its own, even before the heading closes.
    assert_eq!(in_body("<h2><header>Part</header><section>Two</section></h2>"), [heading("Part"), paragraph("Two")]);
}

#[test]
fn a_less_than_inside_a_tag_ends_the_tag_at_its_first_close() {
    assert_eq!(in_body(r#"<p class="x>Text</p><p>Next</p>"#), [paragraph("Text"), paragraph("Next")]);
    assert_eq!(in_body("<p title='x>Text"), [paragraph("Text")]);
    // A later quote closes the one left open, and the `<` that follows is still inside the tag.
    assert_eq!(
        in_body(r#"<p class="x>He said "hi.</p><p>Next</p>"#),
        [paragraph(r#"He said "hi."#), paragraph("Next")]
    );
}

#[test]
fn an_unclosed_script_hides_the_rest_of_the_body_and_an_unclosed_comment_the_rest_of_the_document() {
    assert_eq!(in_body("<p>kept</p><script>x<p>hidden</p>"), [paragraph("kept")]);
    assert_eq!(in_body("<p>kept</p><style>x<p>hidden</p>"), [paragraph("kept")]);
    assert_eq!(in_body("<p>kept</p><!-- x<p>hidden</p>"), [paragraph("kept")]);
}

#[test]
fn comments_are_skipped() {
    assert_eq!(blocks("<body><!-- <p>no</p> --><p>yes</p></body>"), [paragraph("yes")]);
}

#[test]
fn text_outside_body_is_dropped() {
    assert_eq!(
        blocks("<html>before<p>early</p><body><p>inside</p></body><p>late</p>after</html>"),
        [paragraph("inside")]
    );
    assert_eq!(blocks("<p>a document with no body</p>"), []);
}

#[test]
fn nested_inline_markup_stays_one_block() {
    assert_eq!(in_body("<p>a <em>b <b>c</b></em> d</p>"), [paragraph("a b c d")]);
    assert_eq!(in_body(r#"<p>one<span class="x">two</span><a href="n.xhtml#f1">3</a></p>"#), [paragraph("onetwo3")]);
}

#[test]
fn heading_inside_div_is_a_heading() {
    assert_eq!(in_body("<div><h2>Title</h2><p>Text.</p></div>"), [heading("Title"), paragraph("Text.")]);
}

#[test]
fn blocks_left_empty_are_dropped() {
    assert_eq!(in_body("<p> </p><div>&nbsp;</div><p>&shy;</p><hr/><p>kept</p>"), [paragraph("kept")]);
    assert_eq!(blocks(""), []);
}
