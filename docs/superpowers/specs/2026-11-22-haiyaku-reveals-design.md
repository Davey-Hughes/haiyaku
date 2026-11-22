# Reveals: a second call per chapter, and relations a chapter overturns

Date: 2026-11-22. Status: draft for the owner.

## Why

Extraction makes one model call a chapter, and that one call decides
whether a reveal is recorded (O11). In plan 2's three acceptance runs on
Sorcerer's Stone, the two reveals of chapter 17 (Snape had been protecting
Harry; Quirrell serves Voldemort) came in one language per run and never
in both.

A reveal also overturns what the reader believed. The log can only add
relations, so after chapter 17 Snape is Harry's enemy and his ally at once,
and in English he stays Voldemort's ally, the belief Harry formed in
chapter 15. Nothing in the log can say that a recorded relation was false.

Two smaller defects from the same runs belong here because they also
corrupt what a reader sees after a reveal. One English run merged Quirrell
into Voldemort by a same-as, and Quirrell's relations read as Voldemort's
from then on (O12). An alias that outranks a character's shown name does
not replace it, so Mrs. Weasley stays under a description (O13).

This is the first part of plan 3. Linking a series follows in its own
pull request, because a book extracted again gets new character ids and
would lose the links made before.

## Goals

1. Record a chapter's reveals reliably, in English and in Korean.
2. Mark a recorded relation as overturned at the chapter that shows it was
   never true, and keep it: a reader before that chapter sees it as it
   was, a reader after it sees the mark.
3. Refuse a same-as between two characters the text relates to each other
   (O12).
4. Show a fuller name the model gives as an alias (O13).
5. Keep a book's cost within twice what plan 2 measured: about 2.7 USD
   for the English book and 3.8 USD for the Korean one.

## Non-goals

- Linking books across a series: the rest of plan 3.
- Changing the first call's prompt. It stays as prompt 4 left it, so a
  run on a book already extracted takes the first call's reply from the
  cache wherever the roster sent with it is unchanged.
- Relations that held when recorded and later end, such as a friendship
  that breaks. A later relation of another kind records that change; an
  overturn says only that the relation was never true.
- A cut-off inside a chapter.

## Anti-goals

The change has failed, even if it runs, when it:

- records a reveal before its chapter. The second call reads only up to
  the end of the chapter, as the first does, and its items pass the same
  checks;
- removes a relation from what a reader before the overturning chapter
  sees;
- records an overturn of a relation the log does not hold.

## Constraints

- The schema goes from version 1 to version 2. A migration file never
  changes once a database has run it (`store/migrate.rs`).
- A sidecar written at version 1 must still import. `store/sidecar.rs`
  already migrates such a file in a copy held in memory.
- The reply cache keys a call by its whole request. The first call's
  system text, the wording and layout of its prompt, and its schema must
  stay byte for byte as they are for its cached replies to count. Its
  prompt still changes where the roster it lists changes.
- The consumer of the library decides how the data is drawn. The library
  hands out the mark; it does not decide to hide an overturned relation.
- Book text stays out of the repository. The trial below ran in
  `spike/out/`, which is gitignored.

## Approaches

For catching reveals, three ways were weighed (status.md, O11):

- **A. A second call per chapter that asks only about relations.** It
  runs after the first call's reply is checked, when the chapter's new
  characters already have ids, so its reply needs no merging.
- **B. Two full samples of each chapter, merged.** Each sample gives its
  new characters ids of its own, and the check joins two of them only by
  a full name, so a character met by a description or a bare given name
  would come back twice. The two samples can also disagree on same-as and
  named events. It costs as much as A.
- **C. One call a chapter, as now.** It is the cheapest, and a reveal is
  missed now and then. A miss spoils nothing, but the reader keeps the
  picture the book has overturned.

The owner chose A after the first trial round.

For what a reveal does to the relation it overturns:

- **A new event that marks the relation overturned.** The owner chose this.
- **Let the consumer read the latest relation of a pair as the truth.**
  It needs no schema change, but a belief the book never restates for the
  same pair stays. In English run 3, chapter 15 is the only one that
  relates Snape to Voldemort.
- **Drop the relation from the fold after the overturning chapter.** The
  reader would lose what they were led to believe, which the consumer may
  want to show; a mark lets the consumer drop it if it chooses.

## The trial

Ten calls in each of two rounds, sent by hand to Claude Sonnet with the
first call's system text and arguments, on run 3's state of Sorcerer's
Stone (docs/notes.md has the full record):

