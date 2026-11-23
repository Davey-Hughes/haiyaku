# haiyaku library design

Date: 2026-11-21. Status: approved by the owner.

## Why

The owner reads long series (Homer; Harry Potter in English and Korean) in
taiyaku, their own reader, and wants to look up who a character is without
being spoiled. haiyaku builds that lookup. It extracts, once and ahead of
time, a log of what a book says about its characters, and folds that log up to
the reader's position. Nothing past the position may show.

## Goals

1. Extract a whole book's character log ahead of time on the desktop, with
   Claude Sonnet by default.
2. Ground every fact: each event carries a quote from the book, and the
   program checks the quote against the text.
3. Link the books of a series afterwards, per reading order, without
   extracting any book again.
4. Answer "what does the reader know here": the characters, their names,
   aliases and relations up to the last chapter the reader finished. Earlier books of the series count
   by default; an option limits the answer to one book.
5. Store logs in a structured SQLite schema, through a connection the caller
   passes in. A sidecar file beside the book carries a log from the desktop
   to taiyaku.
6. Work as a library that taiyaku calls, with a command-line binary that runs
   extraction and linking.

## Non-goals for this design

- Subtitles and shows, manga OCR, and taiyaku's screens. The log must not rule
  them out.
- A correction UI. The log reserves merge, split and rename events for one.
- Places, factions, terms and wiki-style pages. Every entity has a kind, so
  they can be added to the same log.
- Extraction on taiyaku's server (O4). The backend list stays open to an
  API-key backend.
- How taiyaku finds, imports and shows a log. That is a taiyaku design, made
  in taiyaku's repository once this library exists.

## Anti-goals

haiyaku has failed, even if it runs, when it:

- shows anything the reader has not reached. A wrong link counts: the first
  link step joined Tom Riddle to Tom the barman, which showed the barman as
  Voldemort at the end of the second book;
- states a fact the book does not support;
- puts book text in the repository. Fixtures are invented or public domain,
  and runs on the owner's books write only to gitignored directories.

## Constraints

- Rust, edition 2024, toolchain 1.100.0, GPL-3.0-or-later, and taiyaku's
  gates: unsafe code forbidden, clippy with `-D warnings`, cargo-deny
  advisories.
- taiyaku links rusqlite 0.43 with bundled SQLite. A build can hold only one
  libsqlite3-sys, so haiyaku's rusqlite must be the same minor version as
  taiyaku's.
- taiyaku's database lives only in its NAS container, and library commands
  run on that host. Extraction runs on the desktop, so a log reaches taiyaku
  as a file.
- taiyaku records a reading position in a book as a block: the n-th heading or
  paragraph, in spine order then document order (taiyaku-read's `Block`,
  identified by document path and ordinal). haiyaku's positions must map onto
  these blocks.
- haiyaku's own files respect every XDG variable, with the usual defaults.
- Measured with Sonnet (docs/notes.md): about 12 s a chapter, 200 s and
  1.3 to 1.7 USD a book.

## Approaches

Two ways to handle a series were tested on Chamber of Secrets
(docs/notes.md, 2026-11-20 and 2026-11-21):

- **A. Extract each book alone and link afterwards.** The prompt sees only
  the book at hand. A link step matches each character of a later book to an
  earlier one by name, refuses conflicting names, and asks the model about
  weak matches. A book's log never depends on the series or the order, so a
  new reading order costs a re-link, not an extraction: 8 model calls and
  0.04 USD for the second book.
- **B. Carry the earlier books' cast into the prompt.** Linking happens during
  extraction. B caught two minor characters that A's extraction never listed,
  but a book's log then depends on the books read before it: each reading
  order needs its own extraction, and the second book cost 17% more.

The owner chose A. The second link step matched B on all 49 characters the
standalone extraction produced, with no wrong links.

For storage, three layouts were weighed:

- One JSON document per book: a copy of the sidecar, but SQL sees nothing
  inside it.
- One row per event, with the shared fields as columns and the rest as JSON:
  SQL can cut at a chapter, but half of each event stays opaque.
