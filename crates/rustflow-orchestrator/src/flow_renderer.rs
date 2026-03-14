use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;

use rustflow_core::step::{Step, StepKind, StepState};

/// A group of steps that can execute in parallel (same topological depth).
#[derive(Debug, Clone)]
pub struct ExecutionLayer {
    /// Layer index (0-based).
    pub index: usize,
    /// Step IDs in this layer.
    pub step_ids: Vec<String>,
}

/// Computes execution layers from a list of steps.
///
/// Each layer contains steps that share the same topological depth — meaning
/// all their dependencies are in earlier layers. Steps within one layer can
/// run in parallel.
pub fn compute_layers(steps: &[Step]) -> Vec<ExecutionLayer> {
    if steps.is_empty() {
        return vec![];
    }

    let ids: Vec<String> = steps.iter().map(|s| s.id.as_str().to_string()).collect();
    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    // Build in-degree and adjacency.
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    let dep_map: HashMap<&str, Vec<&str>> = steps
        .iter()
        .map(|s| {
            let deps: Vec<&str> = s
                .depends_on
                .iter()
                .map(|d| d.as_str())
                .filter(|d| id_set.contains(d))
                .collect();
            (s.id.as_str(), deps)
        })
        .collect();

    for &id in &ids.iter().map(|s| s.as_str()).collect::<Vec<_>>() {
        in_degree.insert(id, dep_map.get(id).map_or(0, |d| d.len()));
        dependents.entry(id).or_default();
    }

    for (id, deps) in &dep_map {
        for dep in deps {
            dependents.entry(dep).or_default().push(id);
        }
    }

    // BFS layer by layer (Kahn's algorithm, but collecting by depth).
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut layers: Vec<ExecutionLayer> = vec![];
    let mut layer_idx = 0;

    while !queue.is_empty() {
        let current_layer: Vec<String> = queue.drain(..).map(|s| s.to_string()).collect();
        let mut next_queue: Vec<&str> = vec![];

        for id in &current_layer {
            if let Some(deps) = dependents.get(id.as_str()) {
                for &dep in deps {
                    let deg = in_degree.get_mut(dep).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next_queue.push(dep);
                    }
                }
            }
        }

        // Sort step IDs within a layer for deterministic output.
        let mut sorted = current_layer;
        sorted.sort();

        layers.push(ExecutionLayer {
            index: layer_idx,
            step_ids: sorted,
        });
        layer_idx += 1;

        for id in next_queue {
            queue.push_back(id);
        }
    }

    layers
}

/// Step info for rendering.
struct StepInfo<'a> {
    _id: &'a str,
    name: &'a str,
    kind_label: String,
    deps: Vec<&'a str>,
}

fn step_info<'a>(step: &'a Step) -> StepInfo<'a> {
    let kind_label = match &step.kind {
        StepKind::Llm(cfg) => format!("{}/{}", cfg.provider, cfg.model),
        StepKind::Tool(cfg) => cfg.tool.clone(),
    };
    let deps: Vec<&str> = step.depends_on.iter().map(|d| d.as_str()).collect();
    StepInfo {
        _id: step.id.as_str(),
        name: &step.name,
        kind_label,
        deps,
    }
}

/// Renders the DAG as a text-based flowchart.
pub fn render_flowchart(steps: &[Step], workflow_name: &str) -> String {
    let layers = compute_layers(steps);
    let step_map: HashMap<&str, &Step> = steps.iter().map(|s| (s.id.as_str(), s)).collect();

    let mut out = String::new();

    // Header
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  ╔═══ Workflow: {} ({} steps) ═══╗",
        workflow_name,
        steps.len()
    );
    let _ = writeln!(out);

    for (i, layer) in layers.iter().enumerate() {
        let is_parallel = layer.step_ids.len() > 1;
        let mode = if is_parallel { "parallel" } else { "serial" };

        let _ = writeln!(
            out,
            "  ┌─ Layer {} ── {} ({}) ─────────────────────────",
            layer.index + 1,
            mode,
            layer.step_ids.len()
        );

        for step_id in &layer.step_ids {
            if let Some(&step) = step_map.get(step_id.as_str()) {
                let info = step_info(step);
                let deps_str = if info.deps.is_empty() {
                    String::new()
                } else {
                    format!(" ← {}", info.deps.join(", "))
                };
                let _ = writeln!(
                    out,
                    "  │  ○ {} [{}]{}",
                    info.name, info.kind_label, deps_str
                );
                let _ = writeln!(out, "  │    id: {step_id}");
            }
        }

        let _ = writeln!(out, "  └──────────────────────────────────────────────");

        // Arrow between layers.
        if i < layers.len() - 1 {
            let _ = writeln!(out, "                     │");
            let _ = writeln!(out, "                     ▼");
        }
    }

    let _ = writeln!(out);
    out
}

