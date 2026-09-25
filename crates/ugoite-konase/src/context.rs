use crate::{Capability, ContextCapsule, Observation, ResourceContent};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

const MAX_CONTEXT_SCHEMA_BYTES: usize = 16 * 1024;
const MAX_CAPABILITY_NAME_CHARS: usize = 256;
const MAX_SELECTED_RESOURCE_INPUT_BYTES: usize = 128 * 1024;
const MAX_SELECTED_RESOURCE_CONTENT_CHARS: usize = 1_024;

/// Maximum number of user-selected resources admitted to one Konase Job.
pub const MAX_SELECTED_RESOURCES: usize = 4;

/// Maximum serialized size of a normalized Context Capsule.
pub const MAX_CONTEXT_JSON_BYTES: usize = 48 * 1024;

/// Maximum serialized size of the complete normalized capability payload.
pub const MAX_CONTEXT_CAPABILITY_JSON_BYTES: usize = 24 * 1024;

/// Portable preview information for one unique selected resource.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceAdmission {
    pub uri: String,
    pub status: ResourceAdmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ResourceAdmissionReason>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAdmissionStatus {
    Included,
    Truncated,
    Omitted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAdmissionReason {
    ProjectionCompacted,
    ContextByteBudget,
    ModelPromptCharacterLimit,
}

/// Validate, de-duplicate, and compact explicit Form and Entry projections.
/// This is the single portable normalization used before a selected resource
/// can enter a Context Capsule.
pub(crate) fn normalize_selected_resource_contents(
    resources: Vec<ResourceContent>,
) -> Result<(Vec<ResourceContent>, Vec<ResourceAdmission>), String> {
    if resources.len() > MAX_SELECTED_RESOURCES {
        return Err(format!(
            "selected resource count exceeds the {MAX_SELECTED_RESOURCES}-resource limit"
        ));
    }
    let mut seen = HashSet::new();
    let mut contents = Vec::new();
    let mut admissions = Vec::new();
    for resource in resources {
        if !seen.insert(resource.uri.clone()) {
            continue;
        }
        let (content, truncated) = compact_resource_projection(&resource)?;
        admissions.push(ResourceAdmission {
            uri: resource.uri.clone(),
            status: if truncated {
                ResourceAdmissionStatus::Truncated
            } else {
                ResourceAdmissionStatus::Included
            },
            reason: truncated.then_some(ResourceAdmissionReason::ProjectionCompacted),
        });
        contents.push(ResourceContent {
            uri: resource.uri,
            content,
        });
    }
    Ok((contents, admissions))
}

fn compact_resource_projection(resource: &ResourceContent) -> Result<(String, bool), String> {
    if resource.content.len() > MAX_SELECTED_RESOURCE_INPUT_BYTES {
        return Err("selected resource projection exceeds its input byte limit".into());
    }
    let (kind, resource_id) = parse_selected_resource_uri(&resource.uri)?;
    let projection: Value = serde_json::from_str(&resource.content)
        .map_err(|_| "selected resource projection is not valid JSON".to_string())?;
    let projection = projection
        .as_object()
        .ok_or_else(|| "selected resource projection must be a JSON object".to_string())?;
    if projection.get("_untrusted_content") != Some(&Value::Bool(true)) {
        return Err("selected resource projection must be marked untrusted".into());
    }
    let id = required_string(projection, "id")?;
    if id != resource_id {
        return Err("selected resource projection ID does not match its URI".into());
    }

    match kind {
        SelectedResourceKind::Form => compact_form_projection(projection, id),
        SelectedResourceKind::Entry => compact_entry_projection(projection, id, &resource.uri),
    }
}

#[derive(Clone, Copy)]
enum SelectedResourceKind {
    Form,
    Entry,
}

fn parse_selected_resource_uri(uri: &str) -> Result<(SelectedResourceKind, &str), String> {
    let (kind, id) = if let Some(id) = uri.strip_prefix("ugoite://form/") {
        (SelectedResourceKind::Form, id)
    } else if let Some(id) = uri.strip_prefix("ugoite://entry/") {
        (SelectedResourceKind::Entry, id)
    } else {
        return Err("selected resource URI must be a canonical Form or Entry URI".into());
    };
    if id.is_empty() || id.contains('/') || id.contains(['?', '#', '%']) {
        return Err("selected resource URI must contain one opaque ID segment".into());
    }
    let identifier_kind = match kind {
        SelectedResourceKind::Form => ugoite_domain::id::IdentifierKind::Form,
        SelectedResourceKind::Entry => ugoite_domain::id::IdentifierKind::Entry,
    };
    ugoite_domain::id::validate_identifier(identifier_kind, id)
        .map_err(|_| "selected resource URI contains an invalid opaque ID".to_string())?;
    Ok((kind, id))
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("selected resource projection requires a non-empty {key}"))
}