- Fully structured: a parent table of events in reading order and a child
  table per event type with real columns. It costs a migration per new event
  type and more mapping code, and gives every field to SQL, with foreign keys.

The owner chose the structured schema and asked that the design start from
the database rather than from a sidecar file.

## Design

### One crate

A single crate, `haiyaku`, is both the library and the binary. Its modules:

| Module | Holds |
| --- | --- |
| `book` | the text model and an EPUB reader |
| `log` | the events, the log file format and the fold |
| `extract` | per-chapter prompts and the checks on what the model returns |
| `llm` | the model backends |
| `series` | series files, reading orders and the link step |
| `store` | the database schema, its migrations, and sidecar export and import |

taiyaku's server only reads logs and folds them. A feature split keeps the
backends, the HTTP client and the EPUB reader out of its build: taiyaku uses
`default-features = false`; the binary uses the defaults.

### The text model

A `Book` is what extraction reads: its spine documents in order, each a
list of blocks (heading or paragraph text), and its chapters, each a run of
whole documents labelled from the table of contents. The model reads one
chapter per call, as in the spike.

haiyaku's EPUB reader skips head, script, style, rt and rp and drops soft
hyphens, so its text compares cleanly with taiyaku's. It does not copy
taiyaku-read's block numbering. taiyaku-read has its own walker (about 30
block elements, printed verse numbers removed) and saves a position as one
block index across the book. Two walkers kept in step by hand would drift,
and a drift would put the reader's position past where they are.

A position in the log is haiyaku's own: the spine document's path and the
block's ordinal in that document. The log also records the SHA-256 of the
EPUB file and its spine documents in order, each with the chapter it belongs
to, if any. Since the fold cuts at chapters (see The fold), a caller only has
to say which spine document the reader is in: chapters are runs of whole
documents, so the document fixes the chapter in any reader's numbering.

Spine documents before the first table-of-contents entry belong to no
chapter and hold no events; a reader there sees an empty cast.

### The log

A book's log is a record of the book and its events in reading order, kept
in the tables under Storage. The book record holds the EPUB's SHA-256, title,
language, the chosen chapters, the backend and model, a prompt version and
the date, and per chapter the time, cost and counts of kept and rejected
items.

Every event has `at`, the block holding the start of its quote, and
`quote`, the evidence, checked against the chapter text. The bodies:

| Event | Meaning |
| --- | --- |
| `introduced{id, kind, name}` | a new entity; `kind` is `character` for now |
| `named{id, name}` | the text now gives a fuller name ("the pale boy" becomes Draco Malfoy) |
| `alias{id, alias}` | another name or form of address |
| `appeared{id}` | the entity is present in this chapter |
| `relation{from, to, kind, note}` | a fact about two entities |
| `overturned{from, to, kind, note}` | a later chapter shows a relation was never true; see [the reveals design](2026-11-22-haiyaku-reveals-design.md), "The overturned event" |
| `same_as{id, other}` | two entities turn out to be one |
| `merge`, `split`, `rename` | reserved for hand corrections; v1 never writes them |

The program assigns ids; it never takes one from the model, which named the
pale boy `draco-malfoy-boy` a chapter before the text named him. An id is
unique within its book.

The relation kinds are the spike's ten plus `owns`: the spike filed Fluffy
and Norbert under family and Scabbers under serves.

The items the program rejects, and the model's raw replies, go to
`$XDG_CACHE_HOME/haiyaku/`, not into the log.

### Extraction

Extraction works as in the spike: the model told that the reader has read to
the end of the chapter, every item checked before it enters the log. Each
chapter takes two calls, the spike's and a second for what the chapter
reveals and overturns; see [the reveals
design](2026-11-22-haiyaku-reveals-design.md), "The second call". A quote
passes when its parts, reduced to letters and digits, appear in order in the
chapter text. The program then finds the block where the quote starts and
stores it as `at`. A run resumes after the last chapter it finished.

