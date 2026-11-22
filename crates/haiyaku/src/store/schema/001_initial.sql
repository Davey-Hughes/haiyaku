-- Version 1 of the haiyaku schema. A database that has run this file never runs it again, so a change to
-- the schema is a new numbered file beside this one, never an edit here.
--
-- A column is NOT NULL unless a row can have nothing to say there. A primary key says so too: SQLite lets
-- NULL into a primary key that is not a rowid.

CREATE TABLE haiyaku_book (
    sha256         TEXT NOT NULL PRIMARY KEY CHECK (length(sha256) = 64),
    title          TEXT NOT NULL,
    language       TEXT NOT NULL,
    backend        TEXT NOT NULL,
    model          TEXT NOT NULL,
    prompt_version INTEGER NOT NULL,
    -- RFC 3339, in UTC.
    extracted_at   TEXT NOT NULL
);

CREATE TABLE haiyaku_chapter (
    book      TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    -- From 1, in reading order.
    number    INTEGER NOT NULL,
    -- The chapter's index in the book's table of contents.
    toc_entry INTEGER NOT NULL,
    title     TEXT NOT NULL,
    seconds   REAL NOT NULL,
    -- NULL when the backend reports no cost.
    cost_usd  REAL,
    kept      INTEGER NOT NULL,
    rejected  INTEGER NOT NULL,
    PRIMARY KEY (book, number)
);

CREATE TABLE haiyaku_document (
    book    TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    -- Spine order, from 0.
    ordinal INTEGER NOT NULL,
    path    TEXT NOT NULL,
    -- NULL for a document outside every chosen chapter: front matter, back matter.
    chapter INTEGER,
    PRIMARY KEY (book, ordinal),
    UNIQUE (book, path),
    FOREIGN KEY (book, chapter) REFERENCES haiyaku_chapter (book, number)
);

CREATE TABLE haiyaku_entity (
    book TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    id   TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('character')),
    PRIMARY KEY (book, id)
);

-- One row per event in reading order. What the event says is in the one child table for its type, keyed by
-- the same (book, seq), and goes when the event goes.
CREATE TABLE haiyaku_event (
    book     TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq      INTEGER NOT NULL,
    chapter  INTEGER NOT NULL,
    document INTEGER NOT NULL,
    block    INTEGER NOT NULL,
    type     TEXT NOT NULL CHECK (type IN ('introduced', 'named', 'alias', 'appeared', 'relation', 'same_as')),
    quote    TEXT NOT NULL,
    PRIMARY KEY (book, seq),
    FOREIGN KEY (book, chapter) REFERENCES haiyaku_chapter (book, number),
    FOREIGN KEY (book, document) REFERENCES haiyaku_document (book, ordinal)
);

-- The body of an introduced, named or alias event.
CREATE TABLE haiyaku_name (
    book      TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq       INTEGER NOT NULL,
    entity    TEXT NOT NULL,
    text      TEXT NOT NULL,
    given     TEXT,
    surname   TEXT,
    title     TEXT,
    title_sex TEXT CHECK (title_sex IN ('male', 'female')),
    PRIMARY KEY (book, seq),
    FOREIGN KEY (book, seq) REFERENCES haiyaku_event (book, seq) ON DELETE CASCADE,
    FOREIGN KEY (book, entity) REFERENCES haiyaku_entity (book, id)
);

CREATE TABLE haiyaku_appearance (
    book   TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq    INTEGER NOT NULL,
    entity TEXT NOT NULL,
    PRIMARY KEY (book, seq),
    FOREIGN KEY (book, seq) REFERENCES haiyaku_event (book, seq) ON DELETE CASCADE,
    FOREIGN KEY (book, entity) REFERENCES haiyaku_entity (book, id)
);

CREATE TABLE haiyaku_relation (
    book        TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq         INTEGER NOT NULL,
    from_entity TEXT NOT NULL,
    to_entity   TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN (
        'family', 'romance', 'friend', 'ally', 'rival', 'enemy', 'mentor', 'serves', 'colleague', 'acquaintance',
        'owns'
    )),
    note        TEXT NOT NULL,
    PRIMARY KEY (book, seq),
    CHECK (from_entity <> to_entity),
    FOREIGN KEY (book, seq) REFERENCES haiyaku_event (book, seq) ON DELETE CASCADE,
    FOREIGN KEY (book, from_entity) REFERENCES haiyaku_entity (book, id),
    FOREIGN KEY (book, to_entity) REFERENCES haiyaku_entity (book, id)
);

CREATE TABLE haiyaku_same_as (
    book   TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq    INTEGER NOT NULL,
    entity TEXT NOT NULL,
    other  TEXT NOT NULL,
    PRIMARY KEY (book, seq),
    CHECK (entity <> other),
    FOREIGN KEY (book, seq) REFERENCES haiyaku_event (book, seq) ON DELETE CASCADE,
    FOREIGN KEY (book, entity) REFERENCES haiyaku_entity (book, id),
    FOREIGN KEY (book, other) REFERENCES haiyaku_entity (book, id)
);

CREATE TABLE haiyaku_series (
    name  TEXT NOT NULL PRIMARY KEY,
    title TEXT NOT NULL
);

CREATE TABLE haiyaku_order (
    series TEXT NOT NULL REFERENCES haiyaku_series (name) ON DELETE CASCADE,
    name   TEXT NOT NULL,
    -- 0 is the series' default order.
    rank   INTEGER NOT NULL,
    PRIMARY KEY (series, name),
    UNIQUE (series, rank)
);

CREATE TABLE haiyaku_order_book (
    series     TEXT NOT NULL,
    order_name TEXT NOT NULL,
    position   INTEGER NOT NULL,
    book       TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    PRIMARY KEY (series, order_name, position),
    UNIQUE (series, order_name, book),
    FOREIGN KEY (series, order_name) REFERENCES haiyaku_order (series, name) ON DELETE CASCADE
);

-- A character of a later book is the same person as a character of an earlier book in one reading order.
CREATE TABLE haiyaku_link (
    series         TEXT NOT NULL,
    order_name     TEXT NOT NULL,
    later_book     TEXT NOT NULL,
    later_entity   TEXT NOT NULL,
    earlier_book   TEXT NOT NULL,
    earlier_entity TEXT NOT NULL,
    method         TEXT NOT NULL CHECK (method IN ('name', 'model')),
    -- The model's evidence; a link made on the name alone has none.
    quote          TEXT,
    -- The position in the later book from which the link holds.
    chapter        INTEGER NOT NULL,
    document       INTEGER NOT NULL,
    block          INTEGER NOT NULL,
    PRIMARY KEY (series, order_name, later_book, later_entity),
    CHECK (method = 'name' OR quote IS NOT NULL),
    FOREIGN KEY (series, order_name) REFERENCES haiyaku_order (series, name) ON DELETE CASCADE,
    FOREIGN KEY (later_book, later_entity) REFERENCES haiyaku_entity (book, id),
    FOREIGN KEY (earlier_book, earlier_entity) REFERENCES haiyaku_entity (book, id)
);