fn compact_form_projection(
    projection: &Map<String, Value>,
    id: &str,
) -> Result<(String, bool), String> {
    let name = required_string(projection, "name")?;
    let fields = projection
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| "selected Form projection requires a fields object".to_string())?;
    let description = projection.get("description");
    if description.is_some_and(|value| !value.is_null() && !value.is_string()) {
        return Err("selected Form projection has an invalid description".into());
    }
    let description = description.and_then(Value::as_str);
    let mut compact_fields = Map::new();
    let mut omitted = 0usize;
    for (field_name, field) in fields {
        let field = field
            .as_object()
            .ok_or_else(|| "selected Form fields must be projection objects".to_string())?;
        let field_type = required_string(field, "type")?;
        let required = field
            .get("required")
            .and_then(Value::as_bool)
            .ok_or_else(|| "selected Form fields require a boolean required flag".to_string())?;
        compact_fields.insert(
            field_name.clone(),
            serde_json::json!({"type": field_type, "required": required}),
        );
        let candidate = form_context_value(
            id,
            name,
            description,
            compact_fields.clone(),
            omitted,
            false,
            false,
        );
        if serialized_chars(&candidate)? > MAX_SELECTED_RESOURCE_CONTENT_CHARS {
            compact_fields.remove(field_name);
            omitted = omitted.saturating_add(1);
        }
    }
    let include_description = description.is_some();
    let mut truncated = omitted > 0;
    let mut compact = form_context_value(
        id,
        name,
        description,
        compact_fields.clone(),
        omitted,
        include_description,
        truncated,
    );
    while serialized_chars(&compact)? > MAX_SELECTED_RESOURCE_CONTENT_CHARS
        && !compact_fields.is_empty()
    {
        let last = compact_fields
            .keys()
            .next_back()
            .cloned()
            .expect("not empty");
        compact_fields.remove(&last);
        omitted = omitted.saturating_add(1);
        truncated = true;
        compact = form_context_value(
            id,
            name,
            description,
            compact_fields.clone(),
            omitted,
            include_description,
            true,
        );
    }
    if serialized_chars(&compact)? > MAX_SELECTED_RESOURCE_CONTENT_CHARS && include_description {
        truncated = true;
        compact = form_context_value(id, name, description, compact_fields, omitted, false, true);
    }
    serialize_compact_projection(compact, truncated)
}

fn form_context_value(
    id: &str,
    name: &str,
    description: Option<&str>,
    fields: Map<String, Value>,
    omitted_fields: usize,
    include_description: bool,
    include_marker: bool,
) -> Value {
    let mut value = serde_json::json!({
        "id": id,
        "name": name,
        "fields": fields,
        "_untrusted_content": true,
    });
    if include_description {
        value["description"] = Value::String(description.unwrap_or_default().to_string());
    }
    if include_marker {
        value["_ugoite_context"] = serde_json::json!({
            "truncated": true,
            "omitted_fields": omitted_fields,
            "omitted_description": description.is_some() && !include_description,
        });
    }
    value
}

