//! A whole book extracted: for each chapter in order its prompt, one call and the check of the reply, with a
//! cache of replies that lets a run that stopped start again where it stopped.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::check::{Rejected, check, schema};
use super::prompt::{PROMPT_VERSION, SYSTEM, prompt};
use super::roster::Roster;
use crate::book::{Book, ChapterText};
use crate::llm::{Backend, Reply, Request};
use crate::log::{BookLog, BookRecord, Chapter};
use crate::store::{create_beside, hex};
use crate::{Error, Result};

/// Which chapters [`extract_book`] extracts, and where it keeps what it is told.
#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// The first entry of the book's table of contents to extract, counted from 1.
    pub first: u32,
    /// The last entry to extract, itself included.
    pub last: u32,
    /// The directory a book's replies are kept in, under `<book sha256>/replies/`, so that a run started again
    /// calls no model for a chapter already answered; and under `<book sha256>/rejected/<nnn>.json`, what the
    /// check of the chapter at entry `nnn` of the table of contents turned down and what it noted, an empty
    /// record for a chapter without text. A file is named by the entry and not by the chapter's place in the run,
    /// so it holds the record of the latest run over that entry, whatever range that run covered, and a run over
    /// other entries leaves it as it is. `None` reads and writes nothing.
    pub cache: Option<&'a Path>,
    /// When the book was extracted, in RFC 3339 and UTC: what [`BookRecord::extracted_at`] records.
    pub extracted_at: &'a str,
}

/// The log of table-of-contents entries `options.first` to `options.last` of `book`: each a chapter, in order,
/// sent to `backend` with its [`prompt`] and the [`schema`], and the reply's items [`check`]ed against the
/// chapter and the chapters before it. `progress` is given each chapter's row once the chapter is done.
///
/// A chapter without text is sent to no model: its row keeps nothing and costs 0, which is known. A call that
/// fails with [`Error::Model`], which is how a backend reports any failure, is made once more, and a second
/// failure in a row ends the run. With a cache (see [`Options::cache`]) a reply already kept is taken in place of
/// a call, with the seconds and cost it recorded, so a run started again reports what the whole book took and
/// pays only for the chapters no earlier run finished.
///
/// It writes to no database: the caller stores the log, so a run that fails leaves the database as it was.
///
/// # Errors
///
/// [`Error::Invalid`] when the entries are not in the table of contents, before any call. [`Error::Model`]
/// naming the chapter's number and title, with the cause, when its call fails twice. [`Error::Io`] when the
/// cache cannot be read or written.
pub fn extract_book(
    book: &Book,
    backend: &dyn Backend,
    options: &Options<'_>,
    progress: &mut dyn FnMut(&Chapter),
) -> Result<BookLog> {
    let chapters = book.chapters(options.first, options.last)?;
    let cache = options.cache.map(|dir| Cache { book: dir.join(&book.sha256) });
    let schema = schema();
    let schema_text = schema.to_string();
    let mut roster = Roster::new();
    let mut rows = Vec::with_capacity(chapters.len());
    for chapter in &chapters {
        let (seconds, cost_usd, kept, rejected) = if chapter.blocks.is_empty() {
            // Nothing a model said of a chapter with no text could be checked against it, so the call could only
            // cost.
            if let Some(cache) = &cache {
                // An empty record, so that what an earlier run wrote for this entry does not stand for this one.
                cache.put_rejected(chapter.toc_entry, &[], &BTreeMap::new())?;
            }
            (0.0, Some(0.0), 0, 0)
        } else {
            let user = prompt(&roster, chapter);
            let request = Request { system: SYSTEM, user: &user, schema: &schema };
            let reply = answer(backend, &request, &schema_text, cache.as_ref(), chapter)?;
            let outcome = check(&mut roster, chapter, &reply.json);
            if let Some(cache) = &cache {
                cache.put_rejected(chapter.toc_entry, &outcome.rejected, &outcome.notes)?;
            }
            let rejected = u32::try_from(outcome.rejected.len()).unwrap_or(u32::MAX);
            (reply.seconds, reply.cost_usd, outcome.kept, rejected)
        };
        let row = Chapter {
            number: chapter.number,
            toc_entry: chapter.toc_entry,
            title: chapter.title.clone(),
            seconds,
            cost_usd,
            kept,
            rejected,
        };
        progress(&row);
        rows.push(row);
    }
    Ok(BookLog {
        book: BookRecord {
            sha256: book.sha256.clone(),
            title: book.title.clone(),
            language: book.language.clone(),
            backend: backend.name().to_owned(),
            model: backend.model().to_owned(),
            prompt_version: PROMPT_VERSION,
            extracted_at: options.extracted_at.to_owned(),
        },
        chapters: rows,
        documents: book.document_rows(&chapters),
        entities: roster.entities(),
        events: roster.events().to_vec(),
    })
}

/// The reply to `request`, the call for `chapter`: the one the cache keeps for it, or else the backend's, which
/// the cache then keeps.
///
/// A prompt depends only on the book and the chapters before it, so a run started again asks each chapter that
/// an earlier run finished what that run asked, under the same key, and calls the model from the first chapter
/// no run finished.
fn answer(
    backend: &dyn Backend,
    request: &Request<'_>,
    schema_text: &str,
    cache: Option<&Cache>,
    chapter: &ChapterText,
) -> Result<Reply> {
    let Some(cache) = cache else { return call_twice(backend, request, chapter) };
    let key = key(backend, request, schema_text);
    if let Some(reply) = cache.reply(&key)? {
        return Ok(reply);
    }
    let reply = call_twice(backend, request, chapter)?;
    cache.put_reply(&key, &reply)?;
    Ok(reply)
}

