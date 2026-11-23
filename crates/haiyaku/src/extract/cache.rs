//! The reply cache every model call goes through: a reply is kept under the hash of everything that shapes it,
//! so that a run started again asks no model what an earlier run was told.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::llm::{Backend, Reply, Request};
use crate::store::{create_beside, hex};
use crate::{Error, Result};

/// The reply to `request`: the one `cache` keeps for it, or else the backend's, which `cache` then keeps. `what`
/// names the call in the error of a second failure, and `fits` says whether a reply fits the call's schema.
///
/// A reply that does not fit is no answer: from the backend it is a failed call, and from the cache it is passed
/// over and asked again. It is never kept, so a run started again asks once more rather than failing on a kept
/// reply for ever.
///
/// A caller whose prompts depend only on what it was given and on the replies before them asks, when run again,
/// each call an earlier run finished what that run asked, under the same key, and calls the model from the first
/// call no run finished.
pub(crate) fn answer(
    backend: &dyn Backend,
    request: &Request<'_>,
    schema_text: &str,
    cache: Option<&Cache>,
    what: &str,
    fits: fn(&Value) -> bool,
) -> Result<Reply> {
    let Some(cache) = cache else { return call_twice(backend, request, what, fits) };
    let key = key(backend, request, schema_text);
    if let Some(reply) = cache.reply(&key)?.filter(|reply| fits(&reply.json)) {
        return Ok(reply);
    }
    let reply = call_twice(backend, request, what, fits)?;
    cache.put_reply(&key, &reply)?;
    Ok(reply)
}

/// The backend's reply to `request`, asked a second time when the first call fails with [`Error::Model`]: one
/// timeout, or one reply that is not JSON or does not fit, should not end a run of many calls, and a second in a
/// row is no passing fault. The second failure names the call with `what`, since each call has its own prompt
/// to look into.
fn call_twice(backend: &dyn Backend, request: &Request<'_>, what: &str, fits: fn(&Value) -> bool) -> Result<Reply> {
    match ask(backend, request, fits) {
        Err(Error::Model(_)) => ask(backend, request, fits).map_err(|error| match error {
            Error::Model(cause) => Error::Model(format!("{what}, tried twice: {cause}")),
            other => other,
        }),
        answered => answered,
    }
}

/// The backend's reply to `request`, or [`Error::Model`] when it does not fit: a backend sees to it that a reply
/// is a JSON object, and no more.
fn ask(backend: &dyn Backend, request: &Request<'_>, fits: fn(&Value) -> bool) -> Result<Reply> {
    let reply = backend.call(request)?;
    if fits(&reply.json) { Ok(reply) } else { Err(Error::Model("the reply does not fit the schema".into())) }
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

/// Where one book's replies are kept: `<cache dir>/<book sha256>/replies/<key>.json`.
#[derive(Debug)]
pub(crate) struct Cache {
    book: PathBuf,
}

impl Cache {
    /// The cache of the book whose SHA-256 is `sha256`, in the cache directory `dir`.
    pub(crate) fn new(dir: &Path, sha256: &str) -> Cache {
        Cache { book: dir.join(sha256) }
    }

    /// The book's directory, `<cache dir>/<book sha256>/`, where extraction keeps files of its own beside the
    /// replies.
    pub(super) fn dir(&self) -> &Path {
        &self.book
    }

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
pub(super) fn replace(path: &Path, text: &str) -> Result<()> {
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
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::llm::{Replay, Settings, backend};

    /// The key one request is kept under with the backend `spec` names.
    fn key_of(spec: &str, settings: &Settings) -> String {
        let schema = json!({});
        let request = Request { system: "system", user: "user", schema: &schema };
        key(&*backend(spec, settings).unwrap(), &request, "{}")
    }

    /// Whether a reply names a candidate: a check stricter than extraction's, which takes any object.
    fn names_one(json: &Value) -> bool {
        json.get("candidate").is_some_and(Value::is_string)
    }

    /// `answer` to one fixed request, from `replay` through `cache`.
    fn answer_with(replay: &Replay, cache: &Cache, fits: fn(&Value) -> bool) -> Result<Reply> {
        let schema = json!({});
        let request = Request { system: "system", user: "user", schema: &schema };
        answer(replay, &request, "{}", Some(cache), "the call", fits)
    }

    /// The replies `cache` keeps, read back from their files; none when it has written none.
    fn kept(cache: &Cache) -> Vec<Option<Reply>> {
        let Ok(files) = fs::read_dir(cache.dir().join("replies")) else { return Vec::new() };
        files
            .map(|file| kept_reply(&serde_json::from_slice(&fs::read(file.unwrap().path()).unwrap()).unwrap()))
            .collect()
    }

    #[test]
    fn a_reply_that_does_not_fit_is_asked_again_and_not_kept() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path(), "book");
        let replay = Replay::new(vec![r#"{"candidate":1}"#.into(), r#"{"candidate":"e1"}"#.into()]);
        let reply = answer_with(&replay, &cache, names_one).unwrap();
        assert_eq!(reply.json, json!({ "candidate": "e1" }));
        assert_eq!(kept(&cache), [Some(reply)]);
    }

    #[test]
    fn a_reply_that_never_fits_fails_naming_the_call_and_is_not_kept() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path(), "book");
        let replay = Replay::new(vec![r#"{"candidate":1}"#.into(), "{}".into()]);
        let Err(Error::Model(message)) = answer_with(&replay, &cache, names_one) else { panic!("no model error") };
        assert!(message.starts_with("the call, tried twice: "), "{message}");
        assert!(message.contains("does not fit"), "{message}");
        assert_eq!(kept(&cache), []);
    }

    #[test]
    fn a_kept_reply_that_does_not_fit_is_asked_again() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path(), "book");
        // Kept by a call that took any object, under the key the stricter call asks for.
        answer_with(&Replay::new(vec![r#"{"candidate":1}"#.into()]), &cache, Value::is_object).unwrap();
        let replay = Replay::new(vec![r#"{"candidate":"e1"}"#.into()]);
        let reply = answer_with(&replay, &cache, names_one).unwrap();
        assert_eq!(reply.json, json!({ "candidate": "e1" }));
        assert_eq!(replay.remaining(), 0);
        assert_eq!(kept(&cache), [Some(reply)], "written over the one that did not fit");
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
