//! Opening invented EPUBs, written as zips by the tests: the spine, the table of contents, and the chapters
//! the one cuts the other into.

#![cfg(feature = "extract")]

mod common;

use std::ops::Range;

use common::epub::{
    KEEPER_CHAPTERS, KEEPER_METADATA, KEEPER_TOC, Toc, book, container, epub, keeper_documents, keeper_epub, nav,
    nav_map, ncx, package, toc_nav, xhtml,
};
use haiyaku::book::{Book, TocEntry};
use haiyaku::log::Position;
use haiyaku::store::sha256_file;
use haiyaku::{Error, Result};
use tempfile::TempDir;

fn entry(label: &str, document: u32) -> TocEntry {
    TocEntry { label: label.into(), document }
}

fn keeper(dir: &TempDir) -> Book {
    Book::open(&keeper_epub(dir.path())).unwrap()
}

/// The keeper's spine with `toc` for its table of contents.
fn keeper_with(dir: &TempDir, name: &str, toc: &Toc) -> Result<Book> {
    Book::open(&book(dir.path(), name, KEEPER_METADATA, &keeper_documents(), toc))
}

/// The chapter of each spine document when chapters `first` to `last` are chosen.
fn chapter_of_each(book: &Book, first: u32, last: u32) -> Vec<Option<u32>> {
    let chapters = book.chapters(first, last).unwrap();
    book.document_rows(&chapters).into_iter().map(|row| row.chapter).collect()
}

/// Writes `dir/name`: the container, the package `OEBPS/content.opf` holding `inside`, and `files`, each an
/// archive path and its bytes.
fn with_package(dir: &TempDir, name: &str, inside: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let (container, opf) = (container(), package(inside));
    let mut all: Vec<(&str, &[u8])> =
        vec![("META-INF/container.xml", container.as_bytes()), ("OEBPS/content.opf", opf.as_bytes())];
    all.extend_from_slice(files);
    epub(dir.path(), name, &all)
}

fn epub_error(opened: Result<Book>) -> String {
    match opened {
        Err(Error::Epub(why)) => why,
        other => panic!("expected Error::Epub, got {other:?}"),
    }
}

#[test]
fn opens_the_keeper() {
    let dir = TempDir::new().unwrap();
    let path = keeper_epub(dir.path());
    let book = Book::open(&path).unwrap();
    assert_eq!(book.title, "The Keeper");
    assert_eq!(book.language, "en");
    assert_eq!(book.sha256, sha256_file(&path).unwrap());
    let paths: Vec<&str> = book.documents.iter().map(|document| document.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/front.xhtml", "OEBPS/c1.xhtml", "OEBPS/c2.xhtml", "OEBPS/c3.xhtml", "OEBPS/back.xhtml"]);
    assert_eq!(
        book.toc,
        [entry("Chapter 1", 1), entry("Chapter 2", 2), entry("Chapter 3", 3), entry("About the author", 4)]
    );
}

#[test]
fn the_keeper_tells_what_its_log_records() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    for event in common::keeper().events {
        let document = &book.documents[event.at.document as usize];
        let block = &document.blocks[event.at.block as usize];
        assert!(
            block.text.starts_with(&event.quote),
            "event {} is not at {}: {:?}",
            event.seq,
            document.path,
            block.text
        );
    }
}

#[test]
fn chapters_are_runs_of_whole_documents() {
    let dir = TempDir::new().unwrap();
    let chapters = keeper(&dir).chapters(1, 3).unwrap();
    let cut: Vec<(u32, u32, Range<u32>)> =
        chapters.iter().map(|chapter| (chapter.number, chapter.toc_entry, chapter.documents.clone())).collect();
    assert_eq!(cut, [(1, 1, 1..2), (2, 2, 2..3), (3, 3, 3..4)]);
    for (chapter, paragraphs) in chapters.iter().zip(KEEPER_CHAPTERS) {
        assert_eq!(chapter.title, format!("Chapter {}", chapter.number));
        let at: Vec<&Position> = chapter.blocks.iter().map(|block| &block.at).collect();
        let expected: Vec<Position> =
            (0..).zip(paragraphs).map(|(block, _)| Position { document: chapter.documents.start, block }).collect();
        assert_eq!(at, expected.iter().collect::<Vec<_>>(), "chapter {}", chapter.number);
        assert_eq!(chapter.text(), paragraphs.join("\n"));
        assert_eq!(chapter.text().lines().count(), chapter.blocks.len());
    }
}

