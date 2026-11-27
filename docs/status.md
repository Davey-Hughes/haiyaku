# Status

Where haiyaku stands, what is decided, and what comes next. Updated 2026-11-26.

## Where it stands

The spike in `spike/` has run on Sorcerer's Stone in English and Korean.
Claude Sonnet produced consistent, grounded character events at 12 s a
chapter; gemma4 12B did not, so Claude is the backend while building. The findings are in [notes.md](notes.md).
The core crate exists in `crates/haiyaku`: the schema, the store, the fold
and the sidecars, built from
[the core plan](superpowers/plans/2026-11-21-haiyaku-core.md). Behind the
`extract` feature, built from
[plan 2](superpowers/plans/2026-11-21-haiyaku-extraction.md), are the EPUB
reader (`book`), the model backends (`llm`: the Claude CLI, Ollama, an
OpenAI-compatible server and recorded replies) and extraction itself: two
checked model calls a chapter, the program's own ids for the characters,
and a reply cache that lets a run resume.

The `haiyaku` binary (the `cli` feature, on by default) has five
commands. `extract` lists a book's table of contents, or extracts a range
of its entries into the database, by default
`$XDG_DATA_HOME/haiyaku/haiyaku.db`. `show` prints the cast an extracted
book has after a chapter; with `--html` it writes a page that steps
through the cast chapter by chapter, for checking a run. The other three,
`series`, `link` and `export`, are linking's, below.

The last of plan 2's three acceptance runs on Sorcerer's Stone in English
and Korean, after two rounds of fixes, passes every check but one: the two
reveals of chapter 17 came in one language per run, never both, because one model call a chapter decides whether a reveal
is recorded (O11; the runs are in [notes.md](notes.md)).

Plan 3's first part, built from
[the reveals plan](superpowers/plans/2026-11-22-haiyaku-reveals.md) on
[the reveals design](superpowers/specs/2026-11-22-haiyaku-reveals-design.md),
answers that with a second call a chapter. After the first call's reply is
checked, the second asks only what the chapter reveals about the
characters and which recorded relations it shows were never true. Such a
relation gets an `overturned` event, which schema version 2 stores, and
the fold marks the fact it covers instead of dropping it: a reader before
that chapter sees the fact as it was, a reader after it sees the mark.
`show` prints the mark after the fact, and its page draws a pair whose
facts are all overturned faint, as two rails with a label. The same part
refuses a same-as between related characters (O12) and shows a fuller
name given as an alias (O13). Its acceptance run on Sorcerer's Stone in
English and Korean passes every check, at about 1.7 times plan 2's cost a
book; it leaves O16 and O17 open (the run is in [notes.md](notes.md)).

Plan 3's second part, built from
[the linking plan](superpowers/plans/2026-11-22-haiyaku-linking.md) on
[the linking design](superpowers/specs/2026-11-22-haiyaku-linking-design.md),
links the characters of a series across its books. The owner writes a
series in a TOML file: its books and its reading orders. `series` stores
it, and `link` links each reading order with a model: every character of
an earlier book that a later character's names could match goes to the
model, and a link counts only on a quote found in what the call sent. The
first of a later character's names that has candidates gets one call
listing them all; until a call links, each later name that brings a
candidate not asked about yet gets a call of its own. `export` writes the
sidecar of a book, or of a series, which carries its books' logs with the
series and its links, so nothing has to be imported before it.
`show --series` prints the cast of a book read after the books before it
in a reading order, with the characters a link joins shown as one, and its
page steps through the order up to that book.

Its acceptance run linked Sorcerer's Stone, Chamber of Secrets and Goblet
of Fire in English and Korean, for 21.77 USD. The first run linked
Voldemort's father to Voldemort in both languages and Mr. Granger to
Hermione in English. Turning down a candidate already linked to a relative
of the later character (`Refusal::Related`) removed those three links and
no right one. After the fix checks 1 to 3 pass in both languages, and one
of 202 links is wrong, made through an extraction alias (O19). The run
leaves O18 to O23 open; it is in [notes.md](notes.md).

haiyaku 0.2.0 adds the name look-up
([the design](superpowers/specs/2026-11-23-haiyaku-name-lookup-design.md),
[the plan](superpowers/plans/2026-11-23-haiyaku-name-lookup.md)). Each
character of a cast carries its names with their labels, and the cast lists
the books it folded. `characters_named` finds the characters a text names,
most recently seen first, with how much of the text each match covers and
which name matched. `series_holding` gives the series and reading orders that
hold a book. The name rules that extraction, linking and the look-up share
build without `extract`. 0.2.0 is on crates.io, and taiyaku PR #308's
Cargo.lock resolves haiyaku 0.2.0 from it.

