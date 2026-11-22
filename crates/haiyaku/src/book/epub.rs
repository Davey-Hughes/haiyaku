//! Opening an EPUB: its container names the package, the package lists the spine and names the table of
//! contents, and the table of contents cuts the spine into chapters of whole documents.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read};
use std::ops::Range;
use std::path::Path;

use zip::ZipArchive;
use zip::result::ZipError;

use super::xhtml::{Token, tidy, tokens};
use super::{Block, blocks};
use crate::log::{Document, Position};
use crate::store::sha256_file;
use crate::{Error, Result};

/// An EPUB as extraction reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    /// Of the file, as [`sha256_file`] gives it.
    pub sha256: String,
    /// The package's first `dc:title`; the file's stem when it has none.
    pub title: String,
    /// The package's first `dc:language`; `und` when it has none.
    pub language: String,
    /// The spine, in order: a document's index is its ordinal.
    pub documents: Vec<SpineDocument>,
    /// The table of contents: entry `n` is `toc[n - 1]`. The entries' documents rise strictly through the
    /// spine: of the table's entries, the longest run that rises so is kept, and of two such runs as long, the
    /// one that keeps the earlier entries.
    pub toc: Vec<TocEntry>,
}

/// A document of the spine.
#[derive(Debug, Clone, PartialEq)]
pub struct SpineDocument {
    /// Its path in the archive, which a caller names to say where the reader is.
    pub path: String,
    pub blocks: Vec<Block>,
}

/// An entry of the table of contents, where a chapter starts.
#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub label: String,
    /// The ordinal of the spine document the entry points into.
    pub document: u32,
}

/// One chapter's text, as the model reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterText {
    /// From 1, within the range of entries chosen.
    pub number: u32,
    /// From 1, within [`Book::toc`].
    pub toc_entry: u32,
    pub title: String,
    /// The ordinals of its spine documents.
    pub documents: Range<u32>,
    pub blocks: Vec<PlacedBlock>,
}

/// A block's text with the place it stands in the book.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedBlock {
    pub at: Position,
    pub text: String,
}

/// What a manifest item says of a file of the book.
struct Item {
    id: String,
    /// Its path in the archive.
    path: String,
    media_type: String,
    /// The `properties` attribute, a list separated by spaces.
    properties: String,
}

/// What the package says, before the archive is consulted.
#[derive(Default)]
struct Package {
    title: Option<String>,
    language: Option<String>,
    /// In the order written.
    manifest: Vec<Item>,
    /// The `idref` of each `itemref`, in the order written.
    spine: Vec<String>,
    /// The spine's `toc` attribute, which names the NCX's item.
    ncx: Option<String>,
}

/// Where the entries of a table of contents are read from.
#[derive(Clone, Copy, PartialEq)]
enum TocKind {
    Ncx,
    Nav,
}

const NCX_MEDIA_TYPE: &str = "application/x-dtbncx+xml";

/// The media types of documents of text: XHTML, HTML and SVG. A spine item of one that is not UTF-8 refuses the
/// book, since no quote could match text read in another encoding.
const TEXT_TYPES: [&str; 3] = ["application/xhtml+xml", "text/html", "image/svg+xml"];
/// The families of media types, and the media types, that are never text. A spine item of one keeps its place
/// with no blocks and is neither read nor parsed: its bytes hold no text to find.
const BINARY_FAMILIES: [&str; 6] = ["image/", "audio/", "video/", "font/", "application/font-", "application/x-font-"];
const BINARY_TYPES: [&str; 3] = ["application/vnd.ms-opentype", "application/octet-stream", "application/pdf"];