#[test]
fn document_rows_mark_front_and_back_matter() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    let rows = book.document_rows(&book.chapters(1, 3).unwrap());
    let ordinals: Vec<u32> = rows.iter().map(|row| row.ordinal).collect();
    assert_eq!(ordinals, [0, 1, 2, 3, 4]);
    for (row, document) in rows.iter().zip(&book.documents) {
        assert_eq!(row.path, document.path);
    }
    assert_eq!(chapter_of_each(&book, 1, 3), [None, Some(1), Some(2), Some(3), None]);
}

#[test]
fn a_later_range_renumbers_chapters() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    let chapters = book.chapters(2, 3).unwrap();
    let numbered: Vec<(u32, u32)> = chapters.iter().map(|chapter| (chapter.number, chapter.toc_entry)).collect();
    assert_eq!(numbered, [(1, 2), (2, 3)]);
    assert_eq!(chapters[0].title, "Chapter 2");
    assert_eq!(chapter_of_each(&book, 2, 3), [None, None, Some(1), Some(2), None]);
}

#[test]
fn a_range_that_stops_early_leaves_the_rest_in_no_chapter() {
    let dir = TempDir::new().unwrap();
    assert_eq!(chapter_of_each(&keeper(&dir), 1, 2), [None, Some(1), Some(2), None, None]);
}

#[test]
fn chapter_range_out_of_bounds_is_invalid() {
    let dir = TempDir::new().unwrap();
    let book = keeper(&dir);
    for (first, last) in [(0, 2), (3, 2), (2, 9)] {
        match book.chapters(first, last) {
            Err(Error::Invalid(why)) => assert!(why.contains("1-4"), "{first}-{last}: {why}"),
            other => panic!("{first}-{last}: expected Error::Invalid, got {other:?}"),
        }
    }
}

#[test]
fn last_entry_runs_to_the_end_of_the_spine() {
    let dir = TempDir::new().unwrap();
    let chapters = keeper(&dir).chapters(4, 4).unwrap();
    let [chapter] = chapters.as_slice() else { panic!("{chapters:?}") };
    assert_eq!((chapter.title.as_str(), chapter.documents.clone()), ("About the author", 4..5));
}

#[test]
fn nav_toc_without_ncx() {
    let dir = TempDir::new().unwrap();
    let book = keeper_with(&dir, "nav.epub", &Toc::Nav(nav(&toc_nav(&KEEPER_TOC)))).unwrap();
    assert_eq!(book.toc, keeper(&dir).toc);
    assert_eq!(book.documents.len(), 5, "the navigation document is not in the spine");
}

#[test]
fn toc_entries_inside_one_document_fold_into_the_first() {
    let dir = TempDir::new().unwrap();
    // The section is nested inside its chapter, as an NCX writes one, and the title page comes last.
    let map = r#"<navMap>
  <navPoint id="p1"><navLabel><text>Chapter 1</text></navLabel><content src="c1.xhtml"/>
    <navPoint id="p2"><navLabel><text>The lamp room</text></navLabel><content src="c1.xhtml#s2"/></navPoint>
  </navPoint>
  <navPoint id="p3"><navLabel><text>Chapter 2</text></navLabel><content src="c2.xhtml"/></navPoint>
  <navPoint id="p4"><navLabel><text>Title page</text></navLabel><content src="front.xhtml"/></navPoint>
</navMap>"#;
    let book = keeper_with(&dir, "sections.epub", &Toc::Ncx(ncx(map))).unwrap();
    assert_eq!(book.toc, [entry("Chapter 1", 1), entry("Chapter 2", 2)]);
}

#[test]
fn a_contents_entry_late_in_the_spine_costs_no_chapter() {
    let dir = TempDir::new().unwrap();
    // The contents page is the spine's last document, and its entry comes before the chapters'.
    let entries = [
        ("Title", "front.xhtml"),
        ("Contents", "back.xhtml"),
        ("Chapter 1", "c1.xhtml"),
        ("Chapter 2", "c2.xhtml"),
        ("Chapter 3", "c3.xhtml"),
    ];
    let book = keeper_with(&dir, "late-contents.epub", &Toc::Ncx(ncx(&nav_map(&entries)))).unwrap();
    assert_eq!(book.toc, [entry("Title", 0), entry("Chapter 1", 1), entry("Chapter 2", 2), entry("Chapter 3", 3)]);
}

