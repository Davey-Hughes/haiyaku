//! A self-contained HTML page with a unit slider. Each step's graph is the
//! fold of the units up to it, and its layout starts from the step before, so
//! nothing on screen depends on a later unit.

use anyhow::Result;
use serde::Serialize;
use serde_json::json;

use crate::{Run, events::Event};

const TEMPLATE: &str = include_str!("view.html");

#[derive(Serialize)]
struct Node {
    id: String,
    name: String,
    aliases: Vec<String>,
    first: usize,
    seen: usize,
    here: bool,
    evidence: String,
    merged: Vec<String>,
}

pub fn render(run: &Run) -> Result<String> {
    let mut steps = Vec::new();
    for (k, u) in run.units.iter().enumerate() {
        let before = run.graph_after(k);
        let g = run.graph_after(k + 1);
        let name = |id: &str| g.characters.get(&g.canonical(id)).map_or(id.to_string(), |c| c.name.clone());
        let old_name = |id: &str| before.characters.get(id).map_or_else(|| name(id), |c| c.name.clone());
        let characters: Vec<Node> = g
            .characters
            .values()
            .map(|c| Node {
                id: c.id.clone(),
                name: c.name.clone(),
                aliases: c.aliases.clone(),
                first: c.first,
                seen: c.seen.len(),
                here: c.seen.contains(&u.index),
                evidence: c.evidence.clone(),
                merged: c.merged.clone(),
            })
            .collect();
        let edges: Vec<_> = g.edges.values().collect();
        let changes: Vec<String> = u
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Introduced { name, .. } => Some(format!("New: {name}")),
                Event::Appeared { .. } => None,
                Event::Alias { id, alias } => Some(format!("{} is also “{alias}”", name(id))),
                Event::Relation { from, to, kind, note, .. } => {
                    let note = if note.is_empty() { String::new() } else { format!(" ({note})") };
                    Some(format!("{} – {}{note} – {}", name(from), kind.name(), name(to)))
                }
                Event::SameAs { id, other, .. } => Some(format!("Revealed: {} is {}", old_name(other), old_name(id))),
            })
            .collect();
        let tokens = match (u.prompt_tokens, u.output_tokens) {
            (Some(i), Some(o)) => format!(" · {i} in / {o} out tokens"),
            _ => String::new(),
        };
        let cost = u.cost_usd.map_or(String::new(), |c| format!(" · {c:.3} USD"));
        let error = u.error.as_deref().map_or(String::new(), |e| format!(" · ERROR {e}"));
        let stats = format!(
            "{} · {:.0} s{tokens}{cost} · {} events kept · {} rejected{error}",
            run.backend,
            u.seconds,
            u.events.len(),
            u.rejected.len()
        );
        steps.push(json!({
            "index": u.index, "title": u.title, "stats": stats, "characters": characters,
            "edges": edges, "changes": changes, "rejected": u.rejected,
        }));
    }
    let units: Vec<_> = run.units.iter().map(|u| json!({"index": u.index, "title": u.title})).collect();
    let data = json!({"book": run.book, "backend": run.backend, "units": units, "steps": steps});
    let data = serde_json::to_string(&data)?.replace("</", "<\\/");
    Ok(TEMPLATE.replace("__DATA__", &data))
}
