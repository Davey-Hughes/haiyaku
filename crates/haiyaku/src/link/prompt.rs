//! What one call of the link step sends the model, and the schema its reply fits: the later character's names
//! and quotes, the characters of the books before it that one of its names could be, and the rules of a reply.

use serde_json::{Value, json};

/// The system text of every call of the link step.
pub(super) const SYSTEM: &str = "You tell whether a character of a book is a character of the books before it in its \
                                 series, from the names and quotes you are given, and you reply with JSON only.";

/// What one call sends, each name, quote and title on one line (see [`line`]).
#[derive(Debug)]
pub(super) struct Sent {
    /// The later book's title.
    pub(super) book: String,
    /// The chapter of the name that made the call.
    pub(super) chapter: u32,
    pub(super) name: String,
    /// The later character's names to the end of `chapter`, each once.
    pub(super) names: Vec<String>,
    /// The quotes of the later character's book to the end of `chapter` that name it, each once: what a reply's
    /// quote has to be found in.
    pub(super) quotes: Vec<String>,
    /// In the order listed, the first as [`reference`]`(0)`.
    pub(super) candidates: Vec<Candidate>,
}

/// A character of the books before, as a call lists it.
#[derive(Debug)]
pub(super) struct Candidate {
    /// The title of its book.
    pub(super) book: String,
    /// The name it is shown under, and its other names.
    pub(super) name: String,
    pub(super) others: Vec<String>,
    /// The quote of its introduction.
    pub(super) introduction: String,
}

/// The ref a call gives the candidate at `place` of its list, counted from 0: `e1`, `e2`, ...
pub(super) fn reference(place: usize) -> String {
    format!("e{}", place + 1)
}

/// `text` on one line: each run of whitespace one space, and none at either end. A line break inside a name or a
/// quote would read as the start of the next line of its list; a quote is matched on its letters and digits alone,
/// so one sent this way is still found in the text it came from.
pub(super) fn line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The prompt of a call that sends `sent`: the later character and what its book says of it, then the
/// candidates, then the task and the rules of a reply, which come last, where a model follows instructions best.
pub(super) fn prompt(sent: &Sent) -> String {
    let chapter = sent.chapter;
    let mut prompt =
        format!("The later book: {}\nThe later character's name in chapter {chapter}: {}\n", sent.book, sent.name);
    prompt.push_str(&format!("\nThe later character's names, to the end of chapter {chapter}:\n"));
    for name in &sent.names {
        prompt.push_str(&format!("- {name}\n"));
    }
    prompt.push_str(&format!("\nThe later character's quotes, to the end of chapter {chapter}:\n"));
    for quote in &sent.quotes {
        prompt.push_str(&format!("- {quote}\n"));
    }
    prompt.push_str("\nThe candidates, characters of the books before it in its series:\n");
    for (place, candidate) in sent.candidates.iter().enumerate() {
        prompt.push_str(&format!(
            "\n{}, a character of {}\nname: {}\n",
            reference(place),
            candidate.book,
            candidate.name
        ));
        for other in &candidate.others {
            prompt.push_str(&format!("other name: {other}\n"));
        }
        prompt.push_str(&format!("introduced: {}\n", candidate.introduction));
    }
    prompt.push('\n');
    prompt.push_str(RULES);
    prompt
}

/// The task and the rules of a reply.
const RULES: &str = "Say whether the later character is one of the candidates: the same person, met again in a \
later book.

Rules:
- candidate: the ref of the candidate who is the same person as the later character, or null when none of them is, \
or when the names and quotes given here do not show it.
- Characters of one series often share a name, as a father and a son do: a name the two share does not by itself \
show that they are one person.
- Judge only from what is given here, not from what you know of these books or their series.
- quote: words copied exactly from one of the later character's quotes above, character for character, that show \
the later character is the candidate you give. Never a name alone, never two quotes joined, never a word changed. \
An empty string when candidate is null.
";

/// The JSON schema of a reply: the ref of one candidate or null, and a quote.
pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": { "candidate": { "type": ["string", "null"] }, "quote": { "type": "string" } },
        "required": ["candidate", "quote"],
        "additionalProperties": false,
    })
}

/// Whether `reply` fits [`schema`] as far as the step reads it: a candidate that is a string or null, and a quote
/// that is a string.
pub(super) fn fits(reply: &Value) -> bool {
    reply.get("candidate").is_some_and(|candidate| candidate.is_string() || candidate.is_null())
        && reply.get("quote").is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_makes_whitespace_one_space() {
        assert_eq!(line("  Ada\nwatched \t the\u{2028}boat  "), "Ada watched the boat");
    }

    #[test]
    fn a_reply_fits_with_a_string_or_null_candidate_and_a_string_quote() {
        assert!(fits(&json!({ "candidate": "e1", "quote": "Call me Tom" })));
        assert!(fits(&json!({ "candidate": null, "quote": "" })));
        for unfit in [
            json!({ "candidate": "e1" }),
            json!({ "quote": "Call me Tom" }),
            json!({ "candidate": 1, "quote": "Call me Tom" }),
            json!({ "candidate": "e1", "quote": null }),
        ] {
            assert!(!fits(&unfit), "{unfit}");
        }
    }
}