#[test]
fn of_two_runs_as_long_the_one_with_the_earlier_entries_is_kept() {
    let dir = TempDir::new().unwrap();
    // B-D and C-D both rise through two documents; B comes first. A alone is a shorter run.
    let entries = [("A", "back.xhtml"), ("B", "c2.xhtml"), ("C", "c1.xhtml"), ("D", "c3.xhtml")];
    let book = keeper_with(&dir, "ties.epub", &Toc::Ncx(ncx(&nav_map(&entries)))).unwrap();
    assert_eq!(book.toc, [entry("B", 2), entry("D", 3)]);
}

#[test]
fn only_the_first_label_of_a_nav_point_is_read() {
    let dir = TempDir::new().unwrap();
    let map = r#"<navMap>
  <navPoint id="p1"><navLabel><text>Chapter 1</text></navLabel><navLabel xml:lang="ko"><text>제1장</text></navLabel>
    <content src="c1.xhtml"/></navPoint>
</navMap>"#;
    let book = keeper_with(&dir, "labels.epub", &Toc::Ncx(ncx(map))).unwrap();
    assert_eq!(book.toc, [entry("Chapter 1", 1)]);
}

#[test]
fn a_missing_ncx_leaves_the_nav() {
    let dir = TempDir::new().unwrap();
    let toc = Toc::Both(None, nav(&toc_nav(&KEEPER_TOC)));
    assert_eq!(keeper_with(&dir, "lost-ncx.epub", &toc).unwrap().toc, keeper(&dir).toc);
}

#[test]
fn an_ncx_that_keeps_no_entry_leaves_the_nav() {
    let dir = TempDir::new().unwrap();
    let outside = ncx(&nav_map(&[("Cover", "cover.xhtml")]));
    let toc = Toc::Both(Some(outside), nav(&toc_nav(&KEEPER_TOC)));
    assert_eq!(keeper_with(&dir, "outside-ncx.epub", &toc).unwrap().toc, keeper(&dir).toc);
}

