# haiyaku linking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** link the books of each reading order of a series, every possible match judged by the model in one call per later character and counted only on a quote found in what the call sent; load a series from a hand-written TOML file; carry a series with its books in one sidecar; show the cast at a point in a series, as text and as the checking page.

**Architecture:** `cast` gains `fold_series`, a pure fold by chapter number over logs and links passed in, which `cast_at`'s private fold shares and which the link step and `show --series` use. `store` gains `export_series` and `import_series`, outside `extract`, built on forms of `write_book`, `put_series` and `put_links` that run inside a caller's transaction. A new `link` module behind `extract` finds candidates by the name rules, makes its calls through extraction's reply cache (moved to `extract/cache.rs` so both share it), checks each reply's quote, and returns an order's links and a report; it writes to no database. The binary gains the series file (`toml`, `cli` only), the commands `series`, `link` and `export`, and `show --series`.

**Tech Stack:** Rust 2024, rusqlite 0.43, serde_json behind `extract`; one new dependency, `toml`, under `cli`.

**Spec:** `docs/superpowers/specs/2026-11-22-haiyaku-linking-design.md`. This is plan 3's second part; the first, reveals, landed in #8.

## Global Constraints

- Toolchain `1.100.0`, edition 2024, `rust-version = "1.91"`: no API newer than 1.91. rustfmt (`max_width = 120`) and the workspace lints: `unsafe_code = "forbid"`, `missing_debug_implementations` (every new pub type derives `Debug`).
- One new dependency, `toml`, optional and in `cli`'s list only (`cli = ["extract", "dep:clap", "dep:toml"]`), read into `toml::Table` by hand, with no serde derive. `Cargo.lock` is updated and committed in Task 6.
- `log`, `store`, `fold` and `cast` compile without `extract`: `fold_series`, `export_series` and `import_series` use no `serde_json`. `link` is behind `extract`.
- No schema change: `SCHEMA_VERSION` stays 2 and no migration is added.
- Extraction's calls stay byte for byte: `tests/golden/first_call.txt` and `reveal_call.txt` pass unchanged and `PROMPT_VERSION` stays 5, so the acceptance run's reruns of Sorcerer's Stone hit the reply cache.
- The step makes `LinkMethod::Model` links only; `Name` stays in the schema and in `read_links`.
- The library renders nothing: the report's text and the page are the binary's (`src/bin/haiyaku/`).
- No test calls a model or the network: tests use `llm::Replay` or a backend written in the test. An implementer never runs the binary on the owner's books; the acceptance run is the controller's, with the owner's go-ahead.
- Fixtures are invented. No text from the owner's books in the repository, a test or a commit message.
- Comments say why (the reason, the invariant, what breaks if it changes), never history or measurements; a comment may point at a section of the design or an O-number.
- The shell is zsh with `noclobber`: `>` will not overwrite, use `>|`. `grep` is ugrep (use `/usr/bin/grep`). Run long gates into a log file and read its summary; piping the gate itself through `tail` hides its exit status.
- Branch `linking` in the worktree `/home/davey/temp/haiyaku-wt/linking`, cut from `main`; never push to `main`. Every task ends with `cargo test -p haiyaku --all-features` and `cargo +1.91.0 check --workspace --all-targets --all-features --locked` passing; Tasks 1 and 2 also run `cargo test -p haiyaku --no-default-features --features bundled`; Task 8 runs `scripts/check-all.sh` whole.

## Review Focus

1. A link in a chapter whose last document ends the spine: `fold_series` applies it once that chapter is read, which `cast_at` can never count as finished. Test: Task 1, `fold_series_counts_a_chapter_that_ends_the_spine`.
2. A candidate that is several characters joined, by a same-as in its book and a link from a later one: the call lists the names merged into it, and the link names the survivor's key. Test: Task 5, `third_book_links_through_the_joined_character`.
3. A reply that does not fit the link schema: the step fails, the reply is not kept, and a rerun asks again instead of failing on the cached reply for ever. Test: Task 5, `reply_that_does_not_fit_is_not_kept`.
4. A series import that fails at its links after its books are written: every log and every link in the database, of this series and of others, is as it was. Test: Task 2, `failed_series_import_changes_nothing`.
5. Korean names: a title after the name (마시 부인), no article, a title's sex (씨, 부인). Test: Task 3, `korean_title_after_the_name_is_set_aside`.

---

### Task 1: The series fold

**Files:**
- Modify: `crates/haiyaku/src/cast.rs` (`cast_in_snapshot` 45-76, `fold_in` 78-104), `crates/haiyaku/src/lib.rs:18`
- Test: `crates/haiyaku/tests/series.rs`

