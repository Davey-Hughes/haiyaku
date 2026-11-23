# haiyaku 0.2.0, the name look-up: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a caller holding a cast and the text from a tapped word learns which characters the text names, by which name and how much of the text the match covers; every character in a cast carries its names with their labels; a caller holding a book's SHA-256 learns the series and reading orders that hold it. All of it builds without `extract`, released as haiyaku 0.2.0.

**Architecture:** The fold keeps each name as a `log::Name`, the cast lists the books it folded, and the fold's output types become `#[non_exhaustive]`. A crate-private `words` module takes the word rules that extraction and linking share (what a word is, reducing, the scripts a particle is written onto, when a surname may stand alone); extraction and linking call it and decide nothing differently. A public `lookup` module, outside `extract`, matches a cast's names at the start of a text, with a Korean particle list for what may follow a Hangul name. `store` gains `series_holding`.

**Tech Stack:** Rust 2024 on toolchain 1.100.0, `rust-version` 1.91, rusqlite 0.43. No new dependency.

**Spec:** `docs/superpowers/specs/2026-11-23-haiyaku-name-lookup-design.md` (approved). The plan argues from it and reopens none of its decisions; the choices it leaves to the builder are settled in "The builder's choices" below.

## Global Constraints

- Toolchain `1.100.0`, edition 2024, `rust-version = "1.91"`: no API newer than 1.91. rustfmt (`max_width = 120`); workspace lints `unsafe_code = "forbid"` and `missing_debug_implementations` (every new pub type derives `Debug`); clippy `-D warnings` with all features, default features, and `--no-default-features --features bundled`.
- Runtime dependencies without `extract` stay rusqlite and sha2. No new dependency of any kind, and no Unicode segmentation or normalisation crate.
- `fold`, `cast`, `store`, `lookup` and `words` compile without `extract`. Dead code fails clippy there, so from Task 4 on every item of `words` is used by the look-up as well as by extraction or linking.
- Breaking API changes are allowed (0.x, so 0.2.0). `#[non_exhaustive]` goes on `Cast`, `Character`, `Relation`, `Fact`, `Overturn` and the look-up's `Mention`, and nowhere else: `EntityKey`, `ChapterRef` and `Name` stay open to construction.
- No schema change: `SCHEMA_VERSION` stays 2, no migration, no index.
- Extraction and linking decide nothing differently: `tests/golden/*.txt` pass unchanged in every task and `PROMPT_VERSION` stays as it is. Task 3, the move, changes no file under `crates/haiyaku/tests/`; a test that has to change for it to pass means the move changed behaviour: stop and report it, do not edit the test.
- The binary prints what it prints now: `tests/cli.rs` passes unchanged in every task.
- The look-up reads nothing but the cast and the text it is handed; the library renders nothing.
- Fixtures are invented. No text from the owner's books in the repository, a test or a commit message. The Korean fixture names (에이다 모건, 네드 모건, 단, 한 선장) are invented; 단지 and 단단히 stand where the design says 빌려서 and 빌딩.
- Comments say why (the reason, the invariant, what breaks if it changes), never history or measurements; a comment may point at a design section or an O-number.
- The shell is zsh with `noclobber`: `>` will not overwrite, use `>|`. `grep` is ugrep (use `/usr/bin/grep`). Run long commands into a log in the session scratchpad and read the summary lines; piping the gate itself through `tail` hides its exit status.
- Branch `name-lookup` in the worktree `/home/davey/temp/haiyaku-wt/name-lookup`, cut from `origin/main` 7f08e17. Never push to `main`. Publishing 0.2.0 is not a task: it is the owner's, after the merge.
- Every task ends with `scripts/check-all.sh >| <scratchpad>/check-<task>.log 2>&1; echo "exit $?"` and `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: every step of the script passes up to `cargo deny`, which is not installed on the desktop and stops the script there (O5); the 1.91 check passes.

## The builder's choices

The design leaves these to the builder; the plan settles them.

- Names: `haiyaku::lookup::characters_named`, its result `Mention` and `NamePart`, re-exported at the crate root; `haiyaku::store::series_holding` and `Holding`; the shared rules in `crates/haiyaku/src/words.rs`.
- Particles: taiyaku's list as its `dictionary/korean.rs` has it (71 entries), plus 네 for a family. Each particle must agree with the syllable before it, as taiyaku's do; at most two in a row.
- A match covers the name only: a particle, or what runs on after kana or a Han character, is outside it; a family's "s" or "es" is inside it. Characters before the first word (a quotation mark, a space) are inside it, since it is measured from the start of the text.
- Within one character, of its matches of the most words, the first in this order is reported: a whole name (the shown name, then the aliases in order), a given name, a surname.
- "A name of one word with no labels" is read as a one-word name with neither a given name nor a surname, rank 0 by `Name::rank`, which that method calls a description: so a bare "Professor" labelled only as a title is left out, as the design's list of examples wants.
- A surname of two words or more compares reduced, as every match of two words or more does; the design's surname rule "in the case written" is applied to one-word surnames, which are what its example ("Bell", "bell") is about.
- The shown name keeps the labels of the event that made it the shown one. The first-labelled rule picks the labels of the other names.

## Review Focus

1. Two characters with one full name (two "Tom Reed" rows, as O20's two "Tom Riddle" rows): both come back, most recently seen first. Test: Task 4, `two_characters_of_one_full_name_both_come_back`.
2. A name that only a character merged into another carried, by a same-as or by a link in series scope: the survivor comes back, once, under its own key. Test: Task 4, `a_merged_characters_name_finds_the_survivor`.
3. One character that matches by two of its names equally long (an alias "Ada" and the given name of "Ada Marsh"): it comes back once, reported by the whole name. Test: Task 4, `a_character_comes_back_once`.
4. A text that starts with a quotation mark or a space, a text with no word in it, and a name with no word in it: the leading characters are passed over and lie inside the match; an empty or wordless text names nobody, and a wordless name matches nothing; nothing panics on a byte that is not a character boundary. Tests: Task 4, `punctuation_before_the_name_is_passed_over` and `no_word_names_nobody`.
5. A name with an apostrophe inside (O'Dell), straight or curly: two words, compared reduced, and its possessive still matches. Test: Task 4, `a_name_with_an_apostrophe_is_two_words`.

---

### Task 1: Names in the cast

**Files:**
- Modify: `crates/haiyaku/src/log.rs:135` (`Name`'s derive), `crates/haiyaku/src/fold.rs` (`Cast` 28, `Character` 35, `Relation` 53, `Fact` 63, `Overturn` 79, `Member` 103-139, the `Introduced` arm of `Folder::apply`, `merge` 304-363, `finish` 366-390), `crates/haiyaku/src/link/run.rs` (201, 256, 399-400, 446), `crates/haiyaku/src/extract/roster.rs` (test 333-337), `crates/haiyaku/src/bin/haiyaku/show.rs` (`draw` 170-183), `crates/haiyaku/src/bin/haiyaku/view.rs` (85-92)
- Test: `crates/haiyaku/tests/fold.rs`, `tests/series.rs` (147-148, 168, 174-175, 220-221, 230), `tests/check.rs` (318-319, 346-347, 366, 376, 428-429), `tests/extract.rs` (398, 436-438), `tests/link.rs` (205), `tests/common/mod.rs` (the `texts` helper)

**Interfaces:**
- Produces, in `haiyaku::fold` (Task 4 reads all of it):

```rust
#[non_exhaustive]
pub struct Cast { pub characters: Vec<Character>, pub relations: Vec<Relation>, pub books: Vec<String> }
#[non_exhaustive]
pub struct Character {
    pub key: EntityKey,
    pub name: Name,
    pub aliases: Vec<Name>,
    pub first: ChapterRef,
    pub last: ChapterRef,
    pub merged: Vec<EntityKey>,
}
// Relation, Fact, Overturn: #[non_exhaustive], fields unchanged. All keep Debug, Clone, PartialEq, Eq.
```

- `log::Name` derives `Debug, Clone, PartialEq, Eq`.

Rules:
1. `Character.name` is the `Name` shown, chosen as now (the fullest by `Name::rank`; of equals, the one already shown), with the labels of the event that gave it.
2. `aliases` are the other names, each text once, in the order the text first gave them, to the character or to one merged into it, as now. Where one text came with different labels, its entry is the first, in event order, that has any label (`given`, `surname`, `title` or `title_sex`); with none labelled, the unlabelled one. Event order, not the order in which names reach the character, so where a merge is applied cannot change the labels, as it cannot change the order now.
3. `Cast.books` is the folder's own list of the books started, in that order, moved into the cast by `finish`. A book started with nothing of it read (a series reader in the later book's front matter) is in it. Its doc says it is what orders the `ChapterRef`s of different books.
4. The binary is a crate of its own, so `view.rs`'s destructuring of `Cast` gains `..`. The binary, the link step and the roster's test read `name.text` and the aliases' texts where they read strings now; what they print, send and compare does not change.
5. Tests that read `.name` or `.aliases` as strings read `.name.text` and the aliases' texts (a helper `pub fn texts(names: &[Name]) -> Vec<&str>` in `tests/common/mod.rs`), with every expected value as it is. In `tests/fold.rs` the `Character` literals (170-180, 436-446) become per-field assertions with the same values, and the `Fact` and `Overturn` literals (136-147, 182-193, 276-298, 596-613) are compared through a helper that gives a fact's fields as a tuple (a type alias for the tuple keeps clippy's `type_complexity` quiet). `tests/series.rs` `front_matter_shows_only_earlier_books` (230) compares the whole cast with `fold_book(&keeper(), 3)`, whose `books` is `[first]` where `cast_at`'s is `[first, second]`: it compares `characters` and `relations`, and `books` with `[first, second]`. No other test changes.

- [ ] **Step 1: Write the failing tests.**
  - `tests/fold.rs` `the_shown_name_keeps_its_labels`: `fold_book(&keeper(), 2)`: Ada's `name == proper("Ada Marsh", Some("Ada"), Some("Marsh"))`. `fold_book(&keeper(), 3)`: her `aliases == [described("the keeper"), Name { title: Some("Mrs.".into()), title_sex: Some(Sex::Female), ..proper("Mrs. Marsh", None, Some("Marsh")) }]`.
  - `tests/fold.rs` `of_one_text_the_first_labelled_name_is_kept_whenever_the_merge_comes`: `keeper_with` these events, by chapter: (1) ada introduced as `described("the keeper")`; (1) alias of ada `described("Marsh")`; (2) widow introduced as `Name { title: Some("Mrs.".into()), title_sex: Some(Sex::Female), ..proper("Mrs. Marsh", None, Some("Marsh")) }`; (2) alias of widow `proper("Marsh", None, Some("Marsh"))`; (3) `same_as("ada", "widow")`; (3) alias of ada `proper("Marsh", Some("Marsh"), None)`. Folded twice: by `fold_book(&log, 3)`, and by a `Folder` that applies every event but the same-as and then calls `merge(ada, widow)`. Both times Ada's `name` is the widow's "Mrs. Marsh" with all its labels, and her `aliases == [described("the keeper"), proper("Marsh", None, Some("Marsh"))]`.
  - `tests/series.rs` `the_cast_lists_the_books_it_folded`: `fold_book(&keeper(), 0).books == [first]`; `fold_series(&[keeper(), keepers_return()], Some(1), &keepers_links()).books == [first, second]`; `cast_at(&library(), &second, "front.xhtml", &in_series(None))`'s `books == [first, second]`; `fold_series(&[], None, &[]).books` is empty.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test fold --test series >| <scratchpad>/t1.log 2>&1; echo "exit $?"`. Expected: does not compile (`name` is a `String`, no `books`).
