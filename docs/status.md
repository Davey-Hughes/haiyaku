# Status

Where haiyaku stands, what is decided, and what comes next. Updated 2026-11-21.

## Where it stands

The spike in `spike/` has run on Sorcerer's Stone in English and Korean.
Claude Sonnet produced consistent, grounded character events at 12 s a
chapter; gemma4 12B did not, so Claude is the backend while building. The findings are in [notes.md](notes.md).
Nothing outside `spike/` exists yet. The library design is approved and
the first of three implementation plans is written.

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
- The design: [the library design](superpowers/specs/2026-11-21-haiyaku-library-design.md).
- The spike lives outside the workspace without the gates. Code leaves it only
  through a design doc and a plan.

## Next

1. Plan 1, the core crate (schema, store, fold, sidecars):
   [the core plan](superpowers/plans/2026-11-21-haiyaku-core.md).
2. Plan 2, extraction (EPUB reader, model backends, `extract`, `show`).
3. Plan 3, series linking, with the English and Korean acceptance runs.

## Open

- O1: Which Homer translations the owner has read; only those can be graded.
- O2: Test a larger local model (27-32B at 4-bit) once the library exists;
  gemma4 12B duplicated characters and looped in the spike.
- O3: Closed: haiyaku builds its own backends and shares no code with
  subcheck (the design, "Model backends").
- O4: If taiyaku's server ever runs extraction itself, it cannot use the
  desktop's `claude` login: it needs an Anthropic API key or a model server it
  can reach. Decide when it comes up; the design must not rule it out.