/// Renders a single-line step status update for live progress.
pub fn render_step_event(
    step: &Step,
    state: &StepState,
    elapsed: Option<std::time::Duration>,
) -> String {
    let icon = match state {
        StepState::Pending => "○",
        StepState::Running => "◉",
        StepState::Success => "✓",
        StepState::Failed => "✗",
        StepState::Retrying => "↻",
    };

    let info = step_info(step);
    let elapsed_str = elapsed.map_or(String::new(), |d| format!(" ({:.1}s)", d.as_secs_f64()));

    format!(
        "  {icon} {name} [{kind}]{elapsed}",
        icon = icon,
        name = info.name,
        kind = info.kind_label,
        elapsed = elapsed_str,
    )
}

/// Renders the full execution summary after all steps complete.
pub fn render_summary(
    steps: &[Step],
    states: &HashMap<String, StepState>,
    durations: &HashMap<String, std::time::Duration>,
    total_elapsed: std::time::Duration,
) -> String {
    let mut out = String::new();
    let layers = compute_layers(steps);
    let step_map: HashMap<&str, &Step> = steps.iter().map(|s| (s.id.as_str(), s)).collect();

    let _ = writeln!(out);
    let _ = writeln!(out, "  ╔═══ Execution Summary ═══╗");
    let _ = writeln!(out);

    for layer in &layers {
        let is_parallel = layer.step_ids.len() > 1;
        let mode = if is_parallel { "parallel" } else { "serial" };

        let _ = writeln!(
            out,
            "  ┌─ Layer {} ── {} ─────────────────────────",
            layer.index + 1,
            mode,
        );

        for step_id in &layer.step_ids {
            if let Some(&step) = step_map.get(step_id.as_str()) {
                let state = states
                    .get(step_id)
                    .cloned()
                    .unwrap_or(StepState::Pending);
                let duration = durations.get(step_id);
                let line = render_step_event(step, &state, duration.copied());
                let _ = writeln!(out, "{line}");
            }
        }

        let _ = writeln!(out, "  └──────────────────────────────────────────────");
    }

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  Total: {:.1}s",
        total_elapsed.as_secs_f64()
    );
    let _ = writeln!(out);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_step(id: &str, deps: Vec<&str>) -> Step {
        use rustflow_core::types::StepId;
        Step::new_tool(id, id, "http", serde_json::Value::Null)
            .with_depends_on(deps.into_iter().map(StepId::from).collect())
    }

    #[test]
    fn test_compute_layers_linear() {
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec!["a"]),
            tool_step("c", vec!["b"]),
        ];
        let layers = compute_layers(&steps);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].step_ids, vec!["a"]);
        assert_eq!(layers[1].step_ids, vec!["b"]);
        assert_eq!(layers[2].step_ids, vec!["c"]);
    }

    #[test]
    fn test_compute_layers_parallel() {
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec![]),
            tool_step("c", vec!["a", "b"]),
        ];
        let layers = compute_layers(&steps);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].step_ids, vec!["a", "b"]);
        assert_eq!(layers[1].step_ids, vec!["c"]);
    }

    #[test]
    fn test_compute_layers_diamond() {
        use rustflow_core::types::StepId;
        let steps = vec![
            tool_step("a", vec![]),
            Step::new_tool("b", "b", "http", serde_json::Value::Null)
                .with_depends_on(vec![StepId::from("a")]),
            Step::new_tool("c", "c", "http", serde_json::Value::Null)
                .with_depends_on(vec![StepId::from("a")]),
            Step::new_tool("d", "d", "http", serde_json::Value::Null)
                .with_depends_on(vec![StepId::from("b"), StepId::from("c")]),
        ];
        let layers = compute_layers(&steps);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].step_ids, vec!["a"]);
        assert_eq!(layers[1].step_ids, vec!["b", "c"]);
        assert_eq!(layers[2].step_ids, vec!["d"]);
    }

    #[test]
    fn test_compute_layers_empty() {
        let layers = compute_layers(&[]);
        assert!(layers.is_empty());
    }
}
