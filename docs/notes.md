# Notes

Findings and measurements that are not worth a design doc but should not be
worked out again. Newest section first.

## Linking acceptance, 2026-11-23: Sorcerer's Stone, Chamber of Secrets and Goblet of Fire in English and Korean

The build of
[the linking plan](superpowers/plans/2026-11-22-haiyaku-linking.md)
extracted three books a language on Claude Sonnet, one database a
language, and linked them in one reading order: Sorcerer's Stone, Chamber
of Secrets, Goblet of Fire. Sorcerer's Stone took entries 4-20 in English
and 5-21 in Korean, as in the reveals run, and came from the reply cache
at no cost: no reply file was added, and its counts match that run.
Chamber of Secrets took entries 4-21 and 5-22 (chapters 1-18), Goblet of
Fire 4-40 and 5-41 (chapters 1-37). Left out of both: the title,
Pottermore and contents entries before the story; the list of the author's
books, the excerpt of the next book and the copyright page after it. The
two languages ran in parallel, each book after the one before.

| Book | Language | Time | Kept | Rejected | Share | Cost | Characters |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Sorcerer's Stone | English | 1.0 s | 515 | 4 | 0.8% | 0, from the cache | 79 |
| Sorcerer's Stone | Korean | 2.4 s | 477 | 8 | 1.6% | 0, from the cache | 79 |
| Chamber of Secrets | English | 417 s | 585 | 6 | 1.0% | 2.69 USD | 81 |
| Chamber of Secrets | Korean | 414 s | 530 | 5 | 0.9% | 3.60 USD | 79 |
| Goblet of Fire | English | 813 s | 1233 | 17 | 1.4% | 5.84 USD | 131 |
| Goblet of Fire | Korean | 941 s | 1165 | 9 | 0.8% | 8.25 USD | 117 |

Linking, a row per later book. Run 1 used the build at 0d04ee9. The fix is
`Refusal::Related` (9f67b02, below), after which `link` ran again with the
reply cache.

| Order | Later book | Calls | Links | Cost | Call time | After the fix |
| --- | --- | --- | --- | --- | --- | --- |
| English | Chamber of Secrets | 53 | 50 | 0.31 USD | 148 s | 50 calls, 49 links, all from the cache |
| English | Goblet of Fire | 71 | 66 | 0.43 USD | 201 s | 68 calls, 65 links; 3 paid, 0.024 USD |
| Korean | Chamber of Secrets | 35 | 31 | 0.24 USD | 119 s | 35 calls, 31 links, all from the cache |
| Korean | Goblet of Fire | 61 | 58 | 0.38 USD | 185 s | 60 calls, 57 links; 3 paid, 0.017 USD |

| Check | Run 1, English | Run 1, Korean | After the fix, English | After the fix, Korean |
| --- | --- | --- | --- | --- |
| 1. Tom Riddle joins Voldemort at Chamber of Secrets 17, not before | pass | pass | pass | pass |
| 2. Tom the barman apart from Riddle and Voldemort; Lucius apart from Draco | pass | pass | pass | pass |
| 3. Goblet of Fire's Tom Riddle, the father, apart from Voldemort at the end | fail: linked to Voldemort at 33 | fail: linked to Voldemort at 33 | pass | pass |
| 4. Every link right | 114 of 116 | 87 of 89 | 114 of 114 | 87 of 88 (O19) |
| 5. Linking's cost a book, against goal 7's 0.3 USD | 0.31 and 0.43 USD | 0.24 and 0.38 USD | 0 and 0.024 USD paid | 0 and 0.017 USD paid |
| 6. Moody and Crouch's son; Wormtail, Pettigrew and Scabbers | recorded below | recorded below | unchanged | unchanged |

- **Run 1's linking cost 1.35 USD, against the plan's 1.5:** 0.73 USD for
  124 calls in 349 s in English, 0.62 USD for 96 calls in 304 s in Korean.
  Goal 7 of
  [the linking design](superpowers/specs/2026-11-22-haiyaku-linking-design.md)
  is about 0.3 USD a book in a language: Chamber of Secrets is at 0.31 USD
  in English and 0.24 in Korean, Goblet of Fire over it at 0.43 and 0.38
  (O22). The rerun took 12.2 s in English and 10.2 s in Korean; at what
  its replies cost when they were first made, it comes to 0.70 USD and
  334 s in English, 0.61 USD and 301 s in Korean.
