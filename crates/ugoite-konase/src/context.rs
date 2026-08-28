use crate::{Capability, Observation, ResourceContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Explicit limits keep a model context proportional to the current bounded
/// view, not to the total Work transcript.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextLimits {
    pub max_observations: usize,
    pub max_resources: usize,
    pub max_summary_chars: usize,
    pub max_fact_chars: usize,
    pub max_facts: usize,
    pub max_resource_references: usize,
    pub max_capabilities: usize,
    pub max_safety_hints: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        Self {
            max_observations: 8,
            max_resources: 4,
            max_summary_chars: 1_024,
            max_fact_chars: 512,
            max_facts: 32,
            max_resource_references: 16,
            max_capabilities: 32,
            max_safety_hints: 8,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContextBuildRequest {
    pub work_goal: String,
    pub job_goal: String,
    #[serde(default)]
    pub current_strategy_summary: Option<String>,
    #[serde(default)]
    pub observations: Vec<Observation>,
    #[serde(default)]
    pub available_capabilities: Vec<Capability>,
    #[serde(default)]
    pub selected_resource_contents: Vec<ResourceContent>,
    #[serde(default)]
    pub safety_hints: Vec<String>,
    #[serde(default)]
    pub expected_response_schema: Option<Value>,
    #[serde(default)]
    pub limits: Option<ContextLimits>,
}

#[derive(Clone, Debug, Default)]
pub struct ContextBuilder {
    limits: ContextLimits,
}

impl ContextBuilder {
    pub fn new(limits: ContextLimits) -> Self {
        Self { limits }
    }

    pub fn limits(&self) -> &ContextLimits {
        &self.limits
    }

    pub fn build(&self, request: ContextBuildRequest) -> crate::ContextCapsule {
        let ContextBuildRequest {
            work_goal,
            job_goal,
            current_strategy_summary,
            observations,
            available_capabilities,
            selected_resource_contents,
            safety_hints,
            expected_response_schema,
            limits,
        } = request;
        let limits = limits.unwrap_or_else(|| self.limits.clone());

        let observations = observations
            .into_iter()
            .rev()
            .take(limits.max_observations)
            .map(|mut observation| {
                observation.summary = truncate(&observation.summary, limits.max_summary_chars);
                observation.facts = observation
                    .facts
                    .into_iter()
                    .take(limits.max_facts)
                    .map(|(key, value)| {
                        (
                            truncate(&key, limits.max_summary_chars),
                            truncate(&value, limits.max_fact_chars),
                        )
                    })
                    .collect();
                observation.resource_references = observation
                    .resource_references
                    .into_iter()
                    .take(limits.max_resource_references)
                    .map(|mut reference| {
                        reference.uri = truncate(&reference.uri, limits.max_summary_chars);
                        reference.label = reference
                            .label
                            .map(|label| truncate(&label, limits.max_summary_chars));
                        reference
                    })
                    .collect();
                observation
            })
            .collect::<Vec<_>>();

        let mut available_capabilities = available_capabilities;
        available_capabilities.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.description.cmp(&right.description))
        });
        available_capabilities.truncate(limits.max_capabilities);

        let selected_resource_contents = selected_resource_contents
            .into_iter()
            .take(limits.max_resources)
            .map(|mut resource| {
                resource.content = truncate(&resource.content, limits.max_summary_chars);
                resource
            })
            .collect();

        crate::ContextCapsule {
            work_goal: truncate(&work_goal, limits.max_summary_chars),
            job_goal: truncate(&job_goal, limits.max_summary_chars),
            current_strategy_summary: current_strategy_summary
                .map(|summary| truncate(&summary, limits.max_summary_chars)),
            relevant_observations: observations.into_iter().rev().collect(),
            available_capabilities,
            selected_resource_contents,
            safety_hints: safety_hints
                .into_iter()
                .take(limits.max_safety_hints)
                .map(|hint| truncate(&hint, limits.max_summary_chars))
                .collect(),
            expected_response_schema,
        }
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ObservationKind, ResourceReference};
    use std::collections::BTreeMap;

    fn observation(id: usize) -> Observation {
        Observation {
            id: format!("observation-{id}"),
            kind: ObservationKind::Mcp,
            summary: format!("summary-{id}"),
            facts: BTreeMap::new(),
            resource_references: vec![ResourceReference {
                uri: format!("ugoite://entry/{id}"),
                label: None,
            }],
        }
    }

    #[test]
    fn context_uses_bounded_recent_observations_and_explicit_resources() {
        let request = ContextBuildRequest {
            work_goal: "find notes".into(),
            job_goal: "find the WebAssembly note".into(),
            current_strategy_summary: Some("search first".into()),
            observations: (0..20).map(observation).collect(),
            available_capabilities: vec![Capability {
                name: "ugoite.search".into(),
                description: "Search entries".into(),
            }],
            selected_resource_contents: vec![ResourceContent {
                uri: "ugoite://entry/19".into(),
                content: "selected body".into(),
            }],
            safety_hints: vec!["do not save without confirmation".into()],
            expected_response_schema: None,
            limits: None,
        };

        let context = ContextBuilder::new(ContextLimits {
            max_observations: 3,
            max_resources: 1,
            ..ContextLimits::default()
        })
        .build(request);

        assert_eq!(
            context
                .relevant_observations
                .iter()
                .map(|observation| observation.id.as_str())
                .collect::<Vec<_>>(),
            ["observation-17", "observation-18", "observation-19"]
        );
        assert_eq!(context.selected_resource_contents.len(), 1);
        assert_eq!(
            context.selected_resource_contents[0].uri,
            "ugoite://entry/19"
        );
    }

    #[test]
    fn context_size_does_not_follow_total_observation_count() {
        let builder = ContextBuilder::default();
        let make_request = |count| ContextBuildRequest {
            work_goal: "goal".into(),
            job_goal: "job".into(),
            current_strategy_summary: None,
            observations: (0..count).map(|_| observation(1)).collect(),
            available_capabilities: vec![],
            selected_resource_contents: vec![],
            safety_hints: vec![],
            expected_response_schema: None,
            limits: None,
        };

        let small = serde_json::to_vec(&builder.build(make_request(100))).unwrap();
        let large = serde_json::to_vec(&builder.build(make_request(1_000))).unwrap();
        assert_eq!(small, large);
    }
}