taiyaku's character cards, in taiyaku PR #308 (open), build on haiyaku 0.2.0
from crates.io (the name look-up design, "What taiyaku will do with this"):
a tap on a name in taiyaku's book reader looks the name up in a cast folded
from haiyaku's tables in taiyaku's library database, and shows that
character's card. Its prototype, on 2026-11-24, served the English and
Korean Sorcerer's Stone, Chamber of Secrets and Goblet of Fire, filled by
0.2.0's CLI at the linking acceptance run's chapter ranges, every call
answered from the reply cache. The prototype, the PR's design and its review
raised O26 to O32 and the end of O25 for haiyaku; O31 is closed already.
They also raised O33, a match for each mention of a character on a page,
which taiyaku's O543 (taiyaku PR #308) says needs haiyaku API work first.

## Decided

- A Rust crate that works as a library and as a standalone binary.
- After the spike, taiyaku integration comes first. The standalone binary
  stays a goal: it covers shows (subtitles), which taiyaku does not.
- Claude is the backend while haiyaku is being built. A local model must be
  possible to use, through backends like subcheck's (Ollama, an
  OpenAI-compatible server, a command); it need not be the default. It need
  not be fast, but must not be slow.
- Subtitles take speakers from supporting material. Manga uses OCR first and
  LLM image reading as the fallback.
- A series is read as one story: by default a reader sees everything known
  so far, earlier books included. An option shows only one book's
  information.
- A series can have several reading orders (Narnia in release order or in
  story order); what counts as "earlier" depends on the order chosen.
- Extraction runs ahead of time, a whole book at once. Its output goes to a
  sidecar file, or into a database through a connection the caller passes in.
- Leave room for more analysis from the same log later, such as wiki-style
  pages for characters, places and terms.
- Books are extracted on their own and linked across a series afterwards
  (composable). The link step compares names without titles and turns
  down a pair whose given names or surnames conflict, or whose titles
  state different sexes, or whose earlier character it has already linked
  to a relative of the later one. Every other possible match, an equal
  full name included, goes to the model, one call per later character
  listing all its candidates
  ([the linking design](superpowers/specs/2026-11-22-haiyaku-linking-design.md)).
- The cast cuts at the last chapter the reader finished, never mid-chapter;
  the cut holds for a tap on a name too.
- v1 links Korean series as well as English ones.
- haiyaku builds its own model backends, learning from subcheck's; the two
  share no code.
- Logs live in a structured SQLite schema, database first. A sidecar file
  with the same schema carries a log from the desktop to taiyaku.
- haiyaku is a library in its own right. A caller's needs may justify an API,
  but it must be callable generally. The caller draws the data: haiyaku hands
  out data only and builds no markup, and the page that checks a run belongs
  to the binary (O7).
- taiyaku builds on haiyaku first, with a tap on a name that shows a card; a
  Cast panel comes later.
- The look-up is two general pieces, `cast_at` and a pure look-up over any cast
  ([the name look-up design](superpowers/specs/2026-11-23-haiyaku-name-lookup-design.md)).
- Earlier books of the series show on the card when the reader's setting says
  so.
- One character map per edition's series for now, with joining editions kept
  possible.
- For now the owner fills a local taiyaku database by hand; later taiyaku runs
  extraction itself (O4).
- The design: [the library design](superpowers/specs/2026-11-21-haiyaku-library-design.md).
- haiyaku is published on crates.io, and taiyaku depends on the published
  crate. Its source is public at https://github.com/Davey-Hughes/haiyaku.
- The spike lives outside the workspace without the gates. Code leaves it only
  through a design doc and a plan.

## Next

1. A match mode for the name look-up that stops at a sentence's end or a
   closing quotation mark but not at a title's full stop, so that "Mr.
   Marsh" still matches (O25). taiyaku's character cards work without it;
   only their reading of a name across a sentence's end or a closing
   quotation mark waits for it, by the owner's decision of 2026-11-25, and
   ships meanwhile, held by a test named known wrong.

## Open

- O1: Which Homer translations the owner has read; only those can be graded.
- O2: Test a larger local model (27-32B at 4-bit) once the library exists;
  gemma4 12B duplicated characters and looped in the spike. The backend
  conveniences that plan 2 leaves out come with that work: presets,
  backends read from a settings file, a command as a backend, and waiting
  for a model server to come up.