**Interfaces:**
- Consumes: `fold::Folder`, `store::Link`, `log::BookLog`.
- Produces, in `haiyaku::cast` and re-exported at the crate root:

```rust
/// The cast of a reader who has read every log but the last whole and the last to the end of chapter `finished`
/// (whole for `None`), in that order, with the two characters of each link shown as one once its chapter is read.
pub fn fold_series(logs: &[BookLog], finished: Option<u32>, links: &[Link]) -> Cast;
```

Rules:
1. One private function folds one book's events and its due links into a `Folder`, applying a link once its chapter is in, as `fold_in` does now. `fold_in` reads the events and calls it; `fold_series` calls it once per log. `cast_at` does not change: every test in `tests/series.rs` passes as it stands.
2. A link whose later book is not among `logs`, or whose chapter is past `finished` in the last log, does nothing.

- [ ] **Step 1: Write the failing tests** in `tests/series.rs`:
  - `fold_series_is_cast_at_by_chapter`: for `[keeper(), keepers_return()]` and `keepers_links()`, `fold_series(.., Some(n), ..)` equals `cast_at` at `c{n+1}.xhtml` of the second book in series scope for n in 0..=2, and `None` equals it at `back.xhtml`.
  - `fold_series_of_one_book_is_fold_book`: `fold_series(&[keeper()], Some(n), &[]) == fold_book(&keeper(), n)` for n in 0..=3.
  - `fold_series_counts_a_chapter_that_ends_the_spine`: `keepers_return` without `back.xhtml`: no document of it lets `cast_at` show the sailor joined to Tom Reed, and `fold_series(.., Some(3), ..)` does.
  - `fold_series_passes_over_links_of_books_it_is_not_given`: `fold_series(&[keeper()], None, &keepers_links()) == fold_book(&keeper(), 3)`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test series`. Expected: fails to compile (no `fold_series`).
- [ ] **Step 3: Implement** `fold_series` and the shared fold.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, `cargo test -p haiyaku --no-default-features --features bundled`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "A fold of a series by chapter number over logs and links passed in, which cast_at shares".

### Task 2: The series sidecar

**Files:**
- Modify: `crates/haiyaku/src/store/sidecar.rs` (module doc 1-2; new functions beside `export_book` 44-68 and `import_book` 70-106; `write_sidecar` 127-137 and `migrated_copy` 139-153 reused), `store/book.rs` (`write_book` 13-81), `store/series.rs` (`put_series` 52-91, `put_links` 124-209), `store/mod.rs` (doc 1-2, re-exports 13-14)
- Test: `crates/haiyaku/tests/sidecar.rs`, which has no feature gate, so the gate's `--no-default-features --features bundled` run is the build without `extract` the design asks for

**Interfaces:**
- Produces, in `haiyaku::store`:

```rust
pub fn export_series(conn: &Connection, series: &str, path: &Path) -> Result<()>;
/// Returns the series stored.
pub fn import_series(conn: &mut Connection, sidecar: &Path) -> Result<Series>;
```

- Inside `store`: `write_book`, `put_series` and `put_links` each split into a `pub(super)` form that runs inside a transaction its caller holds, and the public function, which keeps its checks, its own transaction and its behaviour.

Rules:
1. **Export**, as `export_book`: `NotFound` `series <name>` before any file exists; the file written beside `path` and renamed over it; inside it, every book the orders hold, the series and each order's links, read inside one `in_snapshot` and written with `write_book`, `put_series` and `put_links` on the new file with foreign keys on, so it cannot hold what an import refuses.
2. **Import**: `require_foreign_keys` first; the file opened read-only, an older one migrated in memory (`migrated_copy`), a newer one refused (`SchemaTooNew`); `Invalid`, naming the file, unless it holds exactly one series. Then one immediate transaction on `conn`: each book under `write_book`'s rules, in the order the orders first list them (a log that differs from the stored one drops every stored link that names its book, in any series; one that does not keeps them), the series as `put_series` replaces it, each order's links as `put_links` stores them. An error anywhere rolls all of it back.
3. Both get `# Errors` docs in the form of the module's other functions.