- [ ] **Step 3: Implement** rules 1-4 in the files listed; update the docs of `Cast`, `Character.name` and `Character.aliases` to say what they now hold.
- [ ] **Step 4: Update the tests** by rule 5, then run the gates (Global Constraints). Expected: every test passes, `tests/cli.rs` and the golden files untouched (`git diff --stat -- crates/haiyaku/tests/cli.rs crates/haiyaku/tests/golden` prints nothing).
- [ ] **Step 5: Commit**: "The cast carries each character's names with their labels, and the books it folded".

### Task 2: The series that hold a book

**Files:**
- Modify: `crates/haiyaku/src/store/series.rs` (new type and function after `read_series` 118-136; `book_is_stored` 382 reused), `crates/haiyaku/src/store/mod.rs:14` (re-export)
- Test: `crates/haiyaku/tests/series.rs`, which has no feature gate, so the gate's run without `extract` runs it too

**Interfaces:**
- Produces, in `haiyaku::store`:

```rust
/// A series that holds a book, and those of its reading orders that do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    pub series: String,
    /// By rank: the series' default order first, when it holds the book.
    pub orders: Vec<String>,
}

pub fn series_holding(conn: &Connection, book: &str) -> Result<Vec<Holding>>;
```

Rules:
1. Each series that has an order holding the book, once, by name (SQLite's binary order, which is Rust's order of `String`s); each with the names of its orders that hold the book, by `rank`.
2. A stored book in no series: `Ok` of an empty list. A book not stored: `Error::NotFound` naming `book <sha256>`, as `finished_chapters` names it.
3. Read in one `in_snapshot`, since the rows and the stored check are separate statements. No index (the design's call).
4. The doc lists the errors as `read_series`'s does, and says that which series and order a caller uses is the caller's choice.

- [ ] **Step 1: Write the failing tests** in `tests/series.rs`, on `db()` with `keeper()`, `keepers_return()` and `keepers_light()` written:
  - `series_holding_lists_orders_by_rank`: `keepers_and(&[order("chronological", &[&second, &first])])` stored; `series_holding(&conn, &first) == [Holding { series: "keepers".into(), orders: vec!["published".into(), "chronological".into()] }]` (rank order, not name order).
  - `series_holding_lists_series_by_name`: the series above, then `Series { name: "harbour", title: "The Harbour", orders: vec![order("main", &[&second])] }` stored after it; `series_holding(&conn, &second)` is `[Holding { series: "harbour", orders: ["main"] }, Holding { series: "keepers", orders: ["published", "chronological"] }]`.
  - `series_holding_of_a_book_in_no_series_is_empty`: `keepers_light()`'s hash gives `[]`.
  - `series_holding_needs_the_book_stored`: `not_found(series_holding(&conn, &"c".repeat(64)), &format!("book {}", "c".repeat(64)))`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test series`. Expected: does not compile (no `series_holding`).
- [ ] **Step 3: Implement** by rules 1-4.
- [ ] **Step 4: Run the gates.** Expected: all pass.
- [ ] **Step 5: Commit**: "The series and reading orders that hold a book".

### Task 3: The shared word rules move out of `extract`

A move: nothing extraction or linking decides changes, and the existing tests and golden files are the proof.

**Files:**
- Create: `crates/haiyaku/src/words.rs`
- Modify: `crates/haiyaku/src/lib.rs` (module list), `crates/haiyaku/src/extract/text.rs` (`locate_name` 84-99, `locate_surname` 106-120, `weight` 126-136, `reduce` 144-150, `words` 153-155, `written_words` 158-160, `writes_a_syllable` 165-183), `extract/mod.rs:17`, `extract/roster.rs:7`, `extract/check.rs:9`, `crates/haiyaku/src/link/names.rs` (doc 3, import 5, `words` 79-82), `link/run.rs:14`

**Interfaces:**
- Produces, crate-private in `crate::words` (Task 4 uses every one):

```rust
pub(crate) fn reduce(text: &str) -> String;                                  // extract/text.rs 144-150, as it is
pub(crate) fn word_spans(text: &str) -> impl Iterator<Item = (usize, &str)>; // each run of letters and digits, with its byte offset
pub(crate) fn written_words(text: &str) -> Vec<&str>;                        // extract/text.rs 158-160, as word_spans' words
pub(crate) fn reduced_words(text: &str) -> impl Iterator<Item = String>;     // link/names.rs 79-82, as word_spans' words reduced
pub(crate) fn writes_a_syllable(c: char) -> bool;                            // extract/text.rs 165-183, as it is
pub(crate) fn runs_on(word: &str) -> bool;                                   // locate_name's test (text.rs 87): its last char writes a syllable
pub(crate) fn has_case(text: &str) -> bool;                                  // locate_surname's test (111), over the letters and digits
pub(crate) fn stands_alone(surname: &str) -> bool;                           // locate_surname's floor (107-110): two letters or digits or more (O15)
```

Rules:
1. The bodies of `reduce` and `writes_a_syllable` move as they are, with their docs. `word_spans` is the one definition of a word (a run of characters for which `char::is_alphanumeric` holds); `written_words` and `reduced_words` are built on it. `runs_on`, `has_case` and `stands_alone` are the expressions `locate_name` and `locate_surname` hold now, named.
2. `extract/text.rs` keeps `locate_name`, `locate_surname`, `weight`, the quote functions and its own `words`, and calls the moved code. Its `words` stays as it is: `str::to_lowercase` keeps a final ς where `reduce` makes it σ, so putting `reduced_words` in its place would change what extraction finds in Greek.
3. Public paths do not move: `extract::{MIN_QUOTE_WEIGHT, QuoteError, locate_name, locate_quote, weight}` stay. `extract`'s `pub(crate) use text::{quote_in, reduce}` keeps `quote_in` only; `roster.rs`, `check.rs`, `link/names.rs` and `link/run.rs` take `reduce` from `crate::words`, and `link/names.rs` uses `reduced_words` in place of its own `words`.
4. Until Task 4 the module is declared `#[cfg(feature = "extract")] mod words;`, with a comment that only extraction and linking use it so far and that without them each item would be dead code, which clippy refuses. Task 4 removes both.

- [ ] **Step 1: Baseline.** `cargo test -p haiyaku --all-features >| <scratchpad>/t3-before.log 2>&1; echo "exit $?"`; keep its `test result:` lines.
- [ ] **Step 2: Write the failing unit test** `word_spans_give_each_word_and_its_offset`: create `words.rs` holding only its test module, declared in `lib.rs` as rule 4 says, with:

```rust
let spans = |text| word_spans(text).collect::<Vec<_>>();
assert_eq!(spans("\u{201c}Ada,\u{201d} said Mr. Marsh"), [(3, "Ada"), (11, "said"), (16, "Mr"), (20, "Marsh")]);
assert_eq!(spans("에이다는 웃었다"), [(0, "에이다는"), (13, "웃었다")]);
assert_eq!(spans("奈々は灯台へ行った。"), [(0, "奈々は灯台へ行った")]);
assert_eq!(spans(""), []);
assert_eq!(spans("\u{2014} !"), []);
```

- [ ] **Step 3: Run** `cargo test -p haiyaku --all-features --lib words`. Expected: does not compile (no `word_spans`).
- [ ] **Step 4: Move** by rules 1-4.
- [ ] **Step 5: Run** the tests again into `<scratchpad>/t3-after.log`. Expected: every `test result:` line of the baseline, with the same counts, plus the one new unit test; `git diff --stat -- crates/haiyaku/tests` prints nothing. Then the gates.
- [ ] **Step 6: Commit**: "The word rules extraction and linking share move out of extract, deciding nothing differently".

### Task 4: The look-up

**Files:**
- Create: `crates/haiyaku/src/lookup/mod.rs`, `crates/haiyaku/tests/lookup.rs` (no feature gate)
- Modify: `crates/haiyaku/src/lib.rs` (crate doc: it also says which characters a passage names; `mod words` loses its `cfg` and comment; `pub mod lookup`; `pub use lookup::{Mention, NamePart, characters_named}`)

**Interfaces:**
- Consumes: `fold::{Cast, Character}` with `Cast.books` and `Name`d names (Task 1); every function of `crate::words` (Task 3). Each needs a use here, or the build without `extract` fails clippy on dead code: `word_spans` and `reduce` for the text's words, `reduced_words` and `written_words` for a candidate's words reduced and as written, `runs_on` (and through it `writes_a_syllable`), `has_case`, `stands_alone`.
- Produces, in `haiyaku::lookup` and at the crate root (Task 5 extends the matching inside):

```rust
/// Which of a character's names a mention matched, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamePart { Whole, Given, Surname }

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mention<'a> {
    pub character: &'a Character,
    /// The name that matched, or that carries the given name or surname that did: the shown name or an alias.
    pub name: &'a Name,
    pub part: NamePart,
    /// The match is `text[..len]`.
    pub len: usize,
}

/// The characters `text` names at its start, as far as `cast` knows them, most recently seen first.
pub fn characters_named<'a>(cast: &'a Cast, text: &str) -> Vec<Mention<'a>>;
```

Rules (the design's "Which characters a text names"):
1. **Words.** The text's words are its `word_spans`; a name's words are its `written_words`. Only as many of the text's words are read as the longest name in the cast has.
2. **Candidates**, per character, in this order, which also breaks a tie within the character: each of its names (`name`, then `aliases` in order) as a whole, except a one-word name of rank 0; then the labelled given name of each of those names; then the labelled surname of each, where `stands_alone` holds. A candidate with no words is none.
3. **A match of n words** needs n words of the text: the first n-1 equal the candidate's, reduced. The last: when `runs_on` holds for the candidate's last word, the text's word begins with it (the text word's first k characters, k the count of characters in the candidate's word, equal the candidate's word, both reduced) and anything may follow (Task 5 narrows this after Hangul); otherwise the two are equal reduced. The match ends where the candidate's last word ends in the text, so what runs on is outside it.
4. **Case, for one word.** When n is 1, the candidate's word `has_case` and nothing runs on, the text's word must be the candidate's word as written, or be all capitals (an upper-case letter and no lower-case one) and equal it reduced. Two words or more compare reduced, a surname of two words included.
5. **The longest match wins.** A character's best match is its candidate of the most words, the first of rule 2's order among equals. Only the characters whose best has the most words of all come back, each once, with that match.
6. **Order:** by `last`, most recent first: the place of `last.book` in `cast.books`, then `last.chapter`; a stable sort keeps the cast's order for ties.

Fixtures in `tests/lookup.rs`: `cast_of(bodies: Vec<(u32, EventBody)>) -> Cast` folds `keeper()`'s record with these events (seq from 1, each at its chapter's document) to chapter 3; `found(cast, text) -> Vec<(&str, NamePart, &str, usize)>` gives each mention as its key's id, its part, its name's text and its `len`. Names are given as (text, given, surname), with a title where shown.

