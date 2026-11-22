# haiyaku reveals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** every extracted chapter gets a second model call that records what the chapter reveals and which recorded relations it shows were never true; an `overturned` event marks those relations in the cast; a same-as between related characters is refused (O12) and a fuller name given as an alias becomes the shown name (O13).

**Architecture:** The `overturned` event enters the log, the store (schema version 2, migration 002) and the fold, which marks the facts it covers and hands the mark out on `fold::Fact`. In `extract`, `reveal_prompt`, `reveal_schema` and `check_reveals` sit beside the first call's `prompt`, `schema` and `check`, and `extract_book` makes the second call after each chapter's first is checked. The first call's system text, prompt wording and layout, and schema do not change, so cached first calls still count.

**Tech Stack:** Rust 2024, rusqlite 0.43, serde_json behind `extract`; no new dependency.

**Spec:** `docs/superpowers/specs/2026-11-22-haiyaku-reveals-design.md`. The trial behind it is in `docs/notes.md`, "A second call per chapter, 2026-11-22". This is plan 3's first part; linking a series is its second, planned after this lands.

## Global Constraints

- Toolchain `1.100.0`, edition 2024, `rust-version = "1.91"`: no API newer than 1.91. rustfmt (`max_width = 120`) and the workspace lints as they stand: `unsafe_code = "forbid"`, `missing_debug_implementations` (every new pub type derives `Debug`).
- No new dependency. `log`, `store`, `fold` and `cast` compile without the `extract` feature, so they use no `serde_json`.
- `store/schema/001_initial.sql` never changes: a database that ran it would never see the change. Version 2 is a new file.
- The first call stays byte for byte: `SYSTEM`, the wording and layout of `prompt()` and `schema()`. Task 4's golden test pins them. Its prompt still changes where the roster it lists changes (an overturn, O13's names).
- `PROMPT_VERSION` becomes 5 (Task 4): it versions the second call's wording too.
- The library renders nothing. How an overturned fact looks belongs to the binary (`src/bin/haiyaku/`) and to each consumer.
- No test calls a model or the network: tests use `llm::Replay` or a backend written in the test. An implementer never runs the binary on the owner's books; the acceptance run is the controller's, with the owner's go-ahead.
- Fixtures are invented. No text from the owner's books in the repository, a test or a commit message.
- Comments say why (the reason, the invariant, what breaks if it changes), never history or measurements; a comment may point at a section of the design or an O-number.
- The shell is zsh with `noclobber`: `>` will not overwrite, use `>|`. `grep` is ugrep (use `/usr/bin/grep`). Run long gates into a log file and read its summary; piping the gate itself through `tail` hides its exit status.
- Branch `reveals` in the worktree `/home/davey/temp/haiyaku-wt/reveals`, cut from `main`; never push to `main`. Every task ends with `cargo test -p haiyaku --all-features` and `cargo +1.91.0 check --workspace --all-targets --all-features --locked` passing; Task 6 runs `scripts/check-all.sh` whole.

## Review Focus

1. A version-1 database migrated with foreign keys on, with them off, and inside a transaction of the caller's: every row survives (`DROP TABLE haiyaku_event` with foreign keys on deletes the child rows by cascade). Tests: Task 1, `version_one_migrates_keeping_every_row` and `version_one_migrates_inside_a_callers_transaction`.
2. An overturn in a later book that covers a fact from an earlier book, through a link applied before it or after it. Tests: Task 2, `overturn_reaches_an_earlier_book_through_a_link` and `link_after_an_overturn_still_brings_the_fact_under_it`.
3. A second reply that relates and overturns the same pair: the overturn must not cover the relation its own reply adds. Test: Task 4, `overturn_cannot_cover_its_own_replys_relation`.
4. A run that stops between a chapter's first and second call: the next run takes the first from the cache and pays only for the second. Test: Task 5, `resume_after_a_failed_second_call`.
5. `show --at` a chapter before the overturn: the fact prints unmarked, so the cut-off holds. Test: Task 6, `show_before_the_overturn_prints_the_fact_unmarked`.

---

### Task 1: The `overturned` event in the log and the store, and migration 002

