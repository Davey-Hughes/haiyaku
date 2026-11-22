//! EPUB to units: one unit per table-of-contents entry, holding every spine
//! file from that entry up to the next one.

use std::{collections::HashMap, fs::File, io::Read, path::Path};

use anyhow::{Context, Result, bail};

pub struct Unit {
    pub title: String,
    pub text: String,
}

pub fn read_epub(path: &Path) -> Result<Vec<Unit>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file)?;
    let container = entry(&mut zip, "META-INF/container.xml")?;
    let opf_path = tokenize(&container)
        .into_iter()
        .find_map(|t| match t {
            Tok::Open { name, attrs, .. } if name == "rootfile" => attr(&attrs, "full-path").map(str::to_string),
            _ => None,
        })
        .context("container.xml names no rootfile")?;
    let opf = entry(&mut zip, &opf_path)?;

    // id -> (path, media type, properties)
    let mut manifest: HashMap<String, (String, String, String)> = HashMap::new();
    let mut spine_ids = Vec::new();
    let mut toc_id = None;
    for t in tokenize(&opf) {
        let Tok::Open { name, attrs, .. } = t else { continue };
        match name.as_str() {
            "item" => {
                if let (Some(id), Some(href)) = (attr(&attrs, "id"), attr(&attrs, "href")) {
                    let media = attr(&attrs, "media-type").unwrap_or("").to_string();
                    let props = attr(&attrs, "properties").unwrap_or("").to_string();
                    manifest.insert(id.to_string(), (resolve(&opf_path, href), media, props));
                }
            }
            "itemref" => {
                if let Some(id) = attr(&attrs, "idref") {
                    spine_ids.push(id.to_string());
                }
            }
            "spine" => toc_id = attr(&attrs, "toc").map(str::to_string),
            _ => {}
        }
    }
    let spine: Vec<String> = spine_ids.iter().filter_map(|id| manifest.get(id).map(|m| m.0.clone())).collect();
    let ncx = toc_id
        .as_ref()
        .and_then(|id| manifest.get(id))
        .or_else(|| manifest.values().find(|m| m.1 == "application/x-dtbncx+xml"))
        .map(|m| m.0.clone());
    let nav = manifest.values().find(|m| m.2.split_whitespace().any(|p| p == "nav")).map(|m| m.0.clone());
    let toc = if let Some(p) = ncx {
        ncx_entries(&entry(&mut zip, &p)?, &p)
    } else if let Some(p) = nav {
        nav_entries(&entry(&mut zip, &p)?, &p)
    } else {
        bail!("{}: no table of contents", path.display())
    };

    // A TOC entry pointing back into a file an earlier entry already starts
    // in is a section of that unit, not a unit of its own.
    let mut starts: Vec<(usize, String)> = Vec::new();
    for (label, target) in toc {
        let Some(i) = spine.iter().position(|s| *s == target) else { continue };
        if starts.last().is_some_and(|(j, _)| i <= *j) {
            continue;
        }
        starts.push((i, label));
    }
    let mut units = Vec::new();
    for (k, (start, title)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(spine.len(), |(e, _)| *e);
        let mut text = String::new();
        for p in &spine[*start..end] {
            let t = html_to_text(&entry(&mut zip, p)?);
            if !t.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&t);
            }
        }
        units.push(Unit { title: title.clone(), text });
    }
    Ok(units)
}

fn entry(zip: &mut zip::ZipArchive<File>, name: &str) -> Result<String> {
    let mut f = zip.by_name(name).with_context(|| format!("no {name} in the EPUB"))?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

fn ncx_entries(xml: &str, base: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut label = String::new();
    let mut in_text = false;
    for t in tokenize(xml) {
        match t {
            Tok::Open { name, .. } if name == "navpoint" => label.clear(),
            Tok::Open { name, closed: false, .. } if name == "text" => in_text = true,
            Tok::Close(name) if name == "text" => in_text = false,
            Tok::Text(s) if in_text => label.push_str(&s),
            Tok::Open { name, attrs, .. } if name == "content" => {
                if let Some(src) = attr(&attrs, "src") {
                    out.push((collapse(&label), resolve(base, src)));
                }
            }
            _ => {}
        }
    }
    out
}

fn nav_entries(xml: &str, base: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut toc_depth = 0usize;
    let mut href: Option<String> = None;
    let mut label = String::new();
    for t in tokenize(xml) {
        match t {
            Tok::Open { name, attrs, closed: false } if name == "nav" => {
                if toc_depth > 0 || attr(&attrs, "type") == Some("toc") {
                    toc_depth += 1;
                }
            }
            Tok::Close(name) if name == "nav" => toc_depth = toc_depth.saturating_sub(1),
            Tok::Open { name, attrs, .. } if name == "a" && toc_depth > 0 => {
                href = attr(&attrs, "href").map(str::to_string);
                label.clear();
            }
            Tok::Text(s) if href.is_some() => label.push_str(&s),
            Tok::Close(name) if name == "a" => {
                if let Some(h) = href.take() {
                    out.push((collapse(&label), resolve(base, &h)));
                }
            }
            _ => {}
        }
    }
    out
}

const BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "tr",
    "blockquote",
    "section",
    "article",
    "hr",
    "dd",
    "dt",
    "table",
];