The owner picks, once per book, which table-of-contents entries are the
story. `haiyaku extract` without a range lists the entries and stops; with
`--chapters 3-19` it extracts those, and the range is recorded with the
book. Introductions, glossaries and the excerpt of the next book stay out:
an extracted introduction would put the plot's events before chapter 1.
Text outside the range holds no events.

What changes from the spike:

- The model labels the parts of each name and alias: given name, surname,
  title, or a description ("the pale boy", "엄마"). Every name in the log
  carries its labels. Linking and display names read the labels instead of
  guessing from English spelling, so the same rules hold for Korean, where
  the title follows the name (포터 씨, 위즐리 부인) and nothing is
  capitalised, and for Homer, whose characters have no surnames.
- The prompt carries only the relevant part of the roster: the characters
  whose names, or the surname labelled in one of them, occur in the chapter,
  plus those seen in recent chapters. A surname counts only when it is longer
  than one letter or syllable and, in a script with case, written in its own
  case. By chapter 17 the spike sent all 77 characters and 92 pairs.
- When a chapter gives a known character a fuller name, extraction writes a
  `named` event. The fold shows the best name known at the position: a full
  personal name over a partial one, a partial one over a description.
- The shortest quote allowed depends on the script; the spike rejected five
  sound Korean quotes as too short.
- A character whose own quote fails no longer takes its relations with it
  (Mrs. Weasley lost five relations in Korean chapter 6).

### Series and linking

The owner describes a series in a small hand-written file: its books by file
name, and one or more reading orders, the first being the default. The
binary loads it into the database and fills in each book's SHA-256.

The link step runs once per reading order. For each book after the first, it
matches each character against the characters of the books before it in that
order, with the second spike's rules:

1. Compare names by their labelled parts, without titles; a title's sex
   (Mr. and Mrs., 씨 and 부인) keeps Mr. Weasley apart from Mrs. Weasley.
2. Refuse a pair whose given names differ, or whose surnames differ. The
   linking design ("The link step", step 3) also refuses a candidate
   already linked to a relative of the later character.
3. An equal given name and surname makes a candidate, not a link: every
   possible match goes to the model (see [the linking
   design](2026-11-22-haiyaku-linking-design.md), "The link step").
4. Send every candidate the rules leave to the model. The first of the later
   character's names that has candidates gets one call listing them all;
   until a call links, each later name that brings a candidate not asked
   about yet gets a call of its own, listing every candidate that name
   brings, those an earlier call listed included. What a call sends, and
   when its answer counts, are in the linking design ("The link step", steps
   4 and 5). An answer that names a candidate counts only if its quote is
   found in what the model was sent.

A link is a record: the later book's id, the earlier book's id, how it was
made (name, or model with its quote), and `at`, the block of the later book
from which it holds. A link found at a character's introduction holds from
there; one found through an alias that turns up later holds only from that
alias's block. Under the chapter cut-off, a link counts once the reader has
finished the chapter holding its `at`. Links are rows in the database, per
reading order.

### The fold

The library's question is: given a series, a reading order, a book and a
block in it, and a scope, what is known? With the series scope, the default,
it folds every book before this one in the order to its end, then this book
up to the block, joining ids through the links whose `at` the reader has
passed. With the book scope it folds only this book.

The answer lists each character with the best name so far, aliases, where it
was first and last seen, and the ids merged into it; and each pair of
characters with its relation facts (kind, note, first and last seen, quote,
and the chapter, note and quote of the overturn that marks a fact a later
chapter showed was never true; see [the reveals
design](2026-11-22-haiyaku-reveals-design.md), "The fold"). The quotes let
taiyaku show why it says what it says.

The cut-off is the chapter: the fold takes the events of the chapters the
reader has finished, never of the one they are in. A reader in a document
before the first chosen chapter sees nothing from this book; one after the
last sees all of it. Events keep `at` so a cut-off by paragraph can come
later. It needs more work to be safe: the model is told the reader has
finished the chapter, and a common phrase can match earlier than the passage
that supports the fact.

