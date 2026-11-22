//! Invented EPUBs, written as zips by the tests that open them, and chapters made by hand for the tests of
//! what is done with a chapter.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use haiyaku::book::{ChapterText, PlacedBlock};
use haiyaku::extract::{Roster, check};
use haiyaku::log::Position;
use serde_json::{Value, json};
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// The paragraphs of "The Keeper"'s three chapters, which tell the story [`super::keeper`] records: each
/// event's quote opens the paragraph at the block that event gives.
pub const KEEPER_CHAPTERS: [&[&str]; 3] = [
    &[
        "At dusk the keeper climbed to the lamp. The stair went round eleven times, and she counted every turn.",
        "Below the gallery the rocks were black, and the tide was coming in fast over the causeway.",
        "Pip was already on the rail, shouting at the sea. He was a grey gull with one bad foot, and he had \
         shouted at the sea every evening for six years.",
        "The keeper fed Pip, her gull, from her own plate. He took the bread and left the fish, as he always did.",
        "When the lamp was lit she sat down beside him and watched the dark come over the water.",
    ],
    &[
        "In the morning the post boat left a letter on the landing stage.",
        "The letter was addressed to Ada Marsh. She had not seen her own name written down in a long time.",
        "She read it twice on the steps and put it in her apron pocket.",
        "At noon a squall came in from the west and went out again as quickly, leaving the planks of the jetty \
         shining.",
        "On the jetty stood the stranger, his coat dripping. He held his hat in both hands and came no nearer.",
        "Ada nodded to the stranger, and he nodded back. Neither of them said a word.",
        "Pip flew down and walked twice round the stranger's boots, then went back to the rail.",
    ],
    &[
        "The harbour list had one name for the evening boat: Tom Reed. Ada read it on the board by the slipway.",
        "The stranger was waiting at the slipway when she came down with the lantern.",
        "Rain had flattened his hair, and he looked older than he had on the jetty.",
        "The stranger took off his hat, and it was Tom Reed. A scar ran over one eye that had not been there before.",
        "Ada and Tom Reed had been friends since school. She had thought him lost at sea these nine years.",
        "They stood without speaking until the evening boat came round the point.",
        "A boy jumped down from the bow with the mail sack and a rope.",
        "Good night, Mrs. Marsh, said the boy from the boat. Ada lifted the lantern to him and turned for home.",
    ],
];

/// What "The Keeper"'s package says of itself, laid out with the whitespace a package has.
pub const KEEPER_METADATA: &str = "
    <dc:identifier id=\"id\">urn:uuid:00000000-0000-4000-8000-000000000001</dc:identifier>
    <dc:title>
      The Keeper
    </dc:title>
    <dc:language>en</dc:language>";

/// "The Keeper"'s table of contents: each entry's label and its target, from `OEBPS/`.
pub const KEEPER_TOC: [(&str, &str); 4] = [
    ("Chapter 1", "c1.xhtml"),
    ("Chapter 2", "c2.xhtml"),
    ("Chapter 3", "c3.xhtml"),
    ("About the author", "back.xhtml"),
];

