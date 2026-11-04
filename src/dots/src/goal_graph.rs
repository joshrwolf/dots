//! Dependency graph renderings of a goal's leaf tasks. The renderer sees only
//! nodes, their prerequisite edges, labels and glyphs; callers own the store.
use std::collections::{HashMap, HashSet};

use renderdag::{Ancestor, GraphRowRenderer, Renderer as _};
use serde::Serialize;
use unicode_width::UnicodeWidthStr as _;

/// One task to draw. `prerequisites` name other nodes that must finish first.
#[derive(Debug)]
pub(crate) struct Node<'a> {
    pub id: &'a str,
    /// Text for the layered overview; the row graph leaves labels to the caller.
    pub label: String,
    pub glyph: &'static str,
    pub prerequisites: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Graph {
    /// One row per node, prerequisites above dependents.
    pub rows: Vec<Row>,
    /// Layered overview; its width follows the graph, so displays scroll it.
    pub layered: String,
    /// What the drawings could not show, such as omitted cycle or missing edges.
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Row {
    pub task: String,
    /// The node line, then link lines to the next rows, padded to one width.
    pub lines: Vec<String>,
}

pub(crate) fn render(nodes: &[Node<'_>]) -> Graph {
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id, i))
        .collect();
    let mut omitted = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (dependent, node) in nodes.iter().enumerate() {
        for prerequisite in &node.prerequisites {
            match index.get(prerequisite) {
                Some(&before) => edges.push((before, dependent)),
                None => omitted.push(format!(
                    "{} waits on {prerequisite}, which is not in this graph",
                    node.id
                )),
            }
        }
    }
    let order = topological(nodes.len(), &edges);
    let position: Vec<usize> = {
        let mut position = vec![0; nodes.len()];
        for (at, &node) in order.iter().enumerate() {
            position[node] = at;
        }
        position
    };
    // Only cycles leave an edge pointing upward; drawing it would break the lanes.
    let (kept, cyclic): (Vec<_>, Vec<_>) = edges
        .into_iter()
        .partition(|&(before, after)| position[before] < position[after]);
    for (before, after) in cyclic {
        omitted.push(format!(
            "cycle: {} waits on {}; edge omitted",
            nodes[after].id, nodes[before].id
        ));
    }
    Graph {
        rows: rows(nodes, &order, &kept),
        layered: layered(nodes, &order, &kept),
        notes: omitted,
    }
}

/// Kahn's algorithm, stable in input order; nodes left in a cycle follow in input order.
fn topological(count: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut waiting = vec![0usize; count];
    for &(_, after) in edges {
        waiting[after] += 1;
    }
    let mut order = Vec::with_capacity(count);
    let mut placed = vec![false; count];
    while let Some(next) = (0..count).find(|&i| !placed[i] && waiting[i] == 0) {
        placed[next] = true;
        order.push(next);
        for &(before, after) in edges {
            if before == next {
                waiting[after] -= 1;
            }
        }
    }
    order.extend((0..count).filter(|&i| !placed[i]));
    order
}

