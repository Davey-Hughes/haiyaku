# haiyaku

haiyaku (配役, "the cast") reads a story one unit at a time (a novel's
chapter, a manga chapter, a show's episode) and records who the characters are
and how they relate. A reader can step through that record up to the chapter
they have reached and no further, so it reminds them how things stand without
spoiling what comes next.

It is meant to work as a Rust library and as a program of its own. taiyaku,
the manga and book reader, will use the library; the program will also cover
shows through their subtitles.

**Status:** the core crate exists: the schema, the store, the fold and the
sidecars. Extraction and the `haiyaku` program exist too, with five
commands. `haiyaku extract <book.epub>` lists a book's table of contents,
and `haiyaku extract <book.epub> --chapters 1-17` extracts those entries
with a model, Claude by default, into a database.
`haiyaku show <book.epub> --at 5` prints the characters and how they
relate as a reader knows them after chapter 5; with `--html <file>` it
writes a page that steps through them chapter by chapter, for checking a
run. For a series, `haiyaku series <file>` stores the books and reading
orders a series file names, `haiyaku link <file>` links the characters of
each order across its books with a model, `haiyaku export` writes the
sidecar of a book or of a series, and
`haiyaku show <book.epub> --series <file>` shows a book's cast after the
books before it in a reading order.
[docs/status.md](docs/status.md) says where the project stands.

## License

GPL-3.0-or-later. See [LICENSE.md](LICENSE.md).