- **`series` exited 0 in both languages.** The pages are 1.67 MB (English)
  and 1.62 MB (Korean) for Chamber of Secrets and 5.85 MB and 6.00 MB for
  Goblet of Fire, each built in 0.06-0.24 s (O22).
- **Check 1 comes from the extraction, not the link step.** Riddle is a
  row of his own from Chamber of Secrets chapter 13 to 16, and the book's
  own same-as joins him into its Voldemort at chapter 17; that Voldemort
  links to Sorcerer's Stone's. The only call on Riddle listed the barman,
  and the model chose none.
- **Chamber of Secrets in English against the spike's 49** ("Spike,
  2026-11-21: link step v2"). After the fix it makes 49 links, all right,
  to 48 Sorcerer's Stone characters: the book's two Hagrid rows, which its
  extraction joins at chapter 13, both link to Hagrid. With Madam Pomfrey,
  the one it misses, the book lists 49 Sorcerer's Stone characters, as
  many as the spike linked. As in the spike, its extraction lists neither
  the Fat Lady nor the Sorting Hat.
- **Run 1's link step made three wrong links.** In English, Chamber of
  Secrets' Mr. Granger (chapter 4) went to Hermione; in both languages,
  Goblet of Fire's Tom Riddle, Voldemort's father (chapter 33), went to
  Voldemort. Each time the model linked a relative who shares a name,
  against the prompt's rule, and nothing in the step stopped a link that
  makes a character its own relative. The father's call also saw an equal
  name: the joined Voldemort is shown as "Tom Riddle" from Chamber of
  Secrets chapter 17 on (O20).
- **A database query found exactly those three.** It lists each link whose
  later character the later book relates to another later character linked
  to the same earlier character. It returned the three wrong links and
  none of the 201 right ones. The right links that join two later rows to
  one earlier character (Hagrid in English, Oliver Wood in Korean) repair
  a split, and the two rows of a split have no relation between them.
- **The fix, `Refusal::Related` (9f67b02), turns down a candidate already
  linked to a relative of the later character, before the call.** It reads
  the later book's relations at any chapter, and counts an overturned
  relation as one. The rerun made 6 paid calls for 0.042 USD (the cache
  went from 607 to 613 reply files). It removed the three wrong links,
  lost no right one, and added or changed none. In English the father's
  only candidate is now turned down with no call; in Korean his call lists
  only the barman, and the model chose none. The other five paid calls
  asked again about Hermione (English) and the Weasley parents (both
  languages), whose candidate lists changed, and made the links run 1
  made.
- **One wrong link is left,** and the extraction caused it: Korean Goblet
  of Fire gives Peeves the Bloody Baron's name as an alias at chapter 21,
  and the call on that name links Peeves to Chamber of Secrets' Baron
  (O19). The chapter-12 call on Peeves had made no link: its quote was
  only a name.
- **Moody and Crouch's son (check 6).** English keeps them two characters,
  with no same-as between them, and the impostor's row takes the real
  Moody's name at chapter 35 (O18). Korean joins them at chapter 35, gives
  the row the alias Crouch at chapter 36, and makes the real Moody a
  character of his own.
- **Wormtail, Pettigrew and Scabbers (check 6).** Neither language
  extracts the name Pettigrew. Wormtail, who also carries the name Peter,
  shares no name with Scabbers, gets no candidate and stays unlinked in
  both. Korean Goblet of Fire has a Scabbers at chapter 5, rightly linked
  to Sorcerer's Stone's.
