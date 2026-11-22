# haiyaku extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `haiyaku extract book.epub --chapters 3-19` reads an EPUB, asks a model about one chapter at a time, checks every item of the reply against the chapter's text and writes the book's log to the database; `haiyaku show book.epub --at 5` prints the cast a reader has after chapter 5.

**Architecture:** Three library modules behind a new `extract` feature (`book`, `llm`, `extract`) and a binary behind `cli`, so taiyaku's `default-features = false` build gains no dependency. `extract_book` goes from a `Book` and a `Backend` to a `BookLog` and touches no database; the caller hands the result to `store::write_book`, so the database never holds half a book. Replies are cached under a hash of the prompt, and that cache is how an interrupted run resumes.

**Tech Stack:** Rust 2024; new optional dependencies zip 9, serde_json 1, ureq 3 (plain HTTP), html-escape 0.2, clap 4.

**Spec:** `docs/superpowers/specs/2026-11-21-haiyaku-library-design.md`. This plan covers The text model, what extraction writes of The log, Extraction, Model backends, and `extract` and `show` of The binary. `docs/notes.md` ("Spike review, 2026-11-21") lists the spike's defects 1 to 6 that this plan must not repeat; the table below says where each is answered. Plan 3 covers the link step, `series`, `link`, `export` and the link acceptance runs.