/// The backend's reply to `request`, asked a second time when the first call fails with [`Error::Model`]: one
/// timeout or one reply that is not JSON should not end the run of a whole book, and a second in a row is no
/// passing fault.
fn call_twice(backend: &dyn Backend, request: &Request<'_>, chapter: &ChapterText) -> Result<Reply> {
    match backend.call(request) {
        Err(Error::Model(_)) => backend.call(request).map_err(|error| match error {
            Error::Model(cause) => {
                Error::Model(format!("chapter {} ({}), tried twice: {cause}", chapter.number, chapter.title))
            }
            other => other,
        }),
        answered => answered,
    }
}

/// The key a reply is kept under: the hex SHA-256 of everything that shapes it, the backend's endpoint
/// ([`Backend::endpoint`]) among them, each part followed by a zero byte, so that two lists of parts cannot run
/// together into the same bytes.
fn key(backend: &dyn Backend, request: &Request<'_>, schema_text: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [backend.name(), backend.model(), backend.endpoint(), request.system, request.user, schema_text] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

/// Where one book's replies and rejected items are kept: `<cache>/<book sha256>/`.
#[derive(Debug)]
struct Cache {
    book: PathBuf,
}

impl Cache {
    fn reply_path(&self, key: &str) -> PathBuf {
        self.book.join("replies").join(format!("{key}.json"))
    }

    /// The reply kept under `key`, or `None` when there is none.
    fn reply(&self, key: &str) -> Result<Option<Reply>> {
        let bytes = match fs::read(self.reply_path(key)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        // A file that is no reply, cut short or edited by hand, costs one call: the reply to it is written over it.
        Ok(serde_json::from_slice(&bytes).ok().and_then(|kept: Value| kept_reply(&kept)))
    }

    fn put_reply(&self, key: &str, reply: &Reply) -> Result<()> {
        let kept =
            json!({ "raw": reply.raw, "json": reply.json, "seconds": reply.seconds, "cost_usd": reply.cost_usd });
        replace(&self.reply_path(key), &kept.to_string())
    }

    /// Writes what the check of the chapter at entry `toc_entry` turned down, and its notes, over what an
    /// earlier run wrote.
    fn put_rejected(&self, toc_entry: u32, rejected: &[Rejected], notes: &BTreeMap<&str, u32>) -> Result<()> {
        let rejected: Vec<Value> =
            rejected.iter().map(|rejected| json!({ "reason": rejected.reason, "item": rejected.item })).collect();
        let written = json!({ "rejected": rejected, "notes": notes });
        replace(&self.book.join("rejected").join(format!("{toc_entry:03}.json")), &format!("{written:#}"))
    }
}

/// A reply as [`Cache::put_reply`] keeps it, or `None` when `kept` is not one.
fn kept_reply(kept: &Value) -> Option<Reply> {
    let cost_usd = match kept.get("cost_usd")? {
        Value::Null => None,
        cost => Some(cost.as_f64()?),
    };
    Some(Reply {
        json: kept.get("json").filter(|json| json.is_object())?.clone(),
        raw: kept.get("raw")?.as_str()?.to_owned(),
        seconds: kept.get("seconds")?.as_f64()?,
        cost_usd,
    })
}

/// Writes `text` to `path` through a file beside it renamed over it, so that a run killed part way leaves the
/// old file or the whole new one and never a file cut short. `path`'s directory is created when it is missing.
fn replace(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = create_beside(path)?;
    let written = fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        // The write's own error is the one the caller needs; a file that will not go away is a leftover that
        // `create_beside` steps over.
        let _ = fs::remove_file(&temporary);
    }
    Ok(written?)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::llm::{Settings, backend};

    /// The key one request is kept under with the backend `spec` names.
    fn key_of(spec: &str, settings: &Settings) -> String {
        let schema = json!({});
        let request = Request { system: "system", user: "user", schema: &schema };
        key(&*backend(spec, settings).unwrap(), &request, "{}")
    }

    #[test]
    fn the_key_names_where_the_replies_come_from() {
        let settings = Settings::default();
        let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let replay = |dir: &Path| key_of(&format!("replay:{}", dir.display()), &settings);
        assert_ne!(replay(a.path()), replay(b.path()));
        assert_eq!(replay(a.path()), replay(a.path()));
        assert_eq!(replay(a.path()), replay(&a.path().join(".")), "one directory written two ways");

        let openai = |base: &str| key_of(&format!("openai:{base}#m"), &settings);
        assert_ne!(openai("http://127.0.0.1:8000/v1"), openai("http://127.0.0.1:8001/v1"));
        assert_eq!(openai("http://127.0.0.1:8000/v1"), openai("http://127.0.0.1:8000/v1/"));

        let ollama =
            |host: &str| key_of("ollama:m", &Settings { ollama_host: Some(host.into()), ..Settings::default() });
        assert_ne!(ollama("127.0.0.1:8000"), ollama("127.0.0.1:8001"));
        assert_eq!(ollama("127.0.0.1:8000"), ollama("http://127.0.0.1:8000"));
    }
}
