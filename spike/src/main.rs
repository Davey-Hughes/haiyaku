//! haiyaku spike: does a model reading one chapter at a time produce
//! character events that are consistent and grounded in the text, and how
//! long does a chapter take on each backend?
//!
//!   haiyaku-spike units <book.epub>
//!   haiyaku-spike extract <book.epub> --backend ollama:gemma4:12b-it-qat --run hp1-gemma --units 3-19
//!   haiyaku-spike report out/hp1-gemma.json
//!   haiyaku-spike view out/hp1-gemma.json
//!   haiyaku-spike link out/hp1-sonnet.json out/hp2-sonnet.json --run series --backend claude

mod book;
mod events;
mod extract;
mod link;
mod llm;
mod view;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::{events::Graph, extract::Rejected};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List a book's units with their size.
    Units { book: PathBuf },
    /// Extract events unit by unit, in order, resuming a run already started.
    Extract {
        book: PathBuf,
        #[arg(long)]
        backend: String,
        /// The run's name: its results go to <out>/<run>.json.
        #[arg(long)]
        run: String,
        /// Unit indexes from `units`, as 3-19 or 5.
        #[arg(long)]
        units: Option<String>,
        #[arg(long, default_value = "out")]
        out: PathBuf,
        /// Runs of the books before this one, in reading order: their cast is
        /// carried into this book's prompts.
        #[arg(long)]
        after: Vec<PathBuf>,
    },
    /// Join runs of books extracted on their own into one series run, in the
    /// order given, matching characters across books by name.
    Link {
        runs: Vec<PathBuf>,
        /// The series run's name: it goes to <out>/<run>.json.
        #[arg(long)]
        run: String,
        #[arg(long, default_value = "out")]
        out: PathBuf,
        /// The model that decides the weak name matches.
        #[arg(long, default_value = "claude")]
        backend: String,
    },
    /// Timing and validation summary of a run.
    Report { run: PathBuf },
    /// Write a self-contained HTML viewer next to the run file.
    View { run: PathBuf },
}