**Files:**
- Create: `crates/haiyaku/src/store/schema/002_overturned.sql`
- Modify: `crates/haiyaku/src/log.rs:113-128` (the variant), `crates/haiyaku/src/store/migrate.rs:9,13`, `crates/haiyaku/src/store/book.rs` (`read_events` 165-188, `insert_events` 331-370, `event_type` 373-382, `event_from_row` 385-434), `crates/haiyaku/src/extract/roster.rs` (`push` 144-170, `named_ids` 233-242), `crates/haiyaku/src/fold.rs:180-232` (an arm that ignores the event until Task 2)
- Test: `crates/haiyaku/tests/migrate.rs`, `tests/book.rs`, `tests/sidecar.rs`, and the exhaustive `fields` helper in `tests/check.rs:112-123`

**Interfaces:**
- Produces: `EventBody::Overturned { from: String, to: String, kind: RelationKind, note: String }`, documented as: the chapter shows that the relation of `kind` between the two, recorded before this event, was never true; for a directed kind it runs from `from` to `to`, for the others the two are in no order. Event type `'overturned'`; child table `haiyaku_overturn (book, seq, from_entity, to_entity, kind, note)`, shaped like `haiyaku_relation` (same kind check, `from_entity <> to_entity`, the same three foreign keys). `SCHEMA_VERSION = 2`.

Rules:
1. **Migration 002** runs inside the transaction or savepoint `migrate_with` opens, where `PRAGMA foreign_keys` cannot change and is whatever the caller set. With foreign keys on, dropping `haiyaku_event` deletes the four child tables' rows by cascade. So: copy `haiyaku_event` and the four child tables aside (`CREATE TABLE ... AS SELECT`, reading `main.` tables), drop the four child tables, then `haiyaku_event`, create `haiyaku_event` with `'overturned'` in its type check and the four child tables exactly as `001_initial.sql` has them, create `haiyaku_overturn`, copy the rows back (events before their children) and drop the copies. No `ALTER TABLE ... RENAME`: it rewrites other tables' foreign keys. 001 has no index or trigger to recreate.
2. `read_events` gains a `LEFT JOIN` for `haiyaku_overturn`; the comment that counts the child tables says five.

- [ ] **Step 1: Write the failing tests.**
  - `tests/migrate.rs`: `TABLES` gains `haiyaku_overturn` (15); the tests that assert version `Some(1)` assert `Some(2)`; `newer_schema_is_refused` writes version 3 and expects `found: 3, supported: 2`.
  - `tests/migrate.rs`, `version_one_migrates_keeping_every_row`: a database made by running `include_str!("../src/store/schema/001_initial.sql")` and writing version 1 into `haiyaku_schema` as `migrate.rs:106-108` does, filled with a book holding one event of each of the six version-1 types; run once with `PRAGMA foreign_keys = ON` and once with it off: after `migrate`, `schema_version` is `Some(2)`, `read_book` returns the log written before, `PRAGMA foreign_key_check` returns no row, and `foreign_keys` is as the test set it.
  - `tests/migrate.rs`, `version_one_migrates_inside_a_callers_transaction`: the same database, `BEGIN IMMEDIATE`, `migrate`, `COMMIT`: every row is there.
  - `tests/book.rs`: `overturned_round_trips` (a log with a `Relation` and a later `Overturned` of it reads back equal); the child-row list at 354 gains `("haiyaku_overturn", <its seq>)` so `event_without_its_child_row_is_invalid` covers it.
  - `tests/sidecar.rs`, `import_migrates_a_version_one_sidecar` (O6): a version-1 sidecar file built as above, holding a book, imports into a fresh database and reads back equal; the file is unchanged on disk.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test migrate --test book --test sidecar`. Expected: fails to compile (no `Overturned`).
- [ ] **Step 3: Implement** the variant, the migration file, the version bump, the store's five places, the roster arms (`push` stores the event; `named_ids` gives `from` and `to`), the fold arm (ignores it; Task 2 replaces it) and the `fields` arm in `tests/check.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, `cargo test -p haiyaku --no-default-features --features bundled`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "An overturned event, and schema version 2 that stores it".

