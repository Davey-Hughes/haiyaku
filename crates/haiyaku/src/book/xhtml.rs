//! Markup as this crate reads it: a tag scanner that takes whatever it is given, and the walk over its tokens
//! that turns one XHTML document into blocks.
//!
//! The scanner is no XML parser, on purpose. It knows tags, comments and text and nothing of nesting, so there
//! is no document it refuses: an element left unclosed, a close tag with nothing open, a bare `&` or `<` in
//! the text cost the structure around them and not the text. A tag ends at its first `>` outside quotes,
//! unless a `<` comes first, quoted or not, or the input ends inside a quote: then it ends at its first `>`,
//! so a quote left open costs the tag's attributes and not the text after it. Three mistakes cost text as
//! well, since nothing in the markup says where they end: a `script` or `style` left unclosed in the body
//! hides the rest of the body, a comment left unclosed hides the rest of the document, and a tag with no `>`
//! at all takes the rest of the document as its inside. A parser that stops at the first error gives nothing
//! for a chapter whose markup has one.

use super::{Block, BlockKind};

/// One piece of markup, as [`tokens`] yields it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    /// An open tag, `<p>` or `<br/>`.
    Open(Tag),
    /// A close tag, by its name as [`Tag::name`] spells one.
    Close(String),
    /// A run of text with its character references decoded and its whitespace as written. The text of a
    /// CDATA section is a run of its own, with nothing decoded.
    Text(String),
}

/// An open tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tag {
    /// Lowercased and without its namespace prefix: `xhtml:P` is `p`.
    pub(super) name: String,
    /// Written `<br/>`: the element has no contents, and no close tag follows.
    pub(super) self_closing: bool,
    /// Names spelled as [`Tag::name`] is, values with their character references decoded.
    attributes: Vec<(String, String)>,
}

impl Tag {
    /// The value of the attribute `name`, which is empty for one written without a value. A name matches as a
    /// tag's does, so `type` finds `epub:type`; of two attributes with one name, the first is found.
    pub(super) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(found, _)| found == name).map(|(_, value)| value.as_str())
    }
}

/// The tokens of `markup`, in the order written. Comments, processing instructions and declarations such as
/// the doctype yield nothing.
pub(super) fn tokens(markup: &str) -> Tokens<'_> {
    Tokens { rest: markup }
}

/// The iterator [`tokens`] returns.
#[derive(Debug)]
pub(super) struct Tokens<'a> {
    rest: &'a str,
}

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let rest = self.rest;
            if rest.is_empty() {
                return None;
            }
            if !starts_markup(rest) {
                let end = rest.match_indices('<').map(|(at, _)| at).find(|at| starts_markup(&rest[*at..]));
                let (text, after) = rest.split_at(end.unwrap_or(rest.len()));
                self.rest = after;
                return Some(Token::Text(decode(text)));
            }
            // A comment, a section or a tag with no end takes the rest of the input: nothing says where it
            // would have ended.
            if let Some(comment) = rest.strip_prefix("<!--") {
                self.rest = comment.split_once("-->").map_or("", |(_, after)| after);
            } else if let Some(cdata) = rest.strip_prefix("<![CDATA[") {
                let (text, after) = cdata.split_once("]]>").unwrap_or((cdata, ""));
                self.rest = after;
                return Some(Token::Text(text.to_owned()));
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                self.rest = rest.split_once('>').map_or("", |(_, after)| after);
            } else if let Some(close) = rest.strip_prefix("</") {
                let (inner, after) = close.split_once('>').unwrap_or((close, ""));
                self.rest = after;
                return Some(Token::Close(local(inner.split_whitespace().next().unwrap_or(""))));
            } else {
                let (inner, after) = rest[1..].split_at(tag_end(&rest[1..]));
                self.rest = after.strip_prefix('>').unwrap_or(after);
                return Some(Token::Open(tag(inner)));
            }
        }
    }
}

/// Whether `rest` begins a tag, a comment, an instruction or a declaration. A `<` followed by anything else,
/// as in "1 < 2", is text: taken for the start of a tag, it would swallow the text up to the next `>`.
fn starts_markup(rest: &str) -> bool {
    let mut chars = rest.chars();
    chars.next() == Some('<') && chars.next().is_some_and(|next| next.is_ascii_alphabetic() || "/!?".contains(next))
}

