use crate::error::{OrchestratorError, Result};
use rustflow_core::step::Step;
use rustflow_core::types::StepId;
use std::collections::{HashMap, HashSet, VecDeque};

/// Validates a list of steps, ensuring the dependency graph is a valid DAG,
/// and returns the steps in topological execution order.
pub struct DagParser;

impl DagParser {
    /// Parse and validate steps, returning them in topological order.
    ///
    /// Errors if:
    /// - Any step has a duplicate ID.
    /// - Any dependency references a non-existent step ID.
    /// - The dependency graph contains a cycle.
    pub fn parse(steps: &[Step]) -> Result<Vec<StepId>> {
        // 1. Build the ID → index map and check for duplicates.
        let mut id_set: HashMap<&str, usize> = HashMap::new();
        for (i, step) in steps.iter().enumerate() {
            let key = step.id.as_str();
            if id_set.contains_key(key) {
                return Err(OrchestratorError::DuplicateStepId {
                    step_id: key.to_string(),
                });
            }
            id_set.insert(key, i);
        }

        // 2. Validate all dependency references.
        for step in steps {
            for dep in &step.depends_on {
                if !id_set.contains_key(dep.as_str()) {
                    return Err(OrchestratorError::UnknownDependency {
                        step_id: step.id.as_str().to_string(),
                        dependency: dep.as_str().to_string(),
                    });
                }
            }
        }

        // 3. Topological sort using Kahn's algorithm.
        // Build adjacency list (dependency → dependents) and in-degree map.
        let n = steps.len();
        let mut in_degree: Vec<usize> = vec![0; n];
        // edges[i] = list of step indices that depend on step i
        let mut edges: Vec<Vec<usize>> = vec![vec![]; n];

        for (i, step) in steps.iter().enumerate() {
            for dep in &step.depends_on {
                let dep_idx = id_set[dep.as_str()];
                edges[dep_idx].push(i);
                in_degree[i] += 1;
            }
        }

        let mut queue: VecDeque<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|&(_, &d)| d == 0)
            .map(|(i, _)| i)
            .collect();

        let mut order: Vec<StepId> = Vec::with_capacity(n);
        let mut visited = 0usize;

        while let Some(idx) = queue.pop_front() {
            order.push(steps[idx].id.clone());
            visited += 1;
            for &next in &edges[idx] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push_back(next);
                }
            }
        }

        if visited != n {
            // Find a step still in a cycle (has remaining in-degree > 0).
            let cycle_step = steps
                .iter()
                .enumerate()
                .find(|(i, _)| in_degree[*i] > 0)
                .map(|(_, s)| s.id.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(OrchestratorError::CycleDetected {
                step_id: cycle_step,
            });
        }

        Ok(order)
    }

    /// Returns a map of `step_id -> set of direct dependencies`.
    pub fn build_dependency_map(steps: &[Step]) -> HashMap<String, HashSet<String>> {
        steps
            .iter()
            .map(|s| {
                let deps = s
                    .depends_on
                    .iter()
                    .map(|d| d.as_str().to_string())
                    .collect();
                (s.id.as_str().to_string(), deps)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflow_core::step::Step;

    fn tool_step(id: &str, deps: Vec<&str>) -> Step {
        let mut s = Step::new_tool(id, id, "noop", serde_json::Value::Null);
        s.depends_on = deps.into_iter().map(StepId::from).collect();
        s
    }

    #[test]
    fn test_linear_dag() {
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec!["a"]),
            tool_step("c", vec!["b"]),
        ];
        let order = DagParser::parse(&steps).unwrap();
        assert_eq!(
            order,
            vec![StepId::from("a"), StepId::from("b"), StepId::from("c")]
        );
    }

    #[test]
    fn test_cycle_detection() {
        let steps = vec![
            tool_step("a", vec!["c"]),
            tool_step("b", vec!["a"]),
            tool_step("c", vec!["b"]),
        ];
        let err = DagParser::parse(&steps).unwrap_err();
        assert!(matches!(err, OrchestratorError::CycleDetected { .. }));
    }

    #[test]
    fn test_duplicate_step_id() {
        let steps = vec![tool_step("a", vec![]), tool_step("a", vec![])];
        let err = DagParser::parse(&steps).unwrap_err();
        assert!(matches!(err, OrchestratorError::DuplicateStepId { .. }));
    }

    #[test]
    fn test_unknown_dependency() {
        let steps = vec![tool_step("a", vec!["missing"])];
        let err = DagParser::parse(&steps).unwrap_err();
        assert!(matches!(err, OrchestratorError::UnknownDependency { .. }));
    }

    #[test]
    fn test_empty_dag() {
        let steps: Vec<Step> = vec![];
        let order = DagParser::parse(&steps).unwrap();
        assert!(order.is_empty());
    }

    #[test]
    fn test_single_step() {
        let steps = vec![tool_step("only", vec![])];
        let order = DagParser::parse(&steps).unwrap();
        assert_eq!(order, vec![StepId::from("only")]);
    }

    #[test]
    fn test_diamond_dag() {
        //     a
        //    / \
        //   b   c
        //    \ /
        //     d
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec!["a"]),
            tool_step("c", vec!["a"]),
            tool_step("d", vec!["b", "c"]),
        ];
        let order = DagParser::parse(&steps).unwrap();
        // "a" must come first, "d" must come last
        assert_eq!(order[0], StepId::from("a"));
        assert_eq!(order[3], StepId::from("d"));
        // b and c can be in either order
        let mid: Vec<&StepId> = order[1..3].iter().collect();
        assert!(mid.contains(&&StepId::from("b")));
        assert!(mid.contains(&&StepId::from("c")));
    }

    #[test]
    fn test_parallel_independent_steps() {
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec![]),
            tool_step("c", vec![]),
        ];
        let order = DagParser::parse(&steps).unwrap();
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn test_self_cycle() {
        let steps = vec![tool_step("a", vec!["a"])];
        let err = DagParser::parse(&steps).unwrap_err();
        assert!(matches!(err, OrchestratorError::CycleDetected { .. }));
    }

    #[test]
    fn test_build_dependency_map() {
        let steps = vec![
            tool_step("a", vec![]),
            tool_step("b", vec!["a"]),
            tool_step("c", vec!["a", "b"]),
        ];
        let map = DagParser::build_dependency_map(&steps);
        assert!(map["a"].is_empty());
        assert_eq!(map["b"], HashSet::from(["a".to_string()]));
        assert_eq!(
            map["c"],
            HashSet::from(["a".to_string(), "b".to_string()])
        );
    }
}