The fold runs where the log is stored, on taiyaku's server. Sending a whole
log to the browser would put the rest of the book within the page's reach.

### Storage

The database comes first. haiyaku owns its tables, all named
`haiyaku_...`, and their numbered migrations. It keeps its own schema
version and migrates through a function the caller calls with its
`rusqlite::Connection`; taiyaku calls it from its own migration step and
keeps the tables in its one database.

| Table | One row per | Main columns |
| --- | --- | --- |
| `haiyaku_book` | extracted book | SHA-256, title, language, chapter range, backend, model, prompt version, date |
| `haiyaku_document` | spine document | book, ordinal, path, chapter |
| `haiyaku_chapter` | chosen chapter | book, number, title, time, cost, kept and rejected counts |
| `haiyaku_entity` | character (later place, faction, term) | book, id, kind |
| `haiyaku_event` | event, in reading order | book, sequence, chapter, position, type, quote |
| `haiyaku_name` | `introduced`, `named` or `alias` event | event, entity, text and its labelled parts |
| `haiyaku_appearance` | `appeared` event | event, entity |
| `haiyaku_relation` | `relation` event | event, from, to, kind, note |
| `haiyaku_same_as` | `same_as` event | event, entity, other |
| `haiyaku_overturn` | `overturned` event, from schema version 2; see [the reveals design](2026-11-22-haiyaku-reveals-design.md), "The overturned event" | event, from, to, kind, note |
| `haiyaku_series`, `haiyaku_order`, `haiyaku_order_book` | series, reading order, book in an order | series, order name, rank, book |
| `haiyaku_link` | link between books | order, later book and id, earlier book and id, how, quote, position |

Foreign keys tie each child row to its event and each entity reference to a
row of `haiyaku_entity`, so a relation cannot point at a character the book
never introduced. The fold reads `haiyaku_event` for the books in the
order, cut at the chapter, in sequence order, and joins the child tables. A
new event type is a new child table and a new migration.

The sidecar is the same schema in a file of its own: `<stem>.haiyaku.db`
beside the EPUB, following taiyaku's sibling-file convention, holding one
book. A series exports to its own file beside its series file, which
carries its books' logs with the series and its links, so nothing has to be
imported before it (see [the linking
design](2026-11-22-haiyaku-linking-design.md), "Export and import").
Importing migrates the file to the current schema version and then copies
its rows, so there is no second format to keep in step. A book is matched
by SHA-256, never by path; a sidecar whose hash differs from the EPUB's is
refused.

On the desktop the binary keeps its own database,
`$XDG_DATA_HOME/haiyaku/haiyaku.db` unless `--db` names another.
Extraction and linking write there, and `export` writes the sidecars.

haiyaku's rusqlite is the same minor version as taiyaku's. The library does
not turn on rusqlite's `bundled` feature; the binary's default features do.

### Model backends

haiyaku builds its own backend module, grown from the spike's: the Claude
CLI (the default, Sonnet), Ollama, and any OpenAI-compatible server, all
asked for JSON that fits a schema. It shares no code with subcheck, whose
backend returns plain text with no schema and lives in subcheck-core, which
pins rusqlite 0.41 and would break taiyaku's build. The builder reads
subcheck's backend (`core/src/llm.rs`, `core/src/llm/server.rs`) for what
it has already worked out: presets, any command as a backend, entries in a
settings file, timeouts and budgets per call, waiting for a local server to
come up, and parallel jobs. That settles O3. An API-key backend for server-side extraction (O4) fits the same list.

A replay backend returns recorded replies, so no test calls a model.

### The binary

Every command works on one database, `--db`, by default the desktop
database above.

- `haiyaku extract <book.epub> [--chapters <range>] [--backend claude]
  [--no-cache]` lists the table of contents, or extracts the range into
  the database.
- `haiyaku series <series file>` stores a series and its reading orders in
  place of the stored series of its name.
- `haiyaku link <series file> [--order <name>] [--backend claude]
  [--no-cache]` links the characters of each order, or of the one named,
  across its books.