#[derive(Serialize, Deserialize)]
pub struct Run {
    pub book: String,
    pub backend: String,
    pub units: Vec<UnitRecord>,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct UnitRecord {
    pub index: usize,
    pub title: String,
    pub chars: usize,
    pub seconds: f64,
    pub prompt_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub events: Vec<events::Event>,
    pub rejected: Vec<Rejected>,
    pub counts: BTreeMap<String, u32>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Run {
    /// The graph after the first `count` units of the run.
    pub fn graph_after(&self, count: usize) -> Graph {
        Graph::fold(self.units.iter().take(count).map(|u| (u.index, u.events.as_slice())))
    }
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Units { book } => {
            for (i, u) in book::read_epub(&book)?.iter().enumerate() {
                let words = u.text.split_whitespace().count();
                println!("{i:>3}  {:>7} chars  {words:>6} words  {}", u.text.chars().count(), u.title);
            }
        }
        Cmd::Extract { book, backend, run, units, out, after } => {
            extract_run(&book, &backend, &run, units.as_deref(), &out, &after)?
        }
        Cmd::Link { runs, run, out, backend } => {
            let loaded = runs.iter().map(|p| load(p)).collect::<Result<Vec<_>>>()?;
            let (series, lines) = link::link(&loaded, &llm::Backend::parse(&backend)?);
            for l in lines {
                println!("{l}");
            }
            let path = out.join(format!("{run}.json"));
            save(&path, &series)?;
            println!("{}", path.display());
        }
        Cmd::Report { run } => report(&load(&run)?),
        Cmd::View { run } => {
            let r = load(&run)?;
            let path = run.with_extension("html");
            fs::write(&path, view::render(&r)?)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn load(path: &Path) -> Result<Run> {
    serde_json::from_str(&fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?)
        .with_context(|| format!("parsing {}", path.display()))
}

fn save(path: &Path, run: &Run) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(run)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn parse_range(s: &str, len: usize) -> Result<(usize, usize)> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let (a, b): (usize, usize) = (a.trim().parse()?, b.trim().parse()?);
    if a > b || b >= len {
        bail!("units {s} outside 0-{}", len - 1);
    }
    Ok((a, b))
}

fn extract_run(
    book_path: &Path,
    backend: &str,
    name: &str,
    units: Option<&str>,
    out: &Path,
    after: &[PathBuf],
) -> Result<()> {
    let prior = after.iter().map(|p| load(p)).collect::<Result<Vec<_>>>()?;
    let book = book::read_epub(book_path)?;
    let llm = llm::Backend::parse(backend)?;
    let (from, to) = match units {
        Some(s) => parse_range(s, book.len())?,
        None => (0, book.len() - 1),
    };
    fs::create_dir_all(out.join(format!("{name}.raw")))?;
    let path = out.join(format!("{name}.json"));
    let mut run = if path.exists() {
        let r = load(&path)?;
        if r.backend != backend {
            bail!("{} was made with {}, not {backend}", path.display(), r.backend);
        }
        r
    } else {
        Run {
            book: book_path.display().to_string(),
            backend: backend.to_string(),
            units: vec![],
            after: after.iter().map(|p| p.display().to_string()).collect(),
        }
    };
    let schema = extract::schema();
    for index in from..=to {
        if run.units.iter().any(|u| u.index == index && u.error.is_none()) {
            continue;
        }
        run.units.retain(|u| u.index != index);
        if run.units.last().is_some_and(|u| u.index > index) {
            bail!("unit {index} comes before units already in the run; units go in reading order");
        }
        let unit = &book[index];
        let graph = Graph::fold(
            prior
                .iter()
                .flat_map(|r| r.units.iter().map(|u| (0, u.events.as_slice())))
                .chain(run.units.iter().map(|u| (u.index, u.events.as_slice()))),
        );
        let prompt = extract::prompt(&graph, run.units.len() + 1, &unit.title, &unit.text);
        eprint!("[{index}] {} ... ", unit.title);
        let mut rec = UnitRecord {
            index,
            title: unit.title.clone(),
            chars: unit.text.chars().count(),
            seconds: 0.0,
            prompt_tokens: None,
            output_tokens: None,
            cost_usd: None,
            events: vec![],
            rejected: vec![],
            counts: BTreeMap::new(),
            error: None,
        };
        match llm.call(extract::SYSTEM, &prompt, &schema) {
            Ok(reply) => {
                fs::write(out.join(format!("{name}.raw/{index:03}.json")), &reply.raw)?;
                let o = extract::check(&graph, &unit.text, &reply.json);
                rec.seconds = reply.seconds;
                rec.prompt_tokens = reply.prompt_tokens;
                rec.output_tokens = reply.output_tokens;
                rec.cost_usd = reply.cost_usd;
                rec.events = o.events;
                rec.rejected = o.rejected;
                rec.counts = o.counts;
                eprintln!("{:.0} s, {} events, {} rejected", rec.seconds, rec.events.len(), rec.rejected.len());
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                rec.error = Some(format!("{e:#}"));
            }
        }
        let failed = rec.error.is_some();
        run.units.push(rec);
        save(&path, &run)?;
        if failed {
            bail!("stopped at unit {index}; run again to retry it");
        }
    }
    report(&run);
    Ok(())
}

fn report(run: &Run) {
    println!("{}  ({})", run.book, run.backend);
    println!(
        "{:>3}  {:<44} {:>6} {:>6} {:>7} {:>6} {:>5} {:>5} {:>4}",
        "#", "unit", "chars", "secs", "in", "out", "new", "rels", "rej"
    );
    let (mut secs, mut cost, mut rej, mut ev) = (0.0, 0.0, 0usize, 0usize);
    let mut reasons: BTreeMap<String, u32> = BTreeMap::new();
    for u in &run.units {
        let new = u.events.iter().filter(|e| matches!(e, events::Event::Introduced { .. })).count();
        let rels = u.events.iter().filter(|e| matches!(e, events::Event::Relation { .. })).count();
        let title: String = u.title.chars().take(44).collect();
        println!(
            "{:>3}  {:<44} {:>6} {:>6.0} {:>7} {:>6} {:>5} {:>5} {:>4}{}",
            u.index,
            title,
            u.chars,
            u.seconds,
            u.prompt_tokens.map_or("-".into(), |t| t.to_string()),
            u.output_tokens.map_or("-".into(), |t| t.to_string()),
            new,
            rels,
            u.rejected.len(),
            u.error.as_deref().map_or(String::new(), |e| format!("  ERROR {e}"))
        );
        secs += u.seconds;
        cost += u.cost_usd.unwrap_or(0.0);
        rej += u.rejected.len();
        ev += u.events.len();
        for (k, v) in &u.counts {
            *reasons.entry(k.clone()).or_default() += v;
        }
    }
    let n = run.units.len().max(1);
    println!(
        "\n{} units, {:.0} s ({:.0} s a unit), {ev} events kept, {rej} items rejected, {cost:.2} USD",
        run.units.len(),
        secs,
        secs / n as f64
    );
    println!("counts: {}", reasons.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
    let g = run.graph_after(run.units.len());
    println!("at the end: {} characters, {} related pairs", g.characters.len(), g.edges.len());
    let many: Vec<String> = g
        .edges
        .values()
        .filter(|e| e.facts.len() > 2)
        .map(|e| format!("{}–{} ({})", e.a, e.b, e.facts.iter().map(|f| f.kind.name()).collect::<Vec<_>>().join("/")))
        .collect();
    if !many.is_empty() {
        println!("pairs with 3+ kinds: {}", many.join("; "));
    }
}