/// Where the tag that `inner` is the inside of ends: at the first `>` outside a quoted attribute value, or at
/// the end of the input. A `<`, quoted or not, or the end of the input inside a quote ends it at its first `>`
/// instead: XML allows no `<` inside a tag, so a quote that reaches one was left open, and a quote that a later
/// one seems to close has run on into the text all the same.
fn tag_end(inner: &str) -> usize {
    let first_close = || inner.find('>').unwrap_or(inner.len());
    let mut quote = None;
    for (at, c) in inner.char_indices() {
        match quote {
            _ if c == '<' => return first_close(),
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return at,
            None => {}
        }
    }
    if quote.is_some() { first_close() } else { inner.len() }
}

/// The open tag whose inside, between `<` and `>`, is `inner`.
fn tag(inner: &str) -> Tag {
    let inner = inner.trim_end();
    let (inner, self_closing) = inner.strip_suffix('/').map_or((inner, false), |inner| (inner, true));
    let (name, mut rest) = inner.split_at(inner.find(char::is_whitespace).unwrap_or(inner.len()));
    let mut attributes = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let (name, after) = rest.split_at(rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len()));
        rest = after.trim_start();
        let mut value = "";
        if let Some(written) = rest.strip_prefix('=') {
            let written = written.trim_start();
            (value, rest) = match written.chars().next().filter(|quote| matches!(quote, '"' | '\'')) {
                Some(quote) => written[1..].split_once(quote).unwrap_or((&written[1..], "")),
                None => written.split_at(written.find(char::is_whitespace).unwrap_or(written.len())),
            };
        }
        // A stray "=" has no name before it: its value is passed over.
        if !name.is_empty() {
            attributes.push((local(name), decode(value)));
        }
    }
    Tag { name: local(name), self_closing, attributes }
}

/// A tag's or an attribute's name without its namespace prefix, lowercased: what a document calls `xhtml:P`
/// and what it calls `p` are one element to every reader of the tokens.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

/// `text` with its character references decoded. A reference that names nothing and a bare `&` stay as
/// written.
fn decode(text: &str) -> String {
    html_escape::decode_html_entities(text).into_owned()
}

