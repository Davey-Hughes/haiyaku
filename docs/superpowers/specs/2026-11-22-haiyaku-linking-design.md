# Linking a series: the link step, and the commands that load, link, export and show a series

Date: 2026-11-22. Status: draft for the owner.

## Why

The owner reads a series as one story. A reader of Chamber of Secrets
should meet Sorcerer's Stone's characters as the people they already know,
and see nothing of a book they have not reached. Each book is extracted on
its own, with ids that mean nothing outside it (the library design,
approach A). A link records that a character of a later book is the same
person as a character of an earlier book, from a point in the later book,
for one reading order.

The core plan built everything that reads links: the series, order and
link tables, `put_series` and `put_links` with their checks, and the
series scope of `cast_at`, which joins two characters once the reader has
finished the chapter a link holds from. Nothing makes a link yet, and the
binary cannot load a series, link it, export it or show it.

The library design links two characters whose given name and surname are
equal without asking the model ("Series and linking", rule 3). Goblet of
Fire breaks that rule: its chapters 32 and 33 call Voldemort's father Tom
Riddle in full, and by the end of Chamber of Secrets Tom Riddle is
Voldemort. An equal full name does not prove two characters are one person.

This is the second part of plan 3. The first, reveals, came first because
extracting a book again gives its characters new ids and drops its links.

## Goals

1. Load a series from a file the owner writes by hand.
2. Link the books of each reading order. Every possible match the name
   rules leave goes to the model, one call per later character, and a link
   counts only on a quote the program finds in what it sent.
3. Make no wrong link in the acceptance run. A character the step cannot
   link shows as two characters instead.
4. Export a series, with its books, to one sidecar and import it, so that
   taiyaku's server, built without the `extract` feature, can take a linked
   series in one step and never pair links with logs they were not made
   against.
5. Show the cast at a point in a series, as text and as the checking page.
6. Link Korean books as well as English ones.
7. Keep linking near 0.3 USD a book in a language with Claude Sonnet.

## Non-goals

- Correcting links by hand. The series file names books and orders, never
  characters.
- Linking two editions of one book (the library design: not in v1).
- A later book overturning a relation of an earlier book that the later
  book never states itself. Its second call sees only its own book's
  relations.
- Fixing faults of extraction that the acceptance run finds, such as one
  character made of two people. They become open items with pull requests
  of their own.
- How taiyaku finds and imports a series. That is a taiyaku design.

## Anti-goals

The change has failed, even if it runs, when:

- a link joins two different people, as the spike's first link step joined
  Tom Riddle to Tom the barman;
- a link shows before the reader has finished the chapter of the later book
  whose text the link rests on;
- a link rests on a quote the program did not find in what it sent the
  model.

## Constraints

- A link names a character by its book's id. `write_book` drops a book's
  links when its log changes, which extracting it again does, and
  `put_series` drops every link of a series it replaces. Linking is
  therefore a step the owner runs again after either, and it must be cheap
  to repeat.
- `put_links` refuses two links for one later character, a later book that
  does not come after the earlier one in the order, an `at` outside the
  link's chapter, a later character not introduced by the end of that
  chapter, and an earlier character its book never introduces. The step
  must produce links that pass. It is given the logs the links were made
  against, one for each book of the order in its sequence, and stores
  nothing unless the database still holds each of them and the order
  still lists their books in that sequence, so a book extracted again or
  a series stored again while the step runs costs a rerun and never a
  link to a character that now has the id, or one that skips a book.
- The fold applies a link at the end of its chapter (`cast_at`), so the
  chapter is what a reader sees. The block is kept for a cut-off by
  paragraph later.
- The reply cache keys a call by its whole request. The link prompt must
  come out byte for byte the same from the same logs, so candidates and
  quotes go in a fixed order.
- taiyaku's server builds without `extract`. Export and import of a series
  live in `store`, outside that feature. The link step calls a model and
  lives behind it.
- The library builds no markup. The page stays the binary's.
- Book text stays out of the repository. Runs on the owner's books write
  under `spike/out/`, which is gitignored.
- One new dependency: the `toml` crate, under the `cli` feature, for the
  series file.

## Approaches

Two choices shaped the link step.

**Which matches go to the model.** (A) As the library design has it: an
equal given name and surname links at once, and only weaker matches go to
the model. This is the cheapest; for Chamber of Secrets the spike asked
about 8 pairs for 0.04 USD. A namesake links unchecked. (B) Every possible
match goes to the model, equal full names included, and the name rules only
find candidates and turn down conflicting ones. About 50 calls a book. The
owner chose B: a missed link shows a character twice, while a wrong one
states something the book does not say.