fn compact_entry_projection(
    projection: &Map<String, Value>,
    id: &str,
    requested_uri: &str,
) -> Result<(String, bool), String> {
    let uri = required_string(projection, "uri")?;
    if uri != requested_uri {
        return Err("selected Entry projection URI does not match its request".into());
    }
    let form = projection.get("form").cloned().unwrap_or(Value::Null);
    if !form.is_null() && !form.is_string() {
        return Err("selected Entry projection has an invalid Form reference".into());
    }
    let content = projection
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "selected Entry projection requires content text".to_string())?;
    let mut low = 0usize;
    let mut high = content.chars().count();
    let chars = content.chars().collect::<Vec<_>>();
    let mut best = None;
    while low <= high {
        let middle = low + (high - low) / 2;
        let candidate_content = chars.iter().take(middle).collect::<String>();
        let truncated = middle < chars.len();
        let candidate = entry_context_value(id, &form, &candidate_content, truncated);
        if serialized_chars(&candidate)? <= MAX_SELECTED_RESOURCE_CONTENT_CHARS {
            best = Some(candidate);
            low = middle.saturating_add(1);
        } else if middle == 0 {
            break;
        } else {
            high = middle - 1;
        }
    }
    let compact = best.ok_or_else(|| {
        "selected Entry identity cannot fit the compact projection limit".to_string()
    })?;
    let truncated = compact
        .get("_ugoite_context")
        .and_then(Value::as_object)
        .is_some_and(|marker| marker.get("truncated") == Some(&Value::Bool(true)));
    serialize_compact_projection(compact, truncated)
}

fn entry_context_value(id: &str, form: &Value, content: &str, truncated: bool) -> Value {
    let mut value = serde_json::json!({
        "id": id,
        "form": form,
        "content": content,
        "_untrusted_content": true,
    });
    if truncated {
        value["_ugoite_context"] = serde_json::json!({"truncated": true});
    }
    value
}

fn serialized_chars(value: &Value) -> Result<usize, String> {
    serde_json::to_string(value)
        .map(|serialized| serialized.chars().count())
        .map_err(|error| error.to_string())
}

fn serialize_compact_projection(value: Value, truncated: bool) -> Result<(String, bool), String> {
    let serialized = serde_json::to_string(&value).map_err(|error| error.to_string())?;
    if serialized.chars().count() > MAX_SELECTED_RESOURCE_CONTENT_CHARS {
        return Err("selected resource identity cannot fit the compact projection limit".into());
    }
    Ok((serialized, truncated))
}

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

