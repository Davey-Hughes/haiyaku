-- Version 2 of the haiyaku schema: the overturned event. A database that has run this file never runs it again,
-- so a change to the schema is a new numbered file beside this one, never an edit here.
--
-- SQLite cannot change a table's CHECK in place, so haiyaku_event is made again with 'overturned' among its
-- types. This runs inside the transaction or savepoint `migrate` holds, where PRAGMA foreign_keys cannot be
-- changed and is whatever the caller set: with it on, dropping haiyaku_event deletes the rows of its child
-- tables by cascade. So the events and their child rows are copied aside, all five tables are made again as
-- they were, and the rows go back, each event before its body. Renaming haiyaku_event aside instead would
-- not do: ALTER TABLE ... RENAME rewrites the foreign keys of the tables that reference it.
--
-- Every name is qualified with main: a bare name finds a temporary table of that name first, and the
-- caller's temporary tables are not haiyaku's to drop or fill.

CREATE TABLE main.haiyaku_v1_event AS SELECT * FROM main.haiyaku_event;
CREATE TABLE main.haiyaku_v1_name AS SELECT * FROM main.haiyaku_name;
CREATE TABLE main.haiyaku_v1_appearance AS SELECT * FROM main.haiyaku_appearance;
CREATE TABLE main.haiyaku_v1_relation AS SELECT * FROM main.haiyaku_relation;
CREATE TABLE main.haiyaku_v1_same_as AS SELECT * FROM main.haiyaku_same_as;

-- The child tables first, so that dropping haiyaku_event has nothing to cascade to.
DROP TABLE main.haiyaku_name;
DROP TABLE main.haiyaku_appearance;
DROP TABLE main.haiyaku_relation;
DROP TABLE main.haiyaku_same_as;
DROP TABLE main.haiyaku_event;

-- As in 001_initial.sql, with 'overturned' among the types.
CREATE TABLE main.haiyaku_event (
    book     TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq      INTEGER NOT NULL,
    chapter  INTEGER NOT NULL,
    document INTEGER NOT NULL,
    block    INTEGER NOT NULL,
    type     TEXT NOT NULL CHECK (
        type IN ('introduced', 'named', 'alias', 'appeared', 'relation', 'same_as', 'overturned')
    ),
    quote    TEXT NOT NULL,
    PRIMARY KEY (book, seq),
    FOREIGN KEY (book, chapter) REFERENCES haiyaku_chapter (book, number),
    FOREIGN KEY (book, document) REFERENCES haiyaku_document (book, ordinal)
);

-- These four exactly as in 001_initial.sql.
CREATE TABLE main.haiyaku_name (
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

CREATE TABLE main.haiyaku_appearance (
    book   TEXT NOT NULL REFERENCES haiyaku_book (sha256),
    seq    INTEGER NOT NULL,
    entity TEXT NOT NULL,
    PRIMARY KEY (book, seq),
    FOREIGN KEY (book, seq) REFERENCES haiyaku_event (book, seq) ON DELETE CASCADE,
    FOREIGN KEY (book, entity) REFERENCES haiyaku_entity (book, id)
);

CREATE TABLE main.haiyaku_relation (
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

CREATE TABLE main.haiyaku_same_as (
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

-- The body of an overturned event: a relation recorded before it that the chapter shows was never true,
-- named as a relation is.
CREATE TABLE main.haiyaku_overturn (
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

INSERT INTO main.haiyaku_event (book, seq, chapter, document, block, type, quote)
    SELECT book, seq, chapter, document, block, type, quote FROM main.haiyaku_v1_event;
INSERT INTO main.haiyaku_name (book, seq, entity, text, given, surname, title, title_sex)
    SELECT book, seq, entity, text, given, surname, title, title_sex FROM main.haiyaku_v1_name;
INSERT INTO main.haiyaku_appearance (book, seq, entity)
    SELECT book, seq, entity FROM main.haiyaku_v1_appearance;
INSERT INTO main.haiyaku_relation (book, seq, from_entity, to_entity, kind, note)
    SELECT book, seq, from_entity, to_entity, kind, note FROM main.haiyaku_v1_relation;
INSERT INTO main.haiyaku_same_as (book, seq, entity, other)
    SELECT book, seq, entity, other FROM main.haiyaku_v1_same_as;

DROP TABLE main.haiyaku_v1_event;
DROP TABLE main.haiyaku_v1_name;
DROP TABLE main.haiyaku_v1_appearance;
DROP TABLE main.haiyaku_v1_relation;
DROP TABLE main.haiyaku_v1_same_as;