- O3: Closed: haiyaku builds its own backends and shares no code with
  subcheck (the design, "Model backends").
- O4: If taiyaku's server ever runs extraction itself, it cannot use the
  desktop's `claude` login: it needs an Anthropic API key or a model server it
  can reach. Decide when it comes up; the design must not rule it out.
- O5: cargo-deny is not installed on the desktop, so `scripts/check-all.sh`
  stops at its last step there and `deny.toml` is first checked by CI's
  advisories job.
- O6: Closed: migration 002 is the version 2 migration, and
  `import_migrates_a_version_one_sidecar` imports a version 1 sidecar
  through it.
- O7: Closed: data only; the caller draws the data and haiyaku hands out data
  only (the owner, 2026-11-23; the name look-up design).
- O8: A Claude call that times out is stopped by killing the process
  haiyaku started. A launcher that does not exec the real program, such as
  a version manager's shim, leaves the real call running after the
  timeout. The fix is to start the call in a process group of its own and
  to catch Ctrl-C in the binary so that it stops that group. That needs
  unsafe code, which the workspace forbids, or a signal crate, a new
  dependency that plan 2 does not list. The desktop's `claude` execs, so it
  is not affected.
- O9: A name in Japanese text is found only where it starts a run of kana
  or kanji. A run of letters with no space or punctuation in it counts as
  one word, and a name is found at the start of a word, so a name in the
  middle of a run is not found until a word splitter exists. No Japanese
  book is in the corpus or in plan 2's acceptance run. The name look-up
  inherits the limit, letting anything run on after kana or a Han character.
- O10: The reply cache keys the claude backend by the model alias the user
  names (`claude` is `sonnet`), not by the model that answered. When the
  alias moves to a newer model, replies cached from the old one are reused
  until a run with `--no-cache` or a new cache. The fix is to key the cache
  by the model the reply reports.
- O11: Closed: a second call a chapter asks only what the chapter reveals
  or overturns (the reveals design, "The second call"). The measured trial
  is in [notes.md](notes.md) ("A second call per chapter, 2026-11-22").