impl Book {
    /// Reads the EPUB at `path`: its spine documents as blocks, and its table of contents. A file that
    /// cannot be opened, or a read of it that the disk fails, is [`Error::Io`]; bytes that are bad, as in an
    /// archive cut short, and a file that is no EPUB this reader can follow are [`Error::Epub`], saying what is
    /// missing or where.
    pub fn open(path: &Path) -> Result<Book> {
        let sha256 = sha256_file(path)?;
        let mut archive = Archive::open(path)?;
        let package_path = package_path(&archive.text(CONTAINER)?)?;
        let package = Package::read(&archive.text(&package_path)?, &package_path);

        let mut documents = Vec::new();
        // The log keeps one row per path, and a caller names a document by its path: a document the spine
        // lists twice is read where it first stands.
        let mut seen = HashSet::new();
        for idref in &package.spine {
            let Some(item) = package.item(idref) else { continue };
            if !archive.contains(&item.path) || !seen.insert(item.path.as_str()) {
                continue;
            }
            let blocks = match content(&item.media_type) {
                Content::Text => blocks(&archive.text(&item.path)?),
                Content::Binary => Vec::new(),
                Content::Other => archive.utf8(&item.path)?.map_or_else(Vec::new, |text| blocks(&text)),
            };
            documents.push(SpineDocument { path: item.path.clone(), blocks });
        }
        if documents.is_empty() {
            return Err(Error::Epub(format!("the spine of {package_path} names no document in the archive")));
        }

        let toc = table_of_contents(&mut archive, &package, &package_path, &documents)?;

        let stem = || path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Book {
            sha256,
            title: package.title.unwrap_or_else(stem),
            language: package.language.unwrap_or_else(|| "und".into()),
            documents,
            toc,
        })
    }

    /// The chapters for table-of-contents entries `first` to `last`, both included and counted from 1. Entry
    /// `n` covers the documents from its own up to the next entry's, and the last runs to the end of the
    /// spine.
    pub fn chapters(&self, first: u32, last: u32) -> Result<Vec<ChapterText>> {
        let count = u32::try_from(self.toc.len()).unwrap_or(u32::MAX);
        if first < 1 || first > last || last > count {
            return Err(Error::Invalid(format!(
                "chapters {first}-{last}: the table of contents has entries 1-{count}"
            )));
        }
        let spine_end = u32::try_from(self.documents.len()).unwrap_or(u32::MAX);
        let ends = self.toc.iter().skip(1).map(|next| next.document).chain([spine_end]);
        Ok((1..)
            .zip(self.toc.iter().zip(ends))
            .filter(|(entry, _)| (first..=last).contains(entry))
            .map(|(entry, (start, end))| {
                let documents = start.document..end;
                ChapterText {
                    number: entry - first + 1,
                    toc_entry: entry,
                    title: start.label.clone(),
                    blocks: self.placed_blocks(&documents),
                    documents,
                }
            })
            .collect())
    }

    /// One row per spine document, for [`BookLog::documents`](crate::log::BookLog::documents): each with the
    /// number of the chapter among `chapters` that holds it, if one does.
    pub fn document_rows(&self, chapters: &[ChapterText]) -> Vec<Document> {
        (0..)
            .zip(&self.documents)
            .map(|(ordinal, document)| Document {
                ordinal,
                path: document.path.clone(),
                chapter: chapters.iter().find(|chapter| chapter.documents.contains(&ordinal)).map(|c| c.number),
            })
            .collect()
    }

    /// The blocks of the spine documents whose ordinals are in `documents`, each with its place.
    fn placed_blocks(&self, documents: &Range<u32>) -> Vec<PlacedBlock> {
        (0..)
            .zip(&self.documents)
            .filter(|(ordinal, _)| documents.contains(ordinal))
            .flat_map(|(document, spine)| {
                (0..)
                    .zip(&spine.blocks)
                    .map(move |(block, read)| PlacedBlock { at: Position { document, block }, text: read.text.clone() })
            })
            .collect()
    }
}