impl ContextLimits {
    fn bounded(self) -> Self {
        let defaults = Self::default();
        Self {
            max_observations: self.max_observations.min(defaults.max_observations),
            max_resources: self.max_resources.min(defaults.max_resources),
            max_summary_chars: self.max_summary_chars.min(defaults.max_summary_chars),
            max_fact_chars: self.max_fact_chars.min(defaults.max_fact_chars),
            max_facts: self.max_facts.min(defaults.max_facts),
            max_resource_references: self
                .max_resource_references
                .min(defaults.max_resource_references),
            max_capabilities: self.max_capabilities.min(defaults.max_capabilities),
            max_safety_hints: self.max_safety_hints.min(defaults.max_safety_hints),
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
        Self {
            limits: limits.bounded(),
        }
    }

    pub fn limits(&self) -> &ContextLimits {
        &self.limits
    }

    pub fn build(&self, request: ContextBuildRequest) -> crate::ContextCapsule {
        self.build_with_byte_budget(request, MAX_CONTEXT_JSON_BYTES)
    }

    /// Build a deterministic Context Capsule within the requested serialized
    /// byte budget. The budget is also capped by the protocol-wide context
    /// limit so callers cannot expand the portable boundary.
    pub fn build_with_byte_budget(
        &self,
        request: ContextBuildRequest,
        max_bytes: usize,
    ) -> crate::ContextCapsule {
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
        let limits = limits.unwrap_or_else(|| self.limits.clone()).bounded();
        let max_bytes = max_bytes.min(MAX_CONTEXT_JSON_BYTES);

        let mut context = ContextCapsule {
            work_goal: truncate(&work_goal, limits.max_summary_chars),
            job_goal: truncate(&job_goal, limits.max_summary_chars),
            current_strategy_summary: current_strategy_summary
                .map(|summary| truncate(&summary, limits.max_summary_chars)),
            relevant_observations: Vec::new(),
            available_capabilities: Vec::new(),
            selected_resource_contents: Vec::new(),
            safety_hints: Vec::new(),
            expected_response_schema: None,
        };

        let observations = observations
            .into_iter()
            .rev()
            .take(limits.max_observations)
            .map(|mut observation| {
                observation.id = truncate(&observation.id, MAX_CAPABILITY_NAME_CHARS);
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
        let available_capabilities = available_capabilities
            .into_iter()
            .filter_map(|mut capability| {
                capability.name = truncate(&capability.name, MAX_CAPABILITY_NAME_CHARS);
                capability.description =
                    truncate(&capability.description, limits.max_summary_chars);
                if let Some(schema) = capability.input_schema.take() {
                    let schema_is_bounded = serde_json::to_vec(&schema)
                        .map(|serialized| serialized.len() <= MAX_CONTEXT_SCHEMA_BYTES)
                        .unwrap_or(false);
                    if !schema_is_bounded {
                        return None;
                    }
                    capability.input_schema = Some(schema);
                }
                Some(capability)
            })
            .collect::<Vec<_>>();

        let selected_resource_contents = selected_resource_contents
            .into_iter()
            .take(limits.max_resources)
            .map(|mut resource| {
                resource.uri = truncate(&resource.uri, limits.max_summary_chars);
                resource.content = truncate(&resource.content, limits.max_summary_chars);
                resource
            })
            .collect::<Vec<_>>();

        let expected_response_schema = expected_response_schema.filter(|schema| {
            serde_json::to_vec(schema)
                .map(|serialized| serialized.len() <= MAX_CONTEXT_SCHEMA_BYTES)
                .unwrap_or(false)
        });

        // Capability metadata is kept as one atomic payload. In particular,
        // do not retain a capability after dropping only its schema: hosts
        // correctly omit capabilities without a usable input schema.
        for capability in available_capabilities {
            context.available_capabilities.push(capability);
            let capability_payload_fits = serde_json::to_vec(&context.available_capabilities)
                .map(|serialized| serialized.len() <= MAX_CONTEXT_CAPABILITY_JSON_BYTES)
                .unwrap_or(false);
            if !capability_payload_fits || !fits_budget(&context, max_bytes) {
                context.available_capabilities.pop();
                continue;
            }
        }

        // Observations are considered newest first and then restored to
        // chronological order. Once the next recent observation does not fit,
        // older observations are not more relevant and are omitted as well.
        let mut recent_observations = Vec::new();
        for observation in observations {
            recent_observations.push(observation);
            context.relevant_observations = recent_observations.iter().rev().cloned().collect();
            if !fits_budget(&context, max_bytes) {
                recent_observations.pop();
                context.relevant_observations = recent_observations.iter().rev().cloned().collect();
                break;
            }
        }

        for resource in selected_resource_contents {
            context.selected_resource_contents.push(resource);
            if !fits_budget(&context, max_bytes) {
                context.selected_resource_contents.pop();
                break;
            }
        }

        for hint in safety_hints
            .into_iter()
            .take(limits.max_safety_hints)
            .map(|hint| truncate(&hint, limits.max_summary_chars))
        {
            context.safety_hints.push(hint);
            if !fits_budget(&context, max_bytes) {
                context.safety_hints.pop();
                break;
            }
        }

        if expected_response_schema.is_some() {
            context.expected_response_schema = expected_response_schema;
            if !fits_budget(&context, max_bytes) {
                context.expected_response_schema = None;
            }
        }

        context
    }
}

fn fits_budget(context: &ContextCapsule, max_bytes: usize) -> bool {
    serde_json::to_vec(context)
        .map(|serialized| serialized.len() <= max_bytes)
        .unwrap_or(false)
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
    fn selected_projection_compaction_preserves_valid_unicode_json_and_marker() {
        let id = "00000000-0000-0000-0000-0000000000a1";
        let form_fields = (0..80)
            .map(|index| {
                (
                    format!("field_{index}_長い名前"),
                    serde_json::json!({"type":"string", "required": index % 2 == 0}),
                )
            })
            .collect::<Map<_, _>>();
        let form = ResourceContent {
            uri: format!("ugoite://form/{id}"),
            content: serde_json::json!({
                "id": id,
                "name": "経費フォーム",
                "description": "説明🙂".repeat(400),
                "fields": form_fields,
                "_untrusted_content": true
            })
            .to_string(),
        };
        let entry_id = "00000000-0000-0000-0000-0000000000b2";
        let entry = ResourceContent {
            uri: format!("ugoite://entry/{entry_id}"),
            content: serde_json::json!({
                "id": entry_id,
                "uri": format!("ugoite://entry/{entry_id}"),
                "form": "経費フォーム",
                "content": "明細🙂".repeat(2_000),
                "_untrusted_content": true
            })
            .to_string(),
        };

        let (normalized, admissions) =
            normalize_selected_resource_contents(vec![form, entry]).unwrap();
        assert_eq!(normalized.len(), 2);
        assert_eq!(admissions.len(), 2);
        for resource in &normalized {
            assert!(resource.content.chars().count() <= MAX_SELECTED_RESOURCE_CONTENT_CHARS);
            let value: Value = serde_json::from_str(&resource.content).unwrap();
            assert_eq!(value["_untrusted_content"], true);
            assert_eq!(value["_ugoite_context"]["truncated"], true);
        }
        assert_eq!(admissions[0].status, ResourceAdmissionStatus::Truncated);
        assert_eq!(admissions[1].status, ResourceAdmissionStatus::Truncated);
    }

    #[test]
    fn selected_projection_rejects_noncanonical_uri_and_untrusted_mismatch() {
        let id = "00000000-0000-0000-0000-0000000000b2";
        let valid = serde_json::json!({
            "id": id,
            "uri": format!("ugoite://entry/{id}"),
            "form": "Note",
            "content": "body",
            "_untrusted_content": true
        })
        .to_string();
        for uri in [
            format!("ugoite://entry/{id}/schema"),
            format!("ugoite://entry/{id}?history=true"),
        ] {
            assert!(normalize_selected_resource_contents(vec![ResourceContent {
                uri,
                content: valid.clone(),
            }])
            .is_err());
        }
        let mut mismarked: Value = serde_json::from_str(&valid).unwrap();
        mismarked["_untrusted_content"] = Value::Bool(false);
        assert!(normalize_selected_resource_contents(vec![ResourceContent {
            uri: format!("ugoite://entry/{id}"),
            content: mismarked.to_string(),
        }])
        .is_err());

        for content in [
            "not json".to_string(),
            serde_json::json!({
                "id": "00000000-0000-0000-0000-0000000000c4",
                "name": "Note",
                "fields": {},
                "_untrusted_content": true
            })
            .to_string(),
            serde_json::json!({
                "id": "00000000-0000-0000-0000-0000000000c3",
                "name": "Note",
                "fields": {"amount": {"type": "number"}},
                "_untrusted_content": true
            })
            .to_string(),
        ] {
            assert!(normalize_selected_resource_contents(vec![ResourceContent {
                uri: "ugoite://form/00000000-0000-0000-0000-0000000000c3".into(),
                content,
            }])
            .is_err());
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
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {"q": {"type": "string"}},
                    "required": ["q"]
                })),
                effect: Some(crate::CapabilityEffect::Read),
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
            context.available_capabilities[0].input_schema,
            Some(serde_json::json!({
                "type": "object",
                "properties": {"q": {"type": "string"}},
                "required": ["q"]
            }))
        );
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

    #[test]
    fn caller_limits_and_context_fields_remain_hard_bounded() {
        let oversized = "x".repeat(10_000);
        let oversized_schema = "x".repeat(MAX_CONTEXT_SCHEMA_BYTES);
        let request = ContextBuildRequest {
            work_goal: oversized.clone(),
            job_goal: oversized.clone(),
            current_strategy_summary: Some(oversized.clone()),
            observations: (0..20).map(observation).collect(),
            available_capabilities: (0..40)
                .map(|id| Capability {
                    name: format!("capability-{id}-{oversized}"),
                    description: oversized.clone(),
                    input_schema: Some(serde_json::json!({
                        "schema": oversized_schema.clone()
                    })),
                    effect: None,
                })
                .collect(),
            selected_resource_contents: (0..10)
                .map(|id| ResourceContent {
                    uri: format!("ugoite://entry/{id}-{oversized}"),
                    content: oversized.clone(),
                })
                .collect(),
            safety_hints: (0..20).map(|_| oversized.clone()).collect(),
            expected_response_schema: Some(serde_json::json!({
                "schema": "x".repeat(MAX_CONTEXT_SCHEMA_BYTES)
            })),
            limits: Some(ContextLimits {
                max_observations: usize::MAX,
                max_resources: usize::MAX,
                max_summary_chars: usize::MAX,
                max_fact_chars: usize::MAX,
                max_facts: usize::MAX,
                max_resource_references: usize::MAX,
                max_capabilities: usize::MAX,
                max_safety_hints: usize::MAX,
            }),
        };

        let context = ContextBuilder::default().build(request);
        let defaults = ContextLimits::default();
        assert_eq!(
            context.relevant_observations.len(),
            defaults.max_observations
        );
        assert_eq!(
            context.selected_resource_contents.len(),
            defaults.max_resources
        );
        assert!(context.available_capabilities.is_empty());
        assert_eq!(context.safety_hints.len(), defaults.max_safety_hints);
        assert!(context.expected_response_schema.is_none());
        assert!(context.work_goal.chars().count() <= defaults.max_summary_chars);
        assert!(context
            .available_capabilities
            .iter()
            .all(|capability| capability.name.chars().count() <= MAX_CAPABILITY_NAME_CHARS));
    }

    #[test]
    fn complete_capability_payload_and_context_are_aggregately_bounded() {
        let schema = serde_json::json!({
            "type": "object",
            "description": "x".repeat(7_000),
        });
        let request = ContextBuildRequest {
            work_goal: "work goal".into(),
            job_goal: "job goal".into(),
            current_strategy_summary: Some("strategy".into()),
            observations: (0..20).map(observation).collect(),
            available_capabilities: (0..32)
                .map(|id| Capability {
                    name: format!("capability-{id}"),
                    description: "capability description".into(),
                    input_schema: Some(schema.clone()),
                    effect: None,
                })
                .collect(),
            selected_resource_contents: (0..10)
                .map(|id| ResourceContent {
                    uri: format!("ugoite://entry/{id}"),
                    content: "resource content".into(),
                })
                .collect(),
            safety_hints: (0..20).map(|id| format!("hint-{id}")).collect(),
            expected_response_schema: Some(schema),
            limits: None,
        };

        let context = ContextBuilder::default().build(request.clone());
        let serialized_size = serde_json::to_vec(&context).unwrap().len();
        assert!(serialized_size <= MAX_CONTEXT_JSON_BYTES);
        assert!(context.available_capabilities.len() < 32);
        assert!(context
            .available_capabilities
            .iter()
            .all(|capability| capability.input_schema.is_some()));
        assert!(
            serde_json::to_vec(&context.available_capabilities)
                .unwrap()
                .len()
                <= MAX_CONTEXT_CAPABILITY_JSON_BYTES
        );
        assert_eq!(context, ContextBuilder::default().build(request));
    }
}
