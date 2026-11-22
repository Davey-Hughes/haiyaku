# haiyaku core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The `haiyaku` crate's database core: the schema and its migrations, writing and reading a book's event log, the fold that answers "what does the reader know here", series with reading orders and links, and sidecar export and import. No model, no EPUB reader, no binary yet.

**Architecture:** One library crate in a new workspace at the repository root. Every function takes a caller's `rusqlite::Connection`. The fold is a pure in-memory step over events read from the database, cut at a chapter in SQL. A sidecar is a SQLite file with the same schema, so import reads it with the same code that reads the main database.

**Tech Stack:** Rust 2024, rusqlite 0.43, sha2 0.11, tempfile 3 (tests only).

**Spec:** `docs/superpowers/specs/2026-11-21-haiyaku-library-design.md`. This plan covers its Storage, The fold, the series half of Series and linking (tables, not the link step), and Tests and gates. Two later plans cover extraction (book, llm, extract, `extract`/`show`) and the link step (name parts from the model, link rules, `series`/`link`/`export`, the English and Korean acceptance runs).

## Global Constraints

- Toolchain `1.100.0` (`rust-toolchain.toml`, components rustfmt and clippy, **no wasm target**: the fold runs on taiyaku's server). Edition 2024, `rust-version = "1.91"` (taiyaku's, since taiyaku builds haiyaku), license `GPL-3.0-or-later`, repository `https://forge.daveynet.xyz/davey/haiyaku`.
- rustfmt: `edition = "2024"`, `max_width = 120`, `use_small_heuristics = "Max"`.
- Lints as taiyaku's root `Cargo.toml` lines 21-27: `unsafe_code = "forbid"`, `missing_debug_implementations = "warn"`, clippy `todo` and `dbg_macro` `"warn"`. Clippy runs with `-D warnings` on `--all-features` and again on the default features.
- rusqlite `0.43`, features `["backup"]`; `bundled` only through haiyaku's own `bundled` feature, which is in `default`. taiyaku depends with `default-features = false` and brings its own `bundled`. No other runtime dependency in this plan besides `sha2 = "0.11"` (taiyaku's version). No error crate: `Error` is hand-written, as taiyaku's are.
- Every table is named `haiyaku_...`. haiyaku never reads or writes `PRAGMA user_version`: it is taiyaku's.
- Fixtures are invented. No text from the owner's books in the repository, in a test, or in a commit message.
- Comments say why (the reason, the invariant, what breaks), never history or measurements (CLAUDE.md).
- The shell in this environment is zsh with `noclobber`: `>` will not overwrite, use `>|`. `grep` is ugrep (use `/usr/bin/grep`), `ls` is eza.
- Work on a branch cut from `main`; never push to `main`. The whole plan is one PR.

## Review Focus