### Task 2: The fold marks the facts an overturn covers

**Files:**
- Modify: `crates/haiyaku/src/fold.rs` (`Fact` 61-72, `Claim` 128-137, `Folder` 140-150, `apply` 180-232, `merge` 246-303, `finish` 306-338, `claim` 368-383)
- Test: `crates/haiyaku/tests/fold.rs` (the `Fact { .. }` literals at 113, 158, 252, 260 gain `overturned: None`), `tests/series.rs` for the link tests if its helpers fit better

**Interfaces:**
- Consumes: `EventBody::Overturned` (Task 1).
- Produces, in `haiyaku::fold`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overturn {
    /// The chapter whose overturned event covers the fact.
    pub at: ChapterRef,
    pub note: String,
    pub quote: String,
}
// Fact gains:
    /// Set when a later chapter showed the fact was never true; `None` for a fact that stands.
    pub overturned: Option<Overturn>,
```

Rules:
1. **Coverage.** For a pair, a kind, and for a directed kind a direction: an overturn covers every statement (relation event) made before it, by the fold's order of events (`Origin`), that no earlier overturn of the same pair and kind covers. Statements under one overturn make one fact, combined as `claim` combines them today (the earliest note and quote, the latest `last`), marked with that overturn. Statements after the last overturn make one current fact. A kind can so have several facts on one pair, in the order of their first statements.
2. **Through merges and links.** A same-as or a link re-keys statements and overturns to the survivor, and rule 1 is applied to the joined pair: a statement falls under an overturn when it was made before it, under whichever ids. An overturn applies only when both ends are in the cast and differ, as a relation does; one on a pair with nothing to cover marks nothing, and still covers statements a later merge brings that were made before it.
3. **The cut-off.** The fold sees only the events up to the cut, so a fact whose overturn lies past the cut is current.

- [ ] **Step 1: Write the failing tests** in `tests/fold.rs`, with `keeper_with`'s chapters and quotes:
  - `overturn_marks_the_fact_stated_before_it`: chapter 1 `enemy` c1 to c2, chapter 3 `Overturned` `enemy` written c2 to c1: `fold_book(.., 3)` has one `enemy` fact with `overturned == Some(Overturn { at: chapter 3, note, quote: "quote <seq>" })`; `fold_book(.., 2)` has it with `overturned == None`.
  - `statement_after_an_overturn_makes_a_new_current_fact`: `ally` in 1, its overturn in 2, `ally` again in 3: two `ally` facts, the first marked (first and last chapter 1), the second current (first chapter 3).
  - `overturn_of_a_directed_kind_keeps_its_direction`: `serves` c1 to c2 and `serves` c2 to c1; an overturn of `serves` c1 to c2 marks the first only.
  - `overturn_covers_a_statement_a_later_same_as_brings`: `ally` c1 and c3 in chapter 1, `ally` c1 and c2 in 2, its overturn in 3, `SameAs { entity: c2, other: c3 }` in 4: one `ally` fact, first 1, last 2, marked at 3.
  - `statement_joined_after_the_overturn_stays_current`: the overturn of `ally` c1 and c2 in 3, `ally` c1 and c3 in 4, the same-as in 5: a marked fact and a current one (first chapter 4).
  - `overturn_without_a_statement_marks_nothing`.
  - `overturn_reaches_an_earlier_book_through_a_link` (with `fold_linked`): book 1 states `ally` a and b; book 2 introduces a' and b' in chapter 1, linked to a and b from chapter 1, and overturns `ally` a' and b' in chapter 2: the series cast after book 2's chapter 2 has book 1's fact marked; after chapter 1, unmarked.
  - `link_after_an_overturn_still_brings_the_fact_under_it`: as above with the overturn in book 2's chapter 1 and the links holding from chapter 2: after chapter 2 the fact is marked.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test fold`. Expected: fails to compile (no `Overturn`).
- [ ] **Step 3: Implement** the rules in `fold.rs`. Holding each pair's statements and overturns uncombined and building the facts in `finish` is one way; any way that passes the tests and keeps `merge`'s survivor rules will do.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test fold --test series`, `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "The fold marks a fact that a later chapter overturns".

