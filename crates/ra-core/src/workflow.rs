use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentConfig;
use crate::error::{RaError, RaResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub config: WorkflowConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    #[serde(default)]
    pub max_total_budget_usd: Option<f64>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub retry_failed_steps: bool,
    #[serde(default = "default_max_retries")]
    pub max_retries_per_step: u32,
}

fn default_concurrency() -> usize {
    4
}
fn default_max_retries() -> u32 {
    1
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            max_total_budget_usd: None,
            timeout_seconds: None,
            retry_failed_steps: false,
            max_retries_per_step: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub agent_config: AgentConfig,
    #[serde(default)]
    pub depends_on: Vec<StepDependency>,
    #[serde(default)]
    pub on_failure: Option<FailureAction>,
    #[serde(default)]
    pub output_var: Option<String>,
    /// If true, inject shared context values into the agent prompt
    #[serde(default)]
    pub inject_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDependency {
    pub step_id: String,
    #[serde(default = "default_condition")]
    pub condition: DependencyCondition,
}

fn default_condition() -> DependencyCondition {
    DependencyCondition::Success
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyCondition {
    #[default]
    Success,
    Failure,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureAction {
    Skip,
    Abort,
    Fallback {
        step_id: String,
    },
    Retry {
        max_attempts: u32,
    },
    /// Wrapper for YAML shorthand: `on_failure: { retry: { max_attempts: N } }`
    #[serde(untagged)]
    Wrapped(FailureActionWrapper),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureActionWrapper {
    #[serde(default)]
    pub retry: Option<RetryConfig>,
    #[serde(default)]
    pub fallback: Option<FallbackConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    pub step_id: String,
}

impl FailureAction {
    /// Normalize wrapped variants into canonical form
    pub fn normalize(&self) -> FailureAction {
        match self {
            FailureAction::Wrapped(w) => {
                if let Some(ref r) = w.retry {
                    FailureAction::Retry {
                        max_attempts: r.max_attempts,
                    }
                } else if let Some(ref f) = w.fallback {
                    FailureAction::Fallback {
                        step_id: f.step_id.clone(),
                    }
                } else {
                    FailureAction::Skip
                }
            }
            other => other.clone(),
        }
    }
}

impl Workflow {
    /// Validate the DAG: no cycles, all dependencies exist, step IDs unique
    pub fn validate(&self) -> RaResult<()> {
        let step_ids: HashSet<&str> = self.steps.iter().map(|s| s.id.as_str()).collect();

        // Check unique IDs
        if step_ids.len() != self.steps.len() {
            return Err(RaError::WorkflowValidation(
                "Duplicate step IDs found".to_string(),
            ));
        }

        // Check all dependencies exist
        for step in &self.steps {
            for dep in &step.depends_on {
                if !step_ids.contains(dep.step_id.as_str()) {
                    return Err(RaError::DependencyNotFound {
                        step: step.id.clone(),
                        dep: dep.step_id.clone(),
                    });
                }
            }

            // Check fallback references
            if let Some(FailureAction::Fallback { step_id }) = &step.on_failure {
                if !step_ids.contains(step_id.as_str()) {
                    return Err(RaError::WorkflowValidation(format!(
                        "Fallback step '{}' not found for step '{}'",
                        step_id, step.id
                    )));
                }
            }
        }

        // Detect cycles using Kahn's algorithm
        self.check_no_cycles()?;

        Ok(())
    }

    /// Kahn's algorithm for cycle detection
    fn check_no_cycles(&self) -> RaResult<()> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for step in &self.steps {
            in_degree.entry(step.id.as_str()).or_insert(0);
            adjacency.entry(step.id.as_str()).or_default();
        }

        for step in &self.steps {
            for dep in &step.depends_on {
                adjacency
                    .entry(dep.step_id.as_str())
                    .or_default()
                    .push(step.id.as_str());
                *in_degree.entry(step.id.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut visited = 0;

        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(neighbors) = adjacency.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if visited != self.steps.len() {
            return Err(RaError::DagCycle);
        }

        Ok(())
    }

    /// Returns steps in topological order
    pub fn topological_order(&self) -> RaResult<Vec<&Step>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        let step_map: HashMap<&str, &Step> =
            self.steps.iter().map(|s| (s.id.as_str(), s)).collect();

        for step in &self.steps {
            in_degree.entry(step.id.as_str()).or_insert(0);
            adjacency.entry(step.id.as_str()).or_default();
        }

        for step in &self.steps {
            for dep in &step.depends_on {
                adjacency
                    .entry(dep.step_id.as_str())
                    .or_default()
                    .push(step.id.as_str());
                *in_degree.entry(step.id.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();

        while let Some(node) = queue.pop_front() {
            result.push(*step_map.get(node).unwrap());
            if let Some(neighbors) = adjacency.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if result.len() != self.steps.len() {
            return Err(RaError::DagCycle);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_step(id: &str, deps: Vec<&str>) -> Step {
        Step {
            id: id.to_string(),
            name: id.to_string(),
            prompt: format!("Do {}", id),
            agent_config: AgentConfig::default(),
            depends_on: deps
                .into_iter()
                .map(|d| StepDependency {
                    step_id: d.to_string(),
                    condition: DependencyCondition::Success,
                })
                .collect(),
            on_failure: None,
            output_var: None,
            inject_context: false,
        }
    }

    fn make_workflow(steps: Vec<Step>) -> Workflow {
        Workflow {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            steps,
            config: WorkflowConfig::default(),
        }
    }

    #[test]
    fn test_valid_linear_dag() {
        let wf = make_workflow(vec![
            make_step("a", vec![]),
            make_step("b", vec!["a"]),
            make_step("c", vec!["b"]),
        ]);
        assert!(wf.validate().is_ok());
    }

    #[test]
    fn test_valid_diamond_dag() {
        let wf = make_workflow(vec![
            make_step("a", vec![]),
            make_step("b", vec!["a"]),
            make_step("c", vec!["a"]),
            make_step("d", vec!["b", "c"]),
        ]);
        assert!(wf.validate().is_ok());
    }

    #[test]
    fn test_valid_parallel_dag() {
        let wf = make_workflow(vec![
            make_step("a", vec![]),
            make_step("b", vec![]),
            make_step("c", vec![]),
        ]);
        assert!(wf.validate().is_ok());
    }

    #[test]
    fn test_cycle_detection() {
        let wf = make_workflow(vec![
            make_step("a", vec!["c"]),
            make_step("b", vec!["a"]),
            make_step("c", vec!["b"]),
        ]);
        assert!(matches!(wf.validate(), Err(RaError::DagCycle)));
    }

    #[test]
    fn test_missing_dependency() {
        let wf = make_workflow(vec![make_step("a", vec!["nonexistent"])]);
        assert!(matches!(
            wf.validate(),
            Err(RaError::DependencyNotFound { .. })
        ));
    }

    #[test]
    fn test_duplicate_ids() {
        let wf = make_workflow(vec![make_step("a", vec![]), make_step("a", vec![])]);
        assert!(matches!(wf.validate(), Err(RaError::WorkflowValidation(_))));
    }

    #[test]
    fn test_topological_order() {
        let wf = make_workflow(vec![
            make_step("c", vec!["a", "b"]),
            make_step("a", vec![]),
            make_step("b", vec!["a"]),
        ]);
        let order = wf.topological_order().unwrap();
        let ids: Vec<&str> = order.iter().map(|s| s.id.as_str()).collect();

        // a must come before b and c, b must come before c
        let pos_a = ids.iter().position(|&x| x == "a").unwrap();
        let pos_b = ids.iter().position(|&x| x == "b").unwrap();
        let pos_c = ids.iter().position(|&x| x == "c").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_self_cycle() {
        let wf = make_workflow(vec![make_step("a", vec!["a"])]);
        assert!(matches!(wf.validate(), Err(RaError::DagCycle)));
    }
}