/// The tags a block ends at, both where one opens and where it closes: every element HTML lays out as a block,
/// so the text of two neighbours, two cells of a row, never runs together into one word.
const BLOCK_TAGS: [&str; 32] = [
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

/// The elements whose contents are no text of the book: what a document says about itself, code, style
/// rules, and the readings and fallback brackets of ruby, which say again the word they are set over.
const SKIPPED: [&str; 5] = ["head", "script", "style", "rt", "rp"];

/// The blocks of one XHTML document, in document order. Never fails: markup it cannot follow costs structure,
/// not text.
pub fn blocks(xhtml: &str) -> Vec<Block> {
    let mut walk = Walk::default();
    for token in tokens(xhtml) {
        walk.take(token);
    }
    walk.end_block();
    walk.blocks
}

/// The state of one walk over a document's tokens.
#[derive(Default)]
struct Walk {
    blocks: Vec<Block>,
    /// The text of the block being read, as the tokens gave it.
    text: String,
    in_body: bool,
    /// Whether the text being read is a heading's.
    in_heading: bool,
    /// Whether the open heading has given a block yet: a block that opens before then wraps the heading's text,
    /// and one that opens after it does not.
    heading_has_text: bool,
    /// The block elements open, by name, innermost last; headings, line breaks and rules are not among them.
    open_blocks: Vec<String>,
    /// How many of `open_blocks` were open when the latest heading opened: those are around the heading.
    around_heading: usize,
    /// The element whose contents are being passed over.
    skipping: Option<&'static str>,
}

impl Walk {
    fn take(&mut self, token: Token) {
        if let Some(skipped) = self.skipping {
            if !ends_skip(skipped, &token) {
                return;
            }
            self.skipping = None;
            if matches!(&token, Token::Close(name) if name == skipped) {
                return;
            }
        }
        match token {
            Token::Open(tag) => {
                self.tag(&tag.name, !tag.self_closing, false);
                if !tag.self_closing {
                    self.skipping = SKIPPED.into_iter().find(|skipped| *skipped == tag.name);
                }
            }
            Token::Close(name) => self.tag(&name, false, true),
            Token::Text(text) if self.in_body => self.text.push_str(&text),
            Token::Text(_) => {}
        }
    }

    /// A tag named `name`, which `enters` its element when it is an open tag with contents to follow, and
    /// `closes` it when it is a close tag.
    fn tag(&mut self, name: &str, enters: bool, closes: bool) {
        if BLOCK_TAGS.contains(&name) {
            // Before the heading state below changes: the block ending here was read under the old one.
            self.end_block();
        }
        match name {
            "body" => self.in_body = enters,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.in_heading = enters;
                self.heading_has_text = false;
                self.around_heading = self.open_blocks.len();
            }
            // A line break or a rule holds nothing, however it is written, so it opens no block of its own.
            "br" | "hr" => {}
            _ if !BLOCK_TAGS.contains(&name) => {}
            // A heading left unclosed ends where a block opens after its text; otherwise every paragraph after it
            // would be read as a heading. A block that opens before any text, a div or p around it, is part of it,
            // and its close leaves the heading open.
            _ if enters => {
                if self.in_heading && self.heading_has_text {
                    self.in_heading = false;
                }
                self.open_blocks.push(name.to_owned());
            }
            // A block's close also closes every block left unclosed inside it, and a close with no such block open
            // closes nothing. A heading left unclosed ends where a block that was open around it closes, whether
            // or not it has text, as an element ends with its parent; left open, it would make the text after that
            // close a heading.
            _ if closes => {
                if let Some(at) = self.open_blocks.iter().rposition(|open| open == name) {
                    if self.in_heading && at < self.around_heading {
                        self.in_heading = false;
                    }
                    self.open_blocks.truncate(at);
                }
            }
            _ => {}
        }
    }

    fn end_block(&mut self) {
        let text = tidy(&self.text);
        self.text.clear();
        if !text.is_empty() {
            let kind = if self.in_heading { BlockKind::Heading } else { BlockKind::Paragraph };
            self.heading_has_text |= self.in_heading;
            self.blocks.push(Block { kind, text });
        }
    }
}

/// Whether `token` ends the passing over of `skipped`'s contents. Its own close tag does, and so does the
/// place where an element left unclosed cannot go on: any element where the body opens or closes, a ruby
/// reading where its ruby or its block ends. Passed over to its own close tag alone, an unclosed one would
/// take the rest of the document with it.
fn ends_skip(skipped: &str, token: &Token) -> bool {
    let name = match token {
        Token::Open(tag) => tag.name.as_str(),
        Token::Close(name) => name.as_str(),
        Token::Text(_) => return false,
    };
    let own_close = matches!(token, Token::Close(_)) && name == skipped;
    let ruby_over = matches!(skipped, "rt" | "rp") && (name == "ruby" || BLOCK_TAGS.contains(&name));
    own_close || name == "body" || ruby_over
}