- **Characters both books list that stay unlinked** (O21): one in each
  English book, 18 in Korean Chamber of Secrets and 4 in Korean Goblet of
  Fire.
  - The Korean Sorcerer's Stone spells or translates the name differently:
    Vernon, Dean and Pansy, which the name rules turn down for the
    spelling (버논/버넌, 토마스/토머스, 팬시/팬지); Petunia, Dudley, Seamus, Crabbe,
    Alicia, Angelina, Parvati, Hannah, the Fat Friar, the Bloody Baron and
    Madam Hooch in Chamber of Secrets and the Fat Lady in Goblet of Fire,
    which get no candidate.
  - Name parts are unlabelled on one side, so the rules compare whole
    texts and find nothing shared: Madam Pomfrey (English Chamber of
    Secrets) and Mrs. Norris (English Goblet of Fire); Flitwick, Lavender,
    Pomfrey and the first of the two Filch rows (Korean Chamber of
    Secrets); Hannah Abbott and Mrs. Norris (Korean Goblet of Fire). Since
    Goblet of Fire's Pomfrey links to Chamber of Secrets', Sorcerer's
    Stone's stays a second row in English.
  - The model chose the right candidate with a quote that was only a name:
    Draco in Korean Chamber of Secrets, who has no link in run 1 or after
    the fix.
  - Rightly left apart in English: Riddle and the barman, Mrs. Granger and
    Hermione, Arthur and Mrs. Weasley, Lucius and Draco, and Goblet of
    Fire's chapter-1 Tom, Mrs. Riddle, Dennis Creevey, Mrs. Finnigan,
    Frank Longbottom, Narcissa and Padma. In Korean: Riddle and the
    barman, Lucius and Draco, Chamber of Secrets' Mr. Weasley and the
    father in the mirror, and Goblet of Fire's Dennis Creevey, Mrs.
    Finnigan, Frank Longbottom and Narcissa.
- **Korean Goblet of Fire's Draco is linked** (added 2026-11-26, for
  status.md's O28): to Sorcerer's Stone's, from chapter 8, in run 1 and
  after the fix, while Chamber of Secrets' Draco has no link in either.
- **Other measurements.** Korean Goblet of Fire costs about 1.4 times
  English a chapter, as Sorcerer's Stone did. English Goblet of Fire
  chapter 1 rejected 4 of 19 items (21%); no other chapter came near.
- **Spend: 21.73 USD for run 1 and 0.04 for the rerun, 21.77 in all,**
  against the plan's estimate of 21-22: extraction 6.29 USD for Chamber of
  Secrets (estimate about 6) and 14.09 for Goblet of Fire (about 14),
  linking 1.35 (about 1.5).
- **Known weaknesses of the link step, left as they are:**
  - The check that a quote is more than a name takes out whole names only.
    Part of a name, a title and surname with a possessive, passes it, and
    so does a name followed by a Korean particle of three syllables or
    more or by one word of eight letters or more: either reaches
    `MIN_QUOTE_WEIGHT` (8). It also takes names out without regard for
    word boundaries; taking "Tom" out of "bottom" can only make a quote
    lighter, so it costs a missed link at most.
  - The link report lists each later/earlier pair once, so a Related
    turn-down that follows an earlier refusal of the same pair is not
    printed: English Chamber of Secrets' Arthur Weasley turned Sorcerer's
    Stone's Mrs. Weasley down at chapter 3 for a title of the other sex,
    and his Related turn-down at chapter 12 has no line. Fixed after the
    run: the report lists a pair once for each reason, and not at all once
    a call for the later character has listed the earlier one.
  - A relation of any kind turns a candidate down, and so does an
    overturn, not only a family relation. Every case in this run was
    family.
  - Of two related characters, the one the run reaches second is refused,
    so a wrong first link would refuse the right one (O24).
  - Relatives were matched by raw entity id: a relation on an id that the
    later book joins by same-as to a linked character did not refuse.
    Fixed after the run: a relation on any id the later book joins with
    the later character counts, and so does a link made for any id it
    joins with the relative.

## Reveals acceptance, 2026-11-22: Sorcerer's Stone in English and Korean

The build of [the reveals plan](superpowers/plans/2026-11-22-haiyaku-reveals.md),
with its second call a chapter, extracted the 17 chapters of each book
(English entries 4-20, Korean 5-21) on Claude Sonnet, the two books in
parallel. Each chapter's first call missed the reply cache from chapter 2
on, because the new name rule (O13) changes the roster a prompt carries.