pub fn html_to_text(html: &str) -> String {
    let mut paras = Vec::new();
    let mut cur = String::new();
    let mut in_body = !html.contains("<body");
    let mut skip = 0usize;
    for t in tokenize(html) {
        match t {
            Tok::Open { name, closed, .. } => {
                if name == "body" {
                    in_body = true;
                } else if matches!(name.as_str(), "script" | "style" | "head" | "title") && !closed {
                    skip += 1;
                }
                if BLOCK.contains(&name.as_str()) {
                    flush(&mut cur, &mut paras);
                }
            }
            Tok::Close(name) => {
                if name == "body" {
                    in_body = false;
                } else if matches!(name.as_str(), "script" | "style" | "head" | "title") {
                    skip = skip.saturating_sub(1);
                }
                if BLOCK.contains(&name.as_str()) {
                    flush(&mut cur, &mut paras);
                }
            }
            Tok::Text(s) => {
                if in_body && skip == 0 {
                    cur.push_str(&s);
                }
            }
        }
    }
    flush(&mut cur, &mut paras);
    paras.join("\n")
}

fn flush(cur: &mut String, paras: &mut Vec<String>) {
    let p = collapse(cur);
    if !p.is_empty() {
        paras.push(p);
    }
    cur.clear();
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

enum Tok {
    Open { name: String, attrs: Vec<(String, String)>, closed: bool },
    Close(String),
    Text(String),
}

/// Element and attribute names without their namespace prefix, lowercased.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| local(k) == name).map(|(_, v)| v.as_str())
}

/// A forgiving tag scanner: EPUB content is meant to be XHTML, but enough of it
/// is not well-formed that a strict parser would drop whole chapters.
fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map_or("", |i| &r[i + 3..]);
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let end = r.find("]]>").unwrap_or(r.len());
            out.push(Tok::Text(r[..end].to_string()));
            rest = r.get(end + 3..).unwrap_or("");
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            rest = rest.find('>').map_or("", |i| &rest[i + 1..]);
        } else if let Some(r) = rest.strip_prefix("</") {
            let end = r.find('>').unwrap_or(r.len());
            out.push(Tok::Close(local(r[..end].trim())));
            rest = r.get(end + 1..).unwrap_or("");
        } else if rest.starts_with('<') {
            let bytes = rest.as_bytes();
            let mut i = 1;
            let mut quote: Option<u8> = None;
            while i < bytes.len() {
                let c = bytes[i];
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => {}
                    None if c == b'"' || c == b'\'' => quote = Some(c),
                    None if c == b'>' => break,
                    None => {}
                }
                i += 1;
            }
            let inner = rest[1..i.min(rest.len())].trim_end();
            rest = rest.get(i + 1..).unwrap_or("");
            let closed = inner.ends_with('/');
            let (name, attrs) = parse_tag(inner.trim_end_matches('/'));
            out.push(Tok::Open { name, attrs, closed });
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push(Tok::Text(decode_entities(&rest[..end])));
            rest = &rest[end..];
        }
    }
    out
}

fn parse_tag(inner: &str) -> (String, Vec<(String, String)>) {
    let inner = inner.trim();
    let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let name = local(&inner[..name_end]);
    let mut attrs = Vec::new();
    let mut rest = inner[name_end..].trim_start();
    while !rest.is_empty() {
        let key_end = rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len());
        let key = rest[..key_end].to_string();
        rest = rest[key_end..].trim_start();
        let mut value = String::new();
        if let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start();
            if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let body = &r[1..];
                let end = body.find(q).unwrap_or(body.len());
                value = decode_entities(&body[..end]);
                rest = body.get(end + 1..).unwrap_or("").trim_start();
            } else {
                let end = r.find(char::is_whitespace).unwrap_or(r.len());
                value = decode_entities(&r[..end]);
                rest = r[end..].trim_start();
            }
        }
        if key.is_empty() {
            break;
        }
        attrs.push((key, value));
    }
    (name, attrs)
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let decoded = tail.find(';').filter(|&j| j <= 10).and_then(|j| entity(&tail[1..j]).map(|c| (c, j)));
        match decoded {
            Some((c, j)) => {
                out.push(c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(n);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        _ => return None,
    })
}

/// Joins an href to the directory of the file it appears in. The fragment is
/// dropped: a unit is a whole file.
fn resolve(base_file: &str, href: &str) -> String {
    let href = percent_decode(href.split('#').next().unwrap_or(""));
    let dir = base_file.rsplit_once('/').map_or("", |(d, _)| d);
    let mut parts: Vec<&str> = if dir.is_empty() { vec![] } else { dir.split('/').collect() };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