### Task 3: Two guards in the first call's check (O12, O13)

**Files:**
- Modify: `crates/haiyaku/src/extract/check.rs` (`same_as` 273-284, the name rule 241-247)
- Test: `crates/haiyaku/tests/check.rs`

**Interfaces:**
- Produces: the rejection reason `related_pair`. Nothing else changes shape.

Rules:
1. **O12.** `same_as` refuses with `related_pair` when `roster.events()` holds a `Relation` between the two in either direction, both ends resolved to their survivors first. Relations are checked before same-as (`check` 119-126), so the same reply's kept relations are in `roster.events()` by then; keep that order. An overturned relation counts too: the two were still two people.
2. **O13.** A name the character does not carry yet becomes `Named` when its rank is above `roster.shown_rank`, whether it came as the item's `name` or as an alias; otherwise `Alias`. Names are taken in the item's order, so a later alias is weighed against the shown name an earlier one raised.

- [ ] **Step 1: Write the failing tests** in `tests/check.rs`:
  - `same_as_between_characters_related_earlier_is_refused`: chapter 1 relates c1 and c2 (`serves`); chapter 2's same-as of the two is rejected as `related_pair`, both stay in the cast, and no `SameAs` event is written.
  - `same_as_between_characters_related_in_the_same_reply_is_refused`: one reply relates c1 and c2 and makes them the same: the relation is kept, the same-as rejected as `related_pair`.
  - `same_as_through_a_merged_id_is_refused`: c1 relates to c2, c3 merged into c2; a same-as of c1 and c3 is rejected.
  - `alias_outranking_the_shown_name_is_named`: c1 shown as "the tall woman"; a reply listing c1 under that name with the alias "Mrs. Marsh" (surname labelled) writes `Named { c1, "Mrs. Marsh" }`, not `Alias`; a second alias of lower or equal rank in the same item stays `Alias`.
  - `lesser_name_becomes_an_alias` and `name_outranking_the_shown_name_is_named` (321, 349) pass unchanged.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test check`. Expected: the four new tests fail.
- [ ] **Step 3: Implement** both rules.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "Refuse a same-as between related characters, and show a fuller name given as an alias".

### Task 4: The second call's prompt, schema and check

**Files:**
- Create: `crates/haiyaku/tests/golden/first_call.txt`
- Modify: `crates/haiyaku/src/extract/prompt.rs` (`PROMPT_VERSION` 12, `relations` 101-118, a new `reveal_prompt`), `crates/haiyaku/src/extract/check.rs` (new `reveal_schema`, `check_reveals`), `crates/haiyaku/src/extract/roster.rs` (`held`), `crates/haiyaku/src/extract/mod.rs` (re-exports)
- Test: `crates/haiyaku/tests/extract.rs`, `tests/check.rs`

**Interfaces:**
- Consumes: `EventBody::Overturned` (Task 1); `Roster`, `ChapterText`, `Outcome`, `Rejected`, `SYSTEM`, `schema()`, `prompt()`.
- Produces, in `haiyaku::extract`:

```rust
pub const PROMPT_VERSION: u32 = 5;
pub fn reveal_prompt(roster: &Roster, chapter: &ChapterText) -> String;
pub fn reveal_schema() -> serde_json::Value;
pub fn check_reveals(roster: &mut Roster, chapter: &ChapterText, reply: &serde_json::Value) -> Outcome;
```

- Produces, in `roster.rs`: `pub(crate) fn held(&self) -> Vec<Held>` with `pub(crate) struct Held<'a> { from: &'a str, to: &'a str, kind: RelationKind, note: &'a str }` (ends resolved to survivors): the relations stated since the last overturn of their pair and kind, each fact (from, kind, to) once with its first note, in the order first stated, exactly as `relations()` dedupes and orders them today, so a roster with no overturn lists the same rows as before. Both prompts list from it and `check_reveals` tests against it, so the list the model sees and the check agree.
- Rejection reasons: `not_recorded`, `self_overturn`.

Rules:
1. **The lists.** `relations()` in `prompt.rs` lists from `Roster::held`, so a relation an overturn covers leaves both prompts' lists from the next chapter on, and comes back when a later chapter states it again.
2. **`reveal_prompt`**, in this order: `Chapter <number>: <title>`, a blank line and the chapter's text, as `prompt()` writes them; `The reader has just finished this chapter.`; the characters list and the relations list in `prompt()`'s headers and rows, for `roster.listed(chapter)` taken after the first call's check (so it holds the characters that call introduced); then the task and the rules. The task is the design's wording, word for word ("The second call", item 5). The rules: use only this chapter's text read to its end, and nothing the model may know of the book or the series beyond it; refs from the list; `kind` from `RelationKind::ALL`, with `mentor`, `serves` and `owns` running from the mentor, the servant and the owner, as `rules()` words it; `note` says in a few words what the chapter shows; `evidence` copied as `rules()` words that rule; empty lists when there is nothing.
3. **`reveal_schema`**: an object with two required arrays, `relations` (the item `schema()` uses for relations) and `overturned` (`from`, `to`, `kind`, `note`, `evidence`, typed as in the relation item), built with `check.rs`'s `object` and `array` helpers.
4. **`check_reveals`** takes `listed` from the roster when it starts, as `check` does; a ref that is not listed is `unknown_ref` (there are no handles). It checks `overturned` first, then `relations`, so an overturn never covers a relation of its own reply. An overturned item: malformed, `unknown_ref`, `self_overturn` (both ends one character after merges), `bad_kind`, `not_recorded` (`held` has no relation of that kind between the two: in the given direction for a directed kind, either way for the others), then the evidence, as `relation` checks it. A kept one is pushed as `Overturned` with the reply's order of ends and the note made one line as `relation` does. A relation item goes through `relation` unchanged. A key other than `relations` and `overturned` is ignored; a missing one is empty, as `items` treats it.

- [ ] **Step 1: Pin the first call before changing anything.** In `tests/extract.rs`, `first_call_is_byte_for_byte_prompt_4`: for chapter 2 of `keeper_epub` with a roster checked from `keeper_replies()`'s chapter 1 reply (no alias in it outranks a shown name), the text `SYSTEM`, a line `---`, `prompt(..)`, a line `---`, `schema().to_string()` equals `include_str!("golden/first_call.txt")`. Write the file from the code as it stands at the start of this task (a throwaway `#[ignore]` test that writes it, deleted before the commit). Run it: it passes.
- [ ] **Step 2: Write the failing tests.**
  - `tests/extract.rs`: `reveal_prompt_has_the_chapter_the_lists_and_the_task` (starts with `prompt()`'s heading and text; the line about the reader follows; the three task paragraphs stand word for word; a character the chapter's first reply introduced is listed); `reveal_prompt_names_every_relation_kind`; `overturned_relation_leaves_both_lists` (chapter 1 `enemy` c1 to c2, chapter 2 its overturn: neither prompt for chapter 3 has an `enemy` row for the two; after chapter 3 states it again, both prompts for chapter 4 do).
  - `tests/check.rs`: `reveal_schema_requires_both_lists` (both keys required, the overturned item's fields required, `kind` an enum of `RelationKind::ALL`); `overturn_of_a_held_relation_is_kept` (`enemy` c1 to c2 held, an overturn written c2 to c1 with evidence in the chapter: one `Overturned` event, `kept == 1`); `overturn_of_an_unheld_relation_is_rejected` (`not_recorded`); `second_overturn_is_rejected` (`not_recorded`); `overturn_of_a_directed_kind_needs_its_direction`; `overturn_needs_its_own_evidence`; `overturn_of_a_merged_pair_is_rejected` (`self_overturn`); `unlisted_ref_in_a_reveal_is_unknown`; `overturn_cannot_cover_its_own_replys_relation` (a reply that adds `ally` c1 and c2 and overturns `ally` c1 and c2 keeps the relation and rejects the overturn as `not_recorded`); `reveal_reply_with_the_first_calls_shape_keeps_its_relations` (a reply with `characters` and `relations` and no `overturned`: the relations are checked, the characters ignored).
- [ ] **Step 3: Run** `cargo test -p haiyaku --test extract --test check`. Expected: fails to compile.
- [ ] **Step 4: Implement** `Roster::held`, the lists, `reveal_prompt`, `reveal_schema`, `check_reveals` and the re-exports; raise `PROMPT_VERSION`.
- [ ] **Step 5: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass, `first_call_is_byte_for_byte_prompt_4` among them.
- [ ] **Step 6: Commit**: "The second call's prompt, schema and check, and a golden test that pins the first call".

### Task 5: Extraction makes the second call

**Files:**
- Modify: `crates/haiyaku/src/extract/run.rs` (the chapter loop 68-99, `put_rejected` 197-202), `crates/haiyaku/tests/common/epub.rs` (`keeper_replies`)
- Test: `crates/haiyaku/tests/extract.rs`, `tests/cli.rs` (every replay sequence and reply count: `extract.rs` 257-486, `cli.rs` 182-752)

**Interfaces:**
- Consumes: `reveal_prompt`, `reveal_schema`, `check_reveals` (Task 4); `answer`, `Cache`, `Request`.
- Produces: `extract_book`'s signature unchanged. `keeper_replies()` returns six replies, each chapter's reply followed by its second reply (`{"relations":[],"overturned":[]}` for all three), so a `Replay` over it serves a whole book; tests that need a reveal write their own.

Rules:
1. **The loop.** For a chapter with text: the first call and `check` as now, then `reveal_prompt(&roster, chapter)`, a `Request` with `SYSTEM`, that prompt and `reveal_schema()` (its `to_string()` built once beside `schema_text`), `answer` with the same cache, and `check_reveals`. The second call has its own cache key because its prompt and schema differ. A chapter with no text makes neither call, as now. A second call that fails after `call_twice`'s retry fails the run as a failed first call does; the first call's reply is cached by then.
2. **The chapter's row** sums both calls: `seconds`, `kept`, `rejected`, and `cost_usd` (`None` only when both are `None`).
3. **The rejected file** holds both calls' rejections; each entry gains `"call": "first"` or `"call": "reveal"`. The `notes` counts are the first call's, as now.

- [ ] **Step 1: Write the failing tests** in `tests/extract.rs`, with a backend written in the test that records each `Request` and answers from a list (fixed `seconds` 1.5 and `cost_usd` 0.25 a call):
  - `each_chapter_makes_a_second_call`: three chapters, six calls; every even call has `system == SYSTEM`, `schema == reveal_schema()`, and a `user` that starts with the chapter's heading and holds the line about the reader.
  - `second_call_events_follow_the_first_calls`: chapter 2's second reply overturns chapter 1's `owns` (evidence from chapter 2's text): the log's chapter-2 events are the first call's, then the `Overturned`, `seq` running on.
  - `chapter_row_sums_both_calls`: each chapter's row has `seconds == 3.0`, `cost_usd == Some(0.5)`, `kept` and `rejected` the sums.
  - `rejected_file_holds_both_calls`: an overturn of an unheld relation in chapter 2's second reply is in `rejected/<entry>.json` with `"call": "reveal"` and `"reason": "not_recorded"`.
  - `resume_after_a_failed_second_call`: a `Replay` holding three replies fails at chapter 2's second call; a second run with the cache answers chapter 1's two calls and chapter 2's first from the cache and asks only for chapter 2's second and chapter 3's two.
  - Update the replay sequences and counts the brief lists (`second_run_calls_nothing` expects six reply files; `cli.rs:325-337` deletes the files of the chapter it re-asks).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test extract --test cli`. Expected: the new tests fail.
- [ ] **Step 3: Implement** the loop, the row, the rejected file and `keeper_replies`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`, then the 1.91 check. Expected: all pass.
- [ ] **Step 5: Commit**: "Extraction asks each chapter a second time for what it reveals and overturns".

### Task 6: `show` and `--html` mark an overturned fact, and the documents

**Files:**
- Modify: `crates/haiyaku/src/bin/haiyaku/show.rs` (`draw` 67-97), `view.rs` (`step` 22-67), `view.html` (edges 269-296, `factLine` 318-326, the panel 361-370, `legend` 391-401), `docs/status.md`, `docs/superpowers/specs/2026-11-21-haiyaku-library-design.md` (the event table under "The log"), `README.md` where it says something this plan makes false
- Test: `crates/haiyaku/tests/cli.rs`

**Interfaces:**
- Consumes: `fold::{Fact, Overturn}` (Task 2).

Rules:
1. **`show`** prints a marked fact as it prints any fact, followed by ` [overturned in ch <n>: <note>]`. Unmarked lines are unchanged (`cli.rs:550-591` pin them).
2. **The page's data**: a fact gains `overturned`: `null`, or `{chapter, note, quote}`.
3. **The page**: a pair's line is styled from its newest current fact; a pair whose facts are all marked is drawn faint, with a line pattern none of the kind groups (`view.html:52-58`) uses and an "overturned" label: never by colour alone. The panel lists a marked fact with its kind struck through, "overturned in ch <n>", and the overturn's quote under the fact's own. The legend explains the mark.
4. **Documents.** `docs/status.md`: O6, O11, O12 and O13 closed with a line each on what answers them; Where it stands gains the second call and the overturned event; Next is plan 3's linking. The library design's event table gains `overturned{from, to, kind, note}` with a pointer to the reveals design.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs`, with a book written by `store::write_book` straight into the test's database as `html_data_cannot_close_its_script` does, its log gaining an `Overturned` of a chapter-1 relation in chapter 3:
  - `show_marks_an_overturned_fact`: `show` prints that fact's line ending in `[overturned in ch 3: <note>]`.
  - `show_before_the_overturn_prints_the_fact_unmarked`: `show --at 2` prints the line without the mark.
  - `show_html_carries_the_overturn`: in the page's data, step 3's fact has `overturned.chapter == 3` and step 2's has `overturned == null`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: the three new tests fail.
- [ ] **Step 3: Implement** `draw`, `step` and the page. Write the page for that book to `<workspace>/overturn.html` (not committed); the controller looks at it with headless `google-chrome-stable --screenshot` at steps 2 and 3 and records the check in the task ledger.
- [ ] **Step 4: The documents**, per rule 4.
- [ ] **Step 5: Run** `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"`, then the 1.91 check. Expected: every step passes up to `cargo deny` (not installed on the desktop, O5).
- [ ] **Step 6: Commit**: "show and its page mark a fact a later chapter overturned".

### Acceptance run, before the PR

The controller runs this, not an implementer, and only after the owner agrees to the spend: up to about 6.5 USD on the Claude login.

- [ ] **Step 1:** `cargo build --release -p haiyaku` in the worktree, with `CARGO_TARGET_DIR` unset.
- [ ] **Step 2:** Extract Sorcerer's Stone, English entries 4-20 and Korean entries 5-21 (`~/temp/haiyaku-corpus/`), with `--db spike/out/reveals/haiyaku.db` (gitignored), the usual reply cache, and each run's stderr kept in the scratchpad.
- [ ] **Step 3:** Pass means all of the design's acceptance checks:
  1. plan 2's checks: the rejected share at most 3% in English and 5% in Korean; no two characters with one name; the boy met by description at 5 and named at 6; Hagrid's creatures by `owns`; the Korean mother's family relations at 6;
  2. both reveals at 17 in both languages, and neither before;
  3. where the log relates Snape to Voldemort as allies before chapter 17, that relation is overturned at 17; no overturn before 17 touches a relation of Snape's to Harry or to Voldemort;
  4. Quirrell and Voldemort stay two characters in both languages;
  5. in English, Mrs. Weasley is shown by a name, not a description.
- [ ] **Step 4:** A section at the top of `docs/notes.md` with each run's time, kept, rejected and cost and each check's outcome: numbers and character names only, no line of the books. A failed check is a finding against the task that owns it; fix it there, and ask the owner before each rerun.
- [ ] **Step 5:** `/code-review high reveals`, the fixes, a review of the fixes, then the push and the PR (title under 255 characters, one line per paragraph, no commit list).