fn rows(nodes: &[Node<'_>], order: &[usize], edges: &[(usize, usize)]) -> Vec<Row> {
    // renderdag draws each node above its "parents"; dependents play that role.
    let mut renderer = GraphRowRenderer::new()
        .output()
        .with_min_row_height(1)
        .build_box_drawing();
    let mut rows: Vec<Row> = order
        .iter()
        .map(|&node| {
            let dependents = edges
                .iter()
                .filter(|&&(before, _)| before == node)
                .map(|&(_, after)| Ancestor::Parent(after))
                .collect();
            let text = renderer.next_row(node, dependents, nodes[node].glyph.into(), String::new());
            Row {
                task: nodes[node].id.into(),
                lines: text
                    .lines()
                    .map(|line| line.trim_end().to_owned())
                    .collect(),
            }
        })
        .collect();
    let width = rows
        .iter()
        .flat_map(|row| &row.lines)
        .map(|line| line.width())
        .max()
        .unwrap_or(0);
    for line in rows.iter_mut().flat_map(|row| &mut row.lines) {
        let pad = width - line.width();
        line.extend(std::iter::repeat_n(' ', pad));
    }
    rows
}

fn layered(nodes: &[Node<'_>], order: &[usize], edges: &[(usize, usize)]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    let labels: Vec<String> = nodes
        .iter()
        .map(|node| format!("{} {}", node.glyph, node.label))
        .collect();
    let mut graph = ascii_dag::Graph::new();
    let mut ids = vec![None; nodes.len()];
    for &node in order {
        ids[node] = Some(graph.add_node(ascii_dag::AUTO, labels[node].as_str()));
    }
    let mut seen = HashSet::new();
    for &(before, after) in edges {
        if seen.insert((before, after))
            && let (Some(before), Some(after)) = (ids[before], ids[after])
        {
            graph.add_edge(before, after, None);
        }
    }
    let text = graph
        .compute_layout()
        .render_string(&ascii_dag::RenderOptions::plain());
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node<'a>(id: &'a str, glyph: &'static str, prerequisites: &[&'a str]) -> Node<'a> {
        Node {
            id,
            label: id.to_uppercase(),
            glyph,
            prerequisites: prerequisites.to_vec(),
        }
    }

    fn drawn(graph: &Graph) -> String {
        graph
            .rows
            .iter()
            .map(|row| format!("{}|{}", row.lines.join("\n"), row.task))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn rows_put_prerequisites_above_dependents_with_lanes() {
        let graph = render(&[
            node("release", "○", &["cli", "pi"]),
            node("types", "✓", &[]),
            node("cli", "◌", &["types"]),
            node("pi", "●", &["types"]),
        ]);
        let order: Vec<_> = graph.rows.iter().map(|row| row.task.as_str()).collect();
        assert_eq!(order, ["types", "cli", "pi", "release"]);
        let want = "✓  \n├─╮|types\n◌ │|cli\n│ ●\n├─╯|pi\n○  |release";
        assert_eq!(drawn(&graph), want);
        let widths: HashSet<usize> = graph
            .rows
            .iter()
            .flat_map(|row| &row.lines)
            .map(|line| line.width())
            .collect();
        assert_eq!(widths.len(), 1, "lines share one width: {widths:?}");
        assert_eq!(graph.notes, Vec::<String>::new());
        for label in ["✓ TYPES", "◌ CLI", "● PI", "○ RELEASE"] {
            assert!(
                graph.layered.contains(label),
                "{label} in\n{}",
                graph.layered
            );
        }
    }

    #[test]
    fn independent_tasks_keep_input_order() {
        let graph = render(&[node("b", "○", &[]), node("a", "○", &[])]);
        let order: Vec<_> = graph.rows.iter().map(|row| row.task.as_str()).collect();
        assert_eq!(order, ["b", "a"]);
    }

    #[test]
    fn missing_prerequisites_are_reported() {
        let graph = render(&[node("a", "○", &["elsewhere"])]);
        assert_eq!(graph.rows.len(), 1);
        assert_eq!(
            graph.notes,
            ["a waits on elsewhere, which is not in this graph"]
        );
    }

    #[test]
    fn cycles_keep_every_task_and_report_omitted_edges() {
        let graph = render(&[
            node("a", "○", &["b"]),
            node("b", "○", &["a"]),
            node("c", "○", &[]),
        ]);
        let order: Vec<_> = graph.rows.iter().map(|row| row.task.as_str()).collect();
        assert_eq!(order, ["c", "a", "b"]);
        assert_eq!(graph.notes, ["cycle: a waits on b; edge omitted"]);
    }

    #[test]
    fn an_empty_graph_has_no_rows_or_text() {
        let graph = render(&[]);
        assert_eq!(graph.rows.len(), 0);
        assert_eq!(graph.layered, "");
    }
}