**How the calls are grouped.** (1) One call per later character, listing
all its candidates: the model compares them side by side, a bad reply
loses one character's link, and each reply caches on its own. (2) One call
per pair: the simplest prompt, but two to three times the calls, and the
model can answer "same" for two candidates, which then needs a rule to
break the tie. (3) One call per book: the fewest calls, but a malformed
reply loses the whole book's links, fifty answers in one reply are harder
to check, and the cache keeps it only whole. The owner chose 1.

**What a series sidecar holds.** A book's ids start again at `c1` each
time it is extracted, so links exported before a book was extracted again
name ids that now belong to other characters, and every check of
`put_links` still passes. (A) The series sidecar carries its books: every
book's log, the series and its links, from one state of the database.
Links and logs travel together; the file is bigger, and the schema stays
as it is. (B) The sidecar holds the series, the links and a stamp per book,
and import refuses it unless every stored book has the same stamp: a small
file, but a schema change, and its books' sidecars must be imported first.
(C) Keep the links alone and name the mismatch a known weakness. The owner
chose A.

## Design

### The series file

A TOML file the owner writes, kept wherever suits them. Book paths are
relative to the file's own folder unless absolute.

```toml
name = "homer"
title = "Homer"

[books]
iliad = "The Iliad.epub"
odyssey = "The Odyssey.epub"

[[order]]
name = "composition"
books = ["iliad", "odyssey"]
```

`name` is the series' key in the database; `title` is shown. A book's
label exists only in the file: the database knows a book by its SHA-256.
The first `[[order]]` is the default. Unknown keys, a label an order uses
but `[books]` lacks, and the errors `put_series` returns are refused with
a message naming the file.

### haiyaku series <file>

Reads the file, hashes each book with `sha256_file`, and stores the series
with `put_series`, in place of a series of the same name. A book that has
not been extracted into the database is an error naming its path. A
database that is not there holds no book, so the command makes none and
names the first book of the file. When the database holds the file's
series already, with its title and its orders, the command stores
nothing, so the series keeps its links, and says it is unchanged. When
the series it replaced had links, it says how many are gone, as
`put_series` counts them in the transaction that drops them, and that
`link` makes them again.

### The link step

A `link` module in the library, behind `extract`. It links one order;
for each book after the first, in the order:

1. **The earlier characters.** The books before this one, each folded to
   its end and joined by the links already made for them, which the step
   holds in memory until it stores the order. Each is a character of that cast: its key, its shown name,
   every other name it carries, its own and those of the characters merged
   into it by a same-as or a link, and the quote of its introduction. The
   reader has finished these books, so none of this spoils.
2. **The later characters.** Each entity the book introduces, taken alone,
   with its names in the order of its `introduced`, `named` and `alias`
   events. A same-as inside the later book is left to the fold, which
   applies it at its own chapter: a link made through one character must not
   borrow a name another character gets only later in the book.
3. **Candidates for a name.** With titles and a leading "the" set aside,
   an earlier character is a candidate for a later character's name when
   one of its names shares a given name or a surname with it, or equals it
   whole (a description, a nickname). A candidate is turned down without a
   call when both characters have given names and neither ever uses one of
   the other's, likewise for surnames, counting the later character's names
   up to the end of the name's chapter (the spike's v2 rule, which keeps
   Lucius apart from Draco), or when the two names' titles state different
   sexes (Mr. and Mrs. Weasley). It is turned down too when the later book
   relates the later character, at any chapter, to another of its
   characters that the step has already linked to the candidate: linked as
   well, one character would be its own relative. A character the book
   joins by a same-as with either of the two counts as that one, so a
   relation recorded on the later character's other id, or a link made
   for the relative's, turns the candidate down too. The whole book is
   extracted before it is linked and a turn-down shows a reader nothing, so
   a relation or a same-as the book records after the name counts as well.
4. **The call.** The first name of the later character that has candidates
   gets one call listing them all. The model is sent the later character's
   names given by the end of that name's chapter and its quotes from the
   later book up to the end of that chapter, never later; and for each
   candidate its book, its names and its introduction's quote. It answers
   with one candidate or none, and a quote from the later book.