/// Text read out of markup as it is kept, a block's, a title's or a label's: soft hyphens dropped, and each
/// run of whitespace, the no-break space included, made one space, with none at either end. A soft hyphen
/// marks where a renderer may break a word and is no part of it: left in, the word would not match itself
/// written without one.
pub(super) fn tidy(text: &str) -> String {
    let mut tidied = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars().filter(|c| *c != '\u{ad}') {
        if c.is_whitespace() {
            gap = !tidied.is_empty();
        } else {
            if gap {
                tidied.push(' ');
                gap = false;
            }
            tidied.push(c);
        }
    }
    tidied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(markup: &str) -> Tag {
        match tokens(markup).next() {
            Some(Token::Open(tag)) => tag,
            other => panic!("{markup:?} starts with {other:?}, not an open tag"),
        }
    }

    fn text(text: &str) -> Token {
        Token::Text(text.into())
    }

    fn close(name: &str) -> Token {
        Token::Close(name.into())
    }

    #[test]
    fn an_open_tag_carries_its_attributes() {
        let tag = open(r#"<rootfile full-path="OEBPS/content.opf" media-type='application/oebps-package+xml'/>"#);
        assert_eq!(tag.name, "rootfile");
        assert!(tag.self_closing);
        assert_eq!(tag.attribute("full-path"), Some("OEBPS/content.opf"));
        assert_eq!(tag.attribute("media-type"), Some("application/oebps-package+xml"));
        assert_eq!(tag.attribute("path"), None);
    }

    #[test]
    fn attribute_names_match_as_tag_names_do_and_values_are_decoded() {
        let tag =
            open("<opf:Item ID=\"c1\" HREF = 'a&amp;b.xhtml'\n epub:type=\"toc\" hidden linear=no title=\"say 'so'\">");
        assert_eq!(tag.name, "item");
        assert!(!tag.self_closing);
        assert_eq!(tag.attribute("id"), Some("c1"));
        assert_eq!(tag.attribute("href"), Some("a&b.xhtml"));
        assert_eq!(tag.attribute("type"), Some("toc"));
        assert_eq!(tag.attribute("hidden"), Some(""));
        assert_eq!(tag.attribute("linear"), Some("no"));
        assert_eq!(tag.attribute("title"), Some("say 'so'"));
    }

    #[test]
    fn the_first_of_two_attributes_of_one_name_is_the_one_found() {
        assert_eq!(open(r#"<a href="first" href="second">"#).attribute("href"), Some("first"));
    }

    #[test]
    fn a_quoted_value_may_hold_what_would_end_the_tag() {
        let found: Vec<Token> = tokens(r#"<a title="a > b" href='x/'>label</a>"#).collect();
        let [Token::Open(tag), label, end] = found.as_slice() else { panic!("{found:?}") };
        assert_eq!(tag.attribute("title"), Some("a > b"));
        assert_eq!(tag.attribute("href"), Some("x/"));
        assert!(!tag.self_closing);
        assert_eq!((label, end), (&text("label"), &close("a")));
    }

    #[test]
    fn close_tags_are_named_as_open_tags_are() {
        let found: Vec<Token> = tokens("</dc:Title></ NAV ></p\n>").collect();
        assert_eq!(found, [close("title"), close("nav"), close("p")]);
    }

    #[test]
    fn comments_instructions_and_doctype_yield_nothing() {
        let markup = "<?xml version=\"1.0\"?><!DOCTYPE html><!-- <p>no</p> --><?page 3?>";
        assert_eq!(tokens(markup).count(), 0);
    }

    #[test]
    fn cdata_is_text_as_written() {
        let found: Vec<Token> = tokens("<text><![CDATA[Salt &amp; <b>Iron</b>]]></text>").collect();
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(found[1..], [text("Salt &amp; <b>Iron</b>"), close("text")]);
    }

    #[test]
    fn text_keeps_its_whitespace_and_has_its_references_decoded() {
        let found: Vec<Token> = tokens(" Salt &amp;\n Iron&#33; ").collect();
        assert_eq!(found, [text(" Salt &\n Iron! ")]);
    }

    #[test]
    fn a_less_than_that_starts_no_markup_is_text() {
        let found: Vec<Token> = tokens("1 < 2 <3 <= 4 <").collect();
        assert_eq!(found, [text("1 < 2 <3 <= 4 <")]);
    }

    #[test]
    fn markup_cut_short_by_the_end_of_the_input() {
        assert_eq!(tokens("a<!-- never closed").collect::<Vec<_>>(), [text("a")]);
        assert_eq!(tokens("a<![CDATA[b").collect::<Vec<_>>(), [text("a"), text("b")]);
        assert_eq!(tokens("a</p").collect::<Vec<_>>(), [text("a"), close("p")]);
        assert_eq!(tokens("a<!DOCTYPE").collect::<Vec<_>>(), [text("a")]);
        assert_eq!(open(r#"<a href="x"#).attribute("href"), Some("x"));
        assert_eq!(open("<br/").name, "br");
        assert_eq!(open("<p").name, "p");
    }
}
