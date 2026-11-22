# Notes

Findings and measurements that are not worth a design doc but should not be
worked out again. Newest section first.

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