5. **The link.** The answer counts when it names a listed candidate and
   its quote weighs at least `MIN_QUOTE_WEIGHT` and is found, as
   `locate_quote` finds a quote in a chapter, in one of the later
   character's quotes that the call sent. A name alone, which is all a
   `named` or `alias` event's quote holds, is too light to count. The step
   then links the later character to that candidate's key, method `model`
   with the quote, at the position of the name that made the call.
   Otherwise the character's next name that brings a candidate not asked
   about yet gets a call of its own, from its chapter. That call lists
   every candidate its name brings, those an earlier call listed included,
   so the model chooses among all of them; a link holds from the name of the
   call that made it, never from an earlier one. A later character gets one
   link at most. This also settles the spike's defect 7 (notes.md, "Spike
   review"): the step keeps the answers, not only the pairs asked about.
6. **The next book.** Its earlier characters include this book's, joined
   through the links just made.

Every link the step makes is a `model` link. The `name` method stays in
the schema and in `read_links`; the step makes none.

A call that fails, or a reply that does not fit the schema, stops the step
before anything is stored for the order. The replies so far stay in the
cache, so the next run asks only about the rest. A quote that is not found
is no failure: that character stays unlinked, and the report says why.

The step returns the links of the order and a report: for each later
character it asked about, the chapter, the candidates, the answer, the
quote, and whether it linked or why not; and the calls' time and cost.

### haiyaku link <file> [--order <name>] [--backend <spec>] [--no-cache]

Links every order of the series, or the one `--order` names, and stores
each order's links with `put_links`, which replaces those it had. It
prints the report. It links the series as stored, and refuses when the
file gives another title, or other books or orders, than the database
holds, asking for `haiyaku series` first; it asks for that too when
there is no database, and makes none. An order one of whose books is
extracted or imported again while it is linked stores nothing, and the
run fails naming that book and asking for `haiyaku link` again. So does
an order that `haiyaku series` stores again with other books, or the
same in another sequence, while it is linked; the run says the series
was stored again.
`--backend` and `--no-cache` work as they do for `extract`, with the
same cache.

### Export and import

`haiyaku export <file>` writes a sidecar. For an EPUB it is the book's,
`<stem>.haiyaku.db` beside it (`export_book`). For a series file it is
the series', `<name>.series.haiyaku.db` beside the file: every book its
orders hold, the series, its orders and their links, all read from one
state of the database. Links travel with the logs they were made against.
The `.series` keeps a series named as a book's file apart from that
book's sidecar, which is often in the same folder.

`store` gains `export_series` and `import_series`, outside `extract`.
The series sidecar has the database's own schema, foreign keys included,
and migrates on import as a book's does. Import stores each book in place
of the stored log, under `write_book`'s rules, then the series as
`put_series` does and each order's links as `put_links` does, in one
transaction: on any error the database is as it was. Where a book's log
differs from the stored one, `write_book` drops the stored links that
name it, and the sidecar's links take their place. A book sidecar imported
afterwards that differs from the series sidecar's log drops that book's
links in turn, so a mismatch loses links and never makes a wrong one.

### haiyaku show <book.epub> --series <file> [--order <name>] [--at <chapter>] [--html <page>]

With `--series`, `show` prints the cast in series scope: every book
before this one in the order, whole, then this one to `--at`, joined
through the links. `cast_at` takes a reader's document and folds only
stored links, through a private `fold_in`, and `finished_chapters` never
counts a chapter that ends the book's spine as finished. The library
therefore gains a series fold by chapter number over logs and links passed
in, which the link step (step 1) and `show --series` both use and which
`cast_at` can share. A character first met in an earlier book shows that
book and chapter. The page steps chapter by chapter from the start of the
order's first book to the last chapter extracted of this one, and shows
each link with its chapter and quote. The series, the logs and the links
are read in one transaction, so they are of one state of the database.
Without `--series`, `show` works as it does now.

### The library design

Rule 3 of its "Series and linking" changes: an equal given name and
surname makes a candidate, not a link. Its "Storage" changes too: a series
sidecar carries its books, so nothing has to be imported before it. Both
sections point here, as they point to the reveals design for the parts
that design changed.

### Tests

Fixtures are invented, and the model's replies are recorded with the
`Replay` backend. The tests pin at least: two namesakes, father and son,
that stay apart when the reply says so; a candidate turned down by
conflicting given names, one by the sex of a title, and one already linked
to a relative of the later character, with no call, while two later
characters that no relation joins both link to one earlier character; a
link through an alias that holds from the alias's chapter; a reply whose
quote is not in the prompt, which links nothing; three books, where the
third book's character links once and the cast shows one character; a
failed call that stores nothing; the link prompt byte for byte, as the
extraction calls' golden files do; the series file's errors; a series
sidecar's export and import in a build without `extract`, with a book
whose stored log differs from the sidecar's, and a book sidecar imported
afterwards that drops that book's links; and `show --series`.