- [ ] **Step 1: Write the failing tests** in `tests/sidecar.rs`, on the keepers (`common`: both books, `keepers()`, `keepers_links()`):
  - `series_sidecar_round_trips`: exported, then imported into a fresh `db()`: `read_series`, `read_links` and both `read_book`s equal the source's, and the import returns `keepers()`.
  - `series_import_replaces_a_differing_log_and_keeps_the_sidecars_links`: the target holds `keepers_return` with another `extracted_at` and the series without links: after the import its log is the sidecar's and the order's links are `keepers_links()`.
  - `book_sidecar_imported_after_drops_that_books_links`: then `import_book` of a sidecar of the differing `keepers_return` drops the order's links and keeps the series.
  - `failed_series_import_changes_nothing`: a sidecar whose last link is moved by SQL on the file (foreign keys off on it) to a document outside its chapter; the target holds both books with other logs and a second series linking them: the import is `Invalid`, and every log, series and link of the target is as before.
  - `import_refuses_a_sidecar_without_one_series` (a book sidecar), `series_import_requires_foreign_keys`, `export_of_an_unknown_series_writes_nothing`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test sidecar --test series`. Expected: fails to compile.
- [ ] **Step 3: Implement** the in-transaction forms, then export and import.
- [ ] **Step 4: Run** as Task 1, Step 4. Expected: all pass, the existing series and sidecar tests unchanged.
- [ ] **Step 5: Commit**: "A series sidecar that carries its books, its orders and their links, imported in one transaction".

### Task 3: The name rules of the link step

**Files:**
- Create: `crates/haiyaku/src/link/mod.rs` (module doc, `mod names;`), `crates/haiyaku/src/link/names.rs`
- Modify: `crates/haiyaku/src/lib.rs` 6-16 (`#[cfg(feature = "extract")] pub mod link;`), `crates/haiyaku/src/extract/text.rs` (beside `locate_quote` 22-60), `crates/haiyaku/src/extract/mod.rs` 5-15 (`reduce` and `quote_in` reachable from `link`, crate-private)
- Test: unit tests in `names.rs` and a test module in `text.rs`

**Interfaces:**
- Produces, crate-private:

```rust
// extract/text.rs: whether `text` holds `quote`, as `locate_quote` finds it in a chapter whose one block is `text`.
pub(crate) fn quote_in(text: &str, quote: &str) -> Result<(), QuoteError>;
// link/names.rs
pub(crate) fn bare(name: &Name) -> String;
pub(crate) fn matches(later: &Name, earlier: &Name) -> bool;
pub(crate) fn conflict(later: &[&Name], earlier: &[&Name]) -> bool;
```

