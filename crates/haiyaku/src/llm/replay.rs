//! Recorded replies given out in order, so a test or a rerun calls no model.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Backend, Reply, Request, json_object};
use crate::{Error, Result};

/// A backend that answers each call with the next of its recorded reply texts, whatever it is asked. The replies
/// go out in the order of the calls: a run that takes some chapters' replies from a cache calls for the other
/// chapters alone, so the first chapter not cached gets the next recorded reply, not the one recorded for it.
#[derive(Debug)]
pub struct Replay {
    replies: Vec<String>,
    /// The directory the replies were read from, canonicalised so that one directory has one spelling in a
    /// cache key; empty for replies given in code.
    dir: String,
    /// The index of the next reply to give out, past the end once they have run out. An atomic because
    /// [`Backend::call`] takes `&self`, and it keeps a replay `Sync` where a `Cell` would not.
    next: AtomicUsize,
}

impl Replay {
    /// A replay of `replies`, the texts a model wrote, given out in order.
    pub fn new(replies: Vec<String>) -> Replay {
        Replay { replies, dir: String::new(), next: AtomicUsize::new(0) }
    }

    /// A replay of the files in `dir`, given out in the order of their names. A name that starts with `.` or
    /// ends with `~` is passed over: a hidden file, such as the `.gitkeep` that keeps a directory in git, or an
    /// editor's backup is no recorded reply. An I/O error names the directory or the file it came from.
    pub fn from_dir(dir: &Path) -> Result<Replay> {
        let canonical = std::fs::canonicalize(dir).map_err(|error| at(dir, error))?;
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|error| at(dir, error))? {
            let entry = entry.map_err(|error| at(dir, error))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name.ends_with('~') {
                continue;
            }
            let path = entry.path();
            if path.is_file() {
                paths.push(path);
            }
        }
        // One directory's paths differ only in their last component, so this is the order of the names.
        paths.sort();
        let replies = paths
            .iter()
            .map(|path| std::fs::read_to_string(path).map_err(|error| at(path, error)))
            .collect::<Result<_>>()?;
        Ok(Replay { dir: canonical.to_string_lossy().into_owned(), ..Replay::new(replies) })
    }

    /// How many replies are left to give out.
    pub fn remaining(&self) -> usize {
        self.replies.len().saturating_sub(self.next.load(Ordering::Relaxed))
    }
}

/// `error`, which came from `path`, as an [`Error::Io`] that names it: the error alone does not say which file.
fn at(path: &Path, error: io::Error) -> Error {
    Error::Io(io::Error::new(error.kind(), format!("{}: {error}", path.display())))
}

impl Backend for Replay {
    fn name(&self) -> &str {
        "replay"
    }

    fn model(&self) -> &str {
        "none"
    }

    fn endpoint(&self) -> &str {
        &self.dir
    }

    fn call(&self, _request: &Request<'_>) -> Result<Reply> {
        let index = self.next.fetch_add(1, Ordering::Relaxed);
        let Some(raw) = self.replies.get(index) else {
            let total = self.replies.len();
            let calls = if total == 1 { "call" } else { "calls" };
            return Err(Error::Model(format!("replay: out of recorded replies after answering {total} {calls}")));
        };
        let json = json_object(raw).map_err(|cause| Error::Model(format!("replay: reply {}: {cause}", index + 1)))?;
        Ok(Reply { json, raw: raw.clone(), seconds: 0.0, cost_usd: None })
    }
}