| Book | Time | Kept | Rejected | Share | Cost | Plan 2 run 3 |
| --- | --- | --- | --- | --- | --- | --- |
| English | 353 s | 515 | 4 | 0.8% | 2.27 USD | 249 s, 1.34 USD |
| Korean | 381 s | 477 | 8 | 1.7% | 3.19 USD | 279 s, 1.91 USD |

| Check | English | Korean |
| --- | --- | --- |
| Share at most 3% (EN), 5% (KO); no two characters with one full name | pass, 79 characters | pass, 79 characters |
| The boy met by description at 5, named at 6 | pass | pass |
| Hagrid's creatures by `owns` | pass | pass |
| The mother's family relations at 6 | | pass |
| Both reveals at 17, not before | pass | pass |
| The belief that Snape is Voldemort's ally overturned at 17, no overturn of Snape's relations before 17 | pass | no such belief recorded; no early overturn |
| Quirrell and Voldemort stay two characters | pass | pass |
| Mrs. Weasley shown by a name | pass, from chapter 17 | |

- **The first call found both reveals in both books this time,** so the
  run does not test the second call's catch; the hand trial did (6 of 6
  samples, in the section below).
- **The second calls added 36 relations in English and 30 in Korean**
  (18% of each book's relations) and 2 and 3 overturns, with no item
  rejected.
- **The overturns:** in English, Snape's alliance with Voldemort at
  chapter 17, and at chapter 16 an enmity between Hermione and Neville that
  the same chapter's first call had recorded. In Korean, at chapter 17,
  the enmity of Ron and of Hermione towards Snape and a rivalry between
  Snape and Quirrell. The Ron overturn also covers an earlier enmity that
  held, and the rivalry overturn is doubtful (O16).
- **Mrs. Weasley takes her name only at chapter 17 in English:** the
  chapter-6 reply never gave it, so O13 had nothing to raise there.
- **Korean still has second characters** for the mother of chapter 6 and
  for the parents in the mirror (O17).
- Spend: 5.46 USD, against the design's estimate of about 6.5.

## A second call per chapter, 2026-11-22: a trial on chapters 11, 16 and 17

Before building the second call that O11 asks for, it was tried by hand on
run 3's state of Sorcerer's Stone. A stand-in `claude` captured the
first call's prompt and arguments for chapters 11, 16 and 17 of each
book; the second call's prompt was written from them and sent to Claude
Sonnet with the same system text and arguments. The prompt lists the
characters the first call's prompt lists plus those the first call
introduced, and the relations between them, the first call's included.

For chapter 17 the test was strict: both reveals (Snape protecting Harry,
Quirrell serving Voldemort) were taken out of the relations the call was
shown, as if the first call had missed them, and in English so was the
same-as that merged Quirrell into Voldemort. Chapters 11 and 16 were sent
as they stand, to catch an early reveal and to count the noise.

Round 1 asked for relations the chapter "establishes, reveals or changes".
Round 2 asked only for what the chapter reveals, and added a second list
for relations the chapter shows were not true. The wording is in
[the reveals design](superpowers/specs/2026-11-22-haiyaku-reveals-design.md).

| | Round 1 | Round 2 |
| --- | --- | --- |
| Both reveals back at chapter 17 | 6 of 6 samples | 6 of 6 samples |
| An early reveal at chapters 11 and 16 | none | none |
| Relations a call | 10.1 | 2.4 |
| Relations the call was shown already, a call | 2.6 | 0.1 |
| Quotes that pass the check | 100 of 101 | 24 of 24, 6 of 6 overturns |
| Spend, 10 calls | 0.52 USD | 0.53 USD |

- **Round 1 is noisy.** Beside the reveals it gave 6 to 15 relations a
  chapter: `mentor` for a single scolding, `serves` for the school
  nurse, notes that read "placeholder" and "none", and pairs given in both
  directions.
- **Round 2 overturned the stale belief.** All three English samples marked
  "Snape ally Voldemort" (chapter 15, Harry's belief) as not true; round 1
  never touched it, because it could only add. Korean run 3 holds no such
  relation.
- **One overturn is arguable.** Three of the six chapter-17 samples also
  overturned "Snape enemy Harry", though the chapter shows that Snape
  protected Harry while he hated Harry's father.
- **The second call costs about what the first does.** A first sample cost
  0.05 to 0.11 USD against run 3's 0.07 to 0.13 for the same chapters:
  the chapter text is most of the input. A repeat of the same prompt within
  the hour cost about 0.01 USD from the Claude CLI's prompt cache.
- Round 2 still records children's suspicions as relations (in English
  chapter 11, Hermione and Ron as enemies of Snape).

## Plan 2 acceptance, 2026-11-22: Sorcerer's Stone in English and Korean

The `haiyaku` binary extracted the 17 chapters of each book (English
entries 4-20, Korean 5-21) on Claude Sonnet three times: once with Part B
as reviewed, then twice after fixes to the prompt and to the part of the
roster a chapter gets. The share is rejected entries (whole items and
single names) over kept items plus rejected entries.

| Run | Prompt | Book | Time | Kept | Rejected | Share | Cost |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 2 | English | 207 s | 435 | 14 | 3.1% | 1.22 USD |
| 1 | 2 | Korean | 222 s | 394 | 26 | 6.2% | 1.72 USD |
| 2 | 3 | English | 233 s | 466 | 4 | 0.9% | 1.29 USD |
| 2 | 3 | Korean | 248 s | 423 | 4 | 0.9% | 1.77 USD |
| 3 | 4 | English | 249 s | 461 | 6 | 1.3% | 1.34 USD |
| 3 | 4 | Korean | 279 s | 418 | 5 | 1.2% | 1.91 USD |

| Check | Run 1 EN | Run 1 KO | Run 2 EN | Run 2 KO | Run 3 EN | Run 3 KO |
| --- | --- | --- | --- | --- | --- | --- |
| Share at most 3% (EN), 5% (KO) | fail | fail | pass | pass | pass | pass |
| No two characters with one name; cast | 83, see below | 71 | 79, pass | 79, pass | 77, pass | 76, pass |
| Both reveals at 17, not at 16 | pass | fail | pass | fail | fail | pass |
| The boy met by description at 5, named at 6 | fail | fail | pass | pass | pass | pass |
| Hagrid's creatures by `owns` | pass | pass | pass | pass | pass | pass |
| The mother's family relations at 6 (KO) | | pass | | pass | | pass |

**Run 1 failed on the model's rewording.** The checks require a name, a
description or a quote as the chapter writes it. The model composed
descriptions, joined a full name from parts the chapter writes apart, put
a name where the text has a pronoun and changed Korean word endings, and a
rejected new character took its relations with it. No two characters shared
a full name, but four came back as second characters. McGonagall (in both
books) and Lily, first met under a title and surname, were not listed in a
chapter that writes only their full names. Harry's parents came back in the
English chapter 12 under the names he calls them. Flitwick was listed in
chapter 9, the chapter after his first, and the model gave him a new ref
there. Two of three samples of the Korean
chapter-17 prompt came back nearly empty: 5 characters and 0 or 3
relations.

**The fixes.** Prompt 3 puts the chapter first and the rules last. It asks
that a name or description given from the chapter be one run of its words,
that a listed personal name keep its ref, that a quote be copied character
for character, and that a same-as name two different characters. The
roster also lists a character whose labelled surname stands in the
chapter, written in its own case where the script has case and longer than
one letter or syllable. Prompt 4 adds that a listed pair gains a relation of
another kind when the chapter reveals one. On the captured Korean chapter-17
prompt, Snape's protection of Harry came back in 2 of 3 samples with that
sentence and in none of 2 without it.

**One sample a chapter decides the reveal check.** Run 2 recorded both
reveals in English and missed Snape's protection in Korean. Run 3 recorded
both in Korean and missed it in English. There, chapter 17 also merged
Quirrell into Voldemort by a same-as, so Quirrell's relations read as
Voldemort's from then on. See O11 and O12 in [status.md](status.md).

**The runs also show smaller gaps.** An alias that outranks a
character's shown name does not replace it: in English, Mrs. Weasley stays
under a description to the end (O13). In Korean run 3 she came back in
chapter 17 as a second character, because her first one carried no
labelled surname and had not been seen since chapter 6. The `--html` page holds 77 to 83
characters, but its labels collide in the dense centre (O14). The three
runs, the single-chapter samples and the prompt experiment cost about
9.7 USD.

## Spike review, 2026-11-21: defects left in the spike

A review of the spike before its PR found the defects below. The spike keeps
them: fixing them would change what extraction and linking return, and the
numbers in these notes were measured with the code as it stands. The library
must not repeat them. Plan 2 (extraction) answers 1 to 6, plan 3 (linking)
answers 7.

1. **A new character whose name is already in the roster is folded into the
   earlier one.** A father introduced as "Mr. Malfoy" lands on his son when
   the son carries that alias, and takes the chapter's relations with him.
   The link step refuses such a match when the names clash; extraction has no
   such guard.
2. **An empty name passes the check that the name is in the text**, and so
   does a short invented name that happens to sit inside another word: the
   check compares letters without word boundaries.
3. **An id that was merged away can be introduced again.** The new id is
   not resolved through the merges first, so the character is created and
   every later event for it goes to the survivor of the old merge.
4. **Named HTML entities outside a short list stay in the text as written**,
   so a quote or a name with an accented letter written as an entity fails
   the evidence check.
5. **A run extracted with `--after` cannot be shown on its own.** Its file
   holds events for characters of the earlier books with no introduction, and
   `report` and `view` fold only the run's own events.
6. **The backends take their settings too literally.** `OLLAMA_HOST` is used
   as a URL although its usual values have no scheme, and the
   OpenAI-compatible backend sends temperature 0, so retrying a reply that
   looped gives the same reply.
7. **The link step remembers which pairs it asked the model about, not the
   answers.** A character the model matched to two candidates stays unlinked,
   but a later event can link it to a third as if that were its only match.
   A call that failed is not asked again.

## Spike, 2026-11-21: link step v2

The second version of the link step drops a leading "the" and titles (Mr.,
Mrs., Professor, Uncle and the like) before it compares names, and a title's
sex keeps "Mr. Weasley" apart from "Mrs. Weasley". It refuses a link when both
sides have a first name, or both a surname, and the two differ. A later
character links without the model only when its full name equals an earlier
character's. Alias-only matches, bare first names and partial names go to
Claude Sonnet with the later chapter's evidence quotes and the earlier
character's first quote; a "same" answer counts only if its quote appears in
the text the model was sent.

Run on the same inputs and compared with B as before:

| | v1 | v2 |
| --- | --- | --- |
| First-book characters linked by A | 47 | 49 |
| Carried over by B | 51 | 51 |
| In both | 46 | 49 |
| Only in A | Tom the barman (wrong) | none |
| Only in B | 5 | the Fat Lady, the Sorting Hat |

- The model saw 8 pairs and called seven of them the same character (Dudley,
  Fred, George, Ginny, Percy, Charlie, Lavender) and Tom Riddle different
  from Tom the barman: 8 calls, 14 s, 0.04 USD.
- Lucius Malfoy stays a second-book character; the conflict check refused his
  "Mr. Malfoy" alias before any model call.
- Tom Riddle joins Voldemort in chapter 17 and not before. The barman stays
  separate.
- The two misses left belong to the extraction: Chamber of Secrets on its own
  never lists the Fat Lady or the Sorting Hat.

Approach A holds. It links every character the standalone extraction
produces and makes no wrong links.

## Spike, 2026-11-20: linking books extracted on their own

Approach A extracts each book with an empty cast list and links the books in
reading order afterwards. Approach B carries the earlier books' cast into a
book's prompts. The test was Chamber of Secrets (English, Claude Sonnet) both
ways, against Sorcerer's Stone extracted as before.

| Run | Time | Kept | Rejected | Cost |
| --- | --- | --- | --- | --- |
| Chamber of Secrets alone (A) | 199 s, 11 s a chapter | 587 | 7 | 1.39 USD |
| Chamber of Secrets after Sorcerer's Stone (B) | 212 s, 12 s a chapter | 576 | 8 | 1.63 USD |

The link step (`haiyaku-spike link`) matched a character of the later book to
one of an earlier book when its name, or failing that one of its aliases from
the same chapter, equalled a name or alias of exactly one earlier character.
It used no model.

- **Agreement:** A linked 47 characters of the first book; B carried 51 over.
  Both have 46.
- **A's two wrong links:** Lucius Malfoy went to Draco through the alias
  "Mr. Malfoy", and Tom Riddle went to Tom the barman through "Tom". The second
  then merged the barman into Voldemort at the chapter 17 reveal. B made
  neither mistake.
- **A's five misses:** the Fat Friar and the Bloody Baron (the first book
  named them with "the"), Lavender (only "Lavender" in the first book,
  "Lavender Brown" in the second), and the Fat Lady and the Sorting Hat, which
  the standalone extraction never listed. With the cast in the prompt, the
  model also mentions minor characters it already knows.