- O12: Closed: the check refuses a same-as between two characters that the
  log or the same reply relates to each other (the reveals design, "A guard
  on same-as (O12)").
- O13: Closed: where the check would write an alias that outranks the
  shown name, at an introduction too, it writes a `named` event (the
  reveals design, "A fuller name given as an alias (O13)").
- O14: The `--html` page's labels collide in the dense centre of a cast of
  about 80 characters.
- O15: A surname of one letter or syllable (김, 이) never lists its character
  by the surname route, which keeps it from matching the start of other
  words. Such a character is listed only by a whole name in the chapter or
  by a recent sighting, so after a gap it can come back as a second
  character. The name look-up inherits the limit: such a surname alone
  names no one, even where an alias is that surname alone.
- O16: The second call can overturn a relation that still half holds. An
  overturn covers every earlier statement of its kind between the two, so
  in the Korean acceptance run Ron's enmity with Snape, recorded when Snape
  docks his points, was marked at chapter 17 together with the belief that
  Snape cursed Harry's broom. The same run overturned a doubtful rivalry
  between Snape and Quirrell. The mark keeps both in view; a stricter
  prompt rule, or an overturn of one statement instead of a kind, would
  narrow it.
- O17: A character first met only under a description or a form of
  address comes back as a second character when a later chapter names it
  and the roster no longer lists the first. In Korean the mother of
  chapter 6 returns in chapter 17 under her married name, and the parents
  Harry sees in the mirror in chapter 12, called by what he would call
  them, duplicate Lily and James. Plan 2's runs showed the same.
- O18: Extraction joins and splits that the linking acceptance run found.
  In Goblet of Fire, in both languages, Crabbe and Goyle are each joined
  with their Death Eater father at chapter 33 and take his relations. Five
  characters are split in two: Tom Riddle Sr. and Frank Bryce in English
  Goblet of Fire, Justin in English Chamber of Secrets, Oliver Wood and
  Filch in Korean Chamber of Secrets. English Goblet of Fire misses the
  chapter-35 reveal that joins the impostor Moody with Crouch's son. The
  link step repairs a split only when both rows link to one character of
  an earlier book; a character new to the book stays two rows. The run is
  in [notes.md](notes.md) ("Linking acceptance, 2026-11-23").
- O19: Wrong names from the extraction. Korean Goblet of Fire gives Peeves
  the Bloody Baron's name as an alias at chapter 21, the cause of the one
  wrong link left after the linking acceptance run, and aliases Crouch Sr.
  as "father". English Goblet of Fire aliases Draco as "Granger". English
  Chamber of Secrets gives Ron a junk alias and Lockhart a bare
  "Professor", and Korean Chamber of Secrets makes two junk characters,
  "someone" and "Dursley". Name parts are missing or mislabelled: Madam
  Pomfrey has none labelled in English Sorcerer's Stone, Mrs. Norris's
  surname carries her title in English Goblet of Fire, and in Korean
  Goblet of Fire Angelina's surname holds her full name and Dennis
  Creevey's name is stored surname first, with a comma. Such names cost
  links (O21).
- O20: The shown name of a joined character. From Chamber of Secrets
  chapter 17 the joined Voldemort is shown as "Tom Riddle", on the pages
  and to the link step. Goblet of Fire then shows two "Tom Riddle" rows
  with a family relation between them. Before `Refusal::Related`, the
  equal name led the model to link the father to Voldemort.
- O21: Missed links. Korean Chamber of Secrets links 31 characters to 30
  of Sorcerer's Stone, where English links 49 to 48; 18 Korean characters
  that both books list stay unlinked, against one in English. The Korean
  Sorcerer's Stone spells names differently from the later Korean books
  (버논/버넌, 토마스/토머스, 팬시/팬지) or translates them differently, and a name with
  no given name or surname labels is compared as a whole text. English
  misses Madam Pomfrey and Mrs. Norris for unlabelled or mislabelled name
  parts. A looser name match would send more candidates to the model, at
  the cost of more calls.
- O22: Linking's cost and the series page's size. Goblet of Fire's linking
  costs 0.43 USD in English and 0.38 in Korean, over goal 7's 0.3 USD a
  book (the linking design). The series page of a three-book order is
  about 6 MB, because its data holds every step's whole cast.
- O23: On a cache hit, `extract` and `link` print the stored reply's
  original cost and time, so a run's output cannot tell a free rerun from
  a paid one; only the count of reply files in the cache shows it. In the
  linking acceptance run, Sorcerer's Stone came wholly from the cache and
  printed 2.33 USD in English and 3.29 in Korean.
- O24: `Refusal::Related` depends on which of two related later
  characters the run reaches first: when the first one's link is wrong,
  the right one is refused without a call (the report names the
  relative). A joint call for related characters who share a candidate,
  or dropping the first link and asking again, would remove it; the owner
  decides.
- O25: Punctuation splits words, so the name look-up can match a name
  across a sentence break: "Thanks, Ned. Marsh is waiting" tapped at "Ned"
  finds Ned Marsh as a whole name over "Ned. Marsh" and hides Ned Hale.
  "Mr. Marsh" needs the same rule, so a fix has to tell a title's full stop
  from a sentence's. taiyaku's character cards (taiyaku PR #308) start a
  look-up at a word before the tap when a name starting there reaches the
  tap, so there the match reads forward from that word across a sentence's
  end or a closing quotation mark to the tapped word; a test in
  taiyaku-library records it as known wrong. The owner decided on
  2026-11-25 that taiyaku leaves it until haiyaku has a match mode that
  stops at punctuation, rather than working around it.
- O26: Words that are no name, kept as a character's names. On taiyaku's
  character cards at its prototype (2026-11-24), English cards listed such
  words among a character's other names: "boy", and a room's sign written
  in capitals. The second is the junk alias O19 lists for Ron in English
  Chamber of Secrets, a sign written in capitals rather than a name; as a
  name of more than one word it can bring up Ron's card where a book shows
  it, which was not tried. Which character, book and chapter carry "boy"
  was not written down, nor whether it carries a given-name or surname
  label. Without one, a name of one word is left out of the look-up; with
  one, every "boy" written in the name's case or in capitals brings up that
  card, and with a surname label so does "boys", read as the family's
  plural (`lookup/mod.rs`). Not counted: how many such names the six books
  carry. A count of stored names that are one lower-case word or written in
  capitals would show whether extraction's check should turn them down.
- O27: An everyday Korean word brings up a card through a name of several
  words. On taiyaku's character cards at its prototype (2026-11-24), a tap
  on 진짜 ("real", "really") in the Korean books brought up a card one of
  whose names begins with 진짜 and goes on with Moody's name. Not traced:
  whether the text tapped went on with the rest of that name, so that a
  description kept as a name matched whole (O26), or 진짜 matched alone
  through a label the extraction put on it. The character's stored names
  and their labels would tell.
- O28: Two cards for one character in the Korean books. On taiyaku's
  character cards at its prototype (2026-11-24), a look-up in the Korean
  books showed two cards for Draco Malfoy (드레이코 말포이). The linking
  acceptance run, whose link counts the prototype's match (O31), records
  the likely cause, an O21 case: Korean Chamber of Secrets' Draco stays
  unlinked, the model having chosen the right candidate with a quote that
  was only a name, while Goblet of Fire's Draco links to Sorcerer's Stone's
  from its chapter 8 ([notes.md](notes.md), "Linking acceptance,
  2026-11-23"). So from Chamber of Secrets on the series shows two.
  Not traced: the book and chapter of the look-up, and whether that is the
  whole cause.
- O29: A Korean name with 뿐이었다 written onto it finds no one. On taiyaku's
  character cards at its prototype (2026-11-24), a tap on Hagrid's Korean
  name with 뿐이었다 after it found no card. After a name that ends in a Hangul
  syllable the look-up admits only one or two particles from its list
  (`lookup/particles.rs`); 이었다 is on it and 뿐 is not, so the run-on is
  refused. Whether 뿐, and endings like it, join the list decides it; a
  look-up test of an invented name carrying it, such as 에이다뿐이었다, would hold
  the answer.
- O30: Korean cards carry notes in English. On taiyaku's character cards at
  its prototype (2026-11-24), the Korean books' cards showed their
  relations' notes in English; not counted whether every note was. The
  extraction prompt names no language for a note and gives its examples in
  English (`extract/prompt.rs`). Which language a note should be in, the
  book's or the reader's, is the owner's call; the prompt would then say
  so, and a book already extracted keeps its notes until it is extracted
  again.
- O31: Closed: whether the link counts at taiyaku's prototype match the
  linking acceptance run's. They match the run's counts after
  `Refusal::Related`. The prototype's `link` made 114 links at 118 calls
  in English and 88 at 95 in Korean, against 116 at 124 and 89 at 96 in the
  run's report, which are its run 1. After `Refusal::Related` the run made
  49 and 65 links at 50 and 68 calls in English and 31 and 57 at 35 and 60
  in Korean, the same 114 at 118 and 88 at 95 ([notes.md](notes.md),
  "Linking acceptance, 2026-11-23").
- O32: A light read of a book's title and chapter titles. `read_book` is
  the only public read of them, and it, like the sidecar exports
  (`export_book`, `export_series`), reads the book's whole log, which
  taiyaku's character cards design judged too much for every tap. So
  taiyaku reads them once per book and keeps them while its server runs
  (its plan's FLAG 4): a log replaced under a running server shows the old
  titles until a restart, and taiyaku's how-to stops the server for a
  fill. Its one trial of a write while the server ran could not show stale
  titles, as the replaced log had the same ones. A read of the titles
  alone, or a version on a log by which a replaced one can be told, would
  let taiyaku's titles follow a replaced log. taiyaku's plan puts one of
  them before any fill of a running library, which is the owner's call
  there.
- O33: A match for each mention of a character in a text. While a card
  shows, taiyaku's character cards (taiyaku PR #308) mark the other mentions
  of the first card's character by a rule of the reader's own over that
  card's names (`bookMentions.ts`), because `characters_named` answers who a
  text names from its start, not which character each mention on a page is.
  The blocks searched are those on the screen and a screen either way. The
  rule keeps to the names that are that character's alone, since a mark on
  someone else is worse than a mention missed, and marks nothing after a
  look-up that named several characters by neither the first card's shown
  name nor its given name. Tests named KNOWN WRONG in taiyaku's
  `bookMentions.test.ts` record what it gets wrong: it misses a character
  with a given name named by surname alone, by a title and surname other
  than its shown name, or by another name that lacks the given name, and it
  marks a Korean verb form that begins like a Hangul name. Telling a bare
  surname, or another person's title and surname, apart by who it is needs
  haiyaku to answer, for a text, which character each mention in it names;
  then taiyaku's reader asks for it (taiyaku's O543, taiyaku PR #308). Not
  decided: the API's shape, and how a mention that fits several characters,
  such as a surname a family shares, is told apart.