#[test]
fn a_picture_in_the_spine_keeps_its_place() {
    let dir = TempDir::new().unwrap();
    let inside = r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="map" href="map.jpg" media-type="image/jpeg"/>
  <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/><itemref idref="map"/><itemref idref="c2"/></spine>"#;
    let (one, two) = (xhtml("Chapter 1", &["The lamp."]), xhtml("Chapter 2", &["The jetty."]));
    let toc = ncx(&nav_map(&[("Chapter 1", "c1.xhtml"), ("Chapter 2", "c2.xhtml")]));
    let path = with_package(
        &dir,
        "picture.epub",
        inside,
        &[
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("OEBPS/map.jpg", &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]),
            ("OEBPS/c2.xhtml", two.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    let paths: Vec<&str> = book.documents.iter().map(|document| document.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/c1.xhtml", "OEBPS/map.jpg", "OEBPS/c2.xhtml"]);
    assert!(book.documents[1].blocks.is_empty());
    assert_eq!(book.toc, [entry("Chapter 1", 0), entry("Chapter 2", 2)]);
    assert_eq!(book.chapters(1, 1).unwrap()[0].documents, 0..2);
}

#[test]
fn an_ncx_is_found_by_its_media_type_in_any_case_and_with_parameters() {
    let dir = TempDir::new().unwrap();
    // The spine names no NCX, so only the media type finds it.
    let inside = r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
  <item id="contents" href="toc.ncx" media-type="application/X-DTBNCX+XML; charset=utf-8"/>
</manifest>
<spine><itemref idref="c1"/><itemref idref="c2"/></spine>"#;
    let (one, two) = (xhtml("Chapter 1", &["The lamp."]), xhtml("Chapter 2", &["The jetty."]));
    let toc = ncx(&nav_map(&[("Chapter 1", "c1.xhtml"), ("Chapter 2", "c2.xhtml")]));
    let path = with_package(
        &dir,
        "typed-ncx.epub",
        inside,
        &[("OEBPS/c1.xhtml", one.as_bytes()), ("OEBPS/c2.xhtml", two.as_bytes()), ("OEBPS/toc.ncx", toc.as_bytes())],
    );
    assert_eq!(Book::open(&path).unwrap().toc, [entry("Chapter 1", 0), entry("Chapter 2", 1)]);
}

#[test]
fn a_spine_item_of_a_text_or_unknown_media_type_that_is_utf8_is_read() {
    let dir = TempDir::new().unwrap();
    let inside = r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml"/>
  <item id="c2" href="c2.xhtml" media-type="Application/XHTML+XML; charset=utf-8"/>
  <item id="c3" href="c3.xml" media-type="application/x-invented+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c3"/></spine>"#;
    let (one, two) = (xhtml("Chapter 1", &["The lamp."]), xhtml("Chapter 2", &["The jetty."]));
    let three = xhtml("Chapter 3", &["The stranger."]);
    let toc = ncx(&nav_map(&[("Chapter 1", "c1.xhtml"), ("Chapter 2", "c2.xhtml"), ("Chapter 3", "c3.xml")]));
    let path = with_package(
        &dir,
        "types.epub",
        inside,
        &[
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("OEBPS/c2.xhtml", two.as_bytes()),
            ("OEBPS/c3.xml", three.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    let texts: Vec<String> = book.chapters(1, 3).unwrap().iter().map(|chapter| chapter.text()).collect();
    assert_eq!(texts, ["The lamp.", "The jetty.", "The stranger."]);
}

#[test]
fn a_spine_item_of_no_media_type_that_is_not_utf8_keeps_its_place() {
    let dir = TempDir::new().unwrap();
    let inside = r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="cover" href="cover.jpg"/>
  <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/><itemref idref="cover"/><itemref idref="c2"/></spine>"#;
    let (one, two) = (xhtml("Chapter 1", &["The lamp."]), xhtml("Chapter 2", &["The jetty."]));
    let toc = ncx(&nav_map(&[("Chapter 1", "c1.xhtml"), ("Chapter 2", "c2.xhtml")]));
    let path = with_package(
        &dir,
        "cover.epub",
        inside,
        &[
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("OEBPS/cover.jpg", &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]),
            ("OEBPS/c2.xhtml", two.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    let paths: Vec<&str> = book.documents.iter().map(|document| document.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/c1.xhtml", "OEBPS/cover.jpg", "OEBPS/c2.xhtml"]);
    assert!(book.documents[1].blocks.is_empty());
    assert_eq!(book.toc, [entry("Chapter 1", 0), entry("Chapter 2", 2)]);
    let texts: Vec<String> = book.chapters(1, 2).unwrap().iter().map(|chapter| chapter.text()).collect();
    assert_eq!(texts, ["The lamp.", "The jetty."]);
}

#[test]
fn an_empty_or_fragment_only_manifest_href_names_no_file() {
    let dir = TempDir::new().unwrap();
    let inside = r##"<metadata/>
<manifest>
  <item id="empty" href="" media-type="application/xhtml+xml"/>
  <item id="frag" href="#top" media-type="application/xhtml+xml"/>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="empty"/><itemref idref="frag"/><itemref idref="c1"/></spine>"##;
    let (one, toc) = (xhtml("Chapter 1", &["The lamp."]), ncx(&nav_map(&[("Chapter 1", "c1.xhtml")])));
    let path = with_package(
        &dir,
        "empty-href.epub",
        inside,
        &[("OEBPS/c1.xhtml", one.as_bytes()), ("OEBPS/toc.ncx", toc.as_bytes())],
    );
    let book = Book::open(&path).unwrap();
    let paths: Vec<&str> = book.documents.iter().map(|document| document.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/c1.xhtml"]);
}

#[test]
fn a_rootfile_that_names_no_path_is_passed_over() {
    let dir = TempDir::new().unwrap();
    let container = container().replace(
        "<rootfile ",
        r#"<rootfile full-path="./" media-type="application/oebps-package+xml"/>
    <rootfile "#,
    );
    let opf = package(
        r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/></spine>"#,
    );
    let (one, toc) = (xhtml("Chapter 1", &["The lamp."]), ncx(&nav_map(&[("Chapter 1", "c1.xhtml")])));
    let path = epub(
        dir.path(),
        "two-rootfiles.epub",
        &[
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    assert_eq!(Book::open(&path).unwrap().toc, [entry("Chapter 1", 0)]);
}

#[test]
fn a_navigation_document_in_the_spine_links_to_its_own_sections() {
    let dir = TempDir::new().unwrap();
    let inside = r##"<metadata/>
<manifest><item id="all" href="all.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest>
<spine><itemref idref="all"/></spine>"##;
    let body = format!(
        r##"{}
<h1 id="c1">Chapter 1</h1><p>The lamp.</p><h1 id="c2">Chapter 2</h1><p>The jetty.</p>"##,
        toc_nav(&[("Chapter 1", "#c1"), ("Chapter 2", "#c2")])
    );
    let all = nav(&body);
    let book =
        Book::open(&with_package(&dir, "one-file.epub", inside, &[("OEBPS/all.xhtml", all.as_bytes())])).unwrap();
    assert_eq!(book.toc, [entry("Chapter 1", 0)]);
}

#[test]
fn hrefs_that_climb_out_of_the_archive_or_have_a_bad_escape_name_nothing() {
    let dir = TempDir::new().unwrap();
    let inside = r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="up" href="../../c2.xhtml" media-type="application/xhtml+xml"/>
  <item id="pct" href="100%.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/><itemref idref="up"/><itemref idref="pct"/></spine>"#;
    let (one, two, three) = (
        xhtml("Chapter 1", &["The lamp."]),
        xhtml("Chapter 2", &["The jetty."]),
        xhtml("Chapter 3", &["The slipway."]),
    );
    let toc = ncx(&nav_map(&[("Climbed", "../../OEBPS/c1.xhtml"), ("Chapter 1", "c1.xhtml")]));
    let path = with_package(
        &dir,
        "out.epub",
        inside,
        &[
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("c2.xhtml", two.as_bytes()),
            ("OEBPS/100%.xhtml", three.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    let paths: Vec<&str> = book.documents.iter().map(|document| document.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/c1.xhtml"]);
    assert_eq!(book.toc, [entry("Chapter 1", 0)]);
}

#[test]
fn the_package_path_is_resolved_as_an_href_is() {
    let dir = TempDir::new().unwrap();
    let container = container().replace("full-path=\"OEBPS/", "full-path=\"./OEBPS/");
    let opf = package(
        r#"<metadata/>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/></spine>"#,
    );
    let (one, toc) = (xhtml("Chapter 1", &["The lamp."]), ncx(&nav_map(&[("Chapter 1", "c1.xhtml")])));
    let path = epub(
        dir.path(),
        "dot.epub",
        &[
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(book.documents[0].path, "OEBPS/c1.xhtml");
    assert_eq!(book.toc, [entry("Chapter 1", 0)]);
}

#[test]
fn only_the_nav_map_of_an_ncx_is_read() {
    let dir = TempDir::new().unwrap();
    let after_title = format!(
        r#"{}
<pageList><pageTarget id="t1" type="normal" value="9"><navLabel><text>9</text></navLabel>
  <content src="c3.xhtml#page9"/></pageTarget></pageList>
<navList><navLabel><text>Notes</text></navLabel><navTarget id="t2"><navLabel><text>Note 1</text></navLabel>
  <content src="back.xhtml#n1"/></navTarget></navList>"#,
        nav_map(&[("Chapter 2", "c2.xhtml")])
    );
    let book = keeper_with(&dir, "pages.epub", &Toc::Ncx(ncx(&after_title))).unwrap();
    assert_eq!(book.toc, [entry("Chapter 2", 2)]);
}

#[test]
fn only_the_toc_nav_is_read() {
    let dir = TempDir::new().unwrap();
    let body = format!(
        r#"<nav epub:type="landmarks"><ol><li><a epub:type="bodymatter" href="c2.xhtml">Begin</a></li></ol></nav>
{}
<nav epub:type="page-list"><ol><li><a href="c3.xhtml#page9">9</a></li></ol></nav>"#,
        toc_nav(&[("Chapter 1", "c1.xhtml")])
    );
    let book = keeper_with(&dir, "landmarks.epub", &Toc::Nav(nav(&body))).unwrap();
    assert_eq!(book.toc, [entry("Chapter 1", 1)]);
}

#[test]
fn chapter_of_several_documents() {
    let dir = TempDir::new().unwrap();
    let documents = [
        ("c1a.xhtml", xhtml("Chapter 1", &["The lamp.", "The stair."]).into_bytes()),
        ("c1b.xhtml", xhtml("Chapter 1", &["The gallery."]).into_bytes()),
        ("c2.xhtml", xhtml("Chapter 2", &["The jetty."]).into_bytes()),
    ];
    let toc = Toc::Ncx(ncx(&nav_map(&[("Chapter 1", "c1a.xhtml"), ("Chapter 2", "c2.xhtml")])));
    let book = Book::open(&book(dir.path(), "several.epub", KEEPER_METADATA, &documents, &toc)).unwrap();
    let chapters = book.chapters(1, 2).unwrap();
    assert_eq!(chapters[0].documents, 0..2);
    let at: Vec<(u32, u32)> = chapters[0].blocks.iter().map(|block| (block.at.document, block.at.block)).collect();
    assert_eq!(at, [(0, 0), (0, 1), (1, 0)]);
    assert_eq!(chapters[0].text(), "The lamp.\nThe stair.\nThe gallery.");
    assert_eq!(chapter_of_each(&book, 1, 2), [Some(1), Some(1), Some(2)]);
}

#[test]
fn percent_encoded_and_nested_paths() {
    let dir = TempDir::new().unwrap();
    // The spine has no `toc`: the NCX is found by its media type.
    let opf = package(
        r#"<metadata><dc:title>Paths</dc:title></metadata>
<manifest>
  <item id="one" href="text/chapter%20one.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="nav/toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine><itemref idref="one"/></spine>"#,
    );
    let toc = ncx(&nav_map(&[("One", "../text/./chapter%20one.xhtml#start")]));
    let chapter = xhtml("One", &["Only."]);
    let container = container();
    let path = epub(
        dir.path(),
        "paths.epub",
        &[
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/text/chapter one.xhtml", chapter.as_bytes()),
            ("OEBPS/nav/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    assert_eq!(book.documents[0].path, "OEBPS/text/chapter one.xhtml");
    assert_eq!(book.toc, [entry("One", 0)]);
}

#[test]
fn the_spine_is_every_itemref_that_names_a_document_once() {
    let dir = TempDir::new().unwrap();
    // A non-linear document stays; an idref the manifest lacks, an item the archive lacks and a document
    // listed again are passed over.
    let opf = package(
        r#"<metadata/>
<manifest>
  <item id="front" href="front.xhtml" media-type="application/xhtml+xml"/>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="lost" href="lost.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx">
  <itemref idref="front" linear="no"/><itemref idref="nowhere"/><itemref idref="lost"/>
  <itemref idref="c1"/><itemref idref="front"/>
</spine>"#,
    );
    let toc = ncx(&nav_map(&[("Chapter 1", "c1.xhtml")]));
    let front = xhtml("The Keeper", &["The Keeper"]);
    let one = xhtml("Chapter 1", KEEPER_CHAPTERS[0]);
    let container = container();
    let path = epub(
        dir.path(),
        "spine.epub",
        &[
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/front.xhtml", front.as_bytes()),
            ("OEBPS/c1.xhtml", one.as_bytes()),
            ("OEBPS/toc.ncx", toc.as_bytes()),
        ],
    );
    let book = Book::open(&path).unwrap();
    let paths: Vec<&str> = book.documents.iter().map(|document| document.path.as_str()).collect();
    assert_eq!(paths, ["OEBPS/front.xhtml", "OEBPS/c1.xhtml"]);
    assert_eq!(book.toc, [entry("Chapter 1", 1)]);
}

#[test]
fn non_utf8_document_is_refused() {
    let dir = TempDir::new().unwrap();
    let mut documents = keeper_documents();
    documents[2].1 = vec![0xFF, 0xFE, 0x00, 0x3C];
    let toc = Toc::Ncx(ncx(&nav_map(&KEEPER_TOC)));
    let why = epub_error(Book::open(&book(dir.path(), "utf16.epub", KEEPER_METADATA, &documents, &toc)));
    assert!(why.contains("OEBPS/c2.xhtml"), "{why}");
}

#[test]
fn a_leading_byte_order_mark_is_no_text() {
    let dir = TempDir::new().unwrap();
    let mut documents = keeper_documents();
    documents[1].1.splice(0..0, *b"\xEF\xBB\xBF");
    let toc = Toc::Ncx(format!("\u{feff}{}", ncx(&nav_map(&KEEPER_TOC))));
    let book = Book::open(&book(dir.path(), "bom.epub", KEEPER_METADATA, &documents, &toc)).unwrap();
    assert_eq!(book.toc, keeper(&dir).toc);
    assert_eq!(book.chapters(1, 1).unwrap()[0].text(), KEEPER_CHAPTERS[0].join("\n"));
}

#[test]
fn missing_title_and_language_have_defaults() {
    let dir = TempDir::new().unwrap();
    let toc = Toc::Ncx(ncx(&nav_map(&KEEPER_TOC)));
    for (name, metadata) in [("untitled.epub", ""), ("blank.epub", "<dc:title> </dc:title><dc:language/>")] {
        let book = Book::open(&book(dir.path(), name, metadata, &keeper_documents(), &toc)).unwrap();
        assert_eq!((book.title.as_str(), book.language.as_str()), (name.trim_end_matches(".epub"), "und"));
    }
}

#[test]
fn not_an_epub() {
    let dir = TempDir::new().unwrap();
    let text = dir.path().join("notes.epub");
    std::fs::write(&text, "Plain text, not a zip.").unwrap();
    assert!(epub_error(Book::open(&text)).contains("zip"));

    let opf = package("<metadata/><manifest/><spine/>");
    let container = container();
    let no_container = epub(dir.path(), "no-container.epub", &[("OEBPS/content.opf", opf.as_bytes())]);
    assert!(epub_error(Book::open(&no_container)).contains("META-INF/container.xml"));

    let no_package = epub(dir.path(), "no-package.epub", &[("META-INF/container.xml", container.as_bytes())]);
    assert!(epub_error(Book::open(&no_package)).contains("OEBPS/content.opf"));

    let no_rootfile = r#"<?xml version="1.0"?><container version="1.0"><rootfiles/></container>"#;
    let unnamed = epub(dir.path(), "no-rootfile.epub", &[("META-INF/container.xml", no_rootfile.as_bytes())]);
    assert!(epub_error(Book::open(&unnamed)).contains("names no package"));

    let empty = epub(
        dir.path(),
        "empty.epub",
        &[("META-INF/container.xml", container.as_bytes()), ("OEBPS/content.opf", opf.as_bytes())],
    );
    assert!(epub_error(Book::open(&empty)).contains("spine"));
}

#[test]
fn a_book_that_is_not_there_is_an_io_error() {
    let dir = TempDir::new().unwrap();
    match Book::open(&dir.path().join("missing.epub")) {
        Err(Error::Io(error)) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}"),
        other => panic!("expected Error::Io, got {other:?}"),
    }
}

#[test]
fn an_epub_cut_short_anywhere_is_an_epub_error() {
    let dir = TempDir::new().unwrap();
    let whole = std::fs::read(keeper_epub(dir.path())).unwrap();
    let cut = dir.path().join("cut.epub");
    // A book cut short anywhere is Error::Epub, never Error::Io and never a panic.
    let not_epub: Vec<String> = (0..whole.len())
        .filter_map(|length| {
            std::fs::write(&cut, &whole[..length]).unwrap();
            match Book::open(&cut) {
                Err(Error::Epub(_)) => None,
                other => Some(format!("{length}: {other:?}")),
            }
        })
        .collect();
    assert!(not_epub.is_empty(), "{} of {} cuts: {:?}", not_epub.len(), whole.len(), not_epub.first());
}

#[test]
fn no_table_of_contents_is_an_error() {
    let dir = TempDir::new().unwrap();
    assert!(epub_error(keeper_with(&dir, "none.epub", &Toc::None)).contains("table of contents"));
    // A table whose every entry points outside the spine leaves nothing to cut chapters at.
    let outside = Toc::Ncx(ncx(&nav_map(&[("Cover", "cover.xhtml")])));
    assert!(epub_error(keeper_with(&dir, "outside.epub", &outside)).contains("table of contents"));
    // Every table is tried before the book is refused, and the error names each.
    let neither = Toc::Both(None, nav(&toc_nav(&[("Cover", "cover.xhtml")])));
    let why = epub_error(keeper_with(&dir, "neither.epub", &neither));
    assert!(
        why.contains("table of contents") && why.contains("OEBPS/toc.ncx") && why.contains("OEBPS/nav.xhtml"),
        "{why}"
    );
}