**Lands as two PRs.** Part A (Tasks 1 to 5), "the book reader and the model backends", lands with this plan (PR #6). Part B (Tasks 6 to 10, with 7, 8 and 9 each in two halves, a and b, and the acceptance run), "extraction and the haiyaku binary", lands as its own PR, cut from `main` after that merge.

## Global Constraints

- Toolchain `1.100.0`, edition 2024, `rust-version = "1.91"` (taiyaku's floor; do not raise it. If a new dependency needs a newer compiler, take its newest version that passes `cargo +1.91.0 check --workspace --all-targets --all-features --locked` and say so in the PR). rustfmt and lints as they stand in the root `Cargo.toml` and `rustfmt.toml`.
- New dependencies, all `optional = true`: `zip = { version = "9.1.3", default-features = false, features = ["deflate"] }` (taiyaku's version), `serde_json = "1.0.151"` (taiyaku's), `ureq = { version = "3.5", default-features = false, features = ["json"] }`, `html-escape = "0.2"`, `clap = { version = "4.6", features = ["derive"] }`. Each enters the manifest with the first task that uses it (html-escape in Task 2, zip in Task 3, serde_json and ureq in Task 4, clap in Task 9a): cargo warns about a dependency nothing uses. No XML crate, no serde derive, no time crate, no error crate, no async runtime. rusqlite stays `0.43`.
- Features: `default = ["bundled", "cli"]`, `bundled = ["rusqlite/bundled"]`, `extract = ["dep:zip", "dep:serde_json", "dep:ureq", "dep:html-escape"]`, `cli = ["extract", "dep:clap"]`, which is `["extract"]` until Task 9a adds clap. With no features the crate is what it is on `main` today.
- The library reads no environment variable, picks no path of its own and renders nothing. Paths, XDG variables, `OLLAMA_HOST` and every piece of output or markup belong to the binary (`src/bin/haiyaku/`). The library hands out the cast as data (`fold::Cast`) and a consumer decides how it is drawn.
- No test calls a model or the network beyond `127.0.0.1`: tests use `llm::Replay`, a fake `claude` script, or a listener on a local port. An implementer never runs the binary on the owner's books and never calls a real backend; the acceptance run at the end is the controller's, with the owner's go-ahead.
- Fixtures are invented: the test EPUBs are built by test code from invented text, and recorded replies are invented too. No text from the owner's books in the repository, in a test, or in a commit message.
- The program assigns every id (`c1`, `c2`, ... in order of introduction within the book). Nothing the model calls a character reaches the log.
- Comments say why (the reason, the invariant, what breaks), never history or measurements (CLAUDE.md). A rule's reason below may become a comment; its story (what the spike did, how a defect was found, as in Task 6 rule 2, Task 7a rule 3 and Task 8a's rule on evidence) may not. The spike's files are named below as places to read, not to copy.
- The shell here is zsh with `noclobber`: `>` will not overwrite, use `>|`. `grep` is ugrep (use `/usr/bin/grep`), `ls` is eza. Tests that start the binary set `HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` to places inside their `TempDir`.
- Work on a branch cut from `main`; never push to `main`. Each part ends with `/code-review` on the branch at a level sized to the diff, the fixes, a review of the fixes, then the push and the PR (title under 255 characters, one line per paragraph, no commit list).

## Where the spike's defects are answered

| Defect (docs/notes.md) | Answer | Test |
| --- | --- | --- |
| 1. A new character with a name already in the roster is folded into the earlier one | A reply's new character joins a known one only when given name and surname are both labelled and both equal (Task 7a, rule 3) | `new_character_with_a_known_alias_stays_new` |
| 2. An empty name, or one inside another word, passes as "in the text" | Names are matched on whole words (Task 6) | `name_needs_whole_words`, `empty_name_is_absent` |
| 3. An id that was merged away can be introduced again | Ids come from a counter and never repeat, and a reference means a known character only when this chapter's prompt listed its id; a merged-away id is never listed (Task 7a, rules 1 and 10) | `unlisted_id_is_a_handle`, `listed_is_the_named_and_the_recent` |
| 4. Named entities outside a short list stay as written | Text runs are decoded with `html-escape` (Task 2) | `named_entities_decoded` |
| 5. A run made with `--after` cannot be shown alone | Extraction takes no earlier book (approach A); there is no `--after` | none needed |
| 6. `OLLAMA_HOST` taken as a URL; temperature 0 makes a retry repeat | Task 4: the host forms, and no backend sends a temperature | `ollama_host_forms`, `ollama_round_trip`, `openai_round_trip` |

## Not in this plan

- Comparing names across books, the link step, `series`, `link`, `export`, and `show` for a series: plan 3.
- From subcheck's backend: presets, backends named in a settings file, any command as a backend, waiting for a local server to come up, parallel jobs. They matter for local models and wait for O2. Extraction is one call after another by design: each chapter's prompt needs the chapters before it.
- TLS and an API-key backend (O4). An `https://` URL is refused with a message that says so.
- A list of pronouns in code. The prompt forbids them as aliases; the spec replaces the spike's English word lists with the model's labels.

## Review Focus

1. A table of contents with several entries in one file, or one that points backwards in the spine: chapters must stay runs of whole documents in spine order. Test: Task 3, `toc_entries_inside_one_document_fold_into_the_first`.
2. A document that is not UTF-8 (another encoding, or encrypted): the book is refused with the document's path, never read as garbage that no quote can match. Test: Task 3, `non_utf8_document_is_refused`.
3. A chapter range that is reversed, starts at 0 or runs past the table of contents: refused before any model call, with the valid range in the message. Test: Task 3, `chapter_range_out_of_bounds_is_invalid`; Task 9a, `bad_range_calls_no_model`.
4. A run that dies at chapter 12 (the model fails, the user presses Ctrl-C): the database is as it was, and the next run pays only for the chapters left. Tests: Task 8b, `resume_after_failure`; Task 9b, `failed_extract_leaves_database_unchanged`.
5. A chosen entry with no text (a part title page that is one image): no model call for it, a chapter with nothing kept, and the run goes on. Test: Task 8b, `chapter_without_text_calls_no_model`.

---

# Part A: the book reader and the model backends

### Task 1: Features, dependencies, two error kinds and the no-features gate

**Files:**
- Modify: `crates/haiyaku/Cargo.toml` (features, with `cli = ["extract"]`, and `[[bin]]`; no dependency yet), `crates/haiyaku/src/error.rs`, `crates/haiyaku/src/lib.rs`, `scripts/check-all.sh`, `.forgejo/workflows/ci.yml`
- Create: `crates/haiyaku/src/book/mod.rs`, `crates/haiyaku/src/llm/mod.rs`, `crates/haiyaku/src/extract/mod.rs` (module docs only), `crates/haiyaku/src/bin/haiyaku/main.rs` (prints the version and exits 0; Task 9 replaces it)

**Interfaces:**
- Produces: `Error::Epub(String)` (what is wrong with the file, and where) and `Error::Model(String)` (a backend that failed, timed out, or answered with something that is not the JSON asked for), both with `Display`; the modules `haiyaku::book`, `haiyaku::llm`, `haiyaku::extract` under `#[cfg(feature = "extract")]`; a binary `haiyaku` with `required-features = ["cli"]` at `src/bin/haiyaku/main.rs`.

- [ ] **Step 1: Write the failing test** in `src/error.rs`'s unit tests: `epub_and_model_errors_say_what_failed` (`Error::Epub("no table of contents".into()).to_string()` contains `"EPUB"` and the message; `Error::Model("timed out after 600 s".into()).to_string()` contains `"model"` and the message; `source()` is `None` for both).
- [ ] **Step 2: Run** `cargo test -p haiyaku --lib error`. Expected: fails to compile.
- [ ] **Step 3: Manifest, error kinds, empty modules, placeholder binary.** Values in Global Constraints. The two variants are not behind a feature: an enum whose shape depends on features breaks a caller's `match`.
- [ ] **Step 4: The gate.** In `scripts/check-all.sh` and the same place in `ci.yml`, after the default-features clippy: `cargo clippy --workspace --all-targets --no-default-features --features bundled -- -D warnings`; after the all-features tests: `cargo test --workspace --no-default-features --features bundled`. This is taiyaku's build of the crate; without it a `use` of a gated module from ungated code passes every other gate. If the CI run comes near its `timeout-minutes: 10` with the larger build, raise it to 20 in this commit.
- [ ] **Step 5: Verify.** `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"` and read the tail of the log. Expected: every step passes up to `cargo deny`, which is not installed on the desktop (O5). Then `cargo tree -p haiyaku --no-default-features -e normal` lists only rusqlite, sha2 and what they bring.
- [ ] **Step 6: Commit**: "An extract feature and a cli feature, off in taiyaku's build, with a gate for the build without them".

### Task 2: The text model, and XHTML to blocks

**Files:**
- Create: `crates/haiyaku/src/book/xhtml.rs`
- Modify: `crates/haiyaku/src/book/mod.rs`
- Test: `crates/haiyaku/tests/blocks.rs` (first line after the doc comment: `#![cfg(feature = "extract")]`, as in every test file this plan adds)

**Interfaces:**
- Produces, in `haiyaku::book`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block { pub kind: BlockKind, pub text: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind { Heading, Paragraph }
/// The blocks of one XHTML document, in document order. Never fails: markup it cannot follow costs structure, not text.
pub fn blocks(xhtml: &str) -> Vec<Block>;
```

- Produces, for Task 3 only (`pub(super)` in `xhtml.rs`): the tag scanner that `blocks` is built on, yielding open tags (lowercased name without its namespace prefix, attributes), close tags and decoded text runs. Task 3 reads `container.xml`, the OPF, the NCX and the nav document with it, so the crate has one way to read markup.

The walker's rules:
1. Text counts only inside `body`. The contents of `head`, `script`, `style`, `rt` and `rp` are skipped, as are comments, processing instructions and doctype.
2. A block ends at every open or close of `p div br hr h1 h2 h3 h4 h5 h6 li tr blockquote section article dd dt table td th caption figcaption aside nav header footer pre ul ol dl figure main` (32 tags). A block whose text began inside `h1` to `h6` is a `Heading`; every other is a `Paragraph`. A heading ends at its close, where another heading opens, where a block opens once the heading has text, or where a block that was open when the heading opened closes, whether or not the heading has text (a `div` around a heading left unclosed: an HTML parser ends the heading there too), so an unclosed heading ends before the paragraphs and the loose text after it. A block that opens inside the heading before it has any text (a `div` or `p` around its text) is part of it, and its close leaves the heading open; `br` and `hr` do not end a heading.
3. Text: character references decoded (`html_escape::decode_html_entities`), U+00AD dropped, U+00A0 turned into a space, whitespace collapsed to single spaces and trimmed. A block left empty is dropped.
4. The scanner is forgiving on purpose: an unclosed tag, a close with no open, a bare `&` or `<` in text cost nothing but structure. A strict parser dropped whole chapters of real books in the spike (`spike/src/book.rs:235-280` is the scanner to rewrite, with rules 1 to 3 in place of its own).

- [ ] **Step 1: Write the failing tests** in `tests/blocks.rs`, each on a literal document:
  - `paragraphs_and_headings`: `<body><h1>One</h1><p>First.</p><p>Second.</p></body>` gives `[Heading "One", Paragraph "First.", Paragraph "Second."]`.
  - `head_script_style_and_ruby_readings_are_skipped`: `<head><title>T</title></head><body><p>漢<rt>かん</rt>字<rp>(</rp></p><script>x</script><style>y</style></body>` gives `[Paragraph "漢字"]`.
  - `soft_hyphens_dropped_and_space_collapsed`: `<body><p>ex&shy;am&#173;ple  \n text&nbsp;here</p></body>` gives `"example text here"`.
  - `named_entities_decoded`: `caf&eacute; &ldquo;na&iuml;ve&rdquo; &amp; &#233; &#xE9;` gives `café “naïve” & é é`.
  - `unknown_entity_and_bare_ampersand_stay`: `&bogus; R&D` comes back unchanged.
  - `malformed_markup_keeps_text`: `<body><p>One<p>Two</div><b>Three` gives three paragraphs `One`, `Two`, `Three`.
  - `prefixed_and_uppercase_tags`: `<xhtml:BODY><xhtml:P>Hi</xhtml:P></xhtml:BODY>` gives `[Paragraph "Hi"]`.
  - `br_splits_a_paragraph`: `<p>a<br/>b</p>` gives two paragraphs.
  - `comments_are_skipped`: `<body><!-- <p>no</p> --><p>yes</p></body>` gives `[Paragraph "yes"]`.
  - `text_outside_body_is_dropped`, `nested_inline_markup_stays_one_block` (`<p>a <em>b <b>c</b></em> d</p>` gives `"a b c d"`), `heading_inside_div_is_a_heading`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test blocks`. Expected: fails to compile.
- [ ] **Step 3: Implement** `book/xhtml.rs` and the two types in `book/mod.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test blocks`. Expected: all pass.
- [ ] **Step 5: Commit**: "XHTML to heading and paragraph blocks, forgiving of markup that does not parse".

### Task 3: Opening an EPUB: spine, table of contents, chapters

**Files:**
- Create: `crates/haiyaku/src/book/epub.rs`, `crates/haiyaku/tests/common/epub.rs`
- Modify: `crates/haiyaku/src/book/mod.rs`, `crates/haiyaku/tests/common/mod.rs` (`#[cfg(feature = "extract")] pub mod epub;`)
- Test: `crates/haiyaku/tests/epub.rs`

**Interfaces:**
- Consumes: `book::blocks` and the scanner (Task 2); `store::sha256_file`; `log::{Document, Position}`; `Error::{Epub, Invalid}`.
- Produces, in `haiyaku::book`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub sha256: String,                 // of the file, as store::sha256_file gives it
    pub title: String,                  // first dc:title; the file stem when the package has none
    pub language: String,               // first dc:language; "und" when the package has none
    pub documents: Vec<SpineDocument>,  // spine order; the index is the document's ordinal
    pub toc: Vec<TocEntry>,             // entry n is toc[n - 1]
}
#[derive(Debug, Clone, PartialEq)]
pub struct SpineDocument { pub path: String, pub blocks: Vec<Block> }
#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry { pub label: String, pub document: u32 }
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterText {
    pub number: u32,                    // from 1, within the chosen range
    pub toc_entry: u32,                 // from 1, within Book::toc
    pub title: String,
    pub documents: std::ops::Range<u32>,
    pub blocks: Vec<PlacedBlock>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedBlock { pub at: Position, pub text: String }

impl Book {
    pub fn open(path: &Path) -> Result<Book>;
    /// The chapters for table-of-contents entries first..=last.
    pub fn chapters(&self, first: u32, last: u32) -> Result<Vec<ChapterText>>;
    /// One row per spine document, for BookLog::documents.
    pub fn document_rows(&self, chapters: &[ChapterText]) -> Vec<Document>;
}
impl ChapterText {
    /// What the model reads: the blocks' texts joined by "\n".
    pub fn text(&self) -> String;
}
```

- Produces, in `tests/common/epub.rs`: `pub fn epub(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf` (writes a zip whose first entry is a stored `mimetype`, then the files given); `pub fn keeper_epub(dir: &Path) -> PathBuf`; `pub fn chapter_text(number: u32, document: u32, blocks: &[&str]) -> ChapterText` (one document, blocks numbered from 0, `toc_entry = number`).

The fixture `keeper_epub`: title "The Keeper", language `en`, package at `OEBPS/content.opf`, an NCX, spine `front.xhtml`, `c1.xhtml`, `c2.xhtml`, `c3.xhtml`, `back.xhtml`, table of contents "Chapter 1", "Chapter 2", "Chapter 3", "About the author". Three invented chapters of four to eight paragraphs that tell what `tests/common/mod.rs`'s `keeper()` records: in chapter 1 a lighthouse keeper, called only "the keeper", and her dog Pip; in chapter 2 she is named Ada Marsh and meets a stranger on the jetty; in chapter 3 the stranger turns out to be Tom Reed and someone addresses her as "Mrs. Marsh". Tasks 7 to 9 quote this text in recorded replies, so write it once here.

Rules:
1. `META-INF/container.xml` names the package. A manifest `href` resolves against the package's directory, percent-decoded, with `.` and `..` folded: that archive path is `SpineDocument::path`, the same form taiyaku-read's `BlockId::document` uses, so a caller can pass its own path to `cast_at`. An href that climbs above the archive's root, has a bad percent escape, or is empty or a fragment alone names no file, and its manifest item or table entry is dropped; only a table-of-contents target that is empty or a fragment alone names the table's own file.
2. The spine is every `itemref` whose item exists in the manifest and in the archive, `linear="no"` included: a reader can be in such a document, and a document the log does not know is `Error::NotFound` for `cast_at`.
3. A spine item is read by its media type, lower-cased and without parameters, and a leading byte-order mark is dropped. A document of a text type (`application/xhtml+xml`, `text/html`, `image/svg+xml`) that is not UTF-8 refuses the book, `Error::Epub` naming the path. A binary type (`image/*` other than `image/svg+xml`, `audio/*`, `video/*`, `font/*`, `application/font-*`, `application/x-font-*`, `application/vnd.ms-opentype`, `application/octet-stream`, `application/pdf`) is not read. Any other type, or none, is read as markup when it is UTF-8. A binary item, and an item of any other type that is not UTF-8, keeps its place in the spine with no blocks.
4. The tables of contents are tried in this order: the NCX named by `spine@toc`, the manifest item of media type `application/x-dtbncx+xml` (compared as rule 3 compares a spine item's), the manifest item with the `nav` property (its `nav` element of type `toc`). One missing from the archive, of bytes that are bad or not UTF-8, or keeping no entry gives way to the next; when none is left, `Error::Epub` names each. A failed read of the archive, while opening it, finding an entry or reading one, is the book's fault, `Error::Epub`, when its kind says the bytes are bad (`InvalidData`, `InvalidInput`, `UnexpectedEof`), and the disk's, `Error::Io`, otherwise, as is a file that cannot be opened; an `Error::Io` ends the open, and no table of contents is tried after it.
5. Its entries are taken flat, in the order written. An entry's target resolves against the table's own directory, percent-decoded, fragment dropped. An entry whose target is not in the spine is dropped. Of the rest, the longest run whose documents rise strictly through the spine is kept, and of two runs as long, the one that keeps the earlier entries: an entry left out is a section of a chapter, or out of order, as a contents page at the end of the spine listed first is. An entry's number is its place among the kept ones, from 1.
6. Entry `n` covers the documents from its own up to the next entry's, and the last entry runs to the end of the spine. `chapters(first, last)` needs `1 <= first <= last <= toc.len()`, else `Error::Invalid` with the valid range in the message (`"1-4"` for the fixture).
7. `document_rows` gives `chapter: Some(number)` to the documents of a chapter and `None` to every other, with `ordinal` from 0 and `path` as in rule 1.
8. Not a zip, no `container.xml`, no package, an empty spine: `Error::Epub` saying which.

- [ ] **Step 1: Write the fixture helpers and the failing tests** in `tests/epub.rs` (files under a `tempfile::TempDir`):
  - `opens_the_keeper`: title, language, `sha256 == sha256_file(path)`, document paths `OEBPS/front.xhtml` ... `OEBPS/back.xhtml` in spine order, `toc` = the four labels at documents 1, 2, 3, 4.
  - `chapters_are_runs_of_whole_documents`: `chapters(1, 3)` has numbers and `toc_entry` 1, 2, 3 and `documents` `1..2`, `2..3`, `3..4`; every block's `at.document` lies in its chapter's range and `at.block` counts from 0 in each document; `text()` has one line per block.
  - `document_rows_mark_front_and_back_matter`: chapters `[None, Some(1), Some(2), Some(3), None]`.
  - `a_later_range_renumbers_chapters`: `chapters(2, 3)`: number 1 has `toc_entry` 2; rows `[None, None, Some(1), Some(2), None]`.
  - `a_range_that_stops_early_leaves_the_rest_in_no_chapter`: `chapters(1, 2)`: rows `[None, Some(1), Some(2), None, None]` (the next book's excerpt is such a tail).
  - `chapter_range_out_of_bounds_is_invalid`: `(0, 2)`, `(3, 2)`, `(2, 9)` are `Error::Invalid` whose message contains `1-4`.
  - `last_entry_runs_to_the_end_of_the_spine`: `chapters(4, 4)` covers `4..5`.
  - `nav_toc_without_ncx`: the same book with an EPUB 3 nav document and no NCX opens to the same `toc`.
  - `toc_entries_inside_one_document_fold_into_the_first`: entries `c1.xhtml`, `c1.xhtml#s2`, `c2.xhtml`, then `front.xhtml`: `toc` has two entries, labelled as the first and the third.
  - `chapter_of_several_documents`: spine `c1a`, `c1b`, `c2` with entries at `c1a` and `c2`: chapter 1 covers `0..2` and its blocks restart at block 0 in `c1b`.
  - `percent_encoded_and_nested_paths`: manifest `href="text/chapter%20one.xhtml"` gives the path `OEBPS/text/chapter one.xhtml`.
  - `non_utf8_document_is_refused`: a document of bytes `FF FE 00 3C` is `Error::Epub` whose message contains its path.
  - `missing_title_and_language_have_defaults` (file stem, `und`); `not_an_epub` (a text file; a zip without `container.xml`); `no_table_of_contents_is_an_error`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test epub`. Expected: fails to compile.
- [ ] **Step 3: Implement** `book/epub.rs`. `spike/src/book.rs:13-147` shows the same walk through container, package and table of contents; rewrite it under the rules above.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`. Expected: all pass.
- [ ] **Step 5: Commit**: "Open an EPUB as spine documents, a table of contents and chapters of whole documents".

### Task 4: Backends: the trait, replay, Ollama and OpenAI-compatible servers

**Files:**
- Create: `crates/haiyaku/src/llm/replay.rs`, `crates/haiyaku/src/llm/http.rs`
- Modify: `crates/haiyaku/src/llm/mod.rs`, `crates/haiyaku/tests/common/mod.rs`
- Test: `crates/haiyaku/tests/llm.rs`

**Interfaces:**
- Produces, in `haiyaku::llm`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> { pub system: &'a str, pub user: &'a str, pub schema: &'a serde_json::Value }
#[derive(Debug, Clone, PartialEq)]
pub struct Reply { pub json: serde_json::Value, pub raw: String, pub seconds: f64, pub cost_usd: Option<f64> }
pub trait Backend: std::fmt::Debug {
    fn name(&self) -> &str;    // "claude", "ollama", "openai", "replay": BookRecord::backend
    fn model(&self) -> &str;   // BookRecord::model; "none" for replay
    fn endpoint(&self) -> &str { "" }   // a server's base URL, a replay's directory: part of a cached reply's key
    /// One JSON object that should fit the schema. Any failure is Error::Model.
    fn call(&self, request: &Request<'_>) -> Result<Reply>;
}
#[derive(Debug, Clone)]
pub struct Settings { pub timeout: Duration, pub ollama_host: Option<String>, pub claude_program: PathBuf }
impl Default for Settings;   // 600 s, None, "claude"
/// "claude", "claude:<model>", "ollama:<model>", "openai:<base url>#<model>", "replay:<dir>".
pub fn backend(spec: &str, settings: &Settings) -> Result<Box<dyn Backend>>;
pub fn ollama_url(host: Option<&str>) -> Result<String>;
#[derive(Debug)]
pub struct Replay { /* private */ }
impl Replay {
    pub fn new(replies: Vec<String>) -> Replay;        // raw reply texts, given out in order
    pub fn from_dir(dir: &Path) -> Result<Replay>;     // the directory's files, sorted by name
    pub fn remaining(&self) -> usize;
}
```

- Produces, in `tests/common/mod.rs`: `pub fn serve_once(status: u16, body: &str) -> (String, std::thread::JoinHandle<String>)`: a listener on `127.0.0.1:0` that answers one request and hands back the request it read (request line, headers and body); the first value is its `http://127.0.0.1:<port>`.

Rules:
1. `backend`: `claude` means model `sonnet`. An unknown scheme, an empty model, or an `openai:` spec without `#<model>` is `Error::Invalid`. `claude` specs return `Error::Invalid("the claude backend is not built yet")` until Task 5.
2. `ollama_url`: `None` or empty is `http://127.0.0.1:11434`; a value without a scheme gets `http://`, and `:11434` when it has no port; an `http://` URL with no port gets `:80`, as Ollama reads it; an IPv6 address without brackets (`::1`) is bracketed; a trailing `/` goes. `OLLAMA_HOST`'s usual values are `host` or `host:port`. An `https://` URL, here or in an `openai:` spec, is `Error::Invalid` saying TLS is not built in.
3. Ollama: `POST <url>/api/chat` with `model`, the two messages, `stream: false`, `format` = the schema, `think: false`, `options: { num_ctx: 32768, num_predict: 8192 }`. The reply is `message.content` parsed as JSON; `done_reason == "length"` is an error saying the reply was cut off.
4. OpenAI-compatible: `POST <base>/chat/completions` with `model`, the two messages, `max_tokens: 8192`, `response_format: { type: "json_schema", json_schema: { name: "events", schema, strict: true } }`. The reply is `choices[0].message.content` parsed as JSON; `finish_reason == "length"` is the same error.
5. Neither request carries `temperature`: with greedy decoding a reply that looped loops again on the retry.
6. Both use `Settings::timeout` for the whole call. A refused connection, a timeout, a status that is not 2xx, a body that is not the expected shape, content that is not a JSON object: each is `Error::Model` with the backend, the URL and the cause. `seconds` is the wall-clock time of the call; `cost_usd` is `None`.
7. `Replay` gives the next text parsed as JSON with `seconds: 0.0`, `cost_usd: None`; a text that is not a JSON object, or running out, is `Error::Model` (running out says how many calls it had answered). `Replay::from_dir` takes a directory's files in name order, skipping names that start with `.` or end with `~`.

- [ ] **Step 1: Write the failing tests** in `tests/llm.rs`:
  - `replay_returns_replies_in_order_then_fails`; `replay_from_dir_sorts_by_file_name` (files `002.json`, `001.json` written in that order); `replay_reply_that_is_not_json_is_a_model_error` (and the next call returns the reply after it).
  - `spec_strings`: `ollama:gemma` is `("ollama", "gemma")`; `openai:http://h:8000/v1/#m` is `("openai", "m")`; `replay:<dir>` is `("replay", "none")`; `""`, `gpt`, `ollama:`, `openai:http://h`, `openai:https://h#m` are `Error::Invalid`.
  - `ollama_host_forms`: `None` and `""` give the default; `192.168.1.5` gives `http://192.168.1.5:11434`; `0.0.0.0:8080` gives `http://0.0.0.0:8080`; `http://box:1234/` gives `http://box:1234`; `https://box` is `Error::Invalid`.
  - `ollama_round_trip` (against `serve_once`, the host passed through `Settings::ollama_host`): the request line is `POST /api/chat`; its body has `"stream": false`, `format` equal to the schema and no `temperature` key; the reply's `json` is the object inside `message.content`.
  - `ollama_cut_off_reply_is_an_error` (`done_reason: "length"`; the message contains `cut off`).
  - `openai_round_trip`: the request line ends `/chat/completions`; the body has `response_format.json_schema.strict == true` and no `temperature`; `openai_cut_off_reply_is_an_error`.
  - `http_error_status_is_a_model_error` (`serve_once(500, "boom")`); `refused_connection_is_a_model_error` (a port nothing listens on); `silent_server_times_out` (a listener that accepts and never answers, `timeout` 200 ms: `Error::Model` within 5 s).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test llm`. Expected: fails to compile.
- [ ] **Step 3: Implement** `llm/mod.rs`, `llm/replay.rs`, `llm/http.rs`. `spike/src/llm.rs:68-118` has the two request shapes as measured.
- [ ] **Step 4: Run** `cargo test -p haiyaku --all-features`. Expected: all pass.
- [ ] **Step 5: Commit**: "Model backends: a trait, recorded replies, Ollama and OpenAI-compatible servers".

### Task 5: The Claude CLI backend

**Files:**
- Create: `crates/haiyaku/src/llm/claude.rs`
- Modify: `crates/haiyaku/src/llm/mod.rs` (`backend` builds it)
- Test: `crates/haiyaku/tests/claude.rs`

**Interfaces:**
- Consumes: `Backend`, `Request`, `Reply`, `Settings` (Task 4).
- Produces: `backend("claude", ..)` and `backend("claude:<model>", ..)` return a backend named `claude` that runs `Settings::claude_program`.

Rules:
1. The command is the spike's, flag for flag (`spike/src/llm.rs:120-134`): `-p --model <model> --output-format json --tools "" --setting-sources "" --disable-slash-commands --strict-mcp-config --no-session-persistence --system-prompt <system> --json-schema <schema>`, working directory `/`, the user prompt on stdin. Those flags keep the user's settings, tools and session files out of the call.
2. Stdin is written from its own thread: a chapter is larger than a pipe's buffer, and the child may write before it has read everything.
3. After `Settings::timeout` the started process is killed and reaped, and the call is `Error::Model` saying it timed out. Standard library only. The child stays in haiyaku's process group, so a Ctrl-C at the terminal reaches it; a timeout reaches only the process started, so a wrapper script must `exec` the program. A program that exits by itself before the deadline has at least a second for its pipes to close.
4. Stdout is one JSON object. `is_error == true`, a non-zero exit (the message carries the end of stderr), or output that is not JSON is `Error::Model`. The reply is `structured_output`, else `result` parsed as JSON. `cost_usd` is `total_cost_usd`.
5. A program that cannot be started is `Error::Model` naming it. A relative `claude_program` of more than one component is made absolute from the working directory when the backend is built, since the program runs in `/`; a bare name is found on the `PATH`.

- [ ] **Step 1: Write the failing tests** in `tests/claude.rs`. Each writes an executable `sh` script into a `TempDir` and passes it as `claude_program`:
  - `structured_output_is_the_reply`: the script prints `{"is_error":false,"structured_output":{"characters":[]},"total_cost_usd":0.5}`; `json == {"characters":[]}`, `cost_usd == Some(0.5)`.
  - `result_string_is_parsed_without_structured_output`; `is_error_fails`; `output_that_is_not_json_fails`.
  - `nonzero_exit_fails_with_stderr`: the script writes `no login` to stderr and exits 3; the message contains `no login`.
  - `prompt_on_stdin_and_flags_passed`: the script saves its arguments and its stdin to files; the arguments hold `--model sonnet`, `--json-schema` followed by the schema's JSON, `--system-prompt` followed by the system text; stdin equals the user prompt.
  - `large_prompt_does_not_deadlock`: a 1 MiB user prompt against a script that prints its reply before reading stdin.
  - `timeout_kills_the_child`: the script writes its pid and runs `exec sleep 30`, `timeout` 200 ms, a 1 MiB prompt the sleep never reads: `Error::Model` containing `timed out` within 5 s, and on Linux the script's pid has no `/proc` entry left, so it was killed and reaped.
  - `missing_program_is_a_model_error`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test claude`. Expected: fails (the spec is `Invalid`).
- [ ] **Step 3: Implement** `llm/claude.rs`.
- [ ] **Step 4: Run** `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"`. Expected: every step passes up to `cargo deny`.
- [ ] **Step 5: Commit**: "The Claude CLI as a backend, with a timeout that kills the call".

Part A ends here: review, fix, push to PR #6. The advisories job runs on this PR (the manifest and the lockfile change); read its result before asking for the merge.

---

# Part B: extraction and the haiyaku binary

### Task 6: Finding quotes and names in a chapter

**Files:**
- Create: `crates/haiyaku/src/extract/text.rs`
- Modify: `crates/haiyaku/src/extract/mod.rs` (re-exports)
- Test: `crates/haiyaku/tests/text.rs`, with `#![cfg(feature = "extract")]` at its top as `tests/epub.rs` has: `common::epub` exists only with that feature, and `scripts/check-all.sh` also runs the tests without it.

**Interfaces:**
- Consumes: `book::ChapterText`, `log::Position`; `common::epub::chapter_text` (Task 3).
- Produces, in `haiyaku::extract`:

```rust
pub const MIN_QUOTE_WEIGHT: u32 = 8;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteError { TooShort, NotFound }
/// The block holding the start of quote, if the chapter says it.
pub fn locate_quote(chapter: &ChapterText, quote: &str) -> std::result::Result<Position, QuoteError>;
/// The first block in which name stands as whole words.
pub fn locate_name(chapter: &ChapterText, name: &str) -> Option<Position>;
pub fn weight(text: &str) -> u32;
```

- Produces, for Tasks 7 and 8 (`pub(crate)`): `fn reduce(text: &str) -> String`, the letters and digits of `text`, lowercased, each final sigma `ς` made `σ` (lowercased one letter at a time, a capital sigma is always `σ`). No Unicode normalisation: NFD is out of scope.

Rules:
1. **Quote.** The quote is split at `...` and `…`; empty parts are dropped. Each part, reduced, must occur in the chapter's reduced text (all blocks run together, so a quote may cross a paragraph break), each after the one before. The position is the block holding the first letter of the first part's match.
2. **Weight.** A Hangul syllable, a Han character (the iteration mark `々` included) or a kana counts 3; any other letter or digit counts 1. The halfwidth sound marks `ﾞ` and `ﾟ` count 0: each marks the kana before it, so a halfwidth syllable written in two characters weighs what its fullwidth form does (for rule 3 they are kana). A quote whose parts weigh less than `MIN_QUOTE_WEIGHT` together is `TooShort`, checked before the search. Eight Latin letters is a sentence fragment and three Hangul syllables are a name called out, which the spike wrongly refused.
3. **Name.** A text's words are its runs of letters and digits, lowercased. A name with no words is nowhere. A name is in a block when its words match consecutive words of the block: each equal, except that the block's word may run on past the name's last word when that word ends in a Hangul syllable, a Han character or a kana (a particle follows the name without a space). Names do not cross blocks.

- [ ] **Step 1: Write the failing tests** in `tests/text.rs`, on chapters made with `chapter_text` from invented sentences:
  - `quote_ignores_case_spacing_and_punctuation`; `quote_with_ellipsis_matches_parts_in_order` (both `...` and `…`); `parts_out_of_order_are_not_found`; `quote_absent_is_not_found`.
  - `quote_position_is_the_block_of_its_start` (a quote that starts in block 2 gives `Position { document, block: 2 }`; one that crosses from block 1 into block 2 gives block 1).
  - `short_latin_quote_is_too_short` (`"Run now"`, present in the text, is `TooShort`); `three_hangul_syllables_are_enough` (`"지수야!"` passes, `"지수"` is `TooShort`); `weights` (`weight("Ada 12")` is 5, `weight("지수")` is 6, `weight("漢字かな")` is 12, `weight("—!")` is 0); `halfwidth_kana_weigh_as_their_fullwidth_forms` (`weight("ﾘｭｳｼﾞ")` equals `weight("リュウジ")`).
  - `name_needs_whole_words` (`"Al"` is not in `"He would walk to Alba."`; `"Ada"` is in `"Ada's lamp"`); `empty_name_is_absent` (`""`, `"—"`).
  - `multiword_name_matches_consecutive_words` (`"Ada Marsh"` in `"said Ada Marsh."`, not in `"Ada went to the marsh"`).
  - `korean_name_matches_before_a_particle` (`"한지수"` is in `"한지수는 등대로 갔다"`; `"지수"` is not: it does not start a word; `"한 씨"` is in `"한 씨는 웃었다"`).
  - `name_position_is_its_first_block`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test text`. Expected: fails to compile.
- [ ] **Step 3: Implement** `extract/text.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test text`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "Find a quote's block and a name's whole words in a chapter, with a quote length that fits the script".

### Task 7a: From a reply to events: the characters

**Files:**
- Create: `crates/haiyaku/src/extract/check.rs`, `crates/haiyaku/src/extract/roster.rs`
- Modify: `crates/haiyaku/src/extract/mod.rs`, `crates/haiyaku/src/log.rs` (`RelationKind::ALL` becomes `pub`: the schema and the prompt list the kinds from it), `crates/haiyaku/tests/common/epub.rs` (`relevant_roster`)
- Test: `crates/haiyaku/tests/check.rs`, with `#![cfg(feature = "extract")]` at its top, as in Task 6

**Interfaces:**
- Consumes: `locate_quote`, `locate_name`, `reduce` (Task 6); `log::{Event, EventBody, Entity, Name, RelationKind, Sex}`; `fold::Folder` in tests.
- Produces, in `haiyaku::extract`:

```rust
/// The JSON schema of a reply (below), every field required, no extra properties.
pub fn schema() -> serde_json::Value;
/// What extraction knows of the book so far: the characters, their names, the merges, and every event kept.
#[derive(Debug, Default)]
pub struct Roster { /* private */ }
impl Roster {
    pub fn new() -> Roster;
    pub fn events(&self) -> &[Event];        // seq from 1
    pub fn entities(&self) -> Vec<Entity>;   // every id given out, merged ones included
    /// The ids a prompt for this chapter lists (rule 10), in id order.
    pub fn listed(&self, chapter: &ChapterText) -> Vec<String>;
}
#[derive(Debug)]
pub struct Outcome {
    pub events: Vec<Event>,                  // this chapter's, already appended to the roster
    pub kept: u32,                           // reply items that gave at least one event
    pub rejected: Vec<Rejected>,
    pub notes: std::collections::BTreeMap<&'static str, u32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected { pub reason: &'static str, pub item: serde_json::Value }
pub fn check(roster: &mut Roster, chapter: &ChapterText, reply: &serde_json::Value) -> Outcome;
```

- Produces, for Tasks 7b and 8a (`pub(crate)`): `Roster::survivor(&self, id: &str) -> &str`, the id that stands for `id`'s character now: `id` itself, or the survivor of the same-as that merged it away, followed through every later merge. The `Roster` keeps the merges, and rule 1, rule 10 and Task 8a's prompt reach a character only through this one resolver.
- Produces, in `tests/common/epub.rs`: `pub fn relevant_roster() -> (Roster, ChapterText)`, the roster after three invented chapters run through `check` (chapter 1 introduces "Bren Holt" and "Old Sal"; chapter 2 introduces "Mira"; chapter 3 lists Mira again), and a chapter 4 whose text names only Old Sal. `tests/check.rs` and `tests/extract.rs` both take that roster from it, and neither holds a copy.

A reply:

```text
{ "characters": [ { "ref": string, "name": NAME, "aliases": [NAME], "evidence": string } ],
  "relations":  [ { "from": string, "to": string, "kind": string, "note": string, "evidence": string } ],
  "same_as":    [ { "ref": string, "other": string, "evidence": string } ] }
NAME = { "text": string, "given": string|null, "surname": string|null, "title": string|null, "title_sex": "male"|"female"|null }
```

Rules. Reasons and notes are the quoted words. Task 7a builds rules 1 to 5 and 8 to 10, the characters; rules 6 and 7, relations and same-as, are Task 7b's.
1. **References.** A `ref` is a known character only when it is one of `roster.listed(chapter)`, taken before this reply changes the roster. Any other string is a handle for a new character and means something only inside this reply, even when it looks like an id: the model sees a part of the roster, and a model that makes up `c42` beside the `c3` and `c17` it was shown must not land on the unlisted character that holds that id.
2. **Names.** A name's `text` is trimmed. A labelled part whose reduced form is not inside the reduced `text` is cleared (note `label_not_in_name`); `title_sex` stays only beside a `title`. A name is "in the chapter" when `locate_name` finds its `text`.
3. **A new character.** Its candidates are `name`, then `aliases` in order. With none in the chapter the item is rejected, `name_not_in_text`, and its handle stays unknown. Otherwise the first candidate in the chapter is its name (note `name_replaced_by_alias` when that is not `name`). If that name has a given name and a surname, and a surviving character carries a name with the same two (compared reduced), the handle means that character (note `matched_by_name`). When none carries it, the name is a new character's, and an alias that is another character's full name is rejected under rule 4, `name_held_by_other`: a mistaken alias must not pull a new character into a known one. A name with less than both (a lone given name or surname, or a description) can match no one by itself, so it is matched through its candidates: the first of them in the chapter, tried in order, that has a given name and a surname a surviving character carries makes the handle mean that character (note `matched_by_name`). When the handle means a known character, rule 4 applies to every candidate, the one that matched included. A merged-away id never matches; when several survivors carry the name, the one with the lowest id number does. Nothing less joins a new character to a known one: an equal alias, title or lone surname is how a father landed on his son. Otherwise the character gets the next id and an `Introduced` event.
4. **A known character** gets one `Appeared` a chapter, however often the reply lists it, and none in the chapter that introduced it. Then, for every character, each name of the item not yet carried by it (texts compared reduced), which passes over the name rule 3 introduced it under: in the chapter, it becomes `Named` when it is the item's `name` and outranks (`Name::rank`) the name the character is shown under now (its `Introduced` name or its latest `Named`), and `Alias` otherwise; not in the chapter, it is rejected, `alias_not_in_text`. The fold picks the shown name by the same comparison, and a partial name may replace a description, so an alias of equal rank never keeps a description shown. A full name (a given name and a surname) that another surviving character carries is not given to this one: it is rejected, `name_held_by_other`. No two characters carry one full name, and only a same-as joins two characters.
5. **Quotes.** `Introduced` and `Appeared` carry the item's `evidence` at the position `locate_quote` gives. When the evidence fails, they carry the first of the item's names that is in the chapter, as the reply writes it, at `locate_name`'s position (note `evidence_replaced`), so a sound character does not fall, and take its relations with it, over a quote the model re-worded. A known character with neither is rejected, `evidence_too_short` or `evidence_not_found`. `Named` and `Alias` carry the name's own text at its first block: plan 3's link through an alias holds from that block.

Rules 6 and 7 are in Task 7b.

8. **Shape.** An item that lacks a field or has one of the wrong type is rejected, `malformed`, alone. A missing array is an empty one.
9. **Order.** Characters in the reply's order, each with its `Introduced` or `Appeared` first and its `Named` and `Alias` events after; then relations; then same-as. `seq` continues from the roster's last event, `chapter` is `chapter.number`, ids are `c1`, `c2`, ... in order of introduction. `rejected` holds whole items and single names alike.
10. **The part of the roster a chapter gets.** `listed` holds a character when one of its names or aliases is in the chapter (`locate_name`), or a surname labelled in one of them is, or when it was last seen in one of the two chapters before this one. The surname reaches a character the chapter writes in a form it has not carried yet: one met as "Mrs. Marsh" and written "Ada Marsh" later would otherwise go unlisted and be given a second id. A surname stands in the chapter as `locate_surname` finds it: one of a single letter or syllable never does, since it starts too many words (한 starts 한숨); one with case (a letter whose upper and lower case differ) has to stand as whole words written in its own case, since lowercased it is often a word of the language ("Bell", "the bell"); any other is found as `locate_name` finds a name, particle and all. A name or an alias is matched by `locate_name` as before. A merged-away id is never listed; its survivor (`Roster::survivor`) carries its names and its chapters.

- [ ] **Step 1: Write the failing tests** in `tests/check.rs`, on `chapter_text` chapters of invented sentences and replies written with `serde_json::json!`:
  - `first_chapter_introduces`: handles `keeper` (name "the keeper", no labels) and `dog` (name "Pip", given): events `Introduced c1`, `Introduced c2` with `seq` 1, 2, each `at` the block its evidence starts in; `kept == 2`; entities `c1`, `c2`. `ids_are_the_programs`: no entity id and no event field holds `keeper` or `dog`.
  - `known_character_appears_once_a_chapter` (listed twice: one `Appeared`); `no_appeared_in_the_chapter_of_introduction`.
  - `named_upgrades_a_description`: in chapter 2, `ref: "c1"` with name "Ada Marsh" (given, surname) gives `Appeared` then `Named`; a `Folder` over `roster.events()` shows "Ada Marsh" with alias "the keeper". `lesser_name_becomes_an_alias`: in chapter 3, name "Mrs. Marsh" (surname, title, female) gives `Alias` whose `quote` is `Mrs. Marsh` and whose `at` is the block where it first stands.
  - `name_outranking_the_shown_name_is_named`: c1 introduced in chapter 1 as "the stranger" with the alias "Mr. Reed" (surname, title); in chapter 2, `ref: "c1"` named "Tom" (given) gives `Named`, and a `Folder` over `roster.events()` shows "Tom".
  - `new_character_with_a_known_alias_stays_new`: the roster holds c1 "Ada Marsh" with alias "Mrs. Marsh"; a chapter about her husband's mother, also "Mrs. Marsh", with a handle of that name: a new id is introduced, and the note `matched_by_name` is absent. `full_name_match_is_the_known_character`: a handle named "Ada Marsh" with both labels gives `Appeared { entity: c1 }`, note `matched_by_name`, no new entity.
  - `a_known_full_name_among_the_aliases_is_the_known_character`: c1 "Tom Reed"; a handle named "the stranger" (no labels) with the alias "Tom Reed" gives `Appeared { entity: c1 }` and the alias "the stranger", note `matched_by_name`. `a_new_full_name_stays_new_beside_a_known_full_name_among_its_aliases`: c1 "Tom Reed"; a handle named "Hal Brook" (given, surname) with the alias "Tom Reed" introduces c2 "Hal Brook", and "Tom Reed" is rejected, `name_held_by_other`. `a_lone_given_name_is_matched_by_a_known_full_name_among_its_aliases`: c1 "Hal Brook"; a handle named "Hal" (given) with the alias "Hal Brook" gives `Appeared { entity: c1 }` and the alias "Hal", note `matched_by_name`.
  - `full_name_held_by_another_is_rejected`: c1 "Ada Marsh" (given, surname) and c2 "the keeper" in the roster; a later reply gives `c2` the name "Ada Marsh" with both labels: c2 gets no `Named` and no `Alias`, and "Ada Marsh" is rejected, `name_held_by_other`.
  - `empty_name_and_embedded_name_are_rejected`: a handle whose only name is `""`, and one named `Al` in a chapter that has only `walk` and `Alba`: both `name_not_in_text`.
  - `listed_is_the_named_and_the_recent`: for chapter 4 of `relevant_roster()`, `listed` is Old Sal's id and Mira's, not Bren Holt's. `listed_is_also_the_labelled_surname_in_the_chapter`: "Mrs. Marsh" (surname labelled), introduced in chapter 1, is listed for a chapter 4 that writes only "Ada Marsh"; "Captain Holt", with no surname labelled, is not listed for one that writes "Jon Holt". `a_cased_surname_lists_only_as_written_and_whole`: "Nora Bell" is not listed for "The bell rang." and is for "Bell caught it.". `a_one_syllable_surname_never_lists_and_a_longer_one_runs_on`: 한지수 (surname 한) is not listed for "그는 한숨을 쉬었다."; "홀트 부인" (surname 홀트) is listed for "브렌 홀트가 웃었다.".
  - `failed_evidence_falls_back_to_the_name`: a new character whose evidence is not in the chapter but whose name is: `Introduced` with the name as `quote`, note `evidence_replaced`. `character_known_only_by_an_alias_is_introduced_under_it` (name absent, alias "Mum" present: introduced as "Mum", note `name_replaced_by_alias`).
  - `label_outside_the_name_is_cleared` (`{ text: "Pip", surname: "Marsh" }` is stored without a surname); `title_sex_needs_a_title`.
  - `korean_name_with_particle_and_trailing_title`: the chapter `한지수는 등대로 갔다. 한 씨는 웃었다.`, a handle named `한지수` (given `지수`, surname `한`) with alias `한 씨` (surname `한`, title `씨`, male): `Introduced` and `Alias` with exactly those parts.
  - `malformed_items_are_rejected_one_by_one` (a character without `name`: `malformed`, the reply's other characters kept); `missing_array_is_empty`.
  - `schema_lists_every_relation_kind` (the `kind` enum equals `RelationKind::ALL` as strings).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test check`. Expected: fails to compile.
- [ ] **Step 3: Implement** `extract/roster.rs` and `extract/check.rs` for rules 1 to 5 and 8 to 10, and `relevant_roster`. `spike/src/extract.rs:174-324` is the spike's check, with the defects these rules remove.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test check`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "Check the characters of a model's reply against the chapter and give them the program's own ids".

### Task 7b: From a reply to events: relations and same-as

**Files:**
- Modify: `crates/haiyaku/src/extract/check.rs`, `crates/haiyaku/src/extract/roster.rs` (the merges), `crates/haiyaku/tests/common/epub.rs` (a relation in `relevant_roster`)
- Test: `crates/haiyaku/tests/check.rs`

**Interfaces:**
- Consumes and produces what Task 7a does; the interface gains nothing. `relevant_roster`'s chapter 1 reply gains a relation between Bren Holt and Old Sal, which Task 8a's prompt test reads.

Rules, numbered on from Task 7a's; its rules 1, 8 and 9 hold for these items as for characters.

6. **Relations.** Both ends resolve by rule 1, a handle only if rule 3 accepted it (else `unknown_ref`); the two differ after resolving (else `self_relation`); `kind`, trimmed and lowercased, parses with `RelationKind::parse` (else `bad_kind`); the evidence passes `locate_quote`, with no fallback (else the quote's reason). The event keeps the reply's direction, and the `note` trimmed, each run of whitespace in it, line breaks included, made one space.
7. **Same as.** Both resolve (`unknown_ref`), differ (`self_same_as`), and the evidence passes. The survivor is the one whose best name ranks higher, and on a tie the one with the lower id number, so "the stranger" joins Tom Reed and not the other way round. The event is `SameAs { entity: survivor, other }`; from then on the survivor carries the names of both.

- [ ] **Step 1: Write the failing tests** in `tests/check.rs`: new tests, and in six of Task 7a's tests the half that needs a relation or a same-as.
  - `unlisted_id_is_a_handle`: after a same-as of c3 "the stranger" and c4 "Tom Reed", a later reply's `ref: "c3"` with a name that is in the chapter introduces a new character under the next id, and no event names `c3`; the same for the id of a character that exists but is not listed. `survivor_is_the_better_named`: the same-as written either way round gives `SameAs { entity: c4, other: c3 }`; with equal ranks the lower number survives.
  - `full_name_match_lands_on_the_survivor`: c1 "Tom Reed" and c2 "Thomas Reed", each with a given name and a surname, and a same-as of the two, which c1 survives; a later handle named "Thomas Reed" with both labels gives `Appeared { entity: c1 }`, note `matched_by_name`, and no event names `c2`.
  - `character_nowhere_in_the_text_is_rejected_with_its_relations` (`name_not_in_text`, then `unknown_ref`).
  - `relation_needs_its_own_evidence`; `self_relation_rejected` (also for a handle that rule 3 matched to the character at the other end); `kind_is_trimmed_and_lowercased` (`" Owns "`); `unknown_kind_rejected`.
  - `first_chapter_introduces` gains a relation `keeper` to `dog` of kind `owns`: a third event `Relation { from: c1, to: c2, kind: Owns }` with `seq` 3, `at` the block its evidence starts in, and `kept == 3`.
  - `new_character_with_a_known_alias_stays_new` gains a `family` relation from the handle to `c1`, which runs from the new id.
  - `listed_is_the_named_and_the_recent` gains its merge half: after a same-as, the merged id is not listed in a chapter that names it; the survivor is.
  - `failed_evidence_falls_back_to_the_name` and `character_known_only_by_an_alias_is_introduced_under_it` each gain a relation from the character, which is kept.
  - `malformed_items_are_rejected_one_by_one` gains a relation whose `from` is a number: two `malformed`, the reply's other items kept.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test check`. Expected: the new tests and the gained halves fail.
- [ ] **Step 3: Implement** rules 6 and 7 in `extract/check.rs`, the merges in `extract/roster.rs`, and the relation in `relevant_roster`. `spike/src/extract.rs:174-324` is the spike's check, as in Task 7a.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test check`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "Check a reply's relations and same-as against the chapter, and merge a character into the better-named one".

### Task 8a: The prompt and the part of the roster it carries

**Files:**
- Create: `crates/haiyaku/src/extract/prompt.rs`
- Modify: `crates/haiyaku/src/extract/mod.rs`, `crates/haiyaku/src/extract/roster.rs` (what the prompt reads), `crates/haiyaku/tests/common/epub.rs` (`keeper_replies`)
- Test: `crates/haiyaku/tests/extract.rs`, with `#![cfg(feature = "extract")]` at its top, as in Task 6

**Interfaces:**
- Consumes: `ChapterText` (Task 3); `Roster`, `Roster::survivor`, `schema`, `RelationKind::ALL` (Task 7a); `check` and `relevant_roster` (Tasks 7a and 7b) in tests.
- Produces, in `haiyaku::extract`:

```rust
pub const PROMPT_VERSION: u32 = 4;
pub const SYSTEM: &str = /* one sentence: the reader has read to the end of this chapter and no further; reply with JSON only */;
pub fn prompt(roster: &Roster, chapter: &ChapterText) -> String;
```

- Produces, in `tests/common/epub.rs`: `pub fn keeper_replies() -> Vec<String>`, three invented replies for chapters 1 to 3 of `keeper_epub`, quoting its text: chapter 1 introduces "the keeper" and "Pip" with an `owns` relation; chapter 2 names `c1` "Ada Marsh", introduces "the stranger" and an `acquaintance` relation with the note "met on the jetty"; chapter 3 introduces "Tom Reed", gives `c1` the alias "Mrs. Marsh", and a same-as of the stranger and Tom Reed.

Rules:
1. **The part of the roster a prompt carries** is `roster.listed(chapter)` (Task 7a, rule 10), so the prompt and the check agree on which ids the model was shown. Characters go in id order. A relation's two ends are resolved to their survivors first (`Roster::survivor`), and the relation is listed, under the survivors' refs, when both are listed: a merge must not drop a relation from every later prompt. A fact (from, kind, to) is listed once, in the order first seen. A relation whose two ends resolve to one character is not listed.
2. **The prompt**, in this order: `Chapter <number>: <title>`, a blank line, `chapter.text()`; `Characters so far (ref | name | other names):` with a line each, or `(none yet)`; `Relations so far (from | kind | to | note):` when there are any; the rules, last. The chapter, by far the longest part, comes first and the rules last, where a model given a long document follows instructions best. In a cell of either list each `|` is written `/` and each run of line breaks one space, so a name or a note stays one cell of one row. `spike/src/extract.rs:47-82` is the prompt the spike's numbers were measured with; keep its wording where a rule is unchanged. Every example in the rules is invented. The rules say:
   - use only this chapter and the lists above, nothing known about the book from elsewhere;
   - a listed character keeps its `ref`; a character called by a listed personal name (a given name or a surname, not a description or a form of address) is that listed character unless the chapter shows it is someone else, and one called by a listed description or form of address is the listed character only when the chapter shows it is, since a description fits many people; any other gets a `ref` of its own that is not listed (`new1`, `new2`, ...);
   - a name's `text` is written as the chapter writes it, or as the list writes it for a listed name: a name the model gives from this chapter, a description included, is one run of words that stands in the chapter in that order and form (a listed name is spared, since the check takes a name the character carries without looking for it in the chapter), never a name joined from parts the chapter writes apart (when it writes "Ann and Joe Bell", Ann's name is "Ann"), never "Given Surname" built from "Surname, Given", never a description in the model's own words ("a man in a grey coat" as the chapter has it, not "the grey-coated stranger"); an unnamed character's description is copied from the text; `given`, `surname` and `title` label the parts it has and are null otherwise; a description ("the tall woman", "엄마") has no labels; `title_sex` only when the title itself tells it; a title may follow the name;
   - when the chapter gives a listed character a fuller name, that name goes in `name`;
   - `aliases` are the other names and forms of address this chapter uses, never a pronoun;
   - the relation kinds, from `RelationKind::ALL`; `mentor`, `serves` and `owns` run from the mentor, the servant and the owner; a pair already listed gains a further relation when the chapter reveals one of another kind, even one that contradicts what is listed, so that a listed relation does not hold back a new one;
   - `same_as` only when this chapter reveals that two characters are one; `ref` and `other` are two different characters, and a character that only appears again is not a same-as;
   - `evidence` is a phrase or a sentence copied exactly from this chapter, character for character: never a name where the text has a pronoun, never two passages joined with "..." or any other mark, never a word or its ending changed. No word count: the spike's "5 to 25 words" does not fit Korean.

- [ ] **Step 1: Write the failing tests** in `tests/extract.rs`:
  - `first_prompt_says_none_yet`; `prompt_starts_with_the_chapter_and_ends_with_the_rules` (the chapter's heading and text, then the characters list; every line after the rules' heading is a rule); `prompt_names_every_relation_kind`; `only_a_listed_personal_name_makes_a_character_the_listed_one` (the rule on listed characters says a given name or a surname, and that a description or form of address is the listed character only when the chapter shows it is); `a_listed_name_is_spared_the_run_of_words_rule` (the rule of one run of words is for a name given from this chapter); `a_listed_pair_gains_a_relation_of_another_kind` (the relations rule ends with the sentence that a listed pair gains a relation of another kind).
  - `prompt_carries_only_the_relevant_roster`: for chapter 4 of `relevant_roster()`, the prompt has a line for Old Sal and for Mira, "Bren Holt" is nowhere in it, and the relation between Bren Holt and Old Sal is not listed.
  - `prompt_lists_a_relation_through_its_survivors`: chapter 1 introduces c1 "Ada Marsh" and c2 "the stranger", with an `acquaintance` from c1 to c2; chapter 2 introduces c3 "Tom Reed", with an `acquaintance` from c1 to c3, an `ally` between c2 and c3, and a same-as of c2 and c3; the prompt for a chapter 3 that names Ada Marsh and Tom Reed has one `acquaintance` line, from c1 to c3, and no `ally` line.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test extract`. Expected: fails to compile.
- [ ] **Step 3: Implement** `extract/prompt.rs`, what it reads in `extract/roster.rs`, and `keeper_replies`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test extract`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "A prompt that carries the part of the roster a chapter needs, and recorded replies for the keeper book".

### Task 8b: A whole book, with a reply cache that resumes a run

**Files:**
- Create: `crates/haiyaku/src/extract/run.rs`
- Modify: `crates/haiyaku/src/extract/mod.rs`, `crates/haiyaku/src/store/sidecar.rs` (its hex encoding becomes a `pub(crate)` helper, and `create_beside` takes any target path, not only an export's), `crates/haiyaku/src/store/mod.rs` (`sidecar` is a private module, so it re-exports both to the crate)
- Test: `crates/haiyaku/tests/extract.rs`

**Interfaces:**
- Consumes: `Book`, `ChapterText` (Task 3); `Backend`, `Request`, `Replay` (Task 4); `Roster`, `check`, `schema` (Task 7a); `prompt`, `SYSTEM`, `PROMPT_VERSION`, `keeper_replies` (Task 8a); `log::{BookLog, BookRecord, Chapter}`; `store::write_book` and `cast_at` in tests.
- Produces, in `haiyaku::extract`:

```rust
#[derive(Debug, Clone)]
pub struct Options<'a> {
    pub first: u32,                 // table-of-contents entries first..=last
    pub last: u32,
    pub cache: Option<&'a Path>,    // the binary passes $XDG_CACHE_HOME/haiyaku
    pub extracted_at: &'a str,      // RFC 3339, UTC
}
pub fn extract_book(book: &Book, backend: &dyn Backend, options: &Options<'_>, progress: &mut dyn FnMut(&Chapter)) -> Result<BookLog>;
```

Rules:
1. **The run.** For each chapter of `book.chapters(first, last)` in order: the prompt, one call with `schema()`, `check`, a `Chapter` row (number, `toc_entry`, title, the reply's seconds and cost, `kept`, the number rejected), then `progress`. The result is the `BookLog` with `backend.name()`, `backend.model()`, `PROMPT_VERSION`, `book.document_rows`, the roster's entities and events. It writes to no database.
2. **A chapter without text** makes no call. Its row has seconds 0.0, cost `Some(0.0)`, `kept` 0 and rejected 0: it cost nothing, and that is known.
3. **One retry.** A call that is `Error::Model` is made once more. A second failure ends the run with `Error::Model` naming the chapter's number and title and the cause.
4. **The cache**, when given. A reply's key is the lowercase hex SHA-256 of the backend's name, the model, its endpoint (`Backend::endpoint`: a server's base URL, a replay's directory canonicalised, empty for `claude`), the system text, the prompt and the schema's JSON text, each followed by a zero byte, so two servers that serve a model under one name, or two replay directories, do not answer for each other. After a call succeeds its reply is written to `<cache>/<book sha256>/replies/<key>.json` as `{ "raw", "json", "seconds", "cost_usd" }`, through a temporary file in the same directory and a rename. The hex and the temporary file are `store/sidecar.rs`'s, the ones `sha256_file` and `export_book` use, so the crate has one copy of each. Before a call, a file under the key is the reply, with the seconds and cost it recorded, so a resumed run reports what the whole book took; a file that does not parse is ignored. What a chapter's check rejects, and its notes, go to `<cache>/<book sha256>/rejected/<nnn>.json` (the chapter's table-of-contents entry, three digits, so each entry's file holds the latest run's record for that entry, whatever range that run covered), rewritten on each run, as `{ "rejected": [ { "reason", "item" } ], "notes": { <note>: <count> } }`: this file is the one reader of `Outcome.notes`. A chapter without text writes `{ "rejected": [], "notes": {} }` there, so a file an earlier run left for its entry does not stand. Without a cache neither file is written. This is the resume: a prompt depends only on the chapters before it, so a run started again finds every finished chapter in the cache and calls the model from the one that failed.

- [ ] **Step 1: Write the failing tests** in `tests/extract.rs`:
  - `extracts_the_keeper`: `keeper_epub`, `Replay::new(keeper_replies())`, entries 1 to 3, no cache. The record has the title, `en`, backend `replay`, model `none`, `PROMPT_VERSION` and the `extracted_at` given; three chapters with `toc_entry` 1, 2, 3, `kept > 0`, `rejected == 0`; document chapters `[None, Some(1), Some(2), Some(3), None]`. `store::write_book` into `common::db()` accepts it. Then `cast_at(.., "OEBPS/c2.xhtml", &Scope::Book)` names exactly "the keeper" and "Pip"; at `"OEBPS/back.xhtml"` there is "Ada Marsh" with aliases "the keeper" and "Mrs. Marsh", "Tom Reed" with one merged id, and no character named "the stranger".
  - `documents_after_the_range_show_the_whole_extracted_cast`: entries 1 to 2 only; `cast_at` at `OEBPS/c3.xhtml` and at `OEBPS/back.xhtml` both give the cast after chapter 2.
  - `progress_is_called_once_per_chapter`.
  - `one_retry_on_a_model_error`: replies `"not json"` then the three good ones: the run succeeds and `remaining() == 0`; replies `"x"`, `"y"`: `Error::Model` whose message contains `chapter 1`.
  - `second_run_calls_nothing`: with a cache directory, a second run on `Replay::new(vec![])` returns a `BookLog` equal to the first.
  - `resume_after_failure`: with a cache, a run on the first two replies fails naming `chapter 3`; a run on the third reply alone succeeds and equals an uninterrupted run's log.
  - `chapter_without_text_calls_no_model`: a book whose second entry is a document with an image and no text, two replies: the run succeeds, the `Replay`'s `remaining() == 0` after it, and chapter 2's row has `kept == 0`, seconds 0 and cost `Some(0.0)`.
  - `rejected_items_go_to_the_cache`: a reply with one relation to an unknown handle: `rejected/001.json` holds it under `rejected` with the reason `unknown_ref`, beside a `notes` object; the log does not hold it, and the chapter's `rejected == 1`.
  - `rejected_files_are_named_by_contents_entry`: a run over entry 1, then one over entry 2: `001.json` still holds entry 1's rejected relation, and `002.json` is there.
  - `chapter_without_text_writes_an_empty_rejected_file`: over a stale `002.json`, the plate's file is `{ "rejected": [], "notes": {} }`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test extract`. Expected: fails to compile.
- [ ] **Step 3: Implement** `extract/run.rs`, and the two helpers in `store/sidecar.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test extract`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "Extract a whole book: one checked call a chapter, one retry, and a reply cache that resumes a run".

### Task 9a: The binary: `extract`

**Files:**
- Modify: `crates/haiyaku/src/bin/haiyaku/main.rs`, `crates/haiyaku/Cargo.toml` (clap as in Global Constraints, and `"dep:clap"` in `cli`)
- Create: `crates/haiyaku/src/bin/haiyaku/paths.rs`, `clock.rs`, `extract.rs` (same directory)
- Test: `crates/haiyaku/tests/cli.rs` (`#![cfg(feature = "cli")]`), unit tests in `paths.rs` and `clock.rs`

**Interfaces:**
- Consumes: `Book::open`, `llm::{backend, Settings}`, `extract::{extract_book, Options}`, `store::{migrate, write_book}`; `keeper_epub`, `keeper_replies`.
- Produces: `haiyaku [--db <path>] extract <book.epub> [--chapters <a-b>] [--backend <spec>] [--timeout <seconds>] [--no-cache] [--replace]`, and the paths, the opened database and the errors that Task 9b's `show` shares.

Rules:
1. **Paths.** The database is `$XDG_DATA_HOME/haiyaku/haiyaku.db` unless `--db` names another; the cache is `$XDG_CACHE_HOME/haiyaku`. An XDG variable that is unset, empty or not an absolute path gives way to `$HOME/.local/share` and `$HOME/.cache`, as the XDG specification says. The database's directory is created when needed; opening sets `PRAGMA foreign_keys = ON` and calls `store::migrate`.
2. **`extract` without `--chapters`** prints one line per table-of-contents entry (number, label, how many documents, how many characters of text) and a last line saying to pick the story's entries with `--chapters`. It opens no database and builds no backend.
3. **`extract --chapters a-b`** (or one number). `--backend` defaults to `claude`; `Settings` takes `--timeout` (600) and `OLLAMA_HOST`. `--backend`, `--timeout`, `--no-cache` and `--replace` require `--chapters` (clap's `requires`: a usage error without it). `--chapters`' help says a run replaces the book's stored log; `--replace`'s help says the run replaces the stored log and drops the book's series links, which name the old log's characters. A run goes in this order:
   1. The range is checked against the book.
   2. The database is opened: its directory created, a file SQLite opens only read-only refused, a write tried and rolled back, then foreign keys on and migrated. The write comes before migrating, so a database that cannot be written (a file in a directory the user may not write, where SQLite cannot make its journal) fails here as one that cannot be written, and one whose write lock another run holds past the busy timeout fails as in use by another run, whether or not it needs migrating. A database that already holds the book (by its sha256) is refused unless `--replace` is given, with an error that names `--replace`.
   3. The backend is built. Everything above fails before any call is paid for.
   4. `extract_book` runs the chapters, and each finished chapter writes its progress line to stderr.
   5. `write_book` stores the log. It is the run's one stored write, so a run that fails before it leaves every book's rows as they were (an empty migrated database may remain: it holds no book).
   6. The totals go to stdout.

   A progress line is `<number> (entry <toc_entry>) <title>`, then seconds, kept, rejected and cost. A chapter's number counts from the first entry extracted, while the list without `--chapters` numbers contents entries, so the line shows both. Progress lines are written as `say` writes stdout, but a line that cannot be written does not stop the run: the first such failure is the command's (status 1) once the log is stored. The totals are the chapters, kept, rejected, seconds, and the sum of the chapters' costs, or "cost unknown" when any chapter's cost is `None`.
4. **`--no-cache`** gives `extract_book` no cache: no reply is read from it or kept in it, and no rejected items are written. Its help says so.
5. **Errors** go to stderr as `haiyaku: <message>` with exit status 1. Usage errors are clap's, status 2.
6. `clock::rfc3339_utc(seconds_since_epoch: u64) -> String`, to the second, with `Z`.

- [ ] **Step 1: Write the failing tests.** Unit: `rfc3339_utc(0)` is `1970-01-01T00:00:00Z`, `951_782_400` is `2000-02-29T00:00:00Z`, `1_709_251_199` is `2024-02-29T23:59:59Z`; `paths` on a passed-in lookup function rather than the real environment: unset, empty and relative values fall back, an absolute one is used. In `tests/cli.rs`, every test runs `env!("CARGO_BIN_EXE_haiyaku")` after `env_clear()`, with `HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` inside its `TempDir` and `PATH` set to an empty directory inside it, so no test can reach the owner's `claude` or `OLLAMA_HOST`, even after a regression. Every `extract` with a range passes `--backend replay:<dir>`, where `<dir>` holds `keeper_replies` as `001.json` to `003.json` unless the test says otherwise:
  - `extract_without_chapters_lists_the_table_of_contents`: four numbered lines with the labels, status 0, and nothing exists under the data directory.
  - `default_database_follows_xdg_data_home` (the file `<XDG_DATA_HOME>/haiyaku/haiyaku.db` exists, and `<XDG_CACHE_HOME>/haiyaku/<sha256>/replies` holds three files); `relative_xdg_value_is_ignored` (`XDG_DATA_HOME=rel`: the database is under `<HOME>/.local/share/haiyaku`); `db_flag_names_the_database`; `no_cache_flag_writes_no_cache`.
  - `second_extract_uses_the_cache` (after a run, the same replay directory with its replies removed, so the cache key is the same and only the cache can answer: a second run with `--replace` is status 0).
  - `extract_of_a_stored_book_needs_replace` (a second run without `--replace`, against an empty replay directory, fails naming `--replace` before any call and leaves `show`'s output as it was; with `--replace` it succeeds); `failed_run_stores_no_book`; `database_that_cannot_be_made_fails_before_any_call`, `read_only_database_fails_before_any_call`, `database_in_a_directory_that_cannot_be_written_fails_before_any_call` (a current-schema file of mode 644 in a directory of mode 555) and `database_in_use_by_another_run_fails_before_any_call` (the test holds the write lock, and the error says the database is in use): each with `--no-cache` and an empty replay directory, and its error names the database, not the replay.
  - `options_of_a_run_need_chapters` (each of the four without `--chapters` is status 2, naming `--chapters`); `timeout_and_ollama_host_reach_the_backend` (`OLLAMA_HOST` at a 127.0.0.1 listener that never answers, `--backend ollama:<model> --timeout 1`: the request arrives, and the run fails within a minute, its error saying it timed out after 1s); `progress_that_cannot_be_written_fails_once_the_log_is_stored` (stderr on `/dev/full`: status 1, the totals on stdout and three chapters stored).
  - `bad_range_calls_no_model` (`--chapters 3-9` with an empty replay directory: status 1, stderr contains `1-4`, no database and no cache directory appear).
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: fails (the placeholder binary knows no command).
- [ ] **Step 3: Implement** `main.rs`, `paths.rs`, `clock.rs` and `extract.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --bin haiyaku --test cli`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`, the check clap has to pass. Expected: all pass.
- [ ] **Step 5: Commit**: "A haiyaku binary whose extract lists a book's contents or extracts a range into the database".

### Task 9b: The binary: `show`

**Files:**
- Create: `crates/haiyaku/src/bin/haiyaku/show.rs`
- Modify: `crates/haiyaku/src/bin/haiyaku/main.rs`
- Test: `crates/haiyaku/tests/cli.rs`

**Interfaces:**
- Consumes: `store::{read_book, sha256_file}`, `fold::fold_book`; Task 9a's paths, opened database and errors; `keeper_epub`, `keeper_replies`.
- Produces: `haiyaku [--db <path>] show <book.epub> [--at <chapter>]`.

Rules (Task 9a's rule 1, the paths, and rule 5, the errors, hold here too):
1. **`show`** finds the book by `sha256_file`; one the database does not hold is an error saying it has not been extracted. So is a database with no haiyaku schema (`store::schema_version` is `None`), which `show` finds out before `migrate` runs, so it writes no tables into an empty file or another program's database; one at an older schema version is still migrated, since `show` reads the current schema. `--at n` is the cast after chapter `n` (`fold_book(&log, n)`), by default after the last; 0 is allowed and empty; past the last is an error naming the range. Output: a `Characters` section, a line each with the name, the aliases in parentheses and `ch <first>-<last>`; a `Relations` section, a line per fact, `A -> B` for a directed fact and `A - B` otherwise, then the kind and the note. `--at`'s help says that chapters count from the first entry extracted, not from the table of contents.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs`, each set up as Task 9a's are:
  - `extract_then_show`: `extract --chapters 1-3 --backend replay:<dir>` is status 0; `show` prints "Ada Marsh", "Tom Reed" and `owns`; `show --at 1` prints "the keeper" and "Pip" and neither "Ada Marsh" nor "Tom Reed"; `show --at 0` lists no character.
  - `failed_extract_leaves_database_unchanged` (after a good run, `extract --no-cache --replace` against a replay directory holding one reply is status 1, and `show` prints what it printed before).
  - `show_before_extract_fails` (status 1, stderr says the book has not been extracted); `show_past_the_last_chapter_fails`.
  - `show_writes_nothing_into_a_database_without_haiyaku_tables` (a 0-byte `--db` stays 0 bytes, and another program's database keeps only its own table; status 1, the not-extracted message); `show_missing_book_is_named`.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: the four new tests fail.
- [ ] **Step 3: Implement** `show.rs` and the `show` command in `main.rs`.
- [ ] **Step 4: Run** `cargo test -p haiyaku --test cli`, then `cargo test -p haiyaku --all-features`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`. Expected: all pass.
- [ ] **Step 5: Commit**: "show prints the cast an extracted book has after a chapter".

### Task 10: `show --html`, and the documents

**Files:**
- Create: `crates/haiyaku/src/bin/haiyaku/view.rs`, `crates/haiyaku/src/bin/haiyaku/view.html`
- Modify: `crates/haiyaku/src/bin/haiyaku/show.rs`, `main.rs`, `README.md`, `docs/status.md`
- Test: `crates/haiyaku/tests/cli.rs`

**Interfaces:**
- Consumes: `read_book`, `fold_book`, `fold::Cast` (the same data any consumer gets).
- Produces: `haiyaku show <book.epub> --html <file> [--at <chapter>]`.

Rules:
1. The page is one file that asks no other host for anything: no external script, style, font or image.
2. Its data is one step per `n` from 0 to the last chapter: the chapter's title and counts, and `fold_book(&log, n)` with every character (id, name, aliases, first and last chapter, merged ids) and every relation fact (kind, direction, note, quote, first and last chapter). It is embedded as JSON in `<script id="data" type="application/json">` with every `</` written `<\/`, so a quote cannot end the script.
3. `spike/src/view.html` is the page to rebuild on this data: a chapter slider with previous and next, the graph laid out from the step before, a panel for the selected character with its names and its relations with their quotes, and the character list. `--at` sets where the slider starts.
4. What a mark means (in this chapter or not, selected or not) is carried by fill against outline and by line weight, never by hue alone. A relation's kind is told apart by line pattern, line weight or a label as well as by colour, never by hue alone.
5. The page holds the whole book's cast and quotes: the command's help says it is for checking a run, not for a reader. Write it only where the user names a path.
6. The view belongs to the binary. Nothing under `src/` outside `src/bin/` builds markup; a consumer of the library draws the `Cast` its own way.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs`, each set up as Task 9a's are:
  - `show_html_writes_a_self_contained_page`: after `extract_then_show`'s extraction, the file exists; it has none of `src="http`, `href="http`, `url(http`, `@import`, `fetch(`; the text between the data script's tags parses as JSON with four steps; step 1's names lack "Ada Marsh" and step 3's have it.
  - `html_data_cannot_close_its_script`: a book written with `store::write_book` straight into the test's database (`common::keeper()` with its `sha256` set to that of a small file the test writes, and the quote of one of its `Relation` events changed to hold `</script><p>`, since the `Cast` carries quotes on relation facts only): the data still parses as JSON and holds that quote.
- [ ] **Step 2: Run** `cargo test -p haiyaku --test cli`. Expected: the two new tests fail.
- [ ] **Step 3: Implement** `view.rs` and `view.html`. Write the page for `keeper_epub` to `<workspace>/keeper.html`, which is not committed; the controller opens it in the browser, steps through chapters 1 to 3 and records the check in the task ledger.
- [ ] **Step 4: The documents.** `README.md`'s status paragraph: extraction and the program exist, with the two commands. `docs/status.md`: Where it stands (the reader, the backends, `extract` and `show`); Next (plan 3 comes first); O2 gains the backend conveniences left out here (presets, backends from a settings file, a command as a backend, waiting for a server). It also gains an O-item: the Claude timeout kills only the process it started, so a launcher that does not exec (a version-manager shim) leaves the real call running. The fix, the call in a process group of its own and Ctrl-C caught in the binary to stop that group, needs a signal crate or unsafe code, which the workspace forbids, and so a new dependency that these Global Constraints do not list. The owner's own `claude` execs, so it is not affected.
- [ ] **Step 5: Run** `scripts/check-all.sh >| <scratchpad>/check.log 2>&1; echo "exit $?"`, then `cargo +1.91.0 check --workspace --all-targets --all-features --locked`, which `check-all.sh` leaves out. Expected: every step of `check-all.sh` passes up to `cargo deny`, and the check passes.
- [ ] **Step 6: Commit**: "show --html: the cast chapter by chapter on one self-contained page".

### Acceptance run, before Part B's PR

The controller runs this, not an implementer, and only once the owner has agreed to it: two books at about 12 s a chapter and 1.3 to 1.7 USD each on the Claude login.

- [ ] **Step 1:** `cargo build --release -p haiyaku`, then `haiyaku extract <book>` on the English and the Korean first Harry Potter book in `~/temp/haiyaku-corpus/` to list their contents. Pick the 17 chapters; the excerpt of the second book after chapter 17 stays out.
- [ ] **Step 2:** Extract both with `--db spike/out/plan2/haiyaku.db` (gitignored) and keep each run's stderr in the scratchpad.
- [ ] **Step 3:** Compare with `docs/notes.md`, "Spike, 2026-11-20: Sorcerer's Stone in English and Korean". Pass means all of:
  - all 17 chapters extracted in both, with a rejected share of at most 3% in English and 5% in Korean (the spike: 7 of 525 and 17 of 543). The share is rejected entries (whole items and single names) over kept items plus rejected entries, since `kept` counts items and `rejected` counts items and names; the notes state it that way;
  - `show` lists no two characters with the same full name, and the English cast is within a tenth of the spike's 77 characters: a character the prompt did not list (not named in the chapter, not seen for two chapters) can come back under a new description as a second character, and a count well above the spike's is how that shows;
  - the two reveals of chapter 17 are in `show --at 17` and not in `show --at 16`;
  - the boy first met by description carries the description in `show --at 5` and his name in `show --at 6`;
  - Hagrid's creatures are his by `owns`, not `family`;
  - in Korean, the mother introduced in chapter 6 has her family relations in `show --at 6`.
- [ ] **Step 4:** Add a section at the top of `docs/notes.md` with the two runs' time, kept, rejected and cost and the outcome of each check: numbers and character names only, no line of the books. A failed check is a finding against Task 7a, 7b, 8a or 8b: fix it there and run the book again. A fix there changes the roster or the prompt of later chapters, and with them their cache keys, so budget a full run (1.3 to 1.7 USD a book) for each fix; the controller asks the owner before each rerun.
- [ ] **Step 5:** Review, fix, push, PR.