- **Reveal timing:** Tom Riddle became Voldemort in chapter 17 both ways and
  not before.

So A holds up if the link step gets better than names alone: compare names
without articles and titles, refuse a link when the surnames or first names
conflict, and send weak matches (one shared first name, an alias only) to a
model with the later chapter's evidence.

## Spike, 2026-11-20: Sorcerer's Stone in English and Korean

The spike (`spike/`) read each chapter with the roster built from the
chapters before it, and the program checked every item the model returned. An
item survived only if its evidence quote appeared in that chapter's text,
comparing letters and digits only; a new character also needed a name that
appeared there. Each book has 17 chapters, read in order. The runs are in
`spike/out/`, which is gitignored.

| Run | Chapters | Time | Kept | Rejected | Cost |
| --- | --- | --- | --- | --- | --- |
| Claude Sonnet, English | 17 | 205 s, 12 s a chapter | 518 | 7 | 1.26 USD |
| Claude Sonnet, Korean | 17 | 204 s, 12 s a chapter | 526 | 17 | 1.72 USD |
| gemma4 12B (Ollama, RX 7900 XT), English | 8 of 17 | 348 s, 43 s a chapter | 243 | 77 | 0 |

**Sonnet is consistent and grounded in both languages.** Neither run created
the same character twice. Names gathered under one entry as aliases across
chapters (Voldemort and the euphemisms for him; Uncle Vernon, Vernon Dursley
and Mr. Dursley). The book's two late reveals, that Snape had been protecting
Harry and that Quirrell serves Voldemort, appear as relations in chapter 17
and nowhere earlier, in both languages. Korean spacing variants caused no
rejections, because the comparison ignores spaces.

