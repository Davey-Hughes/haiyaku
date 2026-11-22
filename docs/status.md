# Status

Where haiyaku stands, what is decided, and what comes next. Updated 2026-11-22.

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
OpenAI-compatible server and recorded replies) and extraction itself: one
checked model call a chapter, the program's own ids for the characters,
and a reply cache that lets a run resume.

The `haiyaku` binary (the `cli` feature, on by default) has two
commands. `extract` lists a book's table of contents, or extracts a range
of its entries into the database, by default
`$XDG_DATA_HOME/haiyaku/haiyaku.db`. `show` prints the cast an extracted
book has after a chapter; with `--html` it writes a page that steps
through the cast chapter by chapter, for checking a run.

The last of plan 2's three acceptance runs on Sorcerer's Stone in English
and Korean, after two rounds of fixes, passes every check but one: the two
reveals of chapter 17 came in one language per run, never both, because one model call a chapter decides whether a reveal
is recorded (O11; the runs are in [notes.md](notes.md)).

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
  (composable). The link step compares names without titles, refuses
  conflicting first names or surnames, and asks the model about weak
  matches; see the notes.
- The cast cuts at the last chapter the reader finished, never mid-chapter.
- v1 links Korean series as well as English ones.
- haiyaku builds its own model backends, learning from subcheck's; the two
  share no code.
- Logs live in a structured SQLite schema, database first. A sidecar file
  with the same schema carries a log from the desktop to taiyaku.
- The consumer of the library decides how the data is drawn, so that taiyaku
  controls its own style: the library hands out the cast as data and builds
  no markup. The HTML view that checks a run belongs to the binary (O7).
- The design: [the library design](superpowers/specs/2026-11-21-haiyaku-library-design.md).
- The spike lives outside the workspace without the gates. Code leaves it only
  through a design doc and a plan.

## Next

1. Plan 3, series linking: the link step, `series`, `link`, `export` and
   `show` for a series, with the English and Korean acceptance runs.
   O11 decides first how a reveal is recorded reliably.

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
- O6: Importing rows from a sidecar at an older schema version cannot be
  tested until a version 2 migration exists; that migration adds the test.
- O7: How far "the consumer decides how the data is drawn" goes: data only,
  as plan 2 builds it, or also style hooks on a view the library ships.
  Decide when the taiyaku integration is designed.
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
  book is in the corpus or in plan 2's acceptance run.
- O10: The reply cache keys the claude backend by the model alias the user
  names (`claude` is `sonnet`), not by the model that answered. When the
  alias moves to a newer model, replies cached from the old one are reused
  until a run with `--no-cache` or a new cache. The fix is to key the cache
  by the model the reply reports.
- O11: One model call a chapter decides whether a reveal is recorded. In
  plan 2's three acceptance runs, the reveals of chapter 17 came in one
  language per run and never in both. Two samples a chapter merged, or a
  second pass over a chapter's relations, would steady it at about twice
  the cost. Decide before plan 3's acceptance runs.
- O12: A same-as can merge a character into one who speaks through it: one
  English run merged Quirrell into Voldemort in chapter 17, and Quirrell's
  relations read as Voldemort's from then on. The check accepts any same-as
  whose quote is in the chapter; a prompt rule or a guard is needed.
- O13: An alias that outranks a character's shown name does not replace it;
  only a `named` event does, and the model often puts the fuller name among
  the aliases (Mrs. Weasley stays under a description). The check could turn
  such an alias into a `named` event.
- O14: The `--html` page's labels collide in the dense centre of a cast of
  about 80 characters.
- O15: A surname of one letter or syllable (김, 이) never lists its character
  by the surname route, which keeps it from matching the start of other
  words. Such a character is listed only by a whole name in the chapter or
  by a recent sighting, so after a gap it can come back as a second
  character.