- `english()`: ch 1 introduced: `ned` ("Ned Marsh", Ned, Marsh), `ada` ("Ada Marsh", Ada, Marsh), `neddy` ("Ned Hale", Ned, Hale), `bill` ("Bill Reed", Bill, Reed), `ron` ("Ron", Ron, -), `keeper` ("the keeper", -, -), `father` ("Father", -, -), `professor` ("Professor", -, -, title "Professor"); ch 2: alias of `ned` ("Mr. Marsh", -, Marsh, title "Mr.", male); ch 3: `ada` appeared. So Ada is last seen in chapter 3, Ned in 2, the others in 1.
- `korean()`: ch 1 introduced: `ada` ("에이다 모건", 에이다, 모건), `ned` ("네드 모건", 네드, 모건), `dan` ("단", 단, -), `han` ("한 선장", -, 한, title "선장"), `mom` ("엄마", -, -); ch 2: alias of `ned` ("모건 씨", -, 모건, title "씨", male); ch 3: `ada` appeared.

- [ ] **Step 1: Write the failing tests** in `tests/lookup.rs` (`e = english()`, `k = korean()`; `Whole`, `Given`, `Surname` are `NamePart`'s):

```rust
// a_whole_name_and_a_given_name
assert_eq!(found(&e, "Ada Marsh climbed the stair."), [("ada", Whole, "Ada Marsh", 9)]);
assert_eq!(found(&e, "Ada climbed"), [("ada", Given, "Ada Marsh", 3)]);
// a_shared_surname_finds_everyone_most_recent_first
assert_eq!(found(&e, "Marsh nodded."), [("ada", Surname, "Ada Marsh", 5), ("ned", Surname, "Ned Marsh", 5)]);
// a_possessive
assert_eq!(found(&e, "Ada's lamp"), [("ada", Given, "Ada Marsh", 3)]);
assert_eq!(found(&e, "Ada Marsh's lamp"), [("ada", Whole, "Ada Marsh", 9)]);
// a_title_tapped_with_the_name_matches_as_a_whole
assert_eq!(found(&e, "Mr. Marsh rowed out."), [("ned", Whole, "Mr. Marsh", 9)]);
// a_name_does_not_match_the_start_of_a_longer_word
assert_eq!(found(&e, "Ronald came"), []);
assert_eq!(found(&e, "Ron came"), [("ron", Whole, "Ron", 3)]);
// the_longest_match_wins
assert_eq!(found(&e, "Ned Marsh and Ada"), [("ned", Whole, "Ned Marsh", 9)]);
assert_eq!(found(&e, "Ned came"), [("ned", Given, "Ned Marsh", 3), ("neddy", Given, "Ned Hale", 3)]);
// a_one_word_match_keeps_the_names_case
assert_eq!(found(&e, "marsh birds"), []);
assert_eq!(found(&e, "bill of sale"), []);
assert_eq!(found(&e, "reed beds"), []);
assert_eq!(found(&e, "Reed"), [("bill", Surname, "Bill Reed", 4)]);
// a_word_in_capitals_is_any_case
assert_eq!(found(&e, "MARSH!"), [("ada", Surname, "Ada Marsh", 5), ("ned", Surname, "Ned Marsh", 5)]);
assert_eq!(found(&e, "BILL"), [("bill", Given, "Bill Reed", 4)]);
assert_eq!(found(&e, "ADA MARSH"), [("ada", Whole, "Ada Marsh", 9)]);
// a_description_matches_as_a_whole
assert_eq!(found(&e, "the keeper climbed"), [("keeper", Whole, "the keeper", 10)]);
assert_eq!(found(&e, "The Keeper"), [("keeper", Whole, "the keeper", 10)]);
// a_one_word_description_does_not_match
assert_eq!(found(&e, "Father said"), []);
assert_eq!(found(&e, "Professor said"), []);
assert_eq!(found(&k, "엄마가 웃었다"), []);
// a_text_that_names_nobody
assert_eq!(found(&e, "The tide came in."), []);
// a_korean_name_takes_a_particle
assert_eq!(found(&k, "에이다는 웃었다"), [("ada", Given, "에이다 모건", 9)]);
assert_eq!(found(&k, "에이다 모건이 웃었다"), [("ada", Whole, "에이다 모건", 16)]);
// a_korean_title_after_the_name
assert_eq!(found(&k, "모건 씨는"), [("ned", Whole, "모건 씨", 10)]);
// a_korean_surname_alone
assert_eq!(found(&k, "모건이"), [("ada", Surname, "에이다 모건", 6), ("ned", Surname, "네드 모건", 6)]);
// a_one_syllable_surname_does_not_match_alone
assert_eq!(found(&k, "한은"), []);
assert_eq!(found(&k, "한숨을 쉬었다"), []);
assert_eq!(found(&k, "한 선장은"), [("han", Whole, "한 선장", 10)]);
// a_one_syllable_given_name_takes_a_particle
assert_eq!(found(&k, "단은"), [("dan", Whole, "단", 3)]);
// kana_or_han_runs_on_as_extraction_has_it: a cast of one character, "奈々" (given 奈々)
assert_eq!(found(&nana, "奈々は灯台へ行った。"), [("nana", Whole, "奈々", 6)]);
```

  And, each on its own cast:
  - `a_name_given_after_the_reader_does_not_match`, through `cast_at` on `db()` with `keeper()` written, `Scope::Book`: at `c2.xhtml`, "Ada Marsh" and "Ada" find nobody and "the keeper" finds `("ada", Whole, "the keeper", 10)`; at `c3.xhtml`, "Ada" finds `("ada", Given, "Ada Marsh", 3)` and "Tom Reed" nobody; at `back.xhtml`, "Tom Reed" finds `("tom", Whole, "Tom Reed", 8)`.
  - `most_recently_seen_first_by_the_order_of_the_books`: "Ada" over `fold_series(&[keeper(), keepers_return()], None, &[])` gives the keys `[second's ada, first's ada]` (second's last seen in its chapter 2, after all of the first book); over `fold_series(&[keepers_return(), keeper()], None, &[])`, `[first's ada, second's ada]`. Neither the hash order nor the chapter number alone gives both.
  - `two_characters_of_one_full_name_both_come_back`: (1) `elder` and (2) `younger` both introduced as ("Tom Reed", Tom, Reed): "Tom Reed" gives `[("younger", Whole, "Tom Reed", 8), ("elder", Whole, "Tom Reed", 8)]`.
  - `a_merged_characters_name_finds_the_survivor`: over `fold_book(&keeper(), 3)`, "the stranger" gives `[("tom", Whole, "the stranger", 12)]`; over `fold_series(&keepers_logs(), None, &keepers_links())`, "the old sailor" gives one mention, its key the first book's `tom`, its name's text "the old sailor", `len` 14.
  - `a_character_comes_back_once`: (1) `ada` introduced as ("Ada Marsh", Ada, Marsh), (2) alias ("Ada", Ada, -): "Ada" gives `[("ada", Whole, "Ada", 3)]`.
  - `punctuation_before_the_name_is_passed_over`: over `english()`, "\u{201c}Ada,\u{201d} she said" gives `[("ada", Given, "Ada Marsh", 6)]` and "  Ada" `[("ada", Given, "Ada Marsh", 5)]`.
  - `no_word_names_nobody`: (1) `dash` introduced as ("\u{2014}", "\u{2014}", -), (1) `ada` as ("Ada Marsh", Ada, Marsh): "" and "\u{2014} !" give `[]`; "\u{2014} Ada" gives `[("ada", Given, "Ada Marsh", 7)]`.
  - `a_name_with_an_apostrophe_is_two_words`: (1) `bran` introduced as ("Bran O'Dell", Bran, O'Dell): "O'Dell's boat" gives `[("bran", Surname, "Bran O'Dell", 6)]`, "O\u{2019}Dell" `[("bran", Surname, "Bran O'Dell", 8)]`, "Bran O'Dell" `[("bran", Whole, "Bran O'Dell", 11)]`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test lookup`. Expected: does not compile (no `lookup`).
- [ ] **Step 3: Implement** `characters_named` by rules 1-6, with a doc that states them; and the `lib.rs` changes.
- [ ] **Step 4: Run** `cargo test -p haiyaku --no-default-features --features bundled --test lookup`, then the gates. Expected: all pass, in both builds.
- [ ] **Step 5: Commit**: "A look-up of the characters a text names, over any cast".

### Task 5: Particles after a Korean name, and families

**Files:**
- Create: `crates/haiyaku/src/lookup/particles.rs`
- Modify: `crates/haiyaku/src/lookup/mod.rs` (rule 3's run-on, rule 2's surnames), `crates/haiyaku/tests/lookup.rs`

**Interfaces:**
- Consumes: `characters_named` and its rules (Task 4).
- Produces, inside `lookup` only: `particles::follow(word: &str, rest: &str) -> bool`, whether `rest` is one or two particles that may be written after `word`.

Rules (the design's "Particles, for the look-up only" and "A family"):
1. After a candidate whose last word ends in a Hangul syllable (U+AC00 to U+D7A3), what runs on must be empty, or one particle of the list, or two in a row. Each particle must agree with the syllable before it: the name's last syllable for the first, the first particle's last syllable for the second. A syllable's final consonant is `(c - 0xAC00) % 28`, 0 for none and 8 for ㄹ: Consonant needs one; ConsonantNotRieul one that is not ㄹ; Vowel none; VowelOrRieul none or ㄹ; Either takes any. After kana or a Han character anything still runs on (extraction's rule, until O9). Extraction's own rule does not change.
2. **A family.** A labelled surname that `stands_alone`, is one word and `has_case` also matches a text word that is the surname with "s" or "es" on, the surname written as the name writes it or the whole word in capitals. The match covers the whole word, counts as one word, and is a `Surname` match. In Korean, 네 in the list does it.
3. The list goes into `particles.rs` as a const of (particle, needs), with a comment that it is taiyaku's dictionary's list with 네 added for a family, copied so that haiyaku depends on nothing of taiyaku's. It is, by what each needs of the syllable before it:
   - Consonant: 은 이 을 과 이랑 이나 이야 이에요 이었다 이라고 이었고 이라는 이라도 이었는데
   - ConsonantNotRieul: 으로 으로는 으로서 으로써
   - Vowel: 는 가 를 와 랑 나 다 야 예요 라고 였다 였고 라는 라도 였는데
   - VowelOrRieul: 로 로는 로서 로써
   - Either: 의 에 께 도 만 들 인 이다 에서 에게 한테 께서 까지 부터 처럼 보다 하고 마다 조차 밖에 들은 들이 들을 들의 이고 만큼 대로 입니다 에서는 에서도 에게서 에게는 한테서 들에게 네

- [ ] **Step 1: Write the failing tests** in `tests/lookup.rs`:

```rust
// a_one_syllable_given_name_runs_into_nothing_but_particles
assert_eq!(found(&k, "단지 웃었다"), []);
assert_eq!(found(&k, "단단히"), []);
// a_particle_agrees_with_the_syllable_before_it
assert_eq!(found(&k, "단는"), []);
assert_eq!(found(&k, "에이다은"), []);
// two_particles_at_most
assert_eq!(found(&k, "에이다에게는"), [("ada", Given, "에이다 모건", 9)]);
assert_eq!(found(&k, "에이다에게도만"), []);
// a_family_by_its_plural_surname
assert_eq!(found(&e, "Marshes came"), [("ada", Surname, "Ada Marsh", 7), ("ned", Surname, "Ned Marsh", 7)]);
assert_eq!(found(&e, "Reeds' boat"), [("bill", Surname, "Bill Reed", 5)]);
assert_eq!(found(&e, "MARSHES"), [("ada", Surname, "Ada Marsh", 7), ("ned", Surname, "Ned Marsh", 7)]);
assert_eq!(found(&e, "marshes"), []);
assert_eq!(found(&k, "모건네"), [("ada", Surname, "에이다 모건", 6), ("ned", Surname, "네드 모건", 6)]);
```

- [ ] **Step 2: Run** `cargo test -p haiyaku --test lookup`. Expected: all four fail (within them, 에이다에게는, 모건네 and "marshes" already come out as wanted, since Task 4 admits any run-on and no plural; they pin that Task 5 keeps them so); every Task 4 test still passes.
- [ ] **Step 3: Implement** by rules 1-3.
- [ ] **Step 4: Run the gates.** Expected: all pass.
- [ ] **Step 5: Commit**: "Only particles after a Korean name, and a family by its plural surname, in the look-up".

### Task 6: haiyaku 0.2.0

**Files:**
- Modify: `Cargo.toml:9` (`[workspace.package] version`), `Cargo.lock` (the `haiyaku` entry), `docs/status.md`

Rules:
1. `version = "0.2.0"`. Run `cargo check --workspace` once without `--locked` so that `Cargo.lock`'s own entry reads 0.2.0; that one line is the lockfile's whole diff.
2. `docs/status.md`, from the design's "Release" and "Decided by the owner":
   - Where it stands, after the linking acceptance paragraph (66-73): a paragraph on 0.2.0, linking this plan and the design: each character of a cast carries its names with their labels and the cast lists the books it folded; `characters_named` finds the characters a text names, most recently seen first, with how much of the text the match covers and which name matched; `series_holding` gives the series and reading orders that hold a book; the name rules that extraction, linking and the look-up share build without `extract`.
   - Decided: the bullet at 109-111 says that haiyaku is a library in its own right, that a caller's needs may justify an API but it must be callable generally, and that the caller draws the data: haiyaku hands out data only and builds no markup, and the page that checks a run belongs to the binary. The bullet at 103 adds that the cut holds for a tap on a name too. New bullets: taiyaku builds on haiyaku first with a tap on a name that shows a card, and a Cast panel comes later; the look-up is two general pieces, `cast_at` and a pure look-up over any cast (linking the design); earlier books of the series show on the card when the reader's setting says so; one character map per edition's series for now, with joining editions kept possible; for now the owner fills a local taiyaku database by hand, and later taiyaku runs extraction itself (O4). No "(the owner's choice, <date>)" in this list. The bullet that haiyaku is published and taiyaku depends on the published crate (113-114) is there already.
   - Next (120-121): 1. Design the taiyaku integration in taiyaku's repository, on haiyaku 0.2.0 from crates.io (the design's "What taiyaku will do with this").
   - O7 (142-144): Closed: data only; the caller draws the data and haiyaku hands out data only (the owner, 2026-11-23; the name look-up design).
   - The "Updated" date at the top is the day of the change.

- [ ] **Step 1: Bump the version** by rule 1.
- [ ] **Step 2: Update `docs/status.md`** by rule 2.
- [ ] **Step 3: Run the gates**, and `git diff --exit-code origin/main -- crates/haiyaku/tests/golden; echo "exit $?"`. Expected: the gates as in Global Constraints; the golden files unchanged across the branch (exit 0).
- [ ] **Step 4: Commit**: "haiyaku 0.2.0, and the status after the name look-up".