1. A reader in a document the book does not have (another edition's path, a typo) gets `Error::NotFound`, never a guessed chapter. Test: Task 5, `unknown_document_is_not_found`.
2. A sidecar for another edition of the book (hash differs) is refused and the database is unchanged. Test: Task 6, `import_refuses_other_hash`.
3. Re-extracting a book that is already linked leaves links that name ids which no longer mean the same character; they must go, while the book stays in its orders. Test: Task 5, `rewriting_a_book_drops_its_links`.
4. A caller whose connection has foreign keys off would get unchecked rows; haiyaku refuses to write instead. Test: Task 3, `write_requires_foreign_keys`.
5. A write that fails halfway leaves the previous log intact. Test: Task 3, `failed_rewrite_keeps_old_log`.

---

### Task 1: Workspace, crate and gates

**Files:**
- Create: `Cargo.toml` (workspace), `crates/haiyaku/Cargo.toml`, `crates/haiyaku/src/lib.rs`, `crates/haiyaku/src/error.rs`, `deny.toml`, `scripts/check-all.sh`, `.forgejo/workflows/ci.yml`, `.forgejo/workflows/advisories.yml`
- Create or update: `rust-toolchain.toml`, `rustfmt.toml` (values in Global Constraints; the spike branch has versions with a wasm target to drop)

**Interfaces:**
- Produces: `haiyaku::Error` and `haiyaku::Result<T> = std::result::Result<T, Error>`:

```rust
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    NotFound(String),          // what was looked up, e.g. "book <sha256>"
    Invalid(String),           // input that breaks a rule this crate states
    ForeignKeysOff,
    SchemaTooNew { found: u32, supported: u32 },
    HashMismatch { expected: String, found: String },
}
```

  with `Display`, `std::error::Error` (`source` for the two wrapped kinds), and `From<rusqlite::Error>`, `From<std::io::Error>`.

- [ ] **Step 1: Root `Cargo.toml`.** `resolver = "3"`, `members = ["crates/haiyaku"]`, `exclude = ["spike"]` (the spike keeps its own lock and none of the gates), `[workspace.package]` and `[workspace.lints]` per Global Constraints.
- [ ] **Step 2: Crate manifest.** `crates/haiyaku/Cargo.toml`: package fields from the workspace, `[lints] workspace = true`, features `default = ["bundled"]`, `bundled = ["rusqlite/bundled"]`; dependencies rusqlite and sha2; dev-dependency `tempfile = "3"`.
- [ ] **Step 3: `lib.rs` and `error.rs`.** Crate docs (one paragraph: what haiyaku is, that every function takes the caller's connection), `mod error; pub use error::{Error, Result};`.
- [ ] **Step 4: `deny.toml`.** taiyaku's `deny.toml` with its header rewritten for haiyaku and the `paste` ignore removed: `[graph] all-features = true`, `[advisories] unsound = "all"`.
- [ ] **Step 5: `scripts/check-all.sh`.** `set -euo pipefail`, then in order: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo clippy --workspace --all-targets -- -D warnings`; `RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc --workspace --no-deps --all-features`; `cargo test --workspace --all-features`; `cargo deny --locked check advisories`. A header says it mirrors CI and must change with it.
- [ ] **Step 6: CI.** `.forgejo/workflows/ci.yml`: one job modelled on taiyaku's `lint` job (taiyaku `.forgejo/workflows/ci.yml` lines 56-118): `runs-on: docker`, `container.image: rust:1-trixie`, `timeout-minutes: 10`, taiyaku's checkout step copied as it is (this runner has no JS actions inside a container), then the first five commands of `check-all.sh`, and taiyaku's "Check at the declared Rust version" step (its lines 119-166), since taiyaku relies on the declared 1.91. No build volumes: the runner only admits `taiyaku-ci-*` volume names. `advisories.yml`: taiyaku's `.forgejo/workflows/advisories.yml` with names changed.
- [ ] **Step 7: Verify.** Run `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"`, with the session scratchpad directory in place of `<scratchpad>`, and read the tail of the log. Expected: `exit 0`, 0 tests run.
- [ ] **Step 8: Commit** `Cargo.toml Cargo.lock crates/ deny.toml scripts/ .forgejo/ rust-toolchain.toml rustfmt.toml`: "A workspace with the haiyaku crate and taiyaku's gates".

### Task 2: Schema and migrations

**Files:**
- Create: `crates/haiyaku/src/store/mod.rs`, `crates/haiyaku/src/store/migrate.rs`, `crates/haiyaku/src/store/schema/001_initial.sql`
- Test: `crates/haiyaku/tests/migrate.rs`, plus unit tests in `migrate.rs` for the private runner

**Interfaces:**
- Produces: `pub const SCHEMA_VERSION: u32 = 1;` `pub fn migrate(conn: &rusqlite::Connection) -> Result<()>` `pub fn schema_version(conn: &rusqlite::Connection) -> Result<Option<u32>>` (all re-exported from `haiyaku::store`). Private: `fn migrate_with(conn: &Connection, migrations: &[&str]) -> Result<()>`, migration `i` being version `i + 1`.

The version lives in `haiyaku_schema(version INTEGER NOT NULL)`, one row. `migrate` creates it if missing, applies each pending migration and the version bump in one transaction, and returns `Error::SchemaTooNew` when the stored version is above `SCHEMA_VERSION`. It works with foreign keys on or off; it never turns them on or off itself.

`001_initial.sql` creates these tables. Every `book` column references `haiyaku_book(sha256)`; child rows reference their event with `ON DELETE CASCADE`; `entity`/`from_entity`/`to_entity`/`other` reference `haiyaku_entity(book, id)`, and so do a link's `(later_book, later_entity)` and `(earlier_book, earlier_entity)`.

| Table | Columns | Keys and checks |
| --- | --- | --- |
| `haiyaku_book` | `sha256 TEXT`, `title`, `language`, `backend`, `model TEXT`, `prompt_version INTEGER`, `extracted_at TEXT` (RFC 3339, UTC) | PK `sha256`; a text of 64 lowercase hex digits (`typeof(sha256) = 'text' AND length(sha256) = 64 AND instr(sha256, char(0)) = 0 AND sha256 NOT GLOB '*[^0-9a-f]*'`); all NOT NULL |
| `haiyaku_chapter` | `book`, `number INTEGER` (1-based, reading order), `toc_entry INTEGER` (index in the table of contents), `title TEXT`, `seconds REAL`, `cost_usd REAL NULL`, `kept INTEGER`, `rejected INTEGER` | PK `(book, number)` |
| `haiyaku_document` | `book`, `ordinal INTEGER` (spine order from 0), `path TEXT`, `chapter INTEGER NULL` | PK `(book, ordinal)`; UNIQUE `(book, path)`; FK `(book, chapter)` → chapter |
| `haiyaku_entity` | `book`, `id TEXT`, `kind TEXT` | PK `(book, id)`; `kind IN ('character')` |
| `haiyaku_event` | `book`, `seq INTEGER`, `chapter INTEGER`, `document INTEGER`, `block INTEGER`, `type TEXT`, `quote TEXT` | PK `(book, seq)`; FK chapter and document; `type IN ('introduced','named','alias','appeared','relation','same_as')` |
| `haiyaku_name` | `book`, `seq`, `entity`, `text TEXT`, `given TEXT NULL`, `surname TEXT NULL`, `title TEXT NULL`, `title_sex TEXT NULL` | PK `(book, seq)`; `title_sex IN ('male','female')` |
| `haiyaku_appearance` | `book`, `seq`, `entity` | PK `(book, seq)` |
| `haiyaku_relation` | `book`, `seq`, `from_entity`, `to_entity`, `kind TEXT`, `note TEXT` | PK `(book, seq)`; `from_entity <> to_entity`; `kind` in the eleven of Task 3 |
| `haiyaku_same_as` | `book`, `seq`, `entity`, `other` | PK `(book, seq)`; `entity <> other` |
| `haiyaku_series` | `name TEXT`, `title TEXT` | PK `name` |
| `haiyaku_order` | `series`, `name TEXT`, `rank INTEGER` (0 = the default) | PK `(series, name)`; UNIQUE `(series, rank)`; FK series ON DELETE CASCADE |
| `haiyaku_order_book` | `series`, `order_name`, `position INTEGER`, `book` | PK `(series, order_name, position)`; UNIQUE `(series, order_name, book)`; FK order ON DELETE CASCADE |
| `haiyaku_link` | `series`, `order_name`, `later_book`, `later_entity`, `earlier_book`, `earlier_entity`, `method TEXT`, `quote TEXT NULL`, `chapter INTEGER`, `document INTEGER`, `block INTEGER` | PK `(series, order_name, later_book, later_entity)`; `method IN ('name','model')`; `method = 'name' OR quote IS NOT NULL`; FK order ON DELETE CASCADE |

`chapter`, `document` and `block` on a link are the later book's position from which it holds.

- [ ] **Step 1: Write the failing tests** in `tests/migrate.rs`: `migrate_creates_every_table` (the 13 tables above and `haiyaku_schema` in `sqlite_master`, `schema_version` = `Some(1)`); `migrate_twice_is_a_no_op`; `migrate_leaves_user_version_alone` (set `user_version` to 31 first, still 31 after); `newer_schema_is_refused` (set the stored version to 2: `Err(Error::SchemaTooNew { found: 2, supported: 1 })`); `fresh_connection_has_no_version` (`None`). Unit test in `migrate.rs`: `pending_migrations_only` (`migrate_with` two synthetic migrations, then three: only the third runs, version 3).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test migrate`. Expected: fails to compile (`store` missing).
- [ ] **Step 3: Implement** `migrate.rs` and the SQL (`include_str!`).
- [ ] **Step 4: Run** `cargo test -p haiyaku`. Expected: all pass.
- [ ] **Step 5: Commit**: "The schema and its migrations".

### Task 3: The log types, and writing and reading a book

**Files:**
- Create: `crates/haiyaku/src/log.rs`, `crates/haiyaku/src/store/book.rs`, `crates/haiyaku/tests/common/mod.rs` (fixtures)
- Test: `crates/haiyaku/tests/book.rs`

**Interfaces:**
- Consumes: `migrate` (Task 2).
- Produces, in `haiyaku::log` (all `#[derive(Debug, Clone, PartialEq)]`; enums also `Copy, Eq` where they hold no strings):

```rust
pub struct BookLog { pub book: BookRecord, pub chapters: Vec<Chapter>, pub documents: Vec<Document>,
                     pub entities: Vec<Entity>, pub events: Vec<Event> }
pub struct BookRecord { pub sha256: String, pub title: String, pub language: String, pub backend: String,
                        pub model: String, pub prompt_version: u32, pub extracted_at: String }
pub struct Chapter { pub number: u32, pub toc_entry: u32, pub title: String, pub seconds: f64,
                     pub cost_usd: Option<f64>, pub kept: u32, pub rejected: u32 }
pub struct Document { pub ordinal: u32, pub path: String, pub chapter: Option<u32> }
pub struct Entity { pub id: String, pub kind: EntityKind }
pub enum EntityKind { Character }
pub struct Position { pub document: u32, pub block: u32 }
pub struct Event { pub seq: u32, pub chapter: u32, pub at: Position, pub quote: String, pub body: EventBody }
pub enum EventBody {
    Introduced { entity: String, name: Name },
    Named { entity: String, name: Name },
    Alias { entity: String, name: Name },
    Appeared { entity: String },
    Relation { from: String, to: String, kind: RelationKind, note: String },
    SameAs { entity: String, other: String },
}
pub struct Name { pub text: String, pub given: Option<String>, pub surname: Option<String>,
                  pub title: Option<String>, pub title_sex: Option<Sex> }
pub enum Sex { Male, Female }
pub enum RelationKind { Family, Romance, Friend, Ally, Rival, Enemy, Mentor, Serves, Colleague, Acquaintance, Owns }
```

  `RelationKind::directed(self) -> bool` is true for `Mentor`, `Serves` and `Owns` (from → to); `as_str`/`parse` use the snake_case names stored in SQL. `Name::rank(&self) -> u8`: 2 with both `given` and `surname`, 1 with one of them, 0 with neither (a description).
- Produces, in `haiyaku::store`: `pub fn write_book(conn: &mut Connection, log: &BookLog) -> Result<()>`, `pub fn read_book(conn: &Connection, sha256: &str) -> Result<BookLog>`, `pub fn read_events(conn: &Connection, sha256: &str, up_to_chapter: Option<u32>) -> Result<Vec<Event>>` (events with `chapter <= up_to_chapter`, all when `None`, in `seq` order).
- Produces, in `tests/common/mod.rs`: `pub fn keeper() -> BookLog` and `pub fn keepers_return() -> BookLog` (Task 5 uses the second), `pub fn db() -> rusqlite::Connection` (in-memory, `PRAGMA foreign_keys = ON`, migrated).

`write_book` rules, each with its own error:
1. `PRAGMA foreign_keys` off: `Error::ForeignKeysOff`, nothing written.
2. Event `seq` must run 1, 2, 3… in vector order, and event chapters must never decrease: otherwise `Error::Invalid`.
3. One transaction. If the book exists, its old rows go in the order the foreign keys allow: its links in every order (their ids may no longer mean the same character), then its events (child rows cascade), documents, entities and chapters. Its `haiyaku_book` row is updated in place, so its place in reading orders survives.
4. A foreign-key failure rolls everything back and returns `Error::Sqlite`.

`read_book` returns `Error::NotFound("book <sha256>")` for an unknown book, and `Error::Invalid` for an event row without its child row.

The fixture `keeper()`, invented: a book "The Lighthouse Keeper", `sha256` = 64 × `'a'`, 3 chapters; documents `0 "front.xhtml"` (no chapter), `1 "c1.xhtml"`, `2 "c2.xhtml"`, `3 "c3.xhtml"` (chapters 1-3), `4 "back.xhtml"` (no chapter). Events:

| seq | ch | Event |
| --- | --- | --- |
| 1 | 1 | `Introduced ada`, name "the keeper" (rank 0) |
| 2 | 1 | `Introduced pip`, name "Pip" (given) |
| 3 | 1 | `Relation ada → pip, Owns, "her gull"` |
| 4 | 2 | `Named ada`, "Ada Marsh" (given + surname) |
| 5 | 2 | `Introduced stranger`, "the stranger" |
| 6 | 2 | `Relation ada – stranger, Acquaintance, "met on the jetty"` |
| 7 | 3 | `Introduced tom`, "Tom Reed" |
| 8 | 3 | `SameAs tom, stranger` |
| 9 | 3 | `Relation ada – tom, Acquaintance, "old friends"` |
| 10 | 3 | `Alias ada`, "Mrs. Marsh" (surname, title "Mrs.", female) |

Quotes are short invented sentences that contain the names.

- [ ] **Step 1: Write the failing tests** in `tests/book.rs`: `round_trip` (write `keeper()`, `read_book` equals it); `read_events_cuts_at_chapter` (`Some(1)` gives seq 1-3, `None` gives 1-10); `unknown_book_is_not_found`; `write_requires_foreign_keys` (a migrated connection with foreign keys off: `ForeignKeysOff`, then `read_book` is `NotFound`); `relation_to_unknown_entity_is_refused` (a relation to id `"ghost"`: `Error::Sqlite`, book absent); `failed_rewrite_keeps_old_log` (write `keeper()`, then a copy with the `"ghost"` relation: `Err`, and `read_book` still equals `keeper()`); `seq_out_of_order_is_invalid`; `chapters_going_back_is_invalid`; `name_rank` (the three ranks from the fixture's names).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test book`. Expected: fails to compile.
- [ ] **Step 3: Implement** `log.rs` and `store/book.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku`. Expected: all pass.
- [ ] **Step 5: Commit**: "Write and read a book's event log".

### Task 4: The fold

**Files:**
- Create: `crates/haiyaku/src/fold.rs`
- Test: `crates/haiyaku/tests/fold.rs`

**Interfaces:**
- Consumes: `log::*` (Task 3).
- Produces, in `haiyaku::fold`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityKey { pub book: String, pub id: String }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterRef { pub book: String, pub chapter: u32 }
pub struct Cast { pub characters: Vec<Character>, pub relations: Vec<Relation> }
pub struct Character { pub key: EntityKey, pub name: String, pub aliases: Vec<String>,
                       pub first: ChapterRef, pub last: ChapterRef, pub merged: Vec<EntityKey> }
pub struct Relation { pub a: EntityKey, pub b: EntityKey, pub facts: Vec<Fact> }
pub struct Fact { pub kind: RelationKind, pub from: Option<EntityKey>, pub note: String, pub quote: String,
                  pub first: ChapterRef, pub last: ChapterRef }

#[derive(Debug, Default)]
pub struct Folder { /* private */ }
impl Folder {
    pub fn new() -> Self;
    /// Books must be started in reading order; chapters within a book in order.
    pub fn start_book(&mut self, book: &str);
    pub fn apply(&mut self, event: &Event);
    pub fn merge(&mut self, survivor: &EntityKey, other: &EntityKey);
    pub fn finish(self) -> Cast;
}
pub fn fold_book(log: &BookLog, finished: u32) -> Cast; // events with chapter <= finished
```

The fold's rules, which the spec leaves to this plan:
1. **Ids resolve through merges**, transitively: an event naming an id merged earlier applies to its survivor.
2. **Introduced** creates the character; **every event that names it** widens `last`.
3. **Display name**: among the names from the character's own `Introduced` and `Named` events (not aliases, not names merged in), the highest `Name::rank`; on a tie, the earliest. So "the keeper" becomes "Ada Marsh" after chapter 2, and a survivor keeps its own name after a merge.
4. **Aliases**: every other name it has carried (its `Alias` names, its outranked own names, every name of a character merged into it), without duplicates, without the display name, in the order first seen.
5. **Relations**: one `Relation` per unordered pair, `a < b` by `EntityKey`. One fact per kind, and per direction for directed kinds (`from` is `Some` only for those). A repeated fact keeps its first note and quote and widens `first`/`last`.
6. **Merge** (a `SameAs`, or `Folder::merge` for a link): the survivor is `entity` / the earlier book's character. It takes the other's names as aliases, its `merged` list plus its key, the earlier `first` and the later `last`, and its relations, merging facts by rule 5. A pair that becomes the survivor with itself is dropped.
7. **Output order**: characters by `first` (book in fold order, then chapter), then by the `seq` of their introduction; relations by the first of their facts, then by `(a, b)`.

- [ ] **Step 1: Write the failing tests** in `tests/fold.rs` on `keeper()`: `nothing_before_chapter_one` (`fold_book(&k, 0)` is empty); `description_until_named` (after 1: ada's name "the keeper"; after 2: "Ada Marsh" with alias "the keeper"); `owns_is_directed` (the ada–pip fact has `from == Some(ada)`); `stranger_separate_until_revealed` (after 2: stranger and its relation present, no tom); `same_as_merges` (after 3: one character "Tom Reed", `merged == [stranger]`, alias "the stranger", `first` = chapter 2; the ada–tom pair holds one `Acquaintance` fact with note "met on the jetty", `first` 2, `last` 3); `alias_does_not_rename` (after 3: ada still "Ada Marsh", aliases "the keeper", "Mrs. Marsh"); `merge_drops_self_pair` (a hand-built log where the two merged characters had a relation: no pair remains).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test fold`. Expected: fails to compile.
- [ ] **Step 3: Implement** `fold.rs`. The spike's `spike/src/events.rs` (`Graph::fold`, lines 116-233) shows the same idea for one book; rewrite it under the rules above rather than copying it.
- [ ] **Step 4: Run** `cargo test -p haiyaku`. Expected: all pass.
- [ ] **Step 5: Commit**: "Fold a book's events up to a finished chapter".

### Task 5: Series, orders, links and the cast at a position

**Files:**
- Create: `crates/haiyaku/src/store/series.rs`, `crates/haiyaku/src/cast.rs`
- Test: `crates/haiyaku/tests/series.rs`

**Interfaces:**
- Consumes: `write_book`, `read_events` (Task 3); `Folder`, `EntityKey`, `Cast` (Task 4).
- Produces, in `haiyaku::store`:

```rust
pub struct Series { pub name: String, pub title: String, pub orders: Vec<Order> } // orders[0] is the default
pub struct Order { pub name: String, pub books: Vec<String> }                     // sha256s in reading order
pub struct Link { pub later: EntityKey, pub earlier: EntityKey, pub method: LinkMethod, pub chapter: u32, pub at: Position }
pub enum LinkMethod { Name, Model { quote: String } }
pub fn put_series(conn: &mut Connection, series: &Series) -> Result<()>;
pub fn read_series(conn: &Connection, name: &str) -> Result<Series>;
pub fn put_links(conn: &mut Connection, series: &str, order: &str, links: &[Link]) -> Result<()>;
pub fn read_links(conn: &Connection, series: &str, order: &str) -> Result<Vec<Link>>;
pub fn finished_chapters(conn: &Connection, book: &str, document: &str) -> Result<u32>;
```

- Produces, in `haiyaku::cast`: `pub enum Scope { Book, Series { series: String, order: Option<String> } }` and `pub fn cast_at(conn: &Connection, book: &str, document: &str, scope: &Scope) -> Result<Cast>` (re-exported at the crate root).

Rules:
1. `put_series` replaces the series' orders and so, by cascade, all its links: links are only valid for the book lists they were made against. An order naming a book not in the database is `Error::NotFound`; an order listing a book twice, or a series with no order, is `Error::Invalid`.
2. `put_links` replaces one order's links. A link whose later book does not come after its earlier book in that order is `Error::Invalid`.
3. `finished_chapters` = the number of the book's chapters whose last document's ordinal is below the reader's document's ordinal. An unknown document is `Error::NotFound("document <path> in book <sha256>")`.
4. `cast_at` with `Scope::Book`: `read_events(book, Some(finished))` through a `Folder`. With `Scope::Series`: the named order, or rank 0 when `None`; every book before `book` in it whole, then `book` up to `finished`. A link applies after the last event of its chapter in the later book (as `Folder::merge(earlier, later)`), and only if that chapter is finished; under the chapter cut-off this shows exactly what applying it at its block would. A book not in the order is `Error::NotFound`.

Fixture `keepers_return()`: book "The Keeper's Return", `sha256` = 64 × `'b'`, same document layout, 3 chapters: ch 1 introduces `ada` "Ada" (given) and `sailor` "the old sailor"; ch 2 relation ada – sailor `Ally`; ch 3 `Alias sailor "Tom"`. Series `"keepers"`, order `"published"` = [a…, b…]. Links: `b:ada → a:ada` by `Name` at chapter 1; `b:sailor → a:tom` by `Model` with a quote, at chapter 3.

- [ ] **Step 1: Write the failing tests** in `tests/series.rs`: `finished_chapters_by_document` (front.xhtml 0, c1 0, c2 1, c3 2, back 3); `unknown_document_is_not_found`; `book_scope_ignores_links` (book b at c3.xhtml: ada and sailor only, keys in book b); `series_scope_links_at_introduction` (book b at c2.xhtml: one Ada with key in book a, `merged` holding b:ada, plus all of book a); `link_waits_for_its_chapter` (book b at c3.xhtml: sailor separate; at back.xhtml: merged into Tom Reed); `front_matter_shows_only_earlier_books` (book b at front.xhtml: exactly book a's cast after chapter 3); `rewriting_a_book_drops_its_links` (rewrite `keepers_return()`: `read_links` empty, `read_series` unchanged); `link_against_order_is_invalid` (in order [a, b], a link whose later book is a and earlier book is b); `put_series_refuses_unknown_book`; `default_order_is_rank_zero` (a second order "solo" = [b]: `order: None` uses "published").
- [ ] **Step 2: Run** `cargo test -p haiyaku --test series`. Expected: fails to compile.
- [ ] **Step 3: Implement** `store/series.rs` and `cast.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku`. Expected: all pass.
- [ ] **Step 5: Commit**: "Series, reading orders, links, and the cast at a position".

### Task 6: Sidecar export and import, then the status

**Files:**
- Create: `crates/haiyaku/src/store/sidecar.rs`
- Modify: `docs/status.md` (Where it stands, Next, O3)
- Test: `crates/haiyaku/tests/sidecar.rs`

**Interfaces:**
- Consumes: `migrate`, `schema_version` (Task 2); `write_book`, `read_book` (Task 3).
- Produces, in `haiyaku::store`: `pub fn sha256_file(path: &Path) -> Result<String>` (lowercase hex); `pub fn export_book(conn: &Connection, sha256: &str, path: &Path) -> Result<()>`; `pub fn import_book(conn: &mut Connection, sidecar: &Path, expected_sha256: &str) -> Result<()>`.

Rules:
1. `export_book` writes a new SQLite file at `SCHEMA_VERSION` holding that book's rows only (no series, no links): a temporary file in the target's directory, renamed over `path` at the end, so a reader never sees half a sidecar and re-exporting replaces the old one. An unknown book is `NotFound` and creates no file.
2. `import_book` never writes to the sidecar: it may sit on a read-only share. It opens the file read-only; a newer schema is `SchemaTooNew`; an older one is copied into an in-memory database with rusqlite's backup API and migrated there. The sidecar must hold exactly one book, whose `sha256` equals `expected_sha256`, else `HashMismatch`. Then `read_book` from the sidecar and `write_book` into `conn`, so the same rules and the single transaction apply.

- [ ] **Step 1: Write the failing tests** in `tests/sidecar.rs` (files under a `tempfile::TempDir`): `export_then_import` (export `keeper()`, import into a fresh `db()`, `read_book` equal); `import_refuses_other_hash` (expected 64 × `'c'`: `HashMismatch`, target has no book); `import_refuses_newer_schema` (bump the sidecar's `haiyaku_schema` to 2 first); `import_leaves_sidecar_untouched` (bytes before = bytes after); `export_replaces_existing_file`; `export_unknown_book_writes_nothing`; `sha256_of_abc` (a file holding `abc` hashes to `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test sidecar`. Expected: fails to compile.
- [ ] **Step 3: Implement** `store/sidecar.rs`.
- [ ] **Step 4: Run** `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"` (as in Task 1). Expected: `exit 0`.
- [ ] **Step 5: Update `docs/status.md`.** Where it stands: the core crate exists (schema, store, fold, sidecars), with no extraction yet. Next: drop plan 1; plan 2 (extraction) comes first.
- [ ] **Step 6: Commit**: "Sidecar export and import".

After the last task, run `/code-review` on the branch at a level sized to the diff, fix what it finds, review the fixes, then push and open the PR (title under 255 characters, one line per paragraph, no commit list). Tell the owner that `main` can now require the CI check.