- Chapter 17 three times in each language, with both reveals removed from
  the relations the call was shown, as if the first call had missed them.
  In English the same-as that merged Quirrell into Voldemort was left out
  too.
- Chapters 11 and 16 once in each language, as they stand, to catch an
  early reveal and to count the noise.

| | Round 1 | Round 2 |
| --- | --- | --- |
| Task | relations the chapter establishes, reveals or changes | relations it reveals, and relations it overturns |
| Both reveals back, chapter 17 | 6 of 6 | 6 of 6 |
| An early reveal in chapters 11 and 16 | none | none |
| Relations a call | 10.1 | 2.4 |
| Relations the call was shown already | 2.6 a call | 0.1 a call |
| Quotes that pass the check | 100 of 101 | 24 of 24, and 6 of 6 overturns |
| Spend | 0.52 USD | 0.53 USD |

Round 2 overturned "Snape ally Voldemort" in all three English samples.
It also overturned "Snape enemy Harry" in three of the six; that one is
arguable, since the chapter shows that Snape protected Harry while he
hated Harry's father. The second call costs about what the first does: the
chapter text is most of the input.

## Design

### The second call

After the first call's reply for a chapter is checked, extraction sends a
second call for the same chapter, through the same backend and with the
same system text. It carries:

1. the chapter's title and text, as the first call carries them;
2. the line "The reader has just finished this chapter.";
3. the characters the roster lists for the chapter after the first
   call's check, which include the characters that call introduced, with
   their ids as refs, in the first call's format;
4. the relations the log holds between those characters, the first
   call's included, in the first call's format, leaving out every
   relation an overturn covers;
5. the task, in the words round 2 tested:

   > Read the chapter again. This step looks only for what the chapter
   > reveals or overturns about these characters; another step already
   > recorded what the chapter shows in passing.
   >
   > relations: a relation between two of these characters that this
   > chapter reveals and that the list above does not hold in any wording,
   > such as a hidden allegiance, a secret act, or a relation of another
   > kind for a pair already listed. Leave out what the chapter only shows
   > in passing.
   >
   > overturned: a relation in the list above that this chapter shows was
   > not true, such as a character who seemed hostile but was protecting
   > someone. Name it exactly as the list gives it: the same from, to and
   > kind. Leave out a relation that changes later in the story but held
   > when it was recorded.

6. the rules: only this chapter's text, read to its end, and nothing the
   model may know of the book or the series beyond it; refs from the list;
   the first call's relation kinds; a note of a few words on what the
   chapter shows; evidence copied character for character from the
   chapter, as the first call words that rule; empty lists when there is
   nothing.

The reply's schema has two required arrays: `relations`, whose item is
the first call's relation item, and `overturned`, whose item has from,
to, kind, note and evidence typed as in the relation item.

The reply is checked like the first:

- Its relations go through the first call's relation check, and one that
  states a fact the same reply overturned is turned down
  (`overturned_in_reply`), so a reply cannot undo its own overturn.
- An overturned item passes when its refs are known, its evidence is in
  the chapter, and the log holds an earlier relation of that kind between
  the two that is not overturned already. For a directed kind (mentor,
  serves, owns) the relation must run the same way; for the others either
  way matches, since the fold keeps no direction for them.
- What fails goes to the chapter's rejected file with the first call's
  rejections.

Both calls' prompts leave out a relation an overturn covers, from the
chapter after the overturn on. Shown unmarked, it would invite the first
call to state it again, which would make it current; shown with its mark,
it would change the list's format. A later chapter that states it again,
with its own evidence, still makes it current.

The second call has its own entry in the reply cache, so a run that
resumes or repeats takes it from there. `PROMPT_VERSION` goes to 5. The
chapter's row in the book record sums the time, cost, kept and rejected
counts of both calls. A failed call is retried once, as the first is.

### The overturned event

`overturned{from, to, kind, note}`, with `at` and `quote` like every
event. Only the second call writes it.

Migration 002 adds `overturned` to the event types the events table
accepts and adds its child table, shaped like the relation table: two
entities that differ, a kind from the same list, a note. A sidecar
carries the event like any other. A version-1 sidecar imports through the
migrated copy `store/sidecar.rs` already makes, which gives O6 its test.

### The fold

An overturn covers every statement of its kind between the two characters
made before it, under whichever ids the statements were made. The fact
those statements make is marked: it keeps its first and last chapters and
gains the chapter, note and quote of the overturn. A statement of the same
kind after the overturn makes a new fact, which is current; the marked one
keeps its history. A series needs this when a later book shows that an
overturned relation held after all.