Rules (the design's step 3):
1. Parts are compared reduced (`text::reduce`); a part that reduces to nothing is no part.
2. `bare`: the words of the text (runs of letters and digits, reduced) less the words of its `title`, and less a first word `the`, joined by one space. The title may stand before the name or after it (Mrs. Marsh, 마시 부인).
3. `matches`: false when both names state a `title_sex` and they differ; otherwise true when the given names are equal, or the surnames, or the non-empty `bare` forms.
4. `conflict`, per part: both sides have a given name, and no given name of either side is used by the other side's names; likewise surnames. A part is used by a list of names when it equals one of their given names or surnames, or a word of one of their texts. A conflict on either part turns the candidate down.
5. `quote_in` and `locate_quote` share their code, so the two cannot come to disagree.

- [ ] **Step 1: Write the failing tests.**
  - `names.rs`: `bare_sets_aside_the_title_and_a_leading_the` ("the old sailor", "Mrs. Marsh", "Captain Reed" with its title labelled); `korean_title_after_the_name_is_set_aside` ("마시 부인", title "부인", female: `bare` is "마시", it matches "에이다 마시" by surname, and not "마시 씨" titled 씨, male); `names_match_on_a_part_or_whole` (given, surname, a description equal whole; nothing shared; "Mr. Hale" and "Mrs. Hale"); `conflict_needs_a_part_on_both_sides` (["Ned Marsh"] against ["Ada Marsh", "Mrs. Marsh"] conflicts, ["Mr. Marsh"] does not); `a_part_the_other_side_uses_is_no_conflict` (["Thomas Reed", "Tom"] against ["Tom Reed"]).
  - `text.rs`: `quote_in_finds_what_locate_quote_finds` (case, punctuation and `...` as `locate_quote`; `TooShort`; `NotFound`).
- [ ] **Step 2: Run** `cargo test -p haiyaku --all-features --lib`. Expected: fails to compile.
- [ ] **Step 3: Implement.** Until Task 5 the new functions are used only by their tests; the dead-code warnings that gives are expected, and get no `allow`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "The link step's name rules: a shared part or an equal name makes a candidate, a conflicting part or a title of the other sex turns it down".

### Task 4: One reply cache for extraction and linking

**Files:**
- Create: `crates/haiyaku/src/extract/cache.rs`
- Modify: `crates/haiyaku/src/extract/run.rs` (`answer` 133-155, `call_twice` 157-171, `key` 173-183, `Cache` 185-233, `kept_reply` 241-253, `replace` 255-269, the unit tests 271-312 move with the code), `extract/mod.rs` 5-15
- Test: unit tests in `cache.rs`; `tests/extract.rs` and `tests/cli.rs` unchanged

**Interfaces:**
- Produces, crate-private: `Cache` for `<cache dir>/<book sha256>/`, built from the two, and

```rust
pub(crate) fn answer(backend: &dyn Backend, request: &Request<'_>, schema_text: &str, cache: Option<&Cache>,
    what: &str, fits: fn(&Value) -> bool) -> Result<Reply>;
```

Rules:
1. `what` names the call in the error of a second failure (`<what>, tried twice: <cause>`). Extraction passes `chapter <n> (<title>), first call` and `..., reveal call`, so its messages stay as they are.
2. `fits`: a backend's reply that does not fit is a failed call (`Error::Model` saying so), asked once more as any failure is, and never kept; a kept reply that does not fit is passed over and asked again, as a file that does not parse is now. Extraction passes a function that takes any object: its checks turn items down one by one.
3. Keys, file names and the cache's layout do not change, so replies cached before still count. `put_rejected` stays extraction's.

- [ ] **Step 1: Write the failing tests** in `cache.rs`: `a_reply_that_does_not_fit_is_asked_again_and_not_kept` (a `Replay` of one reply that does not fit and one that does: `answer` returns the second, and the one file kept holds it); `a_kept_reply_that_does_not_fit_is_asked_again`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --all-features --lib cache`. Expected: fails to compile.
- [ ] **Step 3: Implement**: move the code, add `what` and `fits`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass, every extraction test unchanged.
- [ ] **Step 5: Commit**: "One reply cache for every model call, which keeps no reply that does not fit its schema".

### Task 5: The link step

**Files:**
- Create: `crates/haiyaku/src/link/run.rs` (and `link/prompt.rs` for the call's text and schema if the implementer splits it), `crates/haiyaku/tests/link.rs` (`#![cfg(feature = "extract")]`), `crates/haiyaku/tests/golden/link_call.txt`
- Modify: `crates/haiyaku/src/link/mod.rs` (doc, re-exports)
- Test: `tests/link.rs`, with `tests/common/mod.rs`'s keepers (255-379), books written in the test, and a recording backend like `tests/extract.rs` 510-546

**Interfaces:**
- Consumes: `fold_series` (Task 1), the name rules and `quote_in` (Task 3), `answer` and `Cache` (Task 4); `Backend`, `Link`, `LinkMethod`, `EntityKey`, `BookLog`, `MIN_QUOTE_WEIGHT`.
- Produces, in `haiyaku::link` (every type `Debug`, `Clone`, `PartialEq`):

```rust
/// The links of one reading order, `logs` its books in reading order, and the report of the calls that made them.
pub fn link_order(logs: &[BookLog], backend: &dyn Backend, cache: Option<&Path>, progress: &mut dyn FnMut(&Call))
    -> Result<(Vec<Link>, Report)>;
pub struct Report { pub calls: Vec<Call>, pub seconds: f64, pub cost_usd: Option<f64> }
pub struct Call {
    pub later: EntityKey,
    /// The name that made the call, and its chapter.
    pub name: String,
    pub chapter: u32,
    /// Each candidate's key and shown name, in the order listed.
    pub candidates: Vec<(EntityKey, String)>,
    /// The ref the reply named, as written; `None` for none.
    pub answer: Option<String>,
    pub quote: String,
    pub verdict: Verdict,
}
pub enum Verdict { Linked(EntityKey), NoneChosen, NotListed, QuoteTooShort, QuoteIsAName, QuoteNotFound }
```

Rules:
1. **The earlier characters.** For each book after the first, the cast of `fold_series(&logs[..i], None, &links)`, `links` the links made so far for the order. Each carries its key; its shown name and other names as the cast gives them, for the prompt; for the name rules, the `Name` of every `Introduced`, `Named` and `Alias` event in the books before whose entity is its key or a key in its `merged`; its key's book's title; and the quote of its key's own `Introduced` event.
2. **The later characters**: each entity the book introduces, in the order of its `Introduced` events, taken alone: its names are its own `Introduced`, `Named` and `Alias` events, in `seq` order, never its same-as partner's.
3. **Candidates for a name** of chapter c: the earlier characters, in the cast's order, one of whose names `matches` it and for which `conflict` of the later character's names up to chapter c and its own names is false.
4. **The calls.** A name whose candidates include one no earlier call of this character listed gets a call listing all of them; any other name gets none. A `Linked` verdict ends the character's calls with `Link { later, earlier: candidate's key, method: Model { quote: the reply's quote }, chapter: c, at: the name event's at }`. A later character gets one link at most.
5. **What a call sends**, in a fixed order: the later book's title, chapter c and the name that made the call; the later character's names up to chapter c; its quotes, the quote of each event of its book up to the end of chapter c that names its id (an `Introduced`, `Named`, `Alias` or `Appeared` of it, or a `Relation`, `Overturned` or `SameAs` with it at either end), each text once, in `seq` order, and when there are more than 12, the first and the 11 latest; then each candidate as `e1`, `e2`, ... in the cast's order, with its book's title, its shown name, its other names and its introduction's quote. Every name and quote stands on one line, its whitespace made one space. The task and the rules close it and say: give the ref of the candidate who is the same person, or null when none is or the names and quotes given do not show it; characters of one series often share a name, as a father and a son do; judge only from what is given here, not from what you know of the books; copy the quote exactly from the later character's quotes. The system text and the wording are the implementer's; the golden file pins them.
6. **The schema**: an object with two required keys and no other, `candidate` (a string or null) and `quote` (a string). A reply fits when its `candidate` is a string or null and its `quote` a string.
7. **The verdict**, in this order: `NoneChosen` for a null candidate; `NotListed` for a ref the call did not list; `QuoteTooShort` when the quote weighs less than `MIN_QUOTE_WEIGHT`; `QuoteIsAName` when the quote, reduced, equals one of the later character's names or a listed candidate's, reduced, whatever its weight (the design, step 5: a name alone is too light to count); `QuoteNotFound` unless `quote_in` finds it in one of the quotes the call sent, taken one at a time, never across two; otherwise `Linked`.
8. **The cache and failure.** Each call goes through `answer` with the cache at `<cache>/<later book sha256>/`, `fits` from rule 6, and `what` naming the later book's title, the chapter and the name. Any error ends the step with no links; the replies kept so far stay, so a rerun pays only for the calls no run finished.
9. **The report** lists the calls in the order made. `seconds` sums theirs, cached ones at what they recorded; `cost_usd` sums their costs and is `None` when any call states none. `progress` is given each call once its verdict is known.

- [ ] **Step 1: Write the failing tests** in `tests/link.rs`:
  - `links_the_keepers`: `[keeper(), keepers_return()]` and a `Replay` of two replies (`e1` with "Ada watched the boat come in.", `e1` with "Call me Tom, said the old sailor."): the links are `keepers_links()` with the first's method `Model` and its quote; two calls, the second at chapter 3 for "Tom" (the sailor's introduction, a description, matches nothing); `put_links` stores them in a `db()` holding the keepers.
  - `link_call_is_byte_for_byte`: that second call, recorded, as `<system>\n---\n<user>---\n<schema>\n`, equals `include_str!("golden/link_call.txt")`. Write the file from the code (a throwaway `#[ignore]` test that writes it, deleted before the commit).
  - `namesakes_stay_apart`: a later book in which "Tom Reed" is the first book's Tom Reed's father; the reply is null: no link, `NoneChosen`, and `fold_series` of the two shows two characters named Tom Reed.
  - `conflicting_given_names_make_no_call` (a later "Ned Marsh" against Ada Marsh) and `a_title_of_the_other_sex_makes_no_call` (an earlier character named only "Mrs. Hale", a later "Mr. Hale"): `Replay::new(vec![])`, no call, no link.
  - `conflict_counts_names_up_to_the_names_chapter`: introduced as "Mr. Marsh" in chapter 1 and named "Ned Marsh" in chapter 3: the chapter-1 call lists Ada Marsh, the reply is null, and chapter 3 brings no candidate: one call in all.
  - `next_name_lists_every_candidate_its_name_brings`: an introduction with one candidate, answered null, and an alias two chapters on with that one and another: the second call lists both, and its link to the second holds from the alias's chapter and block.
  - `quotes_stop_at_the_names_chapter`: 15 events naming the character up to its alias's chapter and more after: the call sends the first and the 11 latest of those, in `seq` order, and no later one.
  - `quote_not_sent_links_nothing`: a listed candidate with, in turn, a quote from a chapter after the name's, one that runs across two sent quotes, and "Tom": `QuoteNotFound`, `QuoteNotFound`, `QuoteTooShort`; then the later character's own full name of at least `MIN_QUOTE_WEIGHT` letters, which is in a sent quote: `QuoteIsAName`; `e9` is `NotListed`. No link.
  - `third_book_links_through_the_joined_character`: `keeper`, `keepers_return`, `keepers_light`: the captain's call lists one candidate whose names include "the stranger", "the old sailor" and "Tom"; the link's earlier key is the first book's `tom`; `fold_series` of the three shows one character whose `merged` is the stranger, the sailor and the captain.
  - `failed_call_returns_no_links_and_a_rerun_asks_only_the_rest`: with a cache, a `Replay` of one reply fails at the second call with `Error::Model` naming "The Keeper's Return", chapter 3 and "Tom"; a rerun with the cache and a `Replay` of the remaining reply makes both links and leaves that replay empty.
  - `reply_that_does_not_fit_is_not_kept`: `{"candidate":"e1"}` twice is `Error::Model`, the cache keeps no reply for the call, and a rerun with a reply that fits links.
  - `report_sums_the_calls`: with a recording backend at 1.5 s and 0.25 USD a call, the keepers' report gives 3.0 s and 0.5 USD; with a `Replay`, `None`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test link`. Expected: fails to compile.
- [ ] **Step 3: Implement** the step per the rules.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "The link step: one model call per later character, counted only on a quote found in what the call sent".

### Task 6: The series file, `haiyaku series` and `haiyaku export`

**Files:**
- Create: `crates/haiyaku/src/bin/haiyaku/series.rs`, `crates/haiyaku/src/bin/haiyaku/export.rs`
- Modify: `crates/haiyaku/Cargo.toml` (features 13-25, dependencies 32-39), `Cargo.lock`, `src/bin/haiyaku/main.rs` (doc 1-3, modules 8-12, `Command` 35-42, `main` 44-49)
- Test: `crates/haiyaku/tests/cli.rs` (`Sandbox` 24-115), with a second EPUB from `common::epub::book` under its own metadata, so its hash differs, and logs written under each EPUB's hash as `Sandbox::store` does

**Interfaces:**
- Produces, in the binary's `series.rs`: `read(path) -> Result<SeriesFile, Failure>`, the file parsed and each book of an order hashed into a `store::Series`, with each label's path kept for messages; and `stored(conn, &SeriesFile) -> Result<Series, Failure>`, the series as stored, refused unless its orders (names and books, in order) are the file's. `link`, `export` and `show --series` all go through `stored`.

Rules:
1. **The file**, as the design's layout: top-level `name` and `title` (strings), `[books]` (label to path), `[[order]]` each with `name` and `books` (labels); all required. An unknown key at any level, a value of the wrong type, and a label an order uses that `[books]` lacks are refused; a label no order uses is passed over. A path is read from the file's folder unless absolute. Every message starts with the file's path.
2. **`haiyaku series <file>`** opens the database for writing, refuses a book an order holds that the database does not (`<book path> has not been extracted into <database>`), counts the links the stored series of that name has, stores the series with `put_series` (its errors prefixed with the file's path) and, when the count was not 0, says that many links are gone and that `haiyaku link <file>` makes them again.
3. **`stored`** refuses a series the database does not hold, or holds with other orders, with a message that asks for `haiyaku series <file>` first.
4. **`haiyaku export <path>`**: a path whose extension is `epub`, in any case, is a book: `export_book` to `<stem>.haiyaku.db` beside it, a book not extracted named by its path; any other path is a series file: `stored`, then `export_series` to `<name>.haiyaku.db` beside the file. It prints the path written.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs`:
  - `series_stores_the_files_series`: a file with one relative and one absolute book path and two orders: `read_series` gives the title, the orders by rank and their books' hashes in order.
  - `series_file_errors_name_the_file`: an unknown key at the top, in `[books]` and in an order; a book path that is not a string; a label `[books]` lacks; no order; two orders of one name; a missing book file; a book not extracted: each exits 1 with the file's path (or, for the last two, the book's) in stderr, and stores no series.
  - `series_says_its_links_are_gone`: with the keepers' links stored, `series` again prints that 2 links are gone and names `haiyaku link`; `read_links` is empty.
  - `export_writes_a_book_sidecar_beside_it` and `export_writes_a_series_sidecar_beside_the_file` (`import_series` into a fresh database gives the series, both logs and the links); `export_refuses_a_file_the_database_differs_from`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: the new tests fail.
- [ ] **Step 3: Implement**: the dependency (run `cargo build` once without `--locked` to update `Cargo.lock`), the file, both commands.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, `cargo clippy --workspace --all-targets --no-default-features --features bundled -- -D warnings` (no `toml` outside `cli`), then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "A series file the owner writes, and the series and export commands".

### Task 7: `haiyaku link`

**Files:**
- Create: `crates/haiyaku/src/bin/haiyaku/link.rs`
- Modify: `src/bin/haiyaku/main.rs` (`Command`, `main`), `src/bin/haiyaku/extract.rs` (`cost` and `counted` 172-179, shared if the report uses them)
- Test: `crates/haiyaku/tests/cli.rs`

**Interfaces:**
- Consumes: `link_order`, `Report`, `Call` (Task 5); `series::read` and `stored` (Task 6); `paths::cache`, `open_database`, `say`, `tell`.

Rules:
1. **Options**: `<file>`, `--order <name>`, `--backend <spec>` (default `claude`, help as `extract`'s, with its note on rerunning a replay with `--no-cache`), `--no-cache`. The backend's settings are `extract`'s: the default timeout and `OLLAMA_HOST` from the environment.
2. **The run**: open the database for writing; `stored`; an `--order` the series lacks is an error naming it. For each order, in rank order or the one named: read each book's log, `link_order` with `paths::cache` unless `--no-cache`, a progress line on stderr for each call, then `put_links`. A failed order stores nothing and fails the command; orders before it keep what they stored.
3. **The report** on stdout, per order: a line per call with the later book's title, the chapter, the name, the candidates, the answer, the verdict and the quote; then a line with the order's name, its links, its calls, the seconds and the cost written as `extract`'s totals write it.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs`, with both books stored, the series loaded and a replay directory of Task 5's two keepers replies:
  - `link_stores_each_orders_links`: exit 0; `read_links` equals Task 5's `links_the_keepers` links; stdout has a line per call with its verdict and an order line with 2 links and 2 calls.
  - `link_order_option_links_one_order`: with a second order, `--order` links only that one.
  - `link_refuses_a_file_the_database_differs_from`: exit 1, nothing stored, the message names `haiyaku series`.
  - `failed_link_keeps_the_orders_links`: links stored before; a replay of one reply: exit 1, the links as they were.
  - `second_link_uses_the_cache`: a rerun with an empty replay directory makes the same links; `--no-cache` leaves no cache directory.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: the new tests fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "The link command, which links each order of a series and stores its links".

### Task 8: `show --series`, and the documents

**Files:**
- Modify: `crates/haiyaku/src/bin/haiyaku/show.rs` (`Args` 15-29, `run` 33-62, `draw` 64-104), `view.rs` (`page` 12-18, `step` 20-71), `view.html` (`#where` 92, `chapters` 218-220, `chosen` and `nameBefore` 222-237, `render` 239-252, the nodes' "here" 316-336, `factRest` and `overturnText` 338-351, `panel` and `about` 357-410, the title 468-469), `src/bin/haiyaku/main.rs` doc 1-3, `docs/superpowers/specs/2026-11-21-haiyaku-library-design.md` (rule 3 at 231, the series sentences of "Storage" at 303-304, the binary's list 335-341), `docs/status.md`, `README.md` 13-22
- Test: `crates/haiyaku/tests/cli.rs`

**Interfaces:**
- Consumes: `fold_series` (Task 1), `stored` (Task 6), `read_links`, `read_book`.

Rules:
1. **`show <book> --series <file> [--order <name>]`**: `stored`; the order named or the first; the book has to be in it. The logs of the books before it and its own, the order's links, and `fold_series(.., Some(at), ..)`. `--at` counts this book's chapters as now. Without `--series` nothing changes (`cli.rs` 579-870 pin it), and `--order` needs `--series`.
2. **Text**: a chapter of another book than the one shown is written `<title> ch <n>`: a character's span is `ch a-b` when both ends are in the shown book, `<title> ch a-b` when both are in one other book, and `<first> to <last>` otherwise; an overturn's mark names its chapter the same way.
3. **The page's data** in series scope: `books`, the titles of the order up to this book; one step for nothing read, then one per chapter of each earlier book and one per chapter of this book up to its last extracted, each with `book` (an index into `books`) and `chapter`; every other chapter in the data gains its book beside it (`first` and `firstBook`, `last` and `lastBook`, an overturn's `chapter` and `book`); a character's id is `<book index>:<id>`, so two books' `c1` never meet; and each step has `links`, the links applied by then, each `{later, earlier, book, chapter, quote}`. `at` is the step of this book's chapter `--at`. The single-book page's data gains `books` and the `*Book` fields and is otherwise as now.
4. **The page**: the place line names the book; "in this chapter" compares book and chapter; a chapter of another book reads `<title> ch <n>`; a character's panel lists the links that joined into it, each with its book, chapter and quote.
5. **Documents.** The library design's rule 3 says that an equal given name and surname makes a candidate, not a link, and "Storage" that a series sidecar carries its books, so nothing is imported before it; both point to the linking design, as they point to the reveals design. Its list of commands matches what was built. `docs/status.md`: Where it stands gains linking, the series commands and the series sidecar; Decided's bullet on the link step (68-71) says every possible match goes to the model, one call per later character; Next drops plan 3 (the controller asks the owner what follows and writes it). `README.md`'s status names the new commands.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs`, with the keepers stored under two EPUBs, the series loaded and the links stored:
  - `show_series_prints_the_series_cast`: the second book at 2: one Ada Marsh, her span starting `The Lighthouse Keeper ch 1`; the old sailor apart from Tom Reed.
  - `show_series_at_the_last_chapter_applies_its_link`: `--at 3`, with the second book's log stored without `back.xhtml`, so its chapter 3 ends the spine: the old sailor is among Tom Reed's other names.
  - `show_series_html_steps_through_the_order`: 7 steps (none read, 3 and 3); each step's `book` and `chapter`; no id twice in a step; the last step's `links` hold the sailor's with chapter 3 and its quote; a character first met in book 1 has `firstBook` 0 in a book-2 step.
  - `show_series_needs_the_book_in_the_order`; `order_without_series_is_a_usage_error`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: the new tests fail.
- [ ] **Step 3: Implement** `show`, the data and the page. Write the series page for the test's books to `.superpowers/sdd/2026-11-22-haiyaku-linking/series.html` (gitignored; no scratch file in the working tree); the controller looks at it with headless `google-chrome-stable --screenshot` at a step of each book and records the check in the task ledger.
- [ ] **Step 4: The documents**, per rule 5.
- [ ] **Step 5: Run** `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"`, then the 1.91 check. Expected: every step passes up to `cargo deny` (not installed on the desktop, O5).
- [ ] **Step 6: Commit**: "show --series prints and pages the cast of a series, and the documents say how linking works".

### Acceptance run, before the PR

The controller runs this, not an implementer, and only after the owner agrees to the spend: about 21-22 USD on the Claude login (Chamber of Secrets about 6 and Goblet of Fire about 14, both languages at the reveals run's cost a chapter; linking about 1.5).

- [ ] **Step 1:** `cargo build --release -p haiyaku` in the worktree, with `CARGO_TARGET_DIR` unset.
- [ ] **Step 2:** One database a language under `spike/out/linking/` (gitignored). In each, Sorcerer's Stone extracted again with the usual reply cache (English entries 4-20, Korean 5-21); it should cost next to nothing, and if its first chapters miss the cache, stop and ask. Chamber of Secrets and Goblet of Fire extracted in both languages, their story chapters picked from each table of contents, leaving out any excerpt of the next book. Each run's stderr kept in the scratchpad.
- [ ] **Step 3:** A series file a language in `spike/out/linking/`, one order: Sorcerer's Stone, Chamber of Secrets, Goblet of Fire. `haiyaku series`, `haiyaku link` with its report kept beside the files, and `show --series --html` pages for the second and third books.
- [ ] **Step 4:** The design's checks, in both languages:
  1. Tom Riddle joins Voldemort at Chamber of Secrets chapter 17 and not before.
  2. Tom the barman stays apart from Tom Riddle and from Voldemort; Lucius Malfoy stays apart from Draco.
  3. Goblet of Fire's Tom Riddle, Voldemort's father, stays apart from Voldemort at the end of the book.
  4. Every link in the report is right, checked by hand; each character both books list that stays unlinked is recorded with the reason; for Chamber of Secrets in English the count is set beside the spike's 49.
  5. Linking's cost a book in a language, against goal 7 (about 0.3 USD).
  6. Recorded, with no pass or fail: whether Goblet of Fire's extraction makes Moody and Barty Crouch Jr. one character, and what the step does with Peter Pettigrew, Wormtail and Scabbers with book 3 left out.

  Checks 1 to 3 judge the link step: a join the link step makes fails the run; a join the extraction makes inside a book (`Roster::holder`) becomes an open item in `docs/status.md` for a pull request of its own and does not fail this one.
- [ ] **Step 5:** A section at the top of `docs/notes.md` with each run's time and cost, each order's calls, links and cost, and each check's outcome: numbers and character names only, no line of the books. A failed check is a finding against the task that owns it; fix it there, and ask the owner before each rerun. Extraction faults the run finds become open items with O-numbers.
- [ ] **Step 6:** `/code-review high linking`, the fixes, a review of the fixes, then the push and the PR (title under 255 characters, one line per paragraph, no commit list).
