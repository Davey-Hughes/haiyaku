# Looking up a name: the parts of a character's names, a look-up over a cast, and the series that hold a book

Date: 2026-11-23. Status: approved by the owner.

## Why

The owner reads long series in taiyaku and wants to tap a name on the page
and see who that character is, as far as the reader knows: nothing from a
chapter not yet finished, and, when the reader wants it, everything from the
books before this one in the series. This is the first thing taiyaku builds
on haiyaku (the owner's choice, 2026-11-23); a Cast panel that lists everyone
comes later.

The owner wants haiyaku to be a library in its own right: taiyaku's needs may
justify an API, but the API must make sense to any caller, and the caller
draws the data. haiyaku 0.1.0 is published on crates.io, and taiyaku will
depend on the published crate, built without the `extract` feature.

Three things are missing for the tap:

- `cast_at` hands out each character's names as plain text. The labels the
  model gave them (given name, surname, title and the sex a title states)
  stay in the log; the fold drops them. A caller cannot tell that
  "Weasley" is a surname that several characters share.
- Nothing finds which characters a piece of text names. The rules that do
  it for extraction and linking (`extract/text.rs`, `link/names.rs`) sit
  behind the `extract` feature, so taiyaku's build has none of them, and
  `locate_name` searches a whole chapter rather than reading from a given
  point.
- Nothing answers which series hold a book. `cast_at` takes a series by
  name, and a reader that has only the book cannot name one.

## Goals

1. A caller holding a cast and the text that starts at a tapped word learns
   which characters that text names, how much of the text the name covers,
   and by which of the character's names.
2. It works for English and Korean as extraction does: a particle after a
   Korean name (해리는), a title after a Korean name (포터 씨), an English
   possessive (Harry's), a title before an English name (Mr. Weasley), a
   surname alone that several characters share.
3. Every character in a cast carries its names with their labels, for the
   look-up and for any caller that draws them.
4. A caller holding a book's SHA-256 learns the series and the reading
   orders that hold it.
5. All of it builds without the `extract` feature and adds no runtime
   dependency.

## Non-goals

- taiyaku's side: mapping a reader's position to a spine document, the
  routes, the reader setting, the card, running haiyaku's migrations on
  taiyaku's database. That is a taiyaku design, in taiyaku's repository,
  once this is released.
- The Cast panel. It needs only `cast_at`.
- Getting logs into taiyaku's database. For now the owner fills a local
  taiyaku database by hand (`haiyaku --db <taiyaku's db>`). Later the owner
  wants taiyaku to run extraction itself on its own connection (O4).
- Joining one character across editions (the English and the Korean books).
  Each edition's books are their own series for now; see "Room for later".
- Japanese word splitting (O9) and one-syllable surnames (O15). The look-up
  inherits both limits.
- Places, factions and terms. The look-up reads characters only.

## Anti-goals

The look-up has failed, even if it runs, when it:

- names a character, or shows a name of one, that the cast it was handed
  does not hold. Taking a cast as its only source is what keeps it
  spoiler-safe: the cast is already cut at the reader's position;
- changes what extraction or linking produce. Moving their rules out of
  the `extract` feature must leave every event, id and link as it was;
- adds a dependency to the build without `extract`.

## Constraints

- Runtime dependencies without `extract` stay rusqlite and sha2 (the core
  plan); no Unicode segmentation or normalisation crate.
- Toolchain 1.100.0, MSRV 1.91, unsafe code forbidden, clippy with
  `-D warnings`, the workspace's gates; CI already builds with
  `--no-default-features --features bundled`.
- 0.x semver: a change to a public struct's fields is a breaking change, so
  this is haiyaku 0.2.0.
- Fixtures are invented or public domain.

## Approaches

Three ways were weighed (the owner picked the second, 2026-11-23):

1. **One call that does it all**: `lookup_at(conn, book, document, scope,
   text)` returns the matching characters. One call a tap, but it works only
   on a database, a caller with a cast from elsewhere (`fold_series`) cannot
   use it, and the Cast panel would need a second entry point anyway.
2. **Two general pieces**: `cast_at` as it is, and a pure function over any
   `Cast` that finds who a text names, with each character carrying its
   labelled names. Each piece is useful alone, nothing in haiyaku knows about
   taps, and the panel later uses `cast_at` alone.
3. **taiyaku matches names itself** from the cast's name texts. No new
   matching code in haiyaku, but the rules for particles, titles after a
   name and surname-only matches would live twice, outside the library that
   labels names.

## Design

### Names in the cast

`fold::Character` keeps each name as a `log::Name`, labels and all:
`name` becomes the `Name` shown (chosen as today: the fullest, by
`Name::rank`), and `aliases` becomes the other names, each once by text, in
the order the text first gave them, to the character or to one merged into
it, as today. Where the same text came with different labels, the entry
keeps the first that has any label (my call). `Name` derives `Eq` so that
`Character` can keep deriving it.

`Cast` gains `books`: the books it folded, in the order they were folded.
A `ChapterRef` names a book and a chapter but has no order of its own, so a
caller that sorts by "last seen", as the look-up does, needs it.

The output types of the fold (`Cast`, `Character`, `Relation`, `Fact`,
`Overturn`) and the look-up's result become `#[non_exhaustive]`, so that a
later field, such as an edition for joined editions, is not another breaking
release (my call). `EntityKey`, `ChapterRef` and `Name` stay open to
construction: callers build them as keys and in logs.

The binary's `show` and its page read the shown name's text where they read
the string today, and print what they print now.

### Which characters a text names

A pure function over a cast; its signature is the builder's. It takes the
cast and the text from the start of the tapped word onward (to the end of the
paragraph is plenty; it reads only as far as the longest name). Finding where
the tapped word starts is the caller's job, as it is for taiyaku's dictionary.

It returns the characters the text names, each once, with:

- how much of the text the match covers (so a caller can mark "Mr.
  Weasley" rather than "Mr."), in bytes of the text;
- which of the character's names matched, and whether as a whole name, its
  given name or its surname.

What matches, using the rules extraction already uses (`locate_name` and
`locate_surname`), applied at the start of the text instead of anywhere in
a chapter:

- **Words.** A word is a run of letters and digits, compared reduced
  (lowercase, ς as σ), so punctuation splits words: "Harry's" reads as
  "Harry" then "s", and "Mr. Weasley" as "mr" then "weasley".
- **A whole name** (the shown name or any alias, of any kind, descriptions
  included) matches when its words are the text's first words, in order.
  Only its last word may run on into a longer word, and only when that last
  word ends in a Hangul syllable, kana or a Han character: that admits a
  Korean particle (해리는, 포터 씨는) and keeps "Ron" from matching "Ronald".
- **Particles, for the look-up only.** After a name ending in a Hangul
  syllable, the look-up admits a run-on only when it is one or two particles
  from a list (은/는, 이/가, 을/를, 의, 에게, 한테, 와/과, 도, 만, 에서, 네, the
  vocative 아/야 and the like), so 론은 and 론아 are Ron but 빌려서 and 빌딩 are not Bill (my call).
  Extraction's rule, which admits any run-on, stays as it is: it searches a
  chapter for names it already expects, where a tap reads whatever word the
  reader touched. After kana or a Han character the run-on rule stays
  extraction's until a word splitter exists (O9).
- **A labelled given name** matches alone by the same rule.
- **One-word descriptions do not match.** A name of one word with no labels
  (엄마, "father", a bare "Professor", O19's junk aliases among them) is left
  out of the look-up; a description of two words or more still matches as a
  whole name. A wrong card above every 아버지 costs more than a missing one
  (my call).
- **A family.** In a script with case, a labelled surname written with "s"
  on, or "es" after s, x, z, ch or sh ("the Weasleys", "the Dursleys'", "the
  Marshes"), matches every character with that surname, the suffix inside
  the match, on the last word of a surname of two words ("the O'Dells"); it counts as one word for "the
  longest match wins" (my call). In Korean the particle list covers it
  (위즐리네).
- **Case, for one word.** In a script with case, a match one word long (a
  given name, a surname, or a name of one word) must be written in the case
  the name is, as `locate_surname` already asks of surnames, so the noun
  "bill" does not bring up Bill and "rose" does not bring up Rose (my call).
  A match of two words or more compares reduced words, as above. A word
  written all in capitals (a heading, a shout) counts as written in any case
  (my call).
- **A labelled surname** matches alone by `locate_surname`'s rule: never
  when shorter than two letters or syllables (O15); in a script with case,
  whole words only and in the case written, so "Bell" is not "bell"; in a
  script without case, as a whole name, particle and all.
- **The longest match wins.** Only the characters whose best match covers
  the most words are returned: a text that starts "Ron Weasley" returns Ron
  and not every Weasley, and one that starts "Weasley" returns every Weasley
  (my call). Of matches of as many words, the one that covers more of the
  text wins, so a name written without spaces (山田太郎) beats its surname
  alone (the builder's call).
- **Order:** most recently seen first, by `Character.last` in the order of
  `Cast.books`; ties keep the order the cast lists them in (my call).
- A text that names nobody returns nothing. The look-up sees only the names
  the cast carries, so a name the text gives only after the reader's
  position does not match.

The look-up does not narrow by a title's sex ("Mr." before a surname). The
caller sends the text from the tapped word, and a title before it is not in
it unless the reader tapped the title; then the whole name "Mr. Weasley"
matches as itself. Narrowing by sex is left for later (my call).

### Moving the rules out of `extract`

The pieces the look-up shares with extraction and linking (reducing text,
splitting words, the script test that admits a particle, the whole-word and
surname rules) move into a module that the build without `extract` has.
`extract/text.rs`, `extract/roster.rs` and `link/names.rs` call the moved
code instead of their own copies. Nothing they decide changes: their tests
and the golden prompts must pass unchanged, and any test that has to change
to pass is a sign the move changed behaviour.

### The series that hold a book

A store function takes a connection and a book's SHA-256 and returns each
series whose orders hold the book, with the names of those orders in rank
order (rank 0, the default, first), series by name. A book in no series
returns an empty list; a book not stored is `Error::NotFound`, as
`cast_at` treats it. No index is added: the order table holds a row per
book per order, a few dozen rows (my call).

Which series and order a caller uses when there are several is the
caller's choice. taiyaku will take the first series and its first order that
holds the book until a reader can choose (taiyaku's design).

### What taiyaku will do with this (for orientation, decided in taiyaku's design)

- haiyaku's tables live in taiyaku's own database; taiyaku runs
  `store::migrate` on its connection when it opens it.
- A tap sends the tapped paragraph and the text from the tapped word.
  taiyaku's server maps the paragraph to its spine document (taiyaku-read's
  `BlockId.document`), calls `cast_at` with the book's SHA-256, that
  document and the scope from the reader's "Include earlier books in the
  series" setting (on by default), runs the look-up, and returns its own JSON.
- The card shows the shown name and the other names; first met and last
  seen; each relation with its note, the quote on demand, an overturned fact
  marked; several matches listed most recent first; the character above the
  dictionary entry. Its look is settled by a prototype on a live taiyaku
  server.
- The cast cuts at the last chapter the reader finished, so a name first
  met in the chapter being read shows only the dictionary until the chapter
  ends (the owner kept this, 2026-11-23).
- taiyaku's design must check that haiyaku's document paths and
  taiyaku-read's are the same strings for the same EPUB, and that taiyaku's
  member SHA-256 is the hash of the whole file, as haiyaku's is.

### Room for later

- **Joining editions.** Nothing here assumes one language a series. A later
  kind of link could join one character across editions, perhaps through
  taiyaku's alignments between editions, with a filter by edition when the
  joined map gets messy (the owner, 2026-11-23). `#[non_exhaustive]` lets
  the cast carry that without another breaking release.
- **Extraction inside taiyaku (O4).** The owner wants taiyaku to run
  extraction on its own connection later. Nothing here stands in the way;
  it needs a backend the NAS can use.

### Tests

Invented fixtures, through the public API:

- The look-up, English: a whole name; a given name; a surname shared by
  several characters, most recently seen first; a possessive; a title before
  the name tapped as part of it; "Ron" not matching "Ronald"; the longest
  match winning; a capitalised surname or given name not matching the common word; a
  description matching as a whole; a text that names nobody.
- The look-up, Korean: a name with a particle; a title after the name with
  a particle; a surname alone; a one-syllable surname not matching alone; a
  one-syllable given name with a particle matching (론은) and running into a
  longer word not (빌려서); a family with 네.
- The look-up, both: a one-word description not matching; a family plural
  ("the Weasleys", "the Dursleys'"); a word in capitals.
- The cut: a name that a later chapter gives does not match a cast cut
  before it, through `cast_at`.
- The series query: a book in two orders of one series, in rank order; a
  book in two series; a book in none; a book not stored.
- Names in the cast: the labels survive the fold, through a merge too; the
  shown name is chosen as before.
- Moving the rules: extraction's and linking's tests and golden prompts pass
  unchanged.

## Release

haiyaku 0.2.0, published from `main` after the PR merges, on the owner's
go. `docs/status.md` closes O7 (the caller draws the data; haiyaku hands out
data only) and records the owner's decisions below; Next becomes the taiyaku
design.

## Decided by the owner (2026-11-23)

- taiyaku comes first with a tap on a name (a card); a Cast panel later.
- haiyaku is a library in its own right: a caller's needs may justify an API,
  but it must be callable generally, and the caller draws the data (closes
  O7).
- haiyaku is published on crates.io and taiyaku depends on the published
  crate.
- The look-up is two general pieces: `cast_at` and a pure look-up over a
  cast (approach 2).
- The cast keeps cutting at the last chapter finished, for taps too.
- Earlier books of the series show on the card when the reader's setting
  says so.
- One character map per edition's series for now; joining editions stays
  possible.
- Data path for now: the owner fills a local taiyaku database by hand. Later:
  taiyaku runs extraction itself (O4).

## My calls

- `name` and `aliases` become `Name`s; of one text with different labels,
  the first labelled one is kept.
- `Cast.books` added, for ordering by last seen.
- `#[non_exhaustive]` on the fold's output types and the look-up's result.
- The longest match wins; ties ordered most recently seen first.
- No narrowing by a title's sex.
- A one-word match in a script with case must be written in the name's case;
  a word in capitals counts as any case.
- After a Hangul name, the look-up admits only one or two particles from a
  list; extraction's rule stays as it is.
- One-word names with no labels are left out of the look-up.
- A surname with "s" or "es" on matches the whole family.
- No index for the series query.

## Left to the builder

- The names and signatures of the look-up, its result type and the series
  query; the module the shared rules move to.
- The particle list, starting from the particles taiyaku's dictionary takes
  off (taiyaku-library's `dictionary/korean.rs`; haiyaku copies a list and
  depends on nothing of taiyaku's), and whether a particle must agree with
  the name's last syllable as taiyaku's do.
- How the moved code is shared between extraction, linking and the look-up,
  as long as the anti-goals hold.