## Acceptance

The controller runs it after the tasks, on the owner's go-ahead, into
`spike/out/linking/`, one database a language.

Sorcerer's Stone is extracted again through the reply cache, with the
reveals run's entries (English 4 to 20, Korean 5 to 21), which should cost
next to nothing. Chamber of Secrets and Goblet of Fire are extracted in
both languages; their story chapters are picked from the table of
contents, leaving out any excerpt of the next book at the end. Each
language gets a series file with one order: Sorcerer's Stone, Chamber of
Secrets, Goblet of Fire. Prisoner of Azkaban is left out to save its cost,
so a reader of this order meets its new characters as new in Goblet of
Fire unless the first two books knew them.

In both languages:

1. Tom Riddle joins Voldemort at Chamber of Secrets chapter 17 and not
   before.
2. Tom the barman stays apart from Tom Riddle and from Voldemort. Lucius
   Malfoy stays apart from Draco.
3. Goblet of Fire's Tom Riddle, Voldemort's father, stays apart from
   Voldemort at the end of the book.

Items 1 to 3 judge the link step. Extraction can make the same join inside
a book on its own: the roster gives a new full name to the character that
already holds it (`Roster::holder`). A join the extraction makes is
recorded as an open item for a pull request of its own and does not fail
this one; a join the link step makes fails it.
4. Every link in the report is right, checked by hand. Each character both
   books list that stays unlinked is recorded with the reason. For Chamber
   of Secrets in English, the count is set beside the spike's 49.
5. The report gives linking's cost a book in a language, against goal 7.
6. Recorded, with no pass or fail: whether Goblet of Fire's extraction
   makes Moody and Barty Crouch Jr. one character, and what the step does
   with Peter Pettigrew, Wormtail and Scabbers with book 3 left out.

The run should cost about 21 to 22 USD: about 6 for Chamber of Secrets and
14 for Goblet of Fire, in both languages at the reveals run's cost a
chapter, and about 1.5 for linking. The controller asks the owner before
spending it.

## Decided by the owner

- Every possible match goes to the model, equal full names included
  (approach B), 2026-11-22. It replaces rule 3 of the library design.
- One call per later character, listing all its candidates (approach 1),
  2026-11-22.
- The acceptance run extracts Goblet of Fire as well as Chamber of Secrets,
  in both languages, 2026-11-22.
- A series sidecar carries its books' logs with the series and its links
  (approach A), 2026-11-22.
- A candidate already linked to a relative of the later character is
  turned down before the call (step 3), after the acceptance run,
  2026-11-23.
- From the library design: books extracted alone and linked afterwards,
  per reading order; a series file written by hand; the commands
  `series`, `link`, `export` and `show` for a series; Korean as well as
  English.

## My calls

These are mine; the owner may change any of them.

- TOML for the series file, in the layout above.
- The series commands take the series file's path; `show` takes it as
  `--series`.
- `link` with no `--order` links every order.
- The candidate and conflict rules of step 3, from the spike's v2.
- What the model is sent (step 4), the names of characters merged into a
  candidate included, so it can see that a candidate is also Voldemort.
- A later character is linked on its own names; a same-as inside its book
  is the fold's to apply.
- A link holds from the name that brought its candidate.
- A failed call stops an order's linking and stores nothing; the cache
  resumes it.
- Faults of extraction that the run finds go to pull requests of their
  own.
- Prisoner of Azkaban is left out of the acceptance order.

## Left to the builder

- The prompt's wording and layout, and the reply schema.
- How many of a later character's quotes the call sends, and which,
  within step 4's rule.
- The names and signatures of the link module's and the series sidecar's
  functions.
- The text of the report and of `show`, and the page.
- Error messages.

## Known weaknesses

- A link is as good as the extraction's characters. Where one character is
  two people, as an impostor makes, the link carries that join into later
  books.
- A description matches only an equal one. A later book's "Harry's aunt"
  finds no candidate and shows as a second character.
- The quote check proves that the quote was in the prompt, not that it
  shows the two are one person.
- A later book cannot mark an earlier book's relation as overturned unless
  it states the relation itself.
- Each change to a book's log, or to the series, loses its links until
  `link` runs again.
- The cache keys a call by the backend's alias, not the model that answered
  (O10).