- `haiyaku export <book.epub | series file>` writes the sidecar of a book,
  or of a series with its books and links.
- `haiyaku show <book.epub> [--series <series file> [--order <name>]]
  [--at <chapter>] [--html <page>]` prints the cast at a point, alone or
  after the books before it in a reading order, or writes a page that steps
  through it chapter by chapter, as the spike's viewer did, to check a run.

### Tests and gates

- Fixtures are small invented EPUBs, built by test code or checked in, and
  public-domain text where length matters. Recorded model replies for them are
  invented too.
- Unit tests cover the quote check, name comparison, the link rules (Lucius
  against Draco, Tom Riddle against Tom the barman, the Fat Friar against
  Fat Friar, Lavender against Lavender Brown), the same rules on invented
  Korean names (포터 씨 against 포터 부인, 엄마 against 엄마), the fold at a
  chapter, and the choice of display name.
- The schema is tested: export then import gives the same rows, a sidecar
  at an older schema version is migrated before import, and the foreign keys
  refuse a relation to a character that was never introduced.
- Mapping a reader's document to a chapter is tested with documents outside
  the chosen range: front matter, back matter, the next book's excerpt.
- Acceptance, on the owner's books into a gitignored directory: the English
  link run of the first two Harry Potter books matches the second spike (49
  characters linked, none wrong), and the Korean run of the same two books
  makes none of the wrong links the English test guards against.
- The gates are taiyaku's: rustfmt, clippy with `-D warnings` on all
  features and on the defaults, cargo-deny, the tests, and Forgejo CI once
  the crate exists.

## Decided by the owner

- A Rust crate that is a library and a standalone binary; taiyaku first.
- Claude while building; a local model must be possible, not the default.
- Extraction ahead of time, a whole book at once, on the desktop.
- Output to a sidecar file, or a database through a connection the caller
  passes in.
- A series is one story: by default the reader sees everything so far,
  earlier books included; an option shows one book only.
- A series can have several reading orders; "earlier" follows the chosen one.
- Room for more analysis from the same log later.
- Approach A: books extracted alone and linked afterwards.
- Code leaves `spike/` only through this design and a plan.
- The fold cuts at the end of the last finished chapter, not mid-chapter
  (2026-11-21).
- v1 links Korean series as well as English ones (2026-11-21).
- haiyaku builds its own model backends, learning from subcheck's; the two
  share no code (2026-11-21).
- Logs live in a structured SQLite schema: a parent table of events and a
  child table per event type. The design starts from the database, not from
  a sidecar file (2026-11-21).

## My calls

These are mine, not the owner's; the owner may change any of them.

- One crate, with features that keep the backends out of taiyaku's server.
- haiyaku numbers blocks its own way. A caller names the reader's spine
  document, which fixes the chapter. Finding an event's block in the caller's
  numbering, by its quote, waits for a cut-off by paragraph.
- The model labels name parts, which replaces the spike's English title and
  role lists.
- The owner picks the story chapters of each book by hand; the range is
  recorded in the log.
- The program assigns ids; `named` events upgrade display names.
- The relation kind `owns` is added.
- A link holds from a block, so a reveal in the later book stays at its
  chapter.
- The fold runs on the server, not in the browser.
- The sidecar is a SQLite file with the database's own schema,
  `<stem>.haiyaku.db`, so import copies rows and no second format exists.
- The desktop keeps its own haiyaku database; extraction and linking write
  there, and `export` writes the sidecars.
- The table list under Storage.
- Each edition is its own book with its own log: reading the Korean edition
  shows the Korean names. Linking two editions of one book is not in v1.

## Left to the builder

- The id format, file names beyond the sidecar, and the CLI output.
- Column types, indexes and constraints beyond those the Storage section
  names.
- How the relevant part of the roster is picked for a prompt.
- The shortest quote per script.
- How a relation survives when its character's own quote fails.
- The HTML view.

## Still open

O4, server-side extraction, stays open in docs/status.md; nothing here rules
it out.