impl ChapterText {
    /// What the model reads: the blocks' texts joined by "\n".
    pub fn text(&self) -> String {
        self.blocks.iter().map(|block| block.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

/// Where every EPUB names its package.
const CONTAINER: &str = "META-INF/container.xml";

/// The zip archive an EPUB is.
struct Archive {
    zip: ZipArchive<File>,
}

impl Archive {
    fn open(path: &Path) -> Result<Self> {
        let zip = ZipArchive::new(File::open(path)?).map_err(open_error)?;
        Ok(Self { zip })
    }

    fn contains(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    /// The text of the file at `name`, as [`Archive::utf8`] reads it, which has to be UTF-8.
    fn text(&mut self, name: &str) -> Result<String> {
        self.utf8(name)?.ok_or_else(|| Error::Epub(format!("{name} is not UTF-8")))
    }

    /// The text of the file at `name` without a leading byte-order mark, which says how the file is encoded and
    /// is no text of it; `None` when the file is not UTF-8.
    fn utf8(&mut self, name: &str) -> Result<Option<String>> {
        let mut file = self.zip.by_name(name).map_err(|error| lookup_error(name, error))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|error| read_error(name, error))?;
        if bytes.starts_with(b"\xEF\xBB\xBF") {
            bytes.drain(..3);
        }
        Ok(String::from_utf8(bytes).ok())
    }
}

/// What a failure of zip to open the archive is: a failed read as [`read_error`] tells it, and anything else that
/// the file is no zip archive, [`Error::Epub`], which a retry cannot fix.
fn open_error(error: ZipError) -> Error {
    match error {
        ZipError::Io(error) => read_error("the zip archive", error),
        error => Error::Epub(format!("not a zip archive: {error}")),
    }
}

/// What a failure of zip to find the entry `name` is: a failed read as [`read_error`] tells it, and anything else
/// a fault of the book, [`Error::Epub`].
fn lookup_error(name: &str, error: ZipError) -> Error {
    match error {
        ZipError::FileNotFound => Error::Epub(format!("no {name} in the archive")),
        ZipError::Io(error) => read_error(name, error),
        error => Error::Epub(format!("{name}: {error}")),
    }
}

/// What a failed read of the archive is, while opening it, finding an entry or reading one; `what` names what was
/// read. Bytes that are bad are the book's fault and [`Error::Epub`]: a bad checksum is `InvalidData`, the deflate
/// decoder reports a corrupt stream as `InvalidInput`, and a read that runs off the end, as in an archive cut
/// short, is `UnexpectedEof`. Any other failure is the disk's, [`Error::Io`], which no fallback to another file
/// hides. One split for every read keeps a truncated archive one error wherever the cut falls.
fn read_error(what: &str, error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof => {
            Error::Epub(format!("{what}: {error}"))
        }
        _ => Error::Io(error),
    }
}

/// How a spine item is read, by its media type.
#[derive(Debug, PartialEq)]
enum Content {
    /// One of [`TEXT_TYPES`]: read, and the book is refused when it is not UTF-8.
    Text,
    /// One of [`BINARY_FAMILIES`] or [`BINARY_TYPES`]: not read.
    Binary,
    /// Any other type, or none: read as markup when it is UTF-8, and given no blocks when it is not. A chapter
    /// mislabelled keeps its text, and a file of bytes the lists do not name costs no book.
    Other,
}

/// How a spine item of `media_type` is read, the type compared lower-cased and without parameters.
fn content(media_type: &str) -> Content {
    let essence = media_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    if TEXT_TYPES.contains(&essence.as_str()) {
        Content::Text
    } else if BINARY_FAMILIES.iter().any(|family| essence.starts_with(family))
        || BINARY_TYPES.contains(&essence.as_str())
    {
        Content::Binary
    } else {
        Content::Other
    }
}

/// The archive path of the package that `container.xml` names: the path its first `rootfile` whose
/// `full-path` resolves to one gives, which the container format makes the default rendition. Whether the
/// archive holds that file is found when it is read.
fn package_path(container: &str) -> Result<String> {
    tokens(container)
        .find_map(|token| match token {
            Token::Open(tag) if tag.name == "rootfile" => tag.attribute("full-path").and_then(|path| resolve("", path)),
            _ => None,
        })
        .ok_or_else(|| Error::Epub(format!("{CONTAINER} names no package")))
}

impl Package {
    /// The package `opf`, read from the archive path `opf_path`.
    fn read(opf: &str, opf_path: &str) -> Package {
        let mut package = Package::default();
        // The Dublin Core element being read, `title` or `language`, and its text so far.
        let mut reading: Option<(String, String)> = None;
        for token in tokens(opf) {
            match token {
                Token::Open(tag) => match tag.name.as_str() {
                    "title" | "language" if !tag.self_closing => reading = Some((tag.name, String::new())),
                    "item" => {
                        // An item whose href names no path in the archive names no file of the book.
                        if let (Some(id), Some(path)) =
                            (tag.attribute("id"), tag.attribute("href").and_then(|href| resolve(opf_path, href)))
                        {
                            package.manifest.push(Item {
                                id: id.to_owned(),
                                path,
                                media_type: tag.attribute("media-type").unwrap_or_default().to_owned(),
                                properties: tag.attribute("properties").unwrap_or_default().to_owned(),
                            });
                        }
                    }
                    "itemref" => package.spine.extend(tag.attribute("idref").map(str::to_owned)),
                    "spine" => package.ncx = tag.attribute("toc").map(str::to_owned),
                    _ => {}
                },
                Token::Close(name) => {
                    if let Some((element, text)) = reading.take_if(|(element, _)| *element == name) {
                        let text = tidy(&text);
                        let field = if element == "title" { &mut package.title } else { &mut package.language };
                        if field.is_none() && !text.is_empty() {
                            *field = Some(text);
                        }
                    }
                }
                Token::Text(text) => {
                    if let Some((_, read)) = &mut reading {
                        read.push_str(&text);
                    }
                }
            }
        }
        package
    }

    fn item(&self, id: &str) -> Option<&Item> {
        self.manifest.iter().find(|item| item.id == id)
    }

    /// The tables of contents to try, in order: the NCX the spine names, the first NCX in the manifest, and the
    /// navigation document, each once.
    fn tables_of_contents(&self) -> Vec<(&Item, TocKind)> {
        let named = self.ncx.as_deref().and_then(|id| self.item(id));
        let typed = self.manifest.iter().find(|item| item.media_type == NCX_MEDIA_TYPE);
        let nav = self.manifest.iter().find(|item| item.properties.split_whitespace().any(|p| p == "nav"));
        let mut tables: Vec<(&Item, TocKind)> = Vec::new();
        for (item, kind) in [(named, TocKind::Ncx), (typed, TocKind::Ncx), (nav, TocKind::Nav)] {
            if let Some(item) = item
                && !tables.iter().any(|(tried, tried_kind)| tried.path == item.path && *tried_kind == kind)
            {
                tables.push((item, kind));
            }
        }
        tables
    }
}

/// The entries of the first of the package's tables of contents that keeps one. The order of the tables is a
/// preference: a table missing from the archive, or one that names no document of the spine, leaves the next,
/// and the book is refused only when none is left.
fn table_of_contents(
    archive: &mut Archive,
    package: &Package,
    package_path: &str,
    documents: &[SpineDocument],
) -> Result<Vec<TocEntry>> {
    let tables = package.tables_of_contents();
    if tables.is_empty() {
        return Err(Error::Epub(format!(
            "no table of contents: {package_path} names neither an NCX nor a navigation document"
        )));
    }
    let mut failed = Vec::new();
    for (item, kind) in tables {
        let markup = match archive.text(&item.path) {
            Ok(markup) => markup,
            Err(Error::Epub(why)) => {
                failed.push(why);
                continue;
            }
            Err(error) => return Err(error),
        };
        let entries = match kind {
            TocKind::Ncx => ncx_entries(&markup),
            TocKind::Nav => nav_entries(&markup),
        };
        let toc = kept_entries(entries, &item.path, documents);
        if !toc.is_empty() {
            return Ok(toc);
        }
        failed.push(format!("{} names none", item.path));
    }
    Err(Error::Epub(format!("no table of contents names a document of the spine ({})", failed.join("; "))))
}

/// Each entry of an NCX's navMap, in the order written, nested ones included: its label and its target as
/// written. Page and navigation lists are no part of it.
fn ncx_entries(ncx: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let (mut in_map, mut in_text) = (false, false);
    let mut label = String::new();
    // A navPoint may have a label in each of several languages, which joined would be no one's label: the
    // first is the one read.
    let mut labelled = false;
    for token in tokens(ncx) {
        match token {
            Token::Open(tag) => match tag.name.as_str() {
                "navmap" => in_map = !tag.self_closing,
                // A navPoint's label comes before its content, and before any navPoint nested in it.
                "navpoint" => {
                    label.clear();
                    labelled = false;
                }
                "text" => in_text = !tag.self_closing && !labelled,
                "content" if in_map => {
                    if let Some(src) = tag.attribute("src") {
                        entries.push((tidy(&label), src.to_owned()));
                    }
                }
                _ => {}
            },
            Token::Close(name) => match name.as_str() {
                "navmap" => in_map = false,
                "text" => in_text = false,
                "navlabel" => labelled = !label.trim().is_empty(),
                _ => {}
            },
            Token::Text(text) if in_text => label.push_str(&text),
            Token::Text(_) => {}
        }
    }
    entries
}

/// Each link of a navigation document's `toc` nav, in the order written, nested ones included: its label and
/// its target as written. Landmarks and page lists are no part of it.
fn nav_entries(nav: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut in_toc = false;
    // The target of the link being read, and its label so far.
    let mut link: Option<(String, String)> = None;
    let mut end_link = |link: &mut Option<(String, String)>| {
        if let Some((target, label)) = link.take() {
            entries.push((tidy(&label), target));
        }
    };
    for token in tokens(nav) {
        match token {
            Token::Open(tag) if tag.name == "nav" => {
                in_toc = !tag.self_closing
                    && tag.attribute("type").is_some_and(|types| types.split_whitespace().any(|t| t == "toc"));
            }
            Token::Open(tag) if tag.name == "a" && in_toc => {
                // A link left unclosed ends where the next one starts.
                end_link(&mut link);
                if !tag.self_closing {
                    link = tag.attribute("href").map(|href| (href.to_owned(), String::new()));
                }
            }
            Token::Close(name) if name == "a" || name == "nav" => {
                end_link(&mut link);
                if name == "nav" {
                    in_toc = false;
                }
            }
            Token::Text(text) => {
                if let Some((_, label)) = &mut link {
                    label.push_str(&text);
                }
            }
            Token::Open(_) | Token::Close(_) => {}
        }
    }
    end_link(&mut link);
    entries
}

/// The entries of a table of contents at the archive path `toc_path` that start chapters: the longest run of
/// entries whose targets are documents of the spine rising strictly, and of two runs as long, the one that
/// keeps the earlier entries. An entry left out points into a document a kept entry already starts, as a
/// section of a chapter does, or out of order, as a contents page at the end of the spine listed first does;
/// keeping such an entry would cost every later chapter it stands in front of.
fn kept_entries(entries: Vec<(String, String)>, toc_path: &str, documents: &[SpineDocument]) -> Vec<TocEntry> {
    let in_spine: Vec<TocEntry> = entries
        .into_iter()
        .filter_map(|(label, target)| {
            let path = target_path(toc_path, &target)?;
            let (document, _) = (0..).zip(documents).find(|(_, spine)| spine.path == path)?;
            Some(TocEntry { label, document })
        })
        .collect();
    // The length of the longest rising run that starts at each entry, read from the last entry back.
    // `starts[k]` is the latest document a run of `k + 1` entries found so far can start at, which falls as
    // `k` grows.
    let mut longest = vec![0; in_spine.len()];
    let mut starts: Vec<u32> = Vec::new();
    for (at, entry) in in_spine.iter().enumerate().rev() {
        let after = starts.partition_point(|start| *start > entry.document);
        if after == starts.len() {
            starts.push(entry.document);
        } else {
            starts[after] = entry.document;
        }
        longest[at] = after + 1;
    }
    // Taking, at each step, the first entry that can still begin a run of the length left keeps the earliest
    // entries.
    let mut left = starts.len();
    let mut kept: Vec<TocEntry> = Vec::with_capacity(left);
    for (entry, length) in in_spine.into_iter().zip(longest) {
        if left > 0 && length == left && kept.last().is_none_or(|last| entry.document > last.document) {
            kept.push(entry);
            left -= 1;
        }
    }
    kept
}

/// The archive path a table-of-contents target written in the file at the archive path `toc_path` names. A
/// target that is a fragment alone points into that file itself, as a navigation document in the spine links to
/// its own sections.
fn target_path(toc_path: &str, target: &str) -> Option<String> {
    if target.split('#').next().is_some_and(str::is_empty) {
        Some(toc_path.to_owned())
    } else {
        resolve(toc_path, target)
    }
}

/// The archive path that `href`, written in the file at the archive path `base`, names: resolved against that
/// file's directory, percent-decoded, with `.` and `..` folded and the fragment dropped. `None` when that
/// leaves no path, as an empty href or a fragment alone does, when the href climbs above the archive's root, or
/// when it is no percent-encoded UTF-8: a path clamped at the root or read with a bad escape as written would
/// name some other file.
fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.split_once('#').map_or(href, |(path, _)| path);
    if href.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = base.rsplit_once('/').map_or_else(Vec::new, |(dir, _)| dir.split('/').collect());
    let href = percent_decoded(href)?;
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/")).filter(|path| !path.is_empty())
}

/// `text` with each `%` and two hex digits made the byte they name; `None` for a `%` without two hex digits
/// after it, or for bytes that are not UTF-8.
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let digit = |at: usize| bytes.get(at).and_then(|byte| char::from(*byte).to_digit(16));
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let (high, low) = (digit(at + 1)?, digit(at + 2)?);
            decoded.extend(u8::try_from(high * 16 + low));
            at += 3;
        } else {
            decoded.push(byte);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn resolve_folds_dots_and_decodes() {
        assert_eq!(resolve("OEBPS/content.opf", "text/a%20b.xhtml").as_deref(), Some("OEBPS/text/a b.xhtml"));
        assert_eq!(resolve("OEBPS/nav/toc.ncx", "../text/./c1.xhtml#s2").as_deref(), Some("OEBPS/text/c1.xhtml"));
        assert_eq!(resolve("content.opf", "c1.xhtml").as_deref(), Some("c1.xhtml"));
        assert_eq!(resolve("OEBPS/content.opf", "../c1.xhtml").as_deref(), Some("c1.xhtml"));
        assert_eq!(resolve("", "./OEBPS/content.opf").as_deref(), Some("OEBPS/content.opf"));
    }

    #[test]
    fn a_toc_target_that_is_a_fragment_alone_names_the_file_it_is_written_in() {
        assert_eq!(target_path("OEBPS/nav.xhtml", "#c2").as_deref(), Some("OEBPS/nav.xhtml"));
        assert_eq!(target_path("OEBPS/nav.xhtml", "").as_deref(), Some("OEBPS/nav.xhtml"));
        assert_eq!(target_path("OEBPS/nav.xhtml", "c2.xhtml#s1").as_deref(), Some("OEBPS/c2.xhtml"));
    }

    #[test]
    fn an_href_that_names_no_path_resolves_to_nothing() {
        for href in ["", "#top"] {
            assert_eq!(resolve("OEBPS/content.opf", href), None, "{href:?}");
        }
        for href in ["./", ".", "a/.."] {
            assert_eq!(resolve("", href), None, "{href:?}");
        }
    }

    #[test]
    fn a_spine_item_is_read_by_its_media_type() {
        for text in ["application/xhtml+xml", "Application/XHTML+XML; charset=utf-8", "text/html", "IMAGE/SVG+XML"] {
            assert_eq!(content(text), Content::Text, "{text}");
        }
        for binary in ["image/jpeg", "IMAGE/PNG", "audio/mpeg", "video/mp4", "font/woff2", "application/font-woff"] {
            assert_eq!(content(binary), Content::Binary, "{binary}");
        }
        for binary in ["application/x-font-ttf", "application/vnd.ms-opentype", "application/octet-stream"] {
            assert_eq!(content(binary), Content::Binary, "{binary}");
        }
        assert_eq!(content("application/pdf; version=1.7"), Content::Binary);
        for other in ["", "application/xml", "application/vnd.ms-fontobject", "application/zip"] {
            assert_eq!(content(other), Content::Other, "{other:?}");
        }
    }

    #[test]
    fn an_href_that_climbs_out_of_the_archive_or_has_a_bad_escape_names_nothing() {
        assert_eq!(resolve("OEBPS/content.opf", "../../c1.xhtml"), None);
        assert_eq!(resolve("content.opf", "../c1.xhtml"), None);
        assert_eq!(resolve("OEBPS/content.opf", "100%.xhtml"), None);
        assert_eq!(resolve("OEBPS/content.opf", "caf%C3.xhtml"), None);
    }

    #[test]
    fn a_percent_without_two_hex_digits_or_bytes_that_are_not_utf8_decode_to_nothing() {
        for text in ["100%", "%zz%4", "%+1", "%C3", "%FF"] {
            assert_eq!(percent_decoded(text), None, "{text}");
        }
        assert_eq!(percent_decoded("caf%C3%A9").as_deref(), Some("café"));
        assert_eq!(percent_decoded("plain").as_deref(), Some("plain"));
    }

    /// The kinds of read error that say the bytes are bad, and two that say the disk failed.
    const BAD_BYTES: [io::ErrorKind; 3] =
        [io::ErrorKind::InvalidData, io::ErrorKind::InvalidInput, io::ErrorKind::UnexpectedEof];
    const DISK: [io::ErrorKind; 2] = [io::ErrorKind::PermissionDenied, io::ErrorKind::Other];

    #[test]
    fn corrupt_entry_data_is_an_epub_error_and_any_other_read_error_an_io_error() {
        for bad in BAD_BYTES {
            assert!(matches!(read_error("a", io::Error::from(bad)), Error::Epub(_)), "{bad:?}");
        }
        for disk in DISK {
            assert!(matches!(read_error("a", io::Error::from(disk)), Error::Io(_)), "{disk:?}");
        }
    }

    #[test]
    fn bad_bytes_met_while_opening_are_an_epub_error() {
        for bad in BAD_BYTES {
            let error = open_error(ZipError::Io(io::Error::from(bad)));
            assert!(matches!(error, Error::Epub(_)), "{bad:?}: {error:?}");
        }
        for disk in DISK {
            let error = open_error(ZipError::Io(io::Error::from(disk)));
            assert!(matches!(error, Error::Io(_)), "{disk:?}: {error:?}");
        }
    }

    #[test]
    fn finding_an_entry_fails_as_reading_one_does() {
        let missing = lookup_error("a.xhtml", ZipError::FileNotFound);
        assert!(matches!(&missing, Error::Epub(why) if why == "no a.xhtml in the archive"), "{missing:?}");
        let invalid = lookup_error("a.xhtml", ZipError::InvalidArchive("bad local header".into()));
        assert!(matches!(invalid, Error::Epub(_)), "{invalid:?}");
        for bad in BAD_BYTES {
            let error = lookup_error("a.xhtml", ZipError::Io(io::Error::from(bad)));
            assert!(matches!(error, Error::Epub(_)), "{bad:?}: {error:?}");
        }
        for disk in DISK {
            let error = lookup_error("a.xhtml", ZipError::Io(io::Error::from(disk)));
            assert!(matches!(error, Error::Io(_)), "{disk:?}: {error:?}");
        }
    }

    #[test]
    fn a_corrupt_entry_is_an_epub_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("corrupt.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("a.xhtml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all("The keeper climbed to the lamp. ".repeat(64).as_bytes()).unwrap();
        zip.finish().unwrap();
        let start = {
            let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
            usize::try_from(zip.by_name("a.xhtml").unwrap().data_start().unwrap()).unwrap()
        };
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[start..start + 8].fill(0xFF);
        std::fs::write(&path, bytes).unwrap();
        let read = Archive::open(&path).unwrap().text("a.xhtml");
        assert!(matches!(read, Err(Error::Epub(_))), "{read:?}");
    }

    #[test]
    fn a_failed_read_while_opening_is_an_io_error() {
        let error = open_error(ZipError::Io(io::Error::other("disk gone")));
        assert!(matches!(error, Error::Io(_)), "{error:?}");
    }

    #[test]
    fn an_invalid_archive_is_an_epub_error() {
        let error = open_error(ZipError::InvalidArchive("no central directory".into()));
        assert!(matches!(error, Error::Epub(_)), "{error:?}");
    }
}