The rule holds through merges and links. When a same-as or a link later
joins two characters, the statements about the joined pair are sorted the
same way: those made before an overturn of that pair and kind fall under
it, later ones do not. If "the hooded man" was Snape's ally in chapter 13,
"Snape ally Voldemort" was overturned in chapter 17, and the hooded man
turns out to be Voldemort in chapter 20, the chapter-13 fact is marked
too: it was the same belief. An overturn in a later book of a series
marks a fact an earlier book stated about the same two characters. Before
the overturning chapter a fact shows unmarked, so the chapter cut-off
holds.

The cast hands the mark out with the fact. Whether to show a marked fact,
strike it through or drop it is the consumer's choice.

### The binary

`show` prints a marked fact with the chapter and note of its overturn.
The `--html` view draws it apart from current facts by line style and a
label, not by colour alone.

### A guard on same-as (O12)

The check refuses a same-as between two characters when the log, or any
relation item of the same reply whose kind is known (kept, or turned down
only for its evidence), relates them to each other in either direction.
As far as the text goes they are two people: Quirrell serves Voldemort.
The refused item goes to the rejected file. A true identity the guard
refuses costs a missed merge, which a link or a hand correction can make
later; a wrong merge costs the relations between the two, which the fold
drops.

### A fuller name given as an alias (O13)

Where the check would write an alias whose name ranks above the
character's shown name, it writes a `named` event instead. The fold and
the roster already raise the shown name on a `named` event.

## Acceptance

One run of the new build on each book: English entries 4 to 20, Korean
entries 5 to 21, into a gitignored directory under `spike/out/`.

1. Plan 2's checks still pass: the rejection share (at most 3% in English,
   5% in Korean), no two characters with one name, the boy met by
   description in chapter 5 and named in chapter 6, Hagrid's creatures by
   `owns`, and the Korean mother's family relations in chapter 6.
2. Both reveals at chapter 17 in both languages, and neither before it.
3. Where the log relates Snape to Voldemort as allies before chapter 17,
   that relation is overturned at chapter 17. No overturn before chapter
   17 touches a relation of Snape's to Harry or to Voldemort.
4. Quirrell and Voldemort stay two characters in both languages.
5. In English, Mrs. Weasley is shown by a name, not a description.

O13 changes shown names (Mrs. Weasley's from chapter 6), and the roster
sent with each later first call changes with them, so most first calls
miss the cache. The two runs should cost up to about 6.5 USD.

## Decided by the owner

- A second call per chapter that asks about relations (approach A),
  2026-11-22.
- A new event in the database for a relation a chapter shows was not true,
  2026-11-22.

## My calls

These are mine; the owner may change any of them.

- The task's wording, as round 2 tested it.
- The second call runs on every chapter, with no switch to turn it off.
- The first call's prompt stays as it is.
- An overturn marks a fact and never deletes one; a statement of the same
  kind after it makes a new, current fact. Through a merge or a link, a
  statement falls under an overturn when it was made before it.
- A relation an overturn covers is left out of later prompts' lists.
- An overturn means "never true"; a relation that ends is recorded by a
  later relation.
- O12's guard and O13's rule as written above, and both in this pull
  request.
- Linking waits for the next pull request.

## Left to the builder

- How migration 002 replaces the events table's type check, which SQLite
  cannot alter in place, keeping every row and foreign key.
- The second call's prompt layout beyond the wording above, reusing the
  first call's way of listing characters and relations.
- The rejected file's shape for the second call's items.
- The text of `show` and the marks on the `--html` page.

## Known weaknesses

- The second call may overturn a relation that still half holds, as with
  "Snape enemy Harry". The mark keeps the relation in view.
- The O12 guard sees the log and the first call's reply, not the second
  call's. The first call's same-as items are applied before the second
  call runs, so a reveal only the second call finds cannot stop a merge,
  and two characters merged by then cannot be related. In the cached
  replies, the one reply that merged Quirrell into Voldemort also related
  them, which the guard catches.
- It records a character's suspicion as a relation ("Hermione enemy Snape",
  chapter 11), which a later chapter then has to overturn. That spoils
  nothing.
- A book costs about twice as much. Caching the chapter text between the
  two calls could make the second call cheaper; the Claude CLI sends the
  prompt as one block, so it was not tried.
