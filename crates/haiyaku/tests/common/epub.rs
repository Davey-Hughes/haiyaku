//! Invented EPUBs, written as zips by the tests that open them, and chapters made by hand for the tests of
//! what is done with a chapter.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use haiyaku::book::{ChapterText, PlacedBlock};
use haiyaku::log::Position;
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