/// Writes `dir/name`: a zip whose first entry is `mimetype`, stored, as an EPUB's must be, then `files` in the
/// order given, each an archive path and its bytes.
pub fn epub(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    for (archive_path, bytes) in files {
        zip.start_file(*archive_path, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

/// "The Keeper": front matter, the three chapters of [`KEEPER_CHAPTERS`] and back matter, with an NCX.
pub fn keeper_epub(dir: &Path) -> PathBuf {
    book(dir, "keeper.epub", KEEPER_METADATA, &keeper_documents(), &Toc::Ncx(ncx(&nav_map(&KEEPER_TOC))))
}

/// "The Keeper"'s spine: each document's href from `OEBPS/` and its bytes.
pub fn keeper_documents() -> Vec<(&'static str, Vec<u8>)> {
    let [one, two, three] = KEEPER_CHAPTERS;
    vec![
        ("front.xhtml", xhtml("The Keeper", &["The Keeper", "A story invented to be read by tests."]).into_bytes()),
        ("c1.xhtml", xhtml("Chapter 1", one).into_bytes()),
        ("c2.xhtml", xhtml("Chapter 2", two).into_bytes()),
        ("c3.xhtml", xhtml("Chapter 3", three).into_bytes()),
        (
            "back.xhtml",
            xhtml("About the author", &["About the author", "The author of this story is invented too."]).into_bytes(),
        ),
    ]
}

/// Replies a model could have written to the two calls a run makes for each of chapters 1 to 3 of
/// [`keeper_epub`], in the order it makes them, so that a replay of them serves a whole run: each chapter's
/// reply to its first call, then its reply to its second, which reveals and overturns nothing. The first replies
/// each quote their chapter: chapter 1 introduces the keeper and Pip, whom she owns; chapter 2 names the keeper
/// Ada Marsh and introduces the stranger she meets on the jetty; chapter 3 introduces Tom Reed, calls Ada "Mrs.
/// Marsh", and reveals that the stranger is Tom Reed.
pub fn keeper_replies() -> Vec<String> {
    let firsts = [
        json!({
            "characters": [
                {
                    "ref": "new1",
                    "name": name("the keeper", None, None, None, None),
                    "aliases": [],
                    "evidence": "At dusk the keeper climbed to the lamp.",
                },
                {
                    "ref": "new2",
                    "name": name("Pip", Some("Pip"), None, None, None),
                    "aliases": [],
                    "evidence": "Pip was already on the rail, shouting at the sea.",
                },
            ],
            "relations": [relation(
                "new1",
                "new2",
                "owns",
                "her gull",
                "The keeper fed Pip, her gull, from her own plate.",
            )],
            "same_as": [],
        }),
        json!({
            "characters": [
                {
                    "ref": "c1",
                    "name": name("Ada Marsh", Some("Ada"), Some("Marsh"), None, None),
                    "aliases": [],
                    "evidence": "The letter was addressed to Ada Marsh.",
                },
                {
                    "ref": "new1",
                    "name": name("the stranger", None, None, None, None),
                    "aliases": [],
                    "evidence": "On the jetty stood the stranger, his coat dripping.",
                },
            ],
            "relations": [relation(
                "c1",
                "new1",
                "acquaintance",
                "met on the jetty",
                "Ada nodded to the stranger, and he nodded back.",
            )],
            "same_as": [],
        }),
        json!({
            "characters": [
                {
                    "ref": "new1",
                    "name": name("Tom Reed", Some("Tom"), Some("Reed"), None, None),
                    "aliases": [],
                    "evidence": "The harbour list had one name for the evening boat: Tom Reed.",
                },
                {
                    "ref": "c1",
                    "name": name("Ada Marsh", Some("Ada"), Some("Marsh"), None, None),
                    "aliases": [name("Mrs. Marsh", None, Some("Marsh"), Some("Mrs."), Some("female"))],
                    "evidence": "Good night, Mrs. Marsh, said the boy from the boat.",
                },
            ],
            "relations": [],
            "same_as": [{
                "ref": "new1",
                "other": "c3",
                "evidence": "The stranger took off his hat, and it was Tom Reed.",
            }],
        }),
    ];
    let reveals = r#"{"relations":[],"overturned":[]}"#;
    firsts.iter().flat_map(|first| [first.to_string(), reveals.to_owned()]).collect()
}

/// A name as a reply writes it: its text, the given name, surname and title it labels, and the sex the title
/// states.
pub fn name(
    text: &str,
    given: Option<&str>,
    surname: Option<&str>,
    title: Option<&str>,
    title_sex: Option<&str>,
) -> Value {
    json!({ "text": text, "given": given, "surname": surname, "title": title, "title_sex": title_sex })
}

/// A relation item of a reply.
pub fn relation(from: &str, to: &str, kind: &str, note: &str, evidence: &str) -> Value {
    json!({ "from": from, "to": to, "kind": kind, "note": note, "evidence": evidence })
}

/// A book's table of contents, as [`book`] writes it.
pub enum Toc {
    /// An NCX at `OEBPS/toc.ncx`, which the spine's `toc` names.
    Ncx(String),
    /// An EPUB 3 navigation document at `OEBPS/nav.xhtml`, in the manifest with the `nav` property and not
    /// in the spine.
    Nav(String),
    /// An NCX and a navigation document, each as above. `None` lists the NCX in the manifest and leaves its
    /// file out of the archive.
    Both(Option<String>, String),
    None,
}

/// Writes `dir/name`, an EPUB whose package is `OEBPS/content.opf` with `metadata` in its metadata element:
/// `documents`, each an href from `OEBPS/` and its bytes, make its manifest and its spine in that order, and
/// `toc` is its table of contents.
pub fn book(dir: &Path, name: &str, metadata: &str, documents: &[(&str, Vec<u8>)], toc: &Toc) -> PathBuf {
    let mut manifest: Vec<String> = documents
        .iter()
        .enumerate()
        .map(|(id, (href, _))| format!(r#"<item id="d{id}" href="{href}" media-type="application/xhtml+xml"/>"#))
        .collect();
    let mut spine_toc = "";
    let mut files: Vec<(String, &[u8])> =
        documents.iter().map(|(href, bytes)| (format!("OEBPS/{href}"), bytes.as_slice())).collect();
    let (ncx, nav) = match toc {
        Toc::Ncx(markup) => (Some(Some(markup)), None),
        Toc::Nav(markup) => (None, Some(markup)),
        Toc::Both(ncx, nav) => (Some(ncx.as_ref()), Some(nav)),
        Toc::None => (None, None),
    };
    if let Some(markup) = ncx {
        manifest.push(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#.into());
        spine_toc = r#" toc="ncx""#;
        files.extend(markup.map(|markup| ("OEBPS/toc.ncx".into(), markup.as_bytes())));
    }
    if let Some(markup) = nav {
        manifest
            .push(r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#.into());
        files.push(("OEBPS/nav.xhtml".into(), markup.as_bytes()));
    }
    let itemrefs: String = (0..documents.len()).map(|id| format!(r#"<itemref idref="d{id}"/>"#)).collect();
    let opf = package(&format!(
        "<metadata>{metadata}</metadata><manifest>{}</manifest><spine{spine_toc}>{itemrefs}</spine>",
        manifest.join("\n")
    ));
    let container = container();
    let mut all: Vec<(&str, &[u8])> =
        vec![("META-INF/container.xml", container.as_bytes()), ("OEBPS/content.opf", opf.as_bytes())];
    all.extend(files.iter().map(|(path, bytes)| (path.as_str(), *bytes)));
    epub(dir, name, &all)
}

/// `META-INF/container.xml`, naming the package `OEBPS/content.opf`.
pub fn container() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
        .into()
}

/// A package document whose package element holds `inside`: its metadata, manifest and spine, as written.
pub fn package(inside: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"
    xmlns:dc="http://purl.org/dc/elements/1.1/">
{inside}
</package>"#
    )
}

/// An XHTML document titled `title` in its head, whose body holds one paragraph for each of `paragraphs`.
pub fn xhtml(title: &str, paragraphs: &[&str]) -> String {
    let body: String = paragraphs.iter().map(|paragraph| format!("\n  <p>{paragraph}</p>")).collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>{title}</title></head>
<body>{body}
</body>
</html>"#
    )
}

/// An NCX whose head and title are followed by `after_title`: its navMap, and any page or navigation list.
pub fn ncx(after_title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<head><meta name="dtb:uid" content="urn:uuid:00000000-0000-4000-8000-000000000001"/></head>
<docTitle><text>The Keeper</text></docTitle>
{after_title}
</ncx>"#
    )
}

/// A navMap of one navPoint for each of `entries`, a label and a target, with the whitespace an NCX has
/// around its labels.
pub fn nav_map(entries: &[(&str, &str)]) -> String {
    let points: String = (1..)
        .zip(entries)
        .map(|(order, (label, target))| {
            format!(
                r#"
  <navPoint id="p{order}" playOrder="{order}">
    <navLabel>
      <text>{label}</text>
    </navLabel>
    <content src="{target}"/>
  </navPoint>"#
            )
        })
        .collect();
    format!("<navMap>{points}\n</navMap>")
}

/// A navigation document whose body holds `body`: its navigation elements.
pub fn nav(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head>
<body>
{body}
</body>
</html>"#
    )
}

/// The `toc` navigation element listing `entries`, a label and a target each.
pub fn toc_nav(entries: &[(&str, &str)]) -> String {
    let items: String = entries
        .iter()
        .map(|(label, target)| format!("\n    <li><a href=\"{target}\">\n      {label}\n    </a></li>"))
        .collect();
    format!("<nav epub:type=\"toc\" id=\"toc\">\n  <h1>Contents</h1>\n  <ol>{items}\n  </ol>\n</nav>")
}

/// Chapter `number`, entry `number` of its table of contents, made of the one document `document`, whose
/// blocks are `blocks` from block 0.
pub fn chapter_text(number: u32, document: u32, blocks: &[&str]) -> ChapterText {
    ChapterText {
        number,
        toc_entry: number,
        title: format!("Chapter {number}"),
        documents: document..document + 1,
        blocks: (0..)
            .zip(blocks)
            .map(|(block, text)| PlacedBlock { at: Position { document, block }, text: (*text).into() })
            .collect(),
    }
}

/// The roster after three invented chapters run through [`check`] (chapter 1 introduces Bren Holt and Old Sal,
/// his grandmother, with a `family` relation between them; chapter 2 introduces Mira; chapter 3 lists Mira
/// again), with chapter 4, whose text names only Old Sal.
pub fn relevant_roster() -> (Roster, ChapterText) {
    let chapters = [
        (
            chapter_text(
                1,
                1,
                &[
                    "Bren Holt hauled the boat up the shingle.",
                    "Old Sal sat on the sea wall, mending nets.",
                    "She was his grandmother, and had taught him every knot he knew.",
                ],
            ),
            json!({
                "characters": [
                    {
                        "ref": "bren",
                        "name": name("Bren Holt", Some("Bren"), Some("Holt"), None, None),
                        "aliases": [],
                        "evidence": "Bren Holt hauled the boat up the shingle",
                    },
                    {
                        "ref": "sal",
                        "name": name("Old Sal", Some("Sal"), None, None, None),
                        "aliases": [],
                        "evidence": "Old Sal sat on the sea wall",
                    },
                ],
                "relations": [relation("sal", "bren", "family", "his grandmother", "She was his grandmother")],
                "same_as": [],
            }),
        ),
        (
            chapter_text(2, 2, &["Mira came down from the lighthouse with a lantern."]),
            json!({
                "characters": [{
                    "ref": "mira",
                    "name": name("Mira", Some("Mira"), None, None, None),
                    "aliases": [],
                    "evidence": "Mira came down from the lighthouse",
                }],
                "relations": [],
                "same_as": [],
            }),
        ),
        (
            chapter_text(3, 3, &["Mira counted the boats in the harbour twice."]),
            json!({
                "characters": [{
                    "ref": "c3",
                    "name": name("Mira", Some("Mira"), None, None, None),
                    "aliases": [],
                    "evidence": "Mira counted the boats in the harbour",
                }],
                "relations": [],
                "same_as": [],
            }),
        ),
    ];
    let mut roster = Roster::new();
    for (chapter, reply) in &chapters {
        let outcome = check(&mut roster, chapter, reply);
        // Every item kept, so the roster is the one this function says it is.
        let items: usize =
            ["characters", "relations", "same_as"].map(|key| reply[key].as_array().unwrap().len()).iter().sum();
        assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
        assert_eq!(outcome.kept as usize, items);
    }
    (roster, chapter_text(4, 4, &["Old Sal mended the last of the nets alone."]))
}