**gemma4 12B is not usable with this prompt.** It created the same person
twice (Hagrid and "the giant", Dumbledore and "the tiny old man", the pale
boy and Draco Malfoy). It recorded a pronoun and groups of people as
characters, and it copied lines of the roster back as evidence. Twice it kept
listing characters until it hit the 8192-token output limit, once re-listing
the whole roster and once repeating one character; the second time stopped the
run at chapter 9. The program's checks rejected 24% of its items, which kept
its invented aliases and roster echoes out of the record.

### What the design has to handle

1. **The program assigns ids, not the model.** Sonnet gave the pale boy in
   chapter 5 the id `draco-malfoy-boy`; the text names him in chapter 6.
   The id came from the model's knowledge of the book. An id the reader never
   sees still leaks to anything that reads the data.
2. **A display name has to improve as the text reveals more.** A character
   keeps the first name it was given: "the pale boy in Madam Malkin's" after
   Draco is named, "Mum" for Mrs. Weasley, "Mrs. Potter" for Lily.
3. **A rejected character takes its relations with it.** In Korean chapter 6
   the text calls Mrs. Weasley "mum" and describes her, but never uses the
   name the model gave her. The program rejected her, and her five family
   relations were dropped as unknown ids. Two Korean descriptions failed the
   same way: the model re-inflected the text's wording.
4. **The minimum evidence length must depend on the script.** Eight letters
   is a sentence fragment in English but a whole short line in Hangul. Five
   Korean quotes were rejected as too short, such as a name being called out.
5. **The relation kinds have no place for owning a creature.** Hagrid and
   Fluffy and Hagrid and Norbert came back as `family` in both languages,
   and Ron and Scabbers as `serves` in Korean.
6. **The roster grows with the story.** By chapter 17 the roster and
   relations sent with each chapter cover 77 characters and 92 pairs. A
   series multiplies that, so a call will have to send only the part of the
   roster that bears on the chapter.
